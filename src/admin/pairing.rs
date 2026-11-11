//! The host user's side of pairing: start, watch, approve or reject.

use std::collections::BTreeSet;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};

use crate::auth::Permission;
use crate::auth::devices::Device;
use crate::auth::pairing::HostView;
use crate::error::AppError;
use crate::filesystem::roots::now_ms;
use crate::state::AppState;

#[derive(Serialize)]
pub struct StartResponse {
    pairing_id: String,
    /// Show this to the user. Never log it.
    code: String,
    expires_in_secs: u64,
}

/// `POST /admin/pairing`: start pairing and get a fresh code.
pub async fn start(State(state): State<AppState>) -> Json<StartResponse> {
    let started = state.pairing().start();
    tracing::info!(pairing_id = %started.pairing_id, "pairing started");

    Json(StartResponse {
        pairing_id: started.pairing_id,
        code: started.code,
        expires_in_secs: started.expires_in_secs,
    })
}

/// `GET /admin/pairing`: what's happening with the current pairing?
pub async fn status(State(state): State<AppState>) -> Result<Json<HostView>, AppError> {
    state
        .pairing()
        .host_view()
        .map(Json)
        .ok_or(AppError::NotFound)
}

/// `DELETE /admin/pairing`: cancel the current pairing.
pub async fn cancel(State(state): State<AppState>) -> StatusCode {
    state.pairing().cancel();
    StatusCode::NO_CONTENT
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApproveRequest {
    pairing_id: String,
    /// Defaults to read-only (browse + download).
    #[serde(default = "Permission::read_only")]
    permissions: BTreeSet<Permission>,
}

/// `POST /admin/pairing/approve`: trust the waiting device.
pub async fn approve(
    State(state): State<AppState>,
    Json(request): Json<ApproveRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), AppError> {
    let pending = state.pairing().approve(&request.pairing_id)?;

    let device = Device {
        id: pending.device_id,
        name: pending.name,
        public_key: crate::auth::crypto::encode_base64(&pending.public_key),
        permissions: request.permissions,
        paired_at_ms: now_ms(),
        revoked_at_ms: None,
    };
    let device = state.devices().write().await.add(device)?;
    tracing::info!(device_id = %device.id, "device paired");

    let body = serde_json::json!({ "device_id": device.id });
    Ok((StatusCode::CREATED, Json(body)))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RejectRequest {
    pairing_id: String,
}

/// `POST /admin/pairing/reject`
pub async fn reject(
    State(state): State<AppState>,
    Json(request): Json<RejectRequest>,
) -> Result<StatusCode, AppError> {
    state.pairing().reject(&request.pairing_id)?;
    Ok(StatusCode::NO_CONTENT)
}
