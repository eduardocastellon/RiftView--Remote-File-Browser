//! Moving deleted items to the Trash of the disk they're on.
//!
//! The rule for deletes: **an item always goes to its disk's Trash.** It
//! is deleted permanently only when that disk has no Trash at all. Any
//! other problem with the Trash means nothing is deleted. So we must tell
//! "this disk has no Trash" apart from every other failure, which is what
//! [`TrashError`] does.

use std::path::{Path, PathBuf};

/// Where deleted items go.
#[derive(Debug, Clone, Default)]
pub enum TrashBin {
    /// The system Trash of the item's disk (on macOS: `~/.Trash` for the
    /// startup disk, `.Trashes` on other disks).
    #[default]
    System,

    /// **Tests only:** a plain folder standing in for the Trash, so tests
    /// don't fill the real one. Items are moved into it.
    Folder(PathBuf),

    /// **Tests only:** behave like a disk that has no Trash.
    NoTrash,
}

#[derive(Debug)]
pub enum TrashError {
    /// This disk has no Trash. The only case in which a delete may
    /// become permanent.
    NoTrashOnThisDisk,

    /// Anything else. Nothing was deleted.
    Failed(String),
}

/// Move the file or folder at `item` (an absolute path) to the Trash.
/// The last part of the path is not followed if it's a symlink: the link
/// itself goes to the Trash, not what it points to.
pub fn move_to_trash(bin: &TrashBin, item: &Path) -> Result<(), TrashError> {
    match bin {
        TrashBin::System => system::move_to_trash(item),
        TrashBin::Folder(folder) => move_into_folder(folder, item),
        TrashBin::NoTrash => Err(TrashError::NoTrashOnThisDisk),
    }
}

fn move_into_folder(folder: &Path, item: &Path) -> Result<(), TrashError> {
    let name = item
        .file_name()
        .ok_or_else(|| TrashError::Failed("nothing to trash".into()))?;
    // Prefix with a unique ID so items with the same name don't collide,
    // much like the real Trash renames duplicates.
    let unique_name = format!(
        "{}-{}",
        uuid::Uuid::new_v4().simple(),
        name.to_string_lossy()
    );

    std::fs::rename(item, folder.join(unique_name))
        .map_err(|error| TrashError::Failed(error.to_string()))
}

#[cfg(target_os = "macos")]
mod system {
    use std::path::Path;

    use objc2_foundation::{NSFeatureUnsupportedError, NSFileManager, NSString, NSURL};

    use super::TrashError;

    /// `NSFileManager.trashItemAtURL`, the same call Finder's "Move to
    /// Trash" uses (minus the sound). macOS answers "feature unsupported"
    /// when the item's disk can't have a Trash (some network drives, for
    /// example).
    pub fn move_to_trash(item: &Path) -> Result<(), TrashError> {
        let path = item
            .to_str()
            .ok_or_else(|| TrashError::Failed("path isn't valid text".into()))?;
        let url = NSURL::fileURLWithPath(&NSString::from_str(path));

        NSFileManager::defaultManager()
            .trashItemAtURL_resultingItemURL_error(&url, None)
            .map_err(|error| {
                let is_cocoa_error = error.domain().to_string() == "NSCocoaErrorDomain";
                if is_cocoa_error && error.code() == NSFeatureUnsupportedError {
                    TrashError::NoTrashOnThisDisk
                } else {
                    TrashError::Failed(format!(
                        "{} (code {})",
                        error.localizedDescription(),
                        error.code()
                    ))
                }
            })
    }
}

/// Linux and Windows (Phase 8): the `trash` crate. Its errors don't tell
/// "no Trash on this disk" apart yet, so every failure is treated as
/// "nothing deleted", which is the safe side of the rule.
#[cfg(not(target_os = "macos"))]
mod system {
    use std::path::Path;

    use super::TrashError;

    pub fn move_to_trash(item: &Path) -> Result<(), TrashError> {
        trash::delete(item).map_err(|error| TrashError::Failed(error.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_bin_receives_items() {
        let share = tempfile::tempdir().unwrap();
        let bin = tempfile::tempdir().unwrap();
        let item = share.path().join("a.txt");
        std::fs::write(&item, b"a").unwrap();

        move_to_trash(&TrashBin::Folder(bin.path().into()), &item).unwrap();
        assert!(!item.exists());
        assert_eq!(std::fs::read_dir(bin.path()).unwrap().count(), 1);
    }

    #[test]
    fn no_trash_bin_says_so() {
        let result = move_to_trash(&TrashBin::NoTrash, Path::new("/whatever"));
        assert!(matches!(result, Err(TrashError::NoTrashOnThisDisk)));
    }
}
