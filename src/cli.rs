//! Commands the host user runs in a terminal (`riftview-api pair` etc.).
//!
//! They talk to the running server through the admin API, authenticating
//! with the admin token from the data folder.

use std::io::{BufRead, Write};
use std::path::Path;
use std::thread::sleep;
use std::time::Duration;

use serde_json::{Value, json};

use crate::admin::token::AdminToken;
use crate::config::Config;
use crate::platform;

const POLL_INTERVAL: Duration = Duration::from_secs(1);

type CliResult = Result<(), String>;

/// A small client for the admin API.
struct AdminClient {
    base_url: String,
    token: String,
    agent: ureq::Agent,
}

impl AdminClient {
    fn connect() -> Result<Self, String> {
        let data_dir = platform::data_dir()
            .ok_or("could not find the RiftView data folder; set RIFTVIEW_DATA_DIR")?;
        let config = Config::load_or_init(&data_dir).map_err(|error| error.to_string())?;
        let token = AdminToken::read(&data_dir)
            .map_err(|error| error.to_string())?
            .ok_or("no admin token found; start the server first with `riftview-api serve`")?;

        // Treat 4xx/5xx as normal responses so we can show the server's message.
        let agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .build()
            .into();

        Ok(Self {
            base_url: format!("http://{}", config.admin_bind),
            token: token.as_str().to_owned(),
            agent,
        })
    }

    fn get(&self, path: &str) -> Result<(u16, Value), String> {
        let request = self.agent.get(format!("{}{path}", self.base_url));
        let response = request
            .header("Authorization", format!("Bearer {}", self.token))
            .call();
        read_response(response)
    }

    fn send(&self, method: &str, path: &str, body: Option<Value>) -> Result<(u16, Value), String> {
        let url = format!("{}{path}", self.base_url);
        let auth = format!("Bearer {}", self.token);

        let response = match (method, body) {
            ("POST", Some(body)) => self
                .agent
                .post(url)
                .header("Authorization", auth)
                .send_json(body),
            ("POST", None) => self
                .agent
                .post(url)
                .header("Authorization", auth)
                .send_empty(),
            ("DELETE", _) => self.agent.delete(url).header("Authorization", auth).call(),
            _ => return Err(format!("unsupported method {method}")),
        };
        read_response(response)
    }
}

fn read_response(
    response: Result<ureq::http::Response<ureq::Body>, ureq::Error>,
) -> Result<(u16, Value), String> {
    let mut response = response.map_err(|error| {
        format!("could not reach the RiftView server (is it running?): {error}")
    })?;
    let status = response.status().as_u16();
    let body = response
        .body_mut()
        .read_json::<Value>()
        .unwrap_or(Value::Null);
    Ok((status, body))
}

/// Turn a non-2xx response into an error showing the server's message.
fn expect_success((status, body): (u16, Value)) -> Result<Value, String> {
    if (200..300).contains(&status) {
        return Ok(body);
    }
    let message = body["error"]["message"]
        .as_str()
        .unwrap_or("request failed");
    Err(format!("{message} (HTTP {status})"))
}

// ---------------------------------------------------------------------------
// Pairing
// ---------------------------------------------------------------------------

pub fn pair() -> CliResult {
    let client = AdminClient::connect()?;
    let started = expect_success(client.send("POST", "/admin/pairing", None)?)?;

    let pairing_id = started["pairing_id"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    let code = started["code"].as_str().unwrap_or_default();
    let minutes = started["expires_in_secs"].as_u64().unwrap_or(300) / 60;

    println!();
    println!("  Pairing code:  {} {}", &code[..3], &code[3..]);
    println!();
    println!("Enter this code in the RiftView app. It expires in {minutes} minutes.");
    println!("Waiting for a device… (Ctrl-C to cancel)");

    let waiting = wait_for_device(&client, &pairing_id)?;

    println!();
    println!(
        "\"{}\" wants to pair.",
        waiting["device_name"].as_str().unwrap_or("Unknown device")
    );
    println!(
        "Key fingerprint: {}",
        waiting["device_key_fingerprint"].as_str().unwrap_or("?")
    );
    println!();

    match ask_access_level()? {
        Some(permissions) => {
            let body = json!({ "pairing_id": pairing_id, "permissions": permissions });
            let approved =
                expect_success(client.send("POST", "/admin/pairing/approve", Some(body))?)?;
            println!(
                "Paired. Device ID: {}",
                approved["device_id"].as_str().unwrap_or("?")
            );
        }
        None => {
            let body = json!({ "pairing_id": pairing_id });
            expect_success(client.send("POST", "/admin/pairing/reject", Some(body))?)?;
            println!("Rejected.");
        }
    }
    Ok(())
}

/// Poll until a device has proved it knows the code, or pairing ends.
fn wait_for_device(client: &AdminClient, pairing_id: &str) -> Result<Value, String> {
    loop {
        sleep(POLL_INTERVAL);
        let view = expect_success(client.get("/admin/pairing")?)
            .map_err(|_| "pairing was cancelled".to_string())?;

        if view["pairing_id"] != pairing_id {
            return Err("pairing was replaced by a newer one".into());
        }
        match view["stage"].as_str().unwrap_or_default() {
            "awaiting_approval" => return Ok(view),
            "failed" => {
                return Err(
                    "pairing failed: wrong code, or the connection was tampered with. \
                            Run `riftview-api pair` again."
                        .into(),
                );
            }
            "expired" => return Err("the code expired. Run `riftview-api pair` again.".into()),
            _ => continue,
        }
    }
}

/// Ask read-only / full / no. Returns the permission list, or `None` to reject.
fn ask_access_level() -> Result<Option<Vec<&'static str>>, String> {
    let stdin = std::io::stdin();
    loop {
        print!("Allow it?  [r] read-only   [f] full access   [n] no  > ");
        std::io::stdout().flush().ok();

        let mut answer = String::new();
        stdin
            .lock()
            .read_line(&mut answer)
            .map_err(|error| error.to_string())?;

        match answer.trim().to_lowercase().as_str() {
            "r" => return Ok(Some(vec!["browse", "read"])),
            "f" => {
                return Ok(Some(vec![
                    "browse", "read", "upload", "rename", "move", "delete",
                ]));
            }
            "n" | "" => return Ok(None),
            _ => println!("Please type r, f or n."),
        }
    }
}

