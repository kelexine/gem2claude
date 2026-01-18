//! Google OAuth2 token management.
//!
//! This module provides the `OAuthManager`, which is responsible for the entire
//! lifecycle of Google Cloud access tokens. It ensures tokens are loaded securely
//! from disk, validates file permissions to prevent accidental leakage, and
//! implements a thread-safe "double-checked locking" refresh mechanism to
//! prevent multiple concurrent requests from triggering redundant token refreshes.

// Author: kelexine (https://github.com/kelexine)

use super::OAuthCredentials;
use crate::config::OAuthConfig;
use crate::error::{ProxyError, Result};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use tracing::{debug, error, info, warn};

/// Public Client ID for the Gemini CLI.
/// Used for OAuth2 device and installed-app flows.
pub const OAUTH_CLIENT_ID: &str =
    "681255809395-oo8ft2oprdrnp9e3aqf6av3hmdib135j.apps.googleusercontent.com";

/// Public Client Secret for the Gemini CLI.
pub const OAUTH_CLIENT_SECRET: &str = "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";

/// Manages Google OAuth2 credentials and provides valid access tokens.
///
/// The `OAuthManager` uses an `Arc<RwLock>` to allow high-concurrency access to the
/// current token, with a `Mutex` to serialize refresh attempts.
#[derive(Clone)]
pub struct OAuthManager {
    /// In-memory cache of the current OAuth2 credentials.
    credentials: Arc<RwLock<OAuthCredentials>>,
    /// Lock used to prevent "thundering herd" refresh attempts when a token expires.
    refresh_lock: Arc<Mutex<()>>,
    /// Configuration for refresh thresholds and file paths.
    config: OAuthConfig,
}

impl OAuthManager {
    /// Initializes a new `OAuthManager` by loading credentials from the configured path.
    ///
    /// # Errors
    ///
    /// Returns `ProxyError::InvalidCredentials` if the file is missing, malformed,
    /// or has insecure permissions.
    pub async fn new(config: &OAuthConfig) -> Result<Self> {
        let credentials = Self::load_credentials(&config.credentials_path)?;

        debug!("Loaded OAuth credentials from {}", config.credentials_path);

        Ok(Self {
            credentials: Arc::new(RwLock::new(credentials)),
            refresh_lock: Arc::new(Mutex::new(())),
            config: config.clone(),
        })
    }

    /// Reads credentials from the filesystem and performs basic validation.
    fn load_credentials(path: &str) -> Result<OAuthCredentials> {
        let path = Path::new(path);

        if !path.exists() {
            return Err(ProxyError::InvalidCredentials(format!(
                "Credentials file not found: {}",
                path.display()
            )));
        }

        // Validate that credentials file is not group- or world-readable.
        Self::validate_permissions(path)?;

        let contents = fs::read_to_string(path).map_err(|e| {
            ProxyError::InvalidCredentials(format!("Failed to read credentials: {}", e))
        })?;

        serde_json::from_str(&contents).map_err(|e| {
            ProxyError::InvalidCredentials(format!("Invalid credentials JSON format: {}", e))
        })
    }

    /// Ensures that the credentials file has secure permissions (0600 or 0400).
    ///
    /// This is a critical security check to prevent sensitive tokens from being
    /// accessible to other users on the system.
    fn validate_permissions(path: &Path) -> Result<()> {
        #[cfg(unix)]
        {
            let metadata = fs::metadata(path)?;
            let permissions = metadata.permissions();
            let mode = permissions.mode() & 0o777;

            // Allow only owner-accessible files.
            if mode != 0o600 && mode != 0o400 {
                warn!(
                    "Insecure permissions on {}: {:o} (expected 0600)",
                    path.display(),
                    mode
                );
                return Err(ProxyError::InvalidCredentials(format!(
                    "Insecure file permissions: {:o}. Security policy requires 0600 (rw-------).",
                    mode
                )));
            }
        }

        Ok(())
    }

    /// Acquires a valid access token, performing a refresh if necessary.
    ///
    /// This method implements a high-performance double-checked locking pattern:
    /// 1. Optimized check with a shared `RwLock` read-lock.
    /// 2. If expired, acquires a exclusive `Mutex` to synchronize refresh logic.
    /// 3. Re-checks expiry inside the mutex to see if another thread already refreshed.
    /// 4. Executes the refresh and updates both in-memory and on-disk state.
    pub async fn get_token(&self) -> Result<String> {
        // Fast path: Token is still valid.
        {
            let creds = self.credentials.read().await;
            if !creds.is_expired(self.config.refresh_buffer_seconds) {
                let seconds_remaining =
                    (creds.expiry_date / 1000 - chrono::Utc::now().timestamp()).max(0);
                crate::metrics::update_oauth_expiry(seconds_remaining);
                return Ok(creds.access_token.clone());
            }
        }

        if !self.config.auto_refresh {
            return Err(ProxyError::TokenExpired);
        }

        // Potentially slow path: Synchronize access to Google's OAuth2 endpoint.
        let _guard = self.refresh_lock.lock().await;

        // Re-verify after gaining the mutex.
        {
            let creds = self.credentials.read().await;
            if !creds.is_expired(self.config.refresh_buffer_seconds) {
                debug!("Token already refreshed by another concurrent request.");
                return Ok(creds.access_token.clone());
            }
        }

        warn!("OAuth access token expired; initiating refresh.");
        match self.refresh_token().await {
            Ok(new_creds) => {
                // Update internal thread-safe state.
                {
                    let mut creds = self.credentials.write().await;
                    *creds = new_creds.clone();
                }

                // Sync new state to disk so it survives restarts.
                if let Err(e) = self.save_credentials(&new_creds) {
                    error!("Persistence error while saving token: {}", e);
                }

                info!("Successfully updated and persisted OAuth transition.");
                crate::metrics::record_oauth_refresh(true);
                Ok(new_creds.access_token.clone())
            }
            Err(e) => {
                crate::metrics::record_oauth_refresh(false);
                Err(e)
            }
        }
    }

