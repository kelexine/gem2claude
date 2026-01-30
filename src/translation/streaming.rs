//! SSE event translation for streaming responses.
//!
//! This module implements the `StreamTranslator`, which acts as a bridge between
//! Gemini's SSE format and the Anthropic SSE format expected by clients like Claude Code.
//! It maintains state across multiple chunks to handle cases where logical blocks
//! (like thinking or tool uses) are split across multiple HTTP chunks.

// Author: kelexine (https://github.com/kelexine)

use crate::error::Result;
use crate::models::gemini::GenerateContentResponse;
use crate::models::streaming::*;

use crate::translation::stream_processors::{process_text_segment, BlockType};

/// Stateful translator for a single streaming request.
///
/// The `StreamTranslator` tracks token usage, current content block indices,
/// and handles the transformation of Gemini's response structure into
/// Anthropic's event-based architecture.
pub struct StreamTranslator {
    /// Unique identifier for the generated message.
    message_id: String,
    /// The model being used for generation.
    pub model: String,
    /// Cumulative input tokens for the request.
    pub input_tokens: u32,
    /// Cumulative output tokens generated so far.
    pub output_tokens: u32,
    /// Tokens read from an upstream cache.
    pub cached_input_tokens: u32,
    /// Tokens used to create a new upstream cache entry.
    pub cached_creation_input_tokens: u32,
    /// Flag to track if the `message_start` event has been sent.
    first_chunk: bool,

    /// 0-indexed position of the current content block in the message.
    current_block_index: i32,
    /// The type of the block currently being emitted.
    current_block_type: Option<BlockType>,
    /// Tracks if any tool use has occurred in this message (affects finish reason).
    had_tool_use: bool,

    /// Buffer for storing partial `<think>` tags between chunks.
    thinking_buffer: String,
    /// Flag indicating if the cursor is currently inside a `<think>` block.
    in_thinking: bool,
}

impl StreamTranslator {
    /// Initializes a new translator for a specific model.
    pub fn new(model: String) -> Self {
        Self {
            message_id: format!("msg_{}", uuid::Uuid::new_v4().simple()),
            model,
            input_tokens: 0,
            output_tokens: 0,
            cached_input_tokens: 0,
            cached_creation_input_tokens: 0,
            first_chunk: true,

            current_block_index: 0,
            current_block_type: None,
            had_tool_use: false,

            thinking_buffer: String::new(),
            in_thinking: false,
        }
    }

