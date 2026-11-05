//! HTTP-facing error type. Messages are safe to show clients: they never
//! include host filesystem paths or OS error strings.

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
    RangeNotSatisfiable { size: u64 },
    #[error("{0}")]
    Unavailable(String),
    #[error("internal error")]
    Internal,
}

impl AppError {
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
    fn into_response(self) -> Response {
        let (status, code) = self.status_and_code();
        let body = Json(json!({ "error": { "code": code, "message": self.to_string() } }));
        let mut res = (status, body).into_response();
        if let AppError::RangeNotSatisfiable { size } = self
            && let Ok(v) = HeaderValue::from_str(&format!("bytes */{size}"))
        {
            res.headers_mut().insert(header::CONTENT_RANGE, v);
        }
        res
    }
}

impl From<FsError> for AppError {
    fn from(e: FsError) -> Self {
        match e {
            FsError::NotFound => AppError::NotFound,
            FsError::NotADirectory => AppError::BadRequest("not a directory".into()),
            FsError::NotAFile => AppError::BadRequest("not a file".into()),
            FsError::InvalidPath(why) => AppError::BadRequest(format!("invalid path: {why}")),
            FsError::AccessDenied => AppError::Forbidden,
            FsError::TooManyEntries(n) => {
                AppError::Conflict(format!("directory has more than {n} entries"))
            }
            FsError::RootUnavailable => AppError::Unavailable("share is unavailable".into()),
            FsError::Io(err) => {
                tracing::error!(error = %err, "filesystem I/O error");
                AppError::Internal
            }
        }
    }
}

impl From<ShareError> for AppError {
    fn from(e: ShareError) -> Self {
        match e {
            ShareError::NotFound => AppError::NotFound,
            ShareError::Invalid(why) => AppError::BadRequest(why.into()),
            ShareError::Overlap => AppError::Conflict(e.to_string()),
            ShareError::UnsupportedVersion(_) | ShareError::Io(_) => {
                tracing::error!(error = %e, "share store error");
                AppError::Internal
            }
        }
    }
}
