//! Shares (the folders the host user chose to expose), saved in the
//! `shares` table of the database.

use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use cap_std::ambient_authority;
use cap_std::fs::Dir;
use rusqlite::{Row, params};

use super::{FsError, RelPath};
use crate::database::{Database, DatabaseError};

const MAX_SHARE_NAME_LEN: usize = 128;

/// A folder the host user has explicitly chosen to expose.
#[derive(Debug, Clone)]
pub struct Share {
    /// Opaque ID such as `shr_3f2a…`.
    pub id: String,

    /// Display name chosen by the host user.
    pub name: String,

    /// Full real path on the host. Never sent to clients.
    pub path: PathBuf,

    /// Host-wide switch. When `false`, no device may change anything in
    /// this share, whatever its own permissions say.
    pub writable: bool,

    /// When the share was created, in Unix epoch milliseconds.
    pub created_at_ms: u64,
}

impl Share {
    /// Open a handle on the share's root folder. Every access to the share
    /// goes through this handle, which is what stops paths escaping it.
    pub fn open_root(&self) -> Result<Dir, FsError> {
        Dir::open_ambient_dir(&self.path, ambient_authority()).map_err(|error| match error.kind() {
            ErrorKind::NotFound | ErrorKind::NotADirectory => FsError::RootUnavailable,
            _ => FsError::from(error),
        })
    }

    /// Open a folder inside the share.
    pub fn open_dir(&self, path: &RelPath) -> Result<Dir, FsError> {
        let root = self.open_root()?;
        if path.is_root() {
            return Ok(root);
        }
        Ok(root.open_dir(path.to_cap_path())?)
    }

