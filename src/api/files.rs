//! Browsing folders, file details, and downloads.

use axum::Json;
use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio_util::io::ReaderStream;

use crate::auth::{AuthenticatedDevice, Permission};
use crate::error::AppError;
use crate::filesystem::browser::{self, DirListing, ListOptions, SortKey, SortOrder};
use crate::filesystem::metadata::Entry;
use crate::filesystem::roots::Share;
use crate::filesystem::{FileId, RelPath, operations};
use crate::state::AppState;

/// Page size when the client doesn't specify one.
const DEFAULT_PAGE_SIZE: usize = 200;

/// How much of a file we hold in memory at once while streaming it.
const STREAM_CHUNK_BYTES: usize = 64 * 1024;

// ---------------------------------------------------------------------------
// Browsing
// ---------------------------------------------------------------------------

/// Query string of `GET /api/shares/{id}/files`,
/// e.g. `?path=/Documents&sort=size&order=desc&offset=0&limit=50`.
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

/// `GET /api/shares/{id}/files`
pub async fn browse(
    device: AuthenticatedDevice,
    State(state): State<AppState>,
    Path(share_id): Path<String>,
    Query(query): Query<BrowseQuery>,
) -> Result<Json<DirListing>, AppError> {
    device.require(Permission::Browse)?;

    let share = state.share(&share_id).await?;
    let path = RelPath::parse(query.path.as_deref().unwrap_or("/"))?;

    let config = state.config();
    let page_size = query
        .limit
        .unwrap_or(DEFAULT_PAGE_SIZE)
        .clamp(1, config.max_list_limit);

    let options = ListOptions {
        sort: query.sort,
        order: query.order,
        offset: query.offset,
        limit: page_size,
        max_entries: config.max_dir_entries,
    };

    let listing = browser::list_dir(share, path, options).await?;
    Ok(Json(listing))
}

// ---------------------------------------------------------------------------
// File details
// ---------------------------------------------------------------------------

/// `GET /api/files/{id}`
pub async fn metadata(
    device: AuthenticatedDevice,
    State(state): State<AppState>,
    Path(file_id): Path<String>,
) -> Result<Json<Entry>, AppError> {
    device.require(Permission::Browse)?;

    let (share, path) = look_up_file_id(&state, &file_id).await?;
    let entry = operations::stat(share, path).await?;
    Ok(Json(entry))
}

/// Turn a file ID from the URL into its share and path. Any bad or unknown
/// ID is a plain 404, so clients can't probe for details.
async fn look_up_file_id(state: &AppState, file_id: &str) -> Result<(Share, RelPath), AppError> {
    let file_id = FileId::decode(file_id).map_err(|_| AppError::NotFound)?;
    let share = state.share(&file_id.share_id).await?;
    Ok((share, file_id.path))
}

// ---------------------------------------------------------------------------
// Downloads
// ---------------------------------------------------------------------------

/// `GET /api/files/{id}/download`
///
/// Streams the file in small chunks, so memory use stays flat no matter
/// how big the file is. Supports a single `Range` (for resuming). If the
/// client disconnects, the response is dropped, which closes the file and
/// stops reading.
pub async fn download(
    device: AuthenticatedDevice,
    State(state): State<AppState>,
    Path(file_id): Path<String>,
    request_headers: HeaderMap,
) -> Result<Response, AppError> {
    device.require(Permission::Read)?;

    let (share, path) = look_up_file_id(&state, &file_id).await?;
    let opened = operations::open_file(share, path).await?;
    let file_size = opened.size;

    // Work out which bytes to send.
    let range_header = request_headers
        .get(header::RANGE)
        .and_then(|value| value.to_str().ok());

    let (status, first_byte, byte_count) = match parse_range(range_header, file_size) {
        RangeRequest::WholeFile => (StatusCode::OK, 0, file_size),
        RangeRequest::Partial { start, end } => {
            (StatusCode::PARTIAL_CONTENT, start, end - start + 1)
        }
        RangeRequest::Unsatisfiable => {
            return Err(AppError::RangeNotSatisfiable { file_size });
        }
    };

    // Build a body that streams exactly those bytes.
    let mut file = tokio::fs::File::from_std(opened.file);
    if first_byte > 0 {
        file.seek(std::io::SeekFrom::Start(first_byte))
            .await
            .map_err(|error| {
                tracing::error!(%error, "seek failed");
                AppError::Internal
            })?;
    }
    let limited_reader = file.take(byte_count);
    let stream = ReaderStream::with_capacity(limited_reader, STREAM_CHUNK_BYTES);
    let mut response = (status, Body::from_stream(stream)).into_response();

    // Response headers.
    let headers = response.headers_mut();
    let mime_type = opened
        .entry
        .mime
        .as_deref()
        .unwrap_or("application/octet-stream");

    headers.insert(header::CONTENT_TYPE, header_value(mime_type));
    headers.insert(header::CONTENT_LENGTH, HeaderValue::from(byte_count));
    headers.insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    // Stop browsers from second-guessing the content type.
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        header::CONTENT_DISPOSITION,
        header_value(&content_disposition(&opened.entry.name)),
    );

    if status == StatusCode::PARTIAL_CONTENT {
        let last_byte = first_byte + byte_count - 1;
        let content_range = format!("bytes {first_byte}-{last_byte}/{file_size}");
        headers.insert(header::CONTENT_RANGE, header_value(&content_range));
    }

    Ok(response)
}

