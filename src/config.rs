//! Host settings, saved as `config.json` in the data folder.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::filesystem::trash::TrashBin;
use crate::storage;

/// Bump this when the layout of `config.json` changes.
pub const CONFIG_SCHEMA_VERSION: u32 = 1;

/// Set this environment variable (e.g. `127.0.0.1:9000`) to override the
/// listen address without editing `config.json`.
pub const BIND_ENV: &str = "RIFTVIEW_BIND";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)] // Any missing field in config.json falls back to its default.
pub struct Config {
    pub schema_version: u32,

    /// Address and port for the HTTPS API that devices connect to.
    /// `0.0.0.0` means every network interface: your LAN, and VPNs such as
    /// Tailscale, Radmin or Hamachi.
    pub bind: SocketAddr,

    /// Address for the host-only admin API (used by `riftview-api pair`,
    /// `share`, `devices`…). Must be a loopback address.
    pub admin_bind: SocketAddr,

    /// How long a device stays logged in before signing a new challenge.
    pub session_lifetime_secs: u64,

    /// Pairing and login requests allowed per IP address per minute.
    pub auth_rate_limit_per_minute: u32,

    /// Friendly name shown to connecting devices.
    pub host_name: String,

    /// The most entries a client may request in one page of a folder listing.
    pub max_list_limit: usize,

    /// Folders with more entries than this are refused rather than loaded
    /// into memory for sorting.
    pub max_dir_entries: usize,

    /// How long a request may take before the response starts. This does
    /// not limit how long a download may take to stream.
    pub request_timeout_secs: u64,

    /// Largest allowed request body for ordinary (non-upload) requests.
    pub max_request_body_bytes: usize,

    /// Transfers that may run at once across all devices: downloads, hash
    /// requests, upload chunks being received and uploads being finished.
    /// Extra requests get `429 too_many_requests`.
    #[serde(alias = "max_concurrent_downloads")] // Name before uploads existed.
    pub max_concurrent_transfers: usize,

    /// Transfers (as above) one device may run at once.
    #[serde(alias = "max_downloads_per_device")] // Name before uploads existed.
    pub max_transfers_per_device: usize,

    /// Largest file a device may upload, in bytes.
    pub max_upload_bytes: u64,

    /// Unfinished uploads one device may have open at once.
    pub max_uploads_per_device: usize,

    /// Unfinished uploads open at once across all devices.
    pub max_uploads_total: usize,

    /// An unfinished upload with no new chunk for this long is deleted,
    /// along with its partial file.
    pub upload_expiry_secs: u64,

    /// How long one upload chunk request may take, including receiving its
    /// body. Longer than `request_timeout_secs` because a 4 MiB chunk over
    /// a slow mobile connection can take a while.
    pub chunk_upload_timeout_secs: u64,

    /// **Tests only:** where deleted items go instead of the system Trash.
    /// Not read from or written to `config.json`: on a real host, deletes
    /// always go to the disk's Trash.
    #[serde(skip)]
    pub trash_for_tests: Option<TrashBin>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            schema_version: CONFIG_SCHEMA_VERSION,
            bind: SocketAddr::from(([0, 0, 0, 0], 7878)),
            admin_bind: SocketAddr::from(([127, 0, 0, 1], 7879)),
            session_lifetime_secs: 60 * 60,
            auth_rate_limit_per_minute: 20,
            host_name: default_host_name(),
            max_list_limit: 1000,
            max_dir_entries: 100_000,
            request_timeout_secs: 30,
            max_request_body_bytes: 64 * 1024,
            max_concurrent_transfers: 16,
            max_transfers_per_device: 4,
            max_upload_bytes: 10 * 1024 * 1024 * 1024, // 10 GiB
            max_uploads_per_device: 4,
            max_uploads_total: 32,
            upload_expiry_secs: 24 * 60 * 60,
            chunk_upload_timeout_secs: 5 * 60,
            trash_for_tests: None,
        }
    }
}

fn default_host_name() -> String {
    std::env::var("HOSTNAME") // macOS / Linux
        .or_else(|_| std::env::var("COMPUTERNAME")) // Windows
        .unwrap_or_else(|_| "RiftView Host".to_string())
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("could not determine application data directory; set RIFTVIEW_DATA_DIR")]
    NoDataDir,

    #[error("unsupported config schema_version {0} (expected {CONFIG_SCHEMA_VERSION})")]
    UnsupportedVersion(u32),

    #[error("invalid {BIND_ENV} value: {0}")]
    BadBindEnv(String),

    #[error("config I/O: {0}")]
    Io(#[from] std::io::Error),
}

impl Config {
    /// Load `config.json` from the data folder, creating it with default
    /// values on first run. Environment variable overrides are applied
    /// afterwards and are not saved back to the file.
    pub fn load_or_init(data_dir: &Path) -> Result<Self, ConfigError> {
        let path = config_path(data_dir);

        let mut config = match storage::read_json::<Config>(&path)? {
            Some(saved) => saved,
            None => {
                let defaults = Config::default();
                storage::write_json_atomic(&path, &defaults)?;
                defaults
            }
        };

        if config.schema_version != CONFIG_SCHEMA_VERSION {
            return Err(ConfigError::UnsupportedVersion(config.schema_version));
        }

        if let Ok(bind_override) = std::env::var(BIND_ENV) {
            config.bind = bind_override
                .parse()
                .map_err(|_| ConfigError::BadBindEnv(bind_override))?;
        }

        Ok(config)
    }
}

pub fn config_path(data_dir: &Path) -> PathBuf {
    data_dir.join("config.json")
}
