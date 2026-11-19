//! A file's "version": a fingerprint of its metadata that changes whenever
//! its contents may have changed.
//!
//! Reading a file's contents to detect changes would be far too slow for
//! big files, so we use what the operating system already tracks: size,
//! last-modified time and (on macOS/Linux) the inode and its change time.
//! Any write to the file updates at least one of these.

use std::fs::{File, Metadata};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileVersion {
    pub size: u64,
    modified_ns: i128,
    /// Which disk and which file on it. Changes if the file is replaced
    /// (e.g. an editor saving by writing a new file and renaming it).
    device: u64,
    inode: u64,
    /// When the file's contents *or* metadata last changed. Unlike the
    /// modified time, programs can't set this back to an old value.
    changed_ns: i128,
}

impl FileVersion {
    /// The version of an open file, as it is right now.
    pub fn of(file: &File) -> std::io::Result<Self> {
        Ok(Self::from_metadata(&file.metadata()?))
    }

    #[cfg(unix)]
    pub fn from_metadata(metadata: &Metadata) -> Self {
        use std::os::unix::fs::MetadataExt;

        Self {
            size: metadata.size(),
            modified_ns: nanoseconds(metadata.mtime(), metadata.mtime_nsec()),
            device: metadata.dev(),
            inode: metadata.ino(),
            changed_ns: nanoseconds(metadata.ctime(), metadata.ctime_nsec()),
        }
    }

    /// Windows has no inode or change time, so size + modified time it is.
    #[cfg(not(unix))]
    pub fn from_metadata(metadata: &Metadata) -> Self {
        let modified_ns = metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|elapsed| elapsed.as_nanos() as i128)
            .unwrap_or(0);

        Self {
            size: metadata.len(),
            modified_ns,
            device: 0,
            inode: 0,
            changed_ns: 0,
        }
    }

    /// The `ETag` header value, e.g. `"q3Jf0bWm9yXc1T2kZ8aP4w"` (quotes
    /// included, as HTTP requires).
    ///
    /// It's a hash of the fields above, so it reveals nothing about the
    /// host (such as inode numbers) and clients can treat it as opaque.
    pub fn etag(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(self.size.to_le_bytes());
        hasher.update(self.modified_ns.to_le_bytes());
        hasher.update(self.device.to_le_bytes());
        hasher.update(self.inode.to_le_bytes());
        hasher.update(self.changed_ns.to_le_bytes());
        let digest = hasher.finalize();

        format!("\"{}\"", URL_SAFE_NO_PAD.encode(&digest[..16]))
    }
}

#[cfg(unix)]
fn nanoseconds(seconds: i64, nanoseconds: i64) -> i128 {
    seconds as i128 * 1_000_000_000 + nanoseconds as i128
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn etag_is_stable_until_the_file_changes() {
        let mut file = tempfile::tempfile().unwrap();
        file.write_all(b"first").unwrap();

        let before = FileVersion::of(&file).unwrap();
        assert_eq!(before, FileVersion::of(&file).unwrap());
        assert_eq!(before.etag(), FileVersion::of(&file).unwrap().etag());
        assert!(before.etag().starts_with('"') && before.etag().ends_with('"'));

        file.write_all(b" and more").unwrap();
        let after = FileVersion::of(&file).unwrap();
        assert_ne!(before, after);
        assert_ne!(before.etag(), after.etag());
    }
}
