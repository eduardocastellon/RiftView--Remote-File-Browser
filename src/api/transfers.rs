//! Uploads: start, send chunks, check progress, finish, cancel.
//!
//! ```text
//! POST   /api/transfers                       start  → {id, chunk_size, chunk_count, …}
//! PUT    /api/transfers/{id}/chunks/{index}   one chunk, with X-Chunk-SHA256
//! GET    /api/transfers/{id}                  progress (which chunks are missing)
//! GET    /api/transfers                       this device's unfinished uploads
//! POST   /api/transfers/{id}/complete         hash, check, and save as the real file
//! DELETE /api/transfers/{id}                  cancel
//! ```
//!
//! Every route needs the `upload` permission, and the share must be marked
//! writable by the host. A device only ever sees its own uploads.

use axum::Json;
use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use serde::{Deserialize, Serialize};

use crate::auth::{AuthenticatedDevice, Permission};
use crate::error::AppError;
use crate::filesystem::metadata::Entry;
use crate::filesystem::roots::Share;
use crate::filesystem::{FsError, RelPath, writing};
use crate::state::AppState;
use crate::transfers::cleanup;
use crate::transfers::hashing;
use crate::transfers::limits::TransferSlot;
use crate::transfers::uploads::{NewUpload, Upload, UploadLimits};

/// Header carrying a chunk's SHA-256 (lowercase hex).
const CHUNK_HASH_HEADER: &str = "x-chunk-sha256";

/// Room left on the disk after an upload, so it never fills up completely.
const FREE_SPACE_MARGIN_BYTES: u64 = 1024 * 1024 * 1024; // 1 GiB

// ---------------------------------------------------------------------------
// Responses
// ---------------------------------------------------------------------------

/// An unfinished upload, as the device sees it.
#[derive(Serialize)]
pub struct UploadView {
    id: String,
    share_id: String,
    /// Target folder.
    path: String,
    name: String,
    size: u64,
    sha256: String,
    chunk_size: u64,
    chunk_count: u64,
    received_count: u64,
    /// Indexes of chunks still to send, in order.
    missing_chunks: Vec<u64>,
    created_at_ms: u64,
    /// When the upload will be deleted if no more chunks arrive.
    expires_at_ms: u64,
}

impl UploadView {
    fn new(state: &AppState, upload: &Upload) -> Result<Self, AppError> {
        let received = state.uploads().received_chunks(&upload.id)?;
        let missing_chunks = (0..upload.chunk_count())
            .filter(|index| !received.contains(index))
            .collect();
        let expiry_ms = state.config().upload_expiry_secs.saturating_mul(1000);

        Ok(Self {
            id: upload.id.clone(),
            share_id: upload.share_id.clone(),
            path: upload.folder_path.clone(),
            name: upload.file_name.clone(),
            size: upload.size,
            sha256: upload.sha256.clone(),
            chunk_size: upload.chunk_size,
            chunk_count: upload.chunk_count(),
            received_count: received.len() as u64,
            missing_chunks,
            created_at_ms: upload.created_at_ms,
            expires_at_ms: upload.last_activity_ms.saturating_add(expiry_ms),
        })
    }
}

#[derive(Serialize)]
pub struct UploadList {
    uploads: Vec<UploadView>,
}

#[derive(Serialize)]
pub struct ChunkReceived {
    received_count: u64,
    chunk_count: u64,
}

#[derive(Serialize)]
pub struct FinishedUpload {
    /// The new file. Its `name` may differ from the one asked for.
    file: Entry,
    /// True if the name was taken, so the file was saved as "name (1).ext".
    renamed: bool,
}

// ---------------------------------------------------------------------------
// Start
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StartRequest {
    share_id: String,
    /// Target folder inside the share, e.g. `/Photos`. Must already exist.
    path: String,
    /// File name to save as.
    name: String,
    /// Exact size in bytes.
    size: u64,
    /// SHA-256 of the whole file, hex.
    sha256: String,
}

