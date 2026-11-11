use std::path::{Path, PathBuf};

/// `%APPDATA%\RiftView`
pub fn default_data_dir() -> Option<PathBuf> {
    let app_data = std::env::var_os("APPDATA")?;
    Some(PathBuf::from(app_data).join("RiftView"))
}

// `%APPDATA%` is already private to the current user by default, so these
// are no-ops for now.
// TODO(phase 8): set an explicit owner-only access-control list.

pub fn make_file_private(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

pub fn make_dir_private(_path: &Path) -> std::io::Result<()> {
    Ok(())
}
