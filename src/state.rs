//! Data shared by every request handler.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio::sync::RwLock;

use crate::config::Config;
use crate::error::AppError;
use crate::filesystem::roots::{Share, ShareError, ShareStore};

/// Shared application state. Cloning is cheap: every clone points at the
/// same data.
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

        Ok(Self(Arc::new(Inner {
            config,
            data_dir,
            shares: RwLock::new(shares),
        })))
    }

    pub fn config(&self) -> &Config {
        &self.0.config
    }

    pub fn data_dir(&self) -> &Path {
        &self.0.data_dir
    }

    pub fn shares(&self) -> &RwLock<ShareStore> {
        &self.0.shares
    }

    /// A copy of one share's details, or a 404 error if it doesn't exist.
    pub async fn share(&self, id: &str) -> Result<Share, AppError> {
        let shares = self.0.shares.read().await;
        shares.get(id).cloned().ok_or(AppError::NotFound)
    }
}
