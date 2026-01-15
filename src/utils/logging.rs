// Structured logging with token sanitization
// Author: kelexine (https://github.com/kelexine)

use crate::config::LoggingConfig;
use crate::error::Result;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

pub fn init(config: &LoggingConfig) -> Result<()> {
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

/// Sanitize sensitive data from strings (access tokens, refresh tokens)
pub fn sanitize(input: &str) -> String {
    let mut result = input.to_string();
    
    // Replace access tokens (ya29.*)
    if let Some(pos) = result.find("ya29.") {
        let start = pos;
        // Find end of token (whitespace, quote, or end of string)
        let end = result[start..].find(|c: char| c.is_whitespace() || c == '"' || c == '\'')
            .map(|i| start + i)
            .unwrap_or(result.len());
        result.replace_range(start..end, "[REDACTED_ACCESS_TOKEN]");
    }
    
    // Replace refresh tokens (1//0*)
    if let Some(pos) = result.find("1//0") {
        let start = pos;
        // Find end of token (whitespace, quote, or end of string)
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
        assert!(output.contains("[REDACTED_ACCESS_TOKEN"));
        assert!(!output.contains("ya29.a0AfH6SMC"));
    }

    #[test]
    fn test_sanitize_refresh_token() {
        let input = "refresh_token: 1//01S6LICZta2ee...";
        let output = sanitize(input);
        assert!(output.contains("[REDACTED_REFRESH_TOKEN"));
        assert!(!output.contains("1//01S6LICZta2ee"));
    }
}
