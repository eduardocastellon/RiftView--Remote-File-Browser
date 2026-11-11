//! The admin token: a random secret in `<data dir>/admin_token`, readable
//! only by the OS user running RiftView.
//!
//! The admin API only listens on loopback, but other users or programs on
//! the same computer could still reach it. Requiring this token means only
//! someone who can read the host user's files can manage RiftView.

use std::path::{Path, PathBuf};

use crate::auth::crypto;
use crate::storage;

const TOKEN_FILE_NAME: &str = "admin_token";

pub struct AdminToken(String);

impl AdminToken {
    pub fn load_or_create(data_dir: &Path) -> std::io::Result<Self> {
        let path = token_path(data_dir);

        if let Some(existing) = Self::read(data_dir)? {
            return Ok(existing);
        }

        let token = crypto::random_token(32);
        storage::write_bytes_atomic(&path, token.as_bytes())?;
        Ok(Self(token))
    }

    /// Read an existing token (used by the command-line tools).
    pub fn read(data_dir: &Path) -> std::io::Result<Option<Self>> {
        match std::fs::read_to_string(token_path(data_dir)) {
            Ok(text) if !text.trim().is_empty() => Ok(Some(Self(text.trim().to_owned()))),
            Ok(_) => Ok(None),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Compare in constant time, so response timing reveals nothing.
    pub fn matches(&self, candidate: &str) -> bool {
        let expected = crypto::sha256(self.0.as_bytes());
        let actual = crypto::sha256(candidate.as_bytes());
        // Comparing fixed-length hashes with a fold avoids early exit.
        expected
            .iter()
            .zip(actual.iter())
            .fold(0u8, |diff, (a, b)| diff | (a ^ b))
            == 0
    }
}

fn token_path(data_dir: &Path) -> PathBuf {
    data_dir.join(TOKEN_FILE_NAME)
}
