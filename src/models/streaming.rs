// Anthropic SSE streaming event types
// Author: kelexine (https://github.com/kelexine)

use super::anthropic::{ContentBlock, Usage};
use serde::{Deserialize, Serialize};

/// All possible Anthropic SSE event types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StreamEvent {
    MessageStart {
        message: MessageStart,
    },
    ContentBlockStart {
        index: i32,
        content_block: ContentBlockStart,
    },
    Ping,
    ContentBlockDelta {
        index: i32,
        delta: Delta,
    },
    ContentBlockStop {
        index: i32,
    },
    MessageDelta {
        delta: MessageDeltaData,
        usage: DeltaUsage,
    },
    MessageStop,
    Error {
        error: ErrorData,
    },
}

/// Message start event payload
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageStart {
    pub id: String,
    #[serde(rename = "type")]
    pub message_type: String, // "message"
    pub role: String, // "assistant"
    pub content: Vec<ContentBlock>,
    pub model: String,
    pub stop_reason: Option<String>,
    pub stop_sequence: Option<String>,
    pub usage: Usage,
}

/// Content block start event payload
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlockStart {
    Text { text: String },
    Thinking, // Extended thinking block
    ToolUse { id: String, name: String },
}

/// Delta types for content_block_delta events
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Delta {
    TextDelta { text: String },
    ThinkingDelta { thinking: String }, // Extended thinking delta
    SignatureDelta { signature: String }, // Thinking block signature
    InputJsonDelta { partial_json: String },
}

/// Message delta event payload
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageDeltaData {
    pub stop_reason: Option<String>,
    pub stop_sequence: Option<String>,
}

/// Usage delta for message_delta events
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeltaUsage {
    pub output_tokens: u32,
}

/// Error event payload
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorData {
    #[serde(rename = "type")]
    pub error_type: String,
    pub message: String,
}

impl StreamEvent {
    /// Format as Server-Sent Event
    pub fn to_sse(&self) -> String {
        let event_name = match self {
            StreamEvent::MessageStart { .. } => "message_start",
            StreamEvent::ContentBlockStart { .. } => "content_block_start",
            StreamEvent::Ping => "ping",
            StreamEvent::ContentBlockDelta { .. } => "content_block_delta",
            StreamEvent::ContentBlockStop { .. } => "content_block_stop",
            StreamEvent::MessageDelta { .. } => "message_delta",
            StreamEvent::MessageStop => "message_stop",
            StreamEvent::Error { .. } => "error",
        };

        let data = serde_json::to_string(self).unwrap_or_else(|_| "{}".to_string());

        format!("event: {}\ndata: {}\n\n", event_name, data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_start_sse_format() {
        let event = StreamEvent::MessageStart {
            message: MessageStart {
                id: "msg_123".to_string(),
                message_type: "message".to_string(),
                role: "assistant".to_string(),
                content: vec![],
                model: "claude-sonnet-4-5".to_string(),
                stop_reason: None,
                stop_sequence: None,
                usage: Usage {
                    input_tokens: 10,
                    output_tokens: 0,
                    cache_creation_input_tokens: 0,
                    cache_read_input_tokens: 0,
                },
            },
        };

        let sse = event.to_sse();
        assert!(sse.starts_with("event: message_start\n"));
        assert!(sse.contains("data: {"));
        assert!(sse.ends_with("\n\n"));
    }

    #[test]
    fn test_content_block_delta_sse_format() {
        let event = StreamEvent::ContentBlockDelta {
            index: 0,
            delta: Delta::TextDelta {
                text: "Hello".to_string(),
            },
        };

        let sse = event.to_sse();
        assert!(sse.starts_with("event: content_block_delta\n"));
        assert!(sse.contains("\"text\":\"Hello\""));
    }

    #[test]
    fn test_message_stop_sse_format() {
        let event = StreamEvent::MessageStop;
        let sse = event.to_sse();
        assert_eq!(
            sse,
            "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n"
        );
    }
}
