// Thought signature storage for preserving Gemini signatures across requests
// Author: kelexine (https://github.com/kelexine)
//
// Gemini requires thoughtSignatures to be preserved when replaying function calls
// in conversation history. This module tracks the mapping from tool_use_id to
// the original thoughtSignature returned by Gemini.

use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::sync::RwLock;
use tracing::debug;

/// Global storage for tool_use_id → thoughtSignature mappings
/// Uses RwLock for thread-safe concurrent access
static SIGNATURE_STORE: Lazy<RwLock<HashMap<String, String>>> =
    Lazy::new(|| RwLock::new(HashMap::new()));

/// Store a thoughtSignature for a given tool_use_id
/// Called when we receive a function call response from Gemini
pub fn store_signature(tool_use_id: &str, thought_signature: &str) {
    if let Ok(mut store) = SIGNATURE_STORE.write() {
        debug!(
            "Storing thoughtSignature for tool_use_id: {} (sig length: {})",
            tool_use_id,
            thought_signature.len()
        );
        store.insert(tool_use_id.to_string(), thought_signature.to_string());
    }
}

/// Retrieve the thoughtSignature for a given tool_use_id
/// Returns None if not found (will use skip_thought_signature_validator as fallback)
pub fn get_signature(tool_use_id: &str) -> Option<String> {
    if let Ok(store) = SIGNATURE_STORE.read() {
        let sig = store.get(tool_use_id).cloned();
        if sig.is_some() {
            debug!(
                "Found stored thoughtSignature for tool_use_id: {}",
                tool_use_id
            );
        } else {
            debug!(
                "No stored thoughtSignature for tool_use_id: {}",
                tool_use_id
            );
        }
        sig
    } else {
        None
    }
}

/// Clean up old signatures to prevent memory growth
/// Call periodically or when conversation ends
pub fn cleanup_signatures(tool_use_ids: &[String]) {
    if let Ok(mut store) = SIGNATURE_STORE.write() {
        let before = store.len();
        store.retain(|k, _| tool_use_ids.contains(k));
        let removed = before - store.len();
        if removed > 0 {
            debug!("Cleaned up {} stale thoughtSignatures", removed);
        }
    }
}

/// Clear all stored signatures (for testing or session reset)
#[allow(dead_code)]
pub fn clear_all() {
    if let Ok(mut store) = SIGNATURE_STORE.write() {
        store.clear();
        debug!("Cleared all stored thoughtSignatures");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_store_and_retrieve() {
        let tool_id = "toolu_test_123";
        let signature = "CiQBjz1rX...test_signature";

        store_signature(tool_id, signature);

        let retrieved = get_signature(tool_id);
        assert_eq!(retrieved, Some(signature.to_string()));
    }

    #[test]
    fn test_missing_signature() {
        let retrieved = get_signature("nonexistent_id");
        assert_eq!(retrieved, None);
    }
}
