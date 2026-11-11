//! Checking paths sent by clients.
//!
//! [`RelPath`] is the only way to name something inside a share. Parsing is
//! strict: anything suspicious is rejected, not "cleaned up". This is the
//! first line of defence. The second, and the one that really counts, is
//! `cap-std`'s confined lookup (see the `filesystem` module docs).

use std::fmt;
use std::path::{Component, Path, PathBuf};

use super::FsError;

const MAX_PATH_BYTES: usize = 4096;
const MAX_NAME_BYTES: usize = 255;

/// A checked path relative to a share's root, stored as a list of names.
/// An empty list means the root folder itself.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RelPath {
    names: Vec<String>,
}

impl RelPath {
    pub fn root() -> Self {
        Self::default()
    }

    /// Parse a client path such as `/Documents/report.pdf` or `Documents`.
    ///
    /// - `/` is the only separator.
    /// - Empty segments (`//`) and `.` are ignored.
    /// - `..`, backslashes, NUL bytes, control characters, and anything the
    ///   OS would treat specially (like `C:` on Windows) are rejected.
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

        let mut names = Vec::new();

        for segment in raw.split('/') {
            if segment.is_empty() || segment == "." {
                continue;
            }
            check_name(segment)?;
            names.push(segment.to_owned());
        }

        Ok(Self { names })
    }

    pub fn is_root(&self) -> bool {
        self.names.is_empty()
    }

    /// The last name in the path (the file or folder itself), if any.
    pub fn file_name(&self) -> Option<&str> {
        self.names.last().map(String::as_str)
    }

    /// A new path with one more name on the end, e.g. `/a` + `b` = `/a/b`.
    pub fn join_name(&self, name: &str) -> Result<Self, FsError> {
        if name.is_empty() || name == "." || name.contains('/') || name.contains('\\') {
            return Err(FsError::InvalidPath("invalid entry name"));
        }
        check_name(name)?;

        let mut joined = self.clone();
        joined.names.push(name.to_owned());
        Ok(joined)
    }

    /// The path in a form to hand *only* to a `cap_std::fs::Dir` opened on
    /// the share root. The root itself becomes `"."`.
    pub(crate) fn to_cap_path(&self) -> PathBuf {
        if self.names.is_empty() {
            PathBuf::from(".")
        } else {
            self.names.iter().collect()
        }
    }
}

/// Reject a single path segment that isn't a plain, ordinary name.
fn check_name(name: &str) -> Result<(), FsError> {
    if name == ".." {
        return Err(FsError::InvalidPath("'..' not allowed"));
    }
    if name.len() > MAX_NAME_BYTES {
        return Err(FsError::InvalidPath("path component too long"));
    }
    if name.chars().any(char::is_control) {
        return Err(FsError::InvalidPath("control character"));
    }

    // Ask the OS's own path parser: is this exactly one ordinary name?
    // This catches things like `C:` on Windows.
    let mut components = Path::new(name).components();
    let is_single_plain_name = matches!(
        (components.next(), components.next()),
        (Some(Component::Normal(only)), None) if only == name
    );
    if !is_single_plain_name {
        return Err(FsError::InvalidPath("invalid path component"));
    }

    // On Windows, `:` is used for drive letters and hidden data streams.
    #[cfg(windows)]
    if name.contains(':') {
        return Err(FsError::InvalidPath("':' not allowed"));
    }

    Ok(())
}

impl fmt::Display for RelPath {
    /// The form clients see: always starts with `/`, e.g. `/Documents/a.txt`.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.names.is_empty() {
            return formatter.write_str("/");
        }
        for name in &self.names {
            write!(formatter, "/{name}")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(input: &str) -> String {
        RelPath::parse(input).unwrap().to_string()
    }

    fn assert_rejected(input: &str) {
        assert!(
            RelPath::parse(input).is_err(),
            "expected {input:?} to be rejected"
        );
    }

    #[test]
    fn accepts_and_tidies_ordinary_paths() {
        assert_eq!(parsed(""), "/");
        assert_eq!(parsed("/"), "/");
        assert_eq!(parsed("/Documents"), "/Documents");
        assert_eq!(parsed("Documents/"), "/Documents");
        assert_eq!(parsed("//a/./b//"), "/a/b");
        assert_eq!(parsed("/my file.txt"), "/my file.txt");
        assert_eq!(parsed("/..hidden"), "/..hidden");
        assert_eq!(parsed("/ünïcödé"), "/ünïcödé");
    }

    #[test]
    fn rejects_traversal_and_injection() {
        assert_rejected("..");
        assert_rejected("/..");
        assert_rejected("/a/../b");
        assert_rejected("a/..");
        assert_rejected("..\\etc");
        assert_rejected("C:\\Windows");
        assert_rejected("\\\\server\\share");
        assert_rejected("/a\0b");
        assert_rejected("/a\nb");
        assert_rejected(&"a".repeat(300));
        assert_rejected(&"/a".repeat(3000));
    }

    #[test]
    fn join_name_only_accepts_a_single_name() {
        let base = RelPath::parse("/a").unwrap();
        assert_eq!(base.join_name("b").unwrap().to_string(), "/a/b");
        assert!(base.join_name("b/c").is_err());
        assert!(base.join_name("..").is_err());
        assert!(base.join_name("").is_err());
    }
}