/// Make a header value, falling back to a harmless default if the text
/// contains characters that aren't allowed in headers.
fn header_value(text: &str) -> HeaderValue {
    HeaderValue::from_str(text)
        .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream"))
}

/// Which part of a file the client asked for.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum RangeRequest {
    WholeFile,
    /// Byte positions, both inclusive.
    Partial {
        start: u64,
        end: u64,
    },
    /// The requested range lies entirely past the end of the file.
    Unsatisfiable,
}

/// Parse an HTTP `Range` header such as `bytes=0-499`, `bytes=500-` or
/// `bytes=-500` (the last 500 bytes).
///
/// We only support one range at a time. Anything we don't understand,
/// including multiple ranges, means "send the whole file", which the HTTP
/// spec (RFC 9110 §14) explicitly allows.
pub(crate) fn parse_range(header: Option<&str>, file_size: u64) -> RangeRequest {
    use RangeRequest::*;

    let Some(spec) = header.and_then(|value| value.trim().strip_prefix("bytes=")) else {
        return WholeFile;
    };
    if spec.contains(',') {
        return WholeFile; // Multiple ranges.
    }
    let Some((start_text, end_text)) = spec.trim().split_once('-') else {
        return WholeFile;
    };
    let (start_text, end_text) = (start_text.trim(), end_text.trim());

    // "bytes=-N": the last N bytes.
    if start_text.is_empty() {
        let Ok(suffix_length) = end_text.parse::<u64>() else {
            return WholeFile;
        };
        if suffix_length == 0 || file_size == 0 {
            return Unsatisfiable;
        }
        return Partial {
            start: file_size.saturating_sub(suffix_length),
            end: file_size - 1,
        };
    }

    // "bytes=START-" or "bytes=START-END".
    let Ok(start) = start_text.parse::<u64>() else {
        return WholeFile;
    };
    let requested_end = if end_text.is_empty() {
        u64::MAX // Open-ended: to the end of the file.
    } else {
        match end_text.parse::<u64>() {
            Ok(end) if end >= start => end,
            _ => return WholeFile,
        }
    };

    if start >= file_size {
        return Unsatisfiable;
    }
    Partial {
        start,
        end: requested_end.min(file_size - 1),
    }
}

/// The `Content-Disposition` header that makes browsers save the file under
/// its real name.
///
/// It carries the name twice: a plain-ASCII copy for old clients, and a
/// percent-encoded UTF-8 copy (`filename*=`, RFC 5987) for everyone else.
fn content_disposition(file_name: &str) -> String {
    let ascii_name: String = file_name
        .chars()
        .map(|c| if is_safe_in_quoted_name(c) { c } else { '_' })
        .collect();

    let mut encoded_name = String::new();
    for byte in file_name.bytes() {
        if byte.is_ascii_alphanumeric() || b"!#$&+-.^_`|~".contains(&byte) {
            encoded_name.push(byte as char);
        } else {
            encoded_name.push_str(&format!("%{byte:02X}"));
        }
    }

    format!("attachment; filename=\"{ascii_name}\"; filename*=UTF-8''{encoded_name}")
}

/// Can this character appear as-is inside `filename="…"`?
fn is_safe_in_quoted_name(c: char) -> bool {
    let printable_ascii = c.is_ascii_graphic() || c == ' ';
    let breaks_quoting = c == '"' || c == '\\';
    printable_ascii && !breaks_quoting
}

#[cfg(test)]
mod tests {
    use super::*;
    use RangeRequest::*;

    #[test]
    fn parses_range_headers() {
        let size = 100;
        assert_eq!(parse_range(None, size), WholeFile);
        assert_eq!(
            parse_range(Some("bytes=0-9"), size),
            Partial { start: 0, end: 9 }
        );
        assert_eq!(
            parse_range(Some("bytes=90-"), size),
            Partial { start: 90, end: 99 }
        );
        assert_eq!(
            parse_range(Some("bytes=90-500"), size),
            Partial { start: 90, end: 99 }
        );
        assert_eq!(
            parse_range(Some("bytes=-10"), size),
            Partial { start: 90, end: 99 }
        );
        assert_eq!(
            parse_range(Some("bytes=-500"), size),
            Partial { start: 0, end: 99 }
        );
    }

    #[test]
    fn out_of_bounds_ranges_are_unsatisfiable() {
        assert_eq!(parse_range(Some("bytes=100-"), 100), Unsatisfiable);
        assert_eq!(parse_range(Some("bytes=-0"), 100), Unsatisfiable);
        assert_eq!(parse_range(Some("bytes=0-0"), 0), Unsatisfiable);
    }

    #[test]
    fn unsupported_or_malformed_ranges_send_whole_file() {
        assert_eq!(parse_range(Some("bytes=5-1"), 100), WholeFile);
        assert_eq!(parse_range(Some("bytes=0-1,5-6"), 100), WholeFile);
        assert_eq!(parse_range(Some("items=0-1"), 100), WholeFile);
        assert_eq!(parse_range(Some("bytes=abc"), 100), WholeFile);
    }

    #[test]
    fn content_disposition_escapes_awkward_names() {
        let header = content_disposition("a \"b\"\nü.txt");
        assert!(!header.contains('\n'));
        assert!(header.contains("filename*=UTF-8''a%20%22b%22%0A%C3%BC.txt"));
    }
}
