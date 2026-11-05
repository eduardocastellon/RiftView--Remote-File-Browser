use std::path::{Path, PathBuf};

pub fn default_data_dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join("Library/Application Support/RiftView"))
}

pub fn restrict_file_permissions(path: &Path) -> std::io::Result<()> {
    super::chmod(path, 0o600)
}

pub fn restrict_dir_permissions(path: &Path) -> std::io::Result<()> {
    super::chmod(path, 0o700)
}
