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
}

pub fn create_router(
    config: AppConfig,
    gemini_client: GeminiClient,
    oauth_manager: OAuthManager,
) -> Result<Router> {
    let state = AppState {
        config,
        gemini_client: Arc::new(gemini_client),
        oauth_manager,
    };

    let (set_request_id, propagate_request_id) = request_id_layers();

    let app = Router::new()
        .route("/health", get(health_handler))
        .route("/v1/messages", post(messages_handler))
        .route("/api/event_logging/batch", post(event_logging_handler))
        // Allow large request bodies for base64-encoded images
        // 7MB PNG = ~9.5MB base64, so allow up to 50MB to be safe
        .layer(tower_http::limit::RequestBodyLimitLayer::new(50 * 1024 * 1024))
        .layer(TraceLayer::new_for_http())
        .layer(propagate_request_id)
        .layer(set_request_id)
        .with_state(state);

    Ok(app)
}
