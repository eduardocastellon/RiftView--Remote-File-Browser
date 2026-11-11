//! Saving and loading small JSON files (there is no database).
//!
//! Every save is "atomic": we write the new contents to a temporary file
//! next to the real one, flush it to disk, then rename it over the real
//! file. If the computer crashes halfway through, you end up with either
//! the old file or the new one, never a half-written mix.

use std::io::{ErrorKind, Write};
use std::path::Path;

use serde::{Serialize, de::DeserializeOwned};

use crate::platform;

/// Read and parse a JSON file. Returns `Ok(None)` if the file doesn't exist.
pub fn read_json<T: DeserializeOwned>(path: &Path) -> std::io::Result<Option<T>> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };

    let value = serde_json::from_slice(&bytes)
        .map_err(|error| std::io::Error::new(ErrorKind::InvalidData, error))?;

    Ok(Some(value))
}

/// Replace the file at `path` with `value` encoded as JSON, atomically.
pub fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> std::io::Result<()> {
    let contents = serde_json::to_vec_pretty(value).map_err(std::io::Error::other)?;
    write_bytes_atomic(path, &contents)
}

/// Replace the file at `path` with `contents`, atomically. The file is
/// readable only by the current user.
pub fn write_bytes_atomic(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    let folder = path
        .parent()
        .ok_or_else(|| std::io::Error::other("state file has no parent folder"))?;

    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("state");
    let unique_suffix = uuid::Uuid::new_v4().simple();
    let temp_path = folder.join(format!(".{file_name}.{unique_suffix}.tmp"));

    let result = write_then_rename(&temp_path, path, contents);

    // If anything went wrong, don't leave the temp file lying around.
    if result.is_err() {
        let _ = std::fs::remove_file(&temp_path);
    }
    result
}

fn write_then_rename(temp_path: &Path, final_path: &Path, contents: &[u8]) -> std::io::Result<()> {
    // `create_new` fails if the file already exists, so we never clobber
    // anything by accident.
    let mut temp_file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(temp_path)?;
    platform::make_file_private(temp_path)?;

    temp_file.write_all(contents)?;
    temp_file.sync_all()?; // Make sure the bytes are physically on disk.
    drop(temp_file);

    std::fs::rename(temp_path, final_path)?;

    // On Unix, the rename itself lives in the folder's entry list; flush
    // the folder too so the rename survives a crash.
    #[cfg(unix)]
    if let Some(folder) = final_path.parent() {
        std::fs::File::open(folder)?.sync_all()?;
    }

    Ok(())
}

/// Create the data folder (private to the current user) if it's missing.
pub fn ensure_data_dir(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    platform::make_dir_private(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Serialize, serde::Deserialize, PartialEq, Debug)]
    struct Counter {
        count: u32,
    }

    #[test]
    fn saves_overwrites_and_reloads() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("counter.json");

        assert_eq!(read_json::<Counter>(&path).unwrap(), None);

        write_json_atomic(&path, &Counter { count: 1 }).unwrap();
        write_json_atomic(&path, &Counter { count: 2 }).unwrap();
        assert_eq!(
            read_json::<Counter>(&path).unwrap(),
            Some(Counter { count: 2 })
        );

        let files_in_folder = std::fs::read_dir(folder.path()).unwrap().count();
        assert_eq!(files_in_folder, 1, "temp files were left behind");
    }
}
