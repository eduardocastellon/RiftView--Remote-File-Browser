//! Background jobs. For now there is one kind: moving a file or folder
//! between shares on different disks, which means copying it (possibly
//! for minutes) and then deleting the original.
//!
//! A job is recorded in SQLite (`move_jobs`) so the device can follow it
//! with `GET /api/jobs/{id}`, and so an interrupted copy can be cleaned up
//! after a host restart. Live progress is kept in memory only.
//!
//! - [`JobStore`]: the records and the in-memory state of running jobs.
//! - [`moves`]: running a move job, and cleaning up old or interrupted ones.

pub mod moves;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use rusqlite::{OptionalExtension, Row, params};
use serde_json::Value;

use crate::database::Database;
use crate::filesystem::StopSignal;
use crate::filesystem::copying::CopyProgress;
use crate::filesystem::roots::now_ms;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobState {
    Running,
    Done,
    Failed,
    Cancelled,
}

impl JobState {
    pub fn as_str(self) -> &'static str {
        match self {
            JobState::Running => "running",
            JobState::Done => "done",
            JobState::Failed => "failed",
            JobState::Cancelled => "cancelled",
        }
    }

    fn from_name(name: &str) -> Option<Self> {
        match name {
            "running" => Some(JobState::Running),
            "done" => Some(JobState::Done),
            "failed" => Some(JobState::Failed),
            "cancelled" => Some(JobState::Cancelled),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct MoveJob {
    /// e.g. `job_3f2a…`
    pub id: String,
    pub device_id: String,
    pub source_share_id: String,
    /// The item being moved, e.g. `/Photos/2019`.
    pub source_path: String,
    pub destination_share_id: String,
    /// The folder it goes into, e.g. `/Archive`.
    pub destination_folder: String,
    pub state: JobState,
    /// Why it failed.
    pub error: Option<String>,
    /// Finished, but with a problem worth telling the user about.
    pub warning: Option<String>,
    /// The moved item (a file entry as JSON), once done.
    pub result: Option<Value>,
    pub created_at_ms: u64,
    pub finished_at_ms: Option<u64>,
}

impl MoveJob {
    fn from_row(row: &Row) -> rusqlite::Result<Self> {
        let state_name: String = row.get("state")?;
        let state = JobState::from_name(&state_name).unwrap_or(JobState::Failed);
        let result_json: Option<String> = row.get("result_json")?;

        Ok(Self {
            id: row.get("id")?,
            device_id: row.get("device_id")?,
            source_share_id: row.get("source_share_id")?,
            source_path: row.get("source_path")?,
            destination_share_id: row.get("destination_share_id")?,
            destination_folder: row.get("destination_folder")?,
            state,
            error: row.get("error")?,
            warning: row.get("warning")?,
            result: result_json.and_then(|json| serde_json::from_str(&json).ok()),
            created_at_ms: row.get("created_at_ms")?,
            finished_at_ms: row.get("finished_at_ms")?,
        })
    }
}

/// What a device asked to move (already checked).
pub struct NewMoveJob {
    pub device_id: String,
    pub source_share_id: String,
    pub source_path: String,
    pub destination_share_id: String,
    pub destination_folder: String,
}

/// How a job ended.
pub enum JobOutcome {
    Done {
        result: Value,
        warning: Option<String>,
    },
    Failed(String),
    Cancelled,
}

/// The live part of a running job, shared between the worker and the
/// status endpoint.
#[derive(Clone, Default)]
pub struct RunningJob {
    pub stop: StopSignal,
    pub progress: Arc<CopyProgress>,
}

const SELECT_JOB: &str = "SELECT id, device_id, source_share_id, source_path, \
     destination_share_id, destination_folder, state, error, warning, result_json, \
     created_at_ms, finished_at_ms FROM move_jobs";

pub struct JobStore {
    database: Database,
    /// Jobs running in this process, by ID.
    running: Mutex<HashMap<String, RunningJob>>,
}

impl JobStore {
    pub fn new(database: Database) -> Self {
        Self {
            database,
            running: Mutex::default(),
        }
    }

    /// Record a new job as running.
    pub fn create(&self, new: NewMoveJob) -> rusqlite::Result<(MoveJob, RunningJob)> {
        let job = MoveJob {
            id: format!("job_{}", uuid::Uuid::new_v4().simple()),
            device_id: new.device_id,
            source_share_id: new.source_share_id,
            source_path: new.source_path,
            destination_share_id: new.destination_share_id,
            destination_folder: new.destination_folder,
            state: JobState::Running,
            error: None,
            warning: None,
            result: None,
            created_at_ms: now_ms(),
            finished_at_ms: None,
        };

        self.database.connection().execute(
            "INSERT INTO move_jobs (id, device_id, source_share_id, source_path, \
             destination_share_id, destination_folder, state, created_at_ms) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                job.id,
                job.device_id,
                job.source_share_id,
                job.source_path,
                job.destination_share_id,
                job.destination_folder,
                job.state.as_str(),
                job.created_at_ms,
            ],
        )?;

        let running = RunningJob::default();
        self.running
            .lock()
            .unwrap()
            .insert(job.id.clone(), running.clone());
        Ok((job, running))
    }

    pub fn get(&self, id: &str) -> rusqlite::Result<Option<MoveJob>> {
        self.database
            .connection()
            .query_row(
                &format!("{SELECT_JOB} WHERE id = ?1"),
                params![id],
                MoveJob::from_row,
            )
            .optional()
    }

    /// Live progress of a job running in this process.
    pub fn progress(&self, id: &str) -> Option<Arc<CopyProgress>> {
        let running = self.running.lock().unwrap();
        running.get(id).map(|job| Arc::clone(&job.progress))
    }

    /// Ask a running job to stop. Returns false if it isn't running.
    pub fn request_cancel(&self, id: &str) -> bool {
        match self.running.lock().unwrap().get(id) {
            Some(job) => {
                job.stop.stop();
                true
            }
            None => false,
        }
    }

    /// Record how a job ended and forget its live state.
    pub fn finish(&self, id: &str, outcome: JobOutcome) -> rusqlite::Result<()> {
        let (state, error, warning, result_json) = match outcome {
            JobOutcome::Done { result, warning } => {
                (JobState::Done, None, warning, Some(result.to_string()))
            }
            JobOutcome::Failed(reason) => (JobState::Failed, Some(reason), None, None),
            JobOutcome::Cancelled => (JobState::Cancelled, None, None, None),
        };

        self.database.connection().execute(
            "UPDATE move_jobs SET state = ?1, error = ?2, warning = ?3, result_json = ?4, \
             finished_at_ms = ?5 WHERE id = ?6",
            params![state.as_str(), error, warning, result_json, now_ms(), id],
        )?;
        self.running.lock().unwrap().remove(id);
        Ok(())
    }

    /// Jobs recorded as running. Right after startup, these are jobs that
    /// were interrupted by the restart.
    pub fn list_recorded_as_running(&self) -> rusqlite::Result<Vec<MoveJob>> {
        let connection = self.database.connection();
        let mut query = connection.prepare(&format!("{SELECT_JOB} WHERE state = 'running'"))?;
        query.query_map([], MoveJob::from_row)?.collect()
    }

    /// Is a job copying from or into this share right now?
    pub fn is_share_busy(&self, share_id: &str) -> rusqlite::Result<bool> {
        self.database.connection().query_row(
            "SELECT EXISTS (SELECT 1 FROM move_jobs WHERE state = 'running' \
             AND (source_share_id = ?1 OR destination_share_id = ?1))",
            params![share_id],
            |row| row.get(0),
        )
    }

    /// Delete records of jobs that finished before `cutoff_ms`.
    pub fn delete_finished_before(&self, cutoff_ms: u64) -> rusqlite::Result<usize> {
        self.database.connection().execute(
            "DELETE FROM move_jobs WHERE state != 'running' AND finished_at_ms < ?1",
            params![cutoff_ms],
        )
    }
}
