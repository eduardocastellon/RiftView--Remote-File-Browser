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
