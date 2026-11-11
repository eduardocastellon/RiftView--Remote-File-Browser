//! The host's HTTPS identity.
//!
//! On first run we create a self-signed certificate and keep it in
//! `<data dir>/tls/`. Nobody vouches for a self-signed certificate, so
//! clients can't check it the usual way. Instead each client learns the
//! certificate's **pin** during pairing (protected by the pairing code) and
//! from then on only talks to a server presenting that exact key.
//!
//! The pin is the SHA-256 hash of the certificate's public key
//! (`SubjectPublicKeyInfo`), written as `sha256/<base64>`. That's the
//! format most mobile pinning libraries expect, and it stays the same if
//! the certificate is ever re-issued with the same key.

use std::path::{Path, PathBuf};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use rcgen::{CertificateParams, KeyPair, PublicKeyData};
use sha2::{Digest, Sha256};

use crate::storage;

/// Names put in the certificate. Clients pin the key rather than checking
/// names, so these are informational only.
const CERT_NAMES: [&str; 2] = ["riftview.local", "localhost"];

pub struct HostIdentity {
    pub cert_pem: Vec<u8>,
    pub key_pem: Vec<u8>,
    /// SHA-256 of the public key. Mixed into pairing and login proofs so
    /// they can't be relayed to a different server.
    pub key_hash: [u8; 32],
}

#[derive(Debug, thiserror::Error)]
pub enum TlsError {
    #[error("TLS identity I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error("TLS key or certificate is invalid: {0}")]
    Invalid(#[from] rcgen::Error),
}

impl HostIdentity {
    /// Load the certificate from the data folder, creating one on first run.
    pub fn load_or_create(data_dir: &Path) -> Result<Self, TlsError> {
        let (cert_path, key_path) = identity_paths(data_dir);

        if !cert_path.exists() || !key_path.exists() {
            create(&cert_path, &key_path)?;
            tracing::info!("created new HTTPS certificate");
        }

        let cert_pem = std::fs::read(&cert_path)?;
        let key_pem = std::fs::read(&key_path)?;

        let key_text = String::from_utf8_lossy(&key_pem);
        let key_pair = KeyPair::from_pem(&key_text)?;
        let key_hash = Sha256::digest(key_pair.subject_public_key_info()).into();

        Ok(Self {
            cert_pem,
            key_pem,
            key_hash,
        })
    }

    /// The pin clients store, e.g. `sha256/47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU=`.
    pub fn pin(&self) -> String {
        format_pin(&self.key_hash)
    }
}

pub fn format_pin(key_hash: &[u8; 32]) -> String {
    format!("sha256/{}", STANDARD.encode(key_hash))
}

fn identity_paths(data_dir: &Path) -> (PathBuf, PathBuf) {
    let folder = data_dir.join("tls");
    (folder.join("cert.pem"), folder.join("key.pem"))
}

fn create(cert_path: &Path, key_path: &Path) -> Result<(), TlsError> {
    if let Some(folder) = cert_path.parent() {
        storage::ensure_data_dir(folder)?;
    }

    let names: Vec<String> = CERT_NAMES.iter().map(|name| name.to_string()).collect();
    let key_pair = KeyPair::generate()?;
    let certificate = CertificateParams::new(names)?.self_signed(&key_pair)?;

    // Write the key first: a certificate without its key is useless, but
    // the reverse would make us silently create a second key next time.
    storage::write_bytes_atomic(key_path, key_pair.serialize_pem().as_bytes())?;
    storage::write_bytes_atomic(cert_path, certificate.pem().as_bytes())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_is_created_once_and_reloaded_unchanged() {
        let data_dir = tempfile::tempdir().unwrap();
        let first = HostIdentity::load_or_create(data_dir.path()).unwrap();
        let second = HostIdentity::load_or_create(data_dir.path()).unwrap();

        assert_eq!(first.key_hash, second.key_hash);
        assert!(first.pin().starts_with("sha256/"));
    }
}
