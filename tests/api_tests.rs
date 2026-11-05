mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::{TestHost, body_bytes};

#[tokio::test]
async fn health_and_version() {
    let h = TestHost::new().await;
    let (s, v) = h.get("/api/health").await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v, serde_json::json!({"status": "ok"}));
    let (s, v) = h.get("/api/version").await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["version"], env!("CARGO_PKG_VERSION"));
}

#[tokio::test]
async fn unknown_route_is_json_404() {
    let h = TestHost::new().await;
    let (s, v) = h.get("/api/nope").await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    assert_eq!(v["error"]["code"], "not_found");
}

#[tokio::test]
async fn share_crud_hides_host_path() {
    let h = TestHost::new().await;
    let (s, v) = h.get("/api/shares").await;
    assert_eq!(s, StatusCode::OK);
    let text = v.to_string();
    assert!(!text.contains(&*h.root().to_string_lossy()), "host path leaked: {text}");
    assert_eq!(v["shares"][0]["writable"], false);

    let uri = format!("/api/shares/{}", h.share_id);
    let (s, v) = h.json("PATCH", &uri, Some(serde_json::json!({"name": "Renamed", "writable": true}))).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["name"], "Renamed");
    assert_eq!(v["writable"], true);

    let (s, _) = h.json("DELETE", &uri, None).await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let (s, _) = h.get(&uri).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn browse_root_sorted_dirs_first() {
    let h = TestHost::new().await;
    let (s, v) = h.browse("/").await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["path"], "/");
    assert_eq!(v["total"], 2);
    let e = v["entries"].as_array().unwrap();
    assert_eq!(e[0]["name"], "Documents");
    assert_eq!(e[0]["type"], "directory");
    assert!(e[0].get("size").is_none());
    assert_eq!(e[1]["name"], "report.pdf");
    assert_eq!(e[1]["type"], "file");
    assert_eq!(e[1]["size"], 2048);
    assert_eq!(e[1]["mime"], "application/pdf");
}

#[tokio::test]
async fn browse_pagination_and_sort() {
    let h = TestHost::new().await;
    let base = format!("/api/shares/{}/files?path=/Documents", h.share_id);
    let (_, v) = h.get(&format!("{base}&limit=1&offset=1")).await;
    assert_eq!(v["total"], 3);
    assert_eq!(v["entries"].as_array().unwrap().len(), 1);
    assert_eq!(v["entries"][0]["name"], "a.txt");

    let (_, v) = h.get(&format!("{base}&sort=size&order=desc")).await;
    let names: Vec<_> = v["entries"].as_array().unwrap().iter().map(|e| e["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["Projects", "notes.txt", "a.txt"]);
}

#[tokio::test]
async fn browse_errors() {
    let h = TestHost::new().await;
    assert_eq!(h.browse("/missing").await.0, StatusCode::NOT_FOUND);
    assert_eq!(h.browse("/report.pdf").await.0, StatusCode::BAD_REQUEST);
    let (s, _) = h.get("/api/shares/shr_nope/files").await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn metadata_by_id() {
    let h = TestHost::new().await;
    let id = h.id_of("/Documents", "notes.txt").await;
    let (s, v) = h.get(&format!("/api/files/{id}")).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["path"], "/Documents/notes.txt");
    assert_eq!(v["size"], 11);
    assert_eq!(h.get("/api/files/garbage").await.0, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn download_full_and_ranges() {
    let h = TestHost::new().await;
    let id = h.id_of("/Documents", "notes.txt").await;
    let uri = format!("/api/files/{id}/download");

    let res = h.request(Request::get(&uri).body(Body::empty()).unwrap()).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(res.headers()["accept-ranges"], "bytes");
    assert_eq!(res.headers()["content-length"], "11");
    assert!(res.headers()["content-disposition"].to_str().unwrap().contains("notes.txt"));
    assert_eq!(body_bytes(res).await, b"hello world");

    let res = h.request(Request::get(&uri).header("range", "bytes=6-").body(Body::empty()).unwrap()).await;
    assert_eq!(res.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(res.headers()["content-range"], "bytes 6-10/11");
    assert_eq!(body_bytes(res).await, b"world");

    let res = h.request(Request::get(&uri).header("range", "bytes=-5").body(Body::empty()).unwrap()).await;
    assert_eq!(body_bytes(res).await, b"world");

    let res = h.request(Request::get(&uri).header("range", "bytes=50-").body(Body::empty()).unwrap()).await;
    assert_eq!(res.status(), StatusCode::RANGE_NOT_SATISFIABLE);
    assert_eq!(res.headers()["content-range"], "bytes */11");
}

#[tokio::test]
async fn download_directory_rejected() {
    let h = TestHost::new().await;
    let id = h.id_of("/", "Documents").await;
    let (s, _) = h.get(&format!("/api/files/{id}/download")).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn large_file_streams_intact() {
    let h = TestHost::new().await;
    let data: Vec<u8> = (0..5 * 1024 * 1024u32).map(|i| (i % 251) as u8).collect();
    std::fs::write(h.root().join("big.bin"), &data).unwrap();
    let id = h.id_of("/", "big.bin").await;
    let res = h.request(Request::get(format!("/api/files/{id}/download")).body(Body::empty()).unwrap()).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(body_bytes(res).await, data);
}

#[tokio::test]
async fn share_registration_validation() {
    let h = TestHost::new().await;
    let (s, _) = h.json("POST", "/api/shares", Some(serde_json::json!({"name": "x", "path": "relative"}))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = h.json("POST", "/api/shares", Some(serde_json::json!({
        "name": "dup", "path": h.root().join("Documents")
    }))).await;
    assert_eq!(s, StatusCode::CONFLICT);
    let (s, _) = h.json("POST", "/api/shares", Some(serde_json::json!({"name": "data", "path": h.data.path()}))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
}
