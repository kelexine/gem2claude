// HTTP routes configuration
// Author: kelexine (https://github.com/kelexine)

use super::handlers::{event_logging_handler, health_handler, messages_handler};
use super::middleware::request_id_layers;
use crate::config::AppConfig;
use crate::error::Result;
use crate::gemini::GeminiClient;
use crate::oauth::OAuthManager;
use axum::{routing::{get, post}, Router};
use std::sync::Arc;
use tower_http::trace::TraceLayer;

#[derive(Clone)]
pub struct AppState {
    pub config: AppConfig,
    pub gemini_client: Arc<GeminiClient>,
    pub oauth_manager: OAuthManager,
    pub cache_manager: Option<Arc<crate::cache::CacheManager>>,
}

pub fn create_router(
    config: AppConfig,
    gemini_client: GeminiClient,
    oauth_manager: OAuthManager,
) -> Result<Router> {
    // Initialize cache manager if enabled
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
        .route("/v1/messages", post(messages_handler))
        .route("/api/event_logging/batch", post(event_logging_handler))
        .layer(tower_http::limit::RequestBodyLimitLayer::new(50 * 1024 * 1024))
        .layer(TraceLayer::new_for_http())
        .layer(propagate_request_id)
        .layer(set_request_id)
        .with_state(state);

    Ok(app)
}
