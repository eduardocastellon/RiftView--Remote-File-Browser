//! Unfinished uploads, stored in SQLite so they survive a restart.
//!
//! An upload is split into numbered chunks of [`CHUNK_BYTES`] (the last
//! one may be shorter). The device can send them in any order, several at
//! once, and resend any of them. Each chunk that arrives and passes its
//! SHA-256 check gets a row in `upload_chunks`. When every chunk is in, the
//! device asks to finish, the whole file is hashed, and the upload's rows
//! are deleted.
//!
//! The data itself lives in a hidden partial file in the target folder (see
//! [`crate::filesystem::writing`]); this store only tracks progress.

use std::collections::{BTreeSet, HashSet};
use std::sync::Mutex;

use rusqlite::{OptionalExtension, Row, params};

use crate::database::Database;
use crate::filesystem::roots::now_ms;
use crate::filesystem::writing::upload_file_name;
use crate::filesystem::{FsError, RelPath};

/// Size of every chunk except possibly the last: 4 MiB.
pub const CHUNK_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct Upload {
    /// e.g. `upl_3f2a…`
    pub id: String,
    pub device_id: String,
    pub share_id: String,
    /// Target folder inside the share, e.g. `/Photos`.
    pub folder_path: String,
    /// The name the device asked for. The final name may get a " (1)" if
    /// it's taken by then.
    pub file_name: String,
    pub size: u64,
    pub chunk_size: u64,
    /// Expected SHA-256 of the whole file, lowercase hex.
    pub sha256: String,
    pub created_at_ms: u64,
    /// When the upload started or last received a chunk.
    pub last_activity_ms: u64,
}

impl Upload {
    pub fn chunk_count(&self) -> u64 {
        self.size.div_ceil(self.chunk_size)
    }

    /// Where chunk `index` starts in the file.
    pub fn chunk_offset(&self, index: u64) -> u64 {
        index * self.chunk_size
    }

    /// How many bytes chunk `index` must contain, or `None` if there's no
    /// such chunk. Every chunk is `chunk_size` except the last.
    pub fn chunk_length(&self, index: u64) -> Option<u64> {
        if index >= self.chunk_count() {
            return None;
        }
        let remaining = self.size - self.chunk_offset(index);
        Some(remaining.min(self.chunk_size))
    }

    /// The target folder as a checked path. It was checked before being
    /// stored, so this only fails if the database was edited by hand.
    pub fn folder(&self) -> Result<RelPath, FsError> {
        RelPath::parse(&self.folder_path)
    }

    /// Name of the hidden partial file in the target folder.
    pub fn partial_file_name(&self) -> String {
        upload_file_name(&self.id)
    }

    fn from_row(row: &Row) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get("id")?,
            device_id: row.get("device_id")?,
            share_id: row.get("share_id")?,
            folder_path: row.get("folder_path")?,
            file_name: row.get("file_name")?,
            size: row.get("size")?,
            chunk_size: row.get("chunk_size")?,
            sha256: row.get("sha256")?,
            created_at_ms: row.get("created_at_ms")?,
            last_activity_ms: row.get("last_activity_ms")?,
        })
    }
}

/// What a device asks for when starting an upload (already validated).
pub struct NewUpload {
    pub device_id: String,
    pub share_id: String,
    pub folder_path: String,
    pub file_name: String,
    pub size: u64,
    pub sha256: String,
}

