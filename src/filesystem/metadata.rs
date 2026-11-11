//! Information about a file or folder (never reads its contents).

use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

use super::{FileId, RelPath};

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum EntryKind {
    File,
    Directory,
}

/// One file or folder, as returned to clients.
#[derive(Debug, Clone, Serialize)]
pub struct Entry {
    pub id: String,

    pub name: String,

    #[serde(rename = "type")]
    pub kind: EntryKind,

    /// Path inside the share, e.g. `/Documents/report.pdf`.
    pub path: String,

    /// Size in bytes. Files only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,

    /// Last modified time, in Unix epoch milliseconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified_ms: Option<u64>,

    /// Best guess at the content type from the file extension. Files only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime: Option<String>,
}

impl Entry {
    /// Build from `cap-std` metadata.
    ///
    /// Returns `None` for anything that isn't a regular file or folder
    /// (sockets, devices, named pipes…). Those are never shown to clients.
    pub(crate) fn from_cap_metadata(
        share_id: &str,
        path: &RelPath,
        metadata: &cap_std::fs::Metadata,
    ) -> Option<Self> {
        let kind = if metadata.is_dir() {
            EntryKind::Directory
        } else if metadata.is_file() {
            EntryKind::File
        } else {
            return None;
        };

        let epoch = cap_std::time::SystemTime::from_std(UNIX_EPOCH);
        let modified_ms = metadata
            .modified()
            .ok()
            .and_then(|modified| modified.duration_since(epoch).ok())
            .map(|elapsed| elapsed.as_millis() as u64);

        Some(Self::new(share_id, path, kind, metadata.len(), modified_ms))
    }

    pub(crate) fn new(
        share_id: &str,
        path: &RelPath,
        kind: EntryKind,
        size_in_bytes: u64,
        modified_ms: Option<u64>,
    ) -> Self {
        let name = path.file_name().unwrap_or("").to_owned();

        let (size, mime) = match kind {
            EntryKind::File => (Some(size_in_bytes), Some(guess_mime_type(&name))),
            EntryKind::Directory => (None, None),
        };

        Self {
            id: FileId::new(share_id, path.clone()).encode(),
            name,
            kind,
            path: path.to_string(),
            size,
            modified_ms,
            mime,
        }
    }
}

/// Guess a MIME type from a file name, e.g. `report.pdf` → `application/pdf`.
pub fn guess_mime_type(file_name: &str) -> String {
    mime_guess::from_path(file_name)
        .first_or_octet_stream()
        .essence_str()
        .to_owned()
}

/// Convert a standard-library timestamp to Unix epoch milliseconds.
pub(crate) fn to_epoch_ms(time: SystemTime) -> Option<u64> {
    time.duration_since(UNIX_EPOCH)
        .ok()
        .map(|elapsed| elapsed.as_millis() as u64)
}
