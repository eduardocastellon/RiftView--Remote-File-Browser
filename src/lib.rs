//! RiftView host service: secure, self-hosted remote access to explicitly
//! shared directories.
//!
//! Layering (see `planning.md` §3):
//! - [`filesystem`] knows nothing about HTTP; it exposes safe, confined
//!   operations on shared roots.
//! - [`api`] translates HTTP requests into filesystem calls.
//! - [`storage`] persists small JSON state files atomically.

pub mod api;
pub mod config;
pub mod error;
pub mod filesystem;
pub mod platform;
pub mod server;
pub mod state;
pub mod storage;
