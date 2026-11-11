//! Starting the servers and the middleware every request passes through.
//!
//! Two listeners run side by side:
//!
//! - the **device API** over HTTPS on `config.bind` (all interfaces by
//!   default, so LAN and VPN clients can reach it), and
//! - the **admin API** over plain HTTP on `config.admin_bind`, which must
//!   be a loopback address.

use std::net::SocketAddr;
use std::time::Duration;

use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::http::StatusCode;
use axum_server::tls_rustls::RustlsConfig;
use tokio_util::sync::CancellationToken;
use tower_http::timeout::TimeoutLayer;
use tower_http::trace::TraceLayer;

use crate::config::{Config, ConfigError};
use crate::state::{AppState, StateError};
use crate::{admin, api, platform, storage};

/// How long open connections get to finish when shutting down.
const SHUTDOWN_GRACE_PERIOD: Duration = Duration::from_secs(10);

/// The device API: routes plus middleware. Used by `run` and by tests.
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

/// The admin API with the same request limits. Used by `run` and by tests.
pub fn admin_app(state: AppState) -> Router {
    let max_body = state.config().max_request_body_bytes;

    admin::router(state)
        .layer(DefaultBodyLimit::max(max_body))
        .layer(TraceLayer::new_for_http())
}

#[derive(Debug, thiserror::Error)]
pub enum StartupError {
    #[error(transparent)]
    Config(#[from] ConfigError),

    #[error(transparent)]
    State(#[from] StateError),

    #[error("admin_bind must be a loopback address (e.g. 127.0.0.1:7879), not {0}")]
    AdminNotLoopback(SocketAddr),

    #[error("I/O: {0}")]
    Io(#[from] std::io::Error),
}

/// Load settings, start both listeners, and serve until Ctrl-C or SIGTERM.
pub async fn run() -> Result<(), StartupError> {
    // rustls needs to be told which crypto library to use, once per process.
    let _ = rustls::crypto::ring::default_provider().install_default();

    let data_dir = platform::data_dir().ok_or(ConfigError::NoDataDir)?;
    storage::ensure_data_dir(&data_dir)?;
    let config = Config::load_or_init(&data_dir)?;

    if !config.admin_bind.ip().is_loopback() {
        return Err(StartupError::AdminNotLoopback(config.admin_bind));
    }

    let state = AppState::load(config.clone(), data_dir.clone())?;
    let identity = state.identity();
    let tls_config =
        RustlsConfig::from_pem(identity.cert_pem.clone(), identity.key_pem.clone()).await?;

    // One signal stops both listeners.
    let shutdown = CancellationToken::new();
    tokio::spawn({
        let shutdown = shutdown.clone();
        async move {
            wait_for_shutdown_signal().await;
            shutdown.cancel();
        }
    });

    let https_handle = axum_server::Handle::new();
    tokio::spawn({
        let shutdown = shutdown.clone();
        let https_handle = https_handle.clone();
        async move {
            shutdown.cancelled().await;
            https_handle.graceful_shutdown(Some(SHUTDOWN_GRACE_PERIOD));
        }
    });

    let admin_listener = tokio::net::TcpListener::bind(config.admin_bind).await?;

    tracing::info!(
        device_api = %format!("https://{}", config.bind),
        admin_api = %format!("http://{}", admin_listener.local_addr()?),
        certificate_pin = %identity.pin(),
        data_dir = %data_dir.display(),
        "RiftView listening"
    );

    let device_server = axum_server::bind_rustls(config.bind, tls_config)
        .handle(https_handle)
        .serve(app(state.clone()).into_make_service_with_connect_info::<SocketAddr>());

    let admin_server = axum::serve(
        admin_listener,
        admin_app(state).into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown.cancelled_owned());

    tokio::try_join!(device_server, async { admin_server.await })?;

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
