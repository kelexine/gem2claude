// SSE event translation for streaming responses
// Author: kelexine (https://github.com/kelexine)

use crate::error::Result;
use crate::models::gemini::GenerateContentResponse;
use crate::models::streaming::*;
use tracing::debug;

/// Translates Gemini streaming chunks into Anthropic SSE events
/// Includes stateful thinking tag stripping to handle tags split across chunks
#[derive(Debug, PartialEq, Clone, Copy)]
enum BlockType {
    Text,
    Thinking,
    ToolUse,
}

/// Translates Gemini streaming chunks into Anthropic SSE events
/// Includes stateful thinking tag stripping to handle tags split across chunks
pub struct StreamTranslator {
    message_id: String,
    model: String,
    input_tokens: u32,
    output_tokens: u32,
    first_chunk: bool,
    
    // Block state tracking
    current_block_index: i32,
    current_block_type: Option<BlockType>,
    had_tool_use: bool,
    
    accumulated_text: String,
    // Stateful thinking stripper fields
    thinking_buffer: String,
    in_thinking: bool,
    accumulated_thinking: String,
}

impl StreamTranslator {
    pub fn new(model: String) -> Self {
        Self {
            message_id: format!("msg_{}", uuid::Uuid::new_v4().simple()),
            model,
            input_tokens: 0,
            output_tokens: 0,
            first_chunk: true,
            
            current_block_index: 0,
            current_block_type: None,
            had_tool_use: false,
            
            accumulated_text: String::new(),
            thinking_buffer: String::new(),
            in_thinking: false,
            accumulated_thinking: String::new(),
        }
    }

