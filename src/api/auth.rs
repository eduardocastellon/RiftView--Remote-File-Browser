//! Logging in with the device key, and info about the logged-in device.
//! See `crate::auth::sessions` for how it works.

use std::collections::BTreeSet;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};

use crate::auth::crypto::{decode_base64, encode_base64};
use crate::auth::devices::{DeviceType, Platform, validate_app_version};
use crate::auth::sessions::CHALLENGE_LIFETIME;
use crate::auth::{AuthenticatedDevice, Permission};
use crate::error::AppError;
use crate::state::AppState;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChallengeRequest {
    device_id: String,
}

#[derive(Serialize)]
pub struct ChallengeResponse {
    challenge_id: String,
    /// 32 random bytes to sign, base64.
    nonce: String,
    expires_in_secs: u64,
}

/// `POST /api/auth/challenge`
pub async fn challenge(
    State(state): State<AppState>,
    Json(request): Json<ChallengeRequest>,
) -> Result<Json<ChallengeResponse>, AppError> {
    let issued = state.sessions().issue_challenge(&request.device_id)?;

    Ok(Json(ChallengeResponse {
        challenge_id: issued.challenge_id,
        nonce: encode_base64(&issued.nonce),
        expires_in_secs: CHALLENGE_LIFETIME.as_secs(),
    }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LoginRequest {
    challenge_id: String,
    /// Ed25519 signature (64 bytes) of the login message, base64.
    signature: String,
    /// The app's current version, if it changed since pairing (optional).
    #[serde(default)]
    app_version: Option<String>,
}

#[derive(Serialize)]
pub struct LoginResponse {
    /// Send as `Authorization: Bearer <token>` on every request.
    token: String,
    expires_in_secs: u64,
}

/// `POST /api/auth/session`
pub async fn log_in(
    State(state): State<AppState>,
    Json(request): Json<LoginRequest>,
) -> Result<Json<LoginResponse>, AppError> {
    let signature = decode_base64(&request.signature).ok_or(AppError::Unauthorized)?;
    let app_version = request
        .app_version
        .as_deref()
        .map(validate_app_version)
        .transpose()
        .map_err(|reason| AppError::BadRequest(reason.into()))?;

    let session = {
        let devices = state.devices().read().await;
        state.sessions().log_in(
            &request.challenge_id,
            &signature,
            &devices,
            &state.identity().key_hash,
        )?
    };

    // The login already succeeded, so a failure to save this bookkeeping is
    // logged rather than turned into an error for the device.
    let recorded = state
        .devices()
        .write()
        .await
        .record_login(&session.device_id, app_version);
    if let Err(error) = recorded {
        tracing::warn!(device_id = %session.device_id, %error, "could not record login");
    }

    Ok(Json(LoginResponse {
        token: session.token,
        expires_in_secs: session.expires_in.as_secs(),
    }))
}

/// `DELETE /api/auth/session`: log out.
pub async fn log_out(device: AuthenticatedDevice, State(state): State<AppState>) -> StatusCode {
    state.sessions().end_session(&device.token);
    StatusCode::NO_CONTENT
}

#[derive(Serialize)]
pub struct MeResponse {
    id: String,
    /// The host user's alias for this device if set, otherwise its own name.
    name: String,
    device_type: DeviceType,
    platform: Platform,
    permissions: BTreeSet<Permission>,
}

/// `GET /api/devices/me`: who am I and what may I do?
pub async fn me(device: AuthenticatedDevice) -> Json<MeResponse> {
    Json(MeResponse {
        id: device.id,
        name: device.name,
        device_type: device.device_type,
        platform: device.platform,
        permissions: device.permissions,
    })
}