/// How many unfinished uploads may be open at once.
#[derive(Debug, Clone, Copy)]
pub struct UploadLimits {
    pub per_device: usize,
    pub total: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum UploadError {
    #[error("too many unfinished uploads")]
    TooMany,

    #[error("database: {0}")]
    Database(#[from] rusqlite::Error),
}

const SELECT_UPLOAD: &str = "SELECT id, device_id, share_id, folder_path, file_name, size, \
     chunk_size, sha256, created_at_ms, last_activity_ms FROM uploads";

pub struct UploadStore {
    database: Database,
    /// Uploads currently being finished. While an upload is in here, no
    /// chunk may be written to it and it can't be finished a second time.
    finishing: Mutex<HashSet<String>>,
}

impl UploadStore {
    pub fn new(database: Database) -> Self {
        Self {
            database,
            finishing: Mutex::default(),
        }
    }

    /// Record a new upload, unless the device or the host is already at its
    /// limit. Counting and inserting happen under one database lock, so two
    /// requests at once can't both squeeze past the limit.
    pub fn create(&self, new: NewUpload, limits: UploadLimits) -> Result<Upload, UploadError> {
        let connection = self.database.connection();

        let open_for_device: usize = connection.query_row(
            "SELECT COUNT(*) FROM uploads WHERE device_id = ?1",
            params![new.device_id],
            |row| row.get(0),
        )?;
        let open_in_total: usize =
            connection.query_row("SELECT COUNT(*) FROM uploads", [], |row| row.get(0))?;
        if open_for_device >= limits.per_device || open_in_total >= limits.total {
            return Err(UploadError::TooMany);
        }

        let now = now_ms();
        let upload = Upload {
            id: format!("upl_{}", uuid::Uuid::new_v4().simple()),
            device_id: new.device_id,
            share_id: new.share_id,
            folder_path: new.folder_path,
            file_name: new.file_name,
            size: new.size,
            chunk_size: CHUNK_BYTES,
            sha256: new.sha256,
            created_at_ms: now,
            last_activity_ms: now,
        };

        connection.execute(
            "INSERT INTO uploads (id, device_id, share_id, folder_path, file_name, size, \
             chunk_size, sha256, created_at_ms, last_activity_ms) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                upload.id,
                upload.device_id,
                upload.share_id,
                upload.folder_path,
                upload.file_name,
                upload.size,
                upload.chunk_size,
                upload.sha256,
                upload.created_at_ms,
                upload.last_activity_ms,
            ],
        )?;

        Ok(upload)
    }

    pub fn get(&self, id: &str) -> rusqlite::Result<Option<Upload>> {
        self.database
            .connection()
            .query_row(
                &format!("{SELECT_UPLOAD} WHERE id = ?1"),
                params![id],
                Upload::from_row,
            )
            .optional()
    }

    pub fn list_for_device(&self, device_id: &str) -> rusqlite::Result<Vec<Upload>> {
        self.select_many(
            &format!("{SELECT_UPLOAD} WHERE device_id = ?1 ORDER BY created_at_ms"),
            device_id,
        )
    }

    pub fn list_for_share(&self, share_id: &str) -> rusqlite::Result<Vec<Upload>> {
        self.select_many(&format!("{SELECT_UPLOAD} WHERE share_id = ?1"), share_id)
    }

    /// Uploads with no activity since `cutoff_ms`.
    pub fn list_inactive_since(&self, cutoff_ms: u64) -> rusqlite::Result<Vec<Upload>> {
        self.select_many(
            &format!("{SELECT_UPLOAD} WHERE last_activity_ms < ?1"),
            cutoff_ms,
        )
    }

    fn select_many(
        &self,
        sql: &str,
        parameter: impl rusqlite::ToSql,
    ) -> rusqlite::Result<Vec<Upload>> {
        let connection = self.database.connection();
        let mut query = connection.prepare(sql)?;
        query
            .query_map(params![parameter], Upload::from_row)?
            .collect()
    }

    /// Total size of the unfinished uploads into one share. Their space
    /// isn't used up on disk yet, so the free-space check counts it.
    pub fn bytes_reserved_in_share(&self, share_id: &str) -> rusqlite::Result<u64> {
        self.database.connection().query_row(
            "SELECT COALESCE(SUM(size), 0) FROM uploads WHERE share_id = ?1",
            params![share_id],
            |row| row.get(0),
        )
    }

    /// Indexes of the chunks received so far.
    pub fn received_chunks(&self, upload_id: &str) -> rusqlite::Result<BTreeSet<u64>> {
        let connection = self.database.connection();
        let mut query = connection.prepare(
            "SELECT chunk_index FROM upload_chunks WHERE upload_id = ?1 ORDER BY chunk_index",
        )?;
        query
            .query_map(params![upload_id], |row| row.get(0))?
            .collect()
    }

    pub fn has_chunk(&self, upload_id: &str, index: u64) -> rusqlite::Result<bool> {
        self.database.connection().query_row(
            "SELECT EXISTS (SELECT 1 FROM upload_chunks WHERE upload_id = ?1 AND chunk_index = ?2)",
            params![upload_id, index],
            |row| row.get(0),
        )
    }

    /// Note that a chunk arrived and was written. Receiving the same chunk
    /// twice is harmless. Also resets the upload's expiry clock.
    pub fn record_chunk(&self, upload_id: &str, index: u64) -> rusqlite::Result<()> {
        let connection = self.database.connection();
        connection.execute(
            "INSERT OR IGNORE INTO upload_chunks (upload_id, chunk_index) VALUES (?1, ?2)",
            params![upload_id, index],
        )?;
        connection.execute(
            "UPDATE uploads SET last_activity_ms = ?1 WHERE id = ?2",
            params![now_ms(), upload_id],
        )?;
        Ok(())
    }

    /// Forget an upload and its chunks.
    pub fn delete(&self, upload_id: &str) -> rusqlite::Result<()> {
        self.database
            .connection()
            .execute("DELETE FROM uploads WHERE id = ?1", params![upload_id])?;
        Ok(())
    }

    /// Mark an upload as being finished. Returns `None` if it already is.
    /// The mark is removed when the returned guard is dropped.
    pub fn start_finishing(&self, upload_id: &str) -> Option<FinishingGuard<'_>> {
        let newly_marked = self.finishing.lock().unwrap().insert(upload_id.to_owned());
        newly_marked.then(|| FinishingGuard {
            store: self,
            upload_id: upload_id.to_owned(),
        })
    }

