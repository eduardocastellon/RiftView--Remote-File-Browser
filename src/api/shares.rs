//! Creating, viewing, changing and removing shares.
//!
//! TODO(phase 4): require the "Manage Shares" permission. Until then the
//! server only listens on this machine (loopback).

use std::path::PathBuf;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};

use crate::error::AppError;
use crate::filesystem::roots::{NewShare, Share, SharePatch};
use crate::state::AppState;

/// A share as clients see it. The real host path is deliberately left out.
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
pub async fn list(State(state): State<AppState>) -> Json<ShareList> {
    let store = state.shares().read().await;
    let shares = store.list().iter().map(ShareView::from).collect();
    Json(ShareList { shares })
}

/// `GET /api/shares/{id}`
pub async fn get_one(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<ShareView>, AppError> {
    let share = state.share(&id).await?;
    Ok(Json(ShareView::from(&share)))
}

/// Body of `POST /api/shares`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateShareRequest {
    name: String,
    path: PathBuf,
    #[serde(default)] // Read-only unless asked otherwise.
    writable: bool,
}

/// `POST /api/shares`
pub async fn create(
    State(state): State<AppState>,
    Json(request): Json<CreateShareRequest>,
) -> Result<(StatusCode, Json<ShareView>), AppError> {
    let new_share = NewShare {
        name: request.name,
        path: request.path,
        writable: request.writable,
    };

    let share = state.shares().write().await.add(new_share)?;
    tracing::info!(share_id = %share.id, "share registered");

    Ok((StatusCode::CREATED, Json(ShareView::from(&share))))
}

/// Body of `PATCH /api/shares/{id}`. Omitted fields are left unchanged.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateShareRequest {
    name: Option<String>,
    writable: Option<bool>,
}

/// `PATCH /api/shares/{id}`
pub async fn update(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(request): Json<UpdateShareRequest>,
) -> Result<Json<ShareView>, AppError> {
    let patch = SharePatch {
        name: request.name,
        writable: request.writable,
    };

    let share = state.shares().write().await.update(&id, patch)?;
    Ok(Json(ShareView::from(&share)))
}

/// `DELETE /api/shares/{id}`
pub async fn remove(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    state.shares().write().await.remove(&id)?;
    tracing::info!(share_id = %id, "share removed");

    Ok(StatusCode::NO_CONTENT)
}
