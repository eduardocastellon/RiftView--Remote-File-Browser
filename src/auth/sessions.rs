//! Logging in a paired device and keeping track of its session.
//!
//! 1. The device asks for a challenge: a random single-use nonce that
//!    expires after [`CHALLENGE_LIFETIME`].
//! 2. The device signs it with its Ed25519 private key.
//! 3. We check the signature against the public key saved at pairing and
//!    hand out a random session token, valid for the configured lifetime.
//!
//! The signed message includes our certificate's key hash, so a signature
//! made for this server is useless anywhere else. Each nonce works once, so
//! a captured signature can't be replayed.
//!
//! Everything here lives in memory. A restart just means devices log in
//! again.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use ed25519_dalek::{Signature, VerifyingKey};

use super::crypto;
use super::devices::DeviceStore;
use super::protocol;

pub const CHALLENGE_LIFETIME: Duration = Duration::from_secs(60);

/// Cap on outstanding challenges, so nobody can fill memory with them.
const MAX_PENDING_CHALLENGES: usize = 1024;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum LoginError {
    /// Unknown/expired challenge, unknown/revoked device, or bad signature.
    /// Deliberately one vague error so attackers learn nothing.
    #[error("login failed")]
    Failed,

    #[error("too many pending logins; try again shortly")]
    Busy,
}

struct Challenge {
    device_id: String,
    nonce: [u8; 32],
    expires_at: Instant,
}

struct Session {
    device_id: String,
    expires_at: Instant,
}

pub struct IssuedChallenge {
    pub challenge_id: String,
    pub nonce: [u8; 32],
}

pub struct IssuedSession {
    pub token: String,
    pub expires_in: Duration,
}

pub struct SessionManager {
    session_lifetime: Duration,
    challenges: Mutex<HashMap<String, Challenge>>,
    /// Keyed by SHA-256 of the token, so a memory dump doesn't reveal
    /// usable tokens.
    sessions: Mutex<HashMap<[u8; 32], Session>>,
}

impl SessionManager {
    pub fn new(session_lifetime: Duration) -> Self {
        Self {
            session_lifetime,
            challenges: Mutex::default(),
            sessions: Mutex::default(),
        }
    }

    /// Step 1. Issued for any device ID, known or not, so this can't be
    /// used to find out which IDs exist.
    pub fn issue_challenge(&self, device_id: &str) -> Result<IssuedChallenge, LoginError> {
        let mut challenges = lock(&self.challenges);
        let now = Instant::now();
        challenges.retain(|_, challenge| challenge.expires_at > now);

        if challenges.len() >= MAX_PENDING_CHALLENGES {
            return Err(LoginError::Busy);
        }

        let issued = IssuedChallenge {
            challenge_id: crypto::random_token(16),
            nonce: crypto::random_bytes(),
        };
        challenges.insert(
            issued.challenge_id.clone(),
            Challenge {
                device_id: device_id.to_owned(),
                nonce: issued.nonce,
                expires_at: now + CHALLENGE_LIFETIME,
            },
        );
        Ok(issued)
    }

    /// Steps 2–3: check the signed challenge and start a session.
    pub fn log_in(
        &self,
        challenge_id: &str,
        signature: &[u8],
        devices: &DeviceStore,
        host_key_hash: &[u8; 32],
    ) -> Result<IssuedSession, LoginError> {
        // Remove the challenge first: it's single-use whatever happens next.
        let challenge = lock(&self.challenges)
            .remove(challenge_id)
            .ok_or(LoginError::Failed)?;

        if Instant::now() >= challenge.expires_at {
            return Err(LoginError::Failed);
        }

        let device = devices
            .get_active(&challenge.device_id)
            .ok_or(LoginError::Failed)?;
        let public_key = device
            .public_key_bytes()
            .and_then(|bytes| VerifyingKey::from_bytes(&bytes).ok())
            .ok_or(LoginError::Failed)?;
        let signature = Signature::from_slice(signature).map_err(|_| LoginError::Failed)?;

        let message = protocol::login_message(host_key_hash, &device.id, &challenge.nonce);
        // `verify_strict` also rejects weak keys and malleable signatures.
        public_key
            .verify_strict(&message, &signature)
            .map_err(|_| LoginError::Failed)?;

        let token = crypto::random_token(32);
        let now = Instant::now();
        let mut sessions = lock(&self.sessions);
        sessions.retain(|_, session| session.expires_at > now);
        sessions.insert(
            crypto::sha256(token.as_bytes()),
            Session {
                device_id: device.id.clone(),
                expires_at: now + self.session_lifetime,
            },
        );

        Ok(IssuedSession {
            token,
            expires_in: self.session_lifetime,
        })
    }

    /// Which device does this token belong to? `None` if unknown or expired.
    pub fn device_for_token(&self, token: &str) -> Option<String> {
        let sessions = lock(&self.sessions);
        let session = sessions.get(&crypto::sha256(token.as_bytes()))?;

        if Instant::now() >= session.expires_at {
            return None;
        }
        Some(session.device_id.clone())
    }

    /// Log out one session.
    pub fn end_session(&self, token: &str) {
        lock(&self.sessions).remove(&crypto::sha256(token.as_bytes()));
    }

    /// End every session of a device (used when it is revoked).
    pub fn end_all_sessions_for(&self, device_id: &str) {
        lock(&self.sessions).retain(|_, session| session.device_id != device_id);
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
