//! Trusted devices and what each one is allowed to do, saved in
//! `trusted_devices.json`.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::crypto;
use crate::filesystem::roots::now_ms;
use crate::storage;

pub const DEVICES_SCHEMA_VERSION: u32 = 1;
const DEVICES_FILE_NAME: &str = "trusted_devices.json";

/// One thing a device may be allowed to do (planning.md §7.4).
///
/// Write permissions are also capped per share: nothing can change a share
/// whose `writable` switch is off.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    /// List shares and folders, see file details.
    Browse,
    /// Download files.
    Read,
    /// Create new files.
    Upload,
    Rename,
    Move,
    Delete,
}

impl Permission {
    pub const ALL: [Permission; 6] = [
        Permission::Browse,
        Permission::Read,
        Permission::Upload,
        Permission::Rename,
        Permission::Move,
        Permission::Delete,
    ];

    pub fn read_only() -> BTreeSet<Permission> {
        BTreeSet::from([Permission::Browse, Permission::Read])
    }

    pub fn full_access() -> BTreeSet<Permission> {
        BTreeSet::from(Permission::ALL)
    }
}

/// A device that completed pairing and was approved by the host user.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Device {
    /// Opaque ID such as `dev_3f2a…`.
    pub id: String,

    /// Name the device gave itself, e.g. "Lalo's iPhone".
    pub name: String,

    /// Ed25519 public key, base64. The private key never leaves the device.
    pub public_key: String,

    pub permissions: BTreeSet<Permission>,

    pub paired_at_ms: u64,

    /// Revoked devices are kept for the record but can never log in again.
    #[serde(default)]
    pub revoked_at_ms: Option<u64>,
}

impl Device {
    pub fn is_revoked(&self) -> bool {
        self.revoked_at_ms.is_some()
    }

    pub fn public_key_bytes(&self) -> Option<[u8; 32]> {
        crypto::decode_base64(&self.public_key)?.try_into().ok()
    }

    pub fn can(&self, permission: Permission) -> bool {
        !self.is_revoked() && self.permissions.contains(&permission)
    }
}

/// Short, human-comparable fingerprint of a device key, e.g. `SHA256:1a2b3c4d5e6f7a8b`.
pub fn key_fingerprint(public_key: &[u8; 32]) -> String {
    let hash = crypto::sha256(public_key);
    let hex: String = hash[..8].iter().map(|byte| format!("{byte:02x}")).collect();
    format!("SHA256:{hex}")
}

/// What `trusted_devices.json` looks like on disk.
#[derive(Debug, Default, Serialize, Deserialize)]
struct DevicesFile {
    schema_version: u32,
    devices: Vec<Device>,
}

#[derive(Debug, thiserror::Error)]
pub enum DeviceError {
    #[error("device not found")]
    NotFound,

    #[error("unsupported {DEVICES_FILE_NAME} schema_version {0}")]
    UnsupportedVersion(u32),

    #[error("device storage I/O: {0}")]
    Io(#[from] std::io::Error),
}

/// All trusted devices, kept in memory and saved after every change.
#[derive(Debug)]
pub struct DeviceStore {
    devices_file: PathBuf,
    devices: Vec<Device>,
}

impl DeviceStore {
    pub fn load(data_dir: &Path) -> Result<Self, DeviceError> {
        let devices_file = data_dir.join(DEVICES_FILE_NAME);

        let devices = match storage::read_json::<DevicesFile>(&devices_file)? {
            None => Vec::new(),
            Some(saved) if saved.schema_version != DEVICES_SCHEMA_VERSION => {
                return Err(DeviceError::UnsupportedVersion(saved.schema_version));
            }
            Some(saved) => saved.devices,
        };

        Ok(Self {
            devices_file,
            devices,
        })
    }

    pub fn list(&self) -> &[Device] {
        &self.devices
    }

    pub fn get(&self, id: &str) -> Option<&Device> {
        self.devices.iter().find(|device| device.id == id)
    }

    /// A device that exists and has not been revoked.
    pub fn get_active(&self, id: &str) -> Option<&Device> {
        self.get(id).filter(|device| !device.is_revoked())
    }

    pub fn add(&mut self, device: Device) -> Result<Device, DeviceError> {
        self.devices.push(device.clone());

        if let Err(error) = self.save() {
            self.devices.pop();
            return Err(error);
        }
        Ok(device)
    }

    pub fn set_permissions(
        &mut self,
        id: &str,
        permissions: BTreeSet<Permission>,
    ) -> Result<Device, DeviceError> {
        let index = self.index_of(id)?;
        let original = self.devices[index].permissions.clone();
        self.devices[index].permissions = permissions;

        if let Err(error) = self.save() {
            self.devices[index].permissions = original;
            return Err(error);
        }
        Ok(self.devices[index].clone())
    }

    /// Permanently revoke a device. Revoking twice is harmless.
    pub fn revoke(&mut self, id: &str) -> Result<Device, DeviceError> {
        let index = self.index_of(id)?;
        if self.devices[index].is_revoked() {
            return Ok(self.devices[index].clone());
        }

        self.devices[index].revoked_at_ms = Some(now_ms());
        if let Err(error) = self.save() {
            self.devices[index].revoked_at_ms = None;
            return Err(error);
        }
        Ok(self.devices[index].clone())
    }

    fn index_of(&self, id: &str) -> Result<usize, DeviceError> {
        self.devices
            .iter()
            .position(|device| device.id == id)
            .ok_or(DeviceError::NotFound)
    }

    fn save(&self) -> Result<(), DeviceError> {
        let file_contents = DevicesFile {
            schema_version: DEVICES_SCHEMA_VERSION,
            devices: self.devices.clone(),
        };
        storage::write_json_atomic(&self.devices_file, &file_contents)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_device() -> Device {
        Device {
            id: "dev_1".into(),
            name: "Phone".into(),
            public_key: crypto::encode_base64(&[7u8; 32]),
            permissions: Permission::read_only(),
            paired_at_ms: 0,
            revoked_at_ms: None,
        }
    }

    #[test]
    fn add_reload_change_permissions_and_revoke() {
        let data_dir = tempfile::tempdir().unwrap();
        let mut store = DeviceStore::load(data_dir.path()).unwrap();
        store.add(sample_device()).unwrap();

        let reloaded = DeviceStore::load(data_dir.path()).unwrap();
        let device = reloaded.get("dev_1").unwrap();
        assert!(device.can(Permission::Read));
        assert!(!device.can(Permission::Delete));
        assert_eq!(device.public_key_bytes(), Some([7u8; 32]));

        store
            .set_permissions("dev_1", Permission::full_access())
            .unwrap();
        assert!(store.get("dev_1").unwrap().can(Permission::Delete));

        store.revoke("dev_1").unwrap();
        let reloaded = DeviceStore::load(data_dir.path()).unwrap();
        assert!(reloaded.get_active("dev_1").is_none());
        assert!(!reloaded.get("dev_1").unwrap().can(Permission::Browse));
    }
}
