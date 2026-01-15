//! Configuration management for the gem2claude bridge.
//!
//! This module handles loading application settings from multiple sources,
//! including environment variables, configuration files, and default values.
//!
//! Author: kelexine (<https://github.com/kelexine>)

mod models;

pub use models::*;

use crate::error::{ProxyError, Result};
use config::{Config, Environment, File};
use std::path::PathBuf;

impl AppConfig {
    /// Loads the application configuration from all supported sources.
    ///
    /// The configuration is resolved using the following priority (highest to lowest):
    /// 1. **Environment Variables**: Prefixed with `GEMINI_PROXY_` (e.g., `GEMINI_PROXY_SERVER_PORT`).
    /// 2. **Configuration File**: Located at `~/.gemini-proxy/config.toml` by default.
    /// 3. **Compiled-in Defaults**: Hardcoded sane defaults for all parameters.
    ///
    /// # Returns
    ///
    /// A `Result` containing the fully initialized `AppConfig`.
    ///
    /// # Errors
    ///
    /// Returns a `ProxyError::Config` if the configuration cannot be built or deserialized.
    pub fn load() -> Result<Self> {
        let config = Config::builder()
            // Step 1: Initialize with default values from the struct's Default implementation
            .add_source(Config::try_from(&Self::default())?)
            
            // Step 2: Merge settings from the optional config file
            .add_source(
                File::with_name(&Self::default_config_path())
                    .required(false)
            )
            
            // Step 3: Override with environment variables
            // Matches variables like GEMINI_PROXY_SERVER_HOST -> server.host
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

    /// Returns the absolute path to the default configuration file.
    ///
    /// Defaults to `~/.gemini-proxy/config.toml`.
    fn default_config_path() -> String {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".gemini-proxy")
            .join("config.toml")
            .to_string_lossy()
            .to_string()
    }
}
