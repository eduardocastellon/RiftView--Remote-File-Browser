use std::path::{Path, PathBuf};

/// `$XDG_DATA_HOME/riftview`, falling back to `~/.local/share/riftview`.
pub fn default_data_dir() -> Option<PathBuf> {
    let xdg_data_home = std::env::var_os("XDG_DATA_HOME").filter(|value| !value.is_empty());
    if let Some(xdg_data_home) = xdg_data_home {
        return Some(PathBuf::from(xdg_data_home).join("riftview"));
    }

    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join(".local/share/riftview"))
}

/// Owner can read and write; nobody else can do anything.
pub fn make_file_private(path: &Path) -> std::io::Result<()> {
    super::set_unix_permissions(path, 0o600)
}

/// Owner can read, write and enter; nobody else can do anything.
pub fn make_dir_private(path: &Path) -> std::io::Result<()> {
    super::set_unix_permissions(path, 0o700)
}
