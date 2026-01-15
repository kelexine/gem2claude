// OAuth credential management module
// Author: kelexine (https://github.com/kelexine)

pub mod login;
mod manager;

pub use manager::OAuthManager;

use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

/// OAuth credentials matching Gemini CLI format
#[derive(Clone, Deserialize, Serialize, Zeroize)]
#[zeroize(drop)]
pub struct OAuthCredentials {
    pub access_token: String,
    pub refresh_token: String,
    pub token_type: String,
    pub expiry_date: i64,
    #[serde(default)]
    pub scope: String,
    #[serde(default)]
    pub id_token: String,
}

// Custom Debug impl that never logs tokens
impl std::fmt::Debug for OAuthCredentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OAuthCredentials")
            .field("access_token", &"[REDACTED]")
            .field("refresh_token", &"[REDACTED]")
            .field("token_type", &self.token_type)
            .field("expiry_date", &self.expiry_date)
            .field("scope", &self.scope)
            .field("id_token", &"[REDACTED]")
            .finish()
    }
}

impl OAuthCredentials {
    /// Check if token is expired or will expire within buffer seconds
    pub fn is_expired(&self, buffer_seconds: i64) -> bool {
        let now = chrono::Utc::now().timestamp_millis();
        self.expiry_date - now < buffer_seconds * 1000
    }

    /// Get remaining time until expiry in seconds
    pub fn expires_in_seconds(&self) -> i64 {
        let now = chrono::Utc::now().timestamp_millis();
        (self.expiry_date - now) / 1000
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_debug_impl_masks_tokens() {
        let creds = OAuthCredentials {
            access_token: "ya29.secret".to_string(),
            refresh_token: "1//01refresh".to_string(),
            token_type: "Bearer".to_string(),
            expiry_date: 1768126811935,
            scope: "cloud-platform".to_string(),
            id_token: "eyJhbGciOiJSUzI1NiI...".to_string(),
        };

        let debug_str = format!("{:?}", creds);
        assert!(debug_str.contains("[REDACTED]"));
        assert!(!debug_str.contains("ya29"));
        assert!(!debug_str.contains("1//01"));
        assert!(!debug_str.contains("eyJhbGci"));
    }

    #[test]
    fn test_expiry_detection() {
        let future_expiry = chrono::Utc::now().timestamp_millis() + 3600000; // 1 hour from now
        let creds = OAuthCredentials {
            access_token: "test".to_string(),
            refresh_token: "test".to_string(),
            token_type: "Bearer".to_string(),
            expiry_date: future_expiry,
            scope: String::new(),
            id_token: String::new(),
        };

        assert!(!creds.is_expired(0));
        assert!(creds.is_expired(3700)); // More than 1 hour buffer
        assert!(creds.expires_in_seconds() > 3500);
    }
}
