//! Trusted devices and what each one is allowed to do, saved in the
//! `devices` and `device_permissions` tables of the database.

use std::collections::BTreeSet;

use rusqlite::types::{FromSql, FromSqlError, FromSqlResult, ToSql, ToSqlOutput, ValueRef};
use rusqlite::{Row, Transaction, params};
use serde::{Deserialize, Serialize};

use super::crypto;
use crate::database::{Database, DatabaseError};
use crate::filesystem::roots::now_ms;

/// Key version given to a newly paired device. There's no key rotation
/// yet; when there is, a device that replaces its key gets the next number.
pub const FIRST_KEY_VERSION: u32 = 1;

const MAX_DEVICE_NAME_LEN: usize = 64;
const MAX_APP_VERSION_LEN: usize = 32;

// ---------------------------------------------------------------------------
// Permissions, device types and platforms
// ---------------------------------------------------------------------------

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

    /// Name used in the API and the database, e.g. `"browse"`.
    pub fn as_str(self) -> &'static str {
        match self {
            Permission::Browse => "browse",
            Permission::Read => "read",
            Permission::Upload => "upload",
            Permission::Rename => "rename",
            Permission::Move => "move",
            Permission::Delete => "delete",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|value| value.as_str() == name)
    }
}

/// What kind of device it is, as the app reports at pairing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceType {
    Phone,
    Tablet,
    Laptop,
    Desktop,
    Other,
}

impl DeviceType {
    pub const ALL: [DeviceType; 5] = [
        DeviceType::Phone,
        DeviceType::Tablet,
        DeviceType::Laptop,
        DeviceType::Desktop,
        DeviceType::Other,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            DeviceType::Phone => "phone",
            DeviceType::Tablet => "tablet",
            DeviceType::Laptop => "laptop",
            DeviceType::Desktop => "desktop",
            DeviceType::Other => "other",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|value| value.as_str() == name)
    }
}

/// The operating system the app runs on, as the app reports at pairing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Platform {
    Ios,
    Android,
    Macos,
    Windows,
    Linux,
    Other,
}

impl Platform {
    pub const ALL: [Platform; 6] = [
        Platform::Ios,
        Platform::Android,
        Platform::Macos,
        Platform::Windows,
        Platform::Linux,
        Platform::Other,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Platform::Ios => "ios",
            Platform::Android => "android",
            Platform::Macos => "macos",
            Platform::Windows => "windows",
            Platform::Linux => "linux",
            Platform::Other => "other",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|value| value.as_str() == name)
    }
}

// Let these be written to and read from SQLite as their text names, so
// `params![device.platform]` and `row.get("platform")` just work.

impl ToSql for Permission {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(self.as_str().into())
    }
}

impl FromSql for Permission {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        parse_text_column(value, Permission::from_name)
    }
}

impl ToSql for DeviceType {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(self.as_str().into())
    }
}

impl FromSql for DeviceType {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        parse_text_column(value, DeviceType::from_name)
    }
}

impl ToSql for Platform {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(self.as_str().into())
    }
}

impl FromSql for Platform {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        parse_text_column(value, Platform::from_name)
    }
}

/// Read a text column and turn it into one of the enums above. An unknown
/// name (only possible if the database was edited by hand) is an error.
fn parse_text_column<T>(value: ValueRef<'_>, parse: fn(&str) -> Option<T>) -> FromSqlResult<T> {
    let text = value.as_str()?;
    parse(text).ok_or_else(|| FromSqlError::Other(format!("unknown value {text:?}").into()))
}

// ---------------------------------------------------------------------------
// Devices
// ---------------------------------------------------------------------------

/// What a device tells us about itself when it pairs.
#[derive(Debug, Clone)]
pub struct DeviceDescription {
    /// Name the device gave itself, e.g. "Lalo's iPhone".
    pub name: String,
    /// Raw Ed25519 public key.
    pub public_key: [u8; 32],
    pub device_type: DeviceType,
    pub platform: Platform,
    pub app_version: String,
}

/// A device that completed pairing and was approved by the host user.
#[derive(Debug, Clone)]
pub struct Device {
    /// Opaque ID such as `dev_3f2a…`.
    pub id: String,

