//! HTTP request handlers for the gem2claude bridge.
//!
//! This module contains the logic for processing incoming requests,
//! translating them between Anthropic and Gemini formats, and managing
//! the response streams (SSE).
//!
//! Author: kelexine (<https://github.com/kelexine>)

use super::routes::AppState;
use axum::{extract::State, response::{IntoResponse, Response}, Json};
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
        [(axum::http::header::CONTENT_TYPE, "text/plain; version=0.0.4")],
        metrics
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

    debug!("Received Anthropic request: model={}, stream={:?}", req.model, req.stream);
    
    // Comprehensive request logging for debugging/audit trails
    debug!("â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”");
    debug!("ðŸ“‹ REQUEST HEADERS:");
    for (name, value) in headers.iter() {
        if let Ok(value_str) = value.to_str() {
            debug!("  {}: {}", name, value_str);
        }
    }
    debug!("â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”");
    
    let body_json = serde_json::to_string_pretty(&req).unwrap_or_else(|_| "{}".to_string());
    let body_preview = if body_json.len() > 1000 {
        format!("{}...\n(truncated)", &body_json[..1000])
    } else {
        body_json
    };
    debug!("ðŸ“„ REQUEST BODY PREVIEW:\n{}", body_preview);
    debug!("â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”");

    // Check for streaming vs non-streaming flow
    if req.stream.unwrap_or(false) {
        stream_messages_handler(state, req).await
    } else {
        non_stream_messages_handler(state, req).await
    }
}

/// Internal handler for non-streaming (unary) message requests.
///
/// Workflow:
/// 1. Maps Anthropic model names to Gemini model strings.
/// 2. Translates the request structure to Gemini format.
/// 3. Executes the request against the Google Gemini API.
/// 4. Translates the Gemini response back to Anthropic format.
/// 5. Records performance metrics and token usage.
async fn non_stream_messages_handler(
    state: AppState,
    req: crate::models::anthropic::MessagesRequest,
) -> Result<Response, crate::error::ProxyError> {
    use crate::translation::{translate_request, translate_response};
    use tracing::{debug, error};

    let request_start = std::time::Instant::now();

    // Model and request translation
    let gemini_model = crate::models::mapping::map_model(&req.model)?;
    
    // Check cache first (returns both cache name and cached translation if available)
    let (cached_content, cached_translation) = if let Some(cache_mgr) = &state.cache_manager {
        cache_mgr.get_or_create_cache(&req, state.gemini_client.project_id(), &state.gemini_client).await?
    } else {
        (None, None)
    };
    
    // Use cached translation if available, otherwise translate now
    let mut gemini_req = if let Some(cached_req) = cached_translation {
        debug!("Using cached translation (cache hit)");
        cached_req
    } else {
        debug!("Translating request (cache miss or disabled)");
        translate_request(
            req.clone(), 
            state.gemini_client.project_id(), 
            None,  // Cache manager already called above
            None   // Gemini client not needed for translation
        ).await?
    };
    
    // Apply cached content reference if present
    if let Some(cache_name) = cached_content {
        gemini_req.cached_content = Some(cache_name);
    }
    
    debug!("Executing unary Gemini request");

    // Upstream API call
    let gemini_resp = match state.gemini_client.generate_content(gemini_req, &gemini_model).await {
        Ok(resp) => resp,
        Err(e) => {
            error!("Gemini API call failed: {}", e);
            return Err(e);
        }
    };
    
    // Response translation back to Anthropic format
    let anthropic_resp = match translate_response(gemini_resp, &req.model) {
        Ok(resp) => resp,
        Err(e) => {
            error!("Response translation failed: {}", e);
            return Err(e);
        }
    };

    // Telemetry and Metrics
    let duration = request_start.elapsed().as_secs_f64();
    crate::metrics::record_request("POST", "/v1/messages", 200, &req.model, duration);
    crate::metrics::record_tokens(
        &req.model,
        anthropic_resp.usage.input_tokens,
        anthropic_resp.usage.output_tokens,
        anthropic_resp.usage.cache_read_input_tokens,
        anthropic_resp.usage.cache_creation_input_tokens,
    );

    Ok(Json(anthropic_resp).into_response())
}

