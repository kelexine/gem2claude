// Simplified cache tests - testing only public APIs
// Author: kelexine (https://github.com/kelexine)

use gem2claude::cache::{CacheConfig, CacheManager};

#[tokio::test]
async fn test_cache_stats_initialization() {
    let cache_manager = CacheManager::new(CacheConfig::default());
    let stats = cache_manager.get_stats().await;
    
    assert_eq!(stats.hits, 0);
    assert_eq!(stats.misses, 0);
    assert_eq!(stats.creates, 0);
}

#[test]
fn test_cache_config_defaults() {
    let config = CacheConfig::default();
    
    assert_eq!(config.min_tokens_for_cache, 1024);
    assert_eq!(config.max_cache_entries, 100);
    assert!(config.enabled);  // Should be enabled by default
}

#[test]
fn test_cache_config_creation() {
    let config = CacheConfig {
        enabled: true,
        min_tokens_for_cache: 2048,
        max_cache_entries: 50,
    };
    
    assert_eq!(config.min_tokens_for_cache, 2048);
    assert_eq!(config.max_cache_entries, 50);
}