    /// Name the device gave itself, e.g. "Lalo's iPhone".
    pub name: String,

    /// Name the host user gave the device, shown instead of `name` if set.
    pub custom_alias: Option<String>,

    /// Ed25519 public key, base64. The private key never leaves the device.
    pub public_key: String,

    /// Which key this is, if the device has ever replaced it. See
    /// [`FIRST_KEY_VERSION`].
    pub key_version: u32,

    pub device_type: DeviceType,
    pub platform: Platform,

    /// App version last reported by the device (at pairing or login).
    pub app_version: String,

    pub permissions: BTreeSet<Permission>,

    pub paired_at_ms: u64,

    /// When the device last logged in successfully.
    pub last_connected_at_ms: Option<u64>,

    /// Revoked devices are kept for the record but can never log in again.
    pub revoked_at_ms: Option<u64>,
}

impl Device {
    /// A freshly approved device.
    pub fn new_paired(
        id: String,
        description: DeviceDescription,
        permissions: BTreeSet<Permission>,
    ) -> Self {
        Self {
            id,
            name: description.name,
            custom_alias: None,
            public_key: crypto::encode_base64(&description.public_key),
            key_version: FIRST_KEY_VERSION,
            device_type: description.device_type,
            platform: description.platform,
            app_version: description.app_version,
            permissions,
            paired_at_ms: now_ms(),
            last_connected_at_ms: None,
            revoked_at_ms: None,
        }
    }

    pub fn is_revoked(&self) -> bool {
        self.revoked_at_ms.is_some()
    }

    /// The host user's alias if set, otherwise the device's own name.
    pub fn display_name(&self) -> &str {
        self.custom_alias.as_deref().unwrap_or(&self.name)
    }

    pub fn public_key_bytes(&self) -> Option<[u8; 32]> {
        crypto::decode_base64(&self.public_key)?.try_into().ok()
    }

    pub fn can(&self, permission: Permission) -> bool {
        !self.is_revoked() && self.permissions.contains(&permission)
    }

    /// Build a device from a row of the `devices` table. Its permissions
    /// live in another table and are filled in by the caller.
    fn from_row(row: &Row) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get("id")?,
            name: row.get("name")?,
            custom_alias: row.get("custom_alias")?,
            public_key: row.get("public_key")?,
            key_version: row.get("key_version")?,
            device_type: row.get("device_type")?,
            platform: row.get("platform")?,
            app_version: row.get("app_version")?,
            permissions: BTreeSet::new(),
            paired_at_ms: row.get("paired_at_ms")?,
            last_connected_at_ms: row.get("last_connected_at_ms")?,
            revoked_at_ms: row.get("revoked_at_ms")?,
        })
    }
}

/// Short, human-comparable fingerprint of a device key, e.g. `SHA256:1a2b3c4d5e6f7a8b`.
pub fn key_fingerprint(public_key: &[u8; 32]) -> String {
    let hash = crypto::sha256(public_key);
    let hex: String = hash[..8].iter().map(|byte| format!("{byte:02x}")).collect();
    format!("SHA256:{hex}")
}

/// Check a device name or alias: 1–64 printable characters. Returns it
/// trimmed.
pub fn validate_device_name(name: &str) -> Result<String, &'static str> {
    let name = name.trim();
    let is_valid = !name.is_empty()
        && name.chars().count() <= MAX_DEVICE_NAME_LEN
        && !name.chars().any(char::is_control);

    if !is_valid {
        return Err("device name must be 1-64 printable characters");
    }
    Ok(name.to_owned())
}

/// Check an app version such as `1.4.0` or `1.4.0-beta.2+77`: 1–32
/// characters, only letters, digits and `.` `-` `+`.
pub fn validate_app_version(version: &str) -> Result<String, &'static str> {
    let is_allowed =
        |character: char| character.is_ascii_alphanumeric() || ".-+".contains(character);
    let is_valid = !version.is_empty()
        && version.len() <= MAX_APP_VERSION_LEN
        && version.chars().all(is_allowed);

    if !is_valid {
        return Err("app_version must be 1-32 characters: letters, digits, '.', '-' or '+'");
    }
    Ok(version.to_owned())
}

