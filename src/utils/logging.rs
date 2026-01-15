//! Structured logging and security-focused trace utilities.
//!
//! This module configures the `tracing` ecosystem for the application,
//! supporting multiple output formats and providing utilities to prevent
//! sensitive data (like API tokens) from leaking into logs.
//!
//! Author: kelexine (<https://github.com/kelexine>)

use crate::config::LoggingConfig;
use crate::error::Result;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

/// Initializes the global tracing subscriber for the application.
///
/// Supports two output formats:
/// - `json`: Structured JSON logs for production ingestion.
/// - `pretty` (default): Human-readable, colorized output for development.
///
/// Log levels are controlled via the `RUST_LOG` environment variable or
/// the provided `LoggingConfig`.
pub fn init(config: &LoggingConfig) -> Result<()> {
    // Configure filter from environment or config file
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(&config.level));

    match config.format.as_str() {
        "json" => {
            tracing_subscriber::registry()
                .with(env_filter)
                .with(tracing_subscriber::fmt::layer().json())
                .init();
        }
        _ => {
            tracing_subscriber::registry()
                .with(env_filter)
                .with(tracing_subscriber::fmt::layer().pretty())
                .init();
        }
    }

    Ok(())
}

/// Sanitizes sensitive information from log messages.
///
/// This function scans strings for common Google Cloud credential patterns
/// (like `ya29.` access tokens) and replaces them with a `\[REDACTED\]` placeholder.
/// This prevents sensitive secrets from being persisted in log sinks.
///
/// # Arguments
///
/// * `input` - The raw string that may contain sensitive data.
///
/// # Returns
///
/// A new string where all detected secrets have been replaced.
pub fn sanitize(input: &str) -> String {
    let mut result = input.to_string();
    
    // Pattern 1: Google OAuth2 Access Tokens
    // These typically start with "ya29."
    if let Some(pos) = result.find("ya29.") {
        let start = pos;
        // Search for the end of the token (delimiter or end of string)
        let end = result[start..].find(|c: char| c.is_whitespace() || c == '"' || c == '\'')
            .map(|i| start + i)
            .unwrap_or(result.len());
        result.replace_range(start..end, "[REDACTED_ACCESS_TOKEN]");
    }
    
    // Pattern 2: Google Refresh Tokens
    // These typically start with "1//0"
    if let Some(pos) = result.find("1//0") {
        let start = pos;
        let end = result[start..].find(|c: char| c.is_whitespace() || c == '"' || c == '\'')
            .map(|i| start + i)
            .unwrap_or(result.len());
        result.replace_range(start..end, "[REDACTED_REFRESH_TOKEN]");
    }
    
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_access_token() {
        let input = "Authorization: Bearer ya29.a0AfH6SMC...";
        let output = sanitize(input);
        assert!(output.contains("[REDACTED_ACCESS_TOKEN]"));
        assert!(!output.contains("ya29.a0AfH6SMC"));
    }

    #[test]
    fn test_sanitize_refresh_token() {
        let input = "refresh_token: 1//01S6LICZta2ee...";
        let output = sanitize(input);
        assert!(output.contains("[REDACTED_REFRESH_TOKEN]"));
        assert!(!output.contains("1//01S6LICZta2ee"));
    }
}