/// Internal handler for Server-Sent Events (SSE) streaming requests.
///
/// Workflow:
/// 1. Translates request and initiates Gemini SSE stream.
/// 2. Spawns an asynchronous stream transformation loop.
/// 3. Translates Gemini response chunks to Anthropic stream events on-the-fly.
/// 4. Injects periodic keep-alive pings and buffer flushes to maintain the connection.
async fn stream_messages_handler(
    state: AppState,
    req: crate::models::anthropic::MessagesRequest,
) -> Result<Response, crate::error::ProxyError> {
    use futures::StreamExt;
    use crate::translation::streaming::StreamTranslator;
    use crate::translation::translate_request;
    use tracing::{debug, warn};

    debug!("Initiating streaming flow for model: {}", req.model);
    crate::metrics::record_sse_connection("opened");

    // 1. Initial translation and stream setup
    let gemini_model = crate::models::mapping::map_model(&req.model)?;
    
    // Check cache first (returns both cache name and cached translation if available)
    let (cached_content, cached_translation) = if let Some(cache_mgr) = &state.cache_manager {
        cache_mgr.get_or_create_cache(&req, state.gemini_client.project_id(), &state.gemini_client).await?
    } else {
        (None, None)
    };
    
    // Use cached translation if available, otherwise translate now
    let mut gemini_req = if let Some(cached_req) = cached_translation {
        debug!("Using cached translation (cache hit)");
        cached_req
    } else {
        debug!("Translating request (cache miss or disabled)");
        translate_request(
            req.clone(), 
            state.gemini_client.project_id(), 
            None,  // Cache manager already called above
            None   // Gemini client not needed for translation
        ).await?
    };
    
    // Apply cached content reference if present
    if let Some(cache_name) = cached_content {
        gemini_req.cached_content = Some(cache_name);
    }

    let gemini_stream = state.gemini_client
        .stream_generate_content(gemini_req, &gemini_model)
        .await?;

    let mut translator = StreamTranslator::new(req.model.clone());

    // 2. Define the transformation closure
    let sse_stream = async_stream::stream! {
        debug!("SSE Stream established");
        futures::pin_mut!(gemini_stream);
        
        let mut chunk_count = 0;
        loop {
            tokio::select! {
                // Poll the upstream Gemini stream
                chunk_opt = gemini_stream.next() => {
                    match chunk_opt {
                        Some(chunk_result) => {
                            chunk_count += 1;
                            match chunk_result {
                                Ok(chunk) => {
                                    // Translate raw Gemini chunk to Anthropic event sequence
                                    match translator.translate_chunk(chunk) {
                                        Ok(events) => {
                                            for event in events.iter() {
                                                yield Ok::<String, std::convert::Infallible>(event.to_sse());
                                                // Buffer flush hint: keepalive comment
                                                yield Ok(": keepalive\n\n".to_string());
                                            }
                                        }
                                        Err(e) => {
                                            warn!("Stream translation error: {}", e);
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
                                    warn!("Upstream stream error: {}", e);
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
                        None => break, // Clean stream completion
                    }
                }
                // Maintain connection with active pings
                _ = tokio::time::sleep(std::time::Duration::from_secs(15)) => {
                    debug!("Yielding keep-alive ping to client");
                    let ping_event = "event: ping\ndata: {\"type\": \"ping\"}\n\n".to_string();
                    yield Ok(ping_event);
                }
            }
        }
        debug!("SSE Stream finished. Total chunks: {}", chunk_count);
    }; 

    // 3. Construct the finalized HTTP/SSE response
    use axum::body::Body;
    let body = Body::from_stream(sse_stream);
    
    Ok(Response::builder()
        .status(200)
        .header("Content-Type", "text/event-stream; charset=utf-8")
        .header("Cache-Control", "no-cache")
        .header("Connection", "keep-alive")
        .header("X-Accel-Buffering", "no") // Important for proxy servers like Nginx
        .header("Transfer-Encoding", "chunked")
        .header("anthropic-version", "2023-06-01") // Mock headers for compatibility
        .header("anthropic-ratelimit-requests-limit", "50")
        .header("anthropic-ratelimit-requests-remaining", "49")
        .header("anthropic-ratelimit-requests-reset", chrono::Utc::now().to_rfc3339())
        .header("request-id", format!("req_{}", uuid::Uuid::new_v4()))
        .body(body)
        .unwrap())
}

/// Sink handler for Claude Code telemetry and event logging.
///
/// Claude Code sometimes sends "batch" event logs. This handler captures them,
/// appends them to a local log file for auditing, and returns a 200 OK to
/// ensure compatibility with the client's expectations.
pub async fn event_logging_handler(
    body: String,
) -> impl IntoResponse {
    use std::fs::OpenOptions;
    use std::io::Write;
    
    // Log telemetry events to the home directory for transparency
    if let Some(home) = std::env::var_os("HOME") {
        let log_path = std::path::Path::new(&home).join("claude_code_events.log");
        
        if let Ok(mut file) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
        {
            let timestamp = chrono::Utc::now().to_rfc3339();
            let _ = writeln!(file, "[{}] {}", timestamp, body);
        }
    }
    
    axum::http::StatusCode::OK
}
