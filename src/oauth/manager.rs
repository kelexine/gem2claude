// OAuth manager with credential loading and automatic refresh
// Author: kelexine (https://github.com/kelexine)

use super::OAuthCredentials;
use crate::config::OAuthConfig;
use crate::error::{ProxyError, Result};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use tracing::{debug, info, warn};

/// Google OAuth2 credentials for Gemini CLI (public, for installed apps)
/// Source: gemini-cli-0.23.0/packages/core/src/code_assist/oauth2.ts
const OAUTH_CLIENT_ID: &str = "681255809395-oo8ft2oprdrnp9e3aqf6av3hmdib135j.apps.googleusercontent.com";
const OAUTH_CLIENT_SECRET: &str = "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";

#[derive(Clone)]
pub struct OAuthManager {
    credentials: Arc<RwLock<OAuthCredentials>>,
    refresh_lock: Arc<Mutex<()>>,  // Prevents thundering herd on refresh
    config: OAuthConfig,
}

impl OAuthManager {
    /// Create a new OAuth manager and load credentials
    pub async fn new(config: &OAuthConfig) -> Result<Self> {
        let credentials = Self::load_credentials(&config.credentials_path)?;
        
        debug!("Loaded OAuth credentials");
        debug!(
            "Token expires in {} seconds",
            credentials.expires_in_seconds()
        );

        Ok(Self {
            credentials: Arc::new(RwLock::new(credentials)),
            refresh_lock: Arc::new(Mutex::new(())),
            config: config.clone(),
        })
    }

    /// Load OAuth credentials from file
    fn load_credentials(path: &str) -> Result<OAuthCredentials> {
        let path = Path::new(path);
        
        if !path.exists() {
            return Err(ProxyError::InvalidCredentials(format!(
                "Credentials file not found: {}",
                path.display()
            )));
        }

        // Validate file permissions (must be 0600 or 0400)
        Self::validate_permissions(path)?;

        let contents = fs::read_to_string(path).map_err(|e| {
            ProxyError::InvalidCredentials(format!("Failed to read credentials: {}", e))
        })?;

        serde_json::from_str(&contents).map_err(|e| {
            ProxyError::InvalidCredentials(format!("Invalid credentials format: {}", e))
        })
    }

    /// Validate file permissions are secure
    fn validate_permissions(path: &Path) -> Result<()> {
        #[cfg(unix)]
        {
            let metadata = fs::metadata(path)?;
            let permissions = metadata.permissions();
            let mode = permissions.mode() & 0o777;

            // Allow only 0600 (rw-------) or 0400 (r--------)
            if mode != 0o600 && mode != 0o400 {
                warn!(
                    "Insecure credentials file permissions: {:o} (should be 0600 or 0400)",
                    mode
                );
                return Err(ProxyError::InvalidCredentials(format!(
                    "Insecure file permissions: {:o}. Run: chmod 600 {}",
                    mode,
                    path.display()
                )));
            }
        }

        Ok(())
    }

    /// Get a valid access token (refreshing if needed using double-checked locking)
    pub async fn get_token(&self) -> Result<String> {
        // FIRST CHECK: Fast path with read lock
        {
            let creds = self.credentials.read().await;
            if !creds.is_expired(self.config.refresh_buffer_seconds) {
                return Ok(creds.access_token.clone());
            }
        } // Release read lock

        // Token expired - check if auto-refresh is enabled
        if !self.config.auto_refresh {
            warn!("Token expired and auto_refresh is disabled");
            return Err(ProxyError::TokenExpired);
        }

        // Acquire refresh lock (only ONE request enters here)
        let _guard = self.refresh_lock.lock().await;
        debug!("Acquired refresh lock");

        // SECOND CHECK: Another thread might have refreshed while we waited
        {
            let creds = self.credentials.read().await;
            if !creds.is_expired(self.config.refresh_buffer_seconds) {
                debug!("Token was refreshed by another request");
                return Ok(creds.access_token.clone());
            }
        }

        // Still expired - we must refresh
        warn!("Refreshing expired OAuth token");
        let new_creds = self.refresh_token().await?;

        // Write new credentials with write lock
        {
            let mut creds = self.credentials.write().await;
            *creds = new_creds.clone();
        }

        // Persist to disk for future runs
        self.save_credentials(&new_creds)?;

        info!("Successfully refreshed and saved OAuth token");
        Ok(new_creds.access_token.clone())
    }

