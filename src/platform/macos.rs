use std::path::{Path, PathBuf};

/// `~/Library/Application Support/RiftView`
pub fn default_data_dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join("Library/Application Support/RiftView"))
}

/// Owner can read and write; nobody else can do anything.
pub fn make_file_private(path: &Path) -> std::io::Result<()> {
    super::set_unix_permissions(path, 0o600)
}

/// Owner can read, write and enter; nobody else can do anything.
pub fn make_dir_private(path: &Path) -> std::io::Result<()> {
    super::set_unix_permissions(path, 0o700)
}
