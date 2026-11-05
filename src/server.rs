//! Server startup and middleware stack.

use std::time::Duration;

use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::http::StatusCode;
use tower_http::timeout::TimeoutLayer;
use tower_http::trace::TraceLayer;

use crate::config::Config;
use crate::state::AppState;
use crate::{api, platform, storage};

/// Build the full application (router + middleware). Used by `run` and tests.
pub fn app(state: AppState) -> Router {
    let cfg = state.config().clone();
    api::router(state)
        .layer(DefaultBodyLimit::max(cfg.max_request_body_bytes))
        // Bounds time-to-headers only; streaming bodies are unaffected.
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            Duration::from_secs(cfg.request_timeout_secs),
        ))
        .layer(TraceLayer::new_for_http())
}

#[derive(Debug, thiserror::Error)]
pub enum StartupError {
    #[error(transparent)]
    Config(#[from] crate::config::ConfigError),
    #[error("state: {0}")]
    State(#[from] crate::filesystem::roots::ShareError),
    #[error("refusing to bind non-loopback address {0}: device authentication and TLS are not implemented yet")]
    NonLoopback(std::net::SocketAddr),
    #[error("I/O: {0}")]
    Io(#[from] std::io::Error),
}

pub async fn run() -> Result<(), StartupError> {
    let data_dir = platform::data_dir().ok_or(crate::config::ConfigError::NoDataDir)?;
    storage::ensure_data_dir(&data_dir)?;
    let config = Config::load_or_init(&data_dir)?;

    // Until phase 4 (auth + TLS), never expose the API beyond this machine.
    if !config.bind.ip().is_loopback() {
        return Err(StartupError::NonLoopback(config.bind));
    }

    let state = AppState::load(config.clone(), data_dir.clone())?;
    let listener = tokio::net::TcpListener::bind(config.bind).await?;
    tracing::info!(addr = %listener.local_addr()?, data_dir = %data_dir.display(), "RiftView listening");

    axum::serve(listener, app(state)).with_graceful_shutdown(shutdown_signal()).await?;
    tracing::info!("shut down cleanly");
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let term = async {
        if let Ok(mut s) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            s.recv().await;
        }
    };
    #[cfg(not(unix))]
    let term = std::future::pending::<()>();
    tokio::select! { _ = ctrl_c => {}, _ = term => {} }
}
