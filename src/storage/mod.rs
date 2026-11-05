//! Small local state files (no database). Every write is atomic: write to a
//! temp file in the same directory, fsync, then rename over the target.

use std::io::Write;
use std::path::Path;

use serde::{Serialize, de::DeserializeOwned};

use crate::platform;

/// Read a JSON file. Returns `Ok(None)` if it does not exist.
pub fn read_json<T: DeserializeOwned>(path: &Path) -> std::io::Result<Option<T>> {
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// Atomically replace `path` with the JSON encoding of `value`.
pub fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> std::io::Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| std::io::Error::other("state file has no parent directory"))?;
    let bytes = serde_json::to_vec_pretty(value).map_err(std::io::Error::other)?;

    let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("state");
    let tmp = dir.join(format!(".{file_name}.{}.tmp", uuid::Uuid::new_v4().simple()));

    let result = (|| {
        let mut f = std::fs::OpenOptions::new().write(true).create_new(true).open(&tmp)?;
        platform::restrict_file_permissions(&tmp)?;
        f.write_all(&bytes)?;
        f.sync_all()?;
        drop(f);
        std::fs::rename(&tmp, path)?;
        // Persist the rename itself (directory entry) on Unix.
        #[cfg(unix)]
        std::fs::File::open(dir)?.sync_all()?;
        Ok(())
    })();

    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

/// Create the data directory (owner-only) if needed.
pub fn ensure_data_dir(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    platform::restrict_dir_permissions(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Serialize, serde::Deserialize, PartialEq, Debug)]
    struct Doc {
        n: u32,
    }

    #[test]
    fn roundtrip_and_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("x.json");
        assert_eq!(read_json::<Doc>(&p).unwrap(), None);
        write_json_atomic(&p, &Doc { n: 1 }).unwrap();
        write_json_atomic(&p, &Doc { n: 2 }).unwrap();
        assert_eq!(read_json::<Doc>(&p).unwrap(), Some(Doc { n: 2 }));
        // No temp files left behind.
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }
}
