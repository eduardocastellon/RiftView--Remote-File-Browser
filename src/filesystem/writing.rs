//! Writing uploaded files into shares.
//!
//! An upload is written to a hidden partial file in its target folder,
//! e.g. `Photos/.riftview-upload-upl_3f2a….part`. Keeping it in the same
//! folder (so on the same disk) means finishing is an instant rename, even
//! on external drives. Partial files are never shown to devices and can't
//! be opened through the API.
//!
//! Like the rest of this module, every path goes through a `cap-std`
//! handle, so nothing here can write outside a share.

use std::io::{ErrorKind, Seek, SeekFrom, Write};

use axum::body::Bytes;
use cap_std::fs::{Dir, OpenOptions};

use super::metadata::Entry;
use super::roots::Share;
use super::{FsError, RelPath, run_blocking};

const UPLOAD_FILE_PREFIX: &str = ".riftview-upload-";
const UPLOAD_FILE_SUFFIX: &str = ".part";

/// How many "name (1).ext", "name (2).ext"… we try before giving up.
const MAX_RENAME_ATTEMPTS: u32 = 999;

/// The hidden partial file's name for an upload.
pub fn upload_file_name(upload_id: &str) -> String {
    format!("{UPLOAD_FILE_PREFIX}{upload_id}{UPLOAD_FILE_SUFFIX}")
}

/// Is this the name of a partial upload file? Such files are hidden from
/// listings and can't be downloaded, uploaded over or named by devices.
pub fn is_upload_file_name(name: &str) -> bool {
    name.starts_with(UPLOAD_FILE_PREFIX) && name.ends_with(UPLOAD_FILE_SUFFIX)
}

/// Check a file name a device wants to upload as.
pub fn validate_upload_name(name: &str) -> Result<(), FsError> {
    RelPath::root().join_name(name)?;
    if is_upload_file_name(name) {
        return Err(FsError::InvalidPath("that file name is reserved"));
    }
    Ok(())
}

/// Bytes free for us on the disk holding `folder`, or `None` if the OS
/// can't tell us (then the caller skips the check).
///
/// Also confirms that `folder` exists and is a folder.
pub async fn free_space(share: Share, folder: RelPath) -> Result<Option<u64>, FsError> {
    run_blocking(move || {
        let dir = share.open_dir(&folder)?;
        Ok(available_bytes(&dir).ok())
    })
    .await
}

#[cfg(unix)]
fn available_bytes(dir: &Dir) -> std::io::Result<u64> {
    use std::os::fd::AsRawFd;

    // SAFETY: `statvfs` is plain data, so all-zeroes is a valid value, and
    // `fstatvfs` only writes into it. The descriptor stays open for the
    // whole call because `dir` is borrowed.
    let mut stats: libc::statvfs = unsafe { std::mem::zeroed() };
    let result = unsafe { libc::fstatvfs(dir.as_raw_fd(), &mut stats) };
    if result != 0 {
        return Err(std::io::Error::last_os_error());
    }

    // `f_bavail` counts blocks available to unprivileged users, which is
    // what we are.
    Ok(stats.f_bavail as u64 * stats.f_frsize as u64)
}

#[cfg(not(unix))]
fn available_bytes(_dir: &Dir) -> std::io::Result<u64> {
    Err(std::io::Error::other(
        "free space check not implemented on this OS",
    ))
}

/// Create the empty partial file for a new upload.
///
/// It is created at its final size straight away so chunks can be written
/// at their offsets in any order. On most disks this doesn't use real space
/// until data is written (a "sparse" file).
pub async fn create_upload_file(
    share: Share,
    folder: RelPath,
    file_name: String,
    size: u64,
) -> Result<(), FsError> {
    run_blocking(move || {
        let dir = share.open_dir(&folder)?;
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);

        let file = dir.open_with(&file_name, &options)?;
        file.set_len(size)?;
        Ok(())
    })
    .await
}

/// Write one verified chunk into the partial file at `offset`.
pub async fn write_chunk(
    share: Share,
    folder: RelPath,
    file_name: String,
    offset: u64,
    bytes: Bytes,
) -> Result<(), FsError> {
    run_blocking(move || {
        let dir = share.open_dir(&folder)?;
        let mut options = OpenOptions::new();
        options.write(true); // Never create: the upload must already exist.

        let mut file = dir.open_with(&file_name, &options)?.into_std();
        file.seek(SeekFrom::Start(offset))?;
        file.write_all(&bytes)?;
        Ok(())
    })
    .await
}

