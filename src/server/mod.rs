//! Axum-based HTTP server implementation for the gem2claude bridge.
//!
//! This module is responsible for setting up the HTTP server, configuring routes,
//! and handling incoming requests from clients that expect an Anthropic-compatible API.
//! It bridges these requests to the Google Gemini API.
//!
//! # Components
//!
//! - `handlers`: Implementation of individual API endpoints (e.g., messages, health, metrics).
//! - `middleware`: Custom tower/axum middleware for request ID tracking, logging, and more.
//! - `routes`: The main router configuration that ties everything together.
//!
//! Author: kelexine (<https://github.com/kelexine>)

mod handlers;
mod middleware;
mod routes;

pub use routes::{create_router, AppState};
