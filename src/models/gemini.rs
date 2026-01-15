// Gemini internal API type definitions
// Author: kelexine (https://github.com/kelexine)
// Based on reverse engineering of cloudcode-pa.googleapis.com/v1internal

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Internal API request wrapper.
///
/// The internal API requires this specific structure with model, project, and user_prompt_id,
/// wrapping the actual `GenerateContentRequest`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InternalApiRequest {
    /// Target Gemini model name (e.g., "gemini-pro").
    pub model: String,
    
    /// Google Cloud project ID (resolved from credentials).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    
    /// User prompt identifier (for internal tracking).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_prompt_id: Option<String>,
    
    /// The actual content generation request.
    pub request: GenerateContentRequest,
}

/// Gemini generate content request (internal API format).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateContentRequest {
    /// Conversation history.
    pub contents: Vec<Content>,
    
    /// System instructions (context).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_instruction: Option<SystemInstruction>,
    
    /// Generation parameters (temperature, max tokens, etc.).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generation_config: Option<GenerationConfig>,
    
    /// Tool definitions.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<ToolDeclaration>>,
    
    /// Tool usage configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_config: Option<ToolConfig>,
    
    /// Reference to cached content.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cached_content: Option<String>,
}

/// Content in a turn (user or model)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Content {
    #[serde(default = "default_role")]
    pub role: String, // "user" or "model"
    #[serde(default)]
    pub parts: Vec<Part>,
}

fn default_role() -> String {
    "model".to_string()
}

/// Individual part of content in a Gemini request/response.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Part {
    /// Text content part.
    Text {
        /// The text string.
        text: String,
        
        /// Flag indicating this is thinking content (Gemini 2.5/3.x).
        #[serde(skip_serializing_if = "Option::is_none")]
        thought: Option<bool>,
        
        /// Metadata hash for API validation.
        #[serde(rename = "thoughtSignature", skip_serializing_if = "Option::is_none")]
        thought_signature: Option<String>,
    },
    
    /// Extended thinking/reasoning from Gemini.
    Thought {
        /// Actual thinking text - translate to Claude's thinking blocks.
        thought: String,
        
        /// Metadata hash for API validation (optional).
        #[serde(rename = "thoughtSignature", skip_serializing_if = "Option::is_none")]
        thought_signature: Option<String>,
    },
    
    /// Inline data (images, etc).
    InlineData {
        #[serde(rename = "inlineData")]
        inline_data: InlineData,
    },
    
    /// Model requesting to call a function.
    FunctionCall {
        #[serde(rename = "functionCall")]
        function_call: FunctionCall,
        
        /// Required by Gemini 3 models for function calls in conversation history.
        #[serde(rename = "thoughtSignature", skip_serializing_if = "Option::is_none")]
        thought_signature: Option<String>,
    },
    
    /// Result of a function call.
    FunctionResponse {
        #[serde(rename = "functionResponse")]
        function_response: FunctionResponse,
    },
}

impl Part {
    /// Get text content if this is a Text or Thought part
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Part::Text { text, .. } => Some(text),
            Part::Thought { thought, .. } => Some(thought),
            _ => None,
        }
    }
}

/// Inline image data for vision
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InlineData {
    #[serde(rename = "mimeType")]
    pub mime_type: String,
    pub data: String, // base64 encoded
}

/// System instruction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemInstruction {
    pub parts: Vec<Part>,
}

/// Function call from model
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCall {
    pub name: String,
    pub args: Value,
}

/// Function response from user
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionResponse {
    pub name: String,
    pub response: Value,
}

/// Generation configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerationConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_k: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_sequences: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub candidate_count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "thinkingConfig")]
    pub thinking_config: Option<ThinkingConfig>,
}

/// Extended thinking configuration for Gemini models.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThinkingConfig {
    /// Whether to include thinking in the output.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include_thoughts: Option<bool>,
    
    /// Token budget for thinking (Gemini 2.5).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking_budget: Option<u32>,
    
    /// Thinking level for Gemini 3.x: "LOW", "MEDIUM", "HIGH".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking_level: Option<String>,
}

/// Tool declaration (must use camelCase for internal API).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolDeclaration {
    /// List of function signatures available to the model.
    pub function_declarations: Vec<FunctionDeclaration>,
}

/// Function declaration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FunctionDeclaration {
    pub name: String,
    pub description: String,
    #[serde(rename = "parametersJsonSchema")]
    pub parameters_json_schema: Value, // JSON Schema (sanitized)
}

/// Tool configuration for function calling behavior
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolConfig {
    pub function_calling_config: FunctionCallingConfig,
}

/// Function calling configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FunctionCallingConfig {
    /// Mode: "AUTO", "ANY", or "NONE".
    pub mode: String,
}

/// Gemini response (with internal API wrapper).
#[derive(Debug, Clone, Deserialize)]
pub struct GenerateContentResponse {
    /// Internal API wraps response in this envelope.
    pub response: Option<ResponseWrapper>,
}

/// Response wrapper from internal API
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResponseWrapper {
    pub candidates: Vec<Candidate>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage_metadata: Option<UsageMetadata>,
}

/// Response candidate
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    pub content: Content,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub safety_ratings: Option<Vec<Value>>,
}

/// Token usage metadata.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageMetadata {
    /// Tokens in the input prompt.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_token_count: Option<u32>,
    
    /// Tokens in the generated response.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub candidates_token_count: Option<u32>,
    
    /// Total tokens (prompt + candidates).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_token_count: Option<u32>,
    
    /// Number of tokens read from the cache.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cached_content_token_count: Option<u32>, 
}
