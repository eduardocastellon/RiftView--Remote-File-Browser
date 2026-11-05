use std::path::{Path, PathBuf};

pub fn default_data_dir() -> Option<PathBuf> {
    let appdata = std::env::var_os("APPDATA")?;
    Some(PathBuf::from(appdata).join("RiftView"))
}

// %APPDATA% is already per-user and ACL-protected by default.
// TODO(phase 8): set an explicit owner-only DACL.
pub fn restrict_file_permissions(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

pub fn restrict_dir_permissions(_path: &Path) -> std::io::Result<()> {
    Ok(())
}
