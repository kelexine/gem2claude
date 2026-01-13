// Request translation (Anthropic → Gemini)
// Author: kelexine (https://github.com/kelexine)

use crate::error::{ProxyError, Result};
use crate::models::anthropic::{ContentBlock, Message, MessageContent, MessagesRequest};
use crate::models::gemini::{
    Content, GenerateContentRequest, GenerationConfig, Part as GeminiPart, SystemInstruction,
};
use crate::models::mapping::map_model;
use crate::translation::tools::{translate_tool_result, translate_tool_use, translate_tools};
use tracing::debug;

/// Translate Anthropic MessagesRequest to Gemini GenerateContentRequest
pub fn translate_request(
    anthropic_req: MessagesRequest,
    _project_id: &str, // Will be used when we add project-specific features
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
    let contents = translate_messages(anthropic_req.messages)?;

    // 3. Translate system instruction
    let system_instruction = anthropic_req.system.map(|sys| SystemInstruction {
        parts: vec![GeminiPart::Text {
            text: sys.to_text(),
        }],
    });

    // 4. Build generation config
    let generation_config = Some(GenerationConfig {
        max_output_tokens: Some(max_tokens),
        temperature: anthropic_req.temperature,
        top_p: anthropic_req.top_p,
    });

    // 5. Translate tools if present
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
            let parts = match msg.content {
                MessageContent::Text(text) => vec![GeminiPart::Text { text }],
                MessageContent::Blocks(blocks) => {
                    blocks
                        .into_iter()
                        .map(|block| translate_content_block(block, &mut tool_id_to_name))
                        .collect::<Result<Vec<_>>>()?
                }
            };

            Ok(Content { role: role.to_string(), parts })
        })
        .collect()
}

/// Translate individual content block
fn translate_content_block(
    block: ContentBlock,
    tool_id_to_name: &mut std::collections::HashMap<String, String>,
) -> Result<GeminiPart> {
    match block {
        ContentBlock::Text { text } => Ok(GeminiPart::Text { text }),

        ContentBlock::ToolUse { id, name, input } => {
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
            translate_tool_result(tool_use_id, tool_name, content, is_error)
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
                },
                ContentBlock::Text {
                    text: "Second block".to_string(),
                },
            ]),
        }];

        let result = translate_messages(messages).unwrap();

        assert_eq!(result[0].parts.len(), 2);
    }
}
