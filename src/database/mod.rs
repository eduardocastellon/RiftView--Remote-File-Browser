//! The SQLite database (`riftview.db` in the data folder) that holds the
//! relational data: shares, trusted devices and device permissions.
//!
//! Settings and secrets that aren't relational stay as files: `config.json`,
//! `admin_token`, `cert.pem` and `key.pem`.
//!
//! **Schema changes** are numbered SQL files in `migrations/`. SQLite keeps
//! a counter in the file (`PRAGMA user_version`) recording how many have been
//! applied; on startup we run the ones it hasn't seen yet. To change the
//! schema, add a new file to [`MIGRATIONS`]; never edit one that has shipped.

use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};

use rusqlite::Connection;

use crate::platform;

pub const DATABASE_FILE_NAME: &str = "riftview.db";

/// Every schema change, oldest first. Entry N moves the database from
/// version N to version N + 1.
const MIGRATIONS: &[&str] = &[include_str!("migrations/001_devices_and_shares.sql")];

#[derive(Debug, thiserror::Error)]
pub enum DatabaseError {
    #[error("database: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error(
        "{DATABASE_FILE_NAME} was written by a newer RiftView \
         (schema version {found}, this build knows up to {known})"
    )]
    TooNew { found: usize, known: usize },

    #[error("database file permissions: {0}")]
    Io(#[from] std::io::Error),
}

/// A shared handle on the open database. Cloning is cheap: every clone uses
/// the same connection.
///
/// There is a single connection behind a mutex. RiftView writes rarely (pairing,
/// login, admin commands), and the stores keep an in-memory copy for reads,
/// so one connection is plenty and keeps things simple.
#[derive(Clone)]
pub struct Database {
    connection: Arc<Mutex<Connection>>,
}

impl Database {
    /// Open (or create) the database in `data_dir` and bring its schema up
    /// to date.
    pub fn open(data_dir: &Path) -> Result<Self, DatabaseError> {
        let path = data_dir.join(DATABASE_FILE_NAME);
        let connection = Connection::open(&path)?;
        platform::make_file_private(&path)?;
        Self::set_up(connection)
    }

    /// A throwaway database that lives only in memory (for tests).
    pub fn open_in_memory() -> Result<Self, DatabaseError> {
        Self::set_up(Connection::open_in_memory()?)
    }

    fn set_up(mut connection: Connection) -> Result<Self, DatabaseError> {
        // SQLite leaves foreign keys off unless asked, per connection.
        connection.pragma_update(None, "foreign_keys", true)?;
        // Write-ahead logging: safer on crashes and lets reads continue while
        // a write is in progress.
        connection.pragma_update(None, "journal_mode", "WAL")?;

        apply_migrations(&mut connection)?;

        Ok(Self {
            connection: Arc::new(Mutex::new(connection)),
        })
    }

    /// Borrow the connection. Keep the guard only for the queries at hand:
    /// never hold it across an `.await`.
    pub fn connection(&self) -> MutexGuard<'_, Connection> {
        // A panic mid-query can't leave the connection unusable (SQLite rolls
        // back unfinished transactions), so carry on after one.
        self.connection
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// Run the migrations this database hasn't had yet, each in its own
/// transaction, so a failure leaves the database at the last good version.
fn apply_migrations(connection: &mut Connection) -> Result<(), DatabaseError> {
    let current_version: usize =
        connection.pragma_query_value(None, "user_version", |row| row.get(0))?;

    if current_version > MIGRATIONS.len() {
        return Err(DatabaseError::TooNew {
            found: current_version,
            known: MIGRATIONS.len(),
        });
    }

    for (index, migration_sql) in MIGRATIONS.iter().enumerate().skip(current_version) {
        let new_version = index + 1;
        let transaction = connection.transaction()?;
        transaction.execute_batch(migration_sql)?;
        transaction.pragma_update(None, "user_version", new_version)?;
        transaction.commit()?;
        tracing::info!(version = new_version, "database schema updated");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schema_version(database: &Database) -> usize {
        database
            .connection()
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap()
    }

    #[test]
    fn creates_schema_once_and_reopens_cleanly() {
        let data_dir = tempfile::tempdir().unwrap();

        let database = Database::open(data_dir.path()).unwrap();
        assert_eq!(schema_version(&database), MIGRATIONS.len());
        drop(database);

        // Opening again must not try to re-run the migrations.
        let database = Database::open(data_dir.path()).unwrap();
        assert_eq!(schema_version(&database), MIGRATIONS.len());
    }

    #[test]
    fn refuses_a_database_from_a_newer_build() {
        let data_dir = tempfile::tempdir().unwrap();
        let database = Database::open(data_dir.path()).unwrap();
        database
            .connection()
            .pragma_update(None, "user_version", MIGRATIONS.len() + 1)
            .unwrap();
        drop(database);

        assert!(matches!(
            Database::open(data_dir.path()),
            Err(DatabaseError::TooNew { .. })
        ));
    }
}
