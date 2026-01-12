// HTTP request handlers
// Author: kelexine (https://github.com/kelexine)

use super::routes::AppState;
use axum::{extract::State, response::{IntoResponse, Response}, Json};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Serialize, Deserialize)]
pub struct HealthResponse {
    pub status: HealthStatus,
    pub checks: HashMap<String, HealthCheck>,
    pub timestamp: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HealthStatus {
    Healthy,
    Degraded,
    Unhealthy,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct HealthCheck {
    pub status: String,
    pub message: String,
}

pub async fn health_handler(State(state): State<AppState>) -> Json<HealthResponse> {
    let mut checks = HashMap::new();
    let mut overall_status = HealthStatus::Healthy;

    // Check OAuth credentials
    let (expires_in, is_expired) = state.oauth_manager.token_info().await;
    let oauth_check = if is_expired {
        overall_status = HealthStatus::Unhealthy;
        HealthCheck {
            status: "error".to_string(),
            message: "Token expired".to_string(),
        }
    } else if expires_in < 600 {
        // Less than 10 minutes
        overall_status = HealthStatus::Degraded;
        HealthCheck {
            status: "warning".to_string(),
            message: format!("Token expires in {} seconds", expires_in),
        }
    } else {
        HealthCheck {
            status: "ok".to_string(),
            message: format!("Valid token, expires in {} seconds", expires_in),
        }
    };
    checks.insert("oauth_credentials".to_string(), oauth_check);

    // Check project resolution
    let project_check = HealthCheck {
        status: "ok".to_string(),
        message: format!("Project ID: {}", state.gemini_client.project_id()),
    };
    checks.insert("project_resolution".to_string(), project_check);

    // Check configuration
    let config_check = HealthCheck {
        status: "ok".to_string(),
        message: format!("API base: {}", state.config.gemini.api_base_url),
    };
    checks.insert("configuration".to_string(), config_check);

    Json(HealthResponse {
        status: overall_status,
        checks,
        timestamp: chrono::Utc::now().to_rfc3339(),
    })
}

/// Handler for /v1/messages endpoint (Anthropic Messages API compatible)
pub async fn messages_handler(
    State(state): State<AppState>,
    Json(req): Json<crate::models::anthropic::MessagesRequest>,
) -> Result<Response, crate::error::ProxyError> {
    use tracing::{info, debug};

    info!(
        "Received messages request: model={}, messages={}, stream={}",
        req.model,
        req.messages.len(),
        req.stream.unwrap_or(false)
    );

    // Check if streaming is requested
    if req.stream.unwrap_or(false) {
        stream_messages_handler(state, req).await
    } else {
        non_stream_messages_handler(state, req).await
    }
}

/// Handle non-streaming messages (original implementation)
async fn non_stream_messages_handler(
    state: AppState,
    req: crate::models::anthropic::MessagesRequest,
) -> Result<Response, crate::error::ProxyError> {
    use crate::translation::{translate_request, translate_response};
    use tracing::{debug, error};

    // 1. Translate Anthropic request to Gemini format
    let gemini_model = crate::models::mapping::map_model(&req.model)?;
    let gemini_req = translate_request(req.clone(), state.gemini_client.project_id())?;
    
    debug!("Translated request to Gemini format");

    // 2. Call Gemini API
    let gemini_resp = match state.gemini_client.generate_content(gemini_req, &gemini_model).await {
        Ok(resp) => resp,
        Err(e) => {
            error!("Gemini API call failed: {}", e);
            return Err(e);
        }
    };
    
    debug!("Received Gemini response");

    // 3. Translate response back to Anthropic format
    let anthropic_resp = match translate_response(gemini_resp, &req.model) {
        Ok(resp) => resp,
        Err(e) => {
            error!("Response translation failed: {}", e);
            return Err(e);
        }
    };

    debug!("Translated response to Anthropic format");

    // Return JSON response
    Ok(Json(anthropic_resp).into_response())
}

/// Handle streaming messages with SSE
async fn stream_messages_handler(
    state: AppState,
    req: crate::models::anthropic::MessagesRequest,
) -> Result<Response, crate::error::ProxyError> {
    use futures::StreamExt;
    use crate::translation::streaming::StreamTranslator;
    use crate::translation::translate_request;
    use tracing::{debug, warn};

    debug!("Starting streaming response for model: {}", req.model);

    // 1. Translate request
    let gemini_model = crate::models::mapping::map_model(&req.model)?;
    let gemini_req = translate_request(req.clone(), state.gemini_client.project_id())?;

    // 2. Start Gemini stream
    let gemini_stream = state.gemini_client
        .stream_generate_content(gemini_req, &gemini_model)
        .await?;

    // 3. Create translator
    let mut translator = StreamTranslator::new(req.model.clone());

    // 4. Transform Gemini chunks to Anthropic SSE events
    let sse_stream = async_stream::stream! {
        debug!("Starting SSE stream transformation");
        futures::pin_mut!(gemini_stream);
        
        let mut chunk_count = 0;
        while let Some(chunk_result) = gemini_stream.next().await {
            chunk_count += 1;
            debug!("Received Gemini chunk #{}", chunk_count);
            
            match chunk_result {
                Ok(chunk) => {
                    // Translate chunk to events
                    match translator.translate_chunk(chunk) {
                        Ok(events) => {
                            debug!("Translated chunk to {} events", events.len());
                            for (i, event) in events.iter().enumerate() {
                                debug!("Yielding event #{}: {:?}", i, event);
                                yield Ok::<String, std::convert::Infallible>(event.to_sse());
                            }
                        }
                        Err(e) => {
                            warn!("Translation error: {}", e);
                            // Send error event
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
                    warn!("Stream error: {}", e);
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
        debug!("SSE stream ended after {} chunks", chunk_count);
    };

    // 5. Convert to axum response
    use axum::body::Body;
    
    let body = Body::from_stream(sse_stream);
    
    Ok(Response::builder()
        .status(200)
        .header("Content-Type", "text/event-stream")
        .header("Cache-Control", "no-cache")
        .header("Connection", "keep-alive")
        .header("X-Accel-Buffering", "no") // Disable nginx buffering
        .body(body)
        .unwrap())
}
