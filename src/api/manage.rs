//! Rename, move, new folder, delete (Phase 7).
//!
//! ```text
//! POST   /api/files/{id}/rename        {name}              → 200 entry
//! POST   /api/files/{id}/move          {share_id?, path}   → 200 entry, or 202 job (other disk)
//! POST   /api/shares/{id}/directories  {path, name}        → 201 entry
//! DELETE /api/files/{id}                                   → 200 {deleted: "trash"|"permanent"}
//! GET    /api/jobs/{id}                                    → job progress / result
//! DELETE /api/jobs/{id}                                    → cancel a running job
//! ```
//!
//! Each needs its own permission (`rename`, `move`, `upload` for new
//! folders, `delete`), and every share involved must be writable. A name
//! that's already taken is a `409`; nothing is ever replaced.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::files::look_up_file_id;
use super::transfers::{transfer_slot, writable_share};
use crate::auth::{AuthenticatedDevice, Permission};
use crate::error::AppError;
use crate::filesystem::RelPath;
use crate::filesystem::managing::{self, DeleteMethod, MoveOutcome};
use crate::filesystem::metadata::Entry;
use crate::jobs::moves::{MoveTask, spawn_move};
use crate::jobs::{JobState, MoveJob, NewMoveJob};
use crate::state::AppState;

// ---------------------------------------------------------------------------
// Rename
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RenameRequest {
    /// The new name (just the name, not a path).
    name: String,
}

/// `POST /api/files/{id}/rename`
pub async fn rename(
    device: AuthenticatedDevice,
    State(state): State<AppState>,
    Path(file_id): Path<String>,
    Json(request): Json<RenameRequest>,
) -> Result<Json<Entry>, AppError> {
    device.require(Permission::Rename)?;

    let (share, path) = look_up_file_id(&state, &file_id).await?;
    let share = writable_share(&state, &share.id).await?;

    let entry = managing::rename(share, path, request.name).await?;
    Ok(Json(entry))
}

// ---------------------------------------------------------------------------
// Move
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MoveRequest {
    /// Destination share. Leave out to stay in the same share.
    #[serde(default)]
    share_id: Option<String>,
    /// Destination folder, e.g. `/Archive`. Must already exist.
    path: String,
}

/// `POST /api/files/{id}/move`
///
/// Within one disk this is an instant rename: `200` with the moved item.
/// To a share on another disk the item has to be copied, so this starts a
/// background job and returns `202` with it; follow it with
/// `GET /api/jobs/{id}`.
pub async fn move_item(
    device: AuthenticatedDevice,
    State(state): State<AppState>,
    Path(file_id): Path<String>,
    Json(request): Json<MoveRequest>,
) -> Result<Response, AppError> {
    device.require(Permission::Move)?;

    let (source_share, source_path) = look_up_file_id(&state, &file_id).await?;
    let source_share = writable_share(&state, &source_share.id).await?;
    let destination_share_id = request.share_id.unwrap_or_else(|| source_share.id.clone());
    let destination_share = writable_share(&state, &destination_share_id).await?;
    let destination_folder = RelPath::parse(&request.path)?;

    let outcome = managing::move_item(
        source_share.clone(),
        source_path.clone(),
        destination_share.clone(),
        destination_folder.clone(),
    )
    .await?;

    match outcome {
        MoveOutcome::Moved(entry) => Ok(Json(entry).into_response()),
        MoveOutcome::NeedsCopy => {
            let slot = transfer_slot(&state, &device)?;
            let (job, running) = state.jobs().create(NewMoveJob {
                device_id: device.id.clone(),
                source_share_id: source_share.id.clone(),
                source_path: source_path.to_string(),
                destination_share_id: destination_share.id.clone(),
                destination_folder: destination_folder.to_string(),
            })?;
            tracing::info!(job_id = %job.id, device_id = %device.id, "move to another disk started");

            let view = JobView::new(&state, &job);
            spawn_move(
                state.clone(),
                MoveTask {
                    job,
                    running,
                    source_share,
                    source_path,
                    destination_share,
                    destination_folder,
                    slot,
                },
            );
            Ok((StatusCode::ACCEPTED, Json(view)).into_response())
        }
    }
}

// ---------------------------------------------------------------------------
// New folder
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewFolderRequest {
    /// The folder to create it in, e.g. `/Photos`. Must already exist.
    path: String,
    /// The new folder's name.
    name: String,
}

