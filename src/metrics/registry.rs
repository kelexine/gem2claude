// Prometheus metrics registry and collectors
// Author: kelexine (https://github.com/kelexine)

use lazy_static::lazy_static;
use prometheus::{
    CounterVec, HistogramVec, GaugeVec, Opts, Registry, TextEncoder, Encoder,
    register_counter_vec_with_registry, register_histogram_vec_with_registry,
    register_gauge_vec_with_registry,
};

lazy_static! {
    /// Global Prometheus registry
    pub static ref REGISTRY: Registry = Registry::new();

    // ============================================================================
    // REQUEST METRICS
    // ============================================================================
    
    /// Total number of API requests
    pub static ref REQUESTS_TOTAL: CounterVec = register_counter_vec_with_registry!(
        Opts::new("requests_total", "Total number of API requests"),
        &["method", "endpoint", "status_code", "model"],
        REGISTRY
    ).unwrap();

    /// Request duration histogram
    pub static ref REQUEST_DURATION: HistogramVec = register_histogram_vec_with_registry!(
        prometheus::HistogramOpts::new("request_duration_seconds", "Request duration in seconds")
            .buckets(vec![0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0]),
        &["method", "endpoint", "status_code"],
        REGISTRY
    ).unwrap();

    // ============================================================================
    // GEMINI API METRICS
    // ============================================================================
    
    /// Total Gemini API calls
    pub static ref GEMINI_API_CALLS: CounterVec = register_counter_vec_with_registry!(
        Opts::new("gemini_api_calls_total", "Total Gemini API calls"),
        &["model", "status_code", "streaming"],
        REGISTRY
    ).unwrap();

    /// Gemini API call duration
    pub static ref GEMINI_API_DURATION: HistogramVec = register_histogram_vec_with_registry!(
        prometheus::HistogramOpts::new("gemini_api_duration_seconds", "Gemini API call duration")
            .buckets(vec![0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0, 60.0]),
        &["model", "streaming"],
        REGISTRY
    ).unwrap();

    // ============================================================================
    // TOKEN METRICS
    // ============================================================================
    
    /// Total tokens processed
    pub static ref TOKENS_TOTAL: CounterVec = register_counter_vec_with_registry!(
        Opts::new("tokens_total", "Total tokens processed"),
        &["model", "type"], // type: input, output, cached_input, cached_create
        REGISTRY
    ).unwrap();

    // ============================================================================
    // CACHE METRICS
    // ============================================================================
    
    /// Cache operations
    pub static ref CACHE_OPERATIONS: CounterVec = register_counter_vec_with_registry!(
        Opts::new("cache_operations_total", "Total cache operations"),
        &["operation"], // operation: hit, miss, create
        REGISTRY
    ).unwrap();

    /// Current cache entries
    pub static ref CACHE_ENTRIES: GaugeVec = register_gauge_vec_with_registry!(
        Opts::new("cache_entries_current", "Current number of cache entries"),
        &["type"], // type: active
        REGISTRY
    ).unwrap();

    // ============================================================================
    // OAUTH METRICS
    // ============================================================================
    
    /// OAuth token refresh events
    pub static ref OAUTH_REFRESHES: CounterVec = register_counter_vec_with_registry!(
        Opts::new("oauth_token_refreshes_total", "Total OAuth token refreshes"),
        &["status"], // status: success, failure
        REGISTRY
    ).unwrap();

    /// OAuth token expiry time
    pub static ref OAUTH_TOKEN_EXPIRY: GaugeVec = register_gauge_vec_with_registry!(
        Opts::new("oauth_token_expiry_seconds", "Seconds until OAuth token expiry"),
        &["status"], // status: valid, expired
        REGISTRY
    ).unwrap();

    // ============================================================================
    // STREAMING METRICS
    // ============================================================================
    
    /// SSE events sent
    pub static ref SSE_EVENTS: CounterVec = register_counter_vec_with_registry!(
        Opts::new("sse_events_total", "Total SSE events sent"),
        &["event_type", "model"],
        REGISTRY
    ).unwrap();

    /// SSE connection events
    pub static ref SSE_CONNECTIONS: CounterVec = register_counter_vec_with_registry!(
        Opts::new("sse_connections_total", "Total SSE connections"),
        &["status"], // status: opened, closed, error
        REGISTRY
    ).unwrap();

    // ============================================================================
    // TRANSLATION METRICS
    // ============================================================================
    
    /// Translation errors
    pub static ref TRANSLATION_ERRORS: CounterVec = register_counter_vec_with_registry!(
        Opts::new("translation_errors_total", "Total translation errors"),
        &["direction", "error_type"], // direction: request, response
        REGISTRY
    ).unwrap();
}

/// Gather all metrics and return as Prometheus text format
pub fn gather_metrics() -> String {
    let encoder = TextEncoder::new();
    let metric_families = REGISTRY.gather();
    let mut buffer = Vec::new();
    encoder.encode(&metric_families, &mut buffer).unwrap();
    String::from_utf8(buffer).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_registration() {
        // Just verify metrics are registered without panicking
        let metrics = gather_metrics();
        assert!(metrics.contains("requests_total"));
        assert!(metrics.contains("gemini_api_calls_total"));
        assert!(metrics.contains("tokens_total"));
        assert!(metrics.contains("cache_operations_total"));
    }
}
