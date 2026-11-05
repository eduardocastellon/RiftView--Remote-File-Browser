use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::RwLock;

use crate::config::Config;
use crate::error::AppError;
use crate::filesystem::roots::{Share, ShareError, ShareStore};

/// Shared application state, cheap to clone.
#[derive(Clone)]
pub struct AppState(Arc<Inner>);

struct Inner {
    config: Config,
    data_dir: PathBuf,
    shares: RwLock<ShareStore>,
}

impl AppState {
    pub fn load(config: Config, data_dir: PathBuf) -> Result<Self, ShareError> {
        let shares = ShareStore::load(&data_dir)?;
        Ok(Self(Arc::new(Inner { config, data_dir, shares: RwLock::new(shares) })))
    }

    pub fn config(&self) -> &Config {
        &self.0.config
    }

    pub fn data_dir(&self) -> &std::path::Path {
        &self.0.data_dir
    }

    pub fn shares(&self) -> &RwLock<ShareStore> {
        &self.0.shares
    }

    /// Snapshot of one share, or 404.
    pub async fn share(&self, id: &str) -> Result<Share, AppError> {
        self.0.shares.read().await.get(id).cloned().ok_or(AppError::NotFound)
    }
}