/// `POST /api/shares/{id}/directories`
pub async fn create_folder(
    device: AuthenticatedDevice,
    State(state): State<AppState>,
    Path(share_id): Path<String>,
    Json(request): Json<NewFolderRequest>,
) -> Result<(StatusCode, Json<Entry>), AppError> {
    device.require(Permission::Upload)?;

    let share = writable_share(&state, &share_id).await?;
    let parent = RelPath::parse(&request.path)?;

    let entry = managing::create_folder(share, parent, request.name).await?;
    Ok((StatusCode::CREATED, Json(entry)))
}

// ---------------------------------------------------------------------------
// Delete
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct Deleted {
    /// `"trash"`: moved to its disk's Trash; the host user can restore it.
    /// `"permanent"`: that disk has no Trash at all, so it's gone for good.
    deleted: &'static str,
}

/// `DELETE /api/files/{id}`: a file, or a folder with everything in it.
pub async fn delete(
    device: AuthenticatedDevice,
    State(state): State<AppState>,
    Path(file_id): Path<String>,
) -> Result<Json<Deleted>, AppError> {
    device.require(Permission::Delete)?;

    let (share, path) = look_up_file_id(&state, &file_id).await?;
    let share = writable_share(&state, &share.id).await?;

    let bin = state.config().trash_for_tests.clone().unwrap_or_default();
    let method = managing::delete(share, path, bin).await?;
    tracing::info!(device_id = %device.id, ?method, "item deleted");

    let deleted = match method {
        DeleteMethod::Trash => "trash",
        DeleteMethod::Permanent => "permanent",
    };
    Ok(Json(Deleted { deleted }))
}

// ---------------------------------------------------------------------------
// Jobs
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct Location {
    share_id: String,
    path: String,
}

/// A background job as the device sees it.
#[derive(Serialize)]
pub struct JobView {
    id: String,
    kind: &'static str,
    /// `running`, `done`, `failed` or `cancelled`.
    state: &'static str,
    source: Location,
    destination: Location,
    /// Known once the item has been measured; `0` before that.
    bytes_total: u64,
    bytes_copied: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    warning: Option<String>,
    /// The moved item, once done.
    #[serde(skip_serializing_if = "Option::is_none")]
    item: Option<Value>,
    created_at_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    finished_at_ms: Option<u64>,
}

impl JobView {
    fn new(state: &AppState, job: &MoveJob) -> Self {
        use std::sync::atomic::Ordering::Relaxed;

        let (bytes_total, bytes_copied) = match state.jobs().progress(&job.id) {
            Some(progress) => (
                progress.bytes_total.load(Relaxed),
                progress.bytes_copied.load(Relaxed),
            ),
            None => (0, 0),
        };

        Self {
            id: job.id.clone(),
            kind: "move",
            state: job.state.as_str(),
            source: Location {
                share_id: job.source_share_id.clone(),
                path: job.source_path.clone(),
            },
            destination: Location {
                share_id: job.destination_share_id.clone(),
                path: job.destination_folder.clone(),
            },
            bytes_total,
            bytes_copied,
            error: job.error.clone(),
            warning: job.warning.clone(),
            item: job.result.clone(),
            created_at_ms: job.created_at_ms,
            finished_at_ms: job.finished_at_ms,
        }
    }
}

/// `GET /api/jobs/{id}`
pub async fn job_status(
    device: AuthenticatedDevice,
    State(state): State<AppState>,
    Path(job_id): Path<String>,
) -> Result<Json<JobView>, AppError> {
    device.require(Permission::Move)?;
    let job = own_job(&state, &device, &job_id)?;
    Ok(Json(JobView::new(&state, &job)))
}

/// `DELETE /api/jobs/{id}`: stop a running move. The partial copy is
/// removed and the original stays where it was. Returns `202`; the job
/// shows `cancelled` a moment later.
pub async fn cancel_job(
    device: AuthenticatedDevice,
    State(state): State<AppState>,
    Path(job_id): Path<String>,
) -> Result<StatusCode, AppError> {
    device.require(Permission::Move)?;
    let job = own_job(&state, &device, &job_id)?;

    if job.state != JobState::Running || !state.jobs().request_cancel(&job.id) {
        return Err(AppError::Conflict("the job has already finished".into()));
    }
    Ok(StatusCode::ACCEPTED)
}

/// The job, if it exists and belongs to this device (otherwise 404).
fn own_job(
    state: &AppState,
    device: &AuthenticatedDevice,
    job_id: &str,
) -> Result<MoveJob, AppError> {
    match state.jobs().get(job_id)? {
        Some(job) if job.device_id == device.id => Ok(job),
        _ => Err(AppError::NotFound),
    }
}
