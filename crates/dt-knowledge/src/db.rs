//! Shared, thread-safe SQLite handle used by `KnowledgeProjection` and
//! `KnowledgeRepository`.
//!
//! `rusqlite::Connection` is `Send` but not `Sync`. Wrapping it in a `Mutex`
//! makes it safe to share across threads (e.g., projections fired by the
//! event store + parallel reads).

use std::path::Path;
use std::sync::Mutex;

use rusqlite::Connection;

use crate::error::KnowledgeError;

/// Thread-safe SQLite handle.
pub struct KnowledgeDb {
    inner: Mutex<Connection>,
}

impl KnowledgeDb {
    /// Open a new connection at the given file path with DT pragmas.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, KnowledgeError> {
        let conn = Connection::open(path)?;
        Self::apply_pragmas(&conn)?;
        Ok(Self {
            inner: Mutex::new(conn),
        })
    }

    /// Open in-memory (tests).
    pub fn open_in_memory() -> Result<Self, KnowledgeError> {
        let conn = Connection::open_in_memory()?;
        Self::apply_pragmas(&conn)?;
        Ok(Self {
            inner: Mutex::new(conn),
        })
    }

    fn apply_pragmas(conn: &Connection) -> Result<(), KnowledgeError> {
        conn.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            PRAGMA foreign_keys = ON;
            PRAGMA temp_store = memory;
            PRAGMA mmap_size = 268435456;
            "#,
        )?;
        Ok(())
    }

    /// Run a closure with exclusive connection access.
    pub fn with<F, R>(&self, f: F) -> Result<R, KnowledgeError>
    where
        F: FnOnce(&Connection) -> Result<R, KnowledgeError>,
    {
        let guard = self
            .inner
            .lock()
            .map_err(|e| KnowledgeError::Storage(format!("db mutex poisoned: {}", e)))?;
        f(&guard)
    }

    /// Execute a batch of SQL statements.
    pub fn execute_batch(&self, sql: &str) -> Result<(), KnowledgeError> {
        self.with(|c| {
            c.execute_batch(sql)?;
            Ok(())
        })
    }
}
