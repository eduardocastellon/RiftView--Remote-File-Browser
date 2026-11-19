//! End-to-end tests of uploads (Phase 6).

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::{TestHost, body_bytes, entry_names};
use riftview_api::config::Config;
use riftview_api::filesystem::roots::now_ms;
use riftview_api::filesystem::{FileId, RelPath};
use riftview_api::transfers::cleanup::remove_expired_uploads;
use riftview_api::transfers::hashing::{sha256_of_bytes, to_hex};
use riftview_api::transfers::uploads::CHUNK_BYTES;
use serde_json::{Value, json};

const CHUNK: usize = CHUNK_BYTES as usize;

// --- Helpers -----------------------------------------------------------------

/// A test host whose share is writable (shares are read-only by default).
async fn writable_host() -> TestHost {
    host_with_config(Config {
        auth_rate_limit_per_minute: 1000,
        ..Config::default()
    })
    .await
}

async fn host_with_config(config: Config) -> TestHost {
    let host = TestHost::with_config(config).await;
    let (status, body) = host
        .admin_json(
            "PATCH",
            &format!("/admin/shares/{}", host.share_id),
            Some(json!({ "writable": true })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    host
}

fn sha256_hex(data: &[u8]) -> String {
    to_hex(&sha256_of_bytes(data))
}

/// Bytes that differ from chunk to chunk, so misplaced chunks are caught.
fn sample_data(length: usize) -> Vec<u8> {
    (0..length).map(|i| (i % 251) as u8).collect()
}

async fn json_of(response: axum::http::Response<Body>) -> (StatusCode, Value) {
    let status = response.status();
    let bytes = body_bytes(response).await;
    let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, value)
}

async fn start_upload(
    host: &TestHost,
    folder: &str,
    name: &str,
    data: &[u8],
) -> (StatusCode, Value) {
    let request = json!({
        "share_id": host.share_id,
        "path": folder,
        "name": name,
        "size": data.len(),
        "sha256": sha256_hex(data),
    });
    host.send_json("POST", "/api/transfers", Some(request))
        .await
}

async fn start_ok(host: &TestHost, folder: &str, name: &str, data: &[u8]) -> String {
    let (status, body) = start_upload(host, folder, name, data).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    body["id"].as_str().unwrap().to_owned()
}

async fn put_chunk_with_hash(
    host: &TestHost,
    upload_id: &str,
    index: usize,
    bytes: &[u8],
    hash: &str,
) -> (StatusCode, Value) {
    let request = Request::put(format!("/api/transfers/{upload_id}/chunks/{index}"))
        .header("authorization", format!("Bearer {}", host.token))
        .header("x-chunk-sha256", hash)
        .body(Body::from(bytes.to_vec()))
        .unwrap();
    json_of(host.send(request).await).await
}

async fn put_chunk(
    host: &TestHost,
    upload_id: &str,
    index: usize,
    data: &[u8],
) -> (StatusCode, Value) {
    let chunk = chunk_of(data, index);
    put_chunk_with_hash(host, upload_id, index, chunk, &sha256_hex(chunk)).await
}

fn chunk_of(data: &[u8], index: usize) -> &[u8] {
    let start = index * CHUNK;
    &data[start..(start + CHUNK).min(data.len())]
}

fn chunk_count(data: &[u8]) -> usize {
    data.len().div_ceil(CHUNK)
}

async fn send_all_chunks(host: &TestHost, upload_id: &str, data: &[u8]) {
    for index in 0..chunk_count(data) {
        let (status, body) = put_chunk(host, upload_id, index, data).await;
        assert_eq!(status, StatusCode::OK, "chunk {index}: {body}");
    }
}

async fn complete(host: &TestHost, upload_id: &str) -> (StatusCode, Value) {
    host.send_json_as(
        Some(&host.token),
        "POST",
        &format!("/api/transfers/{upload_id}/complete"),
        None,
    )
    .await
}

/// Upload `data` start to finish and return the `complete` response body.
async fn upload_file(host: &TestHost, folder: &str, name: &str, data: &[u8]) -> Value {
    let upload_id = start_ok(host, folder, name, data).await;
    send_all_chunks(host, &upload_id, data).await;
    let (status, body) = complete(host, &upload_id).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    body
}

/// Names of the files actually on disk in a folder of the share,
/// including hidden ones.
fn names_on_disk(host: &TestHost, folder: &str) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(host.shared_path().join(folder))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    names
}

fn partial_files(host: &TestHost, folder: &str) -> Vec<String> {
    names_on_disk(host, folder)
        .into_iter()
        .filter(|name| name.starts_with(".riftview-upload-"))
        .collect()
}

