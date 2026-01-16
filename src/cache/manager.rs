// Cache manager - simplified for internal API automatic caching
//
// Author: kelexine (https://github.com/kelexine)
//
// The internal Gemini API (v1internal) handles caching automatically server-side.
// This module exists only to maintain API compatibility but doesn't perform
// client-side cache management.

use crate::cache::models::{CacheConfig, CacheStats};
use crate::error::Result;
use crate::models::anthropic::MessagesRequest;
use crate::models::gemini::GenerateContentRequest;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::debug;

/// Cache manager for Gemini context caching.
///
/// Note: The internal API handles caching automatically server-side.
/// This manager is simplified and always returns None for cache operations.
pub struct CacheManager {
    config: CacheConfig,
    stats: Arc<RwLock<CacheStats>>,
}

impl CacheManager {
    /// Create a new cache manager.
    pub fn new(config: CacheConfig) -> Self {
        Self {
            config,
            stats: Arc::new(RwLock::new(CacheStats::default())),
        }
    }

    /// Get or create cache for request.
    ///
    /// Always returns (None, None) since the internal API handles caching server-side.
    /// The API automatically caches content and reports usage via `cachedContentTokenCount`.
    pub async fn get_or_create_cache(
        &self,
        _anthropic_req: &MessagesRequest,
        _project_id: &str,
        _gemini_client: &crate::gemini::GeminiClient,
    ) -> Result<(Option<String>, Option<GenerateContentRequest>)> {
        // Check if caching is enabled
        if !self.config.enabled {
            debug!("Caching disabled: Server handles caching automatically");
            return Ok((None, None));
        }

        // IMPORTANT: The internal Gemini API (v1internal) handles caching automatically server-side.
        // It does NOT support the public API's /cachedContents endpoint.
        // Caching is automatic and reported via `cachedContentTokenCount` in usage metadata.
        // gem2claude uses the internal API, so client-side cache creation is disabled.
        debug!("Using internal API - cache creation not supported, server handles caching automatically");
        Ok((None, None))
    }

    /// Get cache statistics
    pub async fn get_stats(&self) -> CacheStats {
        self.stats.read().await.clone()
    }
}