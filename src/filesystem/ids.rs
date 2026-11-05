//! Opaque file identifiers.
//!
//! A `FileId` encodes `(share_id, RelPath)` as URL-safe base64. It is opaque
//! to clients but *not* a capability: decoding re-validates the path and every
//! use goes through the confined resolver and the share's permissions.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;

use super::{FsError, RelPath};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileId {
    pub share_id: String,
    pub path: RelPath,
}

impl FileId {
    pub fn new(share_id: impl Into<String>, path: RelPath) -> Self {
        Self { share_id: share_id.into(), path }
    }

    pub fn encode(&self) -> String {
        URL_SAFE_NO_PAD.encode(format!("{}:{}", self.share_id, self.path))
    }

    pub fn decode(s: &str) -> Result<Self, FsError> {
        const INVALID: FsError = FsError::InvalidPath("invalid file id");
        if s.len() > 8192 {
            return Err(INVALID);
        }
        let bytes = URL_SAFE_NO_PAD.decode(s).map_err(|_| INVALID)?;
        let text = String::from_utf8(bytes).map_err(|_| INVALID)?;
        let (share_id, path) = text.split_once(':').ok_or(INVALID)?;
        if share_id.is_empty() || !path.starts_with('/') {
            return Err(INVALID);
        }
        Ok(Self { share_id: share_id.to_owned(), path: RelPath::parse(path)? })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let id = FileId::new("shr_1", RelPath::parse("/a/b c.txt").unwrap());
        assert_eq!(FileId::decode(&id.encode()).unwrap(), id);
    }

    #[test]
    fn rejects_tampered_paths() {
        let forged = URL_SAFE_NO_PAD.encode("shr_1:/../etc/passwd");
        assert!(FileId::decode(&forged).is_err());
        assert!(FileId::decode("not base64!!").is_err());
        assert!(FileId::decode(&URL_SAFE_NO_PAD.encode("noseparator")).is_err());
    }
}
