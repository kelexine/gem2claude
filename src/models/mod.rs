//! Data models for Anthropic and Gemini APIs.
//!
//! This module contains the type definitions for request/response bodies used by:
//! - The inbound Anthropic-compatible API (`anthropic`)
//! - The upstream Google Gemini API (`gemini`)
//! - Model name mapping utilities (`mapping`)
//! - Streaming event types (`streaming`)

// Author: kelexine (https://github.com/kelexine)

pub mod anthropic;
pub mod gemini;
pub mod mapping;
pub mod streaming;

pub use anthropic::{MessagesRequest, MessagesResponse, Message, MessageContent, ContentBlock, Tool, ThinkingConfig as AnthropicThinkingConfig};
pub use gemini::{GenerateContentRequest, GenerateContentResponse, Content, Part, ThinkingConfig as GeminiThinkingConfig};
pub use mapping::map_model;
pub use streaming::*;
