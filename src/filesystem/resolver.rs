//! Client-supplied path validation.
//!
//! [`RelPath`] is the only way to name something inside a share. Parsing is
//! strict and lexical; it is a first line of defence; the second (and
//! authoritative) one is `cap-std`'s confined resolution in
//! [`super::roots::Share::open_root`] and friends.

use std::fmt;
use std::path::{Component, Path, PathBuf};

use super::FsError;

const MAX_PATH_BYTES: usize = 4096;
const MAX_COMPONENT_BYTES: usize = 255;

/// A validated path relative to a share root. Empty means the root itself.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RelPath {
    components: Vec<String>,
}

impl RelPath {
    pub fn root() -> Self {
        Self::default()
    }

    /// Parse a client path such as `/Documents/report.pdf` or `Documents`.
    ///
    /// Rules: `/` is the only separator; empty and `.` segments are ignored;
    /// `..`, backslashes, NUL, drive letters / prefixes and other special
    /// components are rejected outright (never "normalised away").
    pub fn parse(raw: &str) -> Result<Self, FsError> {
        if raw.len() > MAX_PATH_BYTES {
            return Err(FsError::InvalidPath("path too long"));
        }
        if raw.contains('\0') {
            return Err(FsError::InvalidPath("NUL byte"));
        }
        if raw.contains('\\') {
            return Err(FsError::InvalidPath("backslash not allowed"));
        }

        let mut components = Vec::new();
        for seg in raw.split('/') {
            match seg {
                "" | "." => continue,
                ".." => return Err(FsError::InvalidPath("'..' not allowed")),
                _ => {}
            }
            if seg.len() > MAX_COMPONENT_BYTES {
                return Err(FsError::InvalidPath("path component too long"));
            }
            if seg.chars().any(|c| c.is_control()) {
                return Err(FsError::InvalidPath("control character"));
            }
            // Reject anything the host OS would not treat as a single plain
            // name (e.g. `C:` on Windows).
            let mut parsed = Path::new(seg).components();
            match (parsed.next(), parsed.next()) {
                (Some(Component::Normal(n)), None) if n == seg => {}
                _ => return Err(FsError::InvalidPath("invalid path component")),
            }
            #[cfg(windows)]
            if seg.contains(':') {
                // Alternate data streams / drive-relative paths.
                return Err(FsError::InvalidPath("':' not allowed"));
            }
            components.push(seg.to_owned());
        }
        Ok(Self { components })
    }

    pub fn is_root(&self) -> bool {
        self.components.is_empty()
    }

    /// Final component, if any.
    pub fn file_name(&self) -> Option<&str> {
        self.components.last().map(String::as_str)
    }

    /// Append a single already-validated entry name (from `read_dir`).
    pub fn join_name(&self, name: &str) -> Result<Self, FsError> {
        let child = Self::parse(name)?;
        if child.components.len() != 1 {
            return Err(FsError::InvalidPath("invalid entry name"));
        }
        let mut out = self.clone();
        out.components.extend(child.components);
        Ok(out)
    }

    /// Relative path for use *only* with a `cap_std::fs::Dir` opened at the
    /// share root. `"."` for the root.
    pub(crate) fn to_cap_path(&self) -> PathBuf {
        if self.components.is_empty() {
            PathBuf::from(".")
        } else {
            self.components.iter().collect()
        }
    }
}

impl fmt::Display for RelPath {
    /// Client-facing form: always starts with `/`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.components.is_empty() {
            return f.write_str("/");
        }
        for c in &self.components {
            write!(f, "/{c}")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(s: &str) -> String {
        RelPath::parse(s).unwrap().to_string()
    }

    fn bad(s: &str) {
        assert!(RelPath::parse(s).is_err(), "expected {s:?} to be rejected");
    }

    #[test]
    fn accepts_and_normalises() {
        assert_eq!(ok(""), "/");
        assert_eq!(ok("/"), "/");
        assert_eq!(ok("/Documents"), "/Documents");
        assert_eq!(ok("Documents/"), "/Documents");
        assert_eq!(ok("//a/./b//"), "/a/b");
        assert_eq!(ok("/my file.txt"), "/my file.txt");
        assert_eq!(ok("/..hidden"), "/..hidden");
        assert_eq!(ok("/ünïcödé"), "/ünïcödé");
    }

    #[test]
    fn rejects_traversal_and_injection() {
        bad("..");
        bad("/..");
        bad("/a/../b");
        bad("a/..");
        bad("..\\etc");
        bad("C:\\Windows");
        bad("\\\\server\\share");
        bad("/a\0b");
        bad("/a\nb");
        bad(&"a".repeat(300));
        bad(&"/a".repeat(3000));
    }

    #[test]
    fn join_name_rejects_multi_component() {
        let base = RelPath::parse("/a").unwrap();
        assert_eq!(base.join_name("b").unwrap().to_string(), "/a/b");
        assert!(base.join_name("b/c").is_err());
        assert!(base.join_name("..").is_err());
        assert!(base.join_name("").is_err());
    }
}
