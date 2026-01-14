// Cache manager - handles cache key generation and lookup
// Author: kelexine (https://github.com/kelexine)

use crate::cache::models::{CacheConfig, CacheStats};
use crate::error::{ProxyError, Result};
use crate::models::anthropic::{ContentBlock, Message, SystemPrompt};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::debug;

/// Cache manager for handling Gemini context caching
pub struct CacheManager {
    config: CacheConfig,
    /// Hash → Gemini cache name mapping
    cache_map: Arc<RwLock<HashMap<String, String>>>,
    stats: Arc<RwLock<CacheStats>>,
}

impl CacheManager {
    /// Create a new cache manager
    pub fn new(config: CacheConfig) -> Self {
        Self {
            config,
            cache_map: Arc::new(RwLock::new(HashMap::new())),
            stats: Arc::new(RwLock::new(CacheStats::default())),
        }
    }

    /// Generate SHA256 cache key from request components
    fn generate_cache_key(
        &self,
        model: &str,
        system: &Option<SystemPrompt>,
        messages: &[Message],
    ) -> String {
        let mut hasher = Sha256::new();

        // Hash model
        hasher.update(model.as_bytes());

        // Hash system prompt
        if let Some(sys) = system {
            hasher.update(serde_json::to_string(sys).unwrap_or_default().as_bytes());
        }

        // Hash messages
        hasher.update(serde_json::to_string(&messages).unwrap_or_default().as_bytes());

        format!("{:x}", hasher.finalize())
    }

    /// Estimate token count (rough approximation: 1 token ≈ 4 characters)
    fn estimate_token_count(&self, system: &Option<SystemPrompt>, messages: &[Message]) -> usize {
        let mut total_chars = 0;

        // Count system prompt characters
        if let Some(sys) = system {
            total_chars += sys.to_text().len();
        }

        // Count message characters
        for msg in messages {
            total_chars += serde_json::to_string(&msg.content).unwrap_or_default().len();
        }

        total_chars / 4
    }

    /// Check if request has cache_control markers
    fn has_cache_control(system: &Option<SystemPrompt>, messages: &[Message]) -> bool {
        // Check system prompt blocks
        if let Some(SystemPrompt::Blocks(blocks)) = system {
            if blocks.iter().any(|b| Self::block_has_cache_control(b)) {
                return true;
            }
        }

        // Check message content blocks
        messages.iter().any(|msg| match &msg.content {
            crate::models::anthropic::MessageContent::Blocks(blocks) => {
                blocks.iter().any(|b| Self::block_has_cache_control(b))
            }
            _ => false,
        })
    }

    /// Check if a content block has cache_control
    fn block_has_cache_control(block: &ContentBlock) -> bool {
        match block {
            ContentBlock::Text { cache_control, .. } => cache_control.is_some(),
            ContentBlock::Image { cache_control, .. } => cache_control.is_some(),
            ContentBlock::ToolUse { cache_control, .. } => cache_control.is_some(),
            _ => false,
        }
    }

