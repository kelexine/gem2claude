//! Anthropic Messages API type definitions.
//!
//! This module defines the request and response structures for the [Anthropic Messages API](https://docs.anthropic.com/en/api/messages).
//! These types are used to deserialize incoming requests from Claude clients and serialize responses back to them.

// Author: kelexine (https://github.com/kelexine)

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Anthropic Messages API request
/// Anthropic Messages API request structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessagesRequest {
    /// The model that will complete your prompt.
    pub model: String,
    
    /// Input messages.
    pub messages: Vec<Message>,
    
    /// System prompt.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<SystemPrompt>,
    
    /// The maximum number of tokens to generate before stopping.
    pub max_tokens: u32,
    
    /// Amount of randomness injected into the response.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    
    /// Use nucleus sampling.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    
    /// Only sample from the top K options for each subsequent token.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_k: Option<u32>,
    
    /// Custom text sequences that will cause the model to stop generating.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_sequences: Option<Vec<String>>,
    
    /// Definitions of tools that the model may use.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<Tool>>,
    
    /// Configuration for "extended thinking" mode (Claude 3.7+).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<ThinkingConfig>,
    
    /// Whether to incrementally stream the response using server-sent events.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
}

/// System prompt can be either a simple string or structured blocks
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SystemPrompt {
    Text(String),
    Blocks(Vec<ContentBlock>),
}

impl SystemPrompt {
    /// Convert to string for Gemini API (which only accepts text)
    pub fn to_text(&self) -> String {
        match self {
            SystemPrompt::Text(s) => s.clone(),
            SystemPrompt::Blocks(blocks) => {
                // Concatenate all text blocks
                blocks
                    .iter()
                    .filter_map(|block| match block {
                        ContentBlock::Text { text, .. } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            }
        }
    }
}

/// A message in the conversation
/// A single message in the conversation history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    /// The role of the message sender ("user" or "assistant").
    pub role: String, 
    /// The content of the message.
    pub content: MessageContent,
}

/// Message content - can be simple text or structured blocks
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MessageContent {
    Text(String),
    Blocks(Vec<ContentBlock>),
}

/// Content block types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    /// A text content block.
    Text {
        text: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        cache_control: Option<CacheControl>,
    },
    /// Extended thinking block (Claude's thinking feature)
    Thinking {
        thinking: String,
    },
    /// An image content block.
    Image {
        source: ImageSource,
        #[serde(skip_serializing_if = "Option::is_none")]
        cache_control: Option<CacheControl>,
    },
    /// A tool use request from the model.
    ToolUse {
        id: String,
        name: String,
        input: Value,
        #[serde(skip_serializing_if = "Option::is_none")]
        cache_control: Option<CacheControl>,
    },
    /// Result of a tool execution.
    ToolResult {
        /// The ID of the tool use this result corresponds to.
        tool_use_id: String,
        content: ToolResultContent,
        #[serde(skip_serializing_if = "Option::is_none")]
        is_error: Option<bool>,
    },
}

/// Tool result content - can be simple text or structured blocks.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ToolResultContent {
    /// Simple text result.
    Text(String),
    /// Structured result with multiple blocks (e.g., text and images).
    Blocks(Vec<ContentBlock>),
}

impl std::fmt::Display for ToolResultContent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ToolResultContent::Text(s) => write!(f, "{}", s),
            ToolResultContent::Blocks(blocks) => {
                // Concatenate all text from blocks
                let text = blocks
                    .iter()
                    .filter_map(|block| match block {
                        ContentBlock::Text { text, .. } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                write!(f, "{}", text)
            }
        }
    }
}

/// Cache control configuration for prompt caching (Claude feature).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheControl {
    /// Type of cache (currently only "ephemeral" is supported).
    #[serde(rename = "type")]
    pub cache_type: String,
}

/// Image source for vision content
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ImageSource {
    #[serde(rename = "base64")]
    Base64 {
        #[serde(skip_serializing_if = "Option::is_none")]
        media_type: Option<String>,
        data: String,
    },
}

/// Image block convenience type
pub type ImageBlock = ContentBlock;

/// Tool definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tool {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub input_schema: Value, // JSON Schema
}

/// Anthropic Messages API response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessagesResponse {
    /// Unique object identifier.
    pub id: String,
    
    /// Object type (always "message").
    #[serde(rename = "type")]
    pub response_type: String,
    
    /// Conversational role of the generated message (always "assistant").
    pub role: String,
    
    /// Content generated by the model.
    pub content: Vec<ContentBlock>,
    
    /// The model that handled the request.
    pub model: String,
    
    /// The reason why the model stopped generating.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
    
    /// The sequence that caused the model to stop (if applicable).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_sequence: Option<String>,
    
    /// Billing and rate-limit usage.
    pub usage: Usage,
}

/// Token usage statistics.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Usage {
    /// The number of input tokens which were used.
    pub input_tokens: u32,
    
    /// The number of output tokens which were used.
    pub output_tokens: u32,
    
    /// The number of input tokens used to create the cache.
    #[serde(skip_serializing_if = "is_zero", default)]
    pub cache_creation_input_tokens: u32,
    
    /// The number of input tokens read from the cache.
    #[serde(skip_serializing_if = "is_zero", default)]
    pub cache_read_input_tokens: u32,
}

/// Helper function to skip serializing zero values
fn is_zero(val: &u32) -> bool {
    *val == 0
}

impl MessagesResponse {
    /// Create a new response with given content
    pub fn new(model: String, content: Vec<ContentBlock>, usage: Usage) -> Self {
        Self {
            id: format!("msg_{}", uuid::Uuid::new_v4().simple()),
            response_type: "message".to_string(),
            role: "assistant".to_string(),
            content,
            model,
            stop_reason: None,
            stop_sequence: None,
            usage,
        }
    }
}

/// Extended thinking configuration (Claude 3.7+).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThinkingConfig {
    /// Enable thinking (must be "enabled").
    #[serde(rename = "type")]
    pub type_: String,
    
    /// Maximum number of tokens allowed for thinking.
    pub budget_tokens: u32,
}
