//! SHA-256 of a whole file, so a device can check that what it downloaded
//! is exactly what's on the host.
//!
//! Hashing reads the entire file (roughly 1–2 GB per second), so results
//! are cached in memory. A cached hash is only used while the file's
//! [`FileVersion`] is unchanged.

use std::collections::HashMap;
use std::io;
use std::sync::Mutex;

use sha2::{Digest, Sha256};
use tokio::io::{AsyncRead, AsyncReadExt};

use super::version::FileVersion;

/// How much of the file we read at a time while hashing.
const READ_BUFFER_BYTES: usize = 1024 * 1024;

/// The most hashes kept in memory (about 100 bytes each, plus the key).
const MAX_CACHED_HASHES: usize = 10_000;

pub type Sha256Hash = [u8; 32];

/// Read `reader` to the end and return its SHA-256.
///
/// This is an async loop rather than a blocking thread on purpose: if the
/// request is dropped (e.g. the device disconnects), hashing stops at the
/// next read instead of carrying on through a 50 GB file.
pub async fn sha256_of<R: AsyncRead + Unpin>(mut reader: R) -> io::Result<Sha256Hash> {
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; READ_BUFFER_BYTES];

    loop {
        let bytes_read = reader.read(&mut buffer).await?;
        if bytes_read == 0 {
            break;
        }
        hasher.update(&buffer[..bytes_read]);
    }

    Ok(hasher.finalize().into())
}

/// SHA-256 of bytes already in memory (e.g. one upload chunk).
pub fn sha256_of_bytes(bytes: &[u8]) -> Sha256Hash {
    Sha256::digest(bytes).into()
}

/// Lowercase hex, e.g. `9f86d081…`.
pub fn to_hex(hash: &Sha256Hash) -> String {
    hash.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Recently computed hashes, keyed by file ID.
#[derive(Default)]
pub struct HashCache {
    inner: Mutex<CacheInner>,
}

#[derive(Default)]
struct CacheInner {
    entries: HashMap<String, CachedHash>,
    /// Ticks up on every lookup or insert; used to find the least recently
    /// used entry when the cache is full.
    clock: u64,
}

struct CachedHash {
    version: FileVersion,
    sha256: Sha256Hash,
    last_used: u64,
}

impl HashCache {
    /// The cached hash for `file_id`, if it was computed for this exact
    /// version of the file.
    pub fn get(&self, file_id: &str, version: FileVersion) -> Option<Sha256Hash> {
        let mut inner = self.inner.lock().unwrap();
        inner.clock += 1;
        let now = inner.clock;

        let entry = inner.entries.get_mut(file_id)?;
        if entry.version != version {
            return None;
        }
        entry.last_used = now;
        Some(entry.sha256)
    }

    pub fn insert(&self, file_id: &str, version: FileVersion, sha256: Sha256Hash) {
        let mut inner = self.inner.lock().unwrap();
        inner.clock += 1;
        let now = inner.clock;

        let is_new_key = !inner.entries.contains_key(file_id);
        if is_new_key && inner.entries.len() >= MAX_CACHED_HASHES {
            inner.remove_least_recently_used();
        }

        let entry = CachedHash {
            version,
            sha256,
            last_used: now,
        };
        inner.entries.insert(file_id.to_owned(), entry);
    }
}

impl CacheInner {
    fn remove_least_recently_used(&mut self) {
        let oldest = self
            .entries
            .iter()
            .min_by_key(|(_, entry)| entry.last_used)
            .map(|(file_id, _)| file_id.clone());

        if let Some(file_id) = oldest {
            self.entries.remove(&file_id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[tokio::test]
    async fn hashes_match_known_values() {
        let empty = sha256_of(&b""[..]).await.unwrap();
        assert_eq!(
            to_hex(&empty),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );

        let hello = sha256_of(&b"hello world"[..]).await.unwrap();
        assert_eq!(
            to_hex(&hello),
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        );
    }

    #[test]
    fn cache_ignores_hashes_of_old_versions() {
        let mut file = tempfile::tempfile().unwrap();
        file.write_all(b"one").unwrap();
        let old_version = FileVersion::of(&file).unwrap();

        let cache = HashCache::default();
        cache.insert("file", old_version, [1; 32]);
        assert_eq!(cache.get("file", old_version), Some([1; 32]));

        file.write_all(b"two").unwrap();
        let new_version = FileVersion::of(&file).unwrap();
        assert_eq!(cache.get("file", new_version), None);
    }

    #[test]
    fn full_cache_drops_least_recently_used() {
        let file = tempfile::tempfile().unwrap();
        let version = FileVersion::of(&file).unwrap();
        let cache = HashCache::default();

        for number in 0..MAX_CACHED_HASHES {
            cache.insert(&format!("file{number}"), version, [0; 32]);
        }
        cache.get("file0", version); // Now file1 is the least recently used.
        cache.insert("one-more", version, [0; 32]);

        assert!(cache.get("file0", version).is_some());
        assert!(cache.get("file1", version).is_none());
        assert!(cache.get("one-more", version).is_some());
    }
}
