// SSE event translation for streaming responses
// Author: kelexine (https://github.com/kelexine)
// Fixed version with simplified logic and proper tests

use crate::error::Result;
use crate::models::gemini::GenerateContentResponse;
use crate::models::streaming::*;
use tracing::debug;

#[derive(Debug, PartialEq, Clone, Copy)]
enum BlockType {
    Text,
    Thinking,
    #[allow(dead_code)]
    ToolUse,
}

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
    
    // Stateful thinking stripper (for <think> tags only)
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
            
            current_block_index: 0,
            current_block_type: None,
            had_tool_use: false,
            
            thinking_buffer: String::new(),
            in_thinking: false,
        }
    }

    /// Process text chunk with stateful thinking tag stripping
    /// Handles <think> tags that may be split across multiple chunks
    /// Returns a vector of (BlockType, String) tuples representing segments
    fn process_text_chunk(&mut self, text: &str) -> Vec<(BlockType, String)> {
        let mut segments = Vec::new();
        let mut full_text = self.thinking_buffer.clone() + text;
        self.thinking_buffer.clear();

        // Safety limit for buffer (prevent OOM attacks)
        if full_text.len() > 10 * 1024 * 1024 {
            tracing::error!(
                "Thinking buffer exceeded 10MB ({}), forcibly stripping tags", 
                full_text.len()
            );
            // Aggressively strip all thinking tags
            let cleaned = full_text
                .replace("<think>", "")
                .replace("</think>", "");
            segments.push((BlockType::Text, cleaned));
            self.in_thinking = false;
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
                        cache_creation_input_tokens: 0,
                        cache_read_input_tokens: 0,
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
                            // Check if this is native Gemini thinking (thought: true field)
                            if thought == Some(true) {
                                self.emit_thinking_content(&text, thought_signature, &mut events);
                            } else {
                                // Regular text - may contain <think> tags, process them
                                self.emit_text_segments(&text, &mut events);
                            }
                        }
                        
                        crate::models::gemini::Part::Thought { thought, thought_signature } => {
                            // Native Gemini 3 thinking part
                            self.emit_thinking_content(&thought, thought_signature, &mut events);
                        }
                        
                        crate::models::gemini::Part::InlineData { .. } => {
                            // Images aren't streamed incrementally
                        }
                        
                        crate::models::gemini::Part::FunctionCall { function_call, thought_signature } => {
                            self.emit_tool_use(function_call, thought_signature, &mut events);
                        }
                        
                        crate::models::gemini::Part::FunctionResponse { .. } => {}
                    }
                }

                // Check if this is the final chunk
                if let Some(finish_reason) = candidate.finish_reason {
                    self.emit_completion(finish_reason, wrapper.usage_metadata, &mut events);
                }
            }
        }

        Ok(events)
    }

    /// Emit native thinking content (from thought: true or Thought part)
    fn emit_thinking_content(
        &mut self,
        content: &str,
        signature: Option<String>,
        events: &mut Vec<StreamEvent>,
    ) {
        debug!("Emitting Gemini native thinking content");
        
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
        
        // Send thinking delta
        if !content.is_empty() {
            events.push(StreamEvent::ContentBlockDelta {
                index: self.current_block_index,
                delta: Delta::ThinkingDelta { 
                    thinking: content.to_string() 
                },
            });
        }
        
        // Send signature delta if present
        if let Some(sig) = signature {
            events.push(StreamEvent::ContentBlockDelta {
                index: self.current_block_index,
                delta: Delta::SignatureDelta { signature: sig },
            });
        }
    }

    /// Emit text segments (processing <think> tags)
    fn emit_text_segments(&mut self, text: &str, events: &mut Vec<StreamEvent>) {
        let segments = self.process_text_chunk(text);
        
        for (block_type, content) in segments {
            // Handle block transitions
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

            // Start new block if needed
            if self.current_block_type.is_none() {
                let content_block = match block_type {
                    BlockType::Text => ContentBlockStart::Text { text: String::new() },
                    BlockType::Thinking => ContentBlockStart::Thinking,
                    BlockType::ToolUse => unreachable!("Tool use not from text segments"),
                };

                events.push(StreamEvent::ContentBlockStart {
                    index: self.current_block_index,
                    content_block,
                });
                self.current_block_type = Some(block_type);
            }

            // Emit delta
            if !content.is_empty() {
                let delta = match block_type {
                    BlockType::Text => Delta::TextDelta { text: content },
                    BlockType::Thinking => Delta::ThinkingDelta { thinking: content },
                    BlockType::ToolUse => unreachable!(),
                };
                
                events.push(StreamEvent::ContentBlockDelta {
                    index: self.current_block_index,
                    delta,
                });
            }
        }
    }

    /// Emit tool use block
    fn emit_tool_use(
        &mut self,
        function_call: crate::models::gemini::FunctionCall,
        thought_signature: Option<String>,
        events: &mut Vec<StreamEvent>,
    ) {
        // Close any previous block
        if self.current_block_type.is_some() {
            events.push(StreamEvent::ContentBlockStop { 
                index: self.current_block_index 
            });
            self.current_block_index += 1;
            self.current_block_type = None;
        }

        // Generate tool use ID
        let tool_id = format!("toolu_{}", uuid::Uuid::new_v4().simple());
        
        // Store thought signature for later use in conversation history
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
        
        // Send delta with full args (Gemini sends complete object)
        let args_json = serde_json::to_string(&function_call.args)
            .unwrap_or_else(|_| "{}".to_string());
        
        debug!("Tool call: {} with args: {}", function_call.name, args_json);
        
        events.push(StreamEvent::ContentBlockDelta {
            index: self.current_block_index,
            delta: Delta::InputJsonDelta {
                partial_json: args_json,
            },
        });

        // Always close tool blocks immediately (they come as complete objects)
        events.push(StreamEvent::ContentBlockStop { 
            index: self.current_block_index 
        });
        
        self.current_block_index += 1;
        self.current_block_type = None;
        self.had_tool_use = true;
    }

    /// Emit completion events
    fn emit_completion(
        &mut self,
        finish_reason: String,
        usage: Option<crate::models::gemini::UsageMetadata>,
        events: &mut Vec<StreamEvent>,
    ) {
        debug!("Stream finished with reason: {}", finish_reason);

        // Update output tokens
        if let Some(usage_meta) = usage {
            self.output_tokens = usage_meta.candidates_token_count.unwrap_or(0);
        }

        // Close any open block
        if self.current_block_type.is_some() {
            events.push(StreamEvent::ContentBlockStop { 
                index: self.current_block_index 
            });
            self.current_block_type = None;
        }

        // Map finish reason
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
        
        // Reset state for potential next message
        self.thinking_buffer.clear();
        self.in_thinking = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_process_text_simple() {
        let mut translator = StreamTranslator::new("test".to_string());
        
        let segments = translator.process_text_chunk("Hello <think>internal</think> world");
        
        assert_eq!(segments.len(), 3);
        assert_eq!(segments[0], (BlockType::Text, "Hello ".to_string()));
        assert_eq!(segments[1], (BlockType::Thinking, "internal".to_string()));
        assert_eq!(segments[2], (BlockType::Text, " world".to_string()));
    }

    #[test]
    fn test_process_text_split_open_tag() {
        let mut translator = StreamTranslator::new("test".to_string());
        
        // First chunk: partial opening tag
        let seg1 = translator.process_text_chunk("Hello <thi");
        assert_eq!(seg1.len(), 1);
        assert_eq!(seg1[0], (BlockType::Text, "Hello ".to_string()));
        
        // Second chunk: complete tag
        let seg2 = translator.process_text_chunk("nk>secret</think> world");
        assert_eq!(seg2.len(), 2);
        assert_eq!(seg2[0], (BlockType::Thinking, "secret".to_string()));
        assert_eq!(seg2[1], (BlockType::Text, " world".to_string()));
    }

    #[test]
    fn test_process_text_split_close_tag() {
        let mut translator = StreamTranslator::new("test".to_string());
        
        // First chunk: opens and partial close
        let seg1 = translator.process_text_chunk("<think>secret</thi");
        // Emits buffered content up to the thinking tag start
        assert_eq!(seg1.len(), 1);
        assert_eq!(seg1[0].0, BlockType::Thinking); // thinking block starts
        
        // Second chunk: complete close + more text
        let seg2 = translator.process_text_chunk("nk> visible");
        // Should emit visible text after thinking closes
        assert_eq!(seg2.len(), 1);
        assert_eq!(seg2[0], (BlockType::Text, " visible".to_string()));
    }

    #[test]
    fn test_process_text_multiple_blocks() {
        let mut translator = StreamTranslator::new("test".to_string());
        
        let segments = translator.process_text_chunk(
            "A<think>x</think>B<think>y</think>C"
        );
        
        assert_eq!(segments.len(), 5);
        assert_eq!(segments[0], (BlockType::Text, "A".to_string()));
        assert_eq!(segments[1], (BlockType::Thinking, "x".to_string()));
        assert_eq!(segments[2], (BlockType::Text, "B".to_string()));
        assert_eq!(segments[3], (BlockType::Thinking, "y".to_string()));
        assert_eq!(segments[4], (BlockType::Text, "C".to_string()));
    }

    #[test]
    fn test_partial_tag_detection() {
        assert_eq!(StreamTranslator::find_partial_tag("hello<", "<think>"), Some(5));
        assert_eq!(StreamTranslator::find_partial_tag("hello<t", "<think>"), Some(5));
        assert_eq!(StreamTranslator::find_partial_tag("hello<thi", "<think>"), Some(5));
        assert_eq!(StreamTranslator::find_partial_tag("hello<think", "<think>"), Some(5));
        assert_eq!(StreamTranslator::find_partial_tag("hello", "<think>"), None);
        assert_eq!(StreamTranslator::find_partial_tag("hello<x", "<think>"), None);
    }

    #[test]
    fn test_empty_content() {
        let mut translator = StreamTranslator::new("test".to_string());
        
        let segments = translator.process_text_chunk("<think></think>");
        assert_eq!(segments.len(), 0); // Empty thinking block, nothing emitted
        
        let segments2 = translator.process_text_chunk("");
        assert_eq!(segments2.len(), 0);
    }

    #[test]
    fn test_no_thinking_tags() {
        let mut translator = StreamTranslator::new("test".to_string());
        
        let segments = translator.process_text_chunk("Just plain text");
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0], (BlockType::Text, "Just plain text".to_string()));
    }
}