    pub fn is_finishing(&self, upload_id: &str) -> bool {
        self.finishing.lock().unwrap().contains(upload_id)
    }
}

/// See [`UploadStore::start_finishing`].
pub struct FinishingGuard<'a> {
    store: &'a UploadStore,
    upload_id: String,
}

impl Drop for FinishingGuard<'_> {
    fn drop(&mut self) {
        self.store.finishing.lock().unwrap().remove(&self.upload_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn upload_of_size(size: u64) -> Upload {
        Upload {
            id: "upl_1".into(),
            device_id: "dev_1".into(),
            share_id: "shr_1".into(),
            folder_path: "/".into(),
            file_name: "a.bin".into(),
            size,
            chunk_size: 10,
            sha256: String::new(),
            created_at_ms: 0,
            last_activity_ms: 0,
        }
    }

    #[test]
    fn chunks_cover_the_file_exactly() {
        let upload = upload_of_size(25);
        assert_eq!(upload.chunk_count(), 3);
        assert_eq!(upload.chunk_length(0), Some(10));
        assert_eq!(upload.chunk_length(2), Some(5), "last chunk is shorter");
        assert_eq!(upload.chunk_length(3), None);
        assert_eq!(upload.chunk_offset(2), 20);

        assert_eq!(upload_of_size(20).chunk_count(), 2);
        assert_eq!(upload_of_size(0).chunk_count(), 0, "empty file, no chunks");
    }

    #[test]
    fn finishing_twice_is_refused() {
        let store = UploadStore::new(Database::open_in_memory().unwrap());

        let guard = store.start_finishing("upl_1").unwrap();
        assert!(store.is_finishing("upl_1"));
        assert!(store.start_finishing("upl_1").is_none());

        drop(guard);
        assert!(!store.is_finishing("upl_1"));
        assert!(store.start_finishing("upl_1").is_some());
    }
}
