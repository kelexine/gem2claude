// Streaming content processors
// Author: kelexine (https://github.com/kelexine)

/// Internal identifier for the type of content block being processed.
#[derive(Debug, PartialEq, Clone, Copy)]
pub enum BlockType {
    /// Standard assistant response text.
    Text,
    /// Internal reasoning or "thinking" content.
    Thinking,
    /// A call to an external tool/function.
    #[allow(dead_code)]
    ToolUse,
}

/// Segments a text chunk into logical parts by detecting `<think>` and `</think>` tags.
pub fn process_text_segment(
    text: &str,
    in_thinking: &mut bool,
    thinking_buffer: &mut String,
) -> Vec<(BlockType, String)> {
    let mut segments = Vec::new();
    let mut full_text = thinking_buffer.clone() + text;
    thinking_buffer.clear();

    // Security check: ensure the thinking buffer doesn't grow indefinitely.
    if full_text.len() > 10 * 1024 * 1024 {
        tracing::error!("Thinking buffer safety limit (10MB) exceeded.");
        let cleaned = full_text.replace("<think>", "").replace("</think>", "");
        segments.push((BlockType::Text, cleaned));
        *in_thinking = false;
        return segments;
    }

    loop {
        if *in_thinking {
            match full_text.find("</think>") {
                Some(idx) => {
                    let content = full_text[..idx].to_string();
                    if !content.is_empty() {
                        segments.push((BlockType::Thinking, content));
                    }
                    *in_thinking = false;
                    full_text = full_text[idx + 8..].to_string();
                }
                None => {
                    if let Some(partial_idx) = find_partial_tag(&full_text, "</think>") {
                        let content = full_text[..partial_idx].to_string();
                        if !content.is_empty() {
                            segments.push((BlockType::Thinking, content));
                        }
                        *thinking_buffer = full_text[partial_idx..].to_string();
                        break;
                    } else {
                        if !full_text.is_empty() {
                            segments.push((BlockType::Thinking, full_text));
                        }
                        break;
                    }
                }
            }
        } else {
            match full_text.find("<think>") {
                Some(idx) => {
                    let content = full_text[..idx].to_string();
                    if !content.is_empty() {
                        segments.push((BlockType::Text, content));
                    }
                    *in_thinking = true;
                    full_text = full_text[idx + 7..].to_string();
                }
                None => {
                    if let Some(partial_idx) = find_partial_tag(&full_text, "<think>") {
                        let content = full_text[..partial_idx].to_string();
                        if !content.is_empty() {
                            segments.push((BlockType::Text, content));
                        }
                        *thinking_buffer = full_text[partial_idx..].to_string();
                        break;
                    } else {
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

/// Helper to detect if a string ends with the beginning of a specific tag.
pub fn find_partial_tag(text: &str, tag: &str) -> Option<usize> {
    for i in 1..tag.len() {
        let prefix = &tag[..i];
        if text.ends_with(prefix) {
            return Some(text.len() - prefix.len());
        }
    }
    None
}
