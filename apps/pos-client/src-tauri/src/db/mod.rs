//! Encrypted local database (SQLite + SQLCipher, WAL).
//!
//! Only Rust touches this. The key is derived from the hardware fingerprint
//! (`pos_hwid::DatabaseKey`) and exists only in memory.

mod migrations;

use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use pos_hwid::DatabaseKey;
use rusqlite::{Connection, ErrorCode};

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    /// The file exists but cannot be decrypted with this machine's key.
    #[error("the database cannot be decrypted on this machine")]
    WrongKey,
    #[error("the database was created by a newer version of the app (schema v{0})")]
    FromNewerVersion(i64),
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
}

pub struct Database {
    conn: Mutex<Connection>,
}

impl std::fmt::Debug for Database {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Database")
    }
}

impl Database {
    /// Opens (creating if needed) and migrates the database at `path`.
    pub fn open(path: &Path, key: &DatabaseKey) -> Result<Self, DbError> {
        Self::setup(Connection::open(path)?, key)
    }

    #[cfg(test)]
    pub fn open_in_memory(key: &DatabaseKey) -> Result<Self, DbError> {
        Self::setup(Connection::open_in_memory()?, key)
    }

    fn setup(mut conn: Connection, key: &DatabaseKey) -> Result<Self, DbError> {
        // Must be the first statement on the connection.
        conn.pragma_update(None, "key", key.sqlcipher_pragma_value().as_str())?;

        // Touch the schema: this is where a wrong key surfaces.
        match conn.query_row("SELECT count(*) FROM sqlite_master", [], |row| {
            row.get::<_, i64>(0)
        }) {
            Ok(_) => {}
            Err(rusqlite::Error::SqliteFailure(e, _)) if e.code == ErrorCode::NotADatabase => {
                return Err(DbError::WrongKey)
            }
            Err(e) => return Err(e.into()),
        }

        // Durability over speed: a till must not lose a completed sale on power loss.
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = FULL;
             PRAGMA foreign_keys = ON;
             PRAGMA busy_timeout = 5000;
             PRAGMA trusted_schema = OFF;",
        )?;

        migrations::apply(&mut conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Exclusive access to the connection. Callers run on a blocking thread.
    pub fn conn(&self) -> MutexGuard<'_, Connection> {
        // A panic while holding the lock leaves SQLite itself consistent
        // (every write is transactional), so poisoning is safe to ignore.
        self.conn
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}
