//! `KnowledgeService` — write API for the knowledge graph.
//!
//! All mutations flow through this service. Each mutation is encoded as a
//! domain event and appended to the `EventStore`, which then drives the
//! `KnowledgeProjection` to update the materialized view.
//!
//! ## Why route through events?
//! - Single source of truth (audit + replay)
//! - Sync-ready (events are the wire format)
//! - Determinism: rebuilding state == replaying events

use std::sync::Arc;

use ulid::Ulid;

use dt_event::{EventBuilder, EventStore, EventType};

use crate::edge::{KnowledgeEdge, Relation};
use crate::error::KnowledgeError;
use crate::node::{KnowledgeNode, NodeContent, NodeType, NodeStatus, Visibility};

/// Patch applied via `update`. Only `Some` fields are written.
#[derive(Debug, Default, Clone)]
pub struct NodePatch {
    pub title: Option<String>,
    pub body: Option<String>,
    pub abstract_: Option<String>,
    pub status: Option<NodeStatus>,
    pub visibility: Option<Visibility>,
    pub properties: Option<serde_json::Map<String, serde_json::Value>>,
}

/// High-level write API for the knowledge graph.
pub struct KnowledgeService {
    store: Arc<EventStore>,
    node_id: String,   // local node ID for vector clocks
    owner_did: String, // default owner DID
}

impl KnowledgeService {
    pub fn new(store: Arc<EventStore>, node_id: impl Into<String>, owner_did: impl Into<String>) -> Self {
        Self {
            store,
            node_id: node_id.into(),
            owner_did: owner_did.into(),
        }
    }

    /// Create a new knowledge node — emits `knowledge.create`.
    pub fn create(
        &self,
        node_type: NodeType,
        content: NodeContent,
    ) -> Result<KnowledgeNode, KnowledgeError> {
        let node = KnowledgeNode::new(node_type.clone(), content.clone(), &self.owner_did);

        let payload = serde_json::json!({
            "node_id": node.node_id,
            "node_type": node_type.as_str(),
            "content": {
                "title": content.title,
                "body": content.body,
                "abstract": content.abstract_,
            },
            "properties": serde_json::Value::Object(node.properties.clone()),
            "status": node.status.as_str(),
            "visibility": node.visibility.as_str(),
        });

        let ev = EventBuilder::new(EventType::KnowledgeCreate, &self.node_id, &self.owner_did)
            .payload(payload)
            .metadata(node.metadata.clone())
            .build()?;
        self.store.append(&ev)?;
        Ok(node)
    }

    /// Patch a node — emits `knowledge.update`. No-op if the patch is empty.
    pub fn update(&self, node_id: &str, patch: NodePatch) -> Result<(), KnowledgeError> {
        if patch.title.is_none()
            && patch.body.is_none()
            && patch.abstract_.is_none()
            && patch.status.is_none()
            && patch.visibility.is_none()
            && patch.properties.is_none()
        {
            return Ok(());
        }

        let mut payload = serde_json::Map::new();
        payload.insert("node_id".into(), serde_json::Value::String(node_id.into()));

        let mut content = serde_json::Map::new();
        if let Some(t) = patch.title {
            content.insert("title".into(), serde_json::Value::String(t));
        }
        if let Some(b) = patch.body {
            content.insert("body".into(), serde_json::Value::String(b));
        }
        if let Some(a) = patch.abstract_ {
            content.insert("abstract".into(), serde_json::Value::String(a));
        }
        if !content.is_empty() {
            payload.insert("content".into(), serde_json::Value::Object(content));
        }

        if let Some(s) = patch.status {
            payload.insert(
                "status".into(),
                serde_json::Value::String(s.as_str().to_string()),
            );
        }
        if let Some(v) = patch.visibility {
            payload.insert(
                "visibility".into(),
                serde_json::Value::String(v.as_str().to_string()),
            );
        }
        if let Some(p) = patch.properties {
            payload.insert("properties".into(), serde_json::Value::Object(p));
        }

        let ev = EventBuilder::new(EventType::KnowledgeUpdate, &self.node_id, &self.owner_did)
            .payload(serde_json::Value::Object(payload))
            .build()?;
        self.store.append(&ev)?;
        Ok(())
    }

    /// Soft-delete a node — emits `knowledge.delete`.
    pub fn delete(&self, node_id: &str) -> Result<(), KnowledgeError> {
        let payload = serde_json::json!({"node_id": node_id});
        let ev = EventBuilder::new(EventType::KnowledgeDelete, &self.node_id, &self.owner_did)
            .payload(payload)
            .build()?;
        self.store.append(&ev)?;
        Ok(())
    }

    /// Link two nodes — emits `knowledge.link`.
    pub fn link(
        &self,
        source_id: &str,
        target_id: &str,
        relation: Relation,
        weight: Option<f64>,
    ) -> Result<KnowledgeEdge, KnowledgeError> {
        if source_id == target_id {
            return Err(KnowledgeError::Invalid("cannot link node to itself".into()));
        }
        let edge_id = Ulid::new().to_string();
        let payload = serde_json::json!({
            "edge_id": edge_id,
            "source_id": source_id,
            "target_id": target_id,
            "relation": relation.as_str(),
            "weight": weight,
        });
        let mut edge = KnowledgeEdge::new(source_id, target_id, relation, &self.owner_did);
        edge.edge_id = edge_id;
        edge.weight = weight;

        let ev = EventBuilder::new(EventType::KnowledgeLink, &self.node_id, &self.owner_did)
            .payload(payload)
            .metadata(edge.metadata.clone())
            .build()?;
        self.store.append(&ev)?;
        Ok(edge)
    }

    /// Unlink (soft-delete an edge) — emits `knowledge.unlink`.
    pub fn unlink(&self, edge_id: &str) -> Result<(), KnowledgeError> {
        let payload = serde_json::json!({"edge_id": edge_id});
        let ev = EventBuilder::new(EventType::KnowledgeUnlink, &self.node_id, &self.owner_did)
            .payload(payload)
            .build()?;
        self.store.append(&ev)?;
        Ok(())
    }
}
