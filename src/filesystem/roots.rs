//! Shared-root definitions and their persisted registry.

use std::path::{Path, PathBuf};

use cap_std::ambient_authority;
use cap_std::fs::Dir;
use serde::{Deserialize, Serialize};

use super::{FsError, RelPath};
use crate::storage;

pub const SHARES_SCHEMA_VERSION: u32 = 1;

/// A directory the host user has explicitly chosen to expose.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Share {
    /// Opaque identifier, e.g. `shr_3f2a…`.
    pub id: String,
    pub name: String,
    /// Canonical absolute host path. Never sent to clients.
    pub path: PathBuf,
    /// Host-level ceiling: when false, no device may modify this share,
    /// regardless of its own permissions.
    pub writable: bool,
    pub created_at_ms: u64,
}

impl Share {
    /// Open a capability handle on the share root. All access to the share
    /// goes through this handle (or handles derived from it).
    pub fn open_root(&self) -> Result<Dir, FsError> {
        Dir::open_ambient_dir(&self.path, ambient_authority()).map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory => {
                FsError::RootUnavailable
            }
            _ => FsError::from(e),
        })
    }

    /// Open a directory inside the share.
    pub fn open_dir(&self, rel: &RelPath) -> Result<Dir, FsError> {
        let root = self.open_root()?;
        if rel.is_root() {
            return Ok(root);
        }
        Ok(root.open_dir(rel.to_cap_path())?)
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct SharesFile {
    schema_version: u32,
    shares: Vec<Share>,
}

#[derive(Debug, thiserror::Error)]
pub enum ShareError {
    #[error("share not found")]
    NotFound,
    #[error("{0}")]
    Invalid(&'static str),
    #[error("this directory (or one containing/inside it) is already shared")]
    Overlap,
    #[error("unsupported shares.json schema_version {0}")]
    UnsupportedVersion(u32),
    #[error("share storage I/O: {0}")]
    Io(#[from] std::io::Error),
}

/// In-memory registry of shares, persisted to `shares.json` on every change.
#[derive(Debug)]
pub struct ShareStore {
    file: PathBuf,
    /// Directory that must never be exposed (holds device keys, config).
    data_dir: PathBuf,
    shares: Vec<Share>,
}

pub struct NewShare {
    pub name: String,
    pub path: PathBuf,
    pub writable: bool,
}

#[derive(Default)]
pub struct SharePatch {
    pub name: Option<String>,
    pub writable: Option<bool>,
}

impl ShareStore {
    pub fn load(data_dir: &Path) -> Result<Self, ShareError> {
        let file = data_dir.join("shared_directories.json");
        let shares = match storage::read_json::<SharesFile>(&file)? {
            Some(f) if f.schema_version != SHARES_SCHEMA_VERSION => {
                return Err(ShareError::UnsupportedVersion(f.schema_version));
            }
            Some(f) => f.shares,
            None => Vec::new(),
        };
        let data_dir = data_dir.canonicalize().unwrap_or_else(|_| data_dir.to_path_buf());
        Ok(Self { file, data_dir, shares })
    }

    pub fn list(&self) -> &[Share] {
        &self.shares
    }

    pub fn get(&self, id: &str) -> Option<&Share> {
        self.shares.iter().find(|s| s.id == id)
    }

    pub fn add(&mut self, new: NewShare) -> Result<Share, ShareError> {
        let name = validate_name(&new.name)?;
        if !new.path.is_absolute() {
            return Err(ShareError::Invalid("path must be absolute"));
        }
        let canonical = new
            .path
            .canonicalize()
            .map_err(|_| ShareError::Invalid("path does not exist or is not accessible"))?;
        if !canonical.is_dir() {
            return Err(ShareError::Invalid("path is not a directory"));
        }
        if overlaps(&canonical, &self.data_dir) {
            return Err(ShareError::Invalid("cannot share the application data directory"));
        }
        // Nested shares would let one share's permissions bypass another's.
        if self.shares.iter().any(|s| overlaps(&s.path, &canonical)) {
            return Err(ShareError::Overlap);
        }

        let share = Share {
            id: format!("shr_{}", uuid::Uuid::new_v4().simple()),
            name,
            path: canonical,
            writable: new.writable,
            created_at_ms: now_ms(),
        };
        self.shares.push(share.clone());
        if let Err(e) = self.persist() {
            self.shares.pop();
            return Err(e);
        }
        Ok(share)
    }

    pub fn update(&mut self, id: &str, patch: SharePatch) -> Result<Share, ShareError> {
        let idx = self.index_of(id)?;
        let before = self.shares[idx].clone();
        if let Some(name) = patch.name {
            self.shares[idx].name = validate_name(&name)?;
        }
        if let Some(w) = patch.writable {
            self.shares[idx].writable = w;
        }
        if let Err(e) = self.persist() {
            self.shares[idx] = before;
            return Err(e);
        }
        Ok(self.shares[idx].clone())
    }

    pub fn remove(&mut self, id: &str) -> Result<(), ShareError> {
        let idx = self.index_of(id)?;
        let removed = self.shares.remove(idx);
        if let Err(e) = self.persist() {
            self.shares.insert(idx, removed);
            return Err(e);
        }
        Ok(())
    }

    fn index_of(&self, id: &str) -> Result<usize, ShareError> {
        self.shares.iter().position(|s| s.id == id).ok_or(ShareError::NotFound)
    }

    fn persist(&self) -> Result<(), ShareError> {
        let doc = SharesFile { schema_version: SHARES_SCHEMA_VERSION, shares: self.shares.clone() };
        Ok(storage::write_json_atomic(&self.file, &doc)?)
    }
}

fn validate_name(name: &str) -> Result<String, ShareError> {
    let name = name.trim();
    if name.is_empty() || name.len() > 128 || name.chars().any(char::is_control) {
        return Err(ShareError::Invalid("name must be 1-128 printable characters"));
    }
    Ok(name.to_owned())
}

/// True if either path contains the other (component-wise, not string-prefix).
fn overlaps(a: &Path, b: &Path) -> bool {
    a.starts_with(b) || b.starts_with(a)
}

pub(crate) fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (tempfile::TempDir, tempfile::TempDir, ShareStore) {
        let data = tempfile::tempdir().unwrap();
        let files = tempfile::tempdir().unwrap();
        let s = ShareStore::load(data.path()).unwrap();
        (data, files, s)
    }

    fn new(name: &str, path: &Path) -> NewShare {
        NewShare { name: name.into(), path: path.to_path_buf(), writable: false }
    }

    #[test]
    fn add_persist_reload_remove() {
        let (data, files, mut s) = store();
        let share = s.add(new("Docs", files.path())).unwrap();
        assert!(share.id.starts_with("shr_"));

        let reloaded = ShareStore::load(data.path()).unwrap();
        assert_eq!(reloaded.list().len(), 1);
        assert_eq!(reloaded.get(&share.id).unwrap().name, "Docs");

        s.remove(&share.id).unwrap();
        assert!(ShareStore::load(data.path()).unwrap().list().is_empty());
    }

    #[test]
    fn rejects_bad_shares() {
        let (data, files, mut s) = store();
        assert!(s.add(new("x", Path::new("relative/dir"))).is_err());
        assert!(s.add(new("x", &files.path().join("missing"))).is_err());
        assert!(s.add(new("", files.path())).is_err());
        assert!(s.add(new("data", data.path())).is_err());

        std::fs::create_dir(files.path().join("sub")).unwrap();
        s.add(new("root", files.path())).unwrap();
        assert!(matches!(s.add(new("sub", &files.path().join("sub"))), Err(ShareError::Overlap)));
    }
}
