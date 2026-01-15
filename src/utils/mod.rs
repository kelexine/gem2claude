//! Utility functions and helpers for the gem2claude bridge.
//!
//! This module provides cross-cutting concerns like structured logging,
//! token sanitization, and intelligent retry logic with backoff.
//!
//! # Submodules
//!
//! - `logging`: Tracing and logging initialization with security filters.
//! - `retry`: Robust retry mechanisms that respect upstream API hints.
//!
//! Author: kelexine (<https://github.com/kelexine>)

pub mod logging;
pub mod retry;
