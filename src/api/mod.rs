//! HTTP routing. Handlers stay thin: validate input, call into
//! [`crate::filesystem`], serialise the result.

mod files;
mod health;
mod shares;

use axum::Router;
use axum::routing::get;

use crate::error::AppError;
use crate::state::AppState;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/health", get(health::health))
        .route("/api/version", get(health::version))
        .route("/api/host", get(health::host))
        .route("/api/shares", get(shares::list).post(shares::create))
        .route(
            "/api/shares/{id}",
            get(shares::get_one).patch(shares::update).delete(shares::remove),
        )
        .route("/api/shares/{id}/files", get(files::browse))
        .route("/api/files/{id}", get(files::metadata))
        .route("/api/files/{id}/download", get(files::download))
        .fallback(|| async { AppError::NotFound })
        .with_state(state)
}
