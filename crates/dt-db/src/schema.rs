//! SQLite schema definitions for the DT Platform.
//!
//! All tables are append-only (events) or soft-deleted (knowledge).
//! Content is referenced by CAS hash where possible.

/// Events table: append-only log, single source of truth.
pub const EVENTS_SQL: &str = r#"
-- Events: append-only, immutable, ordered by ULID event_id
CREATE TABLE IF NOT EXISTS events (
    event_id TEXT PRIMARY KEY,
    event_type TEXT NOT NULL,
    timestamp TEXT NOT NULL,
    node_id TEXT NOT NULL,
    user_id TEXT,
    payload_json TEXT NOT NULL,
    payload_schema_hash TEXT,
    vector_clock_json TEXT NOT NULL,
    prev_event_id TEXT,
    causal_deps_json TEXT,
    metadata_json TEXT NOT NULL,
    signature TEXT,
    content_hash TEXT NOT NULL
);

-- Indexes for efficient querying
CREATE INDEX IF NOT EXISTS idx_events_type ON events(event_type);
CREATE INDEX IF NOT EXISTS idx_events_timestamp ON events(timestamp);
CREATE INDEX IF NOT EXISTS idx_events_node ON events(node_id);
CREATE INDEX IF NOT EXISTS idx_events_content_hash ON events(content_hash);

-- Virtual FTS5 table for full-text search over event payloads
CREATE VIRTUAL TABLE IF NOT EXISTS events_fts USING fts5(
    event_id,
    payload_text,
    tokenize='porter unicode61'
);
"#;

/// Knowledge nodes: user-facing graph entities.
pub const KNOWLEDGE_SQL: &str = r#"
-- Knowledge nodes: CRDT-friendly with metadata envelope
CREATE TABLE IF NOT EXISTS knowledge_nodes (
    node_id TEXT PRIMARY KEY,
    node_type TEXT NOT NULL,
    title TEXT NOT NULL,
    body TEXT NOT NULL,
    abstract TEXT,
    properties_json TEXT,
    edges_json TEXT,
    metadata_json TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'active',
    visibility TEXT NOT NULL DEFAULT 'private',
    created_at TEXT NOT NULL,
    modified_at TEXT NOT NULL,
    content_hash TEXT NOT NULL,
    deleted INTEGER NOT NULL DEFAULT 0,
    fts_synced INTEGER NOT NULL DEFAULT 0
);

-- Knowledge node full-text search
CREATE VIRTUAL TABLE IF NOT EXISTS knowledge_fts USING fts5(
    node_id,
    title,
    body,
    tokenize='porter unicode61'
);

-- Knowledge edges: explicit graph links
CREATE TABLE IF NOT EXISTS knowledge_edges (
    edge_id TEXT PRIMARY KEY,
    source_id TEXT NOT NULL REFERENCES knowledge_nodes(node_id),
    target_id TEXT NOT NULL REFERENCES knowledge_nodes(node_id),
    relation TEXT NOT NULL,
    weight REAL,
    metadata_json TEXT,
    created_at TEXT NOT NULL,
    content_hash TEXT NOT NULL,
    deleted INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_kn_type ON knowledge_nodes(node_type);
CREATE INDEX IF NOT EXISTS idx_kn_status ON knowledge_nodes(status);
CREATE INDEX IF NOT EXISTS idx_ke_source ON knowledge_edges(source_id);
CREATE INDEX IF NOT EXISTS idx_ke_target ON knowledge_edges(target_id);
CREATE INDEX IF NOT EXISTS idx_ke_relation ON knowledge_edges(relation);
"#;

/// Vector search: approximate nearest neighbors for embeddings.
pub const VECTOR_SQL: &str = r#"
-- Embeddings: one row per knowledge node, vector stored as float blob
CREATE TABLE IF NOT EXISTS embeddings (
    embedding_id TEXT PRIMARY KEY,
    node_id TEXT NOT NULL UNIQUE REFERENCES knowledge_nodes(node_id),
    model TEXT NOT NULL,
    dimensions INTEGER NOT NULL,
    vector BLOB NOT NULL,
    generated_at TEXT NOT NULL,
    content_hash TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_emb_node ON embeddings(node_id);
CREATE INDEX IF NOT EXISTS idx_emb_model ON embeddings(model);

-- sqlite-vss extension creates virtual tables for vector search.
-- This is a stub; actual vss table creation requires the extension loaded.
CREATE TABLE IF NOT EXISTS vss_meta (
    table_name TEXT PRIMARY KEY,
    index_config TEXT NOT NULL,
    created_at TEXT NOT NULL
);
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connection::DbConnection;

    #[test]
    fn test_schema_creation() {
        let db = DbConnection::open_in_memory().unwrap();
        db.execute_batch(EVENTS_SQL).unwrap();
        db.execute_batch(KNOWLEDGE_SQL).unwrap();
        db.execute_batch(VECTOR_SQL).unwrap();

        let tables: Vec<String> = db
            .inner()
            .prepare("SELECT name FROM sqlite_master WHERE type='table'")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        assert!(tables.contains(&"events".to_string()));
        assert!(tables.contains(&"knowledge_nodes".to_string()));
        assert!(tables.contains(&"knowledge_edges".to_string()));
        assert!(tables.contains(&"embeddings".to_string()));
    }
}
