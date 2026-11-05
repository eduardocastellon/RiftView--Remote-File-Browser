//! Shared-root management.
//!
//! TODO(phase 4): these must require the "Manage Shares" permission. Until
//! then the server only binds to loopback.

use std::path::PathBuf;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};

use crate::error::AppError;
use crate::filesystem::roots::{NewShare, Share, SharePatch};
use crate::state::AppState;

/// Client view of a share: the host path is deliberately omitted.
#[derive(Serialize)]
pub struct ShareView {
    id: String,
    name: String,
    writable: bool,
    created_at_ms: u64,
}

impl From<&Share> for ShareView {
    fn from(s: &Share) -> Self {
        Self { id: s.id.clone(), name: s.name.clone(), writable: s.writable, created_at_ms: s.created_at_ms }
    }
}

#[derive(Serialize)]
pub struct ShareList {
    shares: Vec<ShareView>,
}

pub async fn list(State(state): State<AppState>) -> Json<ShareList> {
    let store = state.shares().read().await;
    Json(ShareList { shares: store.list().iter().map(ShareView::from).collect() })
}

pub async fn get_one(State(state): State<AppState>, Path(id): Path<String>) -> Result<Json<ShareView>, AppError> {
    Ok(Json(ShareView::from(&state.share(&id).await?)))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateShare {
    name: String,
    path: PathBuf,
    #[serde(default)]
    writable: bool,
}

pub async fn create(
    State(state): State<AppState>,
    Json(req): Json<CreateShare>,
) -> Result<(StatusCode, Json<ShareView>), AppError> {
    let mut store = state.shares().write().await;
    let share = store.add(NewShare { name: req.name, path: req.path, writable: req.writable })?;
    tracing::info!(share_id = %share.id, "share registered");
    Ok((StatusCode::CREATED, Json(ShareView::from(&share))))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateShare {
    name: Option<String>,
    writable: Option<bool>,
}

pub async fn update(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<UpdateShare>,
) -> Result<Json<ShareView>, AppError> {
    let mut store = state.shares().write().await;
    let share = store.update(&id, SharePatch { name: req.name, writable: req.writable })?;
    Ok(Json(ShareView::from(&share)))
}

pub async fn remove(State(state): State<AppState>, Path(id): Path<String>) -> Result<StatusCode, AppError> {
    state.shares().write().await.remove(&id)?;
    tracing::info!(share_id = %id, "share removed");
    Ok(StatusCode::NO_CONTENT)
}
