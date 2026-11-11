//! Listing devices, changing their permissions, and revoking them (host only).

use std::collections::BTreeSet;

use axum::Json;
use axum::extract::{Path, State};
use serde::{Deserialize, Serialize};

use crate::auth::Permission;
use crate::auth::devices::{Device, key_fingerprint};
use crate::error::AppError;
use crate::state::AppState;

#[derive(Serialize)]
pub struct DeviceView {
    id: String,
    name: String,
    key_fingerprint: String,
    permissions: BTreeSet<Permission>,
    paired_at_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    revoked_at_ms: Option<u64>,
}

impl From<&Device> for DeviceView {
    fn from(device: &Device) -> Self {
        let key_fingerprint = device
            .public_key_bytes()
            .map(|key| key_fingerprint(&key))
            .unwrap_or_default();

        Self {
            id: device.id.clone(),
            name: device.name.clone(),
            key_fingerprint,
            permissions: device.permissions.clone(),
            paired_at_ms: device.paired_at_ms,
            revoked_at_ms: device.revoked_at_ms,
        }
    }
}

#[derive(Serialize)]
pub struct DeviceList {
    devices: Vec<DeviceView>,
}

/// `GET /admin/devices`
pub async fn list(State(state): State<AppState>) -> Json<DeviceList> {
    let store = state.devices().read().await;
    let devices = store.list().iter().map(DeviceView::from).collect();
    Json(DeviceList { devices })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateDeviceRequest {
    permissions: BTreeSet<Permission>,
}

/// `PATCH /admin/devices/{id}`
pub async fn update(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(request): Json<UpdateDeviceRequest>,
) -> Result<Json<DeviceView>, AppError> {
    let device = state
        .devices()
        .write()
        .await
        .set_permissions(&id, request.permissions)?;
    Ok(Json(DeviceView::from(&device)))
}

/// `DELETE /admin/devices/{id}`: revoke a device and log it out everywhere.
pub async fn revoke(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<DeviceView>, AppError> {
    let device = state.devices().write().await.revoke(&id)?;
    state.sessions().end_all_sessions_for(&id);
    tracing::info!(device_id = %id, "device revoked");

    Ok(Json(DeviceView::from(&device)))
}
