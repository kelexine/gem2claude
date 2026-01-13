// Gemini API streaming client
// Author: kelexine (https://github.com/kelexine)

use crate::error::{ProxyError, Result};
use crate::models::gemini::GenerateContentResponse;
use crate::oauth::OAuthManager;
use futures::stream::Stream;
use reqwest::Client;
use serde::Deserialize;
use std::pin::Pin;
use tracing::{debug, warn};

/// Parse Gemini SSE stream with retry logic for initial connection
pub async fn stream_generate_content(
    client: &Client,
    url: String,
    request_body: String,
    oauth_manager: &OAuthManager,
) -> Result<Pin<Box<dyn Stream<Item = Result<GenerateContentResponse>> + Send>>> {
    debug!("Starting Gemini SSE stream to: {}", url);

    // Clone what we need for the retry closure
    let client = client.clone();
    let url_clone = url.clone();
    let request_body_clone = request_body.clone();
    let oauth_manager = oauth_manager.clone();

    // Retry the initial HTTP connection (handles 429 at stream start)
    let response = crate::utils::retry::with_retry(
        "Gemini SSE Stream",
        || async {
            let access_token = oauth_manager.get_token().await
                .map_err(|e| (500u16, format!("OAuth error: {}", e)))?;

            let response = client
                .post(&url_clone)
                .header("Authorization", format!("Bearer {}", access_token))
                .header("Content-Type", "application/json")
                .header("Accept", "text/event-stream")
                .body(request_body_clone.clone())
                .send()
                .await
                .map_err(|e| (500u16, format!("HTTP error: {}", e)))?;

            let status = response.status();
            if !status.is_success() {
                let error_text = response.text().await.unwrap_or_default();
                return Err((status.as_u16(), error_text));
            }

            Ok(response)
        },
    )
    .await
    .map_err(|(status, error_body)| {
        ProxyError::GeminiApi(format!("HTTP {}: {}", status, error_body))
    })?;

    // Convert response to byte stream (after successful connection)
    let byte_stream = response.bytes_stream();

    // Parse SSE events
    let event_stream = parse_sse_stream(byte_stream);

    Ok(Box::pin(event_stream))
}

/// Parse SSE byte stream into GenerateContentResponse chunks
fn parse_sse_stream<S>(
    byte_stream: S,
) -> impl Stream<Item = Result<GenerateContentResponse>> + Send
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

                    // Internal Gemini API uses CRLF line endings (\r\n\r\n), not just LF (\n\n)
                    let mut events_in_this_chunk = 0;
                    while let Some(event_end) = buffer.find("\r\n\r\n") {
                        let event_data = buffer[..event_end].to_string();
                        buffer = buffer[event_end + 4..].to_string(); // Skip 4 chars: \r\n\r\n

                        debug!("Found complete SSE event: {}", event_data.chars().take(100).collect::<String>());
                        
                        // Parse the SSE event
                        if let Some(response) = parse_sse_event(&event_data) {
                            events_in_this_chunk += 1;
                            debug!("Successfully parsed SSE event #{}, yielding response", events_in_this_chunk);
                            yield Ok(response);
                        } else {
                            debug!("Failed to parse SSE event or event was empty");
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
        
        // This handles cases where the final event doesn't have a trailing \n\n
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

/// Parse a single SSE event into GenerateContentResponse
/// The internal API wraps responses in {"response": {...}} envelope
fn parse_sse_event(event_data: &str) -> Option<GenerateContentResponse> {
    // SSE format: "event: <name>\ndata: <json>" or just "data: <json>"
    let lines: Vec<&str> = event_data.lines().collect();

    let mut data_line = None;
    for line in lines {
        if let Some(data) = line.strip_prefix("data:") {
            data_line = Some(data.trim());
            break;
        }
        // Also try without space after colon
        if let Some(data) = line.strip_prefix("data: ") {
            data_line = Some(data);
            break;
        }
    }

    let data = data_line?;
    
    // Skip empty data or "[DONE]" marker
    if data.is_empty() || data == "[DONE]" {
        debug!("Skipping empty or DONE marker");
        return None;
    }
   
    // Parse JSON directly into GenerateContentResponse
    // The internal API returns {"response": {...}} which matches our struct
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
}