/// Open the partial file for reading (to hash it before finishing).
pub async fn open_upload_file(
    share: Share,
    folder: RelPath,
    file_name: String,
) -> Result<std::fs::File, FsError> {
    run_blocking(move || {
        let dir = share.open_dir(&folder)?;
        Ok(dir.open(&file_name)?.into_std())
    })
    .await
}

/// Delete a partial file. Already gone is fine.
pub async fn remove_upload_file(
    share: Share,
    folder: RelPath,
    file_name: String,
) -> Result<(), FsError> {
    run_blocking(move || {
        let dir = share.open_dir(&folder)?;
        match dir.remove_file(&file_name) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    })
    .await
}

/// Turn a complete partial file into the real file.
///
/// The data is flushed to disk first. Then the file gets `wanted_name`, or
/// if that's taken, `name (1).ext`, `name (2).ext`… An existing file is
/// never replaced. Returns the new file's details.
pub async fn finish_upload(
    share: Share,
    folder: RelPath,
    upload_file_name: String,
    wanted_name: String,
) -> Result<Entry, FsError> {
    run_blocking(move || {
        let dir = share.open_dir(&folder)?;
        dir.open(&upload_file_name)?.sync_all()?;

        let final_name = move_to_free_name(&dir, &upload_file_name, &wanted_name)?;

        let final_path = folder.join_name(&final_name)?;
        let metadata = dir.metadata(&final_name)?;
        Entry::from_cap_metadata(&share.id, &final_path, &metadata).ok_or(FsError::NotAFile)
    })
    .await
}

/// Give `from` the first free name out of `wanted`, `wanted (1)`, … and
/// return the name used.
fn move_to_free_name(dir: &Dir, from: &str, wanted: &str) -> Result<String, FsError> {
    for candidate in candidate_names(wanted) {
        // Skip names that are too long or otherwise invalid.
        if RelPath::root().join_name(&candidate).is_err() {
            continue;
        }

        // A hard link fails if the name already exists, so it can never
        // replace someone else's file, even one that appeared a moment ago.
        match dir.hard_link(from, dir, &candidate) {
            Ok(()) => {
                dir.remove_file(from)?;
                return Ok(candidate);
            }
            Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
            Err(_) => {
                // Some disks (e.g. exFAT USB drives) don't support hard
                // links. Fall back to "check, then rename", which has a
                // tiny window in which a file of the same name could
                // appear and be replaced.
                if dir.symlink_metadata(&candidate).is_ok() {
                    continue;
                }
                dir.rename(from, dir, &candidate)?;
                return Ok(candidate);
            }
        }
    }

    Err(FsError::NoFreeName)
}

/// `report.pdf`, then `report (1).pdf`, `report (2).pdf`, …
fn candidate_names(wanted: &str) -> impl Iterator<Item = String> + '_ {
    // A leading dot (".bashrc") is part of the name, not an extension.
    let (stem, extension) = match wanted.rfind('.') {
        Some(dot) if dot > 0 => (&wanted[..dot], &wanted[dot..]),
        _ => (wanted, ""),
    };

    let numbered = (1..=MAX_RENAME_ATTEMPTS).map(move |n| format!("{stem} ({n}){extension}"));
    std::iter::once(wanted.to_owned()).chain(numbered)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbered_names_keep_the_extension() {
        let names: Vec<String> = candidate_names("report.pdf").take(3).collect();
        assert_eq!(names, ["report.pdf", "report (1).pdf", "report (2).pdf"]);

        let names: Vec<String> = candidate_names(".bashrc").take(2).collect();
        assert_eq!(names, [".bashrc", ".bashrc (1)"]);

        let names: Vec<String> = candidate_names("README").take(2).collect();
        assert_eq!(names, ["README", "README (1)"]);
    }

    #[test]
    fn upload_file_names_are_reserved() {
        let name = upload_file_name("upl_123");
        assert_eq!(name, ".riftview-upload-upl_123.part");
        assert!(is_upload_file_name(&name));
        assert!(validate_upload_name(&name).is_err());

        assert!(validate_upload_name("photo.jpg").is_ok());
        assert!(validate_upload_name("../photo.jpg").is_err());
        assert!(validate_upload_name("a/b.jpg").is_err());
        assert!(validate_upload_name("").is_err());
    }
}
