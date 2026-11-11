//! Test helpers: a throwaway host with a share full of sample files, plus a
//! reference client that pairs and logs in exactly the way the React Native
//! app should (see `docs/auth-protocol.md`).

#![allow(dead_code)] // Not every test file uses every helper.

use std::net::SocketAddr;
use std::path::Path;

use axum::Router;
use axum::body::Body;
use axum::extract::connect_info::MockConnectInfo;
use axum::http::{Request, Response, StatusCode};
use ed25519_dalek::{Signer, SigningKey};
use http_body_util::BodyExt;
use riftview_api::auth::{crypto, protocol};
use riftview_api::config::Config;
use riftview_api::server::{admin_app, app};
use riftview_api::state::AppState;
use serde_json::{Value, json};
use spake2::{Ed25519Group, Identity, Password, Spake2};
use tower::ServiceExt;

pub const FULL_ACCESS: &[&str] = &["browse", "read", "upload", "rename", "move", "delete"];

/// Where test requests appear to come from.
const DEVICE_ADDRESS: ([u8; 4], u16) = ([192, 168, 1, 50], 50_000);
const LOOPBACK_ADDRESS: ([u8; 4], u16) = ([127, 0, 0, 1], 50_001);

/// A paired test device: its ID and its private key.
pub struct TestDevice {
    pub id: String,
    pub key: SigningKey,
}

/// A complete app running in memory (no network), with temp folders that
/// are deleted when the test ends. On creation it registers a share and
/// pairs and logs in one device with full access.
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
/// `outside_dir` is a separate folder holding `secret.txt`. Tests use it to
/// check that nothing outside the share can ever be reached.
pub struct TestHost {
    pub state: AppState,
    pub app: Router,
    pub admin: Router,
    pub data_dir: tempfile::TempDir,
    pub shared_dir: tempfile::TempDir,
    pub outside_dir: tempfile::TempDir,
    pub share_id: String,
    pub device: TestDevice,
    /// Session token of `device`, sent automatically by the request helpers.
    pub token: String,
}

impl TestHost {
    pub async fn new() -> Self {
        Self::with_config(Config {
            auth_rate_limit_per_minute: 1000, // Tests make lots of requests.
            ..Config::default()
        })
        .await
    }

    pub async fn with_config(config: Config) -> Self {
        let data_dir = tempfile::tempdir().unwrap();
        let shared_dir = tempfile::tempdir().unwrap();
        let outside_dir = tempfile::tempdir().unwrap();

        create_sample_files(shared_dir.path());
        std::fs::write(outside_dir.path().join("secret.txt"), b"TOP SECRET").unwrap();

        let state = AppState::load(config, data_dir.path().to_path_buf()).unwrap();
        let app = app(state.clone()).layer(MockConnectInfo(SocketAddr::from(DEVICE_ADDRESS)));
        let admin =
            admin_app(state.clone()).layer(MockConnectInfo(SocketAddr::from(LOOPBACK_ADDRESS)));

        let mut host = Self {
            state,
            app,
            admin,
            data_dir,
            shared_dir,
            outside_dir,
            share_id: String::new(),
            device: TestDevice {
                id: String::new(),
                key: SigningKey::from_bytes(&[0; 32]),
            },
            token: String::new(),
        };

        let request = json!({ "name": "Test", "path": host.shared_dir.path() });
        let (status, body) = host
            .admin_json("POST", "/admin/shares", Some(request))
            .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        host.share_id = body["id"].as_str().unwrap().to_owned();

        host.device = host.pair_device("Test Phone", FULL_ACCESS).await;
        host.token = host.log_in(&host.device).await.expect("login failed");
        host
    }

    /// The real path of the shared folder on disk.
    pub fn shared_path(&self) -> &Path {
        self.shared_dir.path()
    }

    /// SHA-256 of the server certificate's public key, as a client that
    /// pinned it would know it.
    pub fn host_key_hash(&self) -> [u8; 32] {
        self.state.identity().key_hash
    }

