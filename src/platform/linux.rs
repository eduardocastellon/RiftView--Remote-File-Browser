use std::path::{Path, PathBuf};

pub fn default_data_dir() -> Option<PathBuf> {
    if let Some(xdg) = std::env::var_os("XDG_DATA_HOME").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(xdg).join("riftview"));
    }
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join(".local/share/riftview"))
}

pub fn restrict_file_permissions(path: &Path) -> std::io::Result<()> {
    super::chmod(path, 0o600)
}

pub fn restrict_dir_permissions(path: &Path) -> std::io::Result<()> {
    super::chmod(path, 0o700)
}
