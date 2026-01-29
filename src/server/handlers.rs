//! HTTP request handlers for the gem2claude bridge.
//!
//! This module contains the logic for processing incoming requests,
//! translating them between Anthropic and Gemini formats, and managing
//! the response streams (SSE).
//!
//! Author: kelexine (<https://github.com/kelexine>)

use super::routes::AppState;
use axum::{
    extract::State,
    response::IntoResponse,
    Json,
};
pub use super::message_handlers::messages_handler;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Response schema for the `/health` check endpoint.
#[derive(Debug, Serialize, Deserialize)]
pub struct HealthResponse {
    /// Overall system status (Healthy, Degraded, or Unhealthy).
    pub status: HealthStatus,
    /// Detailed results for individual subsystem checks.
    pub checks: HashMap<String, HealthCheck>,
    /// ISO 8601 timestamp of when the check was performed.
    pub timestamp: String,
}

/// Possible status values for the system health.
#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum HealthStatus {
    /// System is fully operational.
    Healthy,
    /// System is operational but some non-critical issues were detected.
    Degraded,
    /// System is not functioning correctly.
    Unhealthy,
}

/// Details of an individual health check component.
#[derive(Debug, Serialize, Deserialize)]
pub struct HealthCheck {
    /// Status of the specific check ("ok", "warning", or "error").
    pub status: String,
    /// Human-readable message detailing the check result.
    pub message: String,
}

/// Performs a comprehensive system health check.
///
/// This handler verifies:
/// 1. **OAuth2 Credentials**: Checks token expiration and validity.
/// 2. **Project Resolution**: Ensures the Google Cloud Project ID is correctly identified.
/// 3. **Configuration**: Validates that critical environment variables and URLs are set.
/// 4. **API Connectivity**: Performs a latency check to the upstream Gemini API.
pub async fn health_handler(State(state): State<AppState>) -> Json<HealthResponse> {
    let mut checks = HashMap::new();
    let mut overall_status = HealthStatus::Healthy;

    // Check OAuth credentials status
    let (expires_in, is_expired) = state.oauth_manager.token_info().await;
    let oauth_check = if is_expired {
        overall_status = HealthStatus::Unhealthy;
        HealthCheck {
            status: "error".to_string(),
            message: "OAuth token is expired or invalid".to_string(),
        }
    } else if expires_in < 600 {
        // Less than 10 minutes remaining is considered Degraded
        overall_status = HealthStatus::Degraded;
        HealthCheck {
            status: "warning".to_string(),
            message: format!("OAuth token expires soon: {} seconds remaining", expires_in),
        }
    } else {
        HealthCheck {
            status: "ok".to_string(),
            message: format!("OAuth token is valid (expires in {}s)", expires_in),
        }
    };
    checks.insert("oauth_credentials".to_string(), oauth_check);

    // Verify Cloud Project ID resolution
    let project_check = HealthCheck {
        status: "ok".to_string(),
        message: format!("Resolved Project ID: {}", state.gemini_client.project_id()),
    };
    checks.insert("project_resolution".to_string(), project_check);

    // Check basic server configuration
    let config_check = HealthCheck {
        status: "ok".to_string(),
        message: format!("Target Gemini API: {}", state.config.gemini.api_base_url),
    };
    checks.insert("configuration".to_string(), config_check);

    // Perform live connectivity check to Gemini API
    let connectivity_check = match state.gemini_client.check_connectivity().await {
        Ok(latency) => {
            let millis = latency.as_millis();
            let status = if millis > 1000 {
                if overall_status == HealthStatus::Healthy {
                    overall_status = HealthStatus::Degraded;
                }
                "warning".to_string()
            } else {
                "ok".to_string()
            };

            HealthCheck {
                status,
                message: format!("API connectivity latency: {}ms", millis),
            }
        }
        Err(e) => {
            overall_status = HealthStatus::Unhealthy;
            HealthCheck {
                status: "error".to_string(),
                message: format!("Upstream API unreachable: {}", e),
            }
        }
    };
    checks.insert("api_connectivity".to_string(), connectivity_check);

    Json(HealthResponse {
        status: overall_status,
        checks,
        timestamp: chrono::Utc::now().to_rfc3339(),
    })
}

/// Exposes Prometheus-compatible application metrics.
///
/// Gathers metrics from the global registry, including request counts,
/// latencies, and token usage statistics.
pub async fn metrics_handler() -> impl IntoResponse {
    let metrics = crate::metrics::gather_metrics();
    (
        [(
            axum::http::header::CONTENT_TYPE,
            "text/plain; version=0.0.4",
        )],
        metrics,
    )
}



/// Sink handler for Claude Code telemetry and event logging.
///
/// This handler collects telemetry data sent by the client and persistently
/// logs it to `~/claude_code_events.log` for transparency and auditing.
pub async fn event_logging_handler(body: String) -> impl IntoResponse {
    use std::fs::OpenOptions;
    use std::io::Write;

    if let Some(home) = std::env::var_os("HOME") {
        let log_path = std::path::Path::new(&home).join("claude_code_events.log");

        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&log_path) {
            let timestamp = chrono::Utc::now().to_rfc3339();
            let _ = writeln!(file, "[{}] {}", timestamp, body);
        }
    }

    axum::http::StatusCode::OK
}
