//! Starting the server and the middleware every request passes through.

use std::time::Duration;

use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::http::StatusCode;
use tower_http::timeout::TimeoutLayer;
use tower_http::trace::TraceLayer;

use crate::config::{Config, ConfigError};
use crate::filesystem::roots::ShareError;
use crate::state::AppState;
use crate::{api, platform, storage};

/// The complete app: routes plus middleware. Used by `run` and by tests.
pub fn app(state: AppState) -> Router {
    let config = state.config().clone();
    let request_timeout = Duration::from_secs(config.request_timeout_secs);

    api::router(state)
        // Reject oversized request bodies.
        .layer(DefaultBodyLimit::max(config.max_request_body_bytes))
        // Give up if a response hasn't *started* in time. Once a download
        // is streaming, this no longer applies.
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            request_timeout,
        ))
        // Log every request.
        .layer(TraceLayer::new_for_http())
}

#[derive(Debug, thiserror::Error)]
pub enum StartupError {
    #[error(transparent)]
    Config(#[from] ConfigError),

    #[error("state: {0}")]
    State(#[from] ShareError),

    #[error(
        "refusing to bind non-loopback address {0}: \
         device authentication and TLS are not implemented yet"
    )]
    NonLoopback(std::net::SocketAddr),

    #[error("I/O: {0}")]
    Io(#[from] std::io::Error),
}

/// Load settings, start listening, and serve until Ctrl-C or SIGTERM.
pub async fn run() -> Result<(), StartupError> {
    let data_dir = platform::data_dir().ok_or(ConfigError::NoDataDir)?;
    storage::ensure_data_dir(&data_dir)?;
    let config = Config::load_or_init(&data_dir)?;

    // There's no login or encryption yet (phase 4), so never accept
    // connections from other machines.
    if !config.bind.ip().is_loopback() {
        return Err(StartupError::NonLoopback(config.bind));
    }

    let state = AppState::load(config.clone(), data_dir.clone())?;
    let listener = tokio::net::TcpListener::bind(config.bind).await?;

    tracing::info!(
        addr = %listener.local_addr()?,
        data_dir = %data_dir.display(),
        "RiftView listening"
    );

    axum::serve(listener, app(state))
        .with_graceful_shutdown(wait_for_shutdown_signal())
        .await?;

    tracing::info!("shut down cleanly");
    Ok(())
}

/// Resolves when the user presses Ctrl-C or the OS asks us to stop.
async fn wait_for_shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };

    // SIGTERM is how service managers (systemd, launchd) ask us to stop.
    #[cfg(unix)]
    let terminate = async {
        use tokio::signal::unix::{SignalKind, signal};
        if let Ok(mut sigterm) = signal(SignalKind::terminate()) {
            sigterm.recv().await;
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {}
        _ = terminate => {}
    }
}
