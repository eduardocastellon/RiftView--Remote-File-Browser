//! Entry metadata (never reads file contents).

use serde::Serialize;

use super::{FileId, RelPath};

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum EntryKind {
    File,
    Directory,
}

#[derive(Debug, Clone, Serialize)]
pub struct Entry {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub kind: EntryKind,
    /// Path relative to the share root, e.g. `/Documents/report.pdf`.
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    /// Last modification time, Unix epoch milliseconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime: Option<String>,
}

impl Entry {
    /// Build from cap-std metadata. Returns `None` for things that are
    /// neither regular files nor directories (sockets, devices, FIFOs…).
    pub(crate) fn from_cap(
        share_id: &str,
        path: &RelPath,
        meta: &cap_std::fs::Metadata,
    ) -> Option<Self> {
        let kind = if meta.is_dir() {
            EntryKind::Directory
        } else if meta.is_file() {
            EntryKind::File
        } else {
            return None;
        };
        let modified_ms = meta.modified().ok().and_then(|t| {
            t.duration_since(cap_std::time::SystemTime::from_std(std::time::UNIX_EPOCH))
                .ok()
                .map(|d| d.as_millis() as u64)
        });
        Some(Self::build(share_id, path, kind, meta.len(), modified_ms))
    }

    pub(crate) fn build(
        share_id: &str,
        path: &RelPath,
        kind: EntryKind,
        len: u64,
        modified_ms: Option<u64>,
    ) -> Self {
        let name = path.file_name().unwrap_or("").to_owned();
        let (size, mime) = match kind {
            EntryKind::File => (Some(len), Some(guess_mime(&name))),
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

pub fn guess_mime(name: &str) -> String {
    mime_guess::from_path(name).first_or_octet_stream().essence_str().to_owned()
}
