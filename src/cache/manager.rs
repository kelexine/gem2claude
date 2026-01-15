// Cache manager - handles cache key generation and lookup
// Author: kelexine (https://github.com/kelexine)

use crate::cache::models::{CacheConfig, CacheStats};
use crate::error::Result;
use crate::models::anthropic::{ContentBlock, Message, MessagesRequest, SystemPrompt};
use crate::models::gemini::GenerateContentRequest;
use crate::models::mapping::map_model;
use crate::translation::request::translate_request;
use lru::LruCache;
use parking_lot::Mutex;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::num::NonZeroUsize;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::debug;

/// Cache manager for handling Gemini context caching with LRU translation cache.
pub struct CacheManager {
    config: CacheConfig,
    /// Hash â†’ Gemini cache name mapping
    cache_map: Arc<RwLock<HashMap<String, String>>>,
    /// LRU cache for translated requests (prevents unbounded growth)
    translation_cache: Arc<Mutex<LruCache<String, GenerateContentRequest>>>,
    stats: Arc<RwLock<CacheStats>>,
}

impl CacheManager {
    /// Create a new cache manager with LRU-bounded translation cache.
    pub fn new(config: CacheConfig) -> Self {
        let capacity = NonZeroUsize::new(config.max_cache_entries).unwrap_or(NonZeroUsize::new(100).unwrap());
        
        Self {
            config,
            cache_map: Arc::new(RwLock::new(HashMap::new())),
            translation_cache: Arc::new(Mutex::new(LruCache::new(capacity))),
            stats: Arc::new(RwLock::new(CacheStats::default())),
        }
    }

    /// Generate SHA256 cache key from normalized Gemini request.
    ///
    /// Ensures deterministic hashing by:
    /// - Using the mapped Gemini model name
    /// - Sorting tools by name before serialization
    /// - Using stable JSON serialization
    fn generate_cache_key(
        &self,
        gemini_model: &str,
        gemini_request: &GenerateContentRequest,
    ) -> Result<String> {
        let mut hasher = Sha256::new();

        // Hash the mapped Gemini model name
        hasher.update(gemini_model.as_bytes());

        // Hash system instruction
        if let Some(ref sys) = gemini_request.system_instruction {
            let sys_json = serde_json::to_string(sys)
                .map_err(|e| crate::error::ProxyError::Translation(e.to_string()))?;
            hasher.update(sys_json.as_bytes());
        }

        // Hash contents (conversation history)
        let contents_json = serde_json::to_string(&gemini_request.contents)
            .map_err(|e| crate::error::ProxyError::Translation(e.to_string()))?;
        hasher.update(contents_json.as_bytes());

        // Hash tools (sorted deterministically by function name)
        if let Some(ref tools) = gemini_request.tools {
            let mut sorted_tools = tools.clone();
            for tool_decl in &mut sorted_tools {
                tool_decl.function_declarations.sort_by(|a, b| a.name.cmp(&b.name));
            }
            let tools_json = serde_json::to_string(&sorted_tools)
                .map_err(|e| crate::error::ProxyError::Translation(e.to_string()))?;
            hasher.update(tools_json.as_bytes());
        }

        // Hash thinking config (ensures different thinking settings get different keys)
        if let Some(ref gen_config) = gemini_request.generation_config {
            if let Some(ref thinking) = gen_config.thinking_config {
                let thinking_json = serde_json::to_string(thinking)
                    .map_err(|e| crate::error::ProxyError::Translation(e.to_string()))?;
                hasher.update(thinking_json.as_bytes());
            }
        }

        Ok(format!("{:x}", hasher.finalize()))
    }

    /// Estimate token count (rough approximation: 1 token â‰ˆ 4 characters)
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

