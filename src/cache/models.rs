// Cache configuration and statistics models
// Author: kelexine (https://github.com/kelexine)

/// Cache configuration
#[derive(Debug, Clone)]
pub struct CacheConfig {
    pub enabled: bool,
    pub min_tokens_for_cache: usize,
    pub max_cache_entries: usize,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            min_tokens_for_cache: 1024,
            max_cache_entries: 100,
        }
    }
}

/// Cache statistics
#[derive(Debug, Default, Clone)]
pub struct CacheStats {
    pub hits: u64,
    pub misses: u64,
    pub creates: u64,
}
