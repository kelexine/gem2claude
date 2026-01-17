// Gemini cached content models for cache creation API
// Author: kelexine (https://github.com/kelexine)

use crate::models::gemini::{Content, SystemInstruction};
use serde::{Deserialize, Serialize};

/// Request to create a cached content entry
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateCachedContentRequest {
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_instruction: Option<SystemInstruction>,
    pub contents: Vec<Content>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ttl: Option<String>, // e.g., "300s" for 5 minutes
}

/// Response from cache creation
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CachedContentResponse {
    pub name: String, // e.g., "cachedContents/abc123"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub create_time: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expire_time: Option<String>,
}
