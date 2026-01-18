//! Cache manager for Gemini context caching.
//!
//! This module implements the `CacheManager`, which is responsible for managing
//! the lifecycle of context caches.
//!
//! # Note on Internal API Caching
//! Google's internal Gemini API (v1internal), which this bridge uses, implements
//! "automatic caching" server-side based on prompt prefixes. Unlike the public API,
//! it does not require explicit `/cachedContents` management. The `CacheManager`
//! in this version of `gem2claude` is primarily used to track metrics and
//! provide a consistent interface for future extensions.

// Author: kelexine (https://github.com/kelexine)

use crate::cache::models::{CacheConfig, CacheStats};
use crate::error::Result;
use crate::models::anthropic::MessagesRequest;
use crate::models::gemini::GenerateContentRequest;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::debug;

/// Manages context cache entries and usage statistics.
///
/// The `CacheManager` uses thread-safe primitives to track performance metrics
/// across concurrent requests.
pub struct CacheManager {
    /// Configuration for cache behavior and thresholds.
    config: CacheConfig,
    /// Thread-safe accumulator for cache performance metrics.
    stats: Arc<RwLock<CacheStats>>,
}

impl CacheManager {
    /// Initializes a new `CacheManager` with the provided configuration.
    pub fn new(config: CacheConfig) -> Self {
        Self {
            config,
            stats: Arc::new(RwLock::new(CacheStats::default())),
        }
    }

    /// Evaluates if a request should use a cache entry or create a new one.
    ///
    /// For the internal API, this currently returns `(None, None)` as caching
    /// is handled transparently by the Google Cloud backend. Usage is reported
    /// via the `cachedContentTokenCount` field in the response metadata.
    ///
    /// # Arguments
    ///
    /// * `_anthropic_req` - The original Anthropic request.
    /// * `_project_id` - The Google Cloud Project ID.
    /// * `_gemini_client` - Reference to the Gemini client for any needed API calls.
    pub async fn get_or_create_cache(
        &self,
        _anthropic_req: &MessagesRequest,
        _project_id: &str,
        _gemini_client: &crate::gemini::GeminiClient,
    ) -> Result<(Option<String>, Option<GenerateContentRequest>)> {
        // Check if caching is enabled in configuration.
        if !self.config.enabled {
            debug!("Request-level caching is explicitly disabled in configuration.");
            return Ok((None, None));
        }

        // Context: gem2claude uses the internal v1internal API.
        // This API performs automatic prefix-based caching. Manual cache injection
        // via `cached_content` fields is not required and often unsupported.
        debug!("Internal API detected: Relying on server-side automatic pruning/caching.");
        Ok((None, None))
    }

    /// Returns a snapshot of the current cache performance statistics.
    pub async fn get_stats(&self) -> CacheStats {
        self.stats.read().await.clone()
    }
}
