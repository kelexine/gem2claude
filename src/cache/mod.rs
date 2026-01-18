//! Cache management for the gem2claude bridge.
//!
//! This module provides the infrastructure for context caching. While the
//! current implementation relies on Gemini's internal API automatic caching,
//! this module provides the `CacheManager` used for tracking statistics
//! and maintaining API compatibility with the public caching endpoints.

// Author: kelexine (https://github.com/kelexine)

pub mod manager;
pub mod models;

pub use manager::CacheManager;
pub use models::{CacheConfig, CacheStats};
