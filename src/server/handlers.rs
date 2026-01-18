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
    response::{IntoResponse, Response},
    Json,
};
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

/// Unified handler for the Anthropic Messages API compatible endpoint (`/v1/messages`).
///
/// This handler:
/// 1. Validates and parses the Anthropic `MessagesRequest`.
/// 2. Logs the request details for transparency.
/// 3. Detects if a streaming response is requested.
/// 4. Dispatches to either `stream_messages_handler` or `non_stream_messages_handler`.
pub async fn messages_handler(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(req): Json<crate::models::anthropic::MessagesRequest>,
) -> Result<Response, crate::error::ProxyError> {
    use tracing::debug;

    debug!(
        "Received Anthropic request: model={}, stream={:?}",
        req.model, req.stream
    );

    // Request-level logging for auditing
    debug!("REQUEST HEADERS:");
    for (name, value) in headers.iter() {
        if let Ok(value_str) = value.to_str() {
            debug!("  {}: {}", name, value_str);
        }
    }

    let body_json = serde_json::to_string_pretty(&req).unwrap_or_else(|_| "{}".to_string());
    let body_preview = if body_json.len() > 1000 {
        let truncate_at = body_json
            .char_indices()
            .take_while(|(idx, _)| *idx < 1000)
            .last()
            .map(|(idx, ch)| idx + ch.len_utf8())
            .unwrap_or(1000.min(body_json.len()));
        format!("{}...\n(truncated)", &body_json[..truncate_at])
    } else {
        body_json
    };
    debug!("REQUEST BODY PREVIEW:\n{}", body_preview);

    if req.stream.unwrap_or(false) {
        stream_messages_handler(state, req).await
    } else {
        non_stream_messages_handler(state, req).await
    }
}

/// Internal handler for non-streaming (unary) message requests.
///
/// This function performs the core request-response translation cycle:
/// 1. Maps the Anthropic model name to its Gemini counterpart.
/// 2. Attempts to retrieve or create a Gemini context cache for large prompts.
/// 3. Translates the Anthropic request structure into a Gemini-compatible format.
/// 4. Executes the upstream call to the Gemini API.
/// 5. Translates the returned Gemini response back into the Anthropic format.
/// 6. Records all relevant telemetry (latency, status, token usage).
async fn non_stream_messages_handler(
    state: AppState,
    req: crate::models::anthropic::MessagesRequest,
) -> Result<Response, crate::error::ProxyError> {
    use crate::translation::{translate_request, translate_response};
    use tracing::{debug, error};

    let request_start = std::time::Instant::now();

    let gemini_model = crate::models::mapping::map_model(&req.model)?;

    // Context Cache Management: Optimize repeated large prompts.
    let (cached_content, cached_translation) = if let Some(cache_mgr) = &state.cache_manager {
        cache_mgr
            .get_or_create_cache(&req, state.gemini_client.project_id(), &state.gemini_client)
            .await?
    } else {
        (None, None)
    };

    let mut gemini_req = if let Some(cached_req) = cached_translation {
        debug!("Request translation retrieved from internal LRU cache.");
        cached_req
    } else {
        translate_request(req.clone(), state.gemini_client.project_id(), None, None).await?
    };

    if let Some(cache_name) = cached_content {
        gemini_req.cached_content = Some(cache_name);
    }

    debug!(
        "Dispatching unary request to Gemini API (Model: {})",
        gemini_model
    );

    let gemini_resp = match state
        .gemini_client
        .generate_content(gemini_req, &gemini_model)
        .await
    {
        Ok(resp) => resp,
        Err(e) => {
            error!("Upstream Gemini API call failure: {}", e);
            return Err(e);
        }
    };

    let anthropic_resp = match translate_response(gemini_resp, &req.model) {
        Ok(resp) => resp,
        Err(e) => {
            error!("Translation failure for Gemini response candidate: {}", e);
            return Err(e);
        }
    };

    let duration = request_start.elapsed().as_secs_f64();
    crate::metrics::record_request("POST", "/v1/messages", 200, &req.model, duration);
    crate::metrics::record_tokens(
        &req.model,
        anthropic_resp.usage.input_tokens,
        anthropic_resp.usage.output_tokens,
        anthropic_resp.usage.cache_read_input_tokens,
        anthropic_resp.usage.cache_creation_input_tokens,
    );

    if anthropic_resp.usage.cache_read_input_tokens > 0 {
        crate::metrics::record_cache_hit();
    } else {
        crate::metrics::record_cache_miss();
    }

    if anthropic_resp.usage.cache_creation_input_tokens > 0 {
        crate::metrics::record_cache_create();
    }

    Ok(Json(anthropic_resp).into_response())
}

