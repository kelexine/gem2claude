//! Configuration data structures for the gem2claude bridge.
//!
//! This module defines the schema for the application settings, including
//! server parameters, OAuth2 credentials, and Gemini API specifics.
//!
//! Author: kelexine (<https://github.com/kelexine>)

use serde::{Deserialize, Serialize};

/// The root configuration object for the application.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppConfig {
    /// HTTP server settings (host, port, workers).
    #[serde(default)]
    pub server: ServerConfig,

    /// OAuth2 authentication settings.
    #[serde(default)]
    pub oauth: OAuthConfig,

    /// Upstream Gemini API settings.
    #[serde(default)]
    pub gemini: GeminiConfig,

    /// Logging and observability settings.
    #[serde(default)]
    pub logging: LoggingConfig,

    /// Performance and resource management settings.
    #[serde(default)]
    pub performance: PerformanceConfig,
}

/// Settings for the built-in HTTP server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    /// The IP address or hostname the server should bind to.
    /// Default: `127.0.0.1`
    #[serde(default = "default_host")]
    pub host: String,

    /// The port number the server should listen on.
    /// Default: `8080`
    #[serde(default = "default_port")]
    pub port: u16,

    /// Number of worker threads for the Axum server.
    /// Default: Number of logical CPU cores.
    #[serde(default = "default_workers")]
    pub workers: usize,
}

/// Settings for Google Cloud OAuth2 authentication.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthConfig {
    /// Path to the JSON credentials file (Service Account or User Credentials).
    /// Default: `~/.gemini/oauth_creds.json`
    #[serde(default = "default_credentials_path")]
    pub credentials_path: String,

    /// Whether to automatically refresh the access token before it expires.
    /// Default: `true`
    #[serde(default = "default_true")]
    pub auto_refresh: bool,

    /// Number of seconds before expiration to trigger a token refresh.
    /// Default: `300` (5 minutes)
    #[serde(default = "default_refresh_buffer")]
    pub refresh_buffer_seconds: i64,
}

/// Settings for the upstream Gemini API connection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeminiConfig {
    /// Base URL for the Google Cloud Gemini API.
    /// Default: Google's Cloud Code internal API base.
    #[serde(default = "default_api_base_url")]
    pub api_base_url: String,

    /// The default Gemini model to use if none is specified by the client.
    /// Default: `gemini-3-flash-preview`
    #[serde(default = "default_model")]
    pub default_model: String,

    /// Connection and request timeout in seconds.
    /// Default: `300` (5 minutes)
    #[serde(default = "default_timeout")]
    pub timeout_seconds: u64,

    /// Maximum number of times to retry failed API requests.
    /// Default: `3`
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,
}

/// Settings for application logging and output format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    /// Minimum log level (`trace`, `debug`, `info`, `warn`, `error`).
    /// Default: `info`
    #[serde(default = "default_log_level")]
    pub level: String,

    /// Output format for logs (`pretty`, `json`, `compact`).
    /// Default: `pretty`
    #[serde(default = "default_log_format")]
    pub format: String,

    /// Whether to mask sensitive tokens and credentials in logs.
    /// Default: `true`
    #[serde(default = "default_true")]
    pub sanitize_tokens: bool,
}

/// Settings for tuning application performance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceConfig {
    /// Maximum number of idle connections to keep in the HTTP pool.
    /// Default: `100`
    #[serde(default = "default_pool_size")]
    pub connection_pool_size: usize,

    /// Whether to enable GZIP/Brotli compression for HTTP responses.
    /// Default: `true`
    #[serde(default = "default_true")]
    pub enable_compression: bool,
}

// Default trait implementations linking to custom logic

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: default_host(),
            port: default_port(),
            workers: default_workers(),
        }
    }
}

impl Default for OAuthConfig {
    fn default() -> Self {
        Self {
            credentials_path: default_credentials_path(),
            auto_refresh: true,
            refresh_buffer_seconds: default_refresh_buffer(),
        }
    }
}

impl Default for GeminiConfig {
    fn default() -> Self {
        Self {
            api_base_url: default_api_base_url(),
            default_model: default_model(),
            timeout_seconds: default_timeout(),
            max_retries: default_max_retries(),
        }
    }
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: default_log_level(),
            format: default_log_format(),
            sanitize_tokens: true,
        }
    }
}

impl Default for PerformanceConfig {
    fn default() -> Self {
        Self {
            connection_pool_size: default_pool_size(),
            enable_compression: true,
        }
    }
}

// Helper functions for serde defaults and shared constants
fn default_host() -> String {
    "127.0.0.1".to_string()
}

fn default_port() -> u16 {
    8080
}

fn default_workers() -> usize {
    num_cpus::get()
}

fn default_credentials_path() -> String {
    dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join(".gem2claude")
        .join("oauth_creds.json")
        .to_string_lossy()
        .to_string()
}

fn default_true() -> bool {
    true
}

fn default_refresh_buffer() -> i64 {
    300 // 5 minutes
}

fn default_api_base_url() -> String {
    "https://cloudcode-pa.googleapis.com/v1internal".to_string()
}

fn default_model() -> String {
    "gemini-3-flash-preview".to_string()
}

fn default_timeout() -> u64 {
    300
}

fn default_max_retries() -> u32 {
    3
}

fn default_log_level() -> String {
    "info".to_string()
}

fn default_log_format() -> String {
    "pretty".to_string()
}

fn default_pool_size() -> usize {
    100
}
