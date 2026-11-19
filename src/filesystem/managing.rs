//! Changing what's in a share: rename, move, new folder, delete.
//!
//! - **Rename and move never replace anything.** If the new name is taken,
//!   they fail with [`FsError::AlreadyExists`].
//! - **Delete always moves to the Trash of the item's disk**, so the host
//!   user can put things back. Only on a disk that has no Trash (some
//!   network drives, for example) is it deleted permanently. Any other
//!   Trash problem means nothing is deleted.
//! - **Moves between disks** can't be a rename; [`move_item`] reports
//!   [`MoveOutcome::NeedsCopy`] and the caller copies (see [`super::copying`]).

use std::io::ErrorKind;
use std::path::Path;

use cap_std::fs::{Dir, Metadata};

use super::copying::remove_tree;
use super::metadata::Entry;
use super::os::{is_cross_device, real_path_of, rename_no_replace};
use super::roots::Share;
use super::trash::{TrashBin, TrashError, move_to_trash};
use super::{FsError, RelPath, run_blocking, validate_new_name};

/// How an item was deleted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeleteMethod {
    /// Moved to the system Trash; the host user can restore it.
    Trash,
    /// Gone for good (this disk has no Trash).
    Permanent,
}

pub enum MoveOutcome {
    /// Done: the item is at its new place.
    Moved(Entry),
    /// The destination is on another disk, so the item has to be copied
    /// and then deleted. Nothing has changed yet.
    NeedsCopy,
}

/// Rename a file or folder within its folder.
pub async fn rename(share: Share, path: RelPath, new_name: String) -> Result<Entry, FsError> {
    validate_new_name(&new_name)?;
    let (parent, old_name) = split_item_path(&path)?;

    run_blocking(move || {
        let dir = share.open_dir(&parent)?;
        let old_metadata = dir.symlink_metadata(&old_name)?;

        if new_name != old_name {
            match dir.symlink_metadata(&new_name) {
                // On case-insensitive disks (the macOS default), "Photo.jpg"
                // → "photo.jpg" finds the *same* file under the new name.
                // That's a case change, not a clash.
                Ok(existing) if is_same_item(&existing, &old_metadata) => {
                    dir.rename(&old_name, &dir, &new_name)?;
                }
                _ => rename_no_replace(&dir, &old_name, &dir, &new_name).map_err(clash_or_error)?,
            }
        }

        entry_for(&share, &dir, &parent, &new_name)
    })
    .await
}

/// Move a file or folder into `destination_folder`, keeping its name. The
/// destination may be in another share.
///
/// Returns [`MoveOutcome::NeedsCopy`] without changing anything if the
/// destination is on another disk.
pub async fn move_item(
    source_share: Share,
    source_path: RelPath,
    destination_share: Share,
    destination_folder: RelPath,
) -> Result<MoveOutcome, FsError> {
    let (source_parent, name) = split_item_path(&source_path)?;
    let same_share = source_share.id == destination_share.id;

    if same_share && destination_folder.is_inside_or_equal(&source_path) {
        return Err(FsError::InvalidPath("can't move a folder into itself"));
    }

    run_blocking(move || {
        let source_dir = source_share.open_dir(&source_parent)?;
        source_dir.symlink_metadata(&name)?; // 404 if it doesn't exist.
        let destination_dir = destination_share.open_dir(&destination_folder)?;

        // Moving to where it already is: nothing to do.
        if same_share && destination_folder == source_parent {
            let entry = entry_for(&source_share, &source_dir, &source_parent, &name)?;
            return Ok(MoveOutcome::Moved(entry));
        }

        match rename_no_replace(&source_dir, &name, &destination_dir, &name) {
            Ok(()) => {
                let entry = entry_for(
                    &destination_share,
                    &destination_dir,
                    &destination_folder,
                    &name,
                )?;
                Ok(MoveOutcome::Moved(entry))
            }
            Err(error) if is_cross_device(&error) => {
                // Check the name now, so the phone gets a clear 409 instead
                // of a background copy that fails at the end.
                if destination_dir.symlink_metadata(&name).is_ok() {
                    return Err(FsError::AlreadyExists);
                }
                Ok(MoveOutcome::NeedsCopy)
            }
            Err(error) => Err(clash_or_error(error)),
        }
    })
    .await
}