    // --- Requests ----------------------------------------------------------

    pub async fn send(&self, request: Request<Body>) -> Response<Body> {
        self.app.clone().oneshot(request).await.unwrap()
    }

    /// GET with the logged-in device's token.
    pub async fn get_json(&self, uri: &str) -> (StatusCode, Value) {
        self.send_json("GET", uri, None).await
    }

    /// Request with the logged-in device's token.
    pub async fn send_json(
        &self,
        method: &str,
        uri: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        self.send_json_as(Some(&self.token), method, uri, body)
            .await
    }

    /// Request to the device API with any token (or none).
    pub async fn send_json_as(
        &self,
        token: Option<&str>,
        method: &str,
        uri: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let request = json_request(method, uri, body, token);
        read_json(self.send(request).await).await
    }

    /// Request to the admin API with the admin token.
    pub async fn admin_json(
        &self,
        method: &str,
        uri: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let token = self.state.admin_token().as_str();
        let request = json_request(method, uri, body, Some(token));
        read_json(self.admin.clone().oneshot(request).await.unwrap()).await
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

    // --- Pairing (reference client) -----------------------------------------

    /// Host starts pairing. Returns `(pairing_id, code)`.
    pub async fn start_pairing(&self) -> (String, String) {
        let (status, body) = self.admin_json("POST", "/admin/pairing", None).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        (
            body["pairing_id"].as_str().unwrap().to_owned(),
            body["code"].as_str().unwrap().to_owned(),
        )
    }

    /// The device's side of pairing, with knobs for attack tests.
    pub async fn device_pairing_attempt(
        &self,
        attempt: PairingAttempt<'_>,
    ) -> Result<PairingSuccess, (StatusCode, Value)> {
        // SPAKE2: the device is side A.
        let (spake, device_message) = Spake2::<Ed25519Group>::start_a(
            &Password::new(attempt.code.as_bytes()),
            &Identity::new(protocol::SPAKE2_CLIENT_ID),
            &Identity::new(protocol::SPAKE2_HOST_ID),
        );
        let request = json!({ "spake2_message": crypto::encode_base64(&device_message) });
        let (status, body) = self
            .send_json_as(None, "POST", "/api/pairing/initiate", Some(request))
            .await;
        if status != StatusCode::OK {
            return Err((status, body));
        }

        let pairing_id = body["pairing_id"].as_str().unwrap().to_owned();
        let host_message = crypto::decode_base64(body["spake2_message"].as_str().unwrap()).unwrap();
        let shared_key = spake.finish(&host_message).unwrap();

        // Prove we know the code, binding in the certificate we saw.
        let public_key = attempt.key.verifying_key().to_bytes();
        let proof_message = protocol::client_confirm_message(
            &attempt.seen_host_key_hash,
            &public_key,
            attempt.name,
        );
        let proof = crypto::hmac_sha256(&shared_key, &proof_message);

        let request = json!({
            "pairing_id": pairing_id,
            "device_name": attempt.name,
            "device_public_key": crypto::encode_base64(&public_key),
            "proof": crypto::encode_base64(&proof),
        });
        let (status, body) = self
            .send_json_as(None, "POST", "/api/pairing/complete", Some(request))
            .await;
        if status != StatusCode::OK {
            return Err((status, body));
        }

        // Check the host's proof before trusting anything it said.
        let device_id = body["device_id"].as_str().unwrap().to_owned();
        let host_proof = crypto::decode_base64(body["host_proof"].as_str().unwrap()).unwrap();
        let expected =
            protocol::host_confirm_message(&attempt.seen_host_key_hash, &public_key, &device_id);
        assert!(
            crypto::hmac_sha256_matches(&shared_key, &expected, &host_proof),
            "host proof did not verify"
        );

        Ok(PairingSuccess {
            pairing_id,
            device_id,
            host_pin: body["host_pin"].as_str().unwrap().to_owned(),
        })
    }

    /// Pair a new device end to end, approving it with `permissions`.
    pub async fn pair_device(&self, name: &str, permissions: &[&str]) -> TestDevice {
        let (_, code) = self.start_pairing().await;
        let key = SigningKey::from_bytes(&crypto::random_bytes());

        let success = self
            .device_pairing_attempt(PairingAttempt {
                code: &code,
                name,
                key: &key,
                seen_host_key_hash: self.host_key_hash(),
            })
            .await
            .unwrap_or_else(|(status, body)| panic!("pairing failed: {status} {body}"));

        let approval = json!({ "pairing_id": success.pairing_id, "permissions": permissions });
        let (status, body) = self
            .admin_json("POST", "/admin/pairing/approve", Some(approval))
            .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");

        let status_uri = format!("/api/pairing/{}", success.pairing_id);
        let (_, status_body) = self.send_json_as(None, "GET", &status_uri, None).await;
        assert_eq!(status_body["stage"], "approved");
        assert_eq!(status_body["device_id"], success.device_id.as_str());

        TestDevice {
            id: success.device_id,
            key,
        }
    }

    // --- Login (reference client) --------------------------------------------

    /// Ask for a challenge. Returns `(challenge_id, nonce)`.
    pub async fn get_challenge(&self, device_id: &str) -> (String, Vec<u8>) {
        let request = json!({ "device_id": device_id });
        let (status, body) = self
            .send_json_as(None, "POST", "/api/auth/challenge", Some(request))
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        (
            body["challenge_id"].as_str().unwrap().to_owned(),
            crypto::decode_base64(body["nonce"].as_str().unwrap()).unwrap(),
        )
    }

    /// Sign a challenge and exchange it for a session token.
    pub async fn redeem_challenge(
        &self,
        challenge_id: &str,
        signature: &[u8],
    ) -> Result<String, StatusCode> {
        let request = json!({
            "challenge_id": challenge_id,
            "signature": crypto::encode_base64(signature),
        });
        let (status, body) = self
            .send_json_as(None, "POST", "/api/auth/session", Some(request))
            .await;
        match status {
            StatusCode::OK => Ok(body["token"].as_str().unwrap().to_owned()),
            other => Err(other),
        }
    }

    /// Full login: challenge, sign, redeem.
    pub async fn log_in(&self, device: &TestDevice) -> Result<String, StatusCode> {
        let (challenge_id, nonce) = self.get_challenge(&device.id).await;
        let message = protocol::login_message(&self.host_key_hash(), &device.id, &nonce);
        let signature = device.key.sign(&message).to_bytes();
        self.redeem_challenge(&challenge_id, &signature).await
    }
}

/// Inputs for one pairing attempt by a device.
pub struct PairingAttempt<'a> {
    pub code: &'a str,
    pub name: &'a str,
    pub key: &'a SigningKey,
    /// The certificate key hash the device saw on the connection. A
    /// man-in-the-middle would make this differ from the real one.
    pub seen_host_key_hash: [u8; 32],
}

pub struct PairingSuccess {
    pub pairing_id: String,
    pub device_id: String,
    pub host_pin: String,
}

fn json_request(
    method: &str,
    uri: &str,
    body: Option<Value>,
    token: Option<&str>,
) -> Request<Body> {
    let mut request = Request::builder().method(method).uri(uri);
    if let Some(token) = token {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    let body = match body {
        Some(json) => {
            request = request.header("content-type", "application/json");
            Body::from(json.to_string())
        }
        None => Body::empty(),
    };
    request.body(body).unwrap()
}

async fn read_json(response: Response<Body>) -> (StatusCode, Value) {
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

/// Build a GET request carrying a bearer token.
pub fn authed_get(uri: &str, token: &str) -> Request<Body> {
    Request::get(uri)
        .header("authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap()
}
