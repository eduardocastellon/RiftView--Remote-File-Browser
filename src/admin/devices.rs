//! Listing devices, renaming them, changing their permissions, and
//! revoking them (host only).

use std::collections::BTreeSet;

use axum::Json;
use axum::extract::{Path, State};
use serde::{Deserialize, Serialize};

use crate::auth::Permission;
use crate::auth::devices::{Device, DeviceType, Platform, key_fingerprint};
use crate::error::AppError;
use crate::state::AppState;

/// A device as the host user sees it.
#[derive(Serialize)]
pub struct DeviceView {
    id: String,
    /// Name the device gave itself.
    name: String,
    /// Name the host user gave it, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    custom_alias: Option<String>,
    key_fingerprint: String,
    key_version: u32,
    device_type: DeviceType,
    platform: Platform,
    app_version: String,
    permissions: BTreeSet<Permission>,
    paired_at_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_connected_at_ms: Option<u64>,
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
            custom_alias: device.custom_alias.clone(),
            key_fingerprint,
            key_version: device.key_version,
            device_type: device.device_type,
            platform: device.platform,
            app_version: device.app_version.clone(),
            permissions: device.permissions.clone(),
            paired_at_ms: device.paired_at_ms,
            last_connected_at_ms: device.last_connected_at_ms,
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

/// Body of `PATCH /admin/devices/{id}`. Omitted fields are left unchanged.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateDeviceRequest {
    /// The complete new set of permissions.
    permissions: Option<BTreeSet<Permission>>,
    /// New alias; an empty string removes it.
    custom_alias: Option<String>,
}

/// `PATCH /admin/devices/{id}`
pub async fn update(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(request): Json<UpdateDeviceRequest>,
) -> Result<Json<DeviceView>, AppError> {
    let mut store = state.devices().write().await;
    let mut device = store.get(&id).cloned().ok_or(AppError::NotFound)?;

    if let Some(alias) = request.custom_alias {
        device = store.set_alias(&id, Some(&alias))?;
    }
    if let Some(permissions) = request.permissions {
        device = store.set_permissions(&id, permissions)?;
    }
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
