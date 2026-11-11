//! Exact byte formats shared with clients. Changing anything here breaks
//! every paired device, so bump the `-v1` labels if you must.
//!
//! The full protocol is described in `docs/auth-protocol.md`.

use super::crypto::transcript;

/// SPAKE2 identity of the client (SPAKE2 side "A").
pub const SPAKE2_CLIENT_ID: &[u8] = b"riftview-client";

/// SPAKE2 identity of the host (SPAKE2 side "B").
pub const SPAKE2_HOST_ID: &[u8] = b"riftview-host";

const CLIENT_CONFIRM_LABEL: &[u8] = b"riftview-pair-v1 client-confirm";
const HOST_CONFIRM_LABEL: &[u8] = b"riftview-pair-v1 host-confirm";
const LOGIN_LABEL: &[u8] = b"riftview-login-v1";

/// What the client MACs (with the SPAKE2 key) to prove it knows the code.
///
/// Including the certificate key hash *the client saw* means a
/// man-in-the-middle with a different certificate makes this check fail.
pub fn client_confirm_message(
    host_key_hash: &[u8; 32],
    device_public_key: &[u8; 32],
    device_name: &str,
) -> Vec<u8> {
    transcript(&[
        CLIENT_CONFIRM_LABEL,
        host_key_hash,
        device_public_key,
        device_name.as_bytes(),
    ])
}

/// What the host MACs back, proving to the client that it too knows the
/// code and agrees on the certificate and the device key.
pub fn host_confirm_message(
    host_key_hash: &[u8; 32],
    device_public_key: &[u8; 32],
    device_id: &str,
) -> Vec<u8> {
    transcript(&[
        HOST_CONFIRM_LABEL,
        host_key_hash,
        device_public_key,
        device_id.as_bytes(),
    ])
}

/// What a paired device signs with its Ed25519 key to log in.
pub fn login_message(host_key_hash: &[u8; 32], device_id: &str, nonce: &[u8]) -> Vec<u8> {
    transcript(&[LOGIN_LABEL, host_key_hash, device_id.as_bytes(), nonce])
}
