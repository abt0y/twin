//! `KnowledgeProjection` — applies knowledge.* events to the SQLite
//! materialized view.

use std::sync::Arc;

use rusqlite::{params, Connection};
use tracing::{debug, warn};

use dt_db::schema::KNOWLEDGE_SQL;
use dt_event::{Event, EventError, EventType, Projection};

use crate::db::KnowledgeDb;
use crate::error::KnowledgeError;

/// Knowledge graph projection.
pub struct KnowledgeProjection {
    db: Arc<KnowledgeDb>,
}

impl KnowledgeProjection {
    /// Create a new projection. Ensures knowledge tables exist (idempotent).
    pub fn new(db: Arc<KnowledgeDb>) -> Result<Self, KnowledgeError> {
        db.execute_batch(KNOWLEDGE_SQL)?;
        Ok(Self { db })
    }

    fn apply_create(conn: &Connection, ev: &Event) -> Result<(), EventError> {
        let p = &ev.payload;
        let node_id = require_str(p, "node_id")?;
        let node_type = require_str(p, "node_type")?;
        let title = p.pointer("/content/title").and_then(|v| v.as_str()).unwrap_or("");
        let body = p.pointer("/content/body").and_then(|v| v.as_str()).unwrap_or("");
        let abstract_ = p
            .pointer("/content/abstract")
            .or_else(|| p.pointer("/content/abstract_"))
            .and_then(|v| v.as_str());

        let properties_json = p
            .get("properties")
            .map(|v| serde_json::to_string(v))
            .transpose()?;
        let metadata_json = serde_json::to_string(&ev.metadata)?;
        let status = p.get("status").and_then(|v| v.as_str()).unwrap_or("active");
        let visibility = p
            .get("visibility")
            .and_then(|v| v.as_str())
            .unwrap_or("private");
        let now = ev.timestamp.to_rfc3339();
        let content_hash = ev.content_hash.clone().unwrap_or_else(|| "0".repeat(64));

        let n = conn.execute(
            r#"
            INSERT INTO knowledge_nodes (
                node_id, node_type, title, body, abstract, properties_json,
                edges_json, metadata_json, status, visibility, created_at,
                modified_at, content_hash, deleted, fts_synced
            ) VALUES (?1,?2,?3,?4,?5,?6,'[]',?7,?8,?9,?10,?11,?12,0,0)
            ON CONFLICT(node_id) DO NOTHING
            "#,
            params![
                node_id,
                node_type,
                title,
                body,
                abstract_.unwrap_or(""),
                properties_json.unwrap_or_else(|| "{}".to_string()),
                metadata_json,
                status,
                visibility,
                now,
                now,
                content_hash,
            ],
        )?;
        if n > 0 {
            conn.execute(
                "INSERT INTO knowledge_fts (node_id, title, body) VALUES (?1, ?2, ?3)",
                params![node_id, title, body],
            )?;
            conn.execute(
                "UPDATE knowledge_nodes SET fts_synced = 1 WHERE node_id = ?1",
                params![node_id],
            )?;
            debug!(node_id, "knowledge.create projected");
        }
        Ok(())
    }

