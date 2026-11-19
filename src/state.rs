//! Data shared by every request handler.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::RwLock;

use crate::admin::token::AdminToken;
use crate::auth::devices::DeviceStore;
use crate::auth::pairing::PairingManager;
use crate::auth::rate_limit::RateLimiter;
use crate::auth::sessions::SessionManager;
use crate::config::Config;
use crate::database::{Database, DatabaseError};
use crate::error::AppError;
use crate::filesystem::roots::{Share, ShareStore};
use crate::tls::{HostIdentity, TlsError};
use crate::transfers::hashing::HashCache;
use crate::transfers::limits::TransferLimiter;
use crate::transfers::uploads::UploadStore;

/// Shared application state. Cloning is cheap: every clone points at the
/// same data.
#[derive(Clone)]
pub struct AppState(Arc<Inner>);

struct Inner {
    config: Config,
    data_dir: PathBuf,
    identity: HostIdentity,
    admin_token: AdminToken,
    shares: RwLock<ShareStore>,
    devices: RwLock<DeviceStore>,
    pairing: PairingManager,
    sessions: SessionManager,
    auth_rate_limiter: RateLimiter,
    transfer_limiter: TransferLimiter,
    hash_cache: HashCache,
    uploads: UploadStore,
}

#[derive(Debug, thiserror::Error)]
pub enum StateError {
    #[error(transparent)]
    Database(#[from] DatabaseError),
    #[error(transparent)]
    Tls(#[from] TlsError),
    #[error("admin token: {0}")]
    AdminToken(#[from] std::io::Error),
}

impl AppState {
    /// Load (or create on first run) everything kept in the data folder.
    pub fn load(config: Config, data_dir: PathBuf) -> Result<Self, StateError> {
        let database = Database::open(&data_dir)?;
        let shares = ShareStore::load(database.clone(), &data_dir)?;
        let devices = DeviceStore::load(database.clone())?;
        let uploads = UploadStore::new(database);
        let identity = HostIdentity::load_or_create(&data_dir)?;
        let admin_token = AdminToken::load_or_create(&data_dir)?;
        let sessions = SessionManager::new(Duration::from_secs(config.session_lifetime_secs));
        let auth_rate_limiter = RateLimiter::new(config.auth_rate_limit_per_minute);
        let transfer_limiter = TransferLimiter::new(
            config.max_concurrent_transfers,
            config.max_transfers_per_device,
        );

        Ok(Self(Arc::new(Inner {
            config,
            data_dir,
            identity,
            admin_token,
            shares: RwLock::new(shares),
            devices: RwLock::new(devices),
            pairing: PairingManager::default(),
            sessions,
            auth_rate_limiter,
            transfer_limiter,
            hash_cache: HashCache::default(),
            uploads,
        })))
    }

    pub fn config(&self) -> &Config {
        &self.0.config
    }

    pub fn data_dir(&self) -> &Path {
        &self.0.data_dir
    }

    pub fn identity(&self) -> &HostIdentity {
        &self.0.identity
    }

    pub fn admin_token(&self) -> &AdminToken {
        &self.0.admin_token
    }

    pub fn shares(&self) -> &RwLock<ShareStore> {
        &self.0.shares
    }

    pub fn devices(&self) -> &RwLock<DeviceStore> {
        &self.0.devices
    }

    pub fn pairing(&self) -> &PairingManager {
        &self.0.pairing
    }

    pub fn sessions(&self) -> &SessionManager {
        &self.0.sessions
    }

    pub fn auth_rate_limiter(&self) -> &RateLimiter {
        &self.0.auth_rate_limiter
    }

    pub fn transfer_limiter(&self) -> &TransferLimiter {
        &self.0.transfer_limiter
    }

    pub fn hash_cache(&self) -> &HashCache {
        &self.0.hash_cache
    }

    pub fn uploads(&self) -> &UploadStore {
        &self.0.uploads
    }

    /// A copy of one share's details, or a 404 error if it doesn't exist.
    pub async fn share(&self, id: &str) -> Result<Share, AppError> {
        let shares = self.0.shares.read().await;
        shares.get(id).cloned().ok_or(AppError::NotFound)
    }
}
