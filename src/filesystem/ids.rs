//! File IDs that clients use instead of raw paths.
//!
//! A `FileId` is just "share ID + path inside the share", base64-encoded so
//! it looks opaque in URLs. It is *not* a secret or a permission: when we
//! decode one we re-check the path, and every use still goes through the
//! confined lookup and the share's permissions.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;

use super::{FsError, RelPath};

/// Longest encoded ID we'll bother trying to decode.
const MAX_ENCODED_LEN: usize = 8192;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileId {
    pub share_id: String,
    pub path: RelPath,
}

impl FileId {
    pub fn new(share_id: impl Into<String>, path: RelPath) -> Self {
        Self {
            share_id: share_id.into(),
            path,
        }
    }

    /// Encode as `base64("<share_id>:<path>")`.
    pub fn encode(&self) -> String {
        let plain = format!("{}:{}", self.share_id, self.path);
        URL_SAFE_NO_PAD.encode(plain)
    }

    pub fn decode(encoded: &str) -> Result<Self, FsError> {
        const INVALID: FsError = FsError::InvalidPath("invalid file id");

        if encoded.len() > MAX_ENCODED_LEN {
            return Err(INVALID);
        }

        let bytes = URL_SAFE_NO_PAD.decode(encoded).map_err(|_| INVALID)?;
        let plain = String::from_utf8(bytes).map_err(|_| INVALID)?;

        let (share_id, path) = plain.split_once(':').ok_or(INVALID)?;
        if share_id.is_empty() || !path.starts_with('/') {
            return Err(INVALID);
        }

        // Re-check the path: the client could have forged it.
        Ok(Self::new(share_id, RelPath::parse(path)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_then_decode_gives_back_the_same_id() {
        let id = FileId::new("shr_1", RelPath::parse("/a/b c.txt").unwrap());
        assert_eq!(FileId::decode(&id.encode()).unwrap(), id);
    }

    #[test]
    fn rejects_forged_or_garbage_ids() {
        let forged = URL_SAFE_NO_PAD.encode("shr_1:/../etc/passwd");
        assert!(FileId::decode(&forged).is_err());

        assert!(FileId::decode("not base64!!").is_err());

        let no_separator = URL_SAFE_NO_PAD.encode("noseparator");
        assert!(FileId::decode(&no_separator).is_err());
    }
}