    /// Build a share from a row of `SELECT id, name, path, writable, created_at_ms`.
    fn from_row(row: &Row) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get("id")?,
            name: row.get("name")?,
            path: PathBuf::from(row.get::<_, String>("path")?),
            writable: row.get("writable")?,
            created_at_ms: row.get("created_at_ms")?,
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ShareError {
    #[error("share not found")]
    NotFound,

    #[error("{0}")]
    Invalid(&'static str),

    #[error("this directory (or one containing/inside it) is already shared")]
    Overlap,

    #[error(transparent)]
    Database(#[from] DatabaseError),
}

impl From<rusqlite::Error> for ShareError {
    fn from(error: rusqlite::Error) -> Self {
        ShareError::Database(error.into())
    }
}

/// Details for registering a new share.
pub struct NewShare {
    pub name: String,
    pub path: PathBuf,
    pub writable: bool,
}

/// Changes to an existing share. `None` means "leave as is".
#[derive(Default)]
pub struct SharePatch {
    pub name: Option<String>,
    pub writable: Option<bool>,
}

/// All registered shares.
///
/// The database is the record; `shares` is an in-memory copy so every
/// request can look shares up without touching the disk. Each change is
/// written to the database first and only applied to memory once that
/// succeeded, so the two never disagree.
pub struct ShareStore {
    database: Database,

    /// RiftView's own data folder. It must never be shared, because it
    /// holds the database, device keys and certificate.
    data_dir: PathBuf,

    shares: Vec<Share>,
}

impl ShareStore {
    pub fn load(database: Database, data_dir: &Path) -> Result<Self, DatabaseError> {
        let shares = {
            let connection = database.connection();
            let mut query = connection.prepare(
                "SELECT id, name, path, writable, created_at_ms FROM shares ORDER BY created_at_ms",
            )?;
            query
                .query_map([], Share::from_row)?
                .collect::<Result<Vec<_>, _>>()?
        };

        let data_dir = data_dir
            .canonicalize()
            .unwrap_or_else(|_| data_dir.to_path_buf());

        Ok(Self {
            database,
            data_dir,
            shares,
        })
    }

    pub fn list(&self) -> &[Share] {
        &self.shares
    }

    pub fn get(&self, id: &str) -> Option<&Share> {
        self.shares.iter().find(|share| share.id == id)
    }

    pub fn add(&mut self, new_share: NewShare) -> Result<Share, ShareError> {
        let name = validate_name(&new_share.name)?;
        let path = self.validate_new_share_path(&new_share.path)?;
        let path_text = path
            .to_str()
            .ok_or(ShareError::Invalid("path must be valid Unicode"))?;

        let share = Share {
            id: format!("shr_{}", uuid::Uuid::new_v4().simple()),
            name,
            path: path.clone(),
            writable: new_share.writable,
            created_at_ms: now_ms(),
        };

        self.database.connection().execute(
            "INSERT INTO shares (id, name, path, writable, created_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                share.id,
                share.name,
                path_text,
                share.writable,
                share.created_at_ms
            ],
        )?;

        self.shares.push(share.clone());
        Ok(share)
    }

    pub fn update(&mut self, id: &str, patch: SharePatch) -> Result<Share, ShareError> {
        let index = self.index_of(id)?;

        let mut updated = self.shares[index].clone();
        if let Some(name) = patch.name {
            updated.name = validate_name(&name)?;
        }
        if let Some(writable) = patch.writable {
            updated.writable = writable;
        }

        self.database.connection().execute(
            "UPDATE shares SET name = ?1, writable = ?2 WHERE id = ?3",
            params![updated.name, updated.writable, id],
        )?;

        self.shares[index] = updated.clone();
        Ok(updated)
    }

    pub fn remove(&mut self, id: &str) -> Result<(), ShareError> {
        let index = self.index_of(id)?;

        self.database
            .connection()
            .execute("DELETE FROM shares WHERE id = ?1", params![id])?;

        self.shares.remove(index);
        Ok(())
    }

    /// Check a proposed share folder and return its canonical path.
    fn validate_new_share_path(&self, requested: &Path) -> Result<PathBuf, ShareError> {
        if !requested.is_absolute() {
            return Err(ShareError::Invalid("path must be absolute"));
        }

        // Resolve symlinks and `..` so we compare real locations below.
        let canonical = requested
            .canonicalize()
            .map_err(|_| ShareError::Invalid("path does not exist or is not accessible"))?;

        if !canonical.is_dir() {
            return Err(ShareError::Invalid("path is not a directory"));
        }

        if one_contains_the_other(&canonical, &self.data_dir) {
            return Err(ShareError::Invalid(
                "cannot share the application data directory",
            ));
        }

        // Nested shares would let one share's permissions bypass another's.
        let overlaps_existing = self
            .shares
            .iter()
            .any(|existing| one_contains_the_other(&existing.path, &canonical));
        if overlaps_existing {
            return Err(ShareError::Overlap);
        }

        Ok(canonical)
    }

    fn index_of(&self, id: &str) -> Result<usize, ShareError> {
        self.shares
            .iter()
            .position(|share| share.id == id)
            .ok_or(ShareError::NotFound)
    }
}

fn validate_name(name: &str) -> Result<String, ShareError> {
    let name = name.trim();

    let is_valid =
        !name.is_empty() && name.len() <= MAX_SHARE_NAME_LEN && !name.chars().any(char::is_control);

    if !is_valid {
        return Err(ShareError::Invalid(
            "name must be 1-128 printable characters",
        ));
    }
    Ok(name.to_owned())
}

/// True if either folder is inside the other (or they're the same).
///
/// `Path::starts_with` compares whole path components, so `/a/bc` is
/// correctly *not* treated as inside `/a/b`.
fn one_contains_the_other(a: &Path, b: &Path) -> bool {
    a.starts_with(b) || b.starts_with(a)
}

/// Current time in Unix epoch milliseconds.
pub(crate) fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture {
        data_dir: tempfile::TempDir,
        files_dir: tempfile::TempDir,
        database: Database,
        store: ShareStore,
    }

    fn fixture() -> Fixture {
        let data_dir = tempfile::tempdir().unwrap();
        let files_dir = tempfile::tempdir().unwrap();
        let database = Database::open(data_dir.path()).unwrap();
        let store = ShareStore::load(database.clone(), data_dir.path()).unwrap();
        Fixture {
            data_dir,
            files_dir,
            database,
            store,
        }
    }

    fn reload(fx: &Fixture) -> ShareStore {
        ShareStore::load(fx.database.clone(), fx.data_dir.path()).unwrap()
    }

    fn new_share(name: &str, path: &Path) -> NewShare {
        NewShare {
            name: name.into(),
            path: path.to_path_buf(),
            writable: false,
        }
    }

    #[test]
    fn add_update_reload_remove() {
        let mut fx = fixture();
        let share = fx
            .store
            .add(new_share("Docs", fx.files_dir.path()))
            .unwrap();
        assert!(share.id.starts_with("shr_"));

        let reloaded = reload(&fx);
        assert_eq!(reloaded.list().len(), 1);
        assert_eq!(reloaded.get(&share.id).unwrap().name, "Docs");
        assert_eq!(reloaded.get(&share.id).unwrap().path, share.path);

        let patch = SharePatch {
            name: Some("Documents".into()),
            writable: Some(true),
        };
        fx.store.update(&share.id, patch).unwrap();
        let reloaded = reload(&fx);
        let saved = reloaded.get(&share.id).unwrap();
        assert_eq!(saved.name, "Documents");
        assert!(saved.writable);

        fx.store.remove(&share.id).unwrap();
        let reloaded = reload(&fx);
        assert!(reloaded.list().is_empty());
    }

    #[test]
    fn rejects_bad_shares() {
        let mut fx = fixture();
        let files = fx.files_dir.path();

        assert!(
            fx.store
                .add(new_share("x", Path::new("relative/dir")))
                .is_err()
        );
        assert!(
            fx.store
                .add(new_share("x", &files.join("missing")))
                .is_err()
        );
        assert!(fx.store.add(new_share("", files)).is_err());
        assert!(fx.store.add(new_share("data", fx.data_dir.path())).is_err());

        std::fs::create_dir(files.join("sub")).unwrap();
        fx.store.add(new_share("root", files)).unwrap();
        let nested = fx.store.add(new_share("sub", &files.join("sub")));
        assert!(matches!(nested, Err(ShareError::Overlap)));
    }
}