    /// Process text chunk with stateful thinking tag stripping
    /// Handles tags that may be split across multiple chunks
    /// Returns a vector of (BlockType, String) tuples representing segments
    fn process_text_chunk(&mut self, text: &str) -> Vec<(BlockType, String)> {
        let mut segments = Vec::new();
        let mut full_text = self.thinking_buffer.clone() + text;
        self.thinking_buffer.clear();

        // Safety limit for buffer
        if full_text.len() > 10 * 1024 * 1024 {
            // Buffer too large, flush it as is to prevent OOM
            if self.in_thinking {
                 segments.push((BlockType::Thinking, full_text));
            } else {
                 segments.push((BlockType::Text, full_text));
            }
            return segments;
        }

        loop {
            if self.in_thinking {
                // We are inside a <think> block
                match full_text.find("</think>") {
                    Some(idx) => {
                        // Found closing tag
                        let content = full_text[..idx].to_string();
                        if !content.is_empty() {
                            segments.push((BlockType::Thinking, content));
                        }
                        
                        self.in_thinking = false;
                        full_text = full_text[idx + 8..].to_string();
                        // Continue loop to process remainder
                    }
                    None => {
                        // No closing tag yet
                        // Check for partial closing tag at the end
                        if let Some(partial_idx) = Self::find_partial_tag(&full_text, "</think>") {
                            let content = full_text[..partial_idx].to_string();
                            if !content.is_empty() {
                                segments.push((BlockType::Thinking, content));
                            }
                            self.thinking_buffer = full_text[partial_idx..].to_string();
                            break;
                        } else {
                            // All content is thinking
                            if !full_text.is_empty() {
                                segments.push((BlockType::Thinking, full_text));
                            }
                            break;
                        }
                    }
                }
            } else {
                // We are outside a thinking block (Text)
                match full_text.find("<think>") {
                    Some(idx) => {
                        // Found opening tag
                        let content = full_text[..idx].to_string();
                        if !content.is_empty() {
                            segments.push((BlockType::Text, content));
                        }
                        
                        self.in_thinking = true;
                        full_text = full_text[idx + 7..].to_string(); // Skip <think>
                    }
                    None => {
                        // No opening tag, check for partial at end
                        if let Some(partial_idx) = Self::find_partial_tag(&full_text, "<think>") {
                            let content = full_text[..partial_idx].to_string();
                            if !content.is_empty() {
                                segments.push((BlockType::Text, content));
                            }
                            self.thinking_buffer = full_text[partial_idx..].to_string();
                            break;
                        } else {
                            // All content is text
                            if !full_text.is_empty() {
                                segments.push((BlockType::Text, full_text));
                            }
                            break;
                        }
                    }
                }
            }
        }
        
        segments
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
                        crate::models::gemini::Part::Text { text, thought, thought_signature } => {
                            // Check if this is thinking content (thought: true)
                            if thought == Some(true) {
                                debug!("Streaming Gemini thinking content as Claude thinking block");
                                
                                // Close previous block if it wasn't thinking
                                if let Some(current) = self.current_block_type {
                                    if current != BlockType::Thinking {
                                        events.push(StreamEvent::ContentBlockStop { 
                                            index: self.current_block_index 
                                        });
                                        self.current_block_index += 1;
                                        self.current_block_type = None;
                                    }
                                }

                                // Start thinking block if needed
                                if self.current_block_type.is_none() {
                                    events.push(StreamEvent::ContentBlockStart {
                                        index: self.current_block_index,
                                        content_block: ContentBlockStart::Thinking,
                                    });
                                    self.current_block_type = Some(BlockType::Thinking);
                                }
                                
                                // Send thinking text as delta
                                events.push(StreamEvent::ContentBlockDelta {
                                    index: self.current_block_index,
                                    delta: Delta::ThinkingDelta { thinking: text },
                                });
                                
                                // Send signature delta if provided by Gemini
                                if let Some(sig) = thought_signature {
                                    events.push(StreamEvent::ContentBlockDelta {
                                        index: self.current_block_index,
                                        delta: Delta::SignatureDelta { signature: sig },
                                    });
                                }
                            } else {
                                // Regular text content - use existing logic
                                // Process text chunk into segments (Text vs Thinking from <think> tags)
                                let segments = self.process_text_chunk(&text);
                            
                            for (block_type, content) in segments {
                                // 1. Handle block transitions
                                if let Some(current) = self.current_block_type {
                                    if current != block_type {
                                        // Close previous block
                                        events.push(StreamEvent::ContentBlockStop { 
                                            index: self.current_block_index 
                                        });
                                        self.current_block_index += 1;
                                        self.current_block_type = None;
                                    }
                                }

                                // 2. Start new block if needed
                                if self.current_block_type.is_none() {
                                    let content_block = match block_type {
                                        BlockType::Text => ContentBlockStart::Text { text: String::new() },
                                        BlockType::Thinking => ContentBlockStart::Thinking,
                                        _ => ContentBlockStart::Text { text: String::new() }, // Should not happen
                                    };

                                    events.push(StreamEvent::ContentBlockStart {
                                        index: self.current_block_index,
                                        content_block,
                                    });
                                    self.current_block_type = Some(block_type.clone());
                                }

                                // 3. Emit delta
                                // For text, we need to handle accumulation deduplication
                                // For thinking, we just emit content as is (simplification)
                                let delta_content = match block_type {
                                    BlockType::Text => {
                                        let full_accum = self.accumulated_text.clone() + &content;
                                        let delta = if full_accum.starts_with(&self.accumulated_text) {
                                            full_accum[self.accumulated_text.len()..].to_string()
                                        } else {
                                            content.clone()
                                        };
                                        self.accumulated_text = full_accum; // Update accum *after* calc
                                        delta
                                    },
                                    BlockType::Thinking => {
                                         // Simplify: just emit thinking content directly without dedup logic for now
                                         // as thinking buffer is cleared in process_text_chunk
                                         content
                                    },
                                    _ => String::new(),
                                };

                                if !delta_content.is_empty() {
                                     let delta = match block_type {
                                         BlockType::Text => Delta::TextDelta { text: delta_content },
                                         BlockType::Thinking => Delta::ThinkingDelta { thinking: delta_content },
                                         _ => Delta::TextDelta { text: String::new() },
                                     };
                                     
                                     events.push(StreamEvent::ContentBlockDelta {
                                         index: self.current_block_index,
                                         delta, 
                                     });
                                }
                            }
                            } // End else (regular text)
                        }
                        
                        crate::models::gemini::Part::Thought { thought, thought_signature } => {
                            debug!("Streaming Gemini thought as Claude thinking block");
                            
                            // Close previous block if it wasn't thinking
                            if let Some(current) = self.current_block_type {
                                if current != BlockType::Thinking {
                                    events.push(StreamEvent::ContentBlockStop { 
                                        index: self.current_block_index 
                                    });
                                    self.current_block_index += 1;
                                    self.current_block_type = None;
                                }
                            }

                            // Start thinking block if needed
                            if self.current_block_type.is_none() {
                                events.push(StreamEvent::ContentBlockStart {
                                    index: self.current_block_index,
                                    content_block: ContentBlockStart::Thinking,
                                });
                                self.current_block_type = Some(BlockType::Thinking);
                            }
                            
                            // Send thinking text as delta
                            events.push(StreamEvent::ContentBlockDelta {
                                index: self.current_block_index,
                                delta: Delta::ThinkingDelta { thinking: thought },
                            });
                            
                            // Send signature delta if provided by Gemini
                            if let Some(sig) = thought_signature {
                                events.push(StreamEvent::ContentBlockDelta {
                                    index: self.current_block_index,
                                    delta: Delta::SignatureDelta { signature: sig },
                                });
                            }
                        }
                        
                        crate::models::gemini::Part::InlineData { .. } => {
                            // Images aren't streamed incrementally
                        }
                        crate::models::gemini::Part::FunctionCall { function_call, thought_signature } => {
                            // Close any previous block
                            if self.current_block_type.is_some() {
                                events.push(StreamEvent::ContentBlockStop { 
                                    index: self.current_block_index 
                                });
                                self.current_block_index += 1;
                                self.current_block_type = None;
                            }

                            // Tool use is treated as atomic block for now (GEMINI sends full object)
                            // Generate tool use ID
                            let tool_id = format!("toolu_{}", uuid::Uuid::new_v4().simple());
                            
                            // Store the thoughtSignature
                            if let Some(ref sig) = thought_signature {
                                crate::translation::signature_store::store_signature(&tool_id, sig);
                            }
                            
                            // Send content_block_start
                            events.push(StreamEvent::ContentBlockStart {
                                index: self.current_block_index,
                                content_block: ContentBlockStart::ToolUse {
                                    id: tool_id.clone(),
                                    name: function_call.name.clone(),
                                },
                            });
                            
                            // Send delta
                            let args_json = serde_json::to_string(&function_call.args)
                                .unwrap_or_else(|_| "{}".to_string());
                            
                            debug!("Translated function call: {} with args: {}", function_call.name, args_json);
                            
                            events.push(StreamEvent::ContentBlockDelta {
                                index: self.current_block_index,
                                delta: Delta::InputJsonDelta {
                                    partial_json: args_json,
                                },
                            });

                            // Always close tool blocks immediately since they come as full objects
                            events.push(StreamEvent::ContentBlockStop { 
                                index: self.current_block_index 
                            });
                            
                            self.current_block_index += 1;
                            self.current_block_type = None;
                            self.had_tool_use = true;
                        }
                        crate::models::gemini::Part::FunctionResponse { .. } => {}
                    }
                }

                // Check if this is the final chunk
                if let Some(finish_reason) = candidate.finish_reason {
                    debug!("Stream finished with reason: {}", finish_reason);

                    // Update output tokens
                    if let Some(usage) = wrapper.usage_metadata {
                        self.output_tokens = usage.candidates_token_count.unwrap_or(0);
                    }

                    // Close any open block
                    if self.current_block_type.is_some() {
                        events.push(StreamEvent::ContentBlockStop { 
                            index: self.current_block_index 
                        });
                    }

                    // Set stop reason correctly
                    let stop_reason = if self.had_tool_use && finish_reason == "STOP" {
                        Some("tool_use".to_string())
                    } else {
                        match finish_reason.as_str() {
                            "STOP" => Some("end_turn".to_string()),
                            "MAX_TOKENS" => Some("max_tokens".to_string()),
                            _ => None,
                        }
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
            }),
        };

        let events = translator.translate_chunk(chunk).unwrap();
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], StreamEvent::MessageStart { .. }));
    }
}
