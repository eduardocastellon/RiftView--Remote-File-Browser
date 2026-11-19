//! Copying a file or folder to another disk, for moves between shares that
//! aren't on the same disk (a plain rename can't cross disks).
//!
//! The copy is made under a hidden temporary name in the destination
//! folder, e.g. `.riftview-move-job_3f2a….part`, and only gets its real name
//! once it's complete. So a half-finished copy is never visible, and if
//! anything goes wrong the temporary copy is simply removed: the original
//! is untouched until the very end.
//!
//! These functions are blocking; they run on a background thread.

use std::io::{ErrorKind, Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use cap_std::fs::{Dir, OpenOptions};

use super::{FsError, StopSignal};

const COPY_NAME_PREFIX: &str = ".riftview-move-";
const COPY_NAME_SUFFIX: &str = ".part";

/// How much we read and write at a time while copying.
const COPY_BUFFER_BYTES: usize = 1024 * 1024;

/// Hidden name of the temporary copy for a move job.
pub fn copy_name(job_id: &str) -> String {
    format!("{COPY_NAME_PREFIX}{job_id}{COPY_NAME_SUFFIX}")
}

/// Shared with whoever reports progress (the job status endpoint).
#[derive(Debug, Default)]
pub struct CopyProgress {
    pub bytes_total: AtomicU64,
    pub bytes_copied: AtomicU64,
}

/// Total size in bytes of `dir/name` and everything in it.
///
/// Fails if it contains anything other than plain files and folders
/// (symlinks, named pipes…): copying those to another disk could change
/// what they mean, so we don't.
pub fn measure(dir: &Dir, name: &str) -> Result<u64, FsError> {
    let metadata = dir.symlink_metadata(name)?;

    if metadata.is_file() {
        return Ok(metadata.len());
    }
    if !metadata.is_dir() {
        return Err(not_copyable());
    }

    let folder = dir.open_dir(name)?;
    let mut total = 0;
    for entry in folder.entries()? {
        let child_name = entry?
            .file_name()
            .into_string()
            .map_err(|_| FsError::Unsupported("contains a name that isn't valid text"))?;
        if is_apple_double_companion(&folder, &child_name) {
            continue;
        }
        total += measure(&folder, &child_name)?;
    }
    Ok(total)
}

fn not_copyable() -> FsError {
    FsError::Unsupported(
        "contains symlinks or special files, which can't be moved to another disk; \
         move it on the host instead",
    )
}

/// Copy `from_dir/from_name` (a file or a whole folder) to
/// `to_dir/to_name`, which must not exist yet.
///
/// Stops with [`FsError::Abandoned`] as soon as `stop` is set. Whatever was
/// copied so far is left for the caller to remove with
/// [`remove_partial_copy`].
pub fn copy_item(
    from_dir: &Dir,
    from_name: &str,
    to_dir: &Dir,
    to_name: &str,
    progress: &CopyProgress,
    stop: &StopSignal,
) -> Result<(), FsError> {
    if stop.should_stop() {
        return Err(FsError::Abandoned);
    }

    let metadata = from_dir.symlink_metadata(from_name)?;
    if metadata.is_file() {
        return copy_file(from_dir, from_name, to_dir, to_name, progress, stop);
    }
    if !metadata.is_dir() {
        return Err(not_copyable());
    }

    to_dir.create_dir(to_name)?;
    let from_folder = from_dir.open_dir(from_name)?;
    let to_folder = to_dir.open_dir(to_name)?;

    for entry in from_folder.entries()? {
        let child_name = entry?
            .file_name()
            .into_string()
            .map_err(|_| FsError::Unsupported("contains a name that isn't valid text"))?;
        if is_apple_double_companion(&from_folder, &child_name) {
            continue;
        }
        copy_item(
            &from_folder,
            &child_name,
            &to_folder,
            &child_name,
            progress,
            stop,
        )?;
    }
    Ok(())
}

fn copy_file(
    from_dir: &Dir,
    from_name: &str,
    to_dir: &Dir,
    to_name: &str,
    progress: &CopyProgress,
    stop: &StopSignal,
) -> Result<(), FsError> {
    let mut source = from_dir.open(from_name)?;
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    let mut target = to_dir.open_with(to_name, &options)?;

    let mut buffer = vec![0u8; COPY_BUFFER_BYTES];
    loop {
        if stop.should_stop() {
            return Err(FsError::Abandoned);
        }
        let bytes_read = source.read(&mut buffer)?;
        if bytes_read == 0 {
            break;
        }
        target.write_all(&buffer[..bytes_read])?;
        progress
            .bytes_copied
            .fetch_add(bytes_read as u64, Ordering::Relaxed);
    }

    // The original is deleted after the copy, so the copy must really be on
    // disk first.
    target.sync_all()?;

    // Keep the file's permissions and modified time, like Finder does.
    let source_metadata = source.metadata()?;
    target.set_permissions(source_metadata.permissions())?;
    if let Ok(modified) = source_metadata.modified() {
        target.into_std().set_modified(modified.into_std())?;
    }
    Ok(())
}

/// Is `name` a macOS "AppleDouble" companion file, i.e. `._photo.jpg` next
/// to `photo.jpg`?
///
/// On disks that can't store macOS metadata natively (exFAT, FAT, many
/// network drives), macOS keeps it in these hidden companion files. They
/// aren't real content: copying them to a Mac disk would just leave junk
/// files, so a copy skips them (as Finder does), and macOS makes new ones
/// at the destination if it needs to.
fn is_apple_double_companion(dir: &Dir, name: &str) -> bool {
    match name.strip_prefix("._") {
        Some(partner) if !partner.is_empty() => dir.symlink_metadata(partner).is_ok(),
        _ => false,
    }
}

/// Remove a temporary copy (a file or a whole folder). Already gone is fine.
pub fn remove_partial_copy(dir: &Dir, name: &str) -> Result<(), FsError> {
    remove_tree(dir, name)
}

/// Permanently remove a file, or a folder with everything in it. Never
/// follows symlinks: a link is removed, not what it points to.
///
/// Anything that disappears while we work is fine. On exFAT, for example,
/// macOS deletes the hidden `._name` companion together with `name`, so it
/// may be gone by the time we reach it.
pub fn remove_tree(dir: &Dir, name: impl AsRef<Path>) -> Result<(), FsError> {
    let name = name.as_ref();
    let metadata = match dir.symlink_metadata(name) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };

    if metadata.is_dir() {
        let folder = dir.open_dir(name)?;
        for entry in folder.entries()? {
            remove_tree(&folder, entry?.file_name())?;
        }
        ignore_missing(dir.remove_dir(name))
    } else {
        ignore_missing(dir.remove_file(name))
    }
}

