//! Read-side operations: metadata lookup and opening files for streaming.
//! (Write operations arrive in phases 6–7.)

use super::metadata::{Entry, EntryKind};
use super::roots::Share;
use super::{FsError, RelPath, blocking};

/// Metadata for a single file or directory.
pub async fn stat(share: Share, rel: RelPath) -> Result<Entry, FsError> {
    blocking(move || {
        let root = share.open_root()?;
        let meta = if rel.is_root() {
            root.dir_metadata()?
        } else {
            root.metadata(rel.to_cap_path())?
        };
        Entry::from_cap(&share.id, &rel, &meta).ok_or(FsError::NotFound)
    })
    .await
}

/// A regular file opened for reading via a stable handle.
///
/// `size` is captured from the open handle, so a download serves a
/// consistent length even if the file is replaced on disk mid-transfer.
#[derive(Debug)]
pub struct OpenedFile {
    pub file: std::fs::File,
    pub entry: Entry,
    pub size: u64,
}

pub async fn open_file(share: Share, rel: RelPath) -> Result<OpenedFile, FsError> {
    blocking(move || {
        if rel.is_root() {
            return Err(FsError::NotAFile);
        }
        let root = share.open_root()?;
        let cap_path = rel.to_cap_path();

        let mut opts = cap_std::fs::OpenOptions::new();
        opts.read(true);
        #[cfg(unix)]
        {
            // Don't block a worker thread forever if the target is a FIFO.
            use cap_std::fs::OpenOptionsExt;
            opts.custom_flags(libc::O_NONBLOCK);
        }
        let file = root.open_with(&cap_path, &opts)?.into_std();

        // Authoritative check on the handle we actually opened (no TOCTOU).
        let meta = file.metadata()?;
        if !meta.is_file() {
            return Err(FsError::NotAFile);
        }
        let modified_ms = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as u64);
        let size = meta.len();
        let entry = Entry::build(&share.id, &rel, EntryKind::File, size, modified_ms);
        Ok(OpenedFile { file, entry, size })
    })
    .await
}
