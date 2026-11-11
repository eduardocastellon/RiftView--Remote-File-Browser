//! Things that differ between operating systems.
//!
//! Keep this module small: only code that genuinely has to be different on
//! each OS belongs here.

use std::path::{Path, PathBuf};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "linux")]
use linux as current_os;
#[cfg(target_os = "macos")]
use macos as current_os;
#[cfg(target_os = "windows")]
use windows as current_os;

/// Set this environment variable to store settings somewhere other than
/// the OS default location.
pub const DATA_DIR_ENV: &str = "RIFTVIEW_DATA_DIR";

/// The folder where RiftView keeps its settings and state files.
pub fn data_dir() -> Option<PathBuf> {
    let override_dir = std::env::var_os(DATA_DIR_ENV).filter(|value| !value.is_empty());

    match override_dir {
        Some(dir) => Some(PathBuf::from(dir)),
        None => current_os::default_data_dir(),
    }
}

/// Make a settings file readable only by the current user.
pub fn make_file_private(path: &Path) -> std::io::Result<()> {
    current_os::make_file_private(path)
}

/// Make a folder accessible only by the current user.
pub fn make_dir_private(path: &Path) -> std::io::Result<()> {
    current_os::make_dir_private(path)
}

/// Shared helper for macOS and Linux: set Unix permission bits.
#[cfg(unix)]
fn set_unix_permissions(path: &Path, mode: u32) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
}
