//! Throwing away uploads that won't be finished: cancelled by the device,
//! expired after a day without activity, failed their final SHA-256 check,
//! or in a share the host is removing.

use std::time::Duration;

use crate::error::AppError;
use crate::filesystem::{FsError, writing};
use crate::jobs;
use crate::state::AppState;

use super::uploads::Upload;

/// How often the background task looks for expired uploads.
const CLEANUP_INTERVAL: Duration = Duration::from_secs(10 * 60);

/// Delete an upload's partial file and its records.
///
/// If the share or the target folder no longer exists, there's no file to
/// delete, so only the records go.
pub async fn discard_upload(state: &AppState, upload: &Upload) -> Result<(), AppError> {
    if let Ok(share) = state.share(&upload.share_id).await {
        let removed =
            writing::remove_upload_file(share, upload.folder()?, upload.partial_file_name()).await;

        match removed {
            Ok(()) | Err(FsError::NotFound | FsError::RootUnavailable) => {}
            Err(error) => return Err(error.into()),
        }
    }

    state.uploads().delete(&upload.id)?;
    Ok(())
}

/// Discard every upload into one share. Called before the share is removed,
/// so no partial files are left behind in a folder we stop managing.
pub async fn discard_uploads_in_share(state: &AppState, share_id: &str) -> Result<(), AppError> {
    for upload in state.uploads().list_for_share(share_id)? {
        discard_upload(state, &upload).await?;
    }
    Ok(())
}

/// Discard uploads with no activity for `upload_expiry_secs`, as of
/// `now_ms`. Returns how many were removed. Uploads that are being
/// finished right now are left alone.
pub async fn remove_expired_uploads(state: &AppState, now_ms: u64) -> usize {
    let expiry_ms = state.config().upload_expiry_secs.saturating_mul(1000);
    let cutoff_ms = now_ms.saturating_sub(expiry_ms);

    let expired = match state.uploads().list_inactive_since(cutoff_ms) {
        Ok(expired) => expired,
        Err(error) => {
            tracing::error!(%error, "could not look up expired uploads");
            return 0;
        }
    };

    let mut removed = 0;
    for upload in expired {
        if state.uploads().is_finishing(&upload.id) {
            continue;
        }
        match discard_upload(state, &upload).await {
            Ok(()) => removed += 1,
            Err(error) => {
                tracing::warn!(upload_id = %upload.id, %error, "could not remove expired upload");
            }
        }
    }

    if removed > 0 {
        tracing::info!(count = removed, "removed expired uploads");
    }
    removed
}

/// Housekeeping for as long as the server runs:
///
/// - at startup, clean up moves interrupted by the restart;
/// - now and every few minutes, remove expired uploads and forget move
///   jobs that finished over a day ago.
pub fn spawn_cleanup_task(state: AppState) {
    tokio::spawn(async move {
        jobs::moves::recover_interrupted_jobs(&state).await;

        let mut timer = tokio::time::interval(CLEANUP_INTERVAL);
        loop {
            timer.tick().await; // The first tick is immediate.
            let now = crate::filesystem::roots::now_ms();
            remove_expired_uploads(&state, now).await;
            jobs::moves::remove_old_jobs(&state, now);
        }
    });
}
