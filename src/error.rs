//! Errors as clients see them.
//!
//! Every message here is safe to send over the network: none of them
//! include real host paths or raw OS error text.

use axum::Json;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde_json::json;

use crate::auth::devices::DeviceError;
use crate::auth::pairing::PairingError;
use crate::auth::sessions::LoginError;
use crate::filesystem::FsError;
use crate::filesystem::roots::ShareError;
use crate::transfers::uploads::UploadError;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("{0}")]
    BadRequest(String),

    /// No valid login. Clients should (re-)authenticate.
    #[error("authentication required")]
    Unauthorized,

    /// Logged in, but not allowed to do this.
    #[error("access denied")]
    Forbidden,

    #[error("not found")]
    NotFound,

    #[error("{0}")]
    Conflict(String),

    #[error("range not satisfiable")]
    RangeNotSatisfiable { file_size: u64 },

    #[error("too many requests; slow down")]
    TooManyRequests,

    /// E.g. an upload bigger than `max_upload_bytes`.
    #[error("{0}")]
    PayloadTooLarge(String),

    /// The host's disk doesn't have room for an upload.
    #[error("not enough free disk space on the host")]
    InsufficientStorage,

    /// Data didn't match the SHA-256 the client said it would have.
    #[error("{0}")]
    ChecksumMismatch(String),

    #[error("{0}")]
    Unavailable(String),

    /// Details are logged on the server, never sent to the client.
    #[error("internal error")]
    Internal,
}

impl AppError {
    /// HTTP status, plus a short machine-readable code for the JSON body.
    fn status_and_code(&self) -> (StatusCode, &'static str) {
        match self {
            AppError::BadRequest(_) => (StatusCode::BAD_REQUEST, "bad_request"),
            AppError::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized"),
            AppError::Forbidden => (StatusCode::FORBIDDEN, "forbidden"),
            AppError::NotFound => (StatusCode::NOT_FOUND, "not_found"),
            AppError::Conflict(_) => (StatusCode::CONFLICT, "conflict"),
            AppError::RangeNotSatisfiable { .. } => {
                (StatusCode::RANGE_NOT_SATISFIABLE, "range_not_satisfiable")
            }
            AppError::TooManyRequests => (StatusCode::TOO_MANY_REQUESTS, "too_many_requests"),
            AppError::PayloadTooLarge(_) => (StatusCode::PAYLOAD_TOO_LARGE, "payload_too_large"),
            AppError::InsufficientStorage => {
                (StatusCode::INSUFFICIENT_STORAGE, "insufficient_storage")
            }
            AppError::ChecksumMismatch(_) => {
                (StatusCode::UNPROCESSABLE_ENTITY, "checksum_mismatch")
            }
            AppError::Unavailable(_) => (StatusCode::SERVICE_UNAVAILABLE, "unavailable"),
            AppError::Internal => (StatusCode::INTERNAL_SERVER_ERROR, "internal"),
        }
    }
}

impl IntoResponse for AppError {
    /// Produces e.g. `{"error": {"code": "not_found", "message": "not found"}}`.
    fn into_response(self) -> Response {
        let (status, code) = self.status_and_code();
        let body = json!({ "error": { "code": code, "message": self.to_string() } });
        let mut response = (status, Json(body)).into_response();

        // HTTP says a 401 response should name the login scheme.
        if matches!(self, AppError::Unauthorized) {
            response
                .headers_mut()
                .insert(header::WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"));
        }

        // HTTP says a 416 response should tell the client the real file size.
        if let AppError::RangeNotSatisfiable { file_size } = self
            && let Ok(value) = HeaderValue::from_str(&format!("bytes */{file_size}"))
        {
            response.headers_mut().insert(header::CONTENT_RANGE, value);
        }

        response
    }
}

impl From<FsError> for AppError {
    fn from(error: FsError) -> Self {
        match error {
            FsError::NotFound => AppError::NotFound,
            FsError::NotADirectory => AppError::BadRequest("not a directory".into()),
            FsError::NotAFile => AppError::BadRequest("not a file".into()),
            FsError::InvalidPath(reason) => AppError::BadRequest(format!("invalid path: {reason}")),
            FsError::AccessDenied => AppError::Forbidden,
            FsError::TooManyEntries(limit) => {
                AppError::Conflict(format!("directory has more than {limit} entries"))
            }
            FsError::RootUnavailable => AppError::Unavailable("share is unavailable".into()),
            FsError::Io(io_error) => {
                tracing::error!(error = %io_error, "filesystem I/O error");
                AppError::Internal
            }
            // Only happens when nobody is waiting for the response, so the
            // client never actually sees this.
            FsError::Abandoned => AppError::Unavailable("request was abandoned".into()),
            FsError::NoFreeName => AppError::Conflict("no free file name".into()),
        }
    }
}

impl From<ShareError> for AppError {
    fn from(error: ShareError) -> Self {
        match error {
            ShareError::NotFound => AppError::NotFound,
            ShareError::Invalid(reason) => AppError::BadRequest(reason.into()),
            ShareError::Overlap => AppError::Conflict(error.to_string()),
            ShareError::Database(database_error) => {
                tracing::error!(error = %database_error, "share store error");
                AppError::Internal
            }
        }
    }
}

impl From<PairingError> for AppError {
    fn from(error: PairingError) -> Self {
        match error {
            PairingError::Failed => AppError::Forbidden,
            PairingError::Invalid(reason) => AppError::BadRequest(reason.into()),
            PairingError::NoSession
            | PairingError::Expired
            | PairingError::AlreadyClaimed
            | PairingError::WrongStep => AppError::Conflict(error.to_string()),
        }
    }
}

impl From<LoginError> for AppError {
    fn from(error: LoginError) -> Self {
        match error {
            LoginError::Failed => AppError::Unauthorized,
            LoginError::Busy => AppError::TooManyRequests,
        }
    }
}

impl From<DeviceError> for AppError {
    fn from(error: DeviceError) -> Self {
        match error {
            DeviceError::NotFound => AppError::NotFound,
            DeviceError::Invalid(reason) => AppError::BadRequest(reason.into()),
            DeviceError::Database(database_error) => {
                tracing::error!(error = %database_error, "device store error");
                AppError::Internal
            }
        }
    }
}

impl From<UploadError> for AppError {
    fn from(error: UploadError) -> Self {
        match error {
            UploadError::TooMany => AppError::TooManyRequests,
            UploadError::Database(database_error) => database_error.into(),
        }
    }
}

impl From<rusqlite::Error> for AppError {
    fn from(error: rusqlite::Error) -> Self {
        tracing::error!(%error, "database error");
        AppError::Internal
    }
}
