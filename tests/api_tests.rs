//! End-to-end tests of the HTTP API.

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::{TestHost, authed_get, body_bytes, entry_names};
use serde_json::json;

fn range_request(uri: &str, range: &str, token: &str) -> Request<Body> {
    Request::get(uri)
        .header("range", range)
        .header("authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap()
}

// --- Host information ------------------------------------------------------

#[tokio::test]
async fn health_and_version() {
    let host = TestHost::new().await;

    let (status, body) = host.get_json("/api/health").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({ "status": "ok" }));

    let (status, body) = host.get_json("/api/version").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["version"], env!("CARGO_PKG_VERSION"));
}

#[tokio::test]
async fn unknown_route_is_json_404() {
    let host = TestHost::new().await;
    let (status, body) = host.get_json("/api/nope").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"]["code"], "not_found");
}

// --- Shares ----------------------------------------------------------------

#[tokio::test]
async fn devices_see_shares_without_host_path() {
    let host = TestHost::new().await;

    let (status, body) = host.get_json("/api/shares").await;
    assert_eq!(status, StatusCode::OK);
    let host_path = host.shared_path().to_string_lossy().into_owned();
    assert!(
        !body.to_string().contains(&host_path),
        "host path leaked: {body}"
    );
    assert_eq!(body["shares"][0]["writable"], false);
}

