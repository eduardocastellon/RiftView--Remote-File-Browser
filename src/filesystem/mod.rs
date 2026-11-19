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
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

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

    /// Nobody is waiting for the result any more (the device disconnected
    /// or the request timed out), so the work was stopped early.
    #[error("stopped: request was abandoned")]
    Abandoned,
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

/// Like [`run_blocking`], but the work can be stopped early.
///
/// The work is given a [`StopSignal`] and should check it now and then
/// (e.g. once per folder entry). The signal turns on as soon as the caller
/// stops waiting for the result: when a request is abandoned, axum drops
/// its handler, which drops this future, which turns the signal on.
///
/// Without this, a background thread would carry on to the end even though
/// the answer will be thrown away.
pub(crate) async fn run_blocking_stoppable<T, F>(work: F) -> Result<T, FsError>
where
    F: FnOnce(&StopSignal) -> Result<T, FsError> + Send + 'static,
    T: Send + 'static,
{
    let signal = StopSignal::default();
    let _stop_if_abandoned = StopOnDrop(signal.clone());
    run_blocking(move || work(&signal)).await
}

/// Tells blocking work whether it should give up.
#[derive(Clone, Default)]
pub(crate) struct StopSignal(Arc<AtomicBool>);

impl StopSignal {
    pub(crate) fn should_stop(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }

    fn stop(&self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

/// Turns the signal on when dropped. If the work already finished, this
/// does nothing useful, which is fine.
struct StopOnDrop(StopSignal);

impl Drop for StopOnDrop {
    fn drop(&mut self) {
        self.0.stop();
    }
}

#[cfg(test)]
mod stop_tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn abandoning_the_future_stops_the_work() {
        let (seen_tx, seen_rx) = std::sync::mpsc::channel();

        let work = run_blocking_stoppable(move |stop| {
            // Pretend to be a long folder scan that checks the signal.
            while !stop.should_stop() {
                std::thread::sleep(Duration::from_millis(1));
            }
            seen_tx.send(()).unwrap();
            Err::<(), _>(FsError::Abandoned)
        });

        // Give up waiting after a moment, as an abandoned request would.
        let result = tokio::time::timeout(Duration::from_millis(50), work).await;
        assert!(result.is_err(), "the work should still have been running");

        let stopped = seen_rx.recv_timeout(Duration::from_secs(2));
        assert!(
            stopped.is_ok(),
            "the background work never saw the stop signal"
        );
    }
}
