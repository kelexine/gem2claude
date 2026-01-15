//! # gem2claude
//!
//! OAuth-based Gemini API to Claude Code Compatible Proxy.
//!
//! This library provides the core functionality for translating Claude API requests
//! (used by Claude Code CLI and other tools) into Google Gemini API requests.
//!
//! ## Core Modules
//!
//! - [`translation`]: Handles request/response translation between Claude and Gemini formats.
//! - [`oauth`]: Manages Google Cloud OAuth authentication and token refreshing.
//! - [`gemini`]: Client for the Google Gemini API.
//! - [`server`]: Axum-based HTTP server implementation.
//! - [`metrics`]: Prometheus metrics collection.
//! - [`cache`]: Context caching implementation.

// Author: kelexine (https://github.com/kelexine)

pub mod cache;
pub mod cli;
pub mod config;
pub mod error;
pub mod gemini;
pub mod metrics;
pub mod models;
pub mod oauth;
pub mod server;
pub mod translation;
pub mod utils;
pub mod vision;
