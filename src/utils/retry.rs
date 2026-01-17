//! Intelligent retry mechanisms with upstream API hint support.
//!
//! This module provides an asynchronous retry wrapper that combines standard
//! exponential backoff with support for Google's specific `retryDelay` hints
//! returned in error details. This ensures the bridge is a "good citizen" to
//! the upstream Gemini API, avoiding aggressive retries when explicitly told to wait.
//!
//! Author: kelexine (<https://github.com/kelexine>)

use backoff::{backoff::Backoff, ExponentialBackoff};
use serde_json::Value;
use std::time::Duration;
use tracing::debug;

/// Parses Google's `retryDelay` duration from a JSON error response.
///
/// Google APIs often return a `RetryInfo` detail block in the error response
/// when a request is rate limited or the service is overloaded.
///
/// # Arguments
///
/// * `error_json` - The raw JSON body of the error response.
///
/// # Returns
///
/// `Some(Duration)` if a valid `retryDelay` was found, otherwise `None`.
pub fn parse_retry_delay(error_json: &str) -> Option<Duration> {
    let parsed: Value = serde_json::from_str(error_json).ok()?;

    // Navigate the Google RPC error structure: error.details[] -> RetryInfo -> retryDelay
    let details = parsed.get("error")?.get("details")?.as_array()?;

    for detail in details {
        if detail.get("@type")?.as_str()? == "type.googleapis.com/google.rpc.RetryInfo" {
            if let Some(retry_delay) = detail.get("retryDelay").and_then(|v| v.as_str()) {
                return parse_duration_string(retry_delay);
            }
        }
    }

    None
}

/// Parses a Protobuf-style duration string into a `std::time::Duration`.
///
/// Supports strings like "0.45s", "40s", etc.
/// Results are capped at 60 seconds to prevent excessively long hangs.
fn parse_duration_string(duration_str: &str) -> Option<Duration> {
    // Expected format: <float>s
    let seconds_str = duration_str.strip_suffix('s')?;
    let seconds: f64 = seconds_str.parse().ok()?;

    // Safety cap: never wait more than 60s regardless of what the API says
    let capped_seconds = seconds.min(60.0);

    let millis = (capped_seconds * 1000.0) as u64;
    Some(Duration::from_millis(millis))
}

/// Creates a default exponential backoff configuration.
///
/// * **Initial Interval**: 500ms
/// * **Multiplier**: 2.0x
/// * **Randomization**: 30% jitter to prevent thundering herds.
/// * **Max Interval**: 30s
/// * **Max Elapsed Time**: 2 minutes
pub fn create_backoff() -> ExponentialBackoff {
    ExponentialBackoff {
        current_interval: Duration::from_millis(500),
        initial_interval: Duration::from_millis(500),
        randomization_factor: 0.3,
        multiplier: 2.0,
        max_interval: Duration::from_secs(30),
        max_elapsed_time: Some(Duration::from_secs(120)),
        ..Default::default()
    }
}

/// Determines if an HTTP status code warrants a retry.
///
/// Retryable codes:
/// * **429**: Too Many Requests (Rate Limited)
/// * **500**: Internal Server Error (Transient)
/// * **502**: Bad Gateway
/// * **503**: Service Unavailable
/// * **504**: Gateway Timeout
pub fn is_retryable(status: u16) -> bool {
    matches!(status, 429 | 500 | 502 | 503 | 504)
}

/// Executes an asynchronous operation with intelligent retry logic.
///
/// This is the primary entry point for making resilient API calls.
/// It wraps an operation in a loop that:
/// 1. Executes the provided `operation`.
/// 2. If it fails with a retryable error, it looks for a Google `retryDelay` hint.
/// 3. If no hint is found, it uses exponential backoff with jitter.
/// 4. Waits for the calculated duration before trying again.
/// 5. Gives up after `MAX_ATTEMPTS` (default: 5).
///
/// # Arguments
///
/// * `operation_name` - Descriptive name used for logging retry attempts.
/// * `operation` - A closure that returns a `Future` yielding `Result<T, (u16, String)>`.
pub async fn with_retry<F, Fut, T>(
    operation_name: &str,
    mut operation: F,
) -> Result<T, (u16, String)>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T, (u16, String)>>,
{
    let mut backoff = create_backoff();
    let mut attempt = 0;
    const MAX_ATTEMPTS: u32 = 5;

    loop {
        attempt += 1;

        match operation().await {
            Ok(result) => {
                if attempt > 1 {
                    debug!("{} succeeded on attempt {}", operation_name, attempt);
                }
                return Ok(result);
            }
            Err((status, error_body)) => {
                // Determine if we should attempt another retry
                if !is_retryable(status) || attempt >= MAX_ATTEMPTS {
                    return Err((status, error_body));
                }

                // Calculate wait duration
                let delay =
                    if let Some(google_delay) = parse_retry_delay(&error_body) {
                        // API explicitly told us how long to wait
                        debug!(
                        "{} failed with {} (attempt {}), using Google's suggested delay of {}ms",
                        operation_name, status, attempt, google_delay.as_millis()
                    );
                        google_delay
                    } else {
                        // Fall back to autonomous exponential backoff
                        let backoff_delay =
                            backoff.next_backoff().unwrap_or(Duration::from_secs(30));
                        debug!(
                            "{} failed with {} (attempt {}), retrying after backoff of {}ms",
                            operation_name,
                            status,
                            attempt,
                            backoff_delay.as_millis()
                        );
                        backoff_delay
                    };

                tokio::time::sleep(delay).await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_retry_delay() {
        let error_json = r#"{
  "error": {
    "code": 429,
    "message": "Rate limited",
    "details": [
      {
        "@type": "type.googleapis.com/google.rpc.RetryInfo",
        "retryDelay": "0.457639761s"
      }
    ]
  }
}"#;
        let delay = parse_retry_delay(error_json).unwrap();
        assert_eq!(delay.as_millis(), 457);
    }

    #[test]
    fn test_parse_duration_string() {
        assert_eq!(parse_duration_string("40s").unwrap().as_secs(), 40);
        assert_eq!(parse_duration_string("1.5s").unwrap().as_millis(), 1500);
        assert_eq!(parse_duration_string("0.123s").unwrap().as_millis(), 123);

        // Test safety cap at 60s
        assert_eq!(parse_duration_string("120s").unwrap().as_secs(), 60);
    }

    #[test]
    fn test_is_retryable() {
        assert!(is_retryable(429));
        assert!(is_retryable(500));
        assert!(is_retryable(502));
        assert!(is_retryable(503));
        assert!(!is_retryable(400));
        assert!(!is_retryable(404));
    }
}
