#![allow(dead_code)]

use std::path::Path;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, Response, StatusCode};
use http_body_util::BodyExt;
use riftview_api::config::Config;
use riftview_api::server::app;
use riftview_api::state::AppState;
use serde_json::Value;
use tower::ServiceExt;

pub struct TestHost {
    pub app: Router,
    pub data: tempfile::TempDir,
    /// Directory registered as a share.
    pub files: tempfile::TempDir,
    /// Sibling directory that must never be reachable.
    pub outside: tempfile::TempDir,
    pub share_id: String,
}

impl TestHost {
    pub async fn new() -> Self {
        let data = tempfile::tempdir().unwrap();
        let files = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();

        let f = files.path();
        std::fs::create_dir_all(f.join("Documents/Projects")).unwrap();
        std::fs::write(f.join("report.pdf"), vec![b'x'; 2048]).unwrap();
        std::fs::write(f.join("Documents/notes.txt"), b"hello world").unwrap();
        std::fs::write(f.join("Documents/a.txt"), b"a").unwrap();
        std::fs::write(outside.path().join("secret.txt"), b"TOP SECRET").unwrap();

        let state = AppState::load(Config::default(), data.path().to_path_buf()).unwrap();
        let app = app(state);
        let mut host = Self { app, data, files, outside, share_id: String::new() };

        let (status, body) = host
            .json("POST", "/api/shares", Some(serde_json::json!({
                "name": "Test", "path": host.files.path()
            })))
            .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        host.share_id = body["id"].as_str().unwrap().to_owned();
        host
    }

    pub fn root(&self) -> &Path {
        self.files.path()
    }

    pub async fn request(&self, req: Request<Body>) -> Response<Body> {
        self.app.clone().oneshot(req).await.unwrap()
    }

    pub async fn get(&self, uri: &str) -> (StatusCode, Value) {
        self.json("GET", uri, None).await
    }

    pub async fn json(&self, method: &str, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
        let mut b = Request::builder().method(method).uri(uri);
        let body = match body {
            Some(v) => {
                b = b.header("content-type", "application/json");
                Body::from(v.to_string())
            }
            None => Body::empty(),
        };
        let res = self.request(b.body(body).unwrap()).await;
        let status = res.status();
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        let v = if bytes.is_empty() { Value::Null } else {
            serde_json::from_slice(&bytes).unwrap_or(Value::String(String::from_utf8_lossy(&bytes).into()))
        };
        (status, v)
    }

    pub async fn browse(&self, path: &str) -> (StatusCode, Value) {
        let q = urlencode(path);
        self.get(&format!("/api/shares/{}/files?path={q}", self.share_id)).await
    }

    /// Find an entry's id by name in a directory listing.
    pub async fn id_of(&self, dir: &str, name: &str) -> String {
        let (_, v) = self.browse(dir).await;
        v["entries"].as_array().unwrap().iter()
            .find(|e| e["name"] == name)
            .unwrap_or_else(|| panic!("{name} not in {dir}: {v}"))["id"]
            .as_str().unwrap().to_owned()
    }
}

pub fn urlencode(s: &str) -> String {
    s.bytes().map(|b| match b {
        b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => (b as char).to_string(),
        _ => format!("%{b:02X}"),
    }).collect()
}

pub async fn body_bytes(res: Response<Body>) -> Vec<u8> {
    res.into_body().collect().await.unwrap().to_bytes().to_vec()
}
