//! Metrics module for Prometheus observability.
//!
//! This module provides high-level helper functions for recording application
//! metrics without needing to interact directly with the Prometheus registry.
//! It abstracts the label management and ensures consistent recording of
//! system events.

// Author: kelexine (https://github.com/kelexine)

mod registry;

pub use registry::{
    gather_metrics, CACHE_ENTRIES, CACHE_OPERATIONS, GEMINI_API_CALLS, GEMINI_API_DURATION,
    GEMINI_MODEL_AVAILABILITY, GEMINI_RATE_LIMIT_WAIT_SECONDS, GEMINI_RETRIES, OAUTH_REFRESHES,
    OAUTH_TOKEN_EXPIRY, REQUESTS_TOTAL, REQUEST_DURATION, SSE_CONNECTIONS, SSE_EVENTS,
    TOKENS_TOTAL, TRANSLATION_CACHE_OPERATIONS, TRANSLATION_ERRORS,
};

/// Records an incoming HTTP request's completion status and latency.
///
/// This increments the `REQUESTS_TOTAL` counter and observes the `REQUEST_DURATION`
/// histogram with the provided labels.
pub fn record_request(
    method: &str,
    endpoint: &str,
    status_code: u16,
    model: &str,
    duration_secs: f64,
) {
    REQUESTS_TOTAL
        .with_label_values(&[method, endpoint, &status_code.to_string(), model])
        .inc();

    REQUEST_DURATION
        .with_label_values(&[method, endpoint, &status_code.to_string()])
        .observe(duration_secs);
}

/// Records a call to the upstream Gemini API.
///
/// Tracks the model used, the HTTP status code returned by Google, whether
/// it was a streaming request, and the round-trip latency.
pub fn record_gemini_call(model: &str, status_code: u16, streaming: bool, duration_secs: f64) {
    GEMINI_API_CALLS
        .with_label_values(&[model, &status_code.to_string(), &streaming.to_string()])
        .inc();

    GEMINI_API_DURATION
        .with_label_values(&[model, &streaming.to_string()])
        .observe(duration_secs);
}

/// Records token usage statistics for a request.
///
/// Updates the `TOKENS_TOTAL` counter for each type of token provided:
/// - `input`: Tokens sent in the request prompt.
/// - `output`: Tokens generated in the response.
/// - `cached_input`: Tokens read from an existing upstream cache.
/// - `cached_create`: Tokens used to create a new upstream cache entry.
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

/// Records a Gemini context cache hit.
pub fn record_cache_hit() {
    CACHE_OPERATIONS.with_label_values(&["hit"]).inc();
}

/// Records a Gemini context cache miss.
pub fn record_cache_miss() {
    CACHE_OPERATIONS.with_label_values(&["miss"]).inc();
}

/// Records the creation of a new Gemini context cache entry.
pub fn record_cache_create() {
    CACHE_OPERATIONS.with_label_values(&["create"]).inc();
}

/// Updates the gauge for current active context cache entries.
pub fn update_cache_entries(count: usize) {
    CACHE_ENTRIES
        .with_label_values(&["active"])
        .set(count as f64);
}

/// Records a hit in the internal translation LRU cache.
pub fn record_translation_cache_hit() {
    TRANSLATION_CACHE_OPERATIONS
        .with_label_values(&["hit"])
        .inc();
}

/// Records a miss in the internal translation LRU cache.
pub fn record_translation_cache_miss() {
    TRANSLATION_CACHE_OPERATIONS
        .with_label_values(&["miss"])
        .inc();
}

/// Records an eviction from the internal translation LRU cache.
pub fn record_translation_cache_eviction() {
    TRANSLATION_CACHE_OPERATIONS
        .with_label_values(&["eviction"])
        .inc();
}

/// Records the result of a Google OAuth2 token refresh attempt.
pub fn record_oauth_refresh(success: bool) {
    let status = if success { "success" } else { "failure" };
    OAUTH_REFRESHES.with_label_values(&[status]).inc();
}

/// Updates the gauge for the current OAuth2 token's remaining time to live.
pub fn update_oauth_expiry(seconds: i64) {
    let status = if seconds > 0 { "valid" } else { "expired" };
    OAUTH_TOKEN_EXPIRY
        .with_label_values(&[status])
        .set(seconds as f64);
}

/// Records a Server-Sent Event (SSE) being sent to a client.
pub fn record_sse_event(event_type: &str, model: &str) {
    SSE_EVENTS.with_label_values(&[event_type, model]).inc();
}

/// Records a change in SSE connection status (e.g., opened, closed, error).
pub fn record_sse_connection(status: &str) {
    SSE_CONNECTIONS.with_label_values(&[status]).inc();
}

/// Records a translation failure between Anthropic and Gemini formats.
pub fn record_translation_error(direction: &str, error_type: &str) {
    TRANSLATION_ERRORS
        .with_label_values(&[direction, error_type])
        .inc();
}

/// Updates the reported availability status of a model.
///
/// This sets the gauge for the current status to 1.0 and resets all other
/// unique statuses to 0.0 to ensure a single active state per model.
pub fn record_model_health(model: &str, status: &str, unique_statuses: &[&str]) {
    // Reset other statuses to 0 so we only have one "1" per model
    for s in unique_statuses {
        let val = if *s == status { 1.0 } else { 0.0 };
        GEMINI_MODEL_AVAILABILITY
            .with_label_values(&[model, s])
            .set(val);
    }
}

/// Records a client-side retry attempt for a specific model and reason.
pub fn record_retry_attempt(model: &str, reason: &str) {
    GEMINI_RETRIES.with_label_values(&[model, reason]).inc();
}

/// Observes the duration waited due to upstream rate limiting (429).
pub fn record_rate_limit_wait(model: &str, duration_secs: f64) {
    GEMINI_RATE_LIMIT_WAIT_SECONDS
        .with_label_values(&[model])
        .observe(duration_secs);
}
