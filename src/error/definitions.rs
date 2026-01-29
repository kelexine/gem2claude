// Error definitions for gem2claude
// Author: kelexine (https://github.com/kelexine)

use thiserror::Error;

/// Unified error type for the gem2claude proxy.
#[derive(Error, Debug)]
pub enum ProxyError {
    /// OAuth authentication or token error
    #[error("OAuth error: {0}")]
    OAuth(String),

    /// Google Cloud project resolution failed
    #[error("Project resolution failed: {0}")]
    ProjectResolution(String),

    /// Missing or invalid configuration
    #[error("Configuration error: {0}")]
    Config(String),

    /// Error returned by the upstream Gemini API
    #[error("Gemini API error: {0}")]
    GeminiApi(String),

    /// Failed to translate request or response
    #[error("Translation error: {0}")]
    Translation(String),

    /// User provided invalid credentials
    #[error("Invalid credentials: {0}")]
    InvalidCredentials(String),

    /// OAuth token has expired and could not be refreshed
    #[error("Token expired")]
    TokenExpired,

    /// Low-level I/O error
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// JSON serialization/deserialization error
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// HTTP client error
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    /// Configuration parsing error
    #[error("Config parsing error: {0}")]
    ConfigParsing(#[from] config::ConfigError),

    /// Client request validation failed
    #[error("Invalid request: {0}")]
    InvalidRequest(String),

    /// Failed to refresh OAuth token
    #[error("OAuth token refresh failed: {0}")]
    OAuthRefresh(String),

    /// Internal server error
    #[error("Internal error: {0}")]
    Internal(String),

    /// Rate limit exceeded (429)
    #[error("Rate limit exceeded: {0}")]
    TooManyRequests(String),

    /// Service unavailable (503)
    #[error("Service unavailable: {0}")]
    ServiceUnavailable(String),

    /// Upstream API overloaded (529)
    #[error("API overloaded: {0}")]
    Overloaded(String),
}

/// Helper Result type expecting `ProxyError`
pub type Result<T> = std::result::Result<T, ProxyError>;