    /// Segments a text chunk into logical parts by detecting `<think>` and `</think>` tags.
    fn process_text_chunk<'a>(
        &mut self,
        text: &'a str,
    ) -> Vec<(BlockType, std::borrow::Cow<'a, str>)> {
        process_text_segment(text, &mut self.in_thinking, &mut self.thinking_buffer)
    }

    /// Primary entry point for translating a Gemini API chunk into Anthropic events.
    ///
    /// This method manages the lifecycle of the entire stream:
    /// - Emits `message_start` on the first encounter.
    /// - Dispatches content to `emit_thinking_content`, `emit_text_segments`, or `emit_tool_use`.
    /// - Finalizes the stream with `emit_completion` when the `finish_reason` is detected.
    pub fn translate_chunk(
        &mut self,
        gemini_chunk: GenerateContentResponse,
    ) -> Result<Vec<StreamEvent>> {
        let mut events = Vec::new();

        // Initial handshake: Define the message structure and usage baseline.
        if self.first_chunk {
            if let Some(wrapper) = &gemini_chunk.response {
                if let Some(usage) = &wrapper.usage_metadata {
                    self.input_tokens = usage.prompt_token_count.unwrap_or(0);
                    self.output_tokens = usage.candidates_token_count.unwrap_or(0);
                    self.cached_input_tokens = usage.cached_content_token_count.unwrap_or(0);
                }
            }

            crate::metrics::record_sse_event("message_start", &self.model);
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

        if let Some(wrapper) = gemini_chunk.response {
            if let Some(candidate) = wrapper.candidates.into_iter().next() {
                for part in candidate.content.parts {
                    match part {
                        crate::models::gemini::Part::Text {
                            text,
                            thought,
                            thought_signature,
                        } => {
                            // Gemini 2.0+ can flag text parts as native thinking content.
                            if thought == Some(true) {
                                self.emit_thinking_content(&text, thought_signature, &mut events);
                            } else {
                                self.emit_text_segments(&text, &mut events);
                            }
                        }
                        crate::models::gemini::Part::Thought {
                            thought,
                            thought_signature,
                        } => {
                            // Dedicated thinking part in Gemini's internal protocol.
                            self.emit_thinking_content(&thought, thought_signature, &mut events);
                        }
                        crate::models::gemini::Part::FunctionCall {
                            function_call,
                            thought_signature,
                        } => {
                            self.emit_tool_use(function_call, thought_signature, &mut events);
                        }
                        _ => {}
                    }
                }

                if let Some(finish_reason) = candidate.finish_reason {
                    // Critical: Intercept malformed function calls to provide better error diagnostics.
                    if finish_reason == "MALFORMED_FUNCTION_CALL" {
                        crate::metrics::record_sse_event("error", &self.model);
                        events.push(StreamEvent::Error {
                            error: ErrorData {
                                error_type: "invalid_request_error".to_string(),
                                message: "The model generated a malformed tool call. Verification failed.".to_string(),
                            }
                        });
                        return Ok(events);
                    }
                    self.emit_completion(finish_reason, wrapper.usage_metadata, &mut events);
                }
            }
        }

        Ok(events)
    }

    /// Emits content as an Anthropic `thinking` block.
    fn emit_thinking_content(
        &mut self,
        content: &str,
        signature: Option<String>,
        events: &mut Vec<StreamEvent>,
    ) {
        // Enforce block separation: Close current block if it's not thinking.
        if let Some(current) = self.current_block_type {
            if current != BlockType::Thinking {
                events.push(StreamEvent::ContentBlockStop {
                    index: self.current_block_index,
                });
                self.current_block_index += 1;
                self.current_block_type = None;
            }
        }

        if self.current_block_type.is_none() {
            events.push(StreamEvent::ContentBlockStart {
                index: self.current_block_index,
                content_block: ContentBlockStart::Thinking,
            });
            self.current_block_type = Some(BlockType::Thinking);
        }

        if !content.is_empty() {
            events.push(StreamEvent::ContentBlockDelta {
                index: self.current_block_index,
                delta: Delta::ThinkingDelta {
                    thinking: content.to_string(),
                },
            });
        }

        // Handle cryptographic thinking signatures if provided by Gemini.
        if let Some(sig) = signature {
            events.push(StreamEvent::ContentBlockDelta {
                index: self.current_block_index,
                delta: Delta::SignatureDelta { signature: sig },
            });
        }
    }

    /// Processes regular text and translates it into `text` or `thinking` blocks.
    fn emit_text_segments(&mut self, text: &str, events: &mut Vec<StreamEvent>) {
        let segments = self.process_text_chunk(text);

        for (block_type, content) in segments {
            if let Some(current) = self.current_block_type {
                if current != block_type {
                    events.push(StreamEvent::ContentBlockStop {
                        index: self.current_block_index,
                    });
                    self.current_block_index += 1;
                    self.current_block_type = None;
                }
            }

            if self.current_block_type.is_none() {
                let content_block = match block_type {
                    BlockType::Text => ContentBlockStart::Text {
                        text: String::new(),
                    },
                    BlockType::Thinking => ContentBlockStart::Thinking,
                    BlockType::ToolUse => unreachable!(),
                };
                events.push(StreamEvent::ContentBlockStart {
                    index: self.current_block_index,
                    content_block,
                });
                self.current_block_type = Some(block_type);
            }

            if !content.is_empty() {
                let delta = match block_type {
                    BlockType::Text => Delta::TextDelta {
                        text: content.into_owned(),
                    },
                    BlockType::Thinking => Delta::ThinkingDelta {
                        thinking: content.into_owned(),
                    },
                    BlockType::ToolUse => unreachable!(),
                };
                events.push(StreamEvent::ContentBlockDelta {
                    index: self.current_block_index,
                    delta,
                });
            }
        }
    }

    /// Translates a Gemini function call into an Anthropic `tool_use` event.
    fn emit_tool_use(
        &mut self,
        function_call: crate::models::gemini::FunctionCall,
        thought_signature: Option<String>,
        events: &mut Vec<StreamEvent>,
    ) {
        if self.current_block_type.is_some() {
            events.push(StreamEvent::ContentBlockStop {
                index: self.current_block_index,
            });
            self.current_block_index += 1;
            self.current_block_type = None;
        }

        let tool_id = format!("toolu_{}", uuid::Uuid::new_v4().simple());
        if let Some(ref sig) = thought_signature {
            crate::translation::signature_store::store_signature(&tool_id, sig);
        }

        events.push(StreamEvent::ContentBlockStart {
            index: self.current_block_index,
            content_block: ContentBlockStart::ToolUse {
                id: tool_id.clone(),
                name: function_call.name.clone(),
            },
        });

        let args_json = serde_json::to_string(&function_call.args).unwrap_or_default();
        events.push(StreamEvent::ContentBlockDelta {
            index: self.current_block_index,
            delta: Delta::InputJsonDelta {
                partial_json: args_json,
            },
        });

        // Anthropic protocol requires tool use blocks to stop before message finish.
        events.push(StreamEvent::ContentBlockStop {
            index: self.current_block_index,
        });

        self.current_block_index += 1;
        self.current_block_type = None;
        self.had_tool_use = true;
    }

    /// Emits the final `message_delta` and `message_stop` signals.
    fn emit_completion(
        &mut self,
        finish_reason: String,
        usage: Option<crate::models::gemini::UsageMetadata>,
        events: &mut Vec<StreamEvent>,
    ) {
        if let Some(usage_meta) = usage {
            self.output_tokens = usage_meta.candidates_token_count.unwrap_or(0);
        }

        if self.current_block_type.is_some() {
            events.push(StreamEvent::ContentBlockStop {
                index: self.current_block_index,
            });
            self.current_block_type = None;
        }

        // Map Gemini stop reasons to Anthropic equivalents.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_process_text_simple() {
        let mut translator = StreamTranslator::new("test".to_string());
        let segments = translator.process_text_chunk("Hello <think>internal</think> world");

        assert_eq!(segments.len(), 3);
        assert_eq!(
            segments[0],
            (BlockType::Text, std::borrow::Cow::Borrowed("Hello "))
        );
        assert_eq!(
            segments[1],
            (BlockType::Thinking, std::borrow::Cow::Borrowed("internal"))
        );
    }

    #[test]
    fn test_partial_tag_detection() {
        use crate::translation::stream_processors::find_partial_tag;
        assert_eq!(find_partial_tag("hello<", "<think>"), Some(5));
        assert_eq!(find_partial_tag("hello<think", "<think>"), Some(5));
    }
}
