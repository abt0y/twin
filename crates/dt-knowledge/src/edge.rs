//! Strongly-typed `KnowledgeEdge` model — directed, labeled, weighted.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use ulid::Ulid;

use dt_event::MetadataEnvelope;

/// Edge relation type per spec.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Relation {
    ParentOf,
    ChildOf,
    References,
    RelatedTo,
    DependsOn,
    Blocks,
    CreatedBy,
    Mentions,
    TaggedWith,
    LocatedAt,
    OccurredAt,
    /// Application-defined custom relation.
    #[serde(untagged)]
    Custom(String),
}

impl Relation {
    pub fn as_str(&self) -> &str {
        match self {
            Relation::ParentOf => "parent_of",
            Relation::ChildOf => "child_of",
            Relation::References => "references",
            Relation::RelatedTo => "related_to",
            Relation::DependsOn => "depends_on",
            Relation::Blocks => "blocks",
            Relation::CreatedBy => "created_by",
            Relation::Mentions => "mentions",
            Relation::TaggedWith => "tagged_with",
            Relation::LocatedAt => "located_at",
            Relation::OccurredAt => "occurred_at",
            Relation::Custom(s) => s,
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "parent_of" => Relation::ParentOf,
            "child_of" => Relation::ChildOf,
            "references" => Relation::References,
            "related_to" => Relation::RelatedTo,
            "depends_on" => Relation::DependsOn,
            "blocks" => Relation::Blocks,
            "created_by" => Relation::CreatedBy,
            "mentions" => Relation::Mentions,
            "tagged_with" => Relation::TaggedWith,
            "located_at" => Relation::LocatedAt,
            "occurred_at" => Relation::OccurredAt,
            other => Relation::Custom(other.to_string()),
        }
    }
}

/// A directed, labeled edge between two knowledge nodes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KnowledgeEdge {
    pub edge_id: String,
    pub source_id: String,
    pub target_id: String,
    pub relation: Relation,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub weight: Option<f64>,
    pub metadata: MetadataEnvelope,
    pub created_at: DateTime<Utc>,
}

impl KnowledgeEdge {
    pub fn new(
        source_id: impl Into<String>,
        target_id: impl Into<String>,
        relation: Relation,
        owner: impl Into<String>,
    ) -> Self {
        Self {
            edge_id: Ulid::new().to_string(),
            source_id: source_id.into(),
            target_id: target_id.into(),
            relation,
            weight: None,
            metadata: MetadataEnvelope::new(owner, "1.0.0"),
            created_at: Utc::now(),
        }
    }

    pub fn with_weight(mut self, w: f64) -> Self {
        self.weight = Some(w.clamp(0.0, 1.0));
        self
    }

    pub fn content_hash(&self) -> Result<String, serde_json::Error> {
        let stable = serde_json::json!({
            "edge_id": self.edge_id,
            "source_id": self.source_id,
            "target_id": self.target_id,
            "relation": self.relation,
            "weight": self.weight,
        });
        let bytes = serde_json::to_vec(&stable)?;
        Ok(dt_core::sha3_256_hex(&bytes))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_edge_new() {
        let e = KnowledgeEdge::new("a", "b", Relation::References, "did:u").with_weight(0.5);
        assert_eq!(e.source_id, "a");
        assert_eq!(e.target_id, "b");
        assert_eq!(e.relation, Relation::References);
        assert_eq!(e.weight, Some(0.5));
    }

    #[test]
    fn test_relation_roundtrip() {
        for r in &[
            Relation::References,
            Relation::ChildOf,
            Relation::Custom("supersedes".into()),
        ] {
            let s = r.as_str().to_string();
            assert_eq!(Relation::parse(&s), *r);
        }
    }

    #[test]
    fn test_edge_serde() {
        let e = KnowledgeEdge::new("a", "b", Relation::Blocks, "u");
        let s = serde_json::to_string(&e).unwrap();
        let e2: KnowledgeEdge = serde_json::from_str(&s).unwrap();
        assert_eq!(e, e2);
    }
}
