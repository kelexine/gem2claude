// Model mapping comprehensive tests
// Author: kelexine (https://github.com/kelexine)

use gem2claude::models::mapping::map_model;

#[test]
fn test_all_core_models() {
    // Claude 4.5 models (per PHF map)
    assert_eq!(map_model("claude-opus-4-5").unwrap(), "gemini-3-pro-preview");
    assert_eq!(map_model("claude-sonnet-4-5").unwrap(), "gemini-3-flash-preview");
    assert_eq!(map_model("claude-haiku-4-5").unwrap(), "gemini-2.5-pro");
    
    // Claude 4/4.1 models (per PHF map)
    assert_eq!(map_model("claude-opus-4").unwrap(), "gemini-2.5-pro");
    assert_eq!(map_model("claude-sonnet-4").unwrap(), "gemini-2.5-flash");
    
    // Claude 3.7 models (per PHF map) - use dot notation
    assert_eq!(map_model("claude-3.7-sonnet").unwrap(), "gemini-2.5-flash-lite");
}

#[test]
fn test_date_suffix_handling() {
    // Test models with date suffixes that exist in PHF map
    assert_eq!(
        map_model("claude-sonnet-4-5-20250929").unwrap(),
        "gemini-3-flash-preview"
    );
    
    assert_eq!(
        map_model("claude-haiku-4-5-20251001").unwrap(),
        "gemini-2.5-pro"
    );
}

#[test]
fn test_invalid_model_error() {
    let result = map_model("unknown-model-xyz");
    assert!(result.is_err());
}

#[test]
fn test_case_sensitivity() {
    let result = map_model("CLAUDE-SONNET-4-5");
    assert!(result.is_err(), "Model names should be case-sensitive");
}

#[test]
fn test_dot_notation_models() {
    // Test dot notation variants
    assert_eq!(map_model("claude-opus-4.5").unwrap(), "gemini-3-pro-preview");
    assert_eq!(map_model("claude-sonnet-4.5").unwrap(), "gemini-3-flash-preview");
    assert_eq!(map_model("claude-haiku-4.5").unwrap(), "gemini-2.5-pro");
}

#[test]
fn test_claude_4_1_models() {
    assert_eq!(map_model("claude-opus-4-1").unwrap(), "gemini-2.5-pro");
    assert_eq!(map_model("claude-opus-4.1").unwrap(), "gemini-2.5-pro");
}

#[test]
fn test_all_dated_variants() {
    // Test all dated model variants that exist in PHF map
    assert_eq!(map_model("claude-opus-4-5-20251101").unwrap(), "gemini-3-pro-preview");
    assert_eq!(map_model("claude-opus-4-20250514").unwrap(), "gemini-2.5-pro");
    assert_eq!(map_model("claude-sonnet-4-20250514").unwrap(), "gemini-2.5-flash");
    assert_eq!(map_model("claude-opus-4-1-20250805").unwrap(), "gemini-2.5-pro");
}

#[test]
fn test_empty_model_string() {
    let result = map_model("");
    assert!(result.is_err());
}

#[test]
fn test_whitespace_model() {
    let result = map_model("  ");
    assert!(result.is_err());
}

#[test]
fn test_partial_model_name() {
    let result = map_model("claude-sonnet");
    assert!(result.is_err());
}
