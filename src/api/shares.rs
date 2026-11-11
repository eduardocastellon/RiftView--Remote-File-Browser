//! Viewing shares from a device. Creating and changing shares is host-only
//! (see `crate::admin::shares`).

use axum::Json;
use axum::extract::{Path, State};
use serde::Serialize;

use crate::auth::{AuthenticatedDevice, Permission};
use crate::error::AppError;
use crate::filesystem::roots::Share;
use crate::state::AppState;

/// A share as devices see it. The real host path is deliberately left out.
#[derive(Serialize)]
pub struct ShareView {
    id: String,
    name: String,
    writable: bool,
    created_at_ms: u64,
}

impl From<&Share> for ShareView {
    fn from(share: &Share) -> Self {
        Self {
            id: share.id.clone(),
            name: share.name.clone(),
            writable: share.writable,
            created_at_ms: share.created_at_ms,
        }
    }
}

#[derive(Serialize)]
pub struct ShareList {
    shares: Vec<ShareView>,
}

/// `GET /api/shares`
pub async fn list(
    device: AuthenticatedDevice,
    State(state): State<AppState>,
) -> Result<Json<ShareList>, AppError> {
    device.require(Permission::Browse)?;

    let store = state.shares().read().await;
    let shares = store.list().iter().map(ShareView::from).collect();
    Ok(Json(ShareList { shares }))
}

/// `GET /api/shares/{id}`
pub async fn get_one(
    device: AuthenticatedDevice,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<ShareView>, AppError> {
    device.require(Permission::Browse)?;

    let share = state.share(&id).await?;
    Ok(Json(ShareView::from(&share)))
}