    /// Get or create cache for request.
    ///
    /// Returns a tuple of (cache_name, cached_translation):
    /// - cache_name: The Gemini cache identifier if caching is enabled
    /// - cached_translation: The pre-translated request if available (for performance)
    ///
    /// # Error Handling
    /// If translation fails, returns Ok((None, None)) and falls back to normal path.
    pub async fn get_or_create_cache(
        &self,
        anthropic_req: &MessagesRequest,
        project_id: &str,
        gemini_client: &crate::gemini::GeminiClient,
    ) -> Result<(Option<String>, Option<GenerateContentRequest>)> {
        // Check if caching is enabled
        if !self.config.enabled {
            debug!("Caching disabled");
            return Ok((None, None));
        }

        // Check if request has cache_control markers
        if !Self::has_cache_control(&anthropic_req.system, &anthropic_req.messages) {
            debug!("No cache_control markers found");
            return Ok((None, None));
        }

        // Check token count
        let estimated_tokens = self.estimate_token_count(&anthropic_req.system, &anthropic_req.messages);
        if estimated_tokens < self.config.min_tokens_for_cache {
            debug!(
                "Token count {} below minimum {}",
                estimated_tokens, self.config.min_tokens_for_cache
            );
            return Ok((None, None));
        }

        // Translate request to Gemini format FIRST (for normalized hashing)
        let gemini_request = match translate_request(anthropic_req.clone(), project_id, None, None).await {
            Ok(req) => req,
            Err(e) => {
                debug!("Translation failed during cache key generation: {}", e);
                // Gracefully degrade - return no cache
                return Ok((None, None));
            }
        };

        // Map model name for consistent hashing
        let gemini_model = map_model(&anthropic_req.model)?;

        // Generate cache key from translated Gemini request
        let cache_key = match self.generate_cache_key(&gemini_model, &gemini_request) {
            Ok(key) => key,
            Err(e) => {
                debug!("Cache key generation failed: {}", e);
                return Ok((None, None));
            }
        };
        
        debug!("Generated cache key: {}", &cache_key[..16]);

        // Check if cache exists
        let cache_map = self.cache_map.read().await;
        if let Some(cache_name) = cache_map.get(&cache_key) {
            debug!("Cache hit: {}", cache_name);
            self.stats.write().await.hits += 1;
            crate::metrics::record_cache_hit();
            
            // Return cache name AND check if we have cached translation
            let cached_translation = self.translation_cache.lock().get(&cache_key).cloned();
            
            return Ok((Some(cache_name.clone()), cached_translation));
        }
        drop(cache_map);

        // Cache miss - create new cache via Gemini API
        debug!("Cache miss for key: {}", &cache_key[..16]);
        self.stats.write().await.misses += 1;
        crate::metrics::record_cache_miss();

        // Use the already-translated request to create cache
        let gemini_system = gemini_request.system_instruction.clone();
        let gemini_contents = gemini_request.contents.clone();

        // Don't create cache if we have no cacheable content
        if gemini_contents.is_empty() && gemini_system.is_none() {
            debug!("No cacheable content found, skipping cache creation");
            return Ok((None, None));
        }

        // Create cache
        match gemini_client.create_cache(&gemini_model, gemini_system, gemini_contents).await {
            Ok(cache_name) => {
                debug!("Cache created: {}", cache_name);
                
                // Store both cache mapping AND translation
                self.cache_map.write().await.insert(cache_key.clone(), cache_name.clone());
                self.translation_cache.lock().put(cache_key, gemini_request.clone());
                
                self.stats.write().await.creates += 1;
                crate::metrics::record_cache_create();
                
                Ok((Some(cache_name), Some(gemini_request)))
            }
            Err(e) => {
                debug!("Cache creation failed: {}", e);
                // Don't fail the request, just continue without caching
                Ok((None, None))
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
        self.translation_cache.lock().clear();
        debug!("Cache cleared");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::anthropic::{CacheControl, ContentBlock, Message, MessageContent, SystemPrompt};

    #[test]
    fn test_cache_key_from_translation() {
        let manager = CacheManager::new(CacheConfig::default());

        // Create a simple Gemini request
        let gemini_request = GenerateContentRequest {
            contents: vec![crate::models::gemini::Content {
                role: "user".to_string(),
                parts: vec![crate::models::gemini::Part::Text {
                    text: "Hello".to_string(),
                    thought: None,
                    thought_signature: None,
                }],
            }],
            system_instruction: Some(crate::models::gemini::SystemInstruction {
                parts: vec![crate::models::gemini::Part::Text {
                    text: "You are a helpful assistant.".to_string(),
                    thought: None,
                    thought_signature: None,
                }],
            }),
            generation_config: None,
            tools: None,
            tool_config: None,
            cached_content: None,
        };

        let key1 = manager.generate_cache_key("gemini-1.5-pro", &gemini_request).unwrap();
        let key2 = manager.generate_cache_key("gemini-1.5-pro", &gemini_request).unwrap();

        // Same inputs should produce same key
        assert_eq!(key1, key2);

        // Different model should produce different key
        let key3 = manager.generate_cache_key("gemini-1.5-flash", &gemini_request).unwrap();
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