#[derive(Debug, thiserror::Error)]
pub enum DeviceError {
    #[error("device not found")]
    NotFound,

    #[error("{0}")]
    Invalid(&'static str),

    #[error(transparent)]
    Database(#[from] DatabaseError),
}

impl From<rusqlite::Error> for DeviceError {
    fn from(error: rusqlite::Error) -> Self {
        DeviceError::Database(error.into())
    }
}

/// All trusted devices.
///
/// The database is the record; `devices` is an in-memory copy, because
/// every request checks its device and that shouldn't touch the disk. Each
/// change is written to the database first and only applied to memory once
/// that succeeded, so the two never disagree.
pub struct DeviceStore {
    database: Database,
    devices: Vec<Device>,
}

impl DeviceStore {
    pub fn load(database: Database) -> Result<Self, DatabaseError> {
        let devices = load_all_devices(&database)?;
        Ok(Self { database, devices })
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
        let mut connection = self.database.connection();
        let transaction = connection.transaction()?;
        transaction.execute(
            "INSERT INTO devices (
                 id, name, custom_alias, public_key, key_version, device_type, platform,
                 app_version, paired_at_ms, last_connected_at_ms, revoked_at_ms
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                device.id,
                device.name,
                device.custom_alias,
                device.public_key,
                device.key_version,
                device.device_type,
                device.platform,
                device.app_version,
                device.paired_at_ms,
                device.last_connected_at_ms,
                device.revoked_at_ms,
            ],
        )?;
        insert_permissions(&transaction, &device.id, &device.permissions)?;
        transaction.commit()?;
        drop(connection);

        self.devices.push(device.clone());
        Ok(device)
    }

    /// Replace a device's permissions with exactly `permissions`.
    pub fn set_permissions(
        &mut self,
        id: &str,
        permissions: BTreeSet<Permission>,
    ) -> Result<Device, DeviceError> {
        let index = self.index_of(id)?;

        let mut connection = self.database.connection();
        let transaction = connection.transaction()?;
        transaction.execute(
            "DELETE FROM device_permissions WHERE device_id = ?1",
            params![id],
        )?;
        insert_permissions(&transaction, id, &permissions)?;
        transaction.commit()?;
        drop(connection);

        self.devices[index].permissions = permissions;
        Ok(self.devices[index].clone())
    }

    /// Give the device a name of the host user's choosing. `None` or an
    /// empty name removes the alias.
    pub fn set_alias(&mut self, id: &str, alias: Option<&str>) -> Result<Device, DeviceError> {
        let index = self.index_of(id)?;
        let alias = match alias.map(str::trim) {
            None | Some("") => None,
            Some(alias) => Some(validate_device_name(alias).map_err(DeviceError::Invalid)?),
        };

        self.database.connection().execute(
            "UPDATE devices SET custom_alias = ?1 WHERE id = ?2",
            params![alias, id],
        )?;

        self.devices[index].custom_alias = alias;
        Ok(self.devices[index].clone())
    }

    /// Note a successful login, and the app version if the device sent one.
    pub fn record_login(
        &mut self,
        id: &str,
        app_version: Option<String>,
    ) -> Result<Device, DeviceError> {
        let index = self.index_of(id)?;
        let connected_at = now_ms();
        let app_version = app_version.unwrap_or_else(|| self.devices[index].app_version.clone());

        self.database.connection().execute(
            "UPDATE devices SET last_connected_at_ms = ?1, app_version = ?2 WHERE id = ?3",
            params![connected_at, app_version, id],
        )?;

        let device = &mut self.devices[index];
        device.last_connected_at_ms = Some(connected_at);
        device.app_version = app_version;
        Ok(device.clone())
    }

    /// Permanently revoke a device. Revoking twice is harmless.
    pub fn revoke(&mut self, id: &str) -> Result<Device, DeviceError> {
        let index = self.index_of(id)?;
        if self.devices[index].is_revoked() {
            return Ok(self.devices[index].clone());
        }

        let revoked_at = now_ms();
        self.database.connection().execute(
            "UPDATE devices SET revoked_at_ms = ?1 WHERE id = ?2",
            params![revoked_at, id],
        )?;

        self.devices[index].revoked_at_ms = Some(revoked_at);
        Ok(self.devices[index].clone())
    }

    fn index_of(&self, id: &str) -> Result<usize, DeviceError> {
        self.devices
            .iter()
            .position(|device| device.id == id)
            .ok_or(DeviceError::NotFound)
    }
}

/// Read every device, then attach each one's permissions.
fn load_all_devices(database: &Database) -> Result<Vec<Device>, DatabaseError> {
    let connection = database.connection();

    let mut devices = connection
        .prepare("SELECT * FROM devices ORDER BY paired_at_ms")?
        .query_map([], Device::from_row)?
        .collect::<Result<Vec<_>, _>>()?;

    let mut permissions_query =
        connection.prepare("SELECT device_id, permission FROM device_permissions")?;
    let permission_rows = permissions_query.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, Permission>(1)?))
    })?;

    for permission_row in permission_rows {
        let (device_id, permission) = permission_row?;
        if let Some(device) = devices.iter_mut().find(|device| device.id == device_id) {
            device.permissions.insert(permission);
        }
    }
    Ok(devices)
}

