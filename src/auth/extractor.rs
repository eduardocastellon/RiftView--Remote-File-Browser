//! `AuthenticatedDevice`: add it as a handler argument and the request is
//! rejected with 401 unless it carries a valid session token.

use std::collections::BTreeSet;

use axum::extract::FromRequestParts;
use axum::http::header::AUTHORIZATION;
use axum::http::request::Parts;

use super::devices::Permission;
use crate::error::AppError;
use crate::state::AppState;

/// The logged-in device making this request.
#[derive(Debug, Clone)]
pub struct AuthenticatedDevice {
    pub id: String,
    pub name: String,
    pub permissions: BTreeSet<Permission>,
    /// The bearer token used, so the device can log itself out.
    pub token: String,
}

impl AuthenticatedDevice {
    /// Fail with 403 unless the device has `permission`.
    pub fn require(&self, permission: Permission) -> Result<(), AppError> {
        if self.permissions.contains(&permission) {
            Ok(())
        } else {
            Err(AppError::Forbidden)
        }
    }
}

impl FromRequestParts<AppState> for AuthenticatedDevice {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, AppError> {
        let token = bearer_token(parts).ok_or(AppError::Unauthorized)?;
        let device_id = state
            .sessions()
            .device_for_token(token)
            .ok_or(AppError::Unauthorized)?;

        // Re-read the device on every request, so revocations and
        // permission changes take effect immediately.
        let devices = state.devices().read().await;
        let device = devices
            .get_active(&device_id)
            .ok_or(AppError::Unauthorized)?;

        Ok(Self {
            id: device.id.clone(),
            name: device.name.clone(),
            permissions: device.permissions.clone(),
            token: token.to_owned(),
        })
    }
}

/// The token from an `Authorization: Bearer <token>` header.
pub fn bearer_token(parts: &Parts) -> Option<&str> {
    let header = parts.headers.get(AUTHORIZATION)?.to_str().ok()?;
    let token = header.strip_prefix("Bearer ")?.trim();
    (!token.is_empty()).then_some(token)
}
