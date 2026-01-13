// SSE event translation for streaming responses
// Author: kelexine (https://github.com/kelexine)

use crate::error::Result;
use crate::models::gemini::GenerateContentResponse;
use crate::models::streaming::*;
use tracing::debug;

/// Translates Gemini streaming chunks into Anthropic SSE events
/// Includes stateful thinking tag stripping to handle tags split across chunks
pub struct StreamTranslator {
    message_id: String,
    model: String,
    input_tokens: u32,
    output_tokens: u32,
    first_chunk: bool,
    block_started: bool,
    accumulated_text: String,
    // Stateful thinking stripper fields
    thinking_buffer: String,
    in_thinking: bool,
}

impl StreamTranslator {
    pub fn new(model: String) -> Self {
        Self {
            message_id: format!("msg_{}", uuid::Uuid::new_v4().simple()),
            model,
            input_tokens: 0,
            output_tokens: 0,
            first_chunk: true,
            block_started: false,
            accumulated_text: String::new(),
            thinking_buffer: String::new(),
            in_thinking: false,
        }
    }

    /// Process text chunk with stateful thinking tag stripping
    /// Handles tags that may be split across multiple chunks
    fn process_text_chunk(&mut self, text: &str) -> String {
        let mut output = String::new();
        let mut full_text = self.thinking_buffer.clone() + text;
        self.thinking_buffer.clear();

        // State machine for thinking tag removal
        loop {
            if self.in_thinking {
                // We're inside a <think> block, look for closing tag
                match full_text.find("</think>") {
                    Some(idx) => {
                        // Found closing tag, skip past it
                        self.in_thinking = false;
                        full_text = full_text[idx + 8..].to_string();
                        // Continue loop to process remainder
                    }
                    None => {
                        // No closing tag yet, might be partial at end
                        // Check for partial closing tag
                        if let Some(partial_idx) = Self::find_partial_tag(&full_text, "</think>") {
                            self.thinking_buffer = full_text[partial_idx..].to_string();
                        }
                        // Discard rest - it's thinking content
                        break;
                    }
                }
            } else {
                // We're outside thinking block, look for opening tag
                match full_text.find("<think>") {
                    Some(idx) => {
                        // Found opening tag, emit text before it
                        output.push_str(&full_text[..idx]);
                        self.in_thinking = true;
                        full_text = full_text[idx + 7..].to_string();
                        // Continue loop to find closing tag
                    }
                    None => {
                        // No opening tag, check for partial at end
                        if let Some(partial_idx) = Self::find_partial_tag(&full_text, "<think>") {
                            // Move partial tag to buffer for next chunk
                            output.push_str(&full_text[..partial_idx]);
                            self.thinking_buffer = full_text[partial_idx..].to_string();
                        } else {
                            // Safe to emit all
                            output.push_str(&full_text);
                        }
                        break;
                    }
                }
            }
        }
        output
    }

    /// Find partial tag match at end of string
    /// Returns index where partial match starts, or None
    fn find_partial_tag(text: &str, tag: &str) -> Option<usize> {
        // Check if text ends with any prefix of the tag
        for i in 1..tag.len() {
            let prefix = &tag[..i];
            if text.ends_with(prefix) {
                return Some(text.len() - prefix.len());
            }
        }
        None
    }