    fn apply_update(conn: &Connection, ev: &Event) -> Result<(), EventError> {
        let p = &ev.payload;
        let node_id = require_str(p, "node_id")?;

        let new_title = p.pointer("/content/title").and_then(|v| v.as_str());
        let new_body = p.pointer("/content/body").and_then(|v| v.as_str());
        let new_abstract = p
            .pointer("/content/abstract")
            .or_else(|| p.pointer("/content/abstract_"))
            .and_then(|v| v.as_str());
        let new_status = p.get("status").and_then(|v| v.as_str());
        let new_visibility = p.get("visibility").and_then(|v| v.as_str());
        let new_props = p.get("properties");

        let now = ev.timestamp.to_rfc3339();

        if let Some(t) = new_title {
            conn.execute(
                "UPDATE knowledge_nodes SET title = ?1, modified_at = ?2 WHERE node_id = ?3",
                params![t, now, node_id],
            )?;
        }
        if let Some(b) = new_body {
            conn.execute(
                "UPDATE knowledge_nodes SET body = ?1, modified_at = ?2 WHERE node_id = ?3",
                params![b, now, node_id],
            )?;
        }
        if let Some(a) = new_abstract {
            conn.execute(
                "UPDATE knowledge_nodes SET abstract = ?1, modified_at = ?2 WHERE node_id = ?3",
                params![a, now, node_id],
            )?;
        }
        if let Some(s) = new_status {
            conn.execute(
                "UPDATE knowledge_nodes SET status = ?1, modified_at = ?2 WHERE node_id = ?3",
                params![s, now, node_id],
            )?;
        }
        if let Some(v) = new_visibility {
            conn.execute(
                "UPDATE knowledge_nodes SET visibility = ?1, modified_at = ?2 WHERE node_id = ?3",
                params![v, now, node_id],
            )?;
        }
        if let Some(props) = new_props {
            let s = serde_json::to_string(props)?;
            conn.execute(
                "UPDATE knowledge_nodes SET properties_json = ?1, modified_at = ?2 WHERE node_id = ?3",
                params![s, now, node_id],
            )?;
        }
        if new_title.is_some() || new_body.is_some() {
            let row: Option<(String, String)> = conn
                .query_row(
                    "SELECT title, body FROM knowledge_nodes WHERE node_id = ?1",
                    params![node_id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .ok();
            if let Some((t, b)) = row {
                conn.execute(
                    "DELETE FROM knowledge_fts WHERE node_id = ?1",
                    params![node_id],
                )?;
                conn.execute(
                    "INSERT INTO knowledge_fts (node_id, title, body) VALUES (?1, ?2, ?3)",
                    params![node_id, t, b],
                )?;
            }
        }
        debug!(node_id, "knowledge.update projected");
        Ok(())
    }

    fn apply_delete(conn: &Connection, ev: &Event) -> Result<(), EventError> {
        let p = &ev.payload;
        let node_id = require_str(p, "node_id")?;
        let now = ev.timestamp.to_rfc3339();
        conn.execute(
            "UPDATE knowledge_nodes SET status='deleted', deleted=1, modified_at=?1 WHERE node_id=?2",
            params![now, node_id],
        )?;
        conn.execute(
            "DELETE FROM knowledge_fts WHERE node_id = ?1",
            params![node_id],
        )?;
        debug!(node_id, "knowledge.delete projected");
        Ok(())
    }

    fn apply_link(conn: &Connection, ev: &Event) -> Result<(), EventError> {
        let p = &ev.payload;
        let edge_id = require_str(p, "edge_id")?;
        let source_id = require_str(p, "source_id")?;
        let target_id = require_str(p, "target_id")?;
        let relation = require_str(p, "relation")?;
        let weight = p.get("weight").and_then(|v| v.as_f64());
        let metadata_json = serde_json::to_string(&ev.metadata)?;
        let now = ev.timestamp.to_rfc3339();
        let content_hash = ev.content_hash.clone().unwrap_or_else(|| "0".repeat(64));

        conn.execute(
            r#"
            INSERT INTO knowledge_edges (
                edge_id, source_id, target_id, relation, weight,
                metadata_json, created_at, content_hash, deleted
            ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,0)
            ON CONFLICT(edge_id) DO NOTHING
            "#,
            params![
                edge_id,
                source_id,
                target_id,
                relation,
                weight,
                metadata_json,
                now,
                content_hash,
            ],
        )?;
        debug!(edge_id, "knowledge.link projected");
        Ok(())
    }

    fn apply_unlink(conn: &Connection, ev: &Event) -> Result<(), EventError> {
        let edge_id = require_str(&ev.payload, "edge_id")?;
        conn.execute(
            "UPDATE knowledge_edges SET deleted = 1 WHERE edge_id = ?1",
            params![edge_id],
        )?;
        debug!(edge_id, "knowledge.unlink projected");
        Ok(())
    }
}

impl Projection for KnowledgeProjection {
    fn apply(&self, event: &Event) -> Result<(), EventError> {
        let result = self.db.with(|conn| match event.event_type {
            EventType::KnowledgeCreate => Self::apply_create(conn, event).map_err(Into::into),
            EventType::KnowledgeUpdate => Self::apply_update(conn, event).map_err(Into::into),
            EventType::KnowledgeDelete => Self::apply_delete(conn, event).map_err(Into::into),
            EventType::KnowledgeLink => Self::apply_link(conn, event).map_err(Into::into),
            EventType::KnowledgeUnlink => Self::apply_unlink(conn, event).map_err(Into::into),
            _ => Ok(()),
        });
        match result {
            Ok(()) => Ok(()),
            Err(KnowledgeError::Event(e)) => {
                warn!(error = %e, event_id = %event.event_id, "knowledge projection failed");
                Err(e)
            }
            Err(e) => {
                warn!(error = %e, event_id = %event.event_id, "knowledge projection failed");
                Err(EventError::Storage(e.to_string()))
            }
        }
    }

    fn name(&self) -> &str {
        "dt-knowledge"
    }
}

fn require_str<'a>(v: &'a serde_json::Value, key: &str) -> Result<&'a str, EventError> {
    v.get(key)
        .and_then(|x| x.as_str())
        .ok_or_else(|| EventError::Invalid(format!("missing required field '{}'", key)))
}
