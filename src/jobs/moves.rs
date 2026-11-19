//! Running a move between disks, and tidying up old or interrupted jobs.
//!
//! Steps, on a background thread:
//!
//! 1. Measure the item (and refuse symlinks or special files inside it).
//! 2. Check the destination disk has room for it, plus a safety margin.
//! 3. Copy it to a hidden temporary name in the destination folder.
//! 4. Give the copy its real name (never replacing anything).
//! 5. Delete the original, permanently: the data now lives at the
//!    destination, so this isn't a "delete" the user could regret.
//!
//! If anything fails or the job is cancelled before step 4, the temporary
//! copy is removed and the original is untouched.

use std::sync::atomic::Ordering;

use serde_json::Value;

use crate::error::AppError;
use crate::filesystem::copying::{self, CopyProgress, copy_item, copy_name};
use crate::filesystem::os::{available_bytes, rename_no_replace};
use crate::filesystem::roots::Share;
use crate::filesystem::{FREE_SPACE_MARGIN_BYTES, FsError, RelPath, StopSignal, run_blocking};
use crate::state::AppState;
use crate::transfers::limits::TransferSlot;

use super::{JobOutcome, MoveJob, RunningJob};

/// Finished jobs are kept this long so the device can still read the result.
const KEEP_FINISHED_JOBS_MS: u64 = 24 * 60 * 60 * 1000;

/// Everything a move job needs to run.
pub struct MoveTask {
    pub job: MoveJob,
    pub running: RunningJob,
    pub source_share: Share,
    pub source_path: RelPath,
    pub destination_share: Share,
    pub destination_folder: RelPath,
    /// Held for the whole job, so it counts towards the transfer limits.
    pub slot: TransferSlot,
}

/// Start the job in the background. Its outcome is recorded in the store.
pub fn spawn_move(state: AppState, task: MoveTask) {
    tokio::spawn(async move {
        let job_id = task.job.id.clone();
        let _slot = task.slot;
        let stop = task.running.stop.clone();
        let progress = task.running.progress.clone();

        let result = run_blocking(move || {
            copy_then_delete(
                &job_id,
                &task.source_share,
                &task.source_path,
                &task.destination_share,
                &task.destination_folder,
                &progress,
                &stop,
            )
        })
        .await;

        let outcome = match result {
            Ok((result, warning)) => JobOutcome::Done { result, warning },
            Err(FsError::Abandoned) => JobOutcome::Cancelled,
            // Show the device the same safe message an API error would.
            Err(error) => JobOutcome::Failed(AppError::from(error).to_string()),
        };

        let job_id = &task.job.id;
        match &outcome {
            JobOutcome::Done { .. } => tracing::info!(%job_id, "move finished"),
            JobOutcome::Failed(reason) => tracing::warn!(%job_id, %reason, "move failed"),
            JobOutcome::Cancelled => tracing::info!(%job_id, "move cancelled"),
        }
        if let Err(error) = state.jobs().finish(job_id, outcome) {
            tracing::error!(%job_id, %error, "could not record the end of a move");
        }
    });
}

/// Steps 1–5 above. Returns the moved item as JSON, plus a warning if the
/// original couldn't be fully deleted.
fn copy_then_delete(
    job_id: &str,
    source_share: &Share,
    source_path: &RelPath,
    destination_share: &Share,
    destination_folder: &RelPath,
    progress: &CopyProgress,
    stop: &StopSignal,
) -> Result<(Value, Option<String>), FsError> {
    let source_folder = source_path.parent().ok_or(FsError::NotFound)?;
    let name = source_path.file_name().ok_or(FsError::NotFound)?;
    let source_dir = source_share.open_dir(&source_folder)?;
    let destination_dir = destination_share.open_dir(destination_folder)?;

    // 1–2. Measure, and check there's room.
    let total = copying::measure(&source_dir, name)?;
    progress.bytes_total.store(total, Ordering::Relaxed);
    if let Ok(free) = available_bytes(&destination_dir)
        && free < total.saturating_add(FREE_SPACE_MARGIN_BYTES)
    {
        return Err(FsError::Unsupported(
            "not enough free space on the destination disk",
        ));
    }

    // 3–4. Copy under a hidden name, then give it the real name.
    let temporary_name = copy_name(job_id);
    let copied = copy_item(
        &source_dir,
        name,
        &destination_dir,
        &temporary_name,
        progress,
        stop,
    )
    .and_then(|()| {
        rename_no_replace(&destination_dir, &temporary_name, &destination_dir, name).map_err(
            |error| match error.kind() {
                std::io::ErrorKind::AlreadyExists => FsError::AlreadyExists,
                _ => error.into(),
            },
        )
    });
    if let Err(error) = copied {
        if let Err(cleanup_error) = copying::remove_partial_copy(&destination_dir, &temporary_name)
        {
            tracing::warn!(%cleanup_error, "could not remove a partial copy");
        }
        return Err(error);
    }

    let entry = crate::filesystem::metadata::Entry::from_cap_metadata(
        &destination_share.id,
        &destination_folder.join_name(name)?,
        &destination_dir.metadata(name)?,
    )
    .ok_or(FsError::NotFound)?;
    let result = serde_json::to_value(entry).unwrap_or(Value::Null);

    // 5. Delete the original.
    let warning = match copying::remove_partial_copy(&source_dir, name) {
        Ok(()) => None,
        Err(error) => {
            tracing::warn!(%error, "moved, but the original couldn't be fully deleted");
            Some("moved, but some of the original couldn't be deleted from the source".to_owned())
        }
    };
    Ok((result, warning))
}

/// After a restart, no job is really running: mark the ones recorded as
/// running as failed, and remove their partial copies.
pub async fn recover_interrupted_jobs(state: &AppState) {
    let interrupted = match state.jobs().list_recorded_as_running() {
        Ok(jobs) => jobs,
        Err(error) => {
            tracing::error!(%error, "could not look up interrupted moves");
            return;
        }
    };

    for job in interrupted {
        if let Ok(share) = state.share(&job.destination_share_id).await
            && let Ok(folder) = RelPath::parse(&job.destination_folder)
        {
            let temporary_name = copy_name(&job.id);
            let removed = run_blocking(move || {
                let dir = share.open_dir(&folder)?;
                copying::remove_partial_copy(&dir, &temporary_name)
            })
            .await;
            if let Err(error) = removed {
                tracing::warn!(job_id = %job.id, %error, "could not remove an interrupted copy");
            }
        }

        let reason = "interrupted by a host restart; the original was left in place \
                      unless the copy had already finished"
            .to_owned();
        if let Err(error) = state.jobs().finish(&job.id, JobOutcome::Failed(reason)) {
            tracing::error!(job_id = %job.id, %error, "could not record an interrupted move");
        }
        tracing::info!(job_id = %job.id, "marked an interrupted move as failed");
    }
}

/// Forget jobs that finished more than a day before `now_ms`.
pub fn remove_old_jobs(state: &AppState, now_ms: u64) {
    let cutoff = now_ms.saturating_sub(KEEP_FINISHED_JOBS_MS);
    if let Err(error) = state.jobs().delete_finished_before(cutoff) {
        tracing::error!(%error, "could not delete old job records");
    }
}
