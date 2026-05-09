//! `KnowledgeRepository` — read-only API over the materialized view.

use std::sync::Arc;

use rusqlite::{params, Connection};

use dt_event::MetadataEnvelope;

use crate::db::KnowledgeDb;
use crate::edge::{KnowledgeEdge, Relation};
use crate::error::KnowledgeError;
use crate::node::{KnowledgeNode, NodeContent, NodeStatus, NodeType, Visibility};

/// Direction filter for `neighbors`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NeighborDirection {
    Outgoing,
    Incoming,
    Both,
}

/// Read-only knowledge graph queries.
pub struct KnowledgeRepository {
    db: Arc<KnowledgeDb>,
}

impl KnowledgeRepository {
    pub fn new(db: Arc<KnowledgeDb>) -> Self {
        Self { db }
    }

    pub fn get(&self, node_id: &str) -> Result<Option<KnowledgeNode>, KnowledgeError> {
        self.db.with(|c| {
            query_node_one(
                c,
                "SELECT * FROM knowledge_nodes WHERE node_id = ?1 AND deleted = 0 LIMIT 1",
                params![node_id],
            )
        })
    }

    pub fn get_including_deleted(
        &self,
        node_id: &str,
    ) -> Result<Option<KnowledgeNode>, KnowledgeError> {
        self.db.with(|c| {
            query_node_one(
                c,
                "SELECT * FROM knowledge_nodes WHERE node_id = ?1 LIMIT 1",
                params![node_id],
            )
        })
    }

    pub fn list(
        &self,
        node_type: Option<&NodeType>,
        limit: usize,
    ) -> Result<Vec<KnowledgeNode>, KnowledgeError> {
        self.db.with(|c| match node_type {
            Some(t) => query_nodes_many(
                c,
                "SELECT * FROM knowledge_nodes WHERE node_type = ?1 AND deleted = 0 ORDER BY modified_at DESC LIMIT ?2",
                params![t.as_str(), limit as i64],
            ),
            None => query_nodes_many(
                c,
                "SELECT * FROM knowledge_nodes WHERE deleted = 0 ORDER BY modified_at DESC LIMIT ?1",
                params![limit as i64],
            ),
        })
    }

    pub fn count(&self) -> Result<u64, KnowledgeError> {
        self.db.with(|c| {
            let n: i64 = c.query_row(
                "SELECT count(*) FROM knowledge_nodes WHERE deleted = 0",
                [],
                |r| r.get(0),
            )?;
            Ok(n as u64)
        })
    }

    pub fn search(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<KnowledgeNode>, KnowledgeError> {
        if query.trim().is_empty() {
            return Ok(Vec::new());
        }
        let q = sanitize_fts(query);
        self.db.with(|c| {
            query_nodes_many(
                c,
                r#"SELECT n.* FROM knowledge_nodes n
                   JOIN knowledge_fts f ON f.node_id = n.node_id
                   WHERE knowledge_fts MATCH ?1 AND n.deleted = 0
                   ORDER BY bm25(knowledge_fts)
                   LIMIT ?2"#,
                params![q, limit as i64],
            )
        })
    }

    pub fn get_edge(&self, edge_id: &str) -> Result<Option<KnowledgeEdge>, KnowledgeError> {
        self.db.with(|c| {
            query_edge_one(
                c,
                "SELECT * FROM knowledge_edges WHERE edge_id = ?1 AND deleted = 0 LIMIT 1",
                params![edge_id],
            )
        })
    }

    pub fn neighbors(
        &self,
        node_id: &str,
        direction: NeighborDirection,
        relation: Option<&Relation>,
        limit: usize,
    ) -> Result<Vec<KnowledgeEdge>, KnowledgeError> {
        let rel_filter = relation.map(|r| r.as_str().to_string());
        self.db.with(|c| match direction {
            NeighborDirection::Outgoing => {
                query_edges_for(c, "source_id", node_id, rel_filter.as_deref(), limit)
            }
            NeighborDirection::Incoming => {
                query_edges_for(c, "target_id", node_id, rel_filter.as_deref(), limit)
            }
            NeighborDirection::Both => {
                let mut out =
                    query_edges_for(c, "source_id", node_id, rel_filter.as_deref(), limit)?;
                let inc =
                    query_edges_for(c, "target_id", node_id, rel_filter.as_deref(), limit)?;
                out.extend(inc);
                out.truncate(limit);
                Ok(out)
            }
        })
    }

    /// BFS walk up to `depth` hops.
    pub fn walk(
        &self,
        start_id: &str,
        depth: usize,
        direction: NeighborDirection,
    ) -> Result<Vec<KnowledgeNode>, KnowledgeError> {
        use std::collections::{HashSet, VecDeque};
        let mut visited: HashSet<String> = HashSet::new();
        let mut queue: VecDeque<(String, usize)> = VecDeque::new();
        let mut result: Vec<KnowledgeNode> = Vec::new();

        queue.push_back((start_id.to_string(), 0));
        while let Some((id, d)) = queue.pop_front() {
            if !visited.insert(id.clone()) {
                continue;
            }
            if let Some(n) = self.get(&id)? {
                result.push(n);
            }
            if d >= depth {
                continue;
            }
            for edge in self.neighbors(&id, direction, None, 256)? {
                let next = match direction {
                    NeighborDirection::Outgoing => edge.target_id,
                    NeighborDirection::Incoming => edge.source_id,
                    NeighborDirection::Both => {
                        if edge.source_id == id {
                            edge.target_id
                        } else {
                            edge.source_id
                        }
                    }
                };
                if !visited.contains(&next) {
                    queue.push_back((next, d + 1));
                }
            }
        }
        Ok(result)
    }
}

// ---- query helpers (own statement lifetime within helper scope) -------------

fn query_node_one<P: rusqlite::Params>(
    conn: &Connection,
    sql: &str,
    params: P,
) -> Result<Option<KnowledgeNode>, KnowledgeError> {
    let mut stmt = conn.prepare(sql)?;
    let result = stmt
        .query_row(params, row_to_node)
        .map(Some)
        .or_else(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            other => Err(other),
        })?;
    Ok(result)
}

