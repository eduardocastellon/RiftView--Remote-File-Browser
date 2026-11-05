//! OS-specific behaviour. Keep this module small: only things that genuinely
//! differ between platforms belong here.

use std::path::PathBuf;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "linux")]
use linux as imp;
#[cfg(target_os = "macos")]
use macos as imp;
#[cfg(target_os = "windows")]
use windows as imp;

/// Environment variable that overrides the application data directory.
pub const DATA_DIR_ENV: &str = "RIFTVIEW_DATA_DIR";

/// Directory holding config and state files.
pub fn data_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os(DATA_DIR_ENV).filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(dir));
    }
    imp::default_data_dir()
}

/// Restrict a freshly written state file so only the current user can read it.
pub fn restrict_file_permissions(path: &std::path::Path) -> std::io::Result<()> {
    imp::restrict_file_permissions(path)
}

/// Restrict the data directory so only the current user can access it.
pub fn restrict_dir_permissions(path: &std::path::Path) -> std::io::Result<()> {
    imp::restrict_dir_permissions(path)
}

#[cfg(unix)]
fn chmod(path: &std::path::Path, mode: u32) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
}