    /// Get or create cache for request
    /// Returns cache name if caching should be used, None otherwise
    pub async fn get_or_create_cache(
        &self,
        model: &str,
        system: &Option<SystemPrompt>,
        messages: &[Message],
        gemini_client: &crate::gemini::GeminiClient,
    ) -> Result<Option<String>> {
        // Check if caching is enabled
        if !self.config.enabled {
            debug!("Caching disabled");
            return Ok(None);
        }

        // Check if request has cache_control markers
        if !Self::has_cache_control(system, messages) {
            debug!("No cache_control markers found");
            return Ok(None);
        }

        // Check token count
        let estimated_tokens = self.estimate_token_count(system, messages);
        if estimated_tokens < self.config.min_tokens_for_cache {
            debug!(
                "Token count {} below minimum {}",
                estimated_tokens, self.config.min_tokens_for_cache
            );
            return Ok(None);
        }

        // Generate cache key
        let cache_key = self.generate_cache_key(model, system, messages);
        debug!("Generated cache key: {}", &cache_key[..16]);

        // Check if cache exists
        let cache_map = self.cache_map.read().await;
        if let Some(cache_name) = cache_map.get(&cache_key) {
            debug!("Cache hit: {}", cache_name);
            self.stats.write().await.hits += 1;
            return Ok(Some(cache_name.clone()));
        }
        drop(cache_map);

        // Cache miss - create new cache via Gemini API
        debug!("Cache miss for key: {}", &cache_key[..16]);
        self.stats.write().await.misses += 1;

        // Translate system and messages to Gemini format
        let gemini_system = system.as_ref().map(|s| {
            crate::models::gemini::SystemInstruction {
                parts: vec![crate::models::gemini::Part::Text {
                    text: s.to_text(),
                    thought: None,
                    thought_signature: None,
                }],
            }
        });

        let gemini_contents: Vec<crate::models::gemini::Content> = messages
            .iter()
            .filter_map(|msg| {
                // Only include cacheable content (up to cache_control marker)
                match &msg.content {
                    crate::models::anthropic::MessageContent::Text(t) => {
                        Some(crate::models::gemini::Content {
                            role: if msg.role == "user" { "user" } else { "model" }.to_string(),
                            parts: vec![crate::models::gemini::Part::Text {
                                text: t.clone(),
                                thought: None,
                                thought_signature: None,
                            }],
                        })
                    }
                    _ => None,
                }
            })
            .collect();

        // Don't create cache if we have no cacheable content
        if gemini_contents.is_empty() && gemini_system.is_none() {
            debug!("No cacheable content found, skipping cache creation");
            return Ok(None);
        }

        // Ensure we have at least one content or system instruction with parts
        if gemini_contents.is_empty() {
            // If no messages but we have system instruction, create a minimal user message
            // to satisfy Gemini's requirement of at least one parts field
            debug!("Creating minimal content for cache with only system instruction");
        }

        // Create cache
        match gemini_client.create_cache(model, gemini_system, gemini_contents).await {
            Ok(cache_name) => {
                debug!("Cache created: {}", cache_name);
                self.cache_map.write().await.insert(cache_key, cache_name.clone());
                self.stats.write().await.creates += 1;
                Ok(Some(cache_name))
            }
            Err(e) => {
                debug!("Cache creation failed: {}", e);
                // Don't fail the request, just continue without caching
                Ok(None)
            }
        }
    }

    /// Get cache statistics
    pub async fn get_stats(&self) -> CacheStats {
        self.stats.read().await.clone()
    }

    /// Clear all cached entries
    pub async fn clear(&self) {
        self.cache_map.write().await.clear();
        debug!("Cache cleared");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::anthropic::{CacheControl, ContentBlock, Message, MessageContent, SystemPrompt};

    #[test]
    fn test_cache_key_generation() {
        let manager = CacheManager::new(CacheConfig::default());

        let system = Some(SystemPrompt::Text("You are a helpful assistant.".to_string()));
        let messages = vec![Message {
            role: "user".to_string(),
            content: MessageContent::Text("Hello".to_string()),
        }];

        let key1 = manager.generate_cache_key("claude-sonnet-4-5", &system, &messages);
        let key2 = manager.generate_cache_key("claude-sonnet-4-5", &system, &messages);

        // Same inputs should produce same key
        assert_eq!(key1, key2);

        // Different model should produce different key
        let key3 = manager.generate_cache_key("claude-haiku-4-5", &system, &messages);
        assert_ne!(key1, key3);
    }

    #[test]
    fn test_has_cache_control() {
        let system = Some(SystemPrompt::Blocks(vec![
            ContentBlock::Text {
                text: "System prompt".to_string(),
                cache_control: Some(CacheControl {
                    cache_type: "ephemeral".to_string(),
                }),
            },
        ]));

        let messages = vec![];

        assert!(CacheManager::has_cache_control(&system, &messages));

        let no_cache_system = Some(SystemPrompt::Text("Hello".to_string()));
        assert!(!CacheManager::has_cache_control(&no_cache_system, &messages));
    }
}
