//! RiftView host service: secure, self-hosted remote access to folders the
//! host user has explicitly chosen to share.
//!
//! How the pieces fit together (see `planning.md` §3):
//!
//! - [`filesystem`] reads files and folders inside shares. It knows nothing
//!   about HTTP.
//! - [`transfers`] sends file contents: streaming, ETags, SHA-256 hashes
//!   and download limits.
//! - [`api`] turns HTTP requests into calls to [`filesystem`] and
//!   [`transfers`].
//! - [`database`] is the SQLite database holding shares and devices.
//! - [`storage`] saves small settings files (config, admin token,
//!   certificate) safely.
//! - [`server`] wires everything together and starts listening.

pub mod admin;
pub mod api;
pub mod auth;
pub mod cli;
pub mod config;
pub mod database;
pub mod error;
pub mod filesystem;
pub mod platform;
pub mod server;
pub mod state;
pub mod storage;
pub mod tls;
pub mod transfers;
