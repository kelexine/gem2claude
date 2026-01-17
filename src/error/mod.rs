// Error types for gem2claude proxy
// Author: kelexine (https://github.com/kelexine)

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::json;
use thiserror::Error;

/// Unified error type for the gem2claude proxy.
///
/// This enum captures all possible failure modes including:
/// - OAuth authentication issues
/// - Gemini API errors
/// - Translation failures
/// - Configuration problems
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

// Convert ProxyError to HTTP responses for Axum (matches Claude API error format)
impl IntoResponse for ProxyError {
    fn into_response(self) -> Response {
        let (status, error_type, message) = match self {
            // 401 - authentication_error
            ProxyError::OAuth(_)
            | ProxyError::InvalidCredentials(_)
            | ProxyError::TokenExpired
            | ProxyError::OAuthRefresh(_) => (
                StatusCode::UNAUTHORIZED,
                "authentication_error",
                self.to_string(),
            ),
            // 400 - invalid_request_error
            ProxyError::InvalidRequest(_) | ProxyError::Translation(_) => (
                StatusCode::BAD_REQUEST,
                "invalid_request_error",
                self.to_string(),
            ),
            // 429 - rate_limit_error
            ProxyError::TooManyRequests(_) => (
                StatusCode::TOO_MANY_REQUESTS,
                "rate_limit_error",
                self.to_string(),
            ),
            // 529 - overloaded_error (Gemini API overloaded)
            ProxyError::Overloaded(_) => (
                StatusCode::from_u16(529).unwrap(),
                "overloaded_error",
                self.to_string(),
            ),
            // 503 - api_error (Service unavailable)
            ProxyError::ServiceUnavailable(_) => (
                StatusCode::SERVICE_UNAVAILABLE,
                "api_error",
                self.to_string(),
            ),
            // 500 - api_error (catch-all for internal errors)
            ProxyError::Config(_) | ProxyError::ConfigParsing(_) | ProxyError::Internal(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "api_error",
                self.to_string(),
            ),
            // 502 - api_error (upstream API errors)
            ProxyError::ProjectResolution(_) | ProxyError::GeminiApi(_) => {
                (StatusCode::BAD_GATEWAY, "api_error", self.to_string())
            }
            // Default - api_error
            _ => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "api_error",
                self.to_string(),
            ),
        };

        let body = json!({
            "type": "error",
            "error": {
                "type": error_type,
                "message": message,
            }
        });

        (status, axum::Json(body)).into_response()
    }
}

/// Helper Result type expecting `ProxyError`
pub type Result<T> = std::result::Result<T, ProxyError>;
