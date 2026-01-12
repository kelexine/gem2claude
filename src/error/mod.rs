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
}

// Convert ProxyError to HTTP responses for Axum
impl IntoResponse for ProxyError {
    fn into_response(self) -> Response {
        let (status, error_type, message) = match self {
            ProxyError::OAuth(_) | ProxyError::InvalidCredentials(_) | ProxyError::TokenExpired | ProxyError::OAuthRefresh(_) => {
                (StatusCode::UNAUTHORIZED, "authentication_error", self.to_string())
            }
            ProxyError::InvalidRequest(_) => {
                (StatusCode::BAD_REQUEST, "invalid_request_error", self.to_string())
            }
            ProxyError::Config(_) | ProxyError::ConfigParsing(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, "configuration_error", self.to_string())
            }
            ProxyError::ProjectResolution(_) => {
                (StatusCode::SERVICE_UNAVAILABLE, "project_resolution_error", self.to_string())
            }
            ProxyError::GeminiApi(_) => {
                (StatusCode::BAD_GATEWAY, "api_error", self.to_string())
            }
            ProxyError::Translation(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, "translation_error", self.to_string())
            }
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