fn ignore_missing(result: std::io::Result<()>) -> Result<(), FsError> {
    match result {
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        other => Ok(other?),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cap_std::ambient_authority;

    fn open(path: &std::path::Path) -> Dir {
        Dir::open_ambient_dir(path, ambient_authority()).unwrap()
    }

    #[test]
    fn copies_a_folder_tree_with_progress() {
        let from = tempfile::tempdir().unwrap();
        let to = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(from.path().join("Album/Day 1")).unwrap();
        std::fs::write(from.path().join("Album/a.jpg"), vec![1u8; 3000]).unwrap();
        std::fs::write(from.path().join("Album/Day 1/b.jpg"), vec![2u8; 2000]).unwrap();

        let (from_dir, to_dir) = (open(from.path()), open(to.path()));
        assert_eq!(measure(&from_dir, "Album").unwrap(), 5000);

        let progress = CopyProgress::default();
        copy_item(
            &from_dir,
            "Album",
            &to_dir,
            "Copy",
            &progress,
            &StopSignal::default(),
        )
        .unwrap();

        assert_eq!(progress.bytes_copied.load(Ordering::Relaxed), 5000);
        assert_eq!(
            std::fs::read(to.path().join("Copy/Day 1/b.jpg")).unwrap(),
            vec![2u8; 2000]
        );
        assert!(
            from.path().join("Album/a.jpg").exists(),
            "copying never touches the original"
        );
    }

    #[test]
    fn symlinks_are_refused() {
        let from = tempfile::tempdir().unwrap();
        std::fs::create_dir(from.path().join("Folder")).unwrap();
        std::os::unix::fs::symlink("/etc", from.path().join("Folder/link")).unwrap();

        let result = measure(&open(from.path()), "Folder");
        assert!(matches!(result, Err(FsError::Unsupported(_))));
    }

    #[test]
    fn apple_double_companions_are_skipped() {
        let from = tempfile::tempdir().unwrap();
        let to = tempfile::tempdir().unwrap();
        std::fs::create_dir(from.path().join("F")).unwrap();
        std::fs::write(from.path().join("F/a.txt"), b"a").unwrap();
        std::fs::write(from.path().join("F/._a.txt"), b"metadata").unwrap();
        std::fs::write(from.path().join("F/._alone"), b"no partner: a real file").unwrap();

        let (from_dir, to_dir) = (open(from.path()), open(to.path()));
        let progress = CopyProgress::default();
        copy_item(
            &from_dir,
            "F",
            &to_dir,
            "F",
            &progress,
            &StopSignal::default(),
        )
        .unwrap();

        assert!(to.path().join("F/a.txt").exists());
        assert!(!to.path().join("F/._a.txt").exists());
        assert!(to.path().join("F/._alone").exists());
    }

    #[test]
    fn remove_tree_removes_everything_but_follows_no_links() {
        let temp = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("keep.txt"), b"keep").unwrap();
        std::fs::create_dir_all(temp.path().join("T/sub")).unwrap();
        std::fs::write(temp.path().join("T/sub/x"), b"x").unwrap();
        std::os::unix::fs::symlink(outside.path(), temp.path().join("T/link")).unwrap();

        remove_tree(&open(temp.path()), "T").unwrap();
        assert!(!temp.path().join("T").exists());
        assert!(outside.path().join("keep.txt").exists());
        remove_tree(&open(temp.path()), "T").unwrap(); // Already gone is fine.
    }

    #[test]
    fn stop_signal_stops_the_copy() {
        let from = tempfile::tempdir().unwrap();
        let to = tempfile::tempdir().unwrap();
        std::fs::write(from.path().join("a.bin"), b"data").unwrap();

        let stop = StopSignal::default();
        stop.stop();
        let result = copy_item(
            &open(from.path()),
            "a.bin",
            &open(to.path()),
            "a.bin",
            &CopyProgress::default(),
            &stop,
        );
        assert!(matches!(result, Err(FsError::Abandoned)));
    }
}
