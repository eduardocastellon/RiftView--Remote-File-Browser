//! The host-only admin API.
//!
//! Served over plain HTTP on a loopback address (`127.0.0.1:7879` by
//! default), so it is unreachable from other machines. Every request must
//! also carry the admin token from the data folder, so other users on the
//! same computer can't use it either. The `riftview-api` command-line tools
//! (and a future desktop app) talk to this API.

mod devices;
mod pairing;
mod shares;
pub mod token;

use std::net::SocketAddr;

use axum::Router;
use axum::extract::{ConnectInfo, Request, State};
use axum::http::header::AUTHORIZATION;
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::routing::{get, post};
use serde_json::json;

use crate::error::AppError;
use crate::state::AppState;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/admin/host", get(host_info))
        // Shares
        .route("/admin/shares", get(shares::list).post(shares::create))
        .route(
            "/admin/shares/{id}",
            get(shares::get_one)
                .patch(shares::update)
                .delete(shares::remove),
        )
        // Devices
        .route("/admin/devices", get(devices::list))
        .route(
            "/admin/devices/{id}",
            axum::routing::patch(devices::update).delete(devices::revoke),
        )
        // Pairing
        .route(
            "/admin/pairing",
            get(pairing::status)
                .post(pairing::start)
                .delete(pairing::cancel),
        )
        .route("/admin/pairing/approve", post(pairing::approve))
        .route("/admin/pairing/reject", post(pairing::reject))
        .fallback(|| async { AppError::NotFound })
        .layer(middleware::from_fn_with_state(state.clone(), require_admin))
        .with_state(state)
}

/// Reject anything that isn't from this computer with the admin token.
async fn require_admin(
    State(state): State<AppState>,
    ConnectInfo(client): ConnectInfo<SocketAddr>,
    request: Request,
    next: Next,
) -> Result<Response, AppError> {
    if !client.ip().is_loopback() {
        return Err(AppError::Forbidden);
    }

    let presented = request
        .headers()
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .unwrap_or("");
    if !state.admin_token().matches(presented) {
        return Err(AppError::Unauthorized);
    }

    Ok(next.run(request).await)
}

/// `GET /admin/host`: details the host user needs, like the certificate pin.
async fn host_info(State(state): State<AppState>) -> axum::Json<serde_json::Value> {
    let config = state.config();
    axum::Json(json!({
        "name": config.host_name,
        "version": env!("CARGO_PKG_VERSION"),
        "bind": config.bind.to_string(),
        "certificate_pin": state.identity().pin(),
    }))
}
