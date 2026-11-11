//! Pairing endpoints the device calls. See `crate::auth::pairing` for the
//! full flow and `docs/auth-protocol.md` for client instructions.

use axum::Json;
use axum::extract::{Path, State};
use serde::{Deserialize, Serialize};

use crate::auth::crypto::{decode_base64, encode_base64};
use crate::auth::pairing::DeviceView;
use crate::error::AppError;
use crate::state::AppState;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InitiateRequest {
    /// The device's SPAKE2 message (side A), base64.
    spake2_message: String,
}

#[derive(Serialize)]
pub struct InitiateResponse {
    pairing_id: String,
    /// The host's SPAKE2 message (side B), base64.
    spake2_message: String,
}

/// `POST /api/pairing/initiate`
pub async fn initiate(
    State(state): State<AppState>,
    Json(request): Json<InitiateRequest>,
) -> Result<Json<InitiateResponse>, AppError> {
    let device_message = decode_base64(&request.spake2_message)
        .ok_or_else(|| AppError::BadRequest("spake2_message must be base64".into()))?;

    let (pairing_id, host_message) = state.pairing().initiate(&device_message)?;

    Ok(Json(InitiateResponse {
        pairing_id,
        spake2_message: encode_base64(&host_message),
    }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompleteRequest {
    pairing_id: String,
    device_name: String,
    /// The device's Ed25519 public key (32 bytes), base64.
    device_public_key: String,
    /// HMAC proving the device knows the code, base64.
    proof: String,
}

#[derive(Serialize)]
pub struct CompleteResponse {
    /// Always `awaiting_approval`: the host user must now approve.
    stage: &'static str,
    /// The ID this device will have once approved.
    device_id: String,
    /// HMAC proving the host knows the code, base64. The device must check
    /// it before trusting anything else.
    host_proof: String,
    /// Certificate pin to store, e.g. `sha256/...`.
    host_pin: String,
}

/// `POST /api/pairing/complete`
pub async fn complete(
    State(state): State<AppState>,
    Json(request): Json<CompleteRequest>,
) -> Result<Json<CompleteResponse>, AppError> {
    let device_public_key: [u8; 32] = decode_base64(&request.device_public_key)
        .and_then(|bytes| bytes.try_into().ok())
        .ok_or_else(|| AppError::BadRequest("device_public_key must be 32 bytes, base64".into()))?;
    let proof = decode_base64(&request.proof)
        .ok_or_else(|| AppError::BadRequest("proof must be base64".into()))?;

    let identity = state.identity();
    let host_proof = state.pairing().complete(
        &request.pairing_id,
        &request.device_name,
        device_public_key,
        &proof,
        &identity.key_hash,
    )?;

    Ok(Json(CompleteResponse {
        stage: "awaiting_approval",
        device_id: host_proof.device_id,
        host_proof: encode_base64(&host_proof.proof),
        host_pin: identity.pin(),
    }))
}

/// `GET /api/pairing/{id}`: poll until the host approves or rejects.
pub async fn status(
    State(state): State<AppState>,
    Path(pairing_id): Path<String>,
) -> Result<Json<DeviceView>, AppError> {
    Ok(Json(state.pairing().device_status(&pairing_id)?))
}
