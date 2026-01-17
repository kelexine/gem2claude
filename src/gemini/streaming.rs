//! Gemini API streaming implementation.
//!
//! This module provides functionality for streaming content generation from the Gemini API,
//! handling Server-Sent Events (SSE) and robustly parsing the response stream.
//!
//! Author: kelexine (<https://github.com/kelexine>)

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
/// * The Gemini API returns a non-success status code (429, 529, 503, 504 are mapped to specific errors).
pub async fn stream_generate_content(
    client: &Client,
    url: String,
    request_body: String,
    oauth_manager: &OAuthManager,
    model: &str,
) -> Result<Pin<Box<dyn Stream<Item = Result<GenerateContentResponse>> + Send>>> {
    debug!("Starting Gemini SSE stream to: {}", url);

    // Clone required components for the stream lifecycle
    let client = client.clone();
    let url_clone = url.clone();
    let request_body_clone = request_body.clone();
    let oauth_manager = oauth_manager.clone();
    let model = model.to_string(); // Metric label needs to be owned or static, usually we pass &str.
                                   // But here we just record before returning.

    // Per Claude API docs (our target bridge format): streaming errors should be returned
    // immediately during the initial handshake, not silently retried within the stream.
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
        .map_err(|e| ProxyError::GeminiApi(format!("HTTP error: {}", e)))?;

    let duration = start_time.elapsed().as_secs_f64();
    let status = response.status();

    // Record API setup metric (TTFB/Connection)
    crate::metrics::record_gemini_call(&model, status.as_u16(), true, duration);

    if !status.is_success() {
        let error_text = response.text().await.unwrap_or_default();
        // Map common API error statuses to specific ProxyError variants to match Claude API expectations
        return Err(match status.as_u16() {
            429 => ProxyError::TooManyRequests(error_text),
            529 => ProxyError::Overloaded(error_text),
            503 | 504 => ProxyError::ServiceUnavailable(error_text),
            _ => ProxyError::GeminiApi(format!("HTTP {}: {}", status, error_text)),
        });
    }

    // After success, Convert response to a byte stream for SSE parsing
    let byte_stream = response.bytes_stream();

    // Wrap the byte stream in our SSE parser
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
///
/// # Returns
///
/// A stream yielding `Result<GenerateContentResponse>` items.
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
                    debug!("Received chunk: {} bytes, content: {:?}", chunk.len(), chunk_str.chars().take(200).collect::<String>());
                    buffer.push_str(&chunk_str);

                    // Parse all complete SSE events in the buffer.
                    // Gemini uses either standard LF (\n\n) or CRLF (\r\n\r\n) as event delimiters.
                    let mut events_in_this_chunk = 0;
                    loop {
                        // Scan for the first available delimiter in the current buffer
                        let lf_pos = buffer.find("\n\n");
                        let crlf_pos = buffer.find("\r\n\r\n");

                        // Select the earliest delimiter to maintain order and handle mixed line endings
                        let (event_end, delim_len) = match (lf_pos, crlf_pos) {
                            (Some(lf), Some(crlf)) => {
                                if lf <= crlf {
                                    (lf, 2)
                                } else {
                                    (crlf, 4)
                                }
                            }
                            (Some(lf), None) => (lf, 2),
                            (None, Some(crlf)) => (crlf, 4),
                            (None, None) => break, // No complete event found yet, wait for more data
                        };

                        // Extract the event data and advance the buffer
                        let event_data = buffer[..event_end].to_string();
                        buffer = buffer[event_end + delim_len..].to_string();

                        debug!("Found complete SSE event (len: {})", event_data.len());

                        // Attempt to parse the individual SSE event
                        if let Some(response) = parse_sse_event(&event_data) {
                            events_in_this_chunk += 1;
                            yield Ok(response);
                        }
                    }
                    if events_in_this_chunk > 0 {
                        debug!("Processed {} SSE events from this HTTP chunk", events_in_this_chunk);
                    }
                }
                Err(e) => {
                    warn!("Stream error: {}", e);
                    yield Err(ProxyError::Http(e));
                    break;
                }
            }
        }

        debug!("HTTP byte stream ended - no more chunks from Gemini");

        // Handle potential final event that might not be followed by a delimiter
        if !buffer.trim().is_empty() {
            debug!("Processing remaining buffer: {} chars", buffer.len());
            if let Some(response) = parse_sse_event(&buffer) {
                debug!("Successfully parsed final SSE event from remaining buffer");
                yield Ok(response);
            } else {
                debug!("Failed to parse remaining buffer as SSE event");
            }
        }

        debug!("Gemini SSE stream ended");
    }
}

/// Parses a single SSE event string into a `GenerateContentResponse`.
///
/// This function handles the "data:" prefix and ignores `\[DONE\]` markers or empty lines.
/// It expects the data segment to be a valid JSON representation of `GenerateContentResponse`.
fn parse_sse_event(event_data: &str) -> Option<GenerateContentResponse> {
    // SSE event format can include multiple lines, e.g.: "event: message\ndata: <json>"
    let lines: Vec<&str> = event_data.lines().collect();

    let mut data_line = None;
    for line in lines {
        // Extract the JSON payload after the 'data:' prefix
        if let Some(data) = line.strip_prefix("data:") {
            data_line = Some(data.trim());
            break;
        }
        // Robustness: handle cases where there might be no space after the colon
        if let Some(data) = line.strip_prefix("data: ") {
            data_line = Some(data);
            break;
        }
    }

    let data = data_line?;

    // Ignore internal protocol markers like "[DONE]" which signify the end of SSE stream
    if data.is_empty() || data == "[DONE]" {
        debug!("Skipping empty or DONE marker");
        return None;
    }

    // Deserialize the JSON payload directly into our response model
    match serde_json::from_str::<GenerateContentResponse>(data) {
        Ok(response) => {
            debug!("Successfully parsed SSE event into GenerateContentResponse");
            Some(response)
        }
        Err(e) => {
            warn!("Failed to parse SSE event: {}", e);
            debug!("Raw data: {}", data.chars().take(200).collect::<String>());
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

        // Create a stream with mixed delimiters: \n\n (LF) and \r\n\r\n (CRLF)
        let event1 = r#"data: {"response":{"candidates":[{"content":{"role":"model","parts":[{"text":"First"}]}}]}}"#;
        let event2 = r#"data: {"response":{"candidates":[{"content":{"role":"model","parts":[{"text":"Second"}]}}]}}"#;
        let event3 = r#"data: {"response":{"candidates":[{"content":{"role":"model","parts":[{"text":"Third"}]}}]}}"#;

        // LF, CRLF, LF
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
        assert_eq!(
            events[1].response.as_ref().unwrap().candidates[0]
                .content
                .parts[0]
                .as_text()
                .unwrap(),
            "Second"
        );
        assert_eq!(
            events[2].response.as_ref().unwrap().candidates[0]
                .content
                .parts[0]
                .as_text()
                .unwrap(),
            "Third"
        );
    }
}
