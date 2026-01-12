// Model name mapping (Claude → Gemini)
// Author: kelexine (https://github.com/kelexine)

use crate::error::{ProxyError, Result};
use phf::phf_map;

/// Compile-time perfect hash map for model mapping (zero runtime cost)
static MODEL_MAP: phf::Map<&'static str, &'static str> = phf_map! {
    // Latest models (Gemini 3.x Preview - late 2025 / early 2026)
    "claude-opus-4" => "gemini-3-pro-preview",
    "claude-opus-4-5" => "gemini-3-pro-preview",
    "claude-sonnet-4-5" => "gemini-3-flash-preview",
    "claude-sonnet-4" => "gemini-3-flash-preview",
    "claude-haiku-4" => "gemini-2.5-flash",
    "claude-haiku-4-5" => "gemini-2.5-pro",

    // Previous generation (Gemini 2.5)
    "claude-3-5-sonnet-20241022" => "gemini-2.5-flash",
    "claude-3-5-sonnet" => "gemini-2.5-flash",
    "claude-3-opus-20240229" => "gemini-2.5-pro",
    "claude-3-opus" => "gemini-2.5-pro",
    "claude-3-sonnet-20240229" => "gemini-2.5-flash",
    "claude-3-sonnet" => "gemini-2.5-flash",
    "claude-3-haiku-20240307" => "gemini-2.5-flash-lite",
    "claude-3-haiku" => "gemini-2.5-flash-lite",
};

/// Map Claude model name to Gemini model name
pub fn map_model(claude_model: &str) -> Result<String> {
    // Claude Code often sends versioned model names with date suffixes
    // e.g., "claude-sonnet-4-5-20250929" -> "claude-sonnet-4-5"
    let normalized = strip_date_suffix(claude_model);
    
    MODEL_MAP
        .get(&normalized as &str)
        .map(|s| s.to_string())
        .ok_or_else(|| {
            // Collect all keys for error message
            let supported: Vec<&str> = MODEL_MAP.keys().copied().collect();
            ProxyError::InvalidRequest(format!(
                "Unsupported model: {}. Supported models: {}",
                claude_model,
                supported.join(", ")
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

    #[test]
    fn test_phf_compile_time() {
        // This test verifies that MODEL_MAP is a compile-time constant
        // If phf is working correctly, this lookup has zero runtime overhead
        let result = MODEL_MAP.get("claude-sonnet-4");
        assert_eq!(result, Some(&"gemini-3-flash-preview"));
    }
}
