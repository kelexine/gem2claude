//! gem2claude - OAuth-based Gemini API to Claude Code Compatible Proxy
//!
//! # Application Entry Point
//!
//! This is the main binary crate for `gem2claude`. It orchestrates the entire
//! application lifecycle, from configuration loading and logging initialization
//! to authentication handling and server startup.
//!
//! ## Execution Phases
//!
//! 1.  **Arguments Parsing**: Processes CLI flags using `clap`.
//! 2.  **Configuration**: Loads settings from `config.toml` and environment variables.
//! 3.  **Logging**: Initializes the `tracing` subscriber for structured logging.
//! 4.  **Authentication**:
//!     *   If `--login` is passed: Executes the interactive OAuth flow and exits.
//!     *   Otherwise: Loads existing credentials and initializes the `OAuthManager`.
//! 5.  **Initialization**: performs the `loadCodeAssist` handshake to resolve the
//!     Google Cloud Project ID.
//! 6.  **Server Startup**: Binds the Axum router to the configured port and starts listening.
//! 7.  **Shutdown**: Waits for SIGINT/SIGTERM to perform a graceful shutdown.

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

/// Main asynchronous entry point.
///
/// Orchestrates the boot sequence and manages the application runtime.
///
/// # Returns
///
/// Returns `Ok(())` on clean shutdown, or an `anyhow::Error` if a critical
/// failure occurs during startup or runtime.
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

/// Listens for OS termination signals (Ctrl+C, SIGTERM).
///
/// This future completes when a shutdown signal is received, triggering
/// the graceful shutdown mechanism of the web server.
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
