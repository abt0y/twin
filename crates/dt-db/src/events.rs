//! Event log operations: append-only, immutable events.

use serde::{Deserialize, Serialize};

/// Persistent event record stored in SQLite.
#[derive(Debug, Serialize, Deserialize)]
pub struct EventRecord {
    pub event_id: String,
    pub event_type: String,
    pub timestamp: String,
    pub node_id: String,
    pub user_id: Option<String>,
    pub payload_json: String,
    pub payload_schema_hash: Option<String>,
    pub vector_clock_json: String,
    pub prev_event_id: Option<String>,
    pub causal_deps_json: Option<String>,
    pub metadata_json: String,
    pub signature: Option<String>,
    pub content_hash: String,
}

/// Append an event to the log.
pub fn append_event(
    conn: &rusqlite::Connection,
    record: &EventRecord,
) -> Result<(), dt_core::DTError> {
    conn.execute(
        r#"
        INSERT INTO events (
            event_id, event_type, timestamp, node_id, user_id,
            payload_json, payload_schema_hash, vector_clock_json,
            prev_event_id, causal_deps_json, metadata_json, signature, content_hash
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
        "#,
        rusqlite::params![
            &record.event_id,
            &record.event_type,
            &record.timestamp,
            &record.node_id,
            &record.user_id.as_deref().unwrap_or(""),
            &record.payload_json,
            &record.payload_schema_hash.as_deref().unwrap_or(""),
            &record.vector_clock_json,
            &record.prev_event_id.as_deref().unwrap_or(""),
            &record.causal_deps_json.as_deref().unwrap_or(""),
            &record.metadata_json,
            &record.signature.as_deref().unwrap_or(""),
            &record.content_hash,
        ],
    )?;
    Ok(())
}

/// Get events by type, ordered by event_id (ULID = chronological).
pub fn get_events_by_type(
    conn: &rusqlite::Connection,
    event_type: &str,
    limit: usize,
) -> Result<Vec<EventRecord>, dt_core::DTError> {
    let mut stmt = conn.prepare(
        "SELECT * FROM events WHERE event_type = ?1 ORDER BY event_id LIMIT ?2",
    )?;
    let rows = stmt.query_map([event_type, &limit.to_string()], row_to_event)?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| dt_core::DTError::General(e.to_string()))
}

/// Get event by id.
pub fn get_event_by_id(
    conn: &rusqlite::Connection,
    event_id: &str,
) -> Result<Option<EventRecord>, dt_core::DTError> {
    let mut stmt = conn.prepare("SELECT * FROM events WHERE event_id = ?1 LIMIT 1")?;
    let mut rows = stmt.query_map([event_id], row_to_event)?;
    rows.next().transpose().map_err(|e| dt_core::DTError::General(e.to_string()))
}

/// Total event count.
pub fn event_count(conn: &rusqlite::Connection) -> Result<i64, dt_core::DTError> {
    let count: i64 = conn.query_row("SELECT count(*) FROM events", [], |row| row.get(0))?;
    Ok(count)
}

fn row_to_event(row: &rusqlite::Row) -> Result<EventRecord, rusqlite::Error> {
    Ok(EventRecord {
        event_id: row.get("event_id")?,
        event_type: row.get("event_type")?,
        timestamp: row.get("timestamp")?,
        node_id: row.get("node_id")?,
        user_id: row.get("user_id")?,
        payload_json: row.get("payload_json")?,
        payload_schema_hash: row.get("payload_schema_hash")?,
        vector_clock_json: row.get("vector_clock_json")?,
        prev_event_id: row.get("prev_event_id")?,
        causal_deps_json: row.get("causal_deps_json")?,
        metadata_json: row.get("metadata_json")?,
        signature: row.get("signature")?,
        content_hash: row.get("content_hash")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connection::DbConnection;
    use crate::schema::EVENTS_SQL;

    fn make_event(id: &str) -> EventRecord {
        EventRecord {
            event_id: id.to_string(),
            event_type: "test.event".into(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            node_id: "node-1".into(),
            user_id: None,
            payload_json: r#"{}"#.into(),
            payload_schema_hash: None,
            vector_clock_json: r#"{}"#.into(),
            prev_event_id: None,
            causal_deps_json: None,
            metadata_json: r#"{}"#.into(),
            signature: None,
            content_hash: "0".repeat(64),
        }
    }

    #[test]
    fn test_append_and_get() {
        let db = DbConnection::open_in_memory().unwrap();
        db.execute_batch(EVENTS_SQL).unwrap();

        let ev = make_event("01HQTEST000000000000000000");
        append_event(db.inner(), &ev).unwrap();

        let found = get_event_by_id(db.inner(), &ev.event_id).unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().event_type, "test.event");

        assert_eq!(event_count(db.inner()).unwrap(), 1);
    }
}
