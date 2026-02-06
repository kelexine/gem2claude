// Request translation (Anthropic → Gemini)
// Author: kelexine (https://github.com/kelexine)

use crate::error::{ProxyError, Result};
use crate::models::anthropic::{ContentBlock, Message, MessageContent, MessagesRequest};
use crate::models::gemini::{
    Content, GenerateContentRequest, GenerationConfig, Part as GeminiPart, SystemInstruction,
    ThinkingConfig as GeminiThinkingConfig,
};
use crate::models::mapping::map_model;
use crate::translation::tools::{translate_tool_result, translate_tool_use, translate_tools};
use tracing::debug;

use crate::translation::helpers::{build_system_instruction, detect_ultrathink};

/// Translate Anthropic MessagesRequest to Gemini GenerateContentRequest.
///
/// This is the core logical conversion used by the proxy:
/// 1. Maps model names (e.g., `claude` -> `gemini)
/// 2. Enforces Gemini token limits
/// 3. Converts message history format
/// 4. Extracts system prompts
/// 5. Translates tool definitions
/// 6. Configures generation parameters
pub async fn translate_request(
    mut anthropic_req: MessagesRequest,
    _project_id: &str,
    _cache_manager: Option<&crate::cache::CacheManager>,
    _gemini_client: Option<&crate::gemini::GeminiClient>,
) -> Result<GenerateContentRequest> {
    debug!("Translating request for model: {}", anthropic_req.model);

    // 0. Prefill check for 4.6 models (Breaking Change)
    if let Some(last_msg) = anthropic_req.messages.last() {
        if last_msg.role == "assistant" && anthropic_req.model.contains("4-6") {
            return Err(ProxyError::InvalidRequest(
                "Prefill (assistant message at end of conversation) is not supported for Claude 4.6 models.".to_string(),
            ));
        }
    }

    // 1. Detect Ultrathink keyword and enable extended thinking
    let has_ultrathink = detect_ultrathink(&anthropic_req);
    // Note: Ultrathink override might conflict with adaptive mode if both present?
    // We'll let explicit thinking config take precedence if strictly set, else ultrathink forces it.
    // For now, keep existing ultrathink logic but verify it works with new structs.
    if has_ultrathink && anthropic_req.thinking.is_none() {
        debug!("Ultrathink keyword detected - enabling highest thinking level");
        // Force highest thinking level when Ultrathink is present
        anthropic_req.thinking = Some(crate::models::anthropic::ThinkingConfig {
            type_: "enabled".to_string(),
            budget_tokens: Some(24_576),
        });
    }

    // 2. Map model name
    let gemini_model = match map_model(&anthropic_req.model) {
        Ok(m) => m,
        Err(_) => {
            // Fallback: use model name as-is if no mapping found (heuristic)
            anthropic_req.model.clone().into()
        }
    };

    // 3. Clamp max_tokens
    // Gemini 2.5 and 3.0 support larger outputs. 128k is a safe upper bound for modern models.
    let max_tokens_limit = 128_000;
    let max_tokens = anthropic_req.max_tokens.min(max_tokens_limit);
    if anthropic_req.max_tokens > max_tokens_limit {
        debug!(
            "Clamping max_tokens from {} to {} (Limit)",
            anthropic_req.max_tokens, max_tokens_limit
        );
    }

    // 4. Translate messages to contents
    let contents = translate_messages(anthropic_req.messages.clone())?;

    // 5. Translate system instruction
    let system_instruction = Some(SystemInstruction {
        parts: vec![GeminiPart::Text {
            text: build_system_instruction(anthropic_req.system.as_ref()),
            thought: None,
            thought_signature: None,
        }],
    });

    // 6. Translate thinking config
    let thinking_config = if let Some(thinking) = &anthropic_req.thinking {
        let is_adaptive = thinking.type_ == "adaptive";
        if thinking.type_ != "enabled" && !is_adaptive {
            // If neither enabled nor adaptive, skip thinking translation?
            // Or error? For now, we assume "disabled" implies None.
            None
        } else {
            // Determine effort level
            let effort = anthropic_req
                .output_config
                .as_ref()
                .and_then(|c| c.effort.as_deref())
                .unwrap_or("high");

            // Gemini 3.x models use thinking Level
            if gemini_model.contains("gemini-3") {
                let level = match effort {
                    "low" => "LOW",
                    "medium" => "MEDIUM",
                    "high" | "max" => "HIGH",
                    _ => "HIGH", // Default
                };
                Some(GeminiThinkingConfig {
                    include_thoughts: Some(true),
                    thinking_budget: None,
                    thinking_level: Some(level.to_string()),
                })
            } else {
                // Gemini 2.5 models use thinkingBudget
                // Map effort/budget to token count
                let budget = if let Some(b) = thinking.budget_tokens {
                    b // Use explicit budget if provided
                } else {
                    // Map effort to budget defaults for 2.5
                    match effort {
                        "low" => 5_000,
                        "medium" => 12_000,
                        "high" | "max" => 24_576,
                        _ => 24_576,
                    }
                };
                Some(GeminiThinkingConfig {
                    include_thoughts: Some(true),
                    thinking_budget: Some(budget),
                    thinking_level: None,
                })
            }
        }
    } else {
        None
    };

    // 7. Extract Output Format (JSON Schema)
    let (response_mime_type, response_schema) = if let Some(config) = &anthropic_req.output_config {
        if let Some(format) = &config.format {
            (Some("application/json".to_string()), Some(format.clone()))
        } else {
            (None, None)
        }
    } else {
        (None, None)
    };

    // 8. Build generation config
    let generation_config = Some(GenerationConfig {
        max_output_tokens: Some(max_tokens),
        temperature: anthropic_req.temperature,
        top_p: anthropic_req.top_p,
        top_k: anthropic_req.top_k,
        stop_sequences: anthropic_req.stop_sequences,
        candidate_count: None,
        thinking_config,
        response_mime_type,
        response_schema,
    });

    // 9. Translate tools
    let tools = anthropic_req
        .tools
        .as_ref()
        .map(|t| translate_tools(t.clone()));

    // 10. Set tool_config
    let tool_config = if tools.is_some() {
        Some(crate::models::gemini::ToolConfig {
            function_calling_config: crate::models::gemini::FunctionCallingConfig {
                mode: "AUTO".to_string(),
            },
        })
    } else {
        None
    };

    debug!(
        "Translated request: {} messages, system: {}, tools: {}, tool_config: {}, thinking: {}",
        contents.len(),
        system_instruction.is_some(),
        tools.is_some(),
        tool_config.is_some(),
        generation_config
            .as_ref()
            .and_then(|g| g.thinking_config.as_ref())
            .is_some()
    );

    Ok(GenerateContentRequest {
        contents,
        system_instruction,
        generation_config,
        tools,
        tool_config,
        cached_content: None,
    })
}