fn query_nodes_many<P: rusqlite::Params>(
    conn: &Connection,
    sql: &str,
    params: P,
) -> Result<Vec<KnowledgeNode>, KnowledgeError> {
    let mut stmt = conn.prepare(sql)?;
    let rows: Vec<KnowledgeNode> = stmt
        .query_map(params, row_to_node)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

fn query_edge_one<P: rusqlite::Params>(
    conn: &Connection,
    sql: &str,
    params: P,
) -> Result<Option<KnowledgeEdge>, KnowledgeError> {
    let mut stmt = conn.prepare(sql)?;
    let result = stmt
        .query_row(params, row_to_edge)
        .map(Some)
        .or_else(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            other => Err(other),
        })?;
    Ok(result)
}

fn query_edges_for(
    conn: &Connection,
    col: &str,
    node_id: &str,
    relation: Option<&str>,
    limit: usize,
) -> Result<Vec<KnowledgeEdge>, KnowledgeError> {
    let edges: Vec<KnowledgeEdge> = match relation {
        Some(r) => {
            let sql = format!(
                "SELECT * FROM knowledge_edges WHERE {} = ?1 AND relation = ?2 AND deleted = 0 ORDER BY created_at DESC LIMIT ?3",
                col
            );
            let mut stmt = conn.prepare(&sql)?;
            let rows: Vec<KnowledgeEdge> = stmt
                .query_map(params![node_id, r, limit as i64], row_to_edge)?
                .collect::<Result<Vec<_>, _>>()?;
            rows
        }
        None => {
            let sql = format!(
                "SELECT * FROM knowledge_edges WHERE {} = ?1 AND deleted = 0 ORDER BY created_at DESC LIMIT ?2",
                col
            );
            let mut stmt = conn.prepare(&sql)?;
            let rows: Vec<KnowledgeEdge> = stmt
                .query_map(params![node_id, limit as i64], row_to_edge)?
                .collect::<Result<Vec<_>, _>>()?;
            rows
        }
    };
    Ok(edges)
}

/// Wrap each token in double quotes for FTS5 phrase-safe search.
fn sanitize_fts(q: &str) -> String {
    q.split_whitespace()
        .map(|w| {
            let safe: String = w.chars().filter(|c| !"\"".contains(*c)).collect();
            format!("\"{}\"", safe)
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn row_to_node(row: &rusqlite::Row) -> rusqlite::Result<KnowledgeNode> {
    let node_id: String = row.get("node_id")?;
    let node_type: String = row.get("node_type")?;
    let title: String = row.get("title")?;
    let body: String = row.get("body")?;
    let abstract_: Option<String> = row.get("abstract").ok();
    let properties_json: Option<String> = row.get("properties_json").ok();
    let metadata_json: String = row.get("metadata_json")?;
    let status: String = row.get("status")?;
    let visibility: String = row.get("visibility")?;
    let created_at: String = row.get("created_at")?;
    let modified_at: String = row.get("modified_at")?;

    let properties: serde_json::Map<String, serde_json::Value> = properties_json
        .as_deref()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default();

    let metadata: MetadataEnvelope = serde_json::from_str(&metadata_json).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
    })?;

    let parse_dt = |s: &str| {
        chrono::DateTime::parse_from_rfc3339(s)
            .map(|d| d.with_timezone(&chrono::Utc))
            .unwrap_or_else(|_| chrono::Utc::now())
    };

    Ok(KnowledgeNode {
        node_id,
        node_type: NodeType::parse(&node_type),
        content: NodeContent {
            title,
            body,
            abstract_: abstract_.filter(|s| !s.is_empty()),
        },
        properties,
        metadata,
        status: NodeStatus::parse(&status),
        visibility: Visibility::parse(&visibility),
        created_at: parse_dt(&created_at),
        modified_at: parse_dt(&modified_at),
    })
}

fn row_to_edge(row: &rusqlite::Row) -> rusqlite::Result<KnowledgeEdge> {
    let edge_id: String = row.get("edge_id")?;
    let source_id: String = row.get("source_id")?;
    let target_id: String = row.get("target_id")?;
    let relation: String = row.get("relation")?;
    let weight: Option<f64> = row.get("weight").ok();
    let metadata_json: Option<String> = row.get("metadata_json").ok();
    let created_at: String = row.get("created_at")?;

    let metadata: MetadataEnvelope = metadata_json
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_else(|| MetadataEnvelope::new("did:dt:unknown", "1.0.0"));

    let parse_dt = |s: &str| {
        chrono::DateTime::parse_from_rfc3339(s)
            .map(|d| d.with_timezone(&chrono::Utc))
            .unwrap_or_else(|_| chrono::Utc::now())
    };

    Ok(KnowledgeEdge {
        edge_id,
        source_id,
        target_id,
        relation: Relation::parse(&relation),
        weight,
        metadata,
        created_at: parse_dt(&created_at),
    })
}