// --- The happy path ------------------------------------------------------------

#[tokio::test]
async fn upload_in_any_order_then_finish() {
    let host = writable_host().await;
    let data = sample_data(2 * CHUNK + 1000); // Three chunks; the last is short.

    let (status, started) = start_upload(&host, "/Documents", "video.mov", &data).await;
    assert_eq!(status, StatusCode::CREATED, "{started}");
    assert_eq!(started["chunk_size"], CHUNK_BYTES);
    assert_eq!(started["chunk_count"], 3);
    assert_eq!(started["missing_chunks"], json!([0, 1, 2]));
    let upload_id = started["id"].as_str().unwrap();

    // Chunks out of order; progress shows what's missing.
    put_chunk(&host, upload_id, 2, &data).await;
    put_chunk(&host, upload_id, 0, &data).await;
    let (status, progress) = host
        .send_json_as(
            Some(&host.token),
            "GET",
            &format!("/api/transfers/{upload_id}"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(progress["received_count"], 2);
    assert_eq!(progress["missing_chunks"], json!([1]));

    // The partial file exists on disk but devices never see it.
    assert_eq!(partial_files(&host, "Documents").len(), 1);
    let (_, listing) = host.browse("/Documents").await;
    assert!(
        !entry_names(&listing)
            .iter()
            .any(|name| name.contains("riftview"))
    );

    let (_, last) = put_chunk(&host, upload_id, 1, &data).await;
    assert_eq!(last["received_count"], 3);

    let (status, finished) = complete(&host, upload_id).await;
    assert_eq!(status, StatusCode::CREATED, "{finished}");
    assert_eq!(finished["file"]["name"], "video.mov");
    assert_eq!(finished["file"]["size"], data.len());
    assert_eq!(finished["renamed"], false);

    // The real file has exactly the right bytes, and no partial file is left.
    let saved = std::fs::read(host.shared_path().join("Documents/video.mov")).unwrap();
    assert!(saved == data, "saved file differs from what was uploaded");
    assert!(partial_files(&host, "Documents").is_empty());

    // The upload is gone, and the new file downloads like any other.
    let (status, _) = host
        .send_json_as(
            Some(&host.token),
            "GET",
            &format!("/api/transfers/{upload_id}"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let file_id = finished["file"]["id"].as_str().unwrap();
    let (status, hash) = host.get_json(&format!("/api/files/{file_id}/hash")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(hash["sha256"], sha256_hex(&data));
}

#[tokio::test]
async fn empty_file_needs_no_chunks() {
    let host = writable_host().await;
    let upload_id = start_ok(&host, "/", "empty.txt", b"").await;

    let (status, body) = complete(&host, &upload_id).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(
        std::fs::read(host.shared_path().join("empty.txt")).unwrap(),
        b""
    );
}

#[tokio::test]
async fn name_clashes_are_renamed() {
    let host = writable_host().await;

    // Documents/notes.txt already exists.
    let first = upload_file(&host, "/Documents", "notes.txt", b"one").await;
    assert_eq!(first["file"]["name"], "notes (1).txt");
    assert_eq!(first["renamed"], true);

    let second = upload_file(&host, "/Documents", "notes.txt", b"two").await;
    assert_eq!(second["file"]["name"], "notes (2).txt");

    // The original is untouched.
    let original = std::fs::read(host.shared_path().join("Documents/notes.txt")).unwrap();
    assert_eq!(original, b"hello world");
}

#[tokio::test]
async fn resent_chunk_is_harmless() {
    let host = writable_host().await;
    let data = sample_data(1000);
    let upload_id = start_ok(&host, "/", "a.bin", &data).await;

    put_chunk(&host, &upload_id, 0, &data).await;
    let (status, body) = put_chunk(&host, &upload_id, 0, &data).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["received_count"], 1);

    let (status, _) = complete(&host, &upload_id).await;
    assert_eq!(status, StatusCode::CREATED);
}

// --- Integrity ---------------------------------------------------------------------

#[tokio::test]
async fn corrupt_chunk_is_rejected_and_can_be_resent() {
    let host = writable_host().await;
    let data = sample_data(1000);
    let upload_id = start_ok(&host, "/", "a.bin", &data).await;

    let mut corrupted = data.clone();
    corrupted[10] ^= 0xFF;
    let (status, body) =
        put_chunk_with_hash(&host, &upload_id, 0, &corrupted, &sha256_hex(&data)).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["error"]["code"], "checksum_mismatch");

    let (status, _) = put_chunk(&host, &upload_id, 0, &data).await;
    assert_eq!(status, StatusCode::OK, "a good resend is accepted");
}

#[tokio::test]
async fn chunk_of_the_wrong_size_or_index_is_rejected() {
    let host = writable_host().await;
    let data = sample_data(1000);
    let upload_id = start_ok(&host, "/", "a.bin", &data).await;

    let short = &data[..999];
    let (status, _) = put_chunk_with_hash(&host, &upload_id, 0, short, &sha256_hex(short)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, _) = put_chunk_with_hash(&host, &upload_id, 1, &data, &sha256_hex(&data)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "only chunk 0 exists");

    let (status, _) = put_chunk_with_hash(&host, &upload_id, 0, &data, "not-a-hash").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn whole_file_mismatch_discards_the_upload() {
    let host = writable_host().await;
    let data = sample_data(1000);

    // Claim a different whole-file hash than the data will have.
    let request = json!({
        "share_id": host.share_id, "path": "/", "name": "a.bin",
        "size": data.len(), "sha256": sha256_hex(b"something else"),
    });
    let (status, started) = host
        .send_json("POST", "/api/transfers", Some(request))
        .await;
    assert_eq!(status, StatusCode::CREATED);
    let upload_id = started["id"].as_str().unwrap();

    put_chunk(&host, upload_id, 0, &data).await;
    let (status, body) = complete(&host, upload_id).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["error"]["code"], "checksum_mismatch");

    assert!(partial_files(&host, "").is_empty(), "partial file deleted");
    assert!(!host.shared_path().join("a.bin").exists());
    let (status, _) = host
        .send_json_as(
            Some(&host.token),
            "GET",
            &format!("/api/transfers/{upload_id}"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn cannot_finish_with_missing_chunks() {
    let host = writable_host().await;
    let data = sample_data(CHUNK + 10);
    let upload_id = start_ok(&host, "/", "a.bin", &data).await;
    put_chunk(&host, &upload_id, 0, &data).await;

    let (status, body) = complete(&host, &upload_id).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("1 chunk")
    );
}

// --- Rules -----------------------------------------------------------------------------

#[tokio::test]
async fn uploads_need_permission_and_a_writable_share() {
    // Shares are read-only by default.
    let host = TestHost::new().await;
    let (status, _) = start_upload(&host, "/", "a.txt", b"x").await;
    assert_eq!(status, StatusCode::FORBIDDEN, "read-only share");

    let host = writable_host().await;
    let reader = host.pair_device("Reader", &["browse", "read"]).await;
    let token = host.log_in(&reader).await.unwrap();
    let request = json!({
        "share_id": host.share_id, "path": "/", "name": "a.txt",
        "size": 1, "sha256": sha256_hex(b"x"),
    });
    let (status, _) = host
        .send_json_as(Some(&token), "POST", "/api/transfers", Some(request))
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "no upload permission");
}

#[tokio::test]
async fn bad_names_and_folders_are_refused() {
    let host = writable_host().await;

    for name in ["../escape.txt", "a/b.txt", "", ".riftview-upload-x.part"] {
        let (status, body) = start_upload(&host, "/", name, b"x").await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{name:?}: {body}");
    }

    let (status, _) = start_upload(&host, "/../outside", "a.txt", b"x").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, _) = start_upload(&host, "/Nope", "a.txt", b"x").await;
    assert_eq!(status, StatusCode::NOT_FOUND, "folder must exist");

    let (status, _) = start_upload(&host, "/report.pdf", "a.txt", b"x").await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "target is a file, not a folder"
    );
}

#[tokio::test]
async fn size_limit_and_disk_space_are_checked() {
    let host = host_with_config(Config {
        auth_rate_limit_per_minute: 1000,
        max_upload_bytes: 100,
        ..Config::default()
    })
    .await;
    let (status, body) = start_upload(&host, "/", "big.bin", &[0; 101]).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(body["error"]["code"], "payload_too_large");

    // No size limit, but far more than any test machine's disk.
    let host = host_with_config(Config {
        auth_rate_limit_per_minute: 1000,
        max_upload_bytes: u64::MAX,
        ..Config::default()
    })
    .await;
    let one_exabyte: u64 = 1 << 60;
    let request = json!({
        "share_id": host.share_id, "path": "/", "name": "huge.bin",
        "size": one_exabyte, "sha256": sha256_hex(b""),
    });
    let (status, body) = host
        .send_json("POST", "/api/transfers", Some(request))
        .await;
    assert_eq!(status, StatusCode::INSUFFICIENT_STORAGE, "{body}");
    assert_eq!(body["error"]["code"], "insufficient_storage");
}

#[tokio::test]
async fn open_uploads_are_limited_per_device() {
    let host = host_with_config(Config {
        auth_rate_limit_per_minute: 1000,
        max_uploads_per_device: 1,
        ..Config::default()
    })
    .await;

    let first = start_ok(&host, "/", "a.txt", b"a").await;
    let (status, _) = start_upload(&host, "/", "b.txt", b"b").await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);

    // Cancelling frees the place and deletes the partial file.
    let (status, _) = host
        .send_json_as(
            Some(&host.token),
            "DELETE",
            &format!("/api/transfers/{first}"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(partial_files(&host, "").is_empty());
    start_ok(&host, "/", "b.txt", b"b").await;
}

#[tokio::test]
async fn devices_only_see_their_own_uploads() {
    let host = writable_host().await;
    let mine = start_ok(&host, "/", "mine.txt", b"x").await;

    let other = host
        .pair_device("Other", &["browse", "read", "upload"])
        .await;
    let other_token = host.log_in(&other).await.unwrap();

    let (status, list) = host
        .send_json_as(Some(&other_token), "GET", "/api/transfers", None)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list["uploads"], json!([]));

    for (method, uri) in [
        ("GET", format!("/api/transfers/{mine}")),
        ("POST", format!("/api/transfers/{mine}/complete")),
        ("DELETE", format!("/api/transfers/{mine}")),
    ] {
        let (status, _) = host
            .send_json_as(Some(&other_token), method, &uri, None)
            .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{method} {uri}");
    }

    let (_, list) = host
        .send_json_as(Some(&host.token), "GET", "/api/transfers", None)
        .await;
    assert_eq!(list["uploads"][0]["id"], mine.as_str());
    assert_eq!(list["uploads"][0]["name"], "mine.txt");
}

#[tokio::test]
async fn partial_files_cannot_be_read_through_the_api() {
    let host = writable_host().await;
    let upload_id = start_ok(&host, "/", "secret.txt", b"partial").await;

    // Build the file ID a device would need to name the partial file.
    let partial_path = RelPath::parse(&format!("/.riftview-upload-{upload_id}.part")).unwrap();
    let forged_id = FileId::new(&host.share_id, partial_path).encode();

    let (status, _) = host.get_json(&format!("/api/files/{forged_id}")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = host
        .get_json(&format!("/api/files/{forged_id}/download"))
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// --- Persistence and cleanup ----------------------------------------------------------

#[tokio::test]
async fn upload_progress_survives_a_restart() {
    let mut host = writable_host().await;
    let data = sample_data(CHUNK + 10);
    let upload_id = start_ok(&host, "/", "a.bin", &data).await;
    put_chunk(&host, &upload_id, 0, &data).await;

    host.restart();
    host.token = host.log_in(&host.device).await.unwrap();

    let (status, progress) = host
        .send_json_as(
            Some(&host.token),
            "GET",
            &format!("/api/transfers/{upload_id}"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{progress}");
    assert_eq!(progress["missing_chunks"], json!([1]));

    put_chunk(&host, &upload_id, 1, &data).await;
    let (status, _) = complete(&host, &upload_id).await;
    assert_eq!(status, StatusCode::CREATED);
    assert!(std::fs::read(host.shared_path().join("a.bin")).unwrap() == data);
}

#[tokio::test]
async fn inactive_uploads_expire() {
    let host = writable_host().await;
    let upload_id = start_ok(&host, "/", "a.txt", b"abc").await;

    // Nothing has expired yet.
    assert_eq!(remove_expired_uploads(&host.state, now_ms()).await, 0);

    // A little over a day later, it has.
    let a_day_later = now_ms() + 25 * 60 * 60 * 1000;
    assert_eq!(remove_expired_uploads(&host.state, a_day_later).await, 1);

    assert!(partial_files(&host, "").is_empty());
    let (status, _) = host
        .send_json_as(
            Some(&host.token),
            "GET",
            &format!("/api/transfers/{upload_id}"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn removing_a_share_deletes_its_partial_files() {
    let host = writable_host().await;
    start_ok(&host, "/Documents", "a.txt", b"abc").await;
    assert_eq!(partial_files(&host, "Documents").len(), 1);

    let (status, _) = host
        .admin_json("DELETE", &format!("/admin/shares/{}", host.share_id), None)
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    assert!(partial_files(&host, "Documents").is_empty());
    assert!(
        host.shared_path().join("Documents/notes.txt").exists(),
        "real files untouched"
    );
}
