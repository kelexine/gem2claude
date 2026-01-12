// Configuration data structures
// Author: kelexine (https://github.com/kelexine)

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default)]
    pub server: ServerConfig,
    
    #[serde(default)]
    pub oauth: OAuthConfig,
    
    #[serde(default)]
    pub gemini: GeminiConfig,
    
    #[serde(default)]
    pub logging: LoggingConfig,
    
    #[serde(default)]
    pub performance: PerformanceConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    #[serde(default = "default_host")]
    pub host: String,
    
    #[serde(default = "default_port")]
    pub port: u16,
    
    #[serde(default = "default_workers")]
    pub workers: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthConfig {
    #[serde(default = "default_credentials_path")]
    pub credentials_path: String,
    
    #[serde(default = "default_true")]
    pub auto_refresh: bool,
    
    #[serde(default = "default_refresh_buffer")]
    pub refresh_buffer_seconds: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeminiConfig {
    #[serde(default = "default_api_base_url")]
    pub api_base_url: String,
    
    #[serde(default = "default_model")]
    pub default_model: String,
    
    #[serde(default = "default_timeout")]
    pub timeout_seconds: u64,
    
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    #[serde(default = "default_log_level")]
    pub level: String,
    
    #[serde(default = "default_log_format")]
    pub format: String,
    
    #[serde(default = "default_true")]
    pub sanitize_tokens: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceConfig {
    #[serde(default = "default_pool_size")]
    pub connection_pool_size: usize,
    
    #[serde(default = "default_true")]
    pub enable_compression: bool,
}

// Default implementations
impl Default for AppConfig {
    fn default() -> Self {
        Self {
            server: ServerConfig::default(),
            oauth: OAuthConfig::default(),
            gemini: GeminiConfig::default(),
            logging: LoggingConfig::default(),
            performance: PerformanceConfig::default(),
        }
    }
}

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

// Default value functions
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
        .join(".gemini")
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
