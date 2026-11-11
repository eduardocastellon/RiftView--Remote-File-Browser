//! Errors as clients see them.
//!
//! Every message here is safe to send over the network: none of them
//! include real host paths or raw OS error text.

use axum::Json;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde_json::json;

use crate::filesystem::FsError;
use crate::filesystem::roots::ShareError;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("{0}")]
    BadRequest(String),

    #[error("not found")]
    NotFound,

    #[error("access denied")]
    Forbidden,

    #[error("{0}")]
    Conflict(String),

    #[error("range not satisfiable")]
    RangeNotSatisfiable { file_size: u64 },

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
            AppError::NotFound => (StatusCode::NOT_FOUND, "not_found"),
            AppError::Forbidden => (StatusCode::FORBIDDEN, "forbidden"),
            AppError::Conflict(_) => (StatusCode::CONFLICT, "conflict"),
            AppError::RangeNotSatisfiable { .. } => {
                (StatusCode::RANGE_NOT_SATISFIABLE, "range_not_satisfiable")
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
        }
    }
}

impl From<ShareError> for AppError {
    fn from(error: ShareError) -> Self {
        match error {
            ShareError::NotFound => AppError::NotFound,
            ShareError::Invalid(reason) => AppError::BadRequest(reason.into()),
            ShareError::Overlap => AppError::Conflict(error.to_string()),
            ShareError::UnsupportedVersion(_) | ShareError::Io(_) => {
                tracing::error!(%error, "share store error");
                AppError::Internal
            }
        }
    }
}
