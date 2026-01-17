// gem2claude - OAuth-based Gemini API to Claude Code Compatible Proxy
// Author: kelexine (https://github.com/kelexine)

use anyhow::Result;
use clap::Parser;
use gem2claude::cli::Args;
use gem2claude::config::AppConfig;
use gem2claude::gemini::GeminiClient;
use gem2claude::oauth::{login, OAuthManager};
use gem2claude::server::create_router;
use gem2claude::utils::logging;
use std::net::SocketAddr;
use tokio::signal;
use tracing::info;

#[tokio::main]
async fn main() -> Result<()> {
    // Parse CLI arguments
    let args = Args::parse();

    // Phase 1: Load configuration
    let config = AppConfig::load()?;

    // Phase 2: Initialize logging
    logging::init(&config.logging)?;
    info!("Starting gem2claude v{}", env!("CARGO_PKG_VERSION"));

    // Phase 2.5: Handle --login flag (OAuth flow)
    if args.login {
        login::run().await?;
    }

    // Phase 3: Load OAuth credentials
    info!(
        "Loading OAuth credentials from {}",
        config.oauth.credentials_path
    );
    let oauth_manager = OAuthManager::new(&config.oauth).await?;

    // Phase 4: Resolve project ID (loadCodeAssist handshake)
    info!("Resolving Gemini Cloud Code project ID...");
    let gemini_client = GeminiClient::new(&config.gemini, oauth_manager.clone()).await?;
    info!("Project ID resolved: {}", gemini_client.project_id());

    // Phase 5: Build and start HTTP server
    let app = create_router(config.clone(), gemini_client, oauth_manager)?;
    let addr: SocketAddr = format!("{}:{}", config.server.host, config.server.port).parse()?;

    info!("Starting server on {}", addr);
    let listener = tokio::net::TcpListener::bind(addr).await?;

    // Phase 6: Run server with graceful shutdown
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    info!("Server shut down gracefully");
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            info!("Received Ctrl+C signal");
        },
        _ = terminate => {
            info!("Received SIGTERM signal");
        },
    }
}
