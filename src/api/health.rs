use axum::Json;
use axum::extract::State;
use serde_json::{Value, json};

use crate::state::AppState;

/// Minimal liveness probe; reveals nothing about the host.
pub async fn health() -> Json<Value> {
    Json(json!({ "status": "ok" }))
}

pub async fn version() -> Json<Value> {
    Json(json!({ "name": env!("CARGO_PKG_NAME"), "version": env!("CARGO_PKG_VERSION") }))
}

// TODO(phase 4): require an authenticated device.
pub async fn host(State(state): State<AppState>) -> Json<Value> {
    Json(json!({
        "name": state.config().host_name,
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "version": env!("CARGO_PKG_VERSION"),
    }))
}