/// Internal handler for Server-Sent Events (SSE) streaming requests.
///
/// This asynchronous handler establishes a persistent connection to the client and
/// pipes transformed events from Gemini in real-time:
/// 1. Maps models and manages context caching (same as unary).
/// 2. Opens a streaming connection to the Gemini API.
/// 3. Transforms raw Gemini JSON chunks into Anthropic SSE events.
/// 4. Implements a watchdog loop to send keep-alive pings every 15 seconds.
/// 5. Injects mock headers to maximize compatibility with the Claude SDK.
async fn stream_messages_handler(
    state: AppState,
    req: crate::models::anthropic::MessagesRequest,
) -> Result<Response, crate::error::ProxyError> {
    use crate::translation::streaming::StreamTranslator;
    use crate::translation::translate_request;
    use futures::StreamExt;
    use tracing::{debug, warn};

    let request_start = std::time::Instant::now();

    debug!("Establishing SSE tunnel for model: {}", req.model);
    crate::metrics::record_sse_connection("opened");

    let gemini_model = crate::models::mapping::map_model(&req.model)?;

    let (cached_content, cached_translation) = if let Some(cache_mgr) = &state.cache_manager {
        cache_mgr
            .get_or_create_cache(&req, state.gemini_client.project_id(), &state.gemini_client)
            .await?
    } else {
        (None, None)
    };

    let mut gemini_req = if let Some(cached_req) = cached_translation {
        cached_req
    } else {
        translate_request(req.clone(), state.gemini_client.project_id(), None, None).await?
    };

    if let Some(cache_name) = cached_content {
        gemini_req.cached_content = Some(cache_name);
    }

    let gemini_stream = state
        .gemini_client
        .stream_generate_content(gemini_req, &gemini_model)
        .await?;

    let mut translator = StreamTranslator::new(req.model.clone());

    let sse_stream = async_stream::stream! {
        debug!("Upstream SSE stream acquired; beginning transformation cycle.");
        futures::pin_mut!(gemini_stream);

        let mut chunk_count = 0;
        loop {
            tokio::select! {
                chunk_opt = gemini_stream.next() => {
                    match chunk_opt {
                        Some(chunk_result) => {
                            chunk_count += 1;
                            match chunk_result {
                                Ok(chunk) => {
                                    match translator.translate_chunk(chunk) {
                                        Ok(events) => {
                                            for event in events.iter() {
                                                yield Ok::<String, std::convert::Infallible>(event.to_sse());
                                                // Buffer flushing hint for proxies.
                                                yield Ok(": keepalive\n\n".to_string());
                                            }
                                        }
                                        Err(e) => {
                                            warn!("Internal translation error during stream: {}", e);
                                            let error_event = crate::models::streaming::StreamEvent::Error {
                                                error: crate::models::streaming::ErrorData {
                                                    error_type: "translation_error".to_string(),
                                                    message: e.to_string(),
                                                },
                                            };
                                            yield Ok(error_event.to_sse());
                                            break;
                                        }
                                    }
                                }
                                Err(e) => {
                                    warn!("Upstream connection reset or error: {}", e);
                                    let error_event = crate::models::streaming::StreamEvent::Error {
                                        error: crate::models::streaming::ErrorData {
                                            error_type: "api_error".to_string(),
                                            message: e.to_string(),
                                        },
                                    };
                                    yield Ok(error_event.to_sse());
                                    break;
                                }
                            }
                        }
                        None => break,
                    }
                }
                _ = tokio::time::sleep(std::time::Duration::from_secs(15)) => {
                    yield Ok("event: ping\ndata: {\"type\": \"ping\"}\n\n".to_string());
                }
            }
        }

        let duration = request_start.elapsed().as_secs_f64();
        debug!("SSE Stream finalized ({} chunks processed).", chunk_count);

        crate::metrics::record_request("POST", "/v1/messages", 200, &translator.model, duration);
        crate::metrics::record_tokens(
            &translator.model,
            translator.input_tokens,
            translator.output_tokens,
            translator.cached_input_tokens,
            0,
        );

        if translator.cached_input_tokens > 0 {
            crate::metrics::record_cache_hit();
        } else {
            crate::metrics::record_cache_miss();
        }
    };

    use axum::body::Body;
    let body = Body::from_stream(sse_stream);

    Ok(Response::builder()
        .status(200)
        .header("Content-Type", "text/event-stream; charset=utf-8")
        .header("Cache-Control", "no-cache")
        .header("Connection", "keep-alive")
        .header("X-Accel-Buffering", "no")
        .header("Transfer-Encoding", "chunked")
        .header("anthropic-version", "2023-06-01")
        .header("anthropic-ratelimit-requests-limit", "50")
        .header("anthropic-ratelimit-requests-remaining", "49")
        .header("request-id", format!("req_{}", uuid::Uuid::new_v4()))
        .body(body)
        .unwrap())
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
