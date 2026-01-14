// Response translation (Gemini → Anthropic)
// Author: kelexine (https://github.com/kelexine)

use crate::error::{ProxyError, Result};
use crate::models::anthropic::{ContentBlock, MessagesResponse, Usage};
use crate::models::gemini::{GenerateContentResponse, Part as GeminiPart};
use regex::Regex;
use std::sync::OnceLock;
use tracing::{debug, warn};

/// Lazily initialized regex for thinking tags
static THINKING_REGEX: OnceLock<Regex> = OnceLock::new();

/// Get or initialize the thinking tag regex
fn get_thinking_regex() -> &'static Regex {
    THINKING_REGEX.get_or_init(|| {
        Regex::new(r"(?s)<think>.*?</think>").expect("Invalid regex pattern")
    })
}

/// Translate Gemini response to Anthropic format
pub fn translate_response(
    gemini_resp: GenerateContentResponse,
    model: &str,
) -> Result<MessagesResponse> {
    debug!("Translating Gemini response to Anthropic format");

    // 1. Unwrap response envelope (internal API wrapper)
    let wrapper = gemini_resp.response.ok_or_else(|| {
        ProxyError::Translation("Missing response wrapper from Gemini API".to_string())
    })?;

    // 2. Get first candidate
    let candidate = wrapper.candidates.into_iter().next().ok_or_else(|| {
        ProxyError::Translation("No candidates in Gemini response".to_string())
    })?;

    debug!("Response finish_reason: {:?}", candidate.finish_reason);

    // 3. Strip thinking artifacts from parts
    let cleaned_parts = strip_thinking_artifacts(candidate.content.parts)?;

    // 4. Translate to Anthropic content blocks
    let content = translate_parts(cleaned_parts)?;

    // 5. Map stop reason
    let _stop_reason = map_stop_reason(candidate.finish_reason.as_deref());

    // 6. Extract usage
    let usage = wrapper
        .usage_metadata
        .map(|u| Usage {
            input_tokens: u.prompt_token_count.unwrap_or(0),
            output_tokens: u.candidates_token_count.unwrap_or(0),
        })
        .unwrap_or_default();

    debug!(
        "Translated response: {} content blocks, usage: {:?}",
        content.len(),
        usage
    );

    Ok(MessagesResponse::new(model.to_string(), content, usage))
}

/// Strip <think>...</think> tags from Gemini 3.x responses
fn strip_thinking_artifacts(parts: Vec<GeminiPart>) -> Result<Vec<GeminiPart>> {
    parts
        .into_iter()
        .filter_map(|part| match part {
            GeminiPart::Text { text } => {
                // Remove <think>...</think> tags
                let cleaned = get_thinking_regex().replace_all(&text, "").to_string();

                // Only include if there's remaining content after stripping
                if cleaned.trim().is_empty() {
                    None
                } else {
                    Some(Ok(GeminiPart::Text { text: cleaned }))
                }
            }
            other => Some(Ok(other)),
        })
        .collect()
}

/// Translate Gemini parts to Anthropic content blocks
pub fn translate_parts(parts: Vec<GeminiPart>) -> Result<Vec<ContentBlock>> {
    parts.into_iter().map(translate_part).collect()
}

/// Translate individual part
fn translate_part(part: GeminiPart) -> Result<ContentBlock> {
    match part {
        GeminiPart::Text { text } => Ok(ContentBlock::Text { text }),

        GeminiPart::InlineData { inline_data } => {
            // Gemini can generate images (Imagen) - translate to Claude Image format
            use crate::models::anthropic::ImageSource;
            
            tracing::info!("Translating Gemini-generated image: {} ({} bytes)", 
                inline_data.mime_type, inline_data.data.len());
            
            Ok(ContentBlock::Image {
                source: ImageSource::Base64 {
                    media_type: Some(inline_data.mime_type.clone()),
                    data: inline_data.data.clone(),
                }
            })
        }

        GeminiPart::FunctionCall { function_call, .. } => {
            debug!("Translating function call: {}", function_call.name);
            Ok(ContentBlock::ToolUse {
                id: format!("toolu_{}", uuid::Uuid::new_v4().simple()),
                name: function_call.name,
                input: function_call.args,
            })
        }

        GeminiPart::FunctionResponse { function_response } => {
            // Function responses in Gemini are tool results in Anthropic
            // However, in Anthropic format, tool results come from the user, not the assistant
            // So this should not appear in assistant responses
            warn!(
                "Unexpected function response in model output: {}",
                function_response.name
            );
            Err(ProxyError::Translation(
                "Function response should not appear in assistant messages".to_string(),
            ))
        }
    }
}

/// Map Gemini finish reason to Anthropic stop reason
fn map_stop_reason(_finish_reason: Option<&str>) -> Option<String> {
    match _finish_reason {
        Some("STOP") => Some("end_turn".to_string()),
        Some("MAX_TOKENS") => Some("max_tokens".to_string()),
        Some("SAFETY") => Some("stop_sequence".to_string()),
        Some("RECITATION") => Some("stop_sequence".to_string()),
        Some("OTHER") => None,
        None => None,
        Some(other) => {
            warn!("Unknown finish reason: {}", other);
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::gemini::{
        Candidate, Content, FunctionCall, GenerateContentResponse, ResponseWrapper,
        UsageMetadata,
    };

    #[test]
    fn test_thinking_artifact_stripping() {
        let parts = vec![
            GeminiPart::Text {
                text: "<think>Internal thoughts</think>Hello!".to_string(),
            },
            GeminiPart::Text {
                text: "World".to_string(),
            },
        ];

        let cleaned = strip_thinking_artifacts(parts).unwrap();

        assert_eq!(cleaned.len(), 2);
        if let GeminiPart::Text { text } = &cleaned[0] {
            assert!(!text.contains("<think>"));
            assert!(text.contains("Hello!"));
        }
    }

    #[test]
    fn test_only_thinking_artifacts() {
        let parts = vec![GeminiPart::Text {
            text: "<think>Only thinking, no content</think>".to_string(),
        }];

        let cleaned = strip_thinking_artifacts(parts).unwrap();

        // Should be filtered out since only thinking remains
        assert_eq!(cleaned.len(), 0);
    }

    #[test]
    fn test_stop_reason_mapping() {
        assert_eq!(map_stop_reason(Some("STOP")), Some("end_turn".to_string()));
        assert_eq!(
            map_stop_reason(Some("MAX_TOKENS")),
            Some("max_tokens".to_string())
        );
        assert_eq!(map_stop_reason(None), None);
    }

    #[test]
    fn test_part_translation() {
        let text_part = GeminiPart::Text {
            text: "Hello".to_string(),
        };

        let result = translate_part(text_part).unwrap();

        if let ContentBlock::Text { text } = result {
            assert_eq!(text, "Hello");
        } else {
            panic!("Expected Text content block");
        }
    }

    #[test]
    fn test_function_call_translation() {
        let function_part = GeminiPart::FunctionCall {
            function_call: FunctionCall {
                name: "get_weather".to_string(),
                args: serde_json::json!({"city": "London"}),
            },
            thought_signature: None,
        };

        let result = translate_part(function_part).unwrap();

        if let ContentBlock::ToolUse { name, input, .. } = result {
            assert_eq!(name, "get_weather");
            assert_eq!(input.get("location").unwrap(), "NYC");
        } else {
            panic!("Expected ToolUse content block");
        }
    }
}
