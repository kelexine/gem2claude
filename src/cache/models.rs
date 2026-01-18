//! Cache configuration and statistics models.

// Author: kelexine (https://github.com/kelexine)

/// Configuration for the context caching system.
#[derive(Debug, Clone)]
pub struct CacheConfig {
    /// Whether caching is enabled.
    pub enabled: bool,
    /// Minimum threshold of tokens required before a request is considered for caching.
    pub min_tokens_for_cache: usize,
    /// Maximum number of active cache entries to track.
    pub max_cache_entries: usize,
}

impl Default for CacheConfig {
    /// Provides default values for cache configuration.
    ///
    /// - `enabled`: true
    /// - `min_tokens_for_cache`: 1024
    /// - `max_cache_entries`: 100
    fn default() -> Self {
        Self {
            enabled: true,
            min_tokens_for_cache: 1024,
            max_cache_entries: 100,
        }
    }
}

/// Statistics for cache operations.
#[derive(Debug, Default, Clone)]
pub struct CacheStats {
    /// Number of successful cache hits.
    pub hits: u64,
    /// Number of cache misses.
    pub misses: u64,
    /// Number of newly created cache entries.
    pub creates: u64,
}
