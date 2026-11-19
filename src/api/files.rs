//! Browsing folders, file details, downloads and SHA-256 hashes.

use axum::Json;
use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use tokio::io::AsyncSeekExt;

use crate::auth::{AuthenticatedDevice, Permission};
use crate::error::AppError;
use crate::filesystem::browser::{self, DirListing, ListOptions, SortKey, SortOrder};
use crate::filesystem::metadata::Entry;
use crate::filesystem::roots::Share;
use crate::filesystem::{FileId, RelPath, operations};
use crate::state::AppState;
use crate::transfers::hashing;
use crate::transfers::stream::{DownloadSource, download_stream};
use crate::transfers::version::FileVersion;

/// Page size when the client doesn't specify one.
const DEFAULT_PAGE_SIZE: usize = 200;

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
/// Streams the file (see [`crate::transfers::stream`]). Supports a single
/// `Range` for resuming, guarded by `If-Range` so a resume never mixes two
/// versions of the file. Counts towards the device's download limit.
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
    let version = FileVersion::of(&opened.file).map_err(internal_error)?;
    let etag = version.etag();

    // Work out which bytes to send.
    let range_header = header_text(&request_headers, header::RANGE);
    let if_range_header = header_text(&request_headers, header::IF_RANGE);
    let range = if resume_is_safe(if_range_header, &etag) {
        parse_range(range_header, file_size)
    } else {
        RangeRequest::WholeFile // The file changed: start over.
    };

    let (status, first_byte, byte_count) = match range {
        RangeRequest::WholeFile => (StatusCode::OK, 0, file_size),
        RangeRequest::Partial { start, end } => {
            (StatusCode::PARTIAL_CONTENT, start, end - start + 1)
        }
        RangeRequest::Unsatisfiable => {
            return Err(AppError::RangeNotSatisfiable { file_size });
        }
    };

    let slot = state
        .transfer_limiter()
        .try_start(&device.id)
        .ok_or(AppError::TooManyRequests)?;

    // Build a body that streams exactly those bytes.
    let watcher = opened.file.try_clone().map_err(internal_error)?;
    let mut reader = tokio::fs::File::from_std(opened.file);
    if first_byte > 0 {
        reader
            .seek(std::io::SeekFrom::Start(first_byte))
            .await
            .map_err(internal_error)?;
    }
    let source = DownloadSource {
        reader,
        byte_count,
        watcher,
        version,
        slot,
    };
    let body = Body::from_stream(download_stream(source));
    let mut response = (status, body).into_response();

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
    headers.insert(header::ETAG, header_value(&etag));
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

/// May we honour a `Range` request? Yes, unless the client sent `If-Range`
/// with an ETag that no longer matches (the file changed since its first
/// request). Dates in `If-Range` aren't supported, so they count as a
/// mismatch: sending the whole file is always safe.
fn resume_is_safe(if_range: Option<&str>, current_etag: &str) -> bool {
    match if_range {
        None => true,
        Some(sent_etag) => sent_etag.trim() == current_etag,
    }
}

fn header_text(headers: &HeaderMap, name: header::HeaderName) -> Option<&str> {
    headers.get(name).and_then(|value| value.to_str().ok())
}

// ---------------------------------------------------------------------------
// SHA-256
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct HashResponse {
    /// Lowercase hex.
    sha256: String,
    size: u64,
    /// The same value as the download's `ETag` header for this version of
    /// the file, so the client can tell the hash matches what it downloaded.
    etag: String,
}

/// `GET /api/files/{id}/hash`
///
/// Reads the whole file, so it can take a while for big files (it's exempt
/// from the request timeout). The result is cached until the file changes.
/// Counts towards the device's download limit while it runs.
pub async fn hash(
    device: AuthenticatedDevice,
    State(state): State<AppState>,
    Path(file_id): Path<String>,
) -> Result<Json<HashResponse>, AppError> {
    device.require(Permission::Read)?;

    let (share, path) = look_up_file_id(&state, &file_id).await?;
    let opened = operations::open_file(share, path).await?;
    let version = FileVersion::of(&opened.file).map_err(internal_error)?;
    let cache_key = opened.entry.id.clone();

    let sha256 = match state.hash_cache().get(&cache_key, version) {
        Some(cached) => cached,
        None => {
            let _slot = state
                .transfer_limiter()
                .try_start(&device.id)
                .ok_or(AppError::TooManyRequests)?;

            let watcher = opened.file.try_clone().map_err(internal_error)?;
            let reader = tokio::fs::File::from_std(opened.file);
            let sha256 = hashing::sha256_of(reader).await.map_err(internal_error)?;

            if FileVersion::of(&watcher).map_err(internal_error)? != version {
                return Err(AppError::Conflict(
                    "file changed while it was being hashed; try again".into(),
                ));
            }
            state.hash_cache().insert(&cache_key, version, sha256);
            sha256
        }
    };

    Ok(Json(HashResponse {
        sha256: hashing::to_hex(&sha256),
        size: version.size,
        etag: version.etag(),
    }))
}

/// Log an unexpected I/O error and hide its details from the client.
fn internal_error(error: std::io::Error) -> AppError {
    tracing::error!(%error, "file I/O error");
    AppError::Internal
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
    fn resume_needs_a_matching_etag() {
        let etag = "\"abc\"";
        assert!(resume_is_safe(None, etag));
        assert!(resume_is_safe(Some("\"abc\""), etag));
        assert!(!resume_is_safe(Some("\"old\""), etag));
        assert!(!resume_is_safe(Some("Wed, 21 Oct 2015 07:28:00 GMT"), etag));
    }

    #[test]
    fn content_disposition_escapes_awkward_names() {
        let header = content_disposition("a \"b\"\nü.txt");
        assert!(!header.contains('\n'));
        assert!(header.contains("filename*=UTF-8''a%20%22b%22%0A%C3%BC.txt"));
    }
}