/// Translate messages array (Anthropic → Gemini).
///
/// Handles role mapping:
/// - `user` → `user`
/// - `assistant` → `model`
///
/// Also manages tool use tracking to properly associate `ToolResult`s with their calls.
fn translate_messages(messages: Vec<Message>) -> Result<Vec<Content>> {
    // Build map of tool_use_id → tool_name for FunctionResponse
    let mut tool_id_to_name = std::collections::HashMap::new();

    messages
        .into_iter()
        .map(|msg| {
            //  Map role: "assistant" → "model", "user" → "user"
            let role = match msg.role.as_str() {
                "user" => "user",
                "assistant" => "model",
                _ => {
                    return Err(ProxyError::InvalidRequest(format!(
                        "Invalid role: {}. Must be 'user' or 'assistant'.",
                        msg.role
                    )))
                }
            };

            // Translate content, building tool name map and using it
            let parts = translate_message_content(msg.content, &mut tool_id_to_name)?;

            Ok(Content {
                role: role.to_string(),
                parts,
            })
        })
        .collect()
}

/// Translate individual message content (Anthropic → Gemini).
///
/// Handles conversion of:
/// - Simple text
/// - Structured content blocks (text, images, tool results)
fn translate_message_content(
    content: MessageContent,
    tool_id_to_name: &mut std::collections::HashMap<String, String>,
) -> Result<Vec<GeminiPart>> {
    let parts = match content {
        MessageContent::Text(text) => vec![GeminiPart::Text {
            text,
            thought: None,
            thought_signature: None,
        }],
        MessageContent::Blocks(blocks) => blocks
            .into_iter()
            .map(|block| translate_content_block(block, tool_id_to_name))
            .collect::<Result<Vec<_>>>()?,
    };

    // Filter out empty text parts (from skipped thinking blocks)
    let mut filtered_parts: Vec<GeminiPart> = parts
        .into_iter()
        .filter(|part| !matches!(part, GeminiPart::Text { text, .. } if text.is_empty()))
        .collect();

    // Ensure we never return an empty parts list (causes HTTP 400 from Gemini API)
    // This happens if a message contained only Thinking blocks (which are filtered out)
    if filtered_parts.is_empty() {
        debug!("Message content became empty after filtering (likely only Thinking blocks). Adding placeholder.");
        filtered_parts.push(GeminiPart::Text {
            text: " ".to_string(),
            thought: None,
            thought_signature: None,
        });
    }

    Ok(filtered_parts)
}

