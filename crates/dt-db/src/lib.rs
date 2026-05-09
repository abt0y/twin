//! dt-db: Local-first database layer.
//!
//! SQLite is the primary hot-store (events, knowledge nodes, FTS, vector search).
//! DuckDB + Parquet serve as the cold/analytics tier.

use std::path::PathBuf;

pub mod connection;
pub mod events;
pub mod knowledge;
pub mod migrations;
pub mod schema;

/// Database configuration.
#[derive(Debug, Clone)]
pub struct DbConfig {
    pub sqlite_path: PathBuf,
    pub wal_mode: bool,
    pub page_size: usize,
    pub extensions: Vec<String>,
}

impl Default for DbConfig {
    fn default() -> Self {
        Self {
            sqlite_path: dt_core::resolve_dt_dir().join("db.sqlite"),
            wal_mode: true,
            page_size: 4096,
            extensions: vec![
                "fts5".into(),
                "json1".into(),
                "vss".into(),
            ],
        }
    }
}

/// Initialize the database with all schemas and migrations.
pub fn init(config: &DbConfig) -> Result<connection::DbConnection, dt_core::DTError> {
    let conn = connection::DbConnection::open(&config.sqlite_path)?;
    conn.execute_batch(migrations::INIT_SQL)?;
    conn.execute_batch(schema::EVENTS_SQL)?;
    conn.execute_batch(schema::KNOWLEDGE_SQL)?;
    conn.execute_batch(schema::VECTOR_SQL)?;
    Ok(conn)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_db_init() {
        let dir = TempDir::new().unwrap();
        let mut config = DbConfig::default();
        config.sqlite_path = dir.path().join("test.db");
        let conn = init(&config).unwrap();
        // Verify tables exist
        let count: i64 = conn
            .inner()
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='events'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }
}
