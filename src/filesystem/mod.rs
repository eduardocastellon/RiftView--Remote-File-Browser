//! Reading files and folders inside shares, without ever escaping them.
//!
//! This module knows nothing about HTTP.
//!
//! ## How we stay inside a share
//!
//! Callers never build real host paths. They pass a [`roots::Share`] and a
//! checked [`RelPath`] ("a path relative to the share"). Every filesystem
//! call then goes through a `cap_std::fs::Dir`, which is an open handle on
//! the share's root folder.
//!
//! `cap-std` asks the operating system to look up each path *relative to
//! that handle* and to refuse anything that would leave it. So `..`,
//! absolute paths, and symlinks pointing outside the share are blocked by
//! the OS itself. That is much stronger than checking whether a path string
//! starts with the share's path, which can be tricked or raced.

pub mod browser;
pub mod ids;
pub mod metadata;
pub mod operations;
pub mod resolver;
pub mod roots;

pub use ids::FileId;
pub use resolver::RelPath;

use std::io::ErrorKind;

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

    /// Also returned when a path tries to escape the share.
    #[error("access denied")]
    AccessDenied,

    #[error("directory has too many entries ({0} max)")]
    TooManyEntries(usize),

    /// The share's folder was deleted, moved, or is on a disconnected drive.
    #[error("share root is unavailable")]
    RootUnavailable,

    #[error("I/O error: {0}")]
    Io(std::io::Error),
}

impl From<std::io::Error> for FsError {
    fn from(error: std::io::Error) -> Self {
        match error.kind() {
            ErrorKind::NotFound => FsError::NotFound,
            ErrorKind::PermissionDenied => FsError::AccessDenied,
            ErrorKind::NotADirectory => FsError::NotADirectory,
            ErrorKind::IsADirectory => FsError::NotAFile,
            _ => FsError::Io(error),
        }
    }
}

/// Run slow, blocking filesystem work on a background thread so it doesn't
/// stall the async server.
pub(crate) async fn run_blocking<T, F>(work: F) -> Result<T, FsError>
where
    F: FnOnce() -> Result<T, FsError> + Send + 'static,
    T: Send + 'static,
{
    match tokio::task::spawn_blocking(work).await {
        Ok(result) => result,
        Err(join_error) => Err(FsError::Io(std::io::Error::other(join_error))),
    }
}
