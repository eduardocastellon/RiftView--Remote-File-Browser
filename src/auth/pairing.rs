//! Pairing a new device with a six-digit code (SPAKE2) plus host approval.
//!
//! The steps, and who does what:
//!
//! 1. **Host** runs `riftview-api pair`. We create a session with a fresh
//!    code, valid for [`PAIRING_LIFETIME`].
//! 2. **Device** sends its SPAKE2 message ([`PairingManager::initiate`]).
//!    We answer with ours. Both sides now hold a shared key, but only if
//!    they used the same code. The code itself is never sent.
//! 3. **Device** proves it has the right key by sending an HMAC over the
//!    certificate pin it saw, its public key and its name
//!    ([`PairingManager::complete`]). A wrong proof kills the session, so an
//!    attacker gets exactly one guess (1 in a million).
//! 4. **Host** sees the device's name and key fingerprint and approves or
//!    rejects it.
//! 5. **Device** polls [`PairingManager::device_status`] until it sees the
//!    result.
//!
//! Only one pairing session exists at a time. Starting a new one cancels
//! the old one.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;
use spake2::{Ed25519Group, Identity, Password, Spake2};

use super::crypto;
use super::devices::key_fingerprint;
use super::protocol::{self, SPAKE2_CLIENT_ID, SPAKE2_HOST_ID};

/// How long a pairing code stays valid.
pub const PAIRING_LIFETIME: Duration = Duration::from_secs(5 * 60);

