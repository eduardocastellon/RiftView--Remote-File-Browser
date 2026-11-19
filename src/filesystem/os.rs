//! Small operating-system-specific helpers that `cap-std` doesn't offer.
//!
//! Every function here takes `cap-std` folder handles plus **single names**
//! (already checked: no `/`, no `..`). Paths are always looked up relative
//! to those handles, so these helpers can't reach outside a share either.

use std::io::{self, ErrorKind};
use std::path::PathBuf;

use cap_std::fs::Dir;

/// Rename `from_dir/from_name` to `to_dir/to_name`, failing with
/// `AlreadyExists` instead of replacing something that's already there.
///
/// Works for files and folders. Fails with `CrossesDevices` (EXDEV) if the
/// two folders are on different disks; the caller then has to copy.
pub fn rename_no_replace(
    from_dir: &Dir,
    from_name: &str,
    to_dir: &Dir,
    to_name: &str,
) -> io::Result<()> {
    match native_rename_no_replace(from_dir, from_name, to_dir, to_name) {
        Err(error) if is_unsupported(&error) => {
            // The disk (e.g. exFAT, some network drives) can't do an atomic
            // "rename unless it exists". Fall back to "check, then rename",
            // which has a tiny window in which a same-named item could
            // appear and be replaced.
            if to_dir.symlink_metadata(to_name).is_ok() {
                return Err(io::Error::from(ErrorKind::AlreadyExists));
            }
            from_dir.rename(from_name, to_dir, to_name)
        }
        other => other,
    }
}

fn is_unsupported(error: &io::Error) -> bool {
    #[cfg(unix)]
    if matches!(
        error.raw_os_error(),
        Some(libc::ENOTSUP | libc::EINVAL | libc::ENOSYS)
    ) {
        return true;
    }
    error.kind() == ErrorKind::Unsupported
}

/// Is this the "different disks" error from a rename?
pub fn is_cross_device(error: &io::Error) -> bool {
    #[cfg(unix)]
    if error.raw_os_error() == Some(libc::EXDEV) {
        return true;
    }
    error.kind() == ErrorKind::CrossesDevices
}

#[cfg(target_os = "macos")]
fn native_rename_no_replace(from_dir: &Dir, from: &str, to_dir: &Dir, to: &str) -> io::Result<()> {
    use std::os::fd::AsRawFd;

    let (from, to) = (c_name(from)?, c_name(to)?);
    // SAFETY: both descriptors are open for the duration of the call (the
    // `Dir`s are borrowed) and both names are valid NUL-terminated strings.
    let result = unsafe {
        libc::renameatx_np(
            from_dir.as_raw_fd(),
            from.as_ptr(),
            to_dir.as_raw_fd(),
            to.as_ptr(),
            libc::RENAME_EXCL,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(all(target_os = "linux", target_env = "gnu"))]
fn native_rename_no_replace(from_dir: &Dir, from: &str, to_dir: &Dir, to: &str) -> io::Result<()> {
    use std::os::fd::AsRawFd;

    let (from, to) = (c_name(from)?, c_name(to)?);
    // SAFETY: as above.
    let result = unsafe {
        libc::renameat2(
            from_dir.as_raw_fd(),
            from.as_ptr(),
            to_dir.as_raw_fd(),
            to.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// Other systems (Windows comes in Phase 8): use the check-then-rename
/// fallback.
#[cfg(not(any(target_os = "macos", all(target_os = "linux", target_env = "gnu"))))]
fn native_rename_no_replace(_: &Dir, _: &str, _: &Dir, _: &str) -> io::Result<()> {
    Err(io::Error::from(ErrorKind::Unsupported))
}

#[cfg(unix)]
fn c_name(name: &str) -> io::Result<std::ffi::CString> {
    std::ffi::CString::new(name).map_err(|_| io::Error::from(ErrorKind::InvalidInput))
}

/// Bytes free for us on the disk holding `dir`.
#[cfg(unix)]
pub fn available_bytes(dir: &Dir) -> io::Result<u64> {
    use std::os::fd::AsRawFd;

    // SAFETY: `statvfs` is plain data, so all-zeroes is a valid value, and
    // `fstatvfs` only writes into it. The descriptor stays open for the
    // whole call because `dir` is borrowed.
    let mut stats: libc::statvfs = unsafe { std::mem::zeroed() };
    let result = unsafe { libc::fstatvfs(dir.as_raw_fd(), &mut stats) };
    if result != 0 {
        return Err(io::Error::last_os_error());
    }

    // `f_bavail` counts blocks available to unprivileged users, which is
    // what we are.
    Ok(stats.f_bavail as u64 * stats.f_frsize as u64)
}

#[cfg(not(unix))]
pub fn available_bytes(_dir: &Dir) -> io::Result<u64> {
    Err(io::Error::from(ErrorKind::Unsupported))
}

/// The real, absolute path of an open folder, as the OS sees it right now.
///
/// Used only where an absolute path is unavoidable (moving an item to the
/// system Trash). Because it comes from the open handle, it reflects where
/// the folder actually is, symlinks resolved.
#[cfg(target_os = "macos")]
pub fn real_path_of(dir: &Dir) -> io::Result<PathBuf> {
    use std::ffi::{CStr, OsStr};
    use std::os::fd::AsRawFd;
    use std::os::unix::ffi::OsStrExt;

    let mut buffer = vec![0u8; libc::PATH_MAX as usize];
    // SAFETY: F_GETPATH writes a NUL-terminated path of at most PATH_MAX
    // bytes into the buffer, which is exactly that big.
    let result = unsafe { libc::fcntl(dir.as_raw_fd(), libc::F_GETPATH, buffer.as_mut_ptr()) };
    if result == -1 {
        return Err(io::Error::last_os_error());
    }
    let path =
        CStr::from_bytes_until_nul(&buffer).map_err(|_| io::Error::from(ErrorKind::InvalidData))?;
    Ok(PathBuf::from(OsStr::from_bytes(path.to_bytes())))
}

#[cfg(target_os = "linux")]
pub fn real_path_of(dir: &Dir) -> io::Result<PathBuf> {
    use std::os::fd::AsRawFd;
    std::fs::read_link(format!("/proc/self/fd/{}", dir.as_raw_fd()))
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub fn real_path_of(_dir: &Dir) -> io::Result<PathBuf> {
    Err(io::Error::from(ErrorKind::Unsupported))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cap_std::ambient_authority;

    fn open(path: &std::path::Path) -> Dir {
        Dir::open_ambient_dir(path, ambient_authority()).unwrap()
    }

    #[test]
    fn rename_never_replaces() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("a"), b"a").unwrap();
        std::fs::write(temp.path().join("b"), b"b").unwrap();
        let dir = open(temp.path());

        let error = rename_no_replace(&dir, "a", &dir, "b").unwrap_err();
        assert_eq!(error.kind(), ErrorKind::AlreadyExists);
        assert_eq!(std::fs::read(temp.path().join("b")).unwrap(), b"b");

        rename_no_replace(&dir, "a", &dir, "c").unwrap();
        assert_eq!(std::fs::read(temp.path().join("c")).unwrap(), b"a");
    }

    #[test]
    fn real_path_matches_the_folder() {
        let temp = tempfile::tempdir().unwrap();
        let dir = open(temp.path());
        assert_eq!(
            real_path_of(&dir).unwrap(),
            temp.path().canonicalize().unwrap()
        );
    }
}
