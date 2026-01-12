// Configuration module
// Author: kelexine (https://github.com/kelexine)

mod models;

pub use models::*;

use crate::error::{ProxyError, Result};
use config::{Config, Environment, File};
use std::path::PathBuf;

impl AppConfig {
    /// Load configuration from multiple sources with precedence:
    /// 1. CLI arguments (highest)
    /// 2. Environment variables
    /// 3. Config file
    /// 4. Defaults (lowest)
    pub fn load() -> Result<Self> {
        let config = Config::builder()
            // Start with defaults
            .add_source(Config::try_from(&Self::default())?)
            // Load from config file if it exists
            .add_source(
                File::with_name(&Self::default_config_path())
                    .required(false)
            )
            // Override with environment variables (prefix: GEMINI_PROXY_)
            .add_source(
                Environment::with_prefix("GEMINI_PROXY")
                    .separator("_")
            )
            .build()
            .map_err(|e| ProxyError::Config(e.to_string()))?;

        config
            .try_deserialize()
            .map_err(|e| ProxyError::Config(e.to_string()))
    }

    fn default_config_path() -> String {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".gemini-proxy")
            .join("config.toml")
            .to_string_lossy()
            .to_string()
    }
}
