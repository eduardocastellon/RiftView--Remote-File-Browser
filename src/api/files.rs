//! Browsing, metadata and streaming downloads.

use axum::Json;
use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio_util::io::ReaderStream;

use crate::error::AppError;
use crate::filesystem::browser::{self, DirListing, ListOptions, SortKey, SortOrder};
use crate::filesystem::metadata::Entry;
use crate::filesystem::{FileId, RelPath, operations};
use crate::state::AppState;

const DEFAULT_LIST_LIMIT: usize = 200;
const STREAM_BUFFER_BYTES: usize = 64 * 1024;

#[derive(Deserialize)]
pub struct BrowseQuery {
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    sort: SortKey,
    #[serde(default)]
    order: SortOrder,
    #[serde(default)]
    offset: usize,
    #[serde(default)]
    limit: Option<usize>,
}

// TODO(phase 4): require Browse permission for the calling device.
pub async fn browse(
    State(state): State<AppState>,
    Path(share_id): Path<String>,
    Query(q): Query<BrowseQuery>,
) -> Result<Json<DirListing>, AppError> {
    let share = state.share(&share_id).await?;
    let rel = RelPath::parse(q.path.as_deref().unwrap_or("/"))?;
    let cfg = state.config();
    let limit = q.limit.unwrap_or(DEFAULT_LIST_LIMIT).clamp(1, cfg.max_list_limit);
    let opts = ListOptions {
        sort: q.sort,
        order: q.order,
        offset: q.offset,
        limit,
        max_entries: cfg.max_dir_entries,
    };
    Ok(Json(browser::list_dir(share, rel, opts).await?))
}

async fn resolve_id(state: &AppState, raw: &str) -> Result<(crate::filesystem::roots::Share, RelPath), AppError> {
    let id = FileId::decode(raw).map_err(|_| AppError::NotFound)?;
    let share = state.share(&id.share_id).await?;
    Ok((share, id.path))
}

// TODO(phase 4): require Browse permission.
pub async fn metadata(State(state): State<AppState>, Path(id): Path<String>) -> Result<Json<Entry>, AppError> {
    let (share, rel) = resolve_id(&state, &id).await?;
    Ok(Json(operations::stat(share, rel).await?))
}

/// Stream a file with single-range `Range` support. Memory use is bounded by
/// the stream buffer; dropping the response (client disconnect) drops the
/// file handle and stops I/O.
// TODO(phase 4): require Read permission.
pub async fn download(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let (share, rel) = resolve_id(&state, &id).await?;
    let opened = operations::open_file(share, rel).await?;
    let size = opened.size;

    let range = headers.get(header::RANGE).and_then(|v| v.to_str().ok());
    let (status, start, len) = match parse_range(range, size) {
        RangeReq::Full => (StatusCode::OK, 0, size),
        RangeReq::Partial { start, end } => (StatusCode::PARTIAL_CONTENT, start, end - start + 1),
        RangeReq::Unsatisfiable => return Err(AppError::RangeNotSatisfiable { size }),
    };

    let mut file = tokio::fs::File::from_std(opened.file);
    if start > 0 {
        file.seek(std::io::SeekFrom::Start(start)).await.map_err(|e| {
            tracing::error!(error = %e, "seek failed");
            AppError::Internal
        })?;
    }
    let body = Body::from_stream(ReaderStream::with_capacity(file.take(len), STREAM_BUFFER_BYTES));

    let mut res = (status, body).into_response();
    let h = res.headers_mut();
    let mime = opened.entry.mime.as_deref().unwrap_or("application/octet-stream");
    h.insert(header::CONTENT_TYPE, hv(mime));
    h.insert(header::CONTENT_LENGTH, HeaderValue::from(len));
    h.insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    h.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    h.insert(header::CONTENT_DISPOSITION, hv(&content_disposition(&opened.entry.name)));
    if status == StatusCode::PARTIAL_CONTENT {
        h.insert(header::CONTENT_RANGE, hv(&format!("bytes {start}-{}/{size}", start + len - 1)));
    }
    Ok(res)
}

