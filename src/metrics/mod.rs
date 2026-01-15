// Metrics module for Prometheus observability
// Author: kelexine (https://github.com/kelexine)

mod registry;

pub use registry::{
    gather_metrics,
    REQUESTS_TOTAL,
    REQUEST_DURATION,
    GEMINI_API_CALLS,
    GEMINI_API_DURATION,
    TOKENS_TOTAL,
    CACHE_OPERATIONS,
    CACHE_ENTRIES,
    OAUTH_REFRESHES,
    OAUTH_TOKEN_EXPIRY,
    SSE_EVENTS,
    SSE_CONNECTIONS,
    TRANSLATION_ERRORS,
    TRANSLATION_CACHE_OPERATIONS,
};

/// Helper to record request metrics
pub fn record_request(method: &str, endpoint: &str, status_code: u16, model: &str, duration_secs: f64) {
    REQUESTS_TOTAL
        .with_label_values(&[method, endpoint, &status_code.to_string(), model])
        .inc();
    
    REQUEST_DURATION
        .with_label_values(&[method, endpoint, &status_code.to_string()])
        .observe(duration_secs);
}

/// Helper to record Gemini API call metrics
pub fn record_gemini_call(model: &str, status_code: u16, streaming: bool, duration_secs: f64) {
    GEMINI_API_CALLS
        .with_label_values(&[model, &status_code.to_string(), &streaming.to_string()])
        .inc();
    
    GEMINI_API_DURATION
        .with_label_values(&[model, &streaming.to_string()])
        .observe(duration_secs);
}

/// Helper to record token usage
pub fn record_tokens(model: &str, input: u32, output: u32, cached_input: u32, cached_create: u32) {
    if input > 0 {
        TOKENS_TOTAL
            .with_label_values(&[model, "input"])
            .inc_by(input as f64);
    }
    if output > 0 {
        TOKENS_TOTAL
            .with_label_values(&[model, "output"])
            .inc_by(output as f64);
    }
    if cached_input > 0 {
        TOKENS_TOTAL
            .with_label_values(&[model, "cached_input"])
            .inc_by(cached_input as f64);
    }
    if cached_create > 0 {
        TOKENS_TOTAL
            .with_label_values(&[model, "cached_create"])
            .inc_by(cached_create as f64);
    }
}

/// Helper to record cache operations (Gemini context cache)
pub fn record_cache_hit() {
    CACHE_OPERATIONS.with_label_values(&["hit"]).inc();
}

pub fn record_cache_miss() {
    CACHE_OPERATIONS.with_label_values(&["miss"]).inc();
}

pub fn record_cache_create() {
    CACHE_OPERATIONS.with_label_values(&["create"]).inc();
}

pub fn update_cache_entries(count: usize) {
    CACHE_ENTRIES.with_label_values(&["active"]).set(count as f64);
}

/// Helper to record translation cache operations (LRU in-memory cache)
pub fn record_translation_cache_hit() {
    TRANSLATION_CACHE_OPERATIONS.with_label_values(&["hit"]).inc();
}

pub fn record_translation_cache_miss() {
    TRANSLATION_CACHE_OPERATIONS.with_label_values(&["miss"]).inc();
}

pub fn record_translation_cache_eviction() {
    TRANSLATION_CACHE_OPERATIONS.with_label_values(&["eviction"]).inc();
}

/// Helper to record OAuth metrics
pub fn record_oauth_refresh(success: bool) {
    let status = if success { "success" } else { "failure" };
    OAUTH_REFRESHES.with_label_values(&[status]).inc();
}

pub fn update_oauth_expiry(seconds: i64) {
    let status = if seconds > 0 { "valid" } else { "expired" };
    OAUTH_TOKEN_EXPIRY.with_label_values(&[status]).set(seconds as f64);
}

/// Helper to record SSE events
pub fn record_sse_event(event_type: &str, model: &str) {
    SSE_EVENTS.with_label_values(&[event_type, model]).inc();
}

pub fn record_sse_connection(status: &str) {
    SSE_CONNECTIONS.with_label_values(&[status]).inc();
}

/// Helper to record translation errors
pub fn record_translation_error(direction: &str, error_type: &str) {
    TRANSLATION_ERRORS.with_label_values(&[direction, error_type]).inc();
}
