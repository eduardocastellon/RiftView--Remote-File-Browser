//! Looking up a single file or folder, and opening files to download.
//! (Uploads are in [`super::writing`]; renaming and deleting arrive in
//! phase 7.)

use super::metadata::{Entry, EntryKind, to_epoch_ms};
use super::roots::Share;
use super::{FsError, RelPath, is_internal_name, run_blocking};

/// RiftView's temporary files are invisible to devices, even if named
/// directly.
fn refuse_partial_uploads(path: &RelPath) -> Result<(), FsError> {
    match path.file_name() {
        Some(name) if is_internal_name(name) => Err(FsError::NotFound),
        _ => Ok(()),
    }
}

/// Information about one file or folder inside a share.
pub async fn stat(share: Share, path: RelPath) -> Result<Entry, FsError> {
    refuse_partial_uploads(&path)?;
    run_blocking(move || {
        let root = share.open_root()?;

        let metadata = if path.is_root() {
            root.dir_metadata()?
        } else {
            root.metadata(path.to_cap_path())?
        };

        Entry::from_cap_metadata(&share.id, &path, &metadata).ok_or(FsError::NotFound)
    })
    .await
}

/// A regular file, opened and ready to stream.
///
/// `size` is read from the open file handle. If the file is replaced on
/// disk during a download, we keep serving the original consistently.
#[derive(Debug)]
pub struct OpenedFile {
    pub file: std::fs::File,
    pub entry: Entry,
    pub size: u64,
}

pub async fn open_file(share: Share, path: RelPath) -> Result<OpenedFile, FsError> {
    refuse_partial_uploads(&path)?;
    run_blocking(move || {
        if path.is_root() {
            return Err(FsError::NotAFile);
        }

        let root = share.open_root()?;
        let file = root
            .open_with(path.to_cap_path(), &read_only_options())?
            .into_std();

        // Check the type of the file we *actually opened*, not the path.
        // Checking the path first and then opening it would leave a gap in
        // which someone could swap the file out.
        let metadata = file.metadata()?;
        if !metadata.is_file() {
            return Err(FsError::NotAFile);
        }

        let size = metadata.len();
        let modified_ms = metadata.modified().ok().and_then(to_epoch_ms);
        let entry = Entry::new(&share.id, &path, EntryKind::File, size, modified_ms);

        Ok(OpenedFile { file, entry, size })
    })
    .await
}

fn read_only_options() -> cap_std::fs::OpenOptions {
    let mut options = cap_std::fs::OpenOptions::new();
    options.read(true);

    // Opening a named pipe (FIFO) normally waits until someone writes to
    // it, which could tie up a thread forever. Non-blocking mode returns
    // straight away; the "is it a regular file?" check then rejects it.
    // Regular files are unaffected by this flag.
    #[cfg(unix)]
    {
        use cap_std::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK);
    }

    options
}
