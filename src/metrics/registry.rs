//! Prometheus metrics registry and collectors for application observability.
//!
//! This module defines the global metrics registry and the static declarations
//! for all application-level metrics, spanning request tracking, upstream API
//! performance, cache utilization, and system health.
//!
//! All metrics are registered with the global `REGISTRY` and can be gathered
//! via the `gather_metrics()` function for the `/metrics` endpoint.

// Author: kelexine (https://github.com/kelexine)

use lazy_static::lazy_static;
use prometheus::{
    register_counter_vec_with_registry, register_gauge_vec_with_registry,
    register_histogram_vec_with_registry, CounterVec, Encoder, GaugeVec, HistogramVec, Opts,
    Registry, TextEncoder,
};

lazy_static! {
    /// Global Prometheus registry for all application metrics.
    pub static ref REGISTRY: Registry = Registry::new();

    // ============================================================================
    // REQUEST METRICS (Incoming handle-level metrics)
    // ============================================================================

    /// Total number of incoming API requests.
    /// Labels:
    /// - `method`: HTTP method (e.g., POST)
    /// - `endpoint`: API endpoint (e.g., /v1/messages)
    /// - `status_code`: HTTP response status
    /// - `model`: The model requested by the client
    pub static ref REQUESTS_TOTAL: CounterVec = register_counter_vec_with_registry!(
        Opts::new("requests_total", "Total number of API requests handled by the proxy"),
        &["method", "endpoint", "status_code", "model"],
        REGISTRY
    ).unwrap();

    /// Histogram of incoming request durations.
    /// Captures the end-to-end latency of processing a client request.
    pub static ref REQUEST_DURATION: HistogramVec = register_histogram_vec_with_registry!(
        prometheus::HistogramOpts::new("request_duration_seconds", "End-to-end request duration in seconds")
            .buckets(vec![0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0]),
        &["method", "endpoint", "status_code"],
        REGISTRY
    ).unwrap();

    // ============================================================================
    // GEMINI API METRICS (Upstream client-level metrics)
    // ============================================================================

    /// Total number of calls made to the upstream Gemini API.
    /// Useful for monitoring upstream errors and success rates.
    pub static ref GEMINI_API_CALLS: CounterVec = register_counter_vec_with_registry!(
        Opts::new("gemini_api_calls_total", "Total upstream calls to the Gemini API"),
        &["model", "status_code", "streaming"],
        REGISTRY
    ).unwrap();

    /// Histogram of upstream Gemini API call durations.
    /// Monitors the performance of Google's API independently of proxy overhead.
    pub static ref GEMINI_API_DURATION: HistogramVec = register_histogram_vec_with_registry!(
        prometheus::HistogramOpts::new("gemini_api_duration_seconds", "Upstream Gemini API call latency")
            .buckets(vec![0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0, 60.0]),
        &["model", "streaming"],
        REGISTRY
    ).unwrap();

    // ============================================================================
    // TOKEN METRICS
    // ============================================================================

    /// Cumulative count of tokens processed by the system.
    /// Categorized by model and usage type (input, output, cached).
    pub static ref TOKENS_TOTAL: CounterVec = register_counter_vec_with_registry!(
        Opts::new("tokens_total", "Cumulative token throughput"),
        &["model", "type"], // type: input, output, cached_input, cached_create
        REGISTRY
    ).unwrap();

    // ============================================================================
    // CACHE METRICS (Gemini Context Caching)
    // ============================================================================

    /// Counter for Gemini Context Cache operations.
    /// Tracks hits, misses, and creation events for upstream caching.
    pub static ref CACHE_OPERATIONS: CounterVec = register_counter_vec_with_registry!(
        Opts::new("cache_operations_total", "Upstream context cache operations"),
        &["operation"], // operation: hit, miss, create
        REGISTRY
    ).unwrap();

    /// Gauge representing the current number of active context caches.
    pub static ref CACHE_ENTRIES: GaugeVec = register_gauge_vec_with_registry!(
        Opts::new("cache_entries_current", "Current number of active context caches in the system"),
        &["type"], // type: active
        REGISTRY
    ).unwrap();

    // ============================================================================
    // OAUTH METRICS
    // ============================================================================

    /// Tracks success and failure of token refresh attempts.
    pub static ref OAUTH_REFRESHES: CounterVec = register_counter_vec_with_registry!(
        Opts::new("oauth_token_refreshes_total", "Google OAuth2 token lifecycle events"),
        &["status"], // status: success, failure
        REGISTRY
    ).unwrap();

    /// Gauge of the current access token's remaining validity time.
    pub static ref OAUTH_TOKEN_EXPIRY: GaugeVec = register_gauge_vec_with_registry!(
        Opts::new("oauth_token_expiry_seconds", "Seconds remaining until current OAuth token expires"),
        &["status"], // status: valid, expired
        REGISTRY
    ).unwrap();

    // ============================================================================
    // STREAMING METRICS (SSE)
    // ============================================================================

    /// Count of Server-Sent Events emitted to clients.
    /// Useful for analyzing response verbosity and event distribution.
    pub static ref SSE_EVENTS: CounterVec = register_counter_vec_with_registry!(
        Opts::new("sse_events_total", "Total Server-Sent Events emitted to the client"),
        &["event_type", "model"],
        REGISTRY
    ).unwrap();

    /// Tracks streaming connection lifecycle states.
    pub static ref SSE_CONNECTIONS: CounterVec = register_counter_vec_with_registry!(
        Opts::new("sse_connections_total", "Streaming connection lifecycle events"),
        &["status"], // status: opened, closed, error
        REGISTRY
    ).unwrap();

    // ============================================================================
    // TRANSLATION METRICS (Internal transformation logic)
    // ============================================================================

    /// Records failures during Anthropic-to-Gemini (and vice versa) translation.
    pub static ref TRANSLATION_ERRORS: CounterVec = register_counter_vec_with_registry!(
        Opts::new("translation_errors_total", "Errors during cross-API request/response translation"),
        &["direction", "error_type"], // direction: request, response
        REGISTRY
    ).unwrap();

    /// Operations on the internal LRU cache for translations.
    pub static ref TRANSLATION_CACHE_OPERATIONS: CounterVec = register_counter_vec_with_registry!(
        Opts::new("translation_cache_operations_total", "Internal translation LRU cache operations"),
        &["operation"], // operation: hit, miss, eviction
        REGISTRY
    ).unwrap();

    // ============================================================================
    // AVAILABILITY & RATE LIMIT METRICS
    // ============================================================================

    /// Current reported health of models based on recent upstream responses.
    /// 1 = Available/Healthy, 0 = Marked for Retry or Terminal Error.
    pub static ref GEMINI_MODEL_AVAILABILITY: GaugeVec = register_gauge_vec_with_registry!(
        Opts::new("gemini_model_availability", "Reported upstream model health status (1=Available, 0=Unavailable)"),
        &["model", "status"], // status: healthy, sticky_retry, terminal
        REGISTRY
    ).unwrap();

    /// Total number of automatic retries performed by the client.
    pub static ref GEMINI_RETRIES: CounterVec = register_counter_vec_with_registry!(
        Opts::new("gemini_retries_total", "Automatic client-side retries for failed upstream calls"),
        &["model", "reason"], // reason: 429, 5xx, timeout, etc.
        REGISTRY
    ).unwrap();

    /// Wait duration spent due to 429 Rate Limit responses.
    pub static ref GEMINI_RATE_LIMIT_WAIT_SECONDS: HistogramVec = register_histogram_vec_with_registry!(
        prometheus::HistogramOpts::new("gemini_rate_limit_wait_seconds", "Total time spent waiting on rate limits")
            .buckets(vec![1.0, 5.0, 10.0, 30.0, 60.0, 120.0, 300.0]),
        &["model"],
        REGISTRY
    ).unwrap();
}

/// Gathers all registered metrics into a single Prometheus-formatted string.
///
/// This is intended to be served by the `/metrics` endpoint for Prometheus scraping.
pub fn gather_metrics() -> String {
    let encoder = TextEncoder::new();
    let metric_families = REGISTRY.gather();
    let mut buffer = Vec::new();
    encoder
        .encode(&metric_families, &mut buffer)
        .expect("Failed to encode metrics");
    String::from_utf8(buffer).expect("Metrics contain invalid UTF-8")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_registration() {
        // Initialize metrics by incrementing a counter (triggers lazy_static)
        REQUESTS_TOTAL
            .with_label_values(&["GET", "/test", "200", "test-model"])
            .inc();

        // Now gather metrics
        let metrics = gather_metrics();
        assert!(!metrics.is_empty(), "Metrics should be generated");
        assert!(
            metrics.contains("requests_total"),
            "Should contain requests_total metric"
        );
    }
}
