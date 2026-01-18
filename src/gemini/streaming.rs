//! Gemini API streaming implementation.
//!
//! This module provides the heavy-lifting for streaming content generation from
//! Google's internal Gemini API. It implements a robust Server-Sent Events (SSE)
//! parser that can handle byte-level boundary conditions and various line-ending
//! delimiters.

// Author: kelexine (https://github.com/kelexine)

use crate::error::{ProxyError, Result};
use crate::models::gemini::GenerateContentResponse;
use crate::oauth::OAuthManager;
use futures::stream::Stream;
use reqwest::Client;
use std::pin::Pin;
use std::time::Instant;
use tracing::{debug, warn};

/// Initiates a streaming content generation request to the Gemini API.
///
/// This function handles the initial HTTP connection, authentication via `OAuthManager`,
/// and returns a pinned stream of `GenerateContentResponse` chunks.
///
/// # Arguments
///
/// * `client` - The HTTP client to use for the request.
/// * `url` - The Gemini API endpoint URL.
/// * `request_body` - The JSON-encoded request body.
/// * `oauth_manager` - Manager for handling OAuth2 tokens and authentication.
/// * `model` - The model name for metrics.
///
/// # Returns
///
/// A `Result` containing a pinned, boxable `Stream` of Gemini responses.
///
/// # Errors
///
/// Returns a `ProxyError` if:
/// * Authentication fails.
/// * The HTTP request fails to send.
/// * The Gemini API returns a non-success status code.
pub async fn stream_generate_content(
    client: &Client,
    url: String,
    request_body: String,
    oauth_manager: &OAuthManager,
    model: &str,
) -> Result<Pin<Box<dyn Stream<Item = Result<GenerateContentResponse>> + Send>>> {
    debug!("Starting Gemini SSE stream to: {}", url);

    let client = client.clone();
    let url_clone = url.clone();
    let request_body_clone = request_body.clone();
    let oauth_manager = oauth_manager.clone();
    let model = model.to_string();

    // Handshake stage: authenticate and open the connection.
    let start_time = Instant::now();
    let access_token = oauth_manager.get_token().await?;

    let response = client
        .post(&url_clone)
        .header("Authorization", format!("Bearer {}", access_token))
        .header("Content-Type", "application/json")
        .header("Accept", "text/event-stream")
        .body(request_body_clone.clone())
        .send()
        .await
        .map_err(|e| ProxyError::GeminiApi(format!("HTTP error during handshake: {}", e)))?;

    let duration = start_time.elapsed().as_secs_f64();
    let status = response.status();

    // Record setup time to track Time To First Byte (TTFB) latency.
    crate::metrics::record_gemini_call(&model, status.as_u16(), true, duration);

    if !status.is_success() {
        let error_text = response.text().await.unwrap_or_default();
        return Err(match status.as_u16() {
            429 => ProxyError::TooManyRequests(error_text),
            529 => ProxyError::Overloaded(error_text),
            503 | 504 => ProxyError::ServiceUnavailable(error_text),
            _ => ProxyError::GeminiApi(format!("HTTP {}: {}", status, error_text)),
        });
    }

    // Convert the response body into a byte stream and pipe it into our SSE parser.
    let byte_stream = response.bytes_stream();
    let event_stream = parse_sse_stream(byte_stream);

    Ok(Box::pin(event_stream))
}