/// `POST /api/transfers`
pub async fn start(
    device: AuthenticatedDevice,
    State(state): State<AppState>,
    Json(request): Json<StartRequest>,
) -> Result<(StatusCode, Json<UploadView>), AppError> {
    device.require(Permission::Upload)?;

    let share = writable_share(&state, &request.share_id).await?;
    let folder = RelPath::parse(&request.path)?;
    writing::validate_upload_name(&request.name)?;
    let sha256 = parse_sha256(&request.sha256)?;

    let max_size = state.config().max_upload_bytes;
    if request.size > max_size {
        return Err(AppError::PayloadTooLarge(format!(
            "file is larger than this host's upload limit ({max_size} bytes)"
        )));
    }

    // Also checks the folder exists.
    let free_bytes = writing::free_space(share.clone(), folder.clone()).await?;
    if let Some(free_bytes) = free_bytes {
        let already_promised = state.uploads().bytes_reserved_in_share(&share.id)?;
        let needed = request
            .size
            .saturating_add(already_promised)
            .saturating_add(FREE_SPACE_MARGIN_BYTES);
        if free_bytes < needed {
            return Err(AppError::InsufficientStorage);
        }
    }

    let config = state.config();
    let limits = UploadLimits {
        per_device: config.max_uploads_per_device,
        total: config.max_uploads_total,
    };
    let new_upload = NewUpload {
        device_id: device.id.clone(),
        share_id: share.id.clone(),
        folder_path: folder.to_string(),
        file_name: request.name,
        size: request.size,
        sha256,
    };
    let upload = state.uploads().create(new_upload, limits)?;

    let created =
        writing::create_upload_file(share, folder, upload.partial_file_name(), upload.size).await;
    if let Err(error) = created {
        state.uploads().delete(&upload.id)?;
        return Err(error.into());
    }

    tracing::info!(upload_id = %upload.id, device_id = %device.id, size = upload.size, "upload started");
    Ok((StatusCode::CREATED, Json(UploadView::new(&state, &upload)?)))
}

// ---------------------------------------------------------------------------
// Chunks
// ---------------------------------------------------------------------------

/// `PUT /api/transfers/{id}/chunks/{index}`
///
/// The body is the chunk's bytes; `X-Chunk-SHA256` is their SHA-256. A
/// chunk that fails the check is rejected (`422 checksum_mismatch`) and can
/// simply be sent again. Sending a chunk that already arrived is harmless.
pub async fn put_chunk(
    device: AuthenticatedDevice,
    State(state): State<AppState>,
    Path((upload_id, index)): Path<(String, u64)>,
    headers: HeaderMap,
    body: Body,
) -> Result<Json<ChunkReceived>, AppError> {
    device.require(Permission::Upload)?;

    let upload = own_upload(&state, &device, &upload_id)?;
    let share = writable_share(&state, &upload.share_id).await?;

    let expected_length = upload
        .chunk_length(index)
        .ok_or_else(|| AppError::BadRequest("chunk index out of range".into()))?;
    let expected_hash = headers
        .get(CHUNK_HASH_HEADER)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| AppError::BadRequest("missing X-Chunk-SHA256 header".into()))
        .and_then(parse_sha256)?;

    if state.uploads().is_finishing(&upload.id) {
        return Err(AppError::Conflict("upload is being finished".into()));
    }

    // Hold a transfer slot while the body arrives.
    let _slot = transfer_slot(&state, &device)?;
    let bytes = axum::body::to_bytes(body, expected_length as usize + 1)
        .await
        .map_err(|_| AppError::BadRequest("chunk is larger than expected".into()))?;
    if bytes.len() as u64 != expected_length {
        return Err(AppError::BadRequest(format!(
            "chunk {index} must be exactly {expected_length} bytes"
        )));
    }

    let actual_hash = hashing::to_hex(&hashing::sha256_of_bytes(&bytes));
    if actual_hash != expected_hash {
        return Err(AppError::ChecksumMismatch(format!(
            "chunk {index} doesn't match its X-Chunk-SHA256; send it again"
        )));
    }

    // A resent chunk has the same, already verified, contents: don't write
    // it again. (Its hash matched, so it's the same data anyway.)
    if !state.uploads().has_chunk(&upload.id, index)? {
        writing::write_chunk(
            share,
            upload.folder()?,
            upload.partial_file_name(),
            upload.chunk_offset(index),
            bytes,
        )
        .await?;
    }
    state.uploads().record_chunk(&upload.id, index)?;

    let received_count = state.uploads().received_chunks(&upload.id)?.len() as u64;
    Ok(Json(ChunkReceived {
        received_count,
        chunk_count: upload.chunk_count(),
    }))
}

// ---------------------------------------------------------------------------
// Progress
// ---------------------------------------------------------------------------

/// `GET /api/transfers/{id}`
pub async fn status(
    device: AuthenticatedDevice,
    State(state): State<AppState>,
    Path(upload_id): Path<String>,
) -> Result<Json<UploadView>, AppError> {
    device.require(Permission::Upload)?;
    let upload = own_upload(&state, &device, &upload_id)?;
    Ok(Json(UploadView::new(&state, &upload)?))
}

/// `GET /api/transfers`: this device's unfinished uploads, so an app that
/// crashed or was reinstalled can find and resume them.
pub async fn list(
    device: AuthenticatedDevice,
    State(state): State<AppState>,
) -> Result<Json<UploadList>, AppError> {
    device.require(Permission::Upload)?;

    let uploads = state
        .uploads()
        .list_for_device(&device.id)?
        .iter()
        .map(|upload| UploadView::new(&state, upload))
        .collect::<Result<_, _>>()?;
    Ok(Json(UploadList { uploads }))
}

