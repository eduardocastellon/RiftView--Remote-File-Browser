//! Host configuration, persisted as `config.json` in the data directory.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::storage;

pub const CONFIG_SCHEMA_VERSION: u32 = 1;
pub const BIND_ENV: &str = "RIFTVIEW_BIND";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub schema_version: u32,
    /// Address to listen on. Until device authentication and TLS land
    /// (phase 4), only loopback addresses are accepted.
    pub bind: SocketAddr,
    /// Friendly name shown to clients.
    pub host_name: String,
    /// Hard cap on `limit` for directory listings.
    pub max_list_limit: usize,
    /// Directories with more entries than this are rejected rather than
    /// sorted in memory.
    pub max_dir_entries: usize,
    /// Time allowed to produce response headers. Does not bound streaming
    /// download bodies.
    pub request_timeout_secs: u64,
    /// Max size of a non-transfer request body (JSON etc.).
    pub max_request_body_bytes: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            schema_version: CONFIG_SCHEMA_VERSION,
            bind: SocketAddr::from(([127, 0, 0, 1], 7878)),
            host_name: default_host_name(),
            max_list_limit: 1000,
            max_dir_entries: 100_000,
            request_timeout_secs: 30,
            max_request_body_bytes: 64 * 1024,
        }
    }
}

fn default_host_name() -> String {
    std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("COMPUTERNAME"))
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
    /// Load `config.json` from `data_dir`, writing defaults if missing, then
    /// apply environment overrides (not persisted).
    pub fn load_or_init(data_dir: &Path) -> Result<Self, ConfigError> {
        let path = config_path(data_dir);
        let mut cfg = match storage::read_json::<Config>(&path)? {
            Some(cfg) => cfg,
            None => {
                let cfg = Config::default();
                storage::write_json_atomic(&path, &cfg)?;
                cfg
            }
        };
        if cfg.schema_version != CONFIG_SCHEMA_VERSION {
            return Err(ConfigError::UnsupportedVersion(cfg.schema_version));
        }
        if let Ok(bind) = std::env::var(BIND_ENV) {
            cfg.bind = bind.parse().map_err(|_| ConfigError::BadBindEnv(bind))?;
        }
        Ok(cfg)
    }
}

pub fn config_path(data_dir: &Path) -> PathBuf {
    data_dir.join("config.json")
}
