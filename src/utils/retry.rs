// Retry logic with Google retryDelay hint support
// Author: kelexine (https://github.com/kelexine)

use backoff::{backoff::Backoff, ExponentialBackoff};
use serde_json::Value;
use std::time::Duration;
use tracing::{debug, warn};

/// Parse Google's retryDelay duration string (e.g., "0.457639761s", "40s")
/// Returns duration in milliseconds, capped at 60 seconds
pub fn parse_retry_delay(error_json: &str) -> Option<Duration> {
    let parsed: Value = serde_json::from_str(error_json).ok()?;
    
    // Navigate: error.details[] -> find RetryInfo -> retryDelay
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

/// Parse duration strings like "0.457639761s", "40s", "1.5s"
/// Returns duration, capped at 60 seconds
fn parse_duration_string(duration_str: &str) -> Option<Duration> {
    // Remove 's' suffix and parse as float
    let seconds_str = duration_str.strip_suffix('s')?;
    let seconds: f64 = seconds_str.parse().ok()?;
    
    // Cap at 60 seconds (from Gemini CLI implementation)
    let capped_seconds = seconds.min(60.0);
    
    let millis = (capped_seconds * 1000.0) as u64;
    Some(Duration::from_millis(millis))
}

/// Create exponential backoff configuration for retries
pub fn create_backoff() -> ExponentialBackoff {
    ExponentialBackoff {
        current_interval: Duration::from_millis(500),     // Start at 500ms
        initial_interval: Duration::from_millis(500),
        randomization_factor: 0.3,                         // Add jitter
        multiplier: 2.0,                                  // Double each time
        max_interval: Duration::from_secs(30),            // Cap at 30s
        max_elapsed_time: Some(Duration::from_secs(120)), // Give up after 2 minutes
        ..Default::default()
    }
}

/// Determine if an HTTP status code is retryable
pub fn is_retryable(status: u16) -> bool {
    matches!(status, 429 | 500 | 502 | 503 | 504)
}

/// Execute operation with intelligent retry logic
/// - Uses Google's retryDelay hint if available
/// - Falls back to exponential backoff
/// - Respects max retries and timeouts
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
                if !is_retryable(status) || attempt >= MAX_ATTEMPTS {
                    // Non-retryable error or max attempts reached
                    return Err((status, error_body));
                }

                // Try to parse Google's retry hint
                let delay = if let Some(google_delay) = parse_retry_delay(&error_body) {
                    debug!(
                        "{} failed with {} (attempt {}), Google suggests waiting {}ms",
                        operation_name,
                        status,
                        attempt,
                        google_delay.as_millis()
                    );
                    google_delay
                } else {
                    // Fall back to exponential backoff
                    let backoff_delay = backoff.next_backoff().unwrap_or(Duration::from_secs(30));
                    debug!(
                        "{} failed with {} (attempt {}), retrying after {}ms",
                        operation_name,
                        status,
                        attempt,
                        backoff_delay.as_millis()
                    );
                    backoff_delay
                };

                // Wait before retry
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
        
        // Test cap at 60s
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
