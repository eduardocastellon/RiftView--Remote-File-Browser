//! Moving file contents between devices and the host (planning.md §8).
//!
//! Downloads:
//! - [`version`]: a fingerprint of a file's metadata, sent as the `ETag`,
//!   so a resumed download never mixes bytes from two versions of a file.
//! - [`stream`]: the download body. It streams in small chunks and cuts the
//!   connection if the file changes while it is being sent.
//! - [`hashing`]: SHA-256 of a whole file, so a device can verify what it
//!   downloaded, with a small in-memory cache.
//!
//! Uploads:
//! - [`uploads`]: unfinished uploads and their received chunks (SQLite).
//! - [`cleanup`]: removing cancelled, expired and failed uploads.
//!
//! Both:
//! - [`limits`]: how many transfers may run at once, per device and in
//!   total.
//!
//! Like [`crate::filesystem`], this module knows nothing about HTTP routes.

pub mod cleanup;
pub mod hashing;
pub mod limits;
pub mod stream;
pub mod uploads;
pub mod version;