fn insert_permissions(
    transaction: &Transaction,
    device_id: &str,
    permissions: &BTreeSet<Permission>,
) -> rusqlite::Result<()> {
    let mut insert = transaction
        .prepare("INSERT INTO device_permissions (device_id, permission) VALUES (?1, ?2)")?;
    for permission in permissions {
        insert.execute(params![device_id, permission])?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_device() -> Device {
        let description = DeviceDescription {
            name: "Phone".into(),
            public_key: [7u8; 32],
            device_type: DeviceType::Phone,
            platform: Platform::Ios,
            app_version: "1.0.0".into(),
        };
        Device::new_paired("dev_1".into(), description, Permission::read_only())
    }

    #[test]
    fn add_reload_change_and_revoke() {
        let database = Database::open_in_memory().unwrap();
        let mut store = DeviceStore::load(database.clone()).unwrap();
        store.add(sample_device()).unwrap();

        let reloaded = DeviceStore::load(database.clone()).unwrap();
        let device = reloaded.get("dev_1").unwrap();
        assert!(device.can(Permission::Read));
        assert!(!device.can(Permission::Delete));
        assert_eq!(device.public_key_bytes(), Some([7u8; 32]));
        assert_eq!(device.platform, Platform::Ios);
        assert_eq!(device.key_version, FIRST_KEY_VERSION);
        assert_eq!(device.last_connected_at_ms, None);

        store
            .set_permissions("dev_1", Permission::full_access())
            .unwrap();
        store.set_alias("dev_1", Some("Work phone")).unwrap();
        store.record_login("dev_1", Some("1.1.0".into())).unwrap();

        let reloaded = DeviceStore::load(database.clone()).unwrap();
        let device = reloaded.get("dev_1").unwrap();
        assert_eq!(device.permissions, Permission::full_access());
        assert_eq!(device.display_name(), "Work phone");
        assert_eq!(device.app_version, "1.1.0");
        assert!(device.last_connected_at_ms.is_some());

        store.set_alias("dev_1", None).unwrap();
        store.revoke("dev_1").unwrap();
        let reloaded = DeviceStore::load(database).unwrap();
        assert!(reloaded.get_active("dev_1").is_none());
        let device = reloaded.get("dev_1").unwrap();
        assert!(!device.can(Permission::Browse));
        assert_eq!(device.display_name(), "Phone");
    }

    #[test]
    fn validates_names_and_versions() {
        assert!(validate_device_name("  Lalo's iPhone ").is_ok());
        assert!(validate_device_name("").is_err());
        assert!(validate_device_name("bad\nname").is_err());

        assert!(validate_app_version("1.4.0-beta.2+77").is_ok());
        assert!(validate_app_version("").is_err());
        assert!(validate_app_version("1.0 ; drop").is_err());
    }
}
