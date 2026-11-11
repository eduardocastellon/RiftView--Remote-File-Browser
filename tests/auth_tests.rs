//! Pairing, login, permissions and the admin API (planning.md §7, §13.3).

mod common;

use axum::http::StatusCode;
use ed25519_dalek::{Signer, SigningKey};
use riftview_api::auth::{crypto, protocol};
use riftview_api::config::Config;
use serde_json::json;

use common::{FULL_ACCESS, PairingAttempt, TestHost};

fn new_key() -> SigningKey {
    SigningKey::from_bytes(&crypto::random_bytes())
}

/// A code that is guaranteed to differ from `code`.
fn wrong_code(code: &str) -> String {
    let first = code.as_bytes()[0];
    let changed = if first == b'9' {
        '0'
    } else {
        (first + 1) as char
    };
    format!("{changed}{}", &code[1..])
}

// --- Without a login -------------------------------------------------------

#[tokio::test]
async fn protected_routes_require_a_token() {
    let host = TestHost::new().await;
    let file_id = host.id_of("/", "report.pdf").await;

    let protected = [
        "/api/host".to_string(),
        "/api/devices/me".to_string(),
        "/api/shares".to_string(),
        format!("/api/shares/{}/files", host.share_id),
        format!("/api/files/{file_id}"),
        format!("/api/files/{file_id}/download"),
    ];
    for uri in &protected {
        let (status, _) = host.send_json_as(None, "GET", uri, None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{uri}");

        let (status, _) = host.send_json_as(Some("made-up"), "GET", uri, None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{uri} with bogus token");
    }

    let (status, _) = host.send_json_as(None, "GET", "/api/health", None).await;
    assert_eq!(status, StatusCode::OK, "health should stay public");
}

// --- Pairing ---------------------------------------------------------------

#[tokio::test]
async fn paired_device_knows_who_it_is_and_gets_the_pin() {
    let host = TestHost::new().await;

    let (status, me) = host.get_json("/api/devices/me").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(me["id"], host.device.id.as_str());
    assert_eq!(me["name"], "Test Phone");
    assert_eq!(
        me["permissions"].as_array().unwrap().len(),
        FULL_ACCESS.len()
    );

    // The pin returned during pairing matches the server's certificate.
    let (_, code) = host.start_pairing().await;
    let success = host
        .device_pairing_attempt(PairingAttempt {
            code: &code,
            name: "Tablet",
            key: &new_key(),
            seen_host_key_hash: host.host_key_hash(),
        })
        .await
        .unwrap();
    assert_eq!(success.host_pin, host.state.identity().pin());
}

#[tokio::test]
async fn pairing_needs_the_host_to_start_it() {
    let host = TestHost::new().await;
    // The setup's pairing is finished; cancel it so nothing is open.
    host.admin_json("DELETE", "/admin/pairing", None).await;

    let result = host
        .device_pairing_attempt(PairingAttempt {
            code: "123456",
            name: "Stranger",
            key: &new_key(),
            seen_host_key_hash: host.host_key_hash(),
        })
        .await;
    assert_eq!(result.err().unwrap().0, StatusCode::CONFLICT);
}

#[tokio::test]
async fn wrong_code_fails_and_the_code_is_burned() {
    let host = TestHost::new().await;
    let (_, code) = host.start_pairing().await;
    let key = new_key();

    let guess = host
        .device_pairing_attempt(PairingAttempt {
            code: &wrong_code(&code),
            name: "Attacker",
            key: &key,
            seen_host_key_hash: host.host_key_hash(),
        })
        .await;
    assert_eq!(guess.err().unwrap().0, StatusCode::FORBIDDEN);

    // One guess per code: now even the right code is refused.
    let retry = host
        .device_pairing_attempt(PairingAttempt {
            code: &code,
            name: "Phone",
            key: &key,
            seen_host_key_hash: host.host_key_hash(),
        })
        .await;
    assert_eq!(retry.err().unwrap().0, StatusCode::CONFLICT);

    let (_, view) = host.admin_json("GET", "/admin/pairing", None).await;
    assert_eq!(view["stage"], "failed");
}

#[tokio::test]
async fn man_in_the_middle_certificate_is_detected() {
    let host = TestHost::new().await;
    let (_, code) = host.start_pairing().await;

    // The device saw a different certificate than the real server's.
    let result = host
        .device_pairing_attempt(PairingAttempt {
            code: &code,
            name: "Phone",
            key: &new_key(),
            seen_host_key_hash: [0xAB; 32],
        })
        .await;
    assert_eq!(result.err().unwrap().0, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn rejected_device_is_not_trusted() {
    let host = TestHost::new().await;
    let (_, code) = host.start_pairing().await;
    let success = host
        .device_pairing_attempt(PairingAttempt {
            code: &code,
            name: "Unwanted",
            key: &new_key(),
            seen_host_key_hash: host.host_key_hash(),
        })
        .await
        .unwrap();

    let (_, view) = host.admin_json("GET", "/admin/pairing", None).await;
    assert_eq!(view["stage"], "awaiting_approval");
    assert_eq!(view["device_name"], "Unwanted");
    assert!(
        view["device_key_fingerprint"]
            .as_str()
            .unwrap()
            .starts_with("SHA256:")
    );

    let reject = json!({ "pairing_id": success.pairing_id });
    let (status, _) = host
        .admin_json("POST", "/admin/pairing/reject", Some(reject))
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let status_uri = format!("/api/pairing/{}", success.pairing_id);
    let (_, device_view) = host.send_json_as(None, "GET", &status_uri, None).await;
    assert_eq!(device_view["stage"], "rejected");

    let (_, devices) = host.admin_json("GET", "/admin/devices", None).await;
    assert_eq!(devices["devices"].as_array().unwrap().len(), 1);
}

// --- Login -----------------------------------------------------------------

#[tokio::test]
async fn challenges_cannot_be_replayed() {
    let host = TestHost::new().await;
    let (challenge_id, nonce) = host.get_challenge(&host.device.id).await;
    let message = protocol::login_message(&host.host_key_hash(), &host.device.id, &nonce);
    let signature = host.device.key.sign(&message).to_bytes();

    assert!(
        host.redeem_challenge(&challenge_id, &signature)
            .await
            .is_ok()
    );
    assert_eq!(
        host.redeem_challenge(&challenge_id, &signature).await,
        Err(StatusCode::UNAUTHORIZED)
    );
}

#[tokio::test]
async fn login_rejects_wrong_key_and_signatures_for_other_servers() {
    let host = TestHost::new().await;

    // Signed by a key that isn't the paired one.
    let (challenge_id, nonce) = host.get_challenge(&host.device.id).await;
    let message = protocol::login_message(&host.host_key_hash(), &host.device.id, &nonce);
    let forged = new_key().sign(&message).to_bytes();
    assert_eq!(
        host.redeem_challenge(&challenge_id, &forged).await,
        Err(StatusCode::UNAUTHORIZED)
    );

    // Right key, but signed for a different server's certificate.
    let (challenge_id, nonce) = host.get_challenge(&host.device.id).await;
    let message = protocol::login_message(&[0xCD; 32], &host.device.id, &nonce);
    let wrong_server = host.device.key.sign(&message).to_bytes();
    assert_eq!(
        host.redeem_challenge(&challenge_id, &wrong_server).await,
        Err(StatusCode::UNAUTHORIZED)
    );

    // Unknown device ID.
    let (challenge_id, _) = host.get_challenge("dev_nobody").await;
    assert_eq!(
        host.redeem_challenge(&challenge_id, &[0u8; 64]).await,
        Err(StatusCode::UNAUTHORIZED)
    );
}

#[tokio::test]
async fn logging_out_ends_the_session() {
    let host = TestHost::new().await;
    let (status, _) = host.send_json("DELETE", "/api/auth/session", None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, _) = host.get_json("/api/shares").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

// --- Revocation and permissions ---------------------------------------------

#[tokio::test]
async fn revoked_device_is_logged_out_and_cannot_log_in() {
    let host = TestHost::new().await;
    let uri = format!("/admin/devices/{}", host.device.id);
    let (status, _) = host.admin_json("DELETE", &uri, None).await;
    assert_eq!(status, StatusCode::OK);

    let (status, _) = host.get_json("/api/shares").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(
        host.log_in(&host.device).await,
        Err(StatusCode::UNAUTHORIZED)
    );
}

#[tokio::test]
async fn permissions_are_enforced_and_changes_apply_immediately() {
    let host = TestHost::new().await;
    let browser = host.pair_device("Browse Only", &["browse"]).await;
    let token = host.log_in(&browser).await.unwrap();

    let file_id = host.id_of("/", "report.pdf").await;
    let download = format!("/api/files/{file_id}/download");

    let (status, _) = host
        .send_json_as(Some(&token), "GET", &format!("/api/files/{file_id}"), None)
        .await;
    assert_eq!(status, StatusCode::OK, "browse is allowed");
    let (status, _) = host
        .send_json_as(Some(&token), "GET", &download, None)
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "download needs read");

    let grant = json!({ "permissions": ["browse", "read"] });
    let uri = format!("/admin/devices/{}", browser.id);
    let (status, _) = host.admin_json("PATCH", &uri, Some(grant)).await;
    assert_eq!(status, StatusCode::OK);

    let (status, _) = host
        .send_json_as(Some(&token), "GET", &download, None)
        .await;
    assert_eq!(status, StatusCode::OK, "same session, new permission");
}

#[tokio::test]
async fn approval_defaults_to_read_only() {
    let host = TestHost::new().await;
    let (_, code) = host.start_pairing().await;
    let success = host
        .device_pairing_attempt(PairingAttempt {
            code: &code,
            name: "Default",
            key: &new_key(),
            seen_host_key_hash: host.host_key_hash(),
        })
        .await
        .unwrap();

    let approve = json!({ "pairing_id": success.pairing_id });
    host.admin_json("POST", "/admin/pairing/approve", Some(approve))
        .await;

    let (_, devices) = host.admin_json("GET", "/admin/devices", None).await;
    let device = devices["devices"]
        .as_array()
        .unwrap()
        .iter()
        .find(|device| device["id"] == success.device_id.as_str())
        .unwrap();
    assert_eq!(device["permissions"], json!(["browse", "read"]));
}

// --- Admin API and rate limiting ---------------------------------------------

#[tokio::test]
async fn admin_api_requires_the_admin_token() {
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    let host = TestHost::new().await;

    let no_token = Request::get("/admin/devices").body(Body::empty()).unwrap();
    let response = host.admin.clone().oneshot(no_token).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    // A device's session token is not an admin token.
    let device_token = common::authed_get("/admin/devices", &host.token);
    let response = host.admin.clone().oneshot(device_token).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn admin_api_refuses_non_loopback_clients() {
    use axum::extract::connect_info::MockConnectInfo;
    use riftview_api::server::admin_app;
    use tower::ServiceExt;

    let host = TestHost::new().await;
    let remote: std::net::SocketAddr = "192.168.1.99:5000".parse().unwrap();
    let admin = admin_app(host.state.clone()).layer(MockConnectInfo(remote));

    let request = common::authed_get("/admin/devices", host.state.admin_token().as_str());
    let response = admin.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn pairing_and_login_are_rate_limited() {
    // Setup uses 5 limited requests (pairing 3 + login 2); allow a few more.
    let host = TestHost::with_config(Config {
        auth_rate_limit_per_minute: 8,
        ..Config::default()
    })
    .await;

    let mut statuses = Vec::new();
    for _ in 0..5 {
        let body = json!({ "device_id": host.device.id });
        let (status, _) = host
            .send_json_as(None, "POST", "/api/auth/challenge", Some(body))
            .await;
        statuses.push(status);
    }
    assert_eq!(&statuses[..3], &[StatusCode::OK; 3]);
    assert_eq!(&statuses[3..], &[StatusCode::TOO_MANY_REQUESTS; 2]);

    // Logged-in routes are not affected.
    let (status, _) = host.get_json("/api/shares").await;
    assert_eq!(status, StatusCode::OK);
}