    /// Negotiates a new access token with Google's OAuth2 service.
    async fn refresh_token(&self) -> Result<OAuthCredentials> {
        let creds = self.credentials.read().await;

        let params = [
            ("client_id", OAUTH_CLIENT_ID),
            ("client_secret", OAUTH_CLIENT_SECRET),
            ("refresh_token", &creds.refresh_token),
            ("grant_type", "refresh_token"),
        ];

        let client = reqwest::Client::new();
        let url = "https://oauth2.googleapis.com/token";

        let request_logic = || async {
            let response = client
                .post(url)
                .form(&params)
                .send()
                .await
                .map_err(|e| (500, format!("Google OAuth2 network error: {}", e)))?;

            let status = response.status();
            if !status.is_success() {
                let error_text = response
                    .text()
                    .await
                    .unwrap_or_else(|_| "Unknown".to_string());
                return Err((status.as_u16(), error_text));
            }

            Ok(response)
        };

        // Attempt refresh with automated retries.
        let response = crate::utils::retry::with_retry("OAuth Refresh", request_logic)
            .await
            .map_err(|(status, body)| match status {
                429 => ProxyError::TooManyRequests(body),
                529 => ProxyError::Overloaded(format!("Google overloaded: {}", body)),
                503 | 504 => ProxyError::ServiceUnavailable(body),
                _ => ProxyError::OAuthRefresh(format!("HTTP {}: {}", status, body)),
            })?;

        let token_data: serde_json::Value = response
            .json()
            .await
            .map_err(|e| ProxyError::OAuthRefresh(format!("Malformed JSON response: {}", e)))?;

        let new_access_token = token_data
            .get("access_token")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ProxyError::OAuthRefresh("Missing access_token in Google response".to_string())
            })?
            .to_string();

        let expires_in = token_data
            .get("expires_in")
            .and_then(|v| v.as_i64())
            .unwrap_or(3600);

        let new_expiry = chrono::Utc::now().timestamp_millis() + (expires_in * 1000);

        let new_creds = OAuthCredentials {
            access_token: new_access_token,
            refresh_token: creds.refresh_token.clone(),
            token_type: "Bearer".to_string(),
            expiry_date: new_expiry,
            scope: creds.scope.clone(),
            id_token: String::new(),
        };

        debug!("Refreshed token expires in {} seconds", expires_in);
        Ok(new_creds)
    }

    /// Serializes and writes credentials back to the disk.
    fn save_credentials(&self, creds: &OAuthCredentials) -> Result<()> {
        use std::io::Write;

        let path = Path::new(&self.config.credentials_path);
        let json = serde_json::to_string_pretty(creds)
            .map_err(|e| ProxyError::Internal(format!("Serialization failure: {}", e)))?;

        let mut file = fs::File::create(path).map_err(|e| {
            ProxyError::Internal(format!("Failed to truncate/create credentials file: {}", e))
        })?;

        file.write_all(json.as_bytes())
            .map_err(|e| ProxyError::Internal(format!("Disk write failure: {}", e)))?;

        #[cfg(unix)]
        {
            fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        }

        Ok(())
    }

    /// Provides safe access to public token metadata (expiry time).
    pub async fn token_info(&self) -> (i64, bool) {
        let creds = self.credentials.read().await;
        let expires_in = creds.expires_in_seconds();
        let is_expired = creds.is_expired(self.config.refresh_buffer_seconds);
        (expires_in, is_expired)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn create_test_credentials() -> String {
        serde_json::json!({
            "access_token": "ya29.test",
            "refresh_token": "1//01test",
            "token_type": "Bearer",
            "expiry_date": chrono::Utc::now().timestamp_millis() + 3600000,
            "scope": "https://www.googleapis.com/auth/cloud-platform",
            "id_token": "test_id_token"
        })
        .to_string()
    }

    #[test]
    fn test_load_valid_credentials() {
        let mut temp = NamedTempFile::new().unwrap();
        write!(temp, "{}", create_test_credentials()).unwrap();

        #[cfg(unix)]
        {
            use std::fs::Permissions;
            fs::set_permissions(temp.path(), Permissions::from_mode(0o600)).unwrap();
        }

        let creds = OAuthManager::load_credentials(temp.path().to_str().unwrap()).unwrap();

        assert_eq!(creds.token_type, "Bearer");
    }

    #[test]
    fn test_missing_credentials_file() {
        let result = OAuthManager::load_credentials("/nonexistent/path");
        assert!(result.is_err());
    }
}
