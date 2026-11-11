//! HTTP routes that devices use (served over HTTPS).
//!
//! Handlers are kept thin: check the input, call into
//! [`crate::filesystem`] or [`crate::auth`], and turn the result into JSON.
//! Any handler that takes an `AuthenticatedDevice` argument requires a
//! logged-in device.

mod auth;
mod files;
mod health;
mod pairing;
mod shares;

use std::net::SocketAddr;

use axum::Router;
use axum::extract::{ConnectInfo, Request, State};
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::routing::{get, post};

use crate::error::AppError;
use crate::state::AppState;

pub fn router(state: AppState) -> Router {
    // Pairing and login don't need a login, so they're rate-limited per IP.
    let unauthenticated = Router::new()
        .route("/api/pairing/initiate", post(pairing::initiate))
        .route("/api/pairing/complete", post(pairing::complete))
        .route("/api/pairing/{id}", get(pairing::status))
        .route("/api/auth/challenge", post(auth::challenge))
        .route("/api/auth/session", post(auth::log_in))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            limit_requests_per_ip,
        ));

    Router::new()
        // Public
        .route("/api/health", get(health::health))
        .route("/api/version", get(health::version))
        // Logged-in device
        .route("/api/host", get(health::host))
        .route("/api/auth/session", axum::routing::delete(auth::log_out))
        .route("/api/devices/me", get(auth::me))
        .route("/api/shares", get(shares::list))
        .route("/api/shares/{id}", get(shares::get_one))
        .route("/api/shares/{id}/files", get(files::browse))
        .route("/api/files/{id}", get(files::metadata))
        .route("/api/files/{id}/download", get(files::download))
        .merge(unauthenticated)
        // Anything else is a JSON 404.
        .fallback(|| async { AppError::NotFound })
        .with_state(state)
}

async fn limit_requests_per_ip(
    State(state): State<AppState>,
    ConnectInfo(client): ConnectInfo<SocketAddr>,
    request: Request,
    next: Next,
) -> Result<Response, AppError> {
    if !state.auth_rate_limiter().allow(client.ip()) {
        return Err(AppError::TooManyRequests);
    }
    Ok(next.run(request).await)
}
