//! Knowledge node CRUD + graph edge operations.

use serde::{Deserialize, Serialize};

/// Knowledge node record.
#[derive(Debug, Serialize, Deserialize)]
pub struct KnowledgeNode {
    pub node_id: String,
    pub node_type: String,
    pub title: String,
    pub body: String,
    pub abstract_text: Option<String>,
    pub properties_json: Option<String>,
    pub edges_json: Option<String>,
    pub metadata_json: String,
    pub status: String,
    pub visibility: String,
    pub created_at: String,
    pub modified_at: String,
    pub content_hash: String,
    pub deleted: i64,
}

/// Insert or replace a knowledge node.
pub fn upsert_node(
    conn: &rusqlite::Connection,
    node: &KnowledgeNode,
) -> Result<(), dt_core::DTError> {
    conn.execute(
        r#"
        INSERT INTO knowledge_nodes (
            node_id, node_type, title, body, abstract, properties_json,
            edges_json, metadata_json, status, visibility, created_at,
            modified_at, content_hash, deleted
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
        ON CONFLICT(node_id) DO UPDATE SET
            node_type = excluded.node_type,
            title = excluded.title,
            body = excluded.body,
            abstract = excluded.abstract,
            properties_json = excluded.properties_json,
            edges_json = excluded.edges_json,
            metadata_json = excluded.metadata_json,
            status = excluded.status,
            visibility = excluded.visibility,
            modified_at = excluded.modified_at,
            content_hash = excluded.content_hash,
            deleted = excluded.deleted
        "#,
        rusqlite::params![
            &node.node_id,
            &node.node_type,
            &node.title,
            &node.body,
            &node.abstract_text.as_deref().unwrap_or(""),
            &node.properties_json.as_deref().unwrap_or("{}"),
            &node.edges_json.as_deref().unwrap_or("[]"),
            &node.metadata_json,
            &node.status,
            &node.visibility,
            &node.created_at,
            &node.modified_at,
            &node.content_hash,
            &node.deleted.to_string(),
        ],
    )?;
    Ok(())
}

/// Soft-delete a knowledge node.
pub fn soft_delete_node(
    conn: &rusqlite::Connection,
    node_id: &str,
) -> Result<(), dt_core::DTError> {
    conn.execute(
        "UPDATE knowledge_nodes SET deleted = 1, status = 'deleted' WHERE node_id = ?1",
        [node_id],
    )?;
    Ok(())
}

/// Get node by id.
pub fn get_node(
    conn: &rusqlite::Connection,
    node_id: &str,
) -> Result<Option<KnowledgeNode>, dt_core::DTError> {
    let mut stmt = conn.prepare(
        "SELECT * FROM knowledge_nodes WHERE node_id = ?1 AND deleted = 0 LIMIT 1",
    )?;
    let mut rows = stmt.query_map([node_id], row_to_node)?;
    rows.next().transpose().map_err(|e| dt_core::DTError::General(e.to_string()))
}

/// Search nodes by title prefix.
pub fn search_nodes(
    conn: &rusqlite::Connection,
    query: &str,
    limit: usize,
) -> Result<Vec<KnowledgeNode>, dt_core::DTError> {
    let pattern = format!("%{}%", query);
    let mut stmt = conn.prepare(
        "SELECT * FROM knowledge_nodes WHERE title LIKE ?1 AND deleted = 0 ORDER BY modified_at DESC LIMIT ?2",
    )?;
    let rows = stmt.query_map([&pattern, &limit.to_string()], row_to_node)?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| dt_core::DTError::General(e.to_string()))
}

fn row_to_node(row: &rusqlite::Row) -> Result<KnowledgeNode, rusqlite::Error> {
    Ok(KnowledgeNode {
        node_id: row.get("node_id")?,
        node_type: row.get("node_type")?,
        title: row.get("title")?,
        body: row.get("body")?,
        abstract_text: row.get("abstract")?,
        properties_json: row.get("properties_json")?,
        edges_json: row.get("edges_json")?,
        metadata_json: row.get("metadata_json")?,
        status: row.get("status")?,
        visibility: row.get("visibility")?,
        created_at: row.get("created_at")?,
        modified_at: row.get("modified_at")?,
        content_hash: row.get("content_hash")?,
        deleted: row.get("deleted")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connection::DbConnection;
    use crate::schema::KNOWLEDGE_SQL;

    fn make_node(id: &str) -> KnowledgeNode {
        KnowledgeNode {
            node_id: id.into(),
            node_type: "note".into(),
            title: "Test Note".into(),
            body: "Body text".into(),
            abstract_text: None,
            properties_json: None,
            edges_json: None,
            metadata_json: r#"{}"#.into(),
            status: "active".into(),
            visibility: "private".into(),
            created_at: chrono::Utc::now().to_rfc3339(),
            modified_at: chrono::Utc::now().to_rfc3339(),
            content_hash: "0".repeat(64),
            deleted: 0,
        }
    }

    #[test]
    fn test_upsert_and_get() {
        let db = DbConnection::open_in_memory().unwrap();
        db.execute_batch(KNOWLEDGE_SQL).unwrap();

        let node = make_node("01HQTESTKN0000000000000000");
        upsert_node(db.inner(), &node).unwrap();

        let found = get_node(db.inner(), &node.node_id).unwrap();
        assert!(found.is_some());
    }

    #[test]
    fn test_soft_delete() {
        let db = DbConnection::open_in_memory().unwrap();
        db.execute_batch(KNOWLEDGE_SQL).unwrap();

        let node = make_node("01HQTESTKN0000000000000001");
        upsert_node(db.inner(), &node).unwrap();
        soft_delete_node(db.inner(), &node.node_id).unwrap();

        let found = get_node(db.inner(), &node.node_id).unwrap();
        assert!(found.is_none());
    }
}