/// Create a new, empty folder.
pub async fn create_folder(share: Share, parent: RelPath, name: String) -> Result<Entry, FsError> {
    validate_new_name(&name)?;

    run_blocking(move || {
        let dir = share.open_dir(&parent)?;
        dir.create_dir(&name).map_err(clash_or_error)?;
        entry_for(&share, &dir, &parent, &name)
    })
    .await
}

/// Delete a file or a folder with everything in it.
///
/// It goes to its disk's Trash. Only if that disk has no Trash is it
/// deleted permanently. If moving to the Trash fails for any other reason,
/// nothing is deleted ([`FsError::TrashFailed`]).
pub async fn delete(share: Share, path: RelPath, bin: TrashBin) -> Result<DeleteMethod, FsError> {
    let (parent, name) = split_item_path(&path)?;

    run_blocking(move || {
        let dir = share.open_dir(&parent)?;
        dir.symlink_metadata(&name)?; // 404 if it doesn't exist.

        match trash_item(&share, &dir, &name, &bin) {
            Ok(()) => Ok(DeleteMethod::Trash),
            Err(TrashError::NoTrashOnThisDisk) => {
                // Through the confined handle, never following symlinks:
                // deleting a symlink removes the link, not its target.
                remove_tree(&dir, &name)?;
                Ok(DeleteMethod::Permanent)
            }
            Err(TrashError::Failed(reason)) => {
                tracing::warn!(%reason, "couldn't move an item to the Trash; nothing deleted");
                Err(FsError::TrashFailed)
            }
        }
    })
    .await
}

/// Move `dir/name` to the Trash.
///
/// The Trash API needs an absolute path. We take it from the open folder
/// handle (so it's where the folder really is, symlinks resolved) and check
/// it's still inside the share before handing it over. The last name is
/// passed as-is, so a symlink is trashed as a link, not its target.
fn trash_item(share: &Share, dir: &Dir, name: &str, bin: &TrashBin) -> Result<(), TrashError> {
    let folder = real_path_of(dir).map_err(|error| TrashError::Failed(error.to_string()))?;
    if !is_inside(&folder, &share.path) {
        return Err(TrashError::Failed(
            "folder is no longer inside the share".into(),
        ));
    }
    move_to_trash(bin, &folder.join(name))
}

fn is_inside(path: &Path, root: &Path) -> bool {
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    path.starts_with(root)
}

/// Split `/a/b/c` into its folder `/a/b` and name `c`. The share's root
/// folder itself can't be renamed, moved or deleted.
fn split_item_path(path: &RelPath) -> Result<(RelPath, String), FsError> {
    match (path.parent(), path.file_name()) {
        (Some(parent), Some(name)) => Ok((parent, name.to_owned())),
        _ => Err(FsError::InvalidPath(
            "the share's own folder can't be changed",
        )),
    }
}

/// Same file or folder on disk (same disk and inode), whatever its name.
#[cfg(unix)]
fn is_same_item(a: &Metadata, b: &Metadata) -> bool {
    use cap_std::fs::MetadataExt;
    a.dev() == b.dev() && a.ino() == b.ino()
}

#[cfg(not(unix))]
fn is_same_item(_a: &Metadata, _b: &Metadata) -> bool {
    false
}

/// Turn "already exists" into our own error, so it becomes a 409.
fn clash_or_error(error: std::io::Error) -> FsError {
    if error.kind() == ErrorKind::AlreadyExists {
        FsError::AlreadyExists
    } else {
        error.into()
    }
}

fn entry_for(share: &Share, dir: &Dir, folder: &RelPath, name: &str) -> Result<Entry, FsError> {
    let metadata = dir.symlink_metadata(name)?;
    let metadata = if metadata.is_symlink() {
        dir.metadata(name)? // Describe what the link points to, as listings do.
    } else {
        metadata
    };
    Entry::from_cap_metadata(&share.id, &folder.join_name(name)?, &metadata)
        .ok_or(FsError::NotFound)
}
