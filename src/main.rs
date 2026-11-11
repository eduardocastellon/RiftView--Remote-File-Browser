//! Entry point for the RiftView host service.

use std::process::ExitCode;

use tracing_subscriber::EnvFilter;

/// Log level used when the `RUST_LOG` environment variable is not set.
const DEFAULT_LOG_FILTER: &str = "info,tower_http=info";

#[tokio::main]
async fn main() -> ExitCode {
    set_up_logging();

    match riftview_api::server::run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            tracing::error!("fatal: {error:#}");
            ExitCode::FAILURE
        }
    }
}

fn set_up_logging() {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(DEFAULT_LOG_FILTER));

    tracing_subscriber::fmt().with_env_filter(filter).init();
}
