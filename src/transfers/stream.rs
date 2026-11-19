//! The body of a download response.
//!
//! The file is read and sent in small chunks, so memory use stays flat
//! however big the file is. If the device disconnects, the body is dropped,
//! which closes the file and stops reading.
//!
//! ## Files that change while they're being sent
//!
//! We send from one open file handle, so if the file is *replaced* (saved
//! as a new file and renamed over the old one) the device still gets the
//! old version, complete and consistent.
//!
//! But if the file is *edited in place*, the device could receive some old
//! bytes and some new ones. To stop that from passing as a good download,
//! we check the file's version every few megabytes and once more just
//! before the final chunk. If it changed, we end the stream with an error.
//! That cuts the connection before the last bytes arrive, so the device
//! sees an incomplete download (fewer bytes than `Content-Length`) and can
//! retry instead of saving a corrupted file.

use std::fs::File;
use std::io;

use axum::body::Bytes;
use futures_util::{Stream, StreamExt};
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio_util::io::ReaderStream;

use super::limits::TransferSlot;
use super::version::FileVersion;

/// How much of a file we hold in memory at once.
const CHUNK_BYTES: usize = 64 * 1024;

/// How often (in bytes sent) we check whether the file has changed.
/// Checking is cheap (one `fstat` call), but there's no need to do it for
/// every 64 KiB chunk.
const CHECK_EVERY_BYTES: u64 = 8 * 1024 * 1024;

/// What a download needs to stream one byte range of a file.
pub struct DownloadSource<R> {
    /// Positioned at the first byte to send.
    pub reader: R,
    /// How many bytes to send from there.
    pub byte_count: u64,
    /// A second handle on the same file, used to check its version.
    pub watcher: File,
    /// The version we promised the device (in the `ETag` header).
    pub version: FileVersion,
    /// Held until the stream ends, so the download counts towards limits.
    pub slot: TransferSlot,
}

/// Turn a [`DownloadSource`] into a stream of chunks for the response body.
pub fn download_stream<R>(source: DownloadSource<R>) -> impl Stream<Item = io::Result<Bytes>>
where
    R: AsyncRead + Unpin + Send + 'static,
{
    let state = StreamState {
        chunks: ReaderStream::with_capacity(source.reader.take(source.byte_count), CHUNK_BYTES),
        bytes_left: source.byte_count,
        bytes_since_check: 0,
        watcher: source.watcher,
        version: source.version,
        _slot: source.slot,
    };

    // `None` means the stream has finished (or failed) and must stop.
    futures_util::stream::unfold(Some(state), |state| async move {
        let mut state = state?;
        match state.next_chunk().await {
            Ok(Some(chunk)) => Some((Ok(chunk), Some(state))),
            Ok(None) => None,
            Err(error) => Some((Err(error), None)),
        }
    })
}

struct StreamState<R> {
    chunks: ReaderStream<tokio::io::Take<R>>,
    bytes_left: u64,
    bytes_since_check: u64,
    watcher: File,
    version: FileVersion,
    _slot: TransferSlot,
}

impl<R: AsyncRead + Unpin> StreamState<R> {
    /// The next chunk to send, `None` when everything has been sent, or an
    /// error if the transfer must be cut short.
    async fn next_chunk(&mut self) -> io::Result<Option<Bytes>> {
        let chunk = match self.chunks.next().await {
            Some(chunk) => chunk?,
            None if self.bytes_left == 0 => return Ok(None),
            None => return Err(file_changed("file got shorter during download")),
        };

        let chunk_length = chunk.len() as u64;
        self.bytes_left = self.bytes_left.saturating_sub(chunk_length);
        self.bytes_since_check += chunk_length;

        let is_last_chunk = self.bytes_left == 0;
        if is_last_chunk || self.bytes_since_check >= CHECK_EVERY_BYTES {
            self.bytes_since_check = 0;
            if FileVersion::of(&self.watcher)? != self.version {
                return Err(file_changed("file changed during download"));
            }
        }

        Ok(Some(chunk))
    }
}

fn file_changed(reason: &'static str) -> io::Error {
    tracing::warn!("{reason}; closing the connection");
    io::Error::other(reason)
}
