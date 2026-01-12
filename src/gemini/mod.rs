// Gemini API client module
// Author: kelexine (https://github.com/kelexine)

mod client;
pub mod streaming;

pub use client::GeminiClient;

use serde::{Deserialize, Serialize};

/// Request for project resolution (loadCodeAssist)
/// Based on Gemini CLI source: packages/core/src/code_assist/types.ts
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectResolutionRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cloudaicompanion_project: Option<String>,
    pub metadata: ClientMetadata,
}

/// Client metadata for API requests
#[derive(Debug, Serialize)]
pub struct ClientMetadata {
    #[serde(rename = "ideType", skip_serializing_if = "Option::is_none")]
    pub ide_type: Option<String>,
    #[serde(rename = "platform", skip_serializing_if = "Option::is_none")]
    pub platform: Option<String>,
    #[serde(rename = "pluginType", skip_serializing_if = "Option::is_none")]
    pub plugin_type: Option<String>,
    #[serde(rename = "duetProject", skip_serializing_if = "Option::is_none")]
    pub duet_project: Option<String>,
}

/// Response from project resolution
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectResolutionResponse {
    pub cloudaicompanion_project: Option<String>,
}

impl Default for ProjectResolutionRequest {
    fn default() -> Self {
        Self {
            cloudaicompanion_project: None,
            metadata: ClientMetadata {
                ide_type: Some("GEMINI_CLI".to_string()),
                platform: Some("PLATFORM_UNSPECIFIED".to_string()),
                plugin_type: Some("GEMINI".to_string()),
                duet_project: None,
            },
        }
    }
}