/// Parses a byte stream into a stream of `GenerateContentResponse` chunks according to the SSE protocol.
///
/// This internal function manages an internal buffer to handle partial chunks received over the wire,
/// ensuring that only complete SSE events (separated by double newlines) are processed.
///
/// # Arguments
///
/// * `byte_stream` - An implementation of `Stream` yielding bytes from the HTTP response.
fn parse_sse_stream<S>(byte_stream: S) -> impl Stream<Item = Result<GenerateContentResponse>> + Send
where
    S: Stream<Item = reqwest::Result<bytes::Bytes>> + Send + 'static,
{
    use futures::StreamExt;

    async_stream::stream! {
        let mut buffer = String::new();

        futures::pin_mut!(byte_stream);

        while let Some(chunk_result) = byte_stream.next().await {
            match chunk_result {
                Ok(chunk) => {
                    let chunk_str = String::from_utf8_lossy(&chunk);
                    debug!("Received chunk: {} bytes", chunk.len());
                    buffer.push_str(&chunk_str);

                    // SSE event boundary scanning.
                    // We look for double newlines which indicate the end of an event block.
                    let mut events_in_this_chunk = 0;
                    loop {
                        let lf_pos = buffer.find("\n\n");
                        let crlf_pos = buffer.find("\r\n\r\n");

                        // Pick the first delimiter found to maintain sequence order.
                        let (event_end, delim_len) = match (lf_pos, crlf_pos) {
                            (Some(lf), Some(crlf)) => {
                                if lf <= crlf { (lf, 2) } else { (crlf, 4) }
                            }
                            (Some(lf), None) => (lf, 2),
                            (None, Some(crlf)) => (crlf, 4),
                            (None, None) => break, // Fragmented event; wait for more data.
                        };

                        let event_data = buffer[..event_end].to_string();
                        buffer = buffer[event_end + delim_len..].to_string();

                        if let Some(response) = parse_sse_event(&event_data) {
                            events_in_this_chunk += 1;
                            yield Ok(response);
                        }
                    }
                    if events_in_this_chunk > 0 {
                        debug!("Buffered and sent {} responses", events_in_this_chunk);
                    }
                }
                Err(e) => {
                    warn!("Upstream network error during stream: {}", e);
                    yield Err(ProxyError::Http(e));
                    break;
                }
            }
        }

        // Final buffer flush for streams that might not end with a clean delimiter.
        if !buffer.trim().is_empty() {
            if let Some(response) = parse_sse_event(&buffer) {
                yield Ok(response);
            }
        }

        debug!("Gemini SSE stream closed");
    }
}

/// Parses a raw SSE event string into a structured `GenerateContentResponse`.
///
/// Extracts the `data:` segment and handles protocol control markers like `[DONE]`.
fn parse_sse_event(event_data: &str) -> Option<GenerateContentResponse> {
    let lines: Vec<&str> = event_data.lines().collect();

    let mut data_line = None;
    for line in lines {
        // SSE standard: data lines are prefixed with 'data:'
        if let Some(data) = line.strip_prefix("data:") {
            data_line = Some(data.trim());
            break;
        }
        // Handle variations with explicit space.
        if let Some(data) = line.strip_prefix("data: ") {
            data_line = Some(data);
            break;
        }
    }

    let data = data_line?;

    // The [DONE] marker is a protocol signal that generation is complete.
    if data.is_empty() || data == "[DONE]" {
        debug!("Filtered SSE control marker: {}", data);
        return None;
    }

    // Individual JSON chunks represent incremental updates to the candidate list.
    match serde_json::from_str::<GenerateContentResponse>(data) {
        Ok(response) => Some(response),
        Err(e) => {
            warn!("JSON decode error in SSE stream: {}", e);
            debug!(
                "Fragment causing error: {}",
                data.chars().take(200).collect::<String>()
            );
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_sse_event() {
        let event =  "event: message\ndata: {\"response\":{\"candidates\":[{\"content\":{\"role\":\"model\",\"parts\":[{\"text\":\" Hello\"}]}}]}}";
        let result = parse_sse_event(event);
        assert!(result.is_some());
    }

    #[test]
    fn test_parse_sse_event_no_data() {
        let event = "event: ping";
        let result = parse_sse_event(event);
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_parse_sse_stream_mixed_delimiters() {
        use futures::StreamExt;

        let event1 = r#"data: {"response":{"candidates":[{"content":{"role":"model","parts":[{"text":"First"}]}}]}}"#;
        let event2 = r#"data: {"response":{"candidates":[{"content":{"role":"model","parts":[{"text":"Second"}]}}]}}"#;
        let event3 = r#"data: {"response":{"candidates":[{"content":{"role":"model","parts":[{"text":"Third"}]}}]}}"#;

        let payload = format!("{}\n\n{}\r\n\r\n{}\n\n", event1, event2, event3);

        let stream = futures::stream::iter(vec![Ok(bytes::Bytes::from(payload))]);

        let parsed_stream = parse_sse_stream(stream);
        futures::pin_mut!(parsed_stream);
        let mut events = Vec::new();

        while let Some(result) = parsed_stream.next().await {
            events.push(result.unwrap());
        }

        assert_eq!(events.len(), 3);
        assert_eq!(
            events[0].response.as_ref().unwrap().candidates[0]
                .content
                .parts[0]
                .as_text()
                .unwrap(),
            "First"
        );
    }
}
