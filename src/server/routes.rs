//! Route configuration and shared state for the HTTP server.
//!
//! This module defines the application's routing structure and the shared state
//! used by handlers.
//!
//! Author: kelexine (<https://github.com/kelexine>)

use super::handlers::{event_logging_handler, health_handler, messages_handler, metrics_handler};
use super::middleware::request_id_layers;
use crate::config::AppConfig;
use crate::error::Result;
use crate::gemini::GeminiClient;
use crate::oauth::OAuthManager;
use axum::{
    routing::{get, post},
    Router,
};
use std::sync::Arc;
use tower_http::trace::TraceLayer;

/// Shared application state accessible by all request handlers.
///
/// This struct is wrapped in an `Arc` by Axum and cloned for each request.
/// It contains thread-safe handles to global configuration, API clients, and managers.
#[derive(Clone)]
pub struct AppState {
    /// Read-only global application configuration.
    pub config: AppConfig,
    /// Shared instance of the Gemini API client.
    pub gemini_client: Arc<GeminiClient>,
    /// Manager for OAuth2 authentication with Google Cloud.
    pub oauth_manager: OAuthManager,
    /// Optional manager for Gemini 1.5 context caching feature.
    pub cache_manager: Option<Arc<crate::cache::CacheManager>>,
}

/// Creates the main application router with all core routes and middleware.
///
/// # Arguments
///
/// * `config` - Application configuration loaded from environment or file.
/// * `gemini_client` - Initialized client for the Gemini API.
/// * `oauth_manager` - Manager for handling Google OAuth2 tokens.
///
/// # Returns
///
/// A configured `axum::Router` ready to be served.
///
/// # Routes
///
/// - `GET /health`: Health checks for service and dependencies.
/// - `GET /metrics`: Prometheus-formatted metrics.
/// - `POST /v1/messages`: Anthropic-compatible messages endpoint.
/// - `POST /api/event_logging/batch`: Sink for Claude Code telemetry/logs.
pub fn create_router(
    config: AppConfig,
    gemini_client: GeminiClient,
    oauth_manager: OAuthManager,
) -> Result<Router> {
    // Initialize cache manager if ENABLE_CONTEXT_CACHING is set
    let cache_manager = if std::env::var("ENABLE_CONTEXT_CACHING")
        .unwrap_or_else(|_| "false".to_string())
        .parse::<bool>()
        .unwrap_or(false)
    {
        let cache_config = crate::cache::CacheConfig::default();
        Some(Arc::new(crate::cache::CacheManager::new(cache_config)))
    } else {
        None
    };

    let state = AppState {
        config,
        gemini_client: Arc::new(gemini_client),
        oauth_manager,
        cache_manager,
    };

    let (set_request_id, propagate_request_id) = request_id_layers();

    let app = Router::new()
        .route("/health", get(health_handler))
        .route("/metrics", get(metrics_handler))
        .route("/v1/messages", post(messages_handler))
        .route("/api/event_logging/batch", post(event_logging_handler))
        .layer(tower_http::limit::RequestBodyLimitLayer::new(
            50 * 1024 * 1024,
        )) // 50MB limit
        .layer(TraceLayer::new_for_http())
        .layer(propagate_request_id)
        .layer(set_request_id)
        .with_state(state);

    Ok(app)
}