    /// Refresh the OAuth token using refresh_token
    async fn refresh_token(&self) -> Result<OAuthCredentials> {
        let creds = self.credentials.read().await;

        // Build refresh request body
        let params = [
            ("client_id", OAUTH_CLIENT_ID),
            ("client_secret", OAUTH_CLIENT_SECRET),
            ("refresh_token", &creds.refresh_token),
            ("grant_type", "refresh_token"),
        ];

        // Call Google OAuth2 token endpoint with retry
        let client = reqwest::Client::new();
        let url = "https://oauth2.googleapis.com/token";

        // Build request logic
        let request_logic = || async {
            let response = client
                .post(url)
                .form(&params)
                .send()
                .await
                .map_err(|e| (500, format!("HTTP request failed: {}", e)))?;

            let status = response.status();
            if !status.is_success() {
                let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
                return Err((status.as_u16(), error_text));
            }

            Ok(response)
        };

        // Execute with retry
        let response = crate::utils::retry::with_retry("OAuth Refresh", request_logic)
            .await
            .map_err(|(status, body)| match status {
                429 => ProxyError::TooManyRequests(body),
                529 => ProxyError::Overloaded(format!("OAuth service overloaded: {}", body)),
                503 | 504 => ProxyError::ServiceUnavailable(body),
                _ => ProxyError::OAuthRefresh(format!("HTTP {}: {}", status, body)),
            })?;

        // Parse response
        let token_data: serde_json::Value = response
            .json()
            .await
            .map_err(|e| ProxyError::OAuthRefresh(format!("Failed to parse response: {}", e)))?;

        // Extract new access token
        let new_access_token = token_data
            .get("access_token")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ProxyError::OAuthRefresh("No access_token in response".to_string()))?
            .to_string();

        // Calculate expiry time
        let expires_in = token_data
            .get("expires_in")
            .and_then(|v| v.as_i64())
            .unwrap_or(3600); // Default to 1 hour

        let new_expiry = chrono::Utc::now().timestamp_millis() + (expires_in * 1000);

        // Build new credentials (reuse existing refresh_token)
        let new_creds = OAuthCredentials {
            access_token: new_access_token,
            refresh_token: creds.refresh_token.clone(),
            token_type: "Bearer".to_string(),
            expiry_date: new_expiry,
            scope: creds.scope.clone(),
            id_token: String::new(), // Not provided on refresh
        };

        debug!("Token refreshed, new expiry: {}", new_expiry);
        Ok(new_creds)
    }

    /// Save credentials to disk
    fn save_credentials(&self, creds: &OAuthCredentials) -> Result<()> {
        use std::io::Write;

        let path = Path::new(&self.config.credentials_path);
        let json = serde_json::to_string_pretty(creds)
            .map_err(|e| ProxyError::Internal(format!("Failed to serialize credentials: {}", e)))?;

        let mut file = fs::File::create(path)
            .map_err(|e| ProxyError::Internal(format!("Failed to create credentials file: {}", e)))?;

        file.write_all(json.as_bytes())
            .map_err(|e| ProxyError::Internal(format!("Failed to write credentials: {}", e)))?;

        // Ensure secure permissions
        #[cfg(unix)]
        {
            fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        }

        debug!("Saved refreshed credentials to disk");
        Ok(())
    }

    /// Get token expiry information
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

        let creds = OAuthManager::load_credentials(
            temp.path().to_str().unwrap()
        ).unwrap();
        
        assert_eq!(creds.token_type, "Bearer");
    }

    #[test]
    fn test_missing_credentials_file() {
        let result = OAuthManager::load_credentials("/nonexistent/path");
        assert!(result.is_err());
    }
}