const MAX_DEVICE_NAME_LEN: usize = 64;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PairingError {
    #[error("no pairing in progress; start one on the host with `riftview-api pair`")]
    NoSession,

    #[error("pairing code has expired")]
    Expired,

    #[error("another device is already pairing")]
    AlreadyClaimed,

    #[error("pairing is not at the right step for this request")]
    WrongStep,

    /// Wrong code, tampered message, or a man-in-the-middle. Deliberately
    /// vague so attackers learn nothing.
    #[error("pairing failed")]
    Failed,

    #[error("{0}")]
    Invalid(&'static str),
}

/// A device that proved it knows the code and is waiting for host approval.
#[derive(Debug, Clone)]
pub struct PendingDevice {
    /// ID the device will get if approved (already sent to it in the host
    /// confirmation, so it can't change).
    pub device_id: String,
    pub name: String,
    pub public_key: [u8; 32],
}

#[derive(Debug)]
enum Stage {
    /// Code shown on the host; waiting for a device to start.
    WaitingForDevice,
    /// SPAKE2 done; waiting for the device's proof.
    KeyAgreed {
        shared_key: Vec<u8>,
    },
    /// Device proved itself; waiting for the host user.
    AwaitingApproval(PendingDevice),
    Approved {
        device_id: String,
    },
    Rejected,
    Failed,
}

impl Stage {
    fn is_finished(&self) -> bool {
        matches!(
            self,
            Stage::Approved { .. } | Stage::Rejected | Stage::Failed
        )
    }
}

#[derive(Debug)]
struct Session {
    id: String,
    code: String,
    expires_at: Instant,
    stage: Stage,
}

impl Session {
    fn is_expired(&self) -> bool {
        Instant::now() >= self.expires_at
    }
}

/// What the host user's tool sees (includes the device's details).
#[derive(Debug, Serialize)]
pub struct HostView {
    pub pairing_id: String,
    pub stage: &'static str,
    pub expires_in_secs: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_key_fingerprint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_id: Option<String>,
}

/// What the pairing device sees while it waits.
#[derive(Debug, Serialize)]
pub struct DeviceView {
    pub stage: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_id: Option<String>,
}

/// Returned to the device after a correct proof.
#[derive(Debug)]
pub struct HostProof {
    /// The ID the device will have once approved.
    pub device_id: String,
    /// HMAC proving the host knows the code too.
    pub proof: Vec<u8>,
}

pub struct StartedPairing {
    pub pairing_id: String,
    pub code: String,
    pub expires_in_secs: u64,
}

#[derive(Default)]
pub struct PairingManager {
    current: Mutex<Option<Session>>,
}

impl PairingManager {
    /// Host: start a new pairing session (cancelling any previous one).
    pub fn start(&self) -> StartedPairing {
        let session = Session {
            id: format!("pair_{}", uuid::Uuid::new_v4().simple()),
            code: crypto::six_digit_code(),
            expires_at: Instant::now() + PAIRING_LIFETIME,
            stage: Stage::WaitingForDevice,
        };
        let started = StartedPairing {
            pairing_id: session.id.clone(),
            code: session.code.clone(),
            expires_in_secs: PAIRING_LIFETIME.as_secs(),
        };
        *self.lock() = Some(session);
        started
    }

    /// Host: cancel the current session, if any.
    pub fn cancel(&self) {
        *self.lock() = None;
    }

    /// Device, step 2: exchange SPAKE2 messages.
    /// Returns `(pairing_id, host_spake2_message)`.
    pub fn initiate(&self, device_message: &[u8]) -> Result<(String, Vec<u8>), PairingError> {
        let mut current = self.lock();
        let session = current.as_mut().ok_or(PairingError::NoSession)?;

        if session.is_expired() {
            return Err(PairingError::Expired);
        }
        if !matches!(session.stage, Stage::WaitingForDevice) {
            return Err(PairingError::AlreadyClaimed);
        }

        let (spake, host_message) = Spake2::<Ed25519Group>::start_b(
            &Password::new(session.code.as_bytes()),
            &Identity::new(SPAKE2_CLIENT_ID),
            &Identity::new(SPAKE2_HOST_ID),
        );

        // `finish` only fails on a malformed message. A wrong code is not
        // detected here; it shows up as a bad proof in `complete`.
        match spake.finish(device_message) {
            Ok(shared_key) => {
                session.stage = Stage::KeyAgreed { shared_key };
                Ok((session.id.clone(), host_message))
            }
            Err(_) => {
                session.stage = Stage::Failed;
                Err(PairingError::Failed)
            }
        }
    }

    /// Device, step 3: check the device's proof. On success returns the
    /// host's proof for the device to check, then waits for host approval.
    pub fn complete(
        &self,
        pairing_id: &str,
        device_name: &str,
        device_public_key: [u8; 32],
        device_proof: &[u8],
        host_key_hash: &[u8; 32],
    ) -> Result<HostProof, PairingError> {
        let device_name = validate_device_name(device_name)?;

        let mut current = self.lock();
        let session = current
            .as_mut()
            .filter(|session| session.id == pairing_id)
            .ok_or(PairingError::NoSession)?;

        if session.is_expired() {
            return Err(PairingError::Expired);
        }
        let Stage::KeyAgreed { shared_key } = &session.stage else {
            return Err(PairingError::WrongStep);
        };

        let expected =
            protocol::client_confirm_message(host_key_hash, &device_public_key, &device_name);
        if !crypto::hmac_sha256_matches(shared_key, &expected, device_proof) {
            // One guess only: any wrong proof ends this session for good.
            session.stage = Stage::Failed;
            return Err(PairingError::Failed);
        }

        let device_id = format!("dev_{}", uuid::Uuid::new_v4().simple());
        let host_message =
            protocol::host_confirm_message(host_key_hash, &device_public_key, &device_id);
        let proof = crypto::hmac_sha256(shared_key, &host_message).to_vec();

        session.stage = Stage::AwaitingApproval(PendingDevice {
            device_id: device_id.clone(),
            name: device_name,
            public_key: device_public_key,
        });
        Ok(HostProof { device_id, proof })
    }

    /// Host, step 4: approve. Returns the device to add to the trusted list.
    pub fn approve(&self, pairing_id: &str) -> Result<PendingDevice, PairingError> {
        let mut current = self.lock();
        let session = current
            .as_mut()
            .filter(|session| session.id == pairing_id)
            .ok_or(PairingError::NoSession)?;

        if session.is_expired() {
            return Err(PairingError::Expired);
        }
        let Stage::AwaitingApproval(pending) = &session.stage else {
            return Err(PairingError::WrongStep);
        };

        let pending = pending.clone();
        session.stage = Stage::Approved {
            device_id: pending.device_id.clone(),
        };
        Ok(pending)
    }

    /// Host, step 4: reject.
    pub fn reject(&self, pairing_id: &str) -> Result<(), PairingError> {
        let mut current = self.lock();
        let session = current
            .as_mut()
            .filter(|session| session.id == pairing_id)
            .ok_or(PairingError::NoSession)?;

        session.stage = Stage::Rejected;
        Ok(())
    }

    /// Host: current session details.
    pub fn host_view(&self) -> Option<HostView> {
        let current = self.lock();
        let session = current.as_ref()?;

        let (device_name, device_key_fingerprint, device_id) = match &session.stage {
            Stage::AwaitingApproval(pending) => (
                Some(pending.name.clone()),
                Some(key_fingerprint(&pending.public_key)),
                None,
            ),
            Stage::Approved { device_id } => (None, None, Some(device_id.clone())),
            _ => (None, None, None),
        };

        Some(HostView {
            pairing_id: session.id.clone(),
            stage: stage_name(session),
            expires_in_secs: session
                .expires_at
                .saturating_duration_since(Instant::now())
                .as_secs(),
            device_name,
            device_key_fingerprint,
            device_id,
        })
    }

    /// Device, step 5: has the host decided yet?
    pub fn device_status(&self, pairing_id: &str) -> Result<DeviceView, PairingError> {
        let current = self.lock();
        let session = current
            .as_ref()
            .filter(|session| session.id == pairing_id)
            .ok_or(PairingError::NoSession)?;

        let device_id = match &session.stage {
            Stage::Approved { device_id } => Some(device_id.clone()),
            _ => None,
        };
        Ok(DeviceView {
            stage: stage_name(session),
            device_id,
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Option<Session>> {
        // A panic while holding this lock can't leave the data half-changed
        // in a dangerous way, so carry on with the inner value.
        self.current
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// Stable names for each stage, used in API responses.
fn stage_name(session: &Session) -> &'static str {
    if session.is_expired() && !session.stage.is_finished() {
        return "expired";
    }
    match session.stage {
        Stage::WaitingForDevice => "waiting_for_device",
        Stage::KeyAgreed { .. } => "key_agreed",
        Stage::AwaitingApproval(_) => "awaiting_approval",
        Stage::Approved { .. } => "approved",
        Stage::Rejected => "rejected",
        Stage::Failed => "failed",
    }
}

fn validate_device_name(name: &str) -> Result<String, PairingError> {
    let name = name.trim();
    let is_valid = !name.is_empty()
        && name.chars().count() <= MAX_DEVICE_NAME_LEN
        && !name.chars().any(char::is_control);

    if !is_valid {
        return Err(PairingError::Invalid(
            "device name must be 1-64 printable characters",
        ));
    }
    Ok(name.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOST_KEY_HASH: [u8; 32] = [9u8; 32];
    const DEVICE_KEY: [u8; 32] = [5u8; 32];

    /// Play the device's side of steps 2–3. Returns the result of `complete`.
    fn device_pairs_with(
        manager: &PairingManager,
        code: &str,
        seen_host_key_hash: &[u8; 32],
    ) -> Result<(String, Vec<u8>, HostProof), PairingError> {
        let (spake, device_message) = Spake2::<Ed25519Group>::start_a(
            &Password::new(code.as_bytes()),
            &Identity::new(SPAKE2_CLIENT_ID),
            &Identity::new(SPAKE2_HOST_ID),
        );
        let (pairing_id, host_message) = manager.initiate(&device_message)?;
        let shared_key = spake.finish(&host_message).unwrap();

        let proof_message =
            protocol::client_confirm_message(seen_host_key_hash, &DEVICE_KEY, "Phone");
        let proof = crypto::hmac_sha256(&shared_key, &proof_message);
        let host_proof =
            manager.complete(&pairing_id, "Phone", DEVICE_KEY, &proof, &HOST_KEY_HASH)?;
        Ok((pairing_id, shared_key, host_proof))
    }

    #[test]
    fn right_code_pairs_and_host_proof_checks_out() {
        let manager = PairingManager::default();
        let started = manager.start();

        let (pairing_id, shared_key, host_proof) =
            device_pairs_with(&manager, &started.code, &HOST_KEY_HASH).unwrap();

        let view = manager.host_view().unwrap();
        assert_eq!(view.stage, "awaiting_approval");
        assert_eq!(view.device_name.as_deref(), Some("Phone"));

        let expected =
            protocol::host_confirm_message(&HOST_KEY_HASH, &DEVICE_KEY, &host_proof.device_id);
        assert!(crypto::hmac_sha256_matches(
            &shared_key,
            &expected,
            &host_proof.proof
        ));

        let pending = manager.approve(&pairing_id).unwrap();
        assert_eq!(pending.device_id, host_proof.device_id);

        let status = manager.device_status(&pairing_id).unwrap();
        assert_eq!(status.stage, "approved");
        assert_eq!(status.device_id, Some(pending.device_id));
    }

    #[test]
    fn wrong_code_fails_and_burns_the_session() {
        let manager = PairingManager::default();
        let started = manager.start();
        let wrong_code = if started.code == "000000" {
            "000001"
        } else {
            "000000"
        };

        assert_eq!(
            device_pairs_with(&manager, wrong_code, &HOST_KEY_HASH).unwrap_err(),
            PairingError::Failed
        );
        // Even the right code no longer works: one guess per session.
        assert_eq!(
            device_pairs_with(&manager, &started.code, &HOST_KEY_HASH).unwrap_err(),
            PairingError::AlreadyClaimed
        );
    }

    #[test]
    fn man_in_the_middle_certificate_is_detected() {
        let manager = PairingManager::default();
        let started = manager.start();
        let attacker_key_hash = [1u8; 32];

        let result = device_pairs_with(&manager, &started.code, &attacker_key_hash);
        assert_eq!(result.unwrap_err(), PairingError::Failed);
    }

    #[test]
    fn nothing_to_pair_with_until_host_starts() {
        let manager = PairingManager::default();
        assert_eq!(manager.initiate(b"x").unwrap_err(), PairingError::NoSession);
    }
}
