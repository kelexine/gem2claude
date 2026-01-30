// Message handlers for Anthropic API compatible endpoints
// Author: kelexine (https://github.com/kelexine)

use super::routes::AppState;
use crate::error::ProxyError;
use crate::models::anthropic::MessagesRequest;
use axum::{
    extract::State,
    response::{IntoResponse, Response},
    Json,
};
use tracing::{debug, error, warn};

/// Unified handler for the Anthropic Messages API compatible endpoint (`/v1/messages`).
pub async fn messages_handler(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(req): Json<MessagesRequest>,
) -> Result<Response, ProxyError> {
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
async fn non_stream_messages_handler(
    state: AppState,
    req: MessagesRequest,
) -> Result<Response, ProxyError> {
    use crate::translation::{translate_request, translate_response};

    let request_start = std::time::Instant::now();
    let gemini_model = crate::models::mapping::map_model(&req.model)?;

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
async fn stream_messages_handler(
    state: AppState,
    req: MessagesRequest,
) -> Result<Response, ProxyError> {
    use crate::translation::streaming::StreamTranslator;
    use crate::translation::translate_request;
    use futures::StreamExt;

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

    let body = axum::body::Body::from_stream(sse_stream);

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
