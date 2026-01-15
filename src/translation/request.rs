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

/// Translate Anthropic MessagesRequest to Gemini GenerateContentRequest
pub async fn translate_request(
    anthropic_req: MessagesRequest,
    _project_id: &str, // Will be used when we add project-specific features
    _cache_manager: Option<&crate::cache::CacheManager>,
    _gemini_client: Option<&crate::gemini::GeminiClient>,
) -> Result<GenerateContentRequest> {
    debug!(
        "Translating request for model: {}",
        anthropic_req.model
    );

    // 1. Map model name
    let _gemini_model = map_model(&anthropic_req.model)?;

    // 2. Clamp max_tokens to Gemini's limit (1-65536)
    let max_tokens = anthropic_req.max_tokens.min(65536);
    if anthropic_req.max_tokens > 65536 {
        debug!(
            "Clamping max_tokens from {} to 65536 (Gemini's limit)",
            anthropic_req.max_tokens
        );
    }

    // 3. Translate messages to contents
    let contents = translate_messages(anthropic_req.messages.clone())?;

    // 3. Translate system instruction and inject image generation limitation
    let system_instruction = {
        let mut parts = vec![];
        
        // Add original system instructions if present
        if let Some(ref sys) = anthropic_req.system {
            parts.push(GeminiPart::Text {
                text: sys.to_text(),
                thought: None,
                thought_signature: None,
            });
        }
        
        // Inject no image generation to system instruction
        parts.push(GeminiPart::Text {
            text: "\n\nIMPORTANT: You do not have the ability to generate, create, or produce images. If the user asks you to generate, create, draw, or produce an image, politely inform them that you cannot generate images and can only analyze existing images that are provided to you.".to_string(),
            thought: None,
            thought_signature: None,
        });
        
        Some(SystemInstruction { parts })
    };
    // 4. Translate thinking config if present
    let thinking_config = anthropic_req.thinking.as_ref().and_then(|thinking| {
        if thinking.type_ != "enabled" {
            return None;
        }
        
        // Get the mapped Gemini model name
        // map_model returns Result<String, ProxyError>, unwrap_or falls back to original model
        let gemini_model = crate::models::mapping::map_model(&anthropic_req.model)
            .unwrap_or_else(|_| anthropic_req.model.clone());
        
        // Gemini 3.x models use thinking Level enum
        if gemini_model.contains("gemini-3") {
            let level = match thinking.budget_tokens {
                0..=10_000 => "LOW",
                10_001..=20_000 => "MEDIUM",
                _ => "HIGH",
            };
            Some(GeminiThinkingConfig {
                include_thoughts: Some(true),
                thinking_budget: None,
                thinking_level: Some(level.to_string()),
            })
        } else {
            // Gemini 2.5 models use thinkingBudget (token count)
            Some(GeminiThinkingConfig {
                include_thoughts: Some(true),
                thinking_budget: Some(thinking.budget_tokens),
                thinking_level: None,
            })
        }
    });

    // 5. Build generation config
    let generation_config = Some(GenerationConfig {
        max_output_tokens: Some(max_tokens),
        temperature: anthropic_req.temperature,
        top_p: anthropic_req.top_p,
        thinking_config,
    });

    // 6. Translate tools if present
    let tools = anthropic_req.tools.as_ref().map(|t| translate_tools(t.clone()));
    
    // 6. Set tool_config when tools are present (tells Gemini to wait for function responses)
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
        "Translated request: {} messages, system: {}, tools: {}, tool_config: {}",
        contents.len(),
        system_instruction.is_some(),
        tools.is_some(),
        tool_config.is_some()
    );

    Ok(GenerateContentRequest {
        contents,
        system_instruction,
        generation_config,
        tools,
        tool_config,
        cached_content: None,  // NEW: Will be populated by cache manager
    })
}

/// Translate messages array (Anthropic → Gemini)
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

            Ok(Content { role: role.to_string(), parts })
        })
        .collect()
}

/// Translate individual message content (Anthropic → Gemini)
fn translate_message_content(
    content: MessageContent,
    tool_id_to_name: &mut std::collections::HashMap<String, String>,
) -> Result<Vec<GeminiPart>> {
    let parts = match content {
        MessageContent::Text(text) => vec![GeminiPart::Text { text, thought: None, thought_signature: None }],
        MessageContent::Blocks(blocks) => blocks
            .into_iter()
            .map(|block| translate_content_block(block, tool_id_to_name))
            .collect::<Result<Vec<_>>>()?,
    };
    
    // Filter out empty text parts (from skipped thinking blocks)
    let mut filtered_parts: Vec<GeminiPart> = parts
        .into_iter()
        .filter(|part| {
            !matches!(part, GeminiPart::Text { text, .. } if text.is_empty())
        })
        .collect();

    // Ensure we never return an empty parts list (causes HTTP 400 from Gemini API)
    // This happens if a message contained only Thinking blocks (which are filtered out)
    if filtered_parts.is_empty() {
        debug!("Message content became empty after filtering (likely only Thinking blocks). Adding placeholder.");
        filtered_parts.push(GeminiPart::Text {
            text: " ".to_string(),
            thought: None,
            thought_signature: None
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
        ContentBlock::Text { text, .. } => Ok(GeminiPart::Text { text, thought: None, thought_signature: None }),

        // Skip thinking blocks - Claude's thinking is not sent to Gemini
        ContentBlock::Thinking { .. } => {
            // Return empty text to avoid breaking message structure
            Ok(GeminiPart::Text { text: String::new(), thought: None, thought_signature: None })
        }

        ContentBlock::Image { .. } => {
            // Translate image block to Gemini InlineData
            let inline_data = crate::vision::translate_image_block(&block)?;
            Ok(GeminiPart::InlineData { inline_data })
        }

        ContentBlock::ToolUse { id, name, input, .. } => {
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
