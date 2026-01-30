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
pub fn process_text_segment<'a>(
    text: &'a str,
    in_thinking: &mut bool,
    thinking_buffer: &mut String,
) -> Vec<(BlockType, std::borrow::Cow<'a, str>)> {
    let mut segments = Vec::new();

    // Fast path: if no tags involved and not in thinking mode, just return text
    if !*in_thinking && !text.contains("<think") {
        if !text.is_empty() {
            segments.push((BlockType::Text, std::borrow::Cow::Borrowed(text)));
        }
        return segments;
    }

    let mut current_pos = 0;
    let len = text.len();

    // If we have a buffer from previous chunk, we need to handle it first
    // This is the only place we might need concatenation if the tag was split
    if !thinking_buffer.is_empty() {
        let combined = thinking_buffer.clone() + text;
        thinking_buffer.clear();

        // Safety check
        if combined.len() > 10 * 1024 * 1024 {
            tracing::error!("Thinking buffer safety limit (10MB) exceeded.");
            let cleaned = combined.replace("<think>", "").replace("</think>", "");
            segments.push((BlockType::Text, std::borrow::Cow::Owned(cleaned)));
            *in_thinking = false;
            return segments;
        }

        // Recursive call with cleared buffer to handle the boundary condition
        // This is rare (only happens on split tags), so the recursion overhead is negligible across the stream
        let recursive_result = process_text_segment(&combined, in_thinking, thinking_buffer);

        // We must convert to Owned because 'combined' is local and will be dropped
        return recursive_result
            .into_iter()
            .map(|(bt, cow): (BlockType, std::borrow::Cow<'_, str>)| {
                (bt, std::borrow::Cow::Owned(cow.into_owned()))
            })
            .collect();
    }

    while current_pos < len {
        if *in_thinking {
            match text[current_pos..].find("</think>") {
                Some(offset) => {
                    let end_tag_start = current_pos + offset;
                    let content = &text[current_pos..end_tag_start];

                    if !content.is_empty() {
                        segments.push((BlockType::Thinking, std::borrow::Cow::Borrowed(content)));
                    }

                    *in_thinking = false;
                    current_pos = end_tag_start + 8; // skip </think>
                }
                None => {
                    // check for partial tag at end
                    if let Some(partial_start) = find_partial_tag(&text[current_pos..], "</think>")
                    {
                        let content = &text[current_pos..current_pos + partial_start];
                        if !content.is_empty() {
                            segments
                                .push((BlockType::Thinking, std::borrow::Cow::Borrowed(content)));
                        }
                        thinking_buffer.push_str(&text[current_pos + partial_start..]);
                        current_pos = len; // Done with this chunk
                    } else {
                        // All remaining is thinking
                        let content = &text[current_pos..];
                        if !content.is_empty() {
                            segments
                                .push((BlockType::Thinking, std::borrow::Cow::Borrowed(content)));
                        }
                        current_pos = len;
                    }
                }
            }
        } else {
            match text[current_pos..].find("<think>") {
                Some(offset) => {
                    let tag_start = current_pos + offset;
                    let content = &text[current_pos..tag_start];

                    if !content.is_empty() {
                        segments.push((BlockType::Text, std::borrow::Cow::Borrowed(content)));
                    }

                    *in_thinking = true;
                    current_pos = tag_start + 7; // skip <think>
                }
                None => {
                    // check for partial tag at end
                    if let Some(partial_start) = find_partial_tag(&text[current_pos..], "<think>") {
                        let content = &text[current_pos..current_pos + partial_start];
                        if !content.is_empty() {
                            segments.push((BlockType::Text, std::borrow::Cow::Borrowed(content)));
                        }
                        thinking_buffer.push_str(&text[current_pos + partial_start..]);
                        current_pos = len;
                    } else {
                        // All remaining is text
                        let content = &text[current_pos..];
                        if !content.is_empty() {
                            segments.push((BlockType::Text, std::borrow::Cow::Borrowed(content)));
                        }
                        current_pos = len;
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