#[tokio::test]
async fn devices_cannot_manage_shares() {
    let host = TestHost::new().await;
    let request = json!({ "name": "x", "path": host.outside_dir.path() });
    let (status, _) = host.send_json("POST", "/api/shares", Some(request)).await;
    assert_ne!(status, StatusCode::CREATED);

    let share_uri = format!("/api/shares/{}", host.share_id);
    let (status, _) = host.send_json("DELETE", &share_uri, None).await;
    assert_ne!(status, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn host_manages_shares_through_admin_api() {
    let host = TestHost::new().await;
    let share_uri = format!("/admin/shares/{}", host.share_id);

    let changes = json!({ "name": "Renamed", "writable": true });
    let (status, body) = host.admin_json("PATCH", &share_uri, Some(changes)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["name"], "Renamed");
    assert_eq!(body["writable"], true);

    let (status, _) = host.admin_json("DELETE", &share_uri, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, _) = host
        .get_json(&format!("/api/shares/{}", host.share_id))
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn share_registration_validation() {
    let host = TestHost::new().await;

    let relative_path = json!({ "name": "x", "path": "relative" });
    let (status, _) = host
        .admin_json("POST", "/admin/shares", Some(relative_path))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let inside_existing = json!({ "name": "dup", "path": host.shared_path().join("Documents") });
    let (status, _) = host
        .admin_json("POST", "/admin/shares", Some(inside_existing))
        .await;
    assert_eq!(status, StatusCode::CONFLICT);

    let data_dir = json!({ "name": "data", "path": host.data_dir.path() });
    let (status, _) = host
        .admin_json("POST", "/admin/shares", Some(data_dir))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

// --- Browsing --------------------------------------------------------------

#[tokio::test]
async fn browse_root_lists_folders_first() {
    let host = TestHost::new().await;
    let (status, listing) = host.browse("/").await;
    assert_eq!(status, StatusCode::OK, "{listing}");
    assert_eq!(listing["path"], "/");
    assert_eq!(listing["total"], 2);

    let folder = &listing["entries"][0];
    assert_eq!(folder["name"], "Documents");
    assert_eq!(folder["type"], "directory");
    assert!(folder.get("size").is_none());

    let file = &listing["entries"][1];
    assert_eq!(file["name"], "report.pdf");
    assert_eq!(file["type"], "file");
    assert_eq!(file["size"], 2048);
    assert_eq!(file["mime"], "application/pdf");
}

#[tokio::test]
async fn browse_pagination_and_sort() {
    let host = TestHost::new().await;
    let documents = format!("/api/shares/{}/files?path=/Documents", host.share_id);

    let (_, page) = host
        .get_json(&format!("{documents}&limit=1&offset=1"))
        .await;
    assert_eq!(page["total"], 3);
    assert_eq!(entry_names(&page), ["a.txt"]);

    let (_, by_size) = host
        .get_json(&format!("{documents}&sort=size&order=desc"))
        .await;
    assert_eq!(entry_names(&by_size), ["Projects", "notes.txt", "a.txt"]);
}

#[tokio::test]
async fn browse_errors() {
    let host = TestHost::new().await;
    assert_eq!(host.browse("/missing").await.0, StatusCode::NOT_FOUND);
    assert_eq!(host.browse("/report.pdf").await.0, StatusCode::BAD_REQUEST);

    let (status, _) = host.get_json("/api/shares/shr_nope/files").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// --- File details ----------------------------------------------------------

#[tokio::test]
async fn metadata_by_id() {
    let host = TestHost::new().await;
    let file_id = host.id_of("/Documents", "notes.txt").await;

    let (status, entry) = host.get_json(&format!("/api/files/{file_id}")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(entry["path"], "/Documents/notes.txt");
    assert_eq!(entry["size"], 11);

    let (status, _) = host.get_json("/api/files/garbage").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// --- Downloads -------------------------------------------------------------

#[tokio::test]
async fn download_whole_file_and_ranges() {
    let host = TestHost::new().await;
    let file_id = host.id_of("/Documents", "notes.txt").await;
    let uri = format!("/api/files/{file_id}/download");

    let response = host.send(authed_get(&uri, &host.token)).await;
    assert_eq!(response.status(), StatusCode::OK);
    let headers = response.headers();
    assert_eq!(headers["accept-ranges"], "bytes");
    assert_eq!(headers["content-length"], "11");
    assert!(
        headers["content-disposition"]
            .to_str()
            .unwrap()
            .contains("notes.txt")
    );
    assert_eq!(body_bytes(response).await, b"hello world");

    let response = host
        .send(range_request(&uri, "bytes=6-", &host.token))
        .await;
    assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(response.headers()["content-range"], "bytes 6-10/11");
    assert_eq!(body_bytes(response).await, b"world");

    let response = host
        .send(range_request(&uri, "bytes=-5", &host.token))
        .await;
    assert_eq!(body_bytes(response).await, b"world");

    let response = host
        .send(range_request(&uri, "bytes=50-", &host.token))
        .await;
    assert_eq!(response.status(), StatusCode::RANGE_NOT_SATISFIABLE);
    assert_eq!(response.headers()["content-range"], "bytes */11");
}

#[tokio::test]
async fn downloading_a_folder_is_rejected() {
    let host = TestHost::new().await;
    let folder_id = host.id_of("/", "Documents").await;
    let (status, _) = host
        .get_json(&format!("/api/files/{folder_id}/download"))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn large_file_streams_intact() {
    let host = TestHost::new().await;
    let five_mib: Vec<u8> = (0..5 * 1024 * 1024u32).map(|i| (i % 251) as u8).collect();
    std::fs::write(host.shared_path().join("big.bin"), &five_mib).unwrap();

    let file_id = host.id_of("/", "big.bin").await;
    let response = host
        .send(authed_get(
            &format!("/api/files/{file_id}/download"),
            &host.token,
        ))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_bytes(response).await, five_mib);
}

// --- Download integrity ----------------------------------------------------

fn get_with_headers(uri: &str, token: &str, headers: &[(&str, &str)]) -> Request<Body> {
    let mut request = Request::get(uri).header("authorization", format!("Bearer {token}"));
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    request.body(Body::empty()).unwrap()
}

async fn download_etag(host: &TestHost, uri: &str) -> String {
    let response = host.send(authed_get(uri, &host.token)).await;
    assert_eq!(response.status(), StatusCode::OK);
    response.headers()["etag"].to_str().unwrap().to_owned()
}

#[tokio::test]
async fn resume_only_continues_the_same_version() {
    let host = TestHost::new().await;
    let file_id = host.id_of("/Documents", "notes.txt").await;
    let uri = format!("/api/files/{file_id}/download");
    let etag = download_etag(&host, &uri).await;

    // Same version: the resume is honoured.
    let request = get_with_headers(
        &uri,
        &host.token,
        &[("range", "bytes=6-"), ("if-range", &etag)],
    );
    let response = host.send(request).await;
    assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(body_bytes(response).await, b"world");

    // The file changes: the ETag changes, and the old one no longer resumes.
    std::fs::write(
        host.shared_path().join("Documents/notes.txt"),
        b"HELLO THERE!",
    )
    .unwrap();
    let new_etag = download_etag(&host, &uri).await;
    assert_ne!(etag, new_etag);

    let request = get_with_headers(
        &uri,
        &host.token,
        &[("range", "bytes=6-"), ("if-range", &etag)],
    );
    let response = host.send(request).await;
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "stale resume sends it all"
    );
    assert_eq!(body_bytes(response).await, b"HELLO THERE!");
}

#[tokio::test]
async fn hash_endpoint_matches_the_download() {
    let host = TestHost::new().await;
    let file_id = host.id_of("/Documents", "notes.txt").await;
    let etag = download_etag(&host, &format!("/api/files/{file_id}/download")).await;

    let hash_uri = format!("/api/files/{file_id}/hash");
    let (status, body) = host.get_json(&hash_uri).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["sha256"],
        "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
    );
    assert_eq!(body["size"], 11);
    assert_eq!(body["etag"], etag.as_str());

    // Asking again is answered from the cache, with the same result.
    let (_, again) = host.get_json(&hash_uri).await;
    assert_eq!(again, body);

    // After a change, the hash is recomputed.
    std::fs::write(host.shared_path().join("Documents/notes.txt"), b"changed").unwrap();
    let (_, changed) = host.get_json(&hash_uri).await;
    assert_ne!(changed["sha256"], body["sha256"]);
    assert_eq!(changed["size"], 7);
}

#[tokio::test]
async fn hashing_needs_read_permission() {
    let host = TestHost::new().await;
    let browser = host.pair_device("Browser", &["browse"]).await;
    let token = host.log_in(&browser).await.unwrap();
    let file_id = host.id_of("/Documents", "notes.txt").await;

    let (status, _) = host
        .send_json_as(
            Some(&token),
            "GET",
            &format!("/api/files/{file_id}/hash"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn file_edited_during_download_cuts_the_connection() {
    use http_body_util::BodyExt;

    let host = TestHost::new().await;
    let path = host.shared_path().join("growing.bin");
    std::fs::write(&path, vec![7u8; 1024 * 1024]).unwrap();
    let file_id = host.id_of("/", "growing.bin").await;

    let response = host
        .send(authed_get(
            &format!("/api/files/{file_id}/download"),
            &host.token,
        ))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let mut body = response.into_body();

    // Receive the first chunk, then edit the file in place.
    body.frame().await.unwrap().unwrap();
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap();
    std::io::Write::write_all(&mut file, b"appended").unwrap();

    let rest = body.collect().await;
    assert!(rest.is_err(), "the download must not finish cleanly");
}

#[tokio::test]
async fn concurrent_downloads_are_limited_per_device() {
    let host = TestHost::with_config(riftview_api::config::Config {
        auth_rate_limit_per_minute: 1000,
        max_transfers_per_device: 1,
        ..Default::default()
    })
    .await;
    let file_id = host.id_of("/Documents", "notes.txt").await;
    let uri = format!("/api/files/{file_id}/download");

    // The first download is still open (its body hasn't been read)…
    let first = host.send(authed_get(&uri, &host.token)).await;
    assert_eq!(first.status(), StatusCode::OK);

    // …so a second one is refused, and hashing counts too.
    let second = host.send(authed_get(&uri, &host.token)).await;
    assert_eq!(second.status(), StatusCode::TOO_MANY_REQUESTS);
    let (status, body) = host.get_json(&format!("/api/files/{file_id}/hash")).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(body["error"]["code"], "too_many_requests");

    // Finishing the first download frees the slot.
    assert_eq!(body_bytes(first).await, b"hello world");
    let third = host.send(authed_get(&uri, &host.token)).await;
    assert_eq!(third.status(), StatusCode::OK);
}

#[tokio::test]
async fn concurrent_downloads_are_limited_in_total() {
    let host = TestHost::with_config(riftview_api::config::Config {
        auth_rate_limit_per_minute: 1000,
        max_concurrent_transfers: 1,
        ..Default::default()
    })
    .await;
    let other = host.pair_device("Other", &["browse", "read"]).await;
    let other_token = host.log_in(&other).await.unwrap();
    let file_id = host.id_of("/Documents", "notes.txt").await;
    let uri = format!("/api/files/{file_id}/download");

    let first = host.send(authed_get(&uri, &host.token)).await;
    assert_eq!(first.status(), StatusCode::OK);

    let from_other_device = host.send(authed_get(&uri, &other_token)).await;
    assert_eq!(from_other_device.status(), StatusCode::TOO_MANY_REQUESTS);

    drop(first); // E.g. the phone disconnected.
    let retry = host.send(authed_get(&uri, &other_token)).await;
    assert_eq!(retry.status(), StatusCode::OK);
}

#[tokio::test]
async fn five_gigabyte_file_ranges_work() {
    let host = TestHost::new().await;
    let size: u64 = 5 * 1024 * 1024 * 1024;

    // A sparse file: 5 GiB long but takes almost no disk space. Reads
    // return zeros except for the marker we write near the end.
    let path = host.shared_path().join("huge.bin");
    let file = std::fs::File::create(&path).unwrap();
    file.set_len(size).unwrap();
    std::os::unix::fs::FileExt::write_all_at(&file, b"END!", size - 4).unwrap();
    drop(file);

    let file_id = host.id_of("/", "huge.bin").await;
    let uri = format!("/api/files/{file_id}/download");

    // Whole-file request: headers only (we don't read 5 GiB in a test).
    let response = host.send(authed_get(&uri, &host.token)).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-length"], size.to_string());
    drop(response);

    // The last 1 KiB, as a resume from just before the end would ask.
    let start = size - 1024;
    let response = host
        .send(range_request(&uri, &format!("bytes={start}-"), &host.token))
        .await;
    assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(
        response.headers()["content-range"],
        format!("bytes {start}-{}/{size}", size - 1)
    );
    let tail = body_bytes(response).await;
    assert_eq!(tail.len(), 1024);
    assert_eq!(&tail[1020..], b"END!");
    assert!(tail[..1020].iter().all(|&byte| byte == 0));
}
