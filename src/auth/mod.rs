//! Device pairing, login and permissions (planning.md §7).
//!
//! - [`pairing`]: one-time pairing with a six-digit code (SPAKE2) plus
//!   approval by the host user.
//! - [`sessions`]: every-connection login by signing a challenge with the
//!   device's Ed25519 key.
//! - [`devices`]: the saved list of trusted devices and their permissions.
//! - [`extractor`]: `AuthenticatedDevice`, which handlers use to require a
//!   logged-in device.
//! - [`protocol`]: exact byte formats shared with clients.

pub mod crypto;
pub mod devices;
pub mod extractor;
pub mod pairing;
pub mod protocol;
pub mod rate_limit;
pub mod sessions;

pub use devices::Permission;
pub use extractor::AuthenticatedDevice;