// ---------------------------------------------------------------------------
// Info, devices, shares
// ---------------------------------------------------------------------------

pub fn info() -> CliResult {
    let client = AdminClient::connect()?;
    let host = expect_success(client.get("/admin/host")?)?;

    println!("Name:             {}", host["name"].as_str().unwrap_or("?"));
    println!(
        "Version:          {}",
        host["version"].as_str().unwrap_or("?")
    );
    println!(
        "Device API:       https://{}",
        host["bind"].as_str().unwrap_or("?")
    );
    println!(
        "Certificate pin:  {}",
        host["certificate_pin"].as_str().unwrap_or("?")
    );
    Ok(())
}

pub fn list_devices() -> CliResult {
    let client = AdminClient::connect()?;
    let list = expect_success(client.get("/admin/devices")?)?;
    let devices = list["devices"].as_array().cloned().unwrap_or_default();

    if devices.is_empty() {
        println!("No paired devices. Pair one with `riftview-api pair`.");
        return Ok(());
    }
    for device in devices {
        let status = if device["revoked_at_ms"].is_null() {
            "active"
        } else {
            "REVOKED"
        };
        let permissions: Vec<&str> = device["permissions"]
            .as_array()
            .map(|list| list.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();

        println!(
            "{}  {:<24} {:<8} {}",
            device["id"].as_str().unwrap_or("?"),
            device["name"].as_str().unwrap_or("?"),
            status,
            permissions.join(","),
        );
    }
    Ok(())
}

pub fn revoke_device(device_id: &str) -> CliResult {
    let client = AdminClient::connect()?;
    expect_success(client.send("DELETE", &format!("/admin/devices/{device_id}"), None)?)?;
    println!("Revoked {device_id}. It has been logged out and can't reconnect.");
    Ok(())
}

pub fn list_shares() -> CliResult {
    let client = AdminClient::connect()?;
    let list = expect_success(client.get("/admin/shares")?)?;
    let shares = list["shares"].as_array().cloned().unwrap_or_default();

    if shares.is_empty() {
        println!("Nothing shared yet. Add a folder with `riftview-api share add <name> <path>`.");
        return Ok(());
    }
    for share in shares {
        let access = if share["writable"] == true {
            "writable"
        } else {
            "read-only"
        };
        println!(
            "{}  {:<20} {:<10} {}",
            share["id"].as_str().unwrap_or("?"),
            share["name"].as_str().unwrap_or("?"),
            access,
            share["path"].as_str().unwrap_or("?"),
        );
    }
    Ok(())
}

pub fn add_share(name: &str, path: &Path, writable: bool) -> CliResult {
    // Accept relative paths on the command line for convenience.
    let absolute = std::path::absolute(path).map_err(|error| error.to_string())?;

    let client = AdminClient::connect()?;
    let body = json!({ "name": name, "path": absolute, "writable": writable });
    let share = expect_success(client.send("POST", "/admin/shares", Some(body))?)?;
    println!(
        "Shared \"{}\" as {}.",
        share["name"].as_str().unwrap_or(name),
        share["id"].as_str().unwrap_or("?")
    );
    Ok(())
}

pub fn remove_share(share_id: &str) -> CliResult {
    let client = AdminClient::connect()?;
    expect_success(client.send("DELETE", &format!("/admin/shares/{share_id}"), None)?)?;
    println!("Stopped sharing {share_id}. The folder itself was not changed.");
    Ok(())
}
