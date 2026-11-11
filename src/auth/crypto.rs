//! Small, shared crypto helpers. Nothing clever lives here: randomness,
//! hashing, HMAC and an unambiguous way to join fields before signing.

use base64::Engine;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

type HmacSha256 = Hmac<Sha256>;

/// `N` bytes from the operating system's secure random generator.
pub fn random_bytes<const N: usize>() -> [u8; N] {
    let mut bytes = [0u8; N];
    getrandom::fill(&mut bytes).expect("OS random number generator failed");
    bytes
}

/// A random, URL-safe identifier or token with 128 bits of randomness or more.
pub fn random_token(byte_count: usize) -> String {
    let mut bytes = vec![0u8; byte_count];
    getrandom::fill(&mut bytes).expect("OS random number generator failed");
    URL_SAFE_NO_PAD.encode(bytes)
}

/// A uniformly random six-digit code, `000000`–`999999`.
pub fn six_digit_code() -> String {
    // Rejection sampling: throw away values in the uneven tail of the u32
    // range so every code is exactly equally likely.
    const CODES: u32 = 1_000_000;
    const LIMIT: u32 = u32::MAX - (u32::MAX % CODES);
    loop {
        let value = u32::from_be_bytes(random_bytes::<4>());
        if value < LIMIT {
            return format!("{:06}", value % CODES);
        }
    }
}

pub fn sha256(data: &[u8]) -> [u8; 32] {
    Sha256::digest(data).into()
}

pub fn hmac_sha256(key: &[u8], message: &[u8]) -> [u8; 32] {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(message);
    mac.finalize().into_bytes().into()
}

/// Check an HMAC in constant time (no timing leaks about where it differs).
pub fn hmac_sha256_matches(key: &[u8], message: &[u8], expected: &[u8]) -> bool {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(message);
    mac.verify_slice(expected).is_ok()
}

/// Join fields so they can't be confused with each other: each field is
/// written as a 4-byte big-endian length followed by its bytes.
///
/// Without lengths, `["ab", "c"]` and `["a", "bc"]` would produce the same
/// bytes and one could be passed off as the other.
pub fn transcript(fields: &[&[u8]]) -> Vec<u8> {
    let mut out = Vec::new();
    for field in fields {
        out.extend_from_slice(&(field.len() as u32).to_be_bytes());
        out.extend_from_slice(field);
    }
    out
}

pub fn decode_base64(text: &str) -> Option<Vec<u8>> {
    STANDARD
        .decode(text)
        .or_else(|_| URL_SAFE_NO_PAD.decode(text))
        .ok()
}

pub fn encode_base64(bytes: &[u8]) -> String {
    STANDARD.encode(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn six_digit_codes_are_six_digits() {
        for _ in 0..1000 {
            let code = six_digit_code();
            assert_eq!(code.len(), 6);
            assert!(code.chars().all(|c| c.is_ascii_digit()));
        }
    }

    #[test]
    fn transcript_is_unambiguous() {
        assert_ne!(transcript(&[b"ab", b"c"]), transcript(&[b"a", b"bc"]));
    }

    #[test]
    fn hmac_check() {
        let tag = hmac_sha256(b"key", b"message");
        assert!(hmac_sha256_matches(b"key", b"message", &tag));
        assert!(!hmac_sha256_matches(b"key", b"other", &tag));
    }
}
