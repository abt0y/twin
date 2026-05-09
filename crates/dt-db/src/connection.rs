//! Database connection wrapper with WAL, JSON1, and custom extension loading.

use rusqlite::Connection;
use std::path::Path;

/// Wrapped connection with DT-specific pragmas.
pub struct DbConnection {
    conn: Connection,
}

impl DbConnection {
    /// Open a SQLite connection with DT pragmas.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, dt_core::DTError> {
        let conn = Connection::open(path)?;
        Self::apply_pragmas(&conn)?;
        Ok(DbConnection { conn })
    }

    /// Open in-memory (for tests).
    pub fn open_in_memory() -> Result<Self, dt_core::DTError> {
        let conn = Connection::open_in_memory()?;
        Self::apply_pragmas(&conn)?;
        Ok(DbConnection { conn })
    }

    fn apply_pragmas(conn: &Connection) -> Result<(), dt_core::DTError> {
        conn.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            PRAGMA foreign_keys = ON;
            PRAGMA temp_store = memory;
            PRAGMA mmap_size = 268435456;
            PRAGMA page_size = 4096;
            "#,
        )?;
        Ok(())
    }

    /// Execute a batch of SQL statements.
    pub fn execute_batch(&self, sql: &str) -> Result<(), dt_core::DTError> {
        self.conn.execute_batch(sql)?;
        Ok(())
    }

    /// Access the inner rusqlite Connection.
    pub fn inner(&self) -> &Connection {
        &self.conn
    }

    /// Execute with params.
    pub fn execute<P: rusqlite::Params>(
        &self,
        sql: &str,
        params: P,
    ) -> Result<usize, dt_core::DTError> {
        Ok(self.conn.execute(sql, params)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_in_memory() {
        let db = DbConnection::open_in_memory().unwrap();
        let one: i64 = db.inner().query_row("SELECT 1", [], |row| row.get(0)).unwrap();
        assert_eq!(one, 1);
    }
}
