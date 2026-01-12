// Model name mapping (Claude → Gemini)
// Author: kelexine (https://github.com/kelexine)

use crate::error::{ProxyError, Result};
use std::collections::HashMap;
use std::sync::OnceLock;

/// Lazily initialized model map using OnceLock (zero-cost, panic-free)
static MODEL_MAP: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();

/// Get or initialize the model mapping
fn get_model_map() -> &'static HashMap<&'static str, &'static str> {
    MODEL_MAP.get_or_init(|| {
        let mut m = HashMap::new();

        // Latest models (Gemini 3.x Preview - late 2025 / early 2026)
        m.insert("claude-opus-4", "gemini-3-pro-preview");
        m.insert("claude-opus-4-5", "gemini-3-pro-preview");
        m.insert("claude-sonnet-4-5", "gemini-3-flash-preview");
        m.insert("claude-sonnet-4", "gemini-3-flash-preview");
        m.insert("claude-haiku-4", "gemini-2.5-flash");
        m.insert("claude-haiku-4-5", "gemini-2.5-pro");

        // Previous generation (Gemini 2.5)
        m.insert("claude-3-5-sonnet-20241022", "gemini-2.5-flash");
        m.insert("claude-3-5-sonnet", "gemini-2.5-flash");
        m.insert("claude-3-opus-20240229", "gemini-2.5-pro");
        m.insert("claude-3-opus", "gemini-2.5-pro");
        m.insert("claude-3-sonnet-20240229", "gemini-2.5-flash");
        m.insert("claude-3-sonnet", "gemini-2.5-flash");
        m.insert("claude-3-haiku-20240307", "gemini-2.5-flash-lite");
        m.insert("claude-3-haiku", "gemini-2.5-flash-lite");

        m
    })
}

/// Map Claude model name to Gemini model name
pub fn map_model(claude_model: &str) -> Result<String> {
    // Claude Code often sends versioned model names with date suffixes
    // e.g., "claude-sonnet-4-5-20250929" -> "claude-sonnet-4-5"
    let normalized = strip_date_suffix(claude_model);
    
    get_model_map()
        .get(normalized.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| {
            ProxyError::InvalidRequest(format!(
                "Unsupported model: {}. Supported models: {}",
                claude_model,
                get_model_map().keys().copied().collect::<Vec<_>>().join(", ")
            ))
        })
}

/// Strip date suffix from model names (e.g., "claude-sonnet-4-5-20250929" -> "claude-sonnet-4-5")
fn strip_date_suffix(model: &str) -> String {
    // Date suffixes are 8 digits at the end: YYYYMMDD
    if model.len() > 9 && model.chars().nth(model.len() - 9) == Some('-') {
        let suffix = &model[model.len() - 8..];
        if suffix.chars().all(|c| c.is_ascii_digit()) {
            // It's a date suffix, strip it
            return model[..model.len() - 9].to_string();
        }
    }
    model.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_model_mapping() {
        assert_eq!(map_model("claude-sonnet-4-5").unwrap(), "gemini-3-flash-preview");
        assert_eq!(map_model("claude-opus-4").unwrap(), "gemini-3-pro-preview");
        assert_eq!(map_model("claude-3-5-sonnet").unwrap(), "gemini-2.5-flash");
        assert!(map_model("unknown-model").is_err());
    }

    #[test]
    fn test_date_suffix_stripping() {
        // Test with date suffix
        assert_eq!(
            map_model("claude-sonnet-4-5-20250929").unwrap(),
            "gemini-3-flash-preview"
        );
        assert_eq!(
            map_model("claude-opus-4-5-20251101").unwrap(),
            "gemini-3-pro-preview"
        );
        assert_eq!(
            map_model("claude-haiku-4-5-20251001").unwrap(),
            "gemini-2.5-pro"
        );
        
        // Test without date suffix
        assert_eq!(map_model("claude-sonnet-4-5").unwrap(), "gemini-3-flash-preview");
    }

    #[test]
    fn test_strip_date_suffix() {
        assert_eq!(strip_date_suffix("claude-sonnet-4-5-20250929"), "claude-sonnet-4-5");
        assert_eq!(strip_date_suffix("claude-opus-4-5-20251101"), "claude-opus-4-5");
        assert_eq!(strip_date_suffix("claude-sonnet-4-5"), "claude-sonnet-4-5"); // No date suffix
    }
}
