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

use chrono::Utc;
use ulid::Ulid;

use dt_core::cas::CasStore;
use dt_event::{EventBuilder, EventStore, EventType};

use crate::edge::{KnowledgeEdge, Relation};
use crate::error::KnowledgeError;
use crate::lean::{LeanProofStatus, LeanVerdict, LeanVerification, LeanVerifier};
use crate::meta_cognition::MetaCognition;
use crate::node::{KnowledgeNode, NodeContent, NodeStatus, NodeType, Visibility};

/// Patch applied via `update`. Only `Some` fields are written.
#[derive(Debug, Default, Clone)]
pub struct NodePatch {
    pub title: Option<String>,
    pub body: Option<String>,
    pub abstract_: Option<String>,
    pub status: Option<NodeStatus>,
    pub visibility: Option<Visibility>,
    pub properties: Option<serde_json::Map<String, serde_json::Value>>,
    pub meta_cognition: Option<MetaCognition>,
    pub lean: Option<LeanVerification>,
    pub confidence: Option<f64>,
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
            && patch.meta_cognition.is_none()
            && patch.lean.is_none()
            && patch.confidence.is_none()
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
        if let Some(mc) = patch.meta_cognition {
            payload.insert("meta_cognition".into(), serde_json::to_value(mc)?);
        }
        if let Some(lean) = patch.lean {
            payload.insert("lean".into(), serde_json::to_value(lean)?);
        }
        if let Some(c) = patch.confidence {
            payload.insert(
                "confidence".into(),
                serde_json::Value::from(c.clamp(0.0, 1.0)),
            );
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

    // ── meta-cognition ──────────────────────────────────────────────────────

    /// Create a node *with* an initial meta-cognition envelope and confidence.
    /// Emits a single `knowledge.create` event.
    pub fn create_with_meta(
        &self,
        node_type: NodeType,
        content: NodeContent,
        meta: MetaCognition,
        confidence: Option<f64>,
    ) -> Result<KnowledgeNode, KnowledgeError> {
        let mut node = KnowledgeNode::new(node_type.clone(), content.clone(), &self.owner_did);
        node.meta_cognition = Some(meta.clone());
        if let Some(c) = confidence {
            node.metadata.dt_confidence = Some(c.clamp(0.0, 1.0));
        }

        let mut payload = serde_json::Map::new();
        payload.insert("node_id".into(), serde_json::Value::String(node.node_id.clone()));
        payload.insert(
            "node_type".into(),
            serde_json::Value::String(node_type.as_str().to_string()),
        );
        payload.insert(
            "content".into(),
            serde_json::json!({
                "title": content.title,
                "body": content.body,
                "abstract": content.abstract_,
            }),
        );
        payload.insert("properties".into(), serde_json::Value::Object(node.properties.clone()));
        payload.insert(
            "status".into(),
            serde_json::Value::String(node.status.as_str().to_string()),
        );
        payload.insert(
            "visibility".into(),
            serde_json::Value::String(node.visibility.as_str().to_string()),
        );
        payload.insert("meta_cognition".into(), serde_json::to_value(&meta)?);
        if let Some(c) = confidence {
            payload.insert(
                "confidence".into(),
                serde_json::Value::from(c.clamp(0.0, 1.0)),
            );
        }

        let ev = EventBuilder::new(EventType::KnowledgeCreate, &self.node_id, &self.owner_did)
            .payload(serde_json::Value::Object(payload))
            .metadata(node.metadata.clone())
            .build()?;
        self.store.append(&ev)?;
        Ok(node)
    }

    /// Replace the meta-cognition envelope of an existing node — emits
    /// `knowledge.meta_cognition`.
    pub fn set_meta_cognition(
        &self,
        node_id: &str,
        meta: MetaCognition,
        confidence: Option<f64>,
    ) -> Result<(), KnowledgeError> {
        let mut payload = serde_json::Map::new();
        payload.insert("node_id".into(), serde_json::Value::String(node_id.into()));
        payload.insert("meta_cognition".into(), serde_json::to_value(&meta)?);
        if let Some(c) = confidence {
            payload.insert(
                "confidence".into(),
                serde_json::Value::from(c.clamp(0.0, 1.0)),
            );
        }
        let ev = EventBuilder::new(
            EventType::KnowledgeMetaCognition,
            &self.node_id,
            &self.owner_did,
        )
        .payload(serde_json::Value::Object(payload))
        .build()?;
        self.store.append(&ev)?;
        Ok(())
    }

    // ── Lean 4 verification ─────────────────────────────────────────────────

    /// Create a `theorem` node from Lean source code, store the source in CAS,
    /// and pre-populate the `LeanVerification` block in `Pending` state. Emits
    /// `knowledge.create`.
    pub fn create_theorem(
        &self,
        title: impl Into<String>,
        lean_source: &str,
        cas: &CasStore,
    ) -> Result<KnowledgeNode, KnowledgeError> {
        let theorem_hash = cas.put(lean_source.as_bytes())?;
        let lean = LeanVerification::pending(theorem_hash);
        let title = title.into();

        let mut node = KnowledgeNode::new(
            NodeType::Theorem,
            NodeContent::new(title.clone(), lean_source),
            &self.owner_did,
        );
        node.lean = Some(lean.clone());

        let payload = serde_json::json!({
            "node_id": node.node_id,
            "node_type": NodeType::Theorem.as_str(),
            "content": {
                "title": title,
                "body": lean_source,
                "abstract": null,
            },
            "properties": {},
            "status": NodeStatus::Active.as_str(),
            "visibility": Visibility::Private.as_str(),
            "lean": lean,
        });

        let ev = EventBuilder::new(EventType::KnowledgeCreate, &self.node_id, &self.owner_did)
            .payload(payload)
            .metadata(node.metadata.clone())
            .build()?;
        self.store.append(&ev)?;
        Ok(node)
    }

    /// Run the supplied Lean verifier on a theorem node's source, store any
    /// resulting `.olean` artifact in CAS, and emit
    /// `knowledge.lean.verified` or `knowledge.lean.failed`.
    ///
    /// Returns the resulting `LeanVerification`.
    pub fn verify_with_lean(
        &self,
        node_id: &str,
        lean_source: &str,
        verifier: &dyn LeanVerifier,
        cas: &CasStore,
    ) -> Result<LeanVerification, KnowledgeError> {
        let theorem_hash = cas.put(lean_source.as_bytes())?;
        let verdict: LeanVerdict = verifier.verify(lean_source)?;

        let proof_hash = match &verdict.proof_artifact {
            Some(bytes) => Some(cas.put(bytes)?),
            None => None,
        };

        let lean = LeanVerification {
            verified_by_lean: verdict.status == LeanProofStatus::Verified,
            lean_theorem_hash: Some(theorem_hash),
            lean_proof_hash: proof_hash,
            lean_proof_status: verdict.status,
            verifier_version: Some(verdict.verifier_version.clone()),
            verified_at: Some(Utc::now()),
            last_error: verdict.diagnostics.clone(),
        };

        let event_type = if lean.verified_by_lean {
            EventType::KnowledgeLeanVerified
        } else {
            EventType::KnowledgeLeanFailed
        };

        let payload = serde_json::json!({
            "node_id": node_id,
            "lean": lean,
            "verifier": verifier.name(),
        });

        let ev = EventBuilder::new(event_type, &self.node_id, &self.owner_did)
            .payload(payload)
            .build()?;
        self.store.append(&ev)?;
        Ok(lean)
    }
}
