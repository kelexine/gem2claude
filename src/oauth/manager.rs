// OAuth manager with credential loading and refresh
// Author: kelexine (https://github.com/kelexine)

use super::OAuthCredentials;
use crate::config::OAuthConfig;
use crate::error::{ProxyError, Result};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, warn};

#[derive(Clone)]
pub struct OAuthManager {
    credentials: Arc<RwLock<OAuthCredentials>>,
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

    /// Get a valid access token (refreshing if needed)
    pub async fn get_token(&self) -> Result<String> {
        let creds = self.credentials.read().await;
        
        // Check if token is expired or will expire soon
        if creds.is_expired(self.config.refresh_buffer_seconds) {
            drop(creds); // Release read lock
            
            if !self.config.auto_refresh {
                return Err(ProxyError::TokenExpired);
            }

            warn!("Token expired or expiring soon, refresh needed");
            // TODO: Implement token refresh in Phase 3
            return Err(ProxyError::TokenExpired);
        }

        Ok(creds.access_token.clone())
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
