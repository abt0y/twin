//! Universal Metadata Envelope (per Spec v1.1.0 §6).
//!
//! Every object in the DT Platform — events, knowledge nodes, edges, agent
//! observations — carries a `MetadataEnvelope`. It is the substrate for
//! provenance, AI reasoning, lineage tracking, and ownership.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A single step in the provenance chain.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Lineage {
    pub event_id: String,
    pub event_type: String,
    pub timestamp: DateTime<Utc>,
    pub node_id: String,
}

/// Embedding metadata stub (kept lightweight; vectors live in `embeddings` table).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EmbeddingMeta {
    pub model: String,
    pub dimensions: u32,
    /// Base64-encoded embedding vector. Optional — full vectors stored separately.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vector_b64: Option<String>,
}

/// Universal Metadata Envelope — attached to every object.
///
/// Field naming follows the JSON Schema in `schemas/common/metadata_envelope.schema.json`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MetadataEnvelope {
    /// Unique object identifier (UUID v7 — time-ordered).
    pub dt_id: String,

    /// Semantic version of this object instance.
    pub dt_version: String,

    /// UTC creation timestamp.
    pub dt_created_at: DateTime<Utc>,

    /// UTC last-modified timestamp.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dt_modified_at: Option<DateTime<Utc>>,

    /// Schema version used to validate this object.
    pub dt_schema_version: String,

    /// SHA3-256 hash of the schema definition (for integrity).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dt_schema_hash: Option<String>,

    /// DID of the node/user that owns this object.
    pub dt_owner: String,

    /// Node ID that originally created this object.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dt_source_node: Option<String>,

    /// Provenance chain of events that produced this object.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dt_lineage: Vec<Lineage>,

    /// User-defined tags.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dt_tags: Vec<String>,

    /// Confidence score for AI-generated content (0.0–1.0).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dt_confidence: Option<f64>,

    /// Embedding metadata (full vector lives in `embeddings` table).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dt_embeddings: Option<EmbeddingMeta>,
}

impl MetadataEnvelope {
    /// Create a new envelope with sensible defaults.
    pub fn new(owner: impl Into<String>, schema_version: impl Into<String>) -> Self {
        Self {
            dt_id: Uuid::now_v7().to_string(),
            dt_version: "1.0.0".to_string(),
            dt_created_at: Utc::now(),
            dt_modified_at: None,
            dt_schema_version: schema_version.into(),
            dt_schema_hash: None,
            dt_owner: owner.into(),
            dt_source_node: None,
            dt_lineage: Vec::new(),
            dt_tags: Vec::new(),
            dt_confidence: None,
            dt_embeddings: None,
        }
    }

    /// Begin a builder-style chain.
    pub fn builder(owner: impl Into<String>, schema_version: impl Into<String>) -> MetadataBuilder {
        MetadataBuilder {
            inner: Self::new(owner, schema_version),
        }
    }

    /// Append a lineage step (provenance).
    pub fn push_lineage(&mut self, step: Lineage) {
        self.dt_lineage.push(step);
        self.dt_modified_at = Some(Utc::now());
    }
}

/// Fluent builder for `MetadataEnvelope`.
#[derive(Debug)]
pub struct MetadataBuilder {
    inner: MetadataEnvelope,
}

impl MetadataBuilder {
    pub fn source_node(mut self, node_id: impl Into<String>) -> Self {
        self.inner.dt_source_node = Some(node_id.into());
        self
    }

    pub fn schema_hash(mut self, hash: impl Into<String>) -> Self {
        self.inner.dt_schema_hash = Some(hash.into());
        self
    }

    pub fn confidence(mut self, c: f64) -> Self {
        self.inner.dt_confidence = Some(c.clamp(0.0, 1.0));
        self
    }

    pub fn tag(mut self, tag: impl Into<String>) -> Self {
        self.inner.dt_tags.push(tag.into());
        self
    }

    pub fn tags<I, S>(mut self, tags: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.inner.dt_tags.extend(tags.into_iter().map(Into::into));
        self
    }

    pub fn lineage(mut self, step: Lineage) -> Self {
        self.inner.dt_lineage.push(step);
        self
    }

    pub fn embeddings(mut self, e: EmbeddingMeta) -> Self {
        self.inner.dt_embeddings = Some(e);
        self
    }

    pub fn build(self) -> MetadataEnvelope {
        self.inner
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_envelope_new() {
        let m = MetadataEnvelope::new("did:dt:alice", "1.0.0");
        assert_eq!(m.dt_owner, "did:dt:alice");
        assert_eq!(m.dt_schema_version, "1.0.0");
        assert!(!m.dt_id.is_empty());
        assert!(m.dt_lineage.is_empty());
    }

    #[test]
    fn test_uuid_v7_time_ordered() {
        let m1 = MetadataEnvelope::new("u", "1.0.0");
        std::thread::sleep(std::time::Duration::from_millis(2));
        let m2 = MetadataEnvelope::new("u", "1.0.0");
        // UUID v7 is time-ordered: m2's id should sort >= m1's id
        assert!(m2.dt_id > m1.dt_id);
    }

    #[test]
    fn test_builder() {
        let m = MetadataEnvelope::builder("did:dt:bob", "1.0.0")
            .source_node("node-xyz")
            .confidence(0.9)
            .tags(["work", "important"])
            .build();
        assert_eq!(m.dt_source_node.as_deref(), Some("node-xyz"));
        assert_eq!(m.dt_confidence, Some(0.9));
        assert_eq!(m.dt_tags, vec!["work", "important"]);
    }

    #[test]
    fn test_confidence_clamp() {
        let m = MetadataEnvelope::builder("u", "1.0.0").confidence(2.0).build();
        assert_eq!(m.dt_confidence, Some(1.0));
        let m = MetadataEnvelope::builder("u", "1.0.0").confidence(-0.5).build();
        assert_eq!(m.dt_confidence, Some(0.0));
    }

    #[test]
    fn test_push_lineage() {
        let mut m = MetadataEnvelope::new("u", "1.0.0");
        m.push_lineage(Lineage {
            event_id: "ev1".into(),
            event_type: "test".into(),
            timestamp: Utc::now(),
            node_id: "n1".into(),
        });
        assert_eq!(m.dt_lineage.len(), 1);
        assert!(m.dt_modified_at.is_some());
    }

    #[test]
    fn test_serde_roundtrip() {
        let m = MetadataEnvelope::builder("u", "1.0.0")
            .tag("a")
            .confidence(0.5)
            .build();
        let s = serde_json::to_string(&m).unwrap();
        let m2: MetadataEnvelope = serde_json::from_str(&s).unwrap();
        assert_eq!(m, m2);
    }
}
