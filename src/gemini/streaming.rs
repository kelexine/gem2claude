// Gemini API streaming client
// Author: kelexine (https://github.com/kelexine)

use crate::error::{ProxyError, Result};
use crate::models::gemini::GenerateContentResponse;
use crate::oauth::OAuthManager;
use futures::stream::Stream;
use reqwest::Client;
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

    // Per Claude API docs: streaming errors should be returned immediately, not silently retried
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

    let status = response.status();
    if !status.is_success() {
        let error_text = response.text().await.unwrap_or_default();
        // Match Claude API error types
        return Err(match status.as_u16() {
            429 => ProxyError::TooManyRequests(error_text),
            529 => ProxyError::Overloaded(error_text),
            503 | 504 => ProxyError::ServiceUnavailable(error_text),
            _ => ProxyError::GeminiApi(format!("HTTP {}: {}", status, error_text)),
        });
    }

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

                    // Parse all complete SSE events in the buffer
                    // Robustly handle both standard LF (\n\n) and Gemini's CRLF (\r\n\r\n) delimiters
                    let mut events_in_this_chunk = 0;
                    loop {
                        // Find the earliest delimiter
                        let lf_pos = buffer.find("\n\n");
                        let crlf_pos = buffer.find("\r\n\r\n");

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
                            (None, None) => break,
                        };

                        let event_data = buffer[..event_end].to_string();
                        buffer = buffer[event_end + delim_len..].to_string();

                        debug!("Found complete SSE event (len: {})", event_data.len());

                        // Parse the SSE event
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

    #[tokio::test]
    async fn test_parse_sse_stream_mixed_delimiters() {
        use futures::StreamExt;

        // Create a stream with mixed delimiters: \n\n (LF) and \r\n\r\n (CRLF)
        let event1 = r#"data: {"response":{"candidates":[{"content":{"role":"model","parts":[{"text":"First"}]}}]}}"#;
        let event2 = r#"data: {"response":{"candidates":[{"content":{"role":"model","parts":[{"text":"Second"}]}}]}}"#;
        let event3 = r#"data: {"response":{"candidates":[{"content":{"role":"model","parts":[{"text":"Third"}]}}]}}"#;

        // LF, CRLF, LF
        let payload = format!("{}\n\n{}\r\n\r\n{}\n\n", event1, event2, event3);

        let stream = futures::stream::iter(vec![
            Ok(bytes::Bytes::from(payload))
        ]);

        let parsed_stream = parse_sse_stream(stream);
        futures::pin_mut!(parsed_stream);
        let mut events = Vec::new();

        while let Some(result) = parsed_stream.next().await {
            events.push(result.unwrap());
        }

        assert_eq!(events.len(), 3);
        assert_eq!(events[0].response.as_ref().unwrap().candidates[0].content.parts[0].as_text().unwrap(), "First");
        assert_eq!(events[1].response.as_ref().unwrap().candidates[0].content.parts[0].as_text().unwrap(), "Second");
        assert_eq!(events[2].response.as_ref().unwrap().candidates[0].content.parts[0].as_text().unwrap(), "Third");
    }
}
