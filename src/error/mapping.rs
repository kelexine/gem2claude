// HTTP response mapping for ProxyError
// Author: kelexine (https://github.com/kelexine)

use crate::error::definitions::ProxyError;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::json;

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
