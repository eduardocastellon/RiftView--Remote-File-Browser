//! Filesystem access confined to explicitly shared roots.
//!
//! This module is independent of HTTP. Callers never build host paths:
//! they pass a [`roots::Share`] plus a validated [`RelPath`], and every
//! filesystem call goes through a `cap_std::fs::Dir` handle opened on the
//! share root. `cap-std` resolves paths component by component relative to
//! that handle (using `openat2(RESOLVE_BENEATH)` on Linux and equivalent
//! handle-based logic elsewhere), so `..`, absolute paths and symlinks that
//! point outside the root are rejected by the resolver itself — not by a
//! string-prefix check that could be raced.

pub mod browser;
pub mod ids;
pub mod metadata;
pub mod operations;
pub mod resolver;
pub mod roots;

pub use ids::FileId;
pub use resolver::RelPath;

#[derive(Debug, thiserror::Error)]
pub enum FsError {
    #[error("not found")]
    NotFound,
    #[error("not a directory")]
    NotADirectory,
    #[error("not a file")]
    NotAFile,
    #[error("invalid path: {0}")]
    InvalidPath(&'static str),
    /// Includes attempts to escape the share root.
    #[error("access denied")]
    AccessDenied,
    #[error("directory has too many entries ({0} max)")]
    TooManyEntries(usize),
    #[error("share root is unavailable")]
    RootUnavailable,
    #[error("I/O error: {0}")]
    Io(std::io::Error),
}

impl From<std::io::Error> for FsError {
    fn from(e: std::io::Error) -> Self {
        use std::io::ErrorKind::*;
        match e.kind() {
            NotFound => FsError::NotFound,
            PermissionDenied => FsError::AccessDenied,
            NotADirectory => FsError::NotADirectory,
            IsADirectory => FsError::NotAFile,
            _ => FsError::Io(e),
        }
    }
}

/// Run blocking filesystem work off the async runtime.
pub(crate) async fn blocking<T, F>(f: F) -> Result<T, FsError>
where
    F: FnOnce() -> Result<T, FsError> + Send + 'static,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| FsError::Io(std::io::Error::other(e)))?
}