/// Translate individual content block
fn translate_content_block(
    block: ContentBlock,
    tool_id_to_name: &mut std::collections::HashMap<String, String>,
) -> Result<GeminiPart> {
    match block {
        ContentBlock::Text { text, .. } => Ok(GeminiPart::Text {
            text,
            thought: None,
            thought_signature: None,
        }),

        // Skip thinking blocks - Claude's thinking is not sent to Gemini
        ContentBlock::Thinking { .. } => {
            // Return empty text to avoid breaking message structure
            Ok(GeminiPart::Text {
                text: String::new(),
                thought: None,
                thought_signature: None,
            })
        }

        ContentBlock::Image { .. } => {
            // Translate image block to Gemini InlineData
            let inline_data = crate::vision::translate_image_block(&block)?;
            Ok(GeminiPart::InlineData { inline_data })
        }

        ContentBlock::ToolUse {
            id, name, input, ..
        } => {
            debug!("Translating tool use: {}", name);
            // Track tool name for later FunctionResponse
            tool_id_to_name.insert(id.clone(), name.clone());
            Ok(translate_tool_use(id, name, input))
        }

        ContentBlock::ToolResult {
            tool_use_id,
            content,
            is_error,
        } => {
            debug!("Translating tool result for tool_use_id: {}", tool_use_id);
            // Look up the tool name from our map
            let tool_name = tool_id_to_name
                .get(&tool_use_id)
                .cloned()
                .unwrap_or_else(|| {
                    // Fallback if we somehow don't have the mapping
                    format!("unknown_tool_{}", tool_use_id)
                });
            translate_tool_result(tool_use_id, tool_name, content.to_string(), is_error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::anthropic::{ContentBlock, Message, MessageContent};

    #[test]
    fn test_simple_message_translation() {
        let messages = vec![Message {
            role: "user".to_string(),
            content: MessageContent::Text("Hello, world!".to_string()),
        }];

        let result = translate_messages(messages).unwrap();

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].role, "user");
        assert_eq!(result[0].parts.len(), 1);
    }

    #[test]
    fn test_role_mapping() {
        let user_msg = Message {
            role: "user".to_string(),
            content: MessageContent::Text("test".to_string()),
        };

        let assistant_msg = Message {
            role: "assistant".to_string(),
            content: MessageContent::Text("test".to_string()),
        };

        let user_result = translate_messages(vec![user_msg]).unwrap();
        let assistant_result = translate_messages(vec![assistant_msg]).unwrap();

        assert_eq!(user_result[0].role, "user");
        assert_eq!(assistant_result[0].role, "model");
    }

    #[test]
    fn test_invalid_role() {
        let invalid_msg = Message {
            role: "invalid".to_string(),
            content: MessageContent::Text("test".to_string()),
        };

        let result = translate_messages(vec![invalid_msg]);
        assert!(result.is_err());
    }

    #[test]
    fn test_multi_block_content() {
        let messages = vec![Message {
            role: "user".to_string(),
            content: MessageContent::Blocks(vec![
                ContentBlock::Text {
                    text: "First block".to_string(),
                    cache_control: None,
                },
                ContentBlock::Text {
                    text: "Second block".to_string(),
                    cache_control: None,
                },
            ]),
        }];

        let result = translate_messages(messages).unwrap();

        assert_eq!(result[0].parts.len(), 2);
    }
}
