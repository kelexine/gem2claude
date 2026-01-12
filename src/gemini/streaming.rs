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

/// Parse Gemini SSE stream
pub async fn stream_generate_content(
    client: &Client,
    url: String,
    request_body: String,
    oauth_manager: &OAuthManager,
) -> Result<Pin<Box<dyn Stream<Item = Result<GenerateContentResponse>> + Send>>> {
    let access_token = oauth_manager.get_token().await?;

    debug!("Starting Gemini SSE stream to: {}", url);

    // Make streaming request
    let response = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", access_token))
        .header("Content-Type",  "application/json")
        .header("Accept", "text/event-stream")
        .body(request_body)
        .send()
        .await?;

    let status = response.status();
    if !status.is_success() {
        let error_text = response.text().await.unwrap_or_default();
        return Err(ProxyError::GeminiApi(format!(
            "HTTP {}: {}",
            status, error_text
        )));
    }

    // Convert response to byte stream
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
                    buffer.push_str(&chunk_str);

                    // Process complete events (ends with \n\n)
                    while let Some(event_end) = buffer.find("\n\n") {
                        let event_data = buffer[..event_end].to_string();
                        buffer = buffer[event_end + 2..].to_string();

                        // Parse the SSE event
                        if let Some(response) = parse_sse_event(&event_data) {
                            yield Ok(response);
                        }
                    }
                }
                Err(e) => {
                    warn!("Stream error: {}", e);
                    yield Err(ProxyError::Http(e));
                    break;
                }
            }
        }
    }
}

/// Parse a single SSE event into GenerateContentResponse
fn parse_sse_event(event_data: &str) -> Option<GenerateContentResponse> {
    // SSE format: "event: <name>\ndata: <json>"
    let lines: Vec<&str> = event_data.lines().collect();

    let mut data_line = None;
    for line in lines {
        if line.starts_with("data: ") {
            data_line = Some(&line[6..]); // Skip "data: "
            break;
        }
    }

    let data = data_line?;
   
    // Parse JSON
    match serde_json::from_str::<GenerateContentResponse>(data) {
        Ok(response) => {
            debug!("Parsed SSE event successfully");
            Some(response)
        }
        Err(e) => {
            warn!("Failed to parse SSE event: {}", e);
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