// ---------------------------------------------------------------------------
// Finish and cancel
// ---------------------------------------------------------------------------

/// `POST /api/transfers/{id}/complete`
///
/// Needs every chunk. Hashes the whole partial file (this can take a while
/// for big files, so the route has no request timeout), compares it with
/// the SHA-256 given at the start, and saves it under its name, or
/// "name (1).ext" if that's taken. A mismatch discards the upload.
pub async fn complete(
    device: AuthenticatedDevice,
    State(state): State<AppState>,
    Path(upload_id): Path<String>,
) -> Result<(StatusCode, Json<FinishedUpload>), AppError> {
    device.require(Permission::Upload)?;

    let upload = own_upload(&state, &device, &upload_id)?;
    let share = writable_share(&state, &upload.share_id).await?;
    let folder = upload.folder()?;

    let _finishing = state
        .uploads()
        .start_finishing(&upload.id)
        .ok_or_else(|| AppError::Conflict("upload is already being finished".into()))?;

    let received = state.uploads().received_chunks(&upload.id)?;
    let missing = upload.chunk_count() - received.len() as u64;
    if missing > 0 {
        return Err(AppError::Conflict(format!(
            "{missing} chunk(s) still missing"
        )));
    }

    let _slot = transfer_slot(&state, &device)?;
    let partial_file =
        writing::open_upload_file(share.clone(), folder.clone(), upload.partial_file_name())
            .await?;
    let actual_hash = hashing::sha256_of(tokio::fs::File::from_std(partial_file))
        .await
        .map_err(FsError::from)?;

    if hashing::to_hex(&actual_hash) != upload.sha256 {
        cleanup::discard_upload(&state, &upload).await?;
        tracing::warn!(upload_id = %upload.id, "upload discarded: whole-file SHA-256 mismatch");
        return Err(AppError::ChecksumMismatch(
            "the uploaded file doesn't match the sha256 given at the start; \
             the upload was discarded, start again"
                .into(),
        ));
    }

    let file = writing::finish_upload(
        share,
        folder,
        upload.partial_file_name(),
        upload.file_name.clone(),
    )
    .await?;
    state.uploads().delete(&upload.id)?;

    let renamed = file.name != upload.file_name;
    tracing::info!(upload_id = %upload.id, file_id = %file.id, renamed, "upload finished");
    Ok((StatusCode::CREATED, Json(FinishedUpload { file, renamed })))
}

/// `DELETE /api/transfers/{id}`
pub async fn cancel(
    device: AuthenticatedDevice,
    State(state): State<AppState>,
    Path(upload_id): Path<String>,
) -> Result<StatusCode, AppError> {
    device.require(Permission::Upload)?;

    let upload = own_upload(&state, &device, &upload_id)?;
    let _finishing = state
        .uploads()
        .start_finishing(&upload.id)
        .ok_or_else(|| AppError::Conflict("upload is being finished".into()))?;

    cleanup::discard_upload(&state, &upload).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// The upload, if it exists and belongs to this device. Someone else's
/// upload is a plain 404, so devices can't probe for them.
fn own_upload(
    state: &AppState,
    device: &AuthenticatedDevice,
    upload_id: &str,
) -> Result<Upload, AppError> {
    match state.uploads().get(upload_id)? {
        Some(upload) if upload.device_id == device.id => Ok(upload),
        _ => Err(AppError::NotFound),
    }
}

/// The share, if it exists and the host allows changes in it.
async fn writable_share(state: &AppState, share_id: &str) -> Result<Share, AppError> {
    let share = state.share(share_id).await?;
    if !share.writable {
        return Err(AppError::Forbidden);
    }
    Ok(share)
}

fn transfer_slot(state: &AppState, device: &AuthenticatedDevice) -> Result<TransferSlot, AppError> {
    state
        .transfer_limiter()
        .try_start(&device.id)
        .ok_or(AppError::TooManyRequests)
}

/// Accept 64 hex characters, either case; return them in lowercase.
fn parse_sha256(text: &str) -> Result<String, AppError> {
    let text = text.trim();
    let is_sha256 = text.len() == 64 && text.chars().all(|c| c.is_ascii_hexdigit());
    if !is_sha256 {
        return Err(AppError::BadRequest(
            "sha256 must be 64 hexadecimal characters".into(),
        ));
    }
    Ok(text.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_must_be_64_hex_characters() {
        let upper = "B94D27B9934D3E08A52E52D7DA7DABFAC484EFE37A5380EE9088F7ACE2EFCDE9";
        assert_eq!(parse_sha256(upper).unwrap(), upper.to_ascii_lowercase());
        assert!(parse_sha256("abc").is_err());
        assert!(parse_sha256(&"z".repeat(64)).is_err());
    }
}
