// Error types for gem2claude proxy
// Author: kelexine (https://github.com/kelexine)

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::json;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ProxyError {
    #[error("OAuth error: {0}")]
    OAuth(String),

    #[error("Project resolution failed: {0}")]
    ProjectResolution(String),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Gemini API error: {0}")]
    GeminiApi(String),

    #[error("Translation error: {0}")]
    Translation(String),

    #[error("Invalid credentials: {0}")]
    InvalidCredentials(String),

    #[error("Token expired")]
    TokenExpired,

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("Config parsing error: {0}")]
    ConfigParsing(#[from] config::ConfigError),

    #[error("Invalid request: {0}")]
    InvalidRequest(String),

    #[error("OAuth token refresh failed: {0}")]
    OAuthRefresh(String),

    #[error("Internal error: {0}")]
    Internal(String),

    #[error("Rate limit exceeded: {0}")]
    TooManyRequests(String),

    #[error("Service unavailable: {0}")]
    ServiceUnavailable(String),

    #[error("API overloaded: {0}")]
    Overloaded(String),
}

// Convert ProxyError to HTTP responses for Axum (matches Claude API error format)
impl IntoResponse for ProxyError {
    fn into_response(self) -> Response {
        let (status, error_type, message) = match self {
            // 401 - authentication_error
            ProxyError::OAuth(_) | ProxyError::InvalidCredentials(_) | ProxyError::TokenExpired | ProxyError::OAuthRefresh(_) => {
                (StatusCode::UNAUTHORIZED, "authentication_error", self.to_string())
            }
            // 400 - invalid_request_error
            ProxyError::InvalidRequest(_) | ProxyError::Translation(_) => {
                (StatusCode::BAD_REQUEST, "invalid_request_error", self.to_string())
            }
            // 429 - rate_limit_error
            ProxyError::TooManyRequests(_) => {
                (StatusCode::TOO_MANY_REQUESTS, "rate_limit_error", self.to_string())
            }
            // 529 - overloaded_error (Gemini API overloaded)
            ProxyError::Overloaded(_) => {
                (StatusCode::from_u16(529).unwrap(), "overloaded_error", self.to_string())
            }
            // 503 - api_error (Service unavailable)
            ProxyError::ServiceUnavailable(_) => {
                (StatusCode::SERVICE_UNAVAILABLE, "api_error", self.to_string())
            }
            // 500 - api_error (catch-all for internal errors)
            ProxyError::Config(_) | ProxyError::ConfigParsing(_) | ProxyError::Internal(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, "api_error", self.to_string())
            }
            // 502 - api_error (upstream API errors)
            ProxyError::ProjectResolution(_) | ProxyError::GeminiApi(_) => {
                (StatusCode::BAD_GATEWAY, "api_error", self.to_string())
            }
            // Default - api_error
            _ => {
                (StatusCode::INTERNAL_SERVER_ERROR, "api_error", self.to_string())
            }
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

pub type Result<T> = std::result::Result<T, ProxyError>;