    /// Translate a Gemini chunk into Anthropic SSE events
    pub fn translate_chunk(
        &mut self,
        gemini_chunk: GenerateContentResponse,
    ) -> Result<Vec<StreamEvent>> {
        let mut events = Vec::new();

        // 1. On first chunk, send message_start
        if self.first_chunk {
            // Extract usage from first chunk
            if let Some(wrapper) = &gemini_chunk.response {
                if let Some(usage) = &wrapper.usage_metadata {
                    self.input_tokens = usage.prompt_token_count.unwrap_or(0);
                    self.output_tokens = usage.candidates_token_count.unwrap_or(0);
                }
            }

            events.push(StreamEvent::MessageStart {
                message: MessageStart {
                    id: self.message_id.clone(),
                    message_type: "message".to_string(),
                    role: "assistant".to_string(),
                    content: vec![],
                    model: self.model.clone(),
                    stop_reason: None,
                    stop_sequence: None,
                    usage: crate::models::anthropic::Usage {
                        input_tokens: self.input_tokens,
                        output_tokens: 0,
                    },
                },
            });

            self.first_chunk = false;
        }

        // 2. Extract content from response
        if let Some(wrapper) = gemini_chunk.response {
            if let Some(candidate) = wrapper.candidates.into_iter().next() {
                // Process all parts (text and function calls)
                for part in candidate.content.parts {
                    match part {
                        crate::models::gemini::Part::Text { text } => {
                            // Strip thinking artifacts using stateful processor
                            let clean_text = self.process_text_chunk(&text);
                            
                            if !clean_text.is_empty() {
                                // 3. On first text, send content_block_start
                                if !self.block_started {
                                    events.push(StreamEvent::ContentBlockStart {
                                        index: 0,
                                        content_block: ContentBlockStart::Text {
                                            text: String::new(),
                                        },
                                    });
                                    self.block_started = true;
                                }

                                // 4. Calculate delta
                                let delta = if clean_text.starts_with(&self.accumulated_text) {
                                    clean_text[self.accumulated_text.len()..].to_string()
                                } else {
                                    // Full text if not incremental
                                    clean_text.clone()
                                };

                                if !delta.is_empty() {
                                    events.push(StreamEvent::ContentBlockDelta {
                                        index: 0,
                                        delta: Delta::TextDelta { text: delta },
                                    });
                                }

                                self.accumulated_text = clean_text;
                            }
                        }
                        crate::models::gemini::Part::FunctionCall { function_call, .. } => {
                            // Generate tool use ID
                            let tool_id = format!("toolu_{}", uuid::Uuid::new_v4().simple());
                            
                            // Send content_block_start for tool_use
                            events.push(StreamEvent::ContentBlockStart {
                                index: 0,
                                content_block: ContentBlockStart::ToolUse {
                                    id: tool_id.clone(),
                                    name: function_call.name.clone(),
                                },
                            });
                            self.block_started = true;
                            
                            // Convert args to JSON string and send as input_json_delta
                            let args_json = serde_json::to_string(&function_call.args)
                                .unwrap_or_else(|_| "{}".to_string());
                            
                            debug!("Translated function call: {} with args: {}", function_call.name, args_json);
                            
                            events.push(StreamEvent::ContentBlockDelta {
                                index: 0,
                                delta: Delta::InputJsonDelta {
                                    partial_json: args_json,
                                },
                            });
                        }
                        crate::models::gemini::Part::FunctionResponse { .. } => {
                            // Function responses are in user messages, not assistant messages
                            // Skip for now
                        }
                    }
                }

                // Check if this is the final chunk
                if let Some(finish_reason) = candidate.finish_reason {
                    debug!("Stream finished with reason: {}", finish_reason);

                    // Update output tokens from usage
                    if let Some(usage) = wrapper.usage_metadata {
                        self.output_tokens = usage.candidates_token_count.unwrap_or(0);
                    }

                    // 5. Send content_block_stop
                    if self.block_started {
                        events.push(StreamEvent::ContentBlockStop { index: 0 });
                    }

                    // 6. Send message_delta with stop_reason and usage
                    let stop_reason = match finish_reason.as_str() {
                        "STOP" => Some("end_turn".to_string()),
                        "MAX_TOKENS" => Some("max_tokens".to_string()),
                        _ => None,
                    };

                    events.push(StreamEvent::MessageDelta {
                        delta: MessageDeltaData {
                            stop_reason,
                            stop_sequence: None,
                        },
                        usage: DeltaUsage {
                            output_tokens: self.output_tokens,
                        },
                    });

                    // 7. Send message_stop
                    events.push(StreamEvent::MessageStop);
                }
            }
        }

        Ok(events)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_thinking_stripper_simple() {
        let mut translator = StreamTranslator::new("test".to_string());
        
        // Simple case: complete tag in one chunk
        let result = translator.process_text_chunk("Hello <think>internal</think> world");
        assert_eq!(result, "Hello  world");
    }

    #[test]
    fn test_thinking_stripper_split_open() {
        let mut translator = StreamTranslator::new("test".to_string());
        
        // Tag split across chunks: open tag
        let result1 = translator.process_text_chunk("Hello <thi");
        assert_eq!(result1, "Hello ");
        
        let result2 = translator.process_text_chunk("nk>secret</think> world");
        assert_eq!(result2, " world");
    }

    #[test]
    fn test_thinking_stripper_split_close() {
        let mut translator = StreamTranslator::new("test".to_string());
        
        // Already in thinking, close tag split
        let result1 = translator.process_text_chunk("<think>secret</thi");
        assert_eq!(result1, "");
        
        let result2 = translator.process_text_chunk("nk> visible");
        assert_eq!(result2, " visible");
    }

    #[test]
    fn test_thinking_stripper_nested() {
        let mut translator = StreamTranslator::new("test".to_string());
        
        // Multiple think blocks
        let result = translator.process_text_chunk("A<think>x</think>B<think>y</think>C");
        assert_eq!(result, "ABC");
    }

    #[test]
    fn test_partial_tag_detection() {
        assert_eq!(StreamTranslator::find_partial_tag("hello<", "<think>"), Some(5));
        assert_eq!(StreamTranslator::find_partial_tag("hello<t", "<think>"), Some(5));
        assert_eq!(StreamTranslator::find_partial_tag("hello<thi", "<think>"), Some(5));
        assert_eq!(StreamTranslator::find_partial_tag("hello", "<think>"), None);
    }

    #[test]
    fn test_translator_first_chunk() {
        let mut translator = StreamTranslator::new("claude-sonnet-4".to_string());
        
        // First chunk should generate message_start
        let chunk = GenerateContentResponse {
            response: Some(crate::models::gemini::ResponseWrapper {
                candidates: vec![],
                usage_metadata: Some(crate::models::gemini::UsageMetadata {
                    prompt_token_count: Some(10),
                    candidates_token_count: Some(0),
                    total_token_count: Some(10),
                }),
                prompt_feedback: None,
                model_version: None,
            }),
        };

        let events = translator.translate_chunk(chunk).unwrap();
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], StreamEvent::MessageStart { .. }));
    }
}
