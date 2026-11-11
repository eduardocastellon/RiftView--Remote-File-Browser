//! Test helpers: a throwaway host with a share full of sample files.

#![allow(dead_code)] // Not every test file uses every helper.

use std::path::Path;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, Response, StatusCode};
use http_body_util::BodyExt;
use riftview_api::config::Config;
use riftview_api::server::app;
use riftview_api::state::AppState;
use serde_json::{Value, json};
use tower::ServiceExt;

/// A complete app running in memory (no network), with temp folders that
/// are deleted when the test ends.
///
/// The shared folder contains:
///
/// ```text
/// report.pdf            (2048 bytes)
/// Documents/
///   notes.txt           ("hello world")
///   a.txt               ("a")
///   Projects/
/// ```
///
/// `outside` is a separate folder holding `secret.txt`. Tests use it to
/// check that nothing outside the share can ever be reached.
pub struct TestHost {
    pub app: Router,
    pub data_dir: tempfile::TempDir,
    pub shared_dir: tempfile::TempDir,
    pub outside_dir: tempfile::TempDir,
    pub share_id: String,
}

impl TestHost {
    pub async fn new() -> Self {
        let data_dir = tempfile::tempdir().unwrap();
        let shared_dir = tempfile::tempdir().unwrap();
        let outside_dir = tempfile::tempdir().unwrap();

        create_sample_files(shared_dir.path());
        std::fs::write(outside_dir.path().join("secret.txt"), b"TOP SECRET").unwrap();

        let state = AppState::load(Config::default(), data_dir.path().to_path_buf()).unwrap();
        let mut host = Self {
            app: app(state),
            data_dir,
            shared_dir,
            outside_dir,
            share_id: String::new(),
        };

        let request = json!({ "name": "Test", "path": host.shared_dir.path() });
        let (status, body) = host.send_json("POST", "/api/shares", Some(request)).await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        host.share_id = body["id"].as_str().unwrap().to_owned();

        host
    }

    /// The real path of the shared folder on disk.
    pub fn shared_path(&self) -> &Path {
        self.shared_dir.path()
    }

    pub async fn send(&self, request: Request<Body>) -> Response<Body> {
        self.app.clone().oneshot(request).await.unwrap()
    }

    pub async fn get_json(&self, uri: &str) -> (StatusCode, Value) {
        self.send_json("GET", uri, None).await
    }

    /// Send a request (with an optional JSON body) and parse the JSON reply.
    pub async fn send_json(
        &self,
        method: &str,
        uri: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let mut request = Request::builder().method(method).uri(uri);
        let request_body = match body {
            Some(json) => {
                request = request.header("content-type", "application/json");
                Body::from(json.to_string())
            }
            None => Body::empty(),
        };

        let response = self.send(request.body(request_body).unwrap()).await;
        let status = response.status();
        let bytes = body_bytes(response).await;

        let parsed = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes)
                .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into()))
        };
        (status, parsed)
    }

    /// List a folder in the test share.
    pub async fn browse(&self, path: &str) -> (StatusCode, Value) {
        let uri = format!(
            "/api/shares/{}/files?path={}",
            self.share_id,
            url_encode(path)
        );
        self.get_json(&uri).await
    }

    /// Find the file ID of `name` inside folder `dir`.
    pub async fn id_of(&self, dir: &str, name: &str) -> String {
        let (_, listing) = self.browse(dir).await;
        let entry = listing["entries"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["name"] == name)
            .unwrap_or_else(|| panic!("{name} not in {dir}: {listing}"));
        entry["id"].as_str().unwrap().to_owned()
    }
}

fn create_sample_files(root: &Path) {
    std::fs::create_dir_all(root.join("Documents/Projects")).unwrap();
    std::fs::write(root.join("report.pdf"), vec![b'x'; 2048]).unwrap();
    std::fs::write(root.join("Documents/notes.txt"), b"hello world").unwrap();
    std::fs::write(root.join("Documents/a.txt"), b"a").unwrap();
}

/// Percent-encode a path for use in a query string (keeps `/`).
pub fn url_encode(text: &str) -> String {
    text.bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                (byte as char).to_string()
            }
            _ => format!("%{byte:02X}"),
        })
        .collect()
}

pub async fn body_bytes(response: Response<Body>) -> Vec<u8> {
    response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes()
        .to_vec()
}

/// Names of all entries in a folder listing.
pub fn entry_names(listing: &Value) -> Vec<String> {
    listing["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["name"].as_str().unwrap().to_owned())
        .collect()
}
