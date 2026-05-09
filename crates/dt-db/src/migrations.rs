//! Database migrations: versioned, deterministic, applied once.

/// Initial migration: create the migrations table itself.
pub const INIT_SQL: &str = r#"
-- Migration tracking
CREATE TABLE IF NOT EXISTS dt_migrations (
    migration_id TEXT PRIMARY KEY,
    applied_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    checksum TEXT NOT NULL,
    description TEXT
);

-- Helper to check if a migration was already applied
CREATE TABLE IF NOT EXISTS dt_meta (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

INSERT OR IGNORE INTO dt_meta (key, value) VALUES ('schema_version', '1.0.0');
INSERT OR IGNORE INTO dt_meta (key, value) VALUES ('init_timestamp', strftime('%Y-%m-%dT%H:%M:%fZ', 'now'));
"#;

/// Mark a migration as applied.
pub fn record_migration(
    conn: &rusqlite::Connection,
    id: &str,
    checksum: &str,
    description: &str,
) -> Result<(), dt_core::DTError> {
    conn.execute(
        "INSERT OR IGNORE INTO dt_migrations (migration_id, checksum, description) VALUES (?1, ?2, ?3)",
        [id, checksum, description],
    )?;
    Ok(())
}

/// Check if a migration was already applied.
pub fn is_applied(conn: &rusqlite::Connection, id: &str) -> Result<bool, dt_core::DTError> {
    let count: i64 = conn.query_row(
        "SELECT count(*) FROM dt_migrations WHERE migration_id = ?1",
        [id],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connection::DbConnection;

    #[test]
    fn test_migration_tracking() {
        let db = DbConnection::open_in_memory().unwrap();
        db.execute_batch(INIT_SQL).unwrap();

        assert!(!is_applied(db.inner(), "test_001").unwrap());
        record_migration(db.inner(), "test_001", "abc123", "test migration").unwrap();
        assert!(is_applied(db.inner(), "test_001").unwrap());
    }
}
