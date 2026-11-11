//! Logging in with the device key, and info about the logged-in device.
//! See `crate::auth::sessions` for how it works.

use std::collections::BTreeSet;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};

use crate::auth::crypto::{decode_base64, encode_base64};
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

    let devices = state.devices().read().await;
    let session = state.sessions().log_in(
        &request.challenge_id,
        &signature,
        &devices,
        &state.identity().key_hash,
    )?;

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
    name: String,
    permissions: BTreeSet<Permission>,
}

/// `GET /api/devices/me`: who am I and what may I do?
pub async fn me(device: AuthenticatedDevice) -> Json<MeResponse> {
    Json(MeResponse {
        id: device.id,
        name: device.name,
        permissions: device.permissions,
    })
}