fn hv(s: &str) -> HeaderValue {
    HeaderValue::from_str(s).unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream"))
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum RangeReq {
    Full,
    /// Inclusive byte range.
    Partial { start: u64, end: u64 },
    Unsatisfiable,
}

/// Parse a `Range` header (RFC 9110 §14). Only a single byte range is
/// honoured; anything malformed or multi-range falls back to the full body,
/// which the RFC permits.
pub(crate) fn parse_range(header: Option<&str>, size: u64) -> RangeReq {
    let Some(spec) = header.and_then(|h| h.trim().strip_prefix("bytes=")) else {
        return RangeReq::Full;
    };
    if spec.contains(',') {
        return RangeReq::Full;
    }
    let Some((a, b)) = spec.trim().split_once('-') else { return RangeReq::Full };
    let (a, b) = (a.trim(), b.trim());

    if a.is_empty() {
        // Suffix range: last N bytes.
        let Ok(n) = b.parse::<u64>() else { return RangeReq::Full };
        if n == 0 || size == 0 {
            return RangeReq::Unsatisfiable;
        }
        return RangeReq::Partial { start: size.saturating_sub(n), end: size - 1 };
    }

    let Ok(start) = a.parse::<u64>() else { return RangeReq::Full };
    let end = if b.is_empty() {
        u64::MAX
    } else {
        match b.parse::<u64>() {
            Ok(e) if e >= start => e,
            _ => return RangeReq::Full,
        }
    };
    if start >= size {
        return RangeReq::Unsatisfiable;
    }
    RangeReq::Partial { start, end: end.min(size - 1) }
}

/// `attachment` with an ASCII fallback and an RFC 5987 UTF-8 filename.
fn content_disposition(name: &str) -> String {
    let fallback: String = name
        .chars()
        .map(|c| if c.is_ascii_graphic() && c != '"' && c != '\\' || c == ' ' { c } else { '_' })
        .collect();
    let mut encoded = String::new();
    for b in name.bytes() {
        if b.is_ascii_alphanumeric() || b"!#$&+-.^_`|~".contains(&b) {
            encoded.push(b as char);
        } else {
            encoded.push_str(&format!("%{b:02X}"));
        }
    }
    format!("attachment; filename=\"{fallback}\"; filename*=UTF-8''{encoded}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges() {
        use RangeReq::*;
        assert_eq!(parse_range(None, 100), Full);
        assert_eq!(parse_range(Some("bytes=0-9"), 100), Partial { start: 0, end: 9 });
        assert_eq!(parse_range(Some("bytes=90-"), 100), Partial { start: 90, end: 99 });
        assert_eq!(parse_range(Some("bytes=90-500"), 100), Partial { start: 90, end: 99 });
        assert_eq!(parse_range(Some("bytes=-10"), 100), Partial { start: 90, end: 99 });
        assert_eq!(parse_range(Some("bytes=-500"), 100), Partial { start: 0, end: 99 });
        assert_eq!(parse_range(Some("bytes=100-"), 100), Unsatisfiable);
        assert_eq!(parse_range(Some("bytes=-0"), 100), Unsatisfiable);
        assert_eq!(parse_range(Some("bytes=0-0"), 0), Unsatisfiable);
        assert_eq!(parse_range(Some("bytes=5-1"), 100), Full);
        assert_eq!(parse_range(Some("bytes=0-1,5-6"), 100), Full);
        assert_eq!(parse_range(Some("items=0-1"), 100), Full);
        assert_eq!(parse_range(Some("bytes=abc"), 100), Full);
    }

    #[test]
    fn disposition_escapes() {
        let d = content_disposition("a \"b\"\nü.txt");
        assert!(!d.contains('\n'));
        assert!(d.contains("filename*=UTF-8''a%20%22b%22%0A%C3%BC.txt"));
    }
}
