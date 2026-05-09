//! Strongly-typed `KnowledgeNode` model.
//!
//! Mirrors `schemas/knowledge/knowledge_node.schema.json` (v1.0.0).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use ulid::Ulid;

use dt_event::MetadataEnvelope;

use crate::lean::LeanVerification;
use crate::meta_cognition::MetaCognition;

/// Domain category of a knowledge node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Hash)]
#[serde(rename_all = "snake_case")]
pub enum NodeType {
    Note,
    Task,
    Project,
    Person,
    Organization,
    Document,
    Media,
    Link,
    Concept,
    Agent,
    Event,
    Reminder,
    Habit,
    Goal,
    Metric,
    // ── meta-cognition extensions ───────────────────────────────────────────
    /// A reflective annotation on another node (the act of thinking *about* a
    /// thought). Typically links via `Relation::References` to its subject.
    Reflection,
    /// A tentative claim awaiting verification or counter-evidence.
    Hypothesis,
    /// A synthesized conclusion derived from one or more hypotheses /
    /// evidence chains.
    Insight,
    /// A recurring shape of reasoning (analogy, abstraction, decomposition).
    CognitivePattern,
    /// An open question the user wants the twin to keep alive.
    MetaQuestion,
    /// A formal statement that may be verified by an external prover (Lean 4).
    Theorem,
    /// Evidence supporting (or contradicting) a hypothesis or insight.
    Evidence,
    /// Application-defined custom type.
    #[serde(untagged)]
    Custom(String),
}

impl NodeType {
    pub fn as_str(&self) -> &str {
        match self {
            NodeType::Note => "note",
            NodeType::Task => "task",
            NodeType::Project => "project",
            NodeType::Person => "person",
            NodeType::Organization => "organization",
            NodeType::Document => "document",
            NodeType::Media => "media",
            NodeType::Link => "link",
            NodeType::Concept => "concept",
            NodeType::Agent => "agent",
            NodeType::Event => "event",
            NodeType::Reminder => "reminder",
            NodeType::Habit => "habit",
            NodeType::Goal => "goal",
            NodeType::Metric => "metric",
            NodeType::Reflection => "reflection",
            NodeType::Hypothesis => "hypothesis",
            NodeType::Insight => "insight",
            NodeType::CognitivePattern => "cognitive_pattern",
            NodeType::MetaQuestion => "meta_question",
            NodeType::Theorem => "theorem",
            NodeType::Evidence => "evidence",
            NodeType::Custom(s) => s,
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "note" => NodeType::Note,
            "task" => NodeType::Task,
            "project" => NodeType::Project,
            "person" => NodeType::Person,
            "organization" => NodeType::Organization,
            "document" => NodeType::Document,
            "media" => NodeType::Media,
            "link" => NodeType::Link,
            "concept" => NodeType::Concept,
            "agent" => NodeType::Agent,
            "event" => NodeType::Event,
            "reminder" => NodeType::Reminder,
            "habit" => NodeType::Habit,
            "goal" => NodeType::Goal,
            "metric" => NodeType::Metric,
            "reflection" => NodeType::Reflection,
            "hypothesis" => NodeType::Hypothesis,
            "insight" => NodeType::Insight,
            "cognitive_pattern" => NodeType::CognitivePattern,
            "meta_question" => NodeType::MetaQuestion,
            "theorem" => NodeType::Theorem,
            "evidence" => NodeType::Evidence,
            other => NodeType::Custom(other.to_string()),
        }
    }

    /// Whether this node type is part of the meta-cognition family.
    pub fn is_meta_cognitive(&self) -> bool {
        matches!(
            self,
            NodeType::Reflection
                | NodeType::Hypothesis
                | NodeType::Insight
                | NodeType::CognitivePattern
                | NodeType::MetaQuestion
                | NodeType::Theorem
                | NodeType::Evidence
        )
    }
}

/// Lifecycle status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeStatus {
    Active,
    Archived,
    Deleted,
    Draft,
    Pending,
}

impl NodeStatus {
    pub fn as_str(&self) -> &str {
        match self {
            NodeStatus::Active => "active",
            NodeStatus::Archived => "archived",
            NodeStatus::Deleted => "deleted",
            NodeStatus::Draft => "draft",
            NodeStatus::Pending => "pending",
        }
    }
    pub fn parse(s: &str) -> Self {
        match s {
            "archived" => NodeStatus::Archived,
            "deleted" => NodeStatus::Deleted,
            "draft" => NodeStatus::Draft,
            "pending" => NodeStatus::Pending,
            _ => NodeStatus::Active,
        }
    }
}

/// Visibility level (who can see this node).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Visibility {
    Private,
    Shared,
    Public,
}

impl Visibility {
    pub fn as_str(&self) -> &str {
        match self {
            Visibility::Private => "private",
            Visibility::Shared => "shared",
            Visibility::Public => "public",
        }
    }
    pub fn parse(s: &str) -> Self {
        match s {
            "shared" => Visibility::Shared,
            "public" => Visibility::Public,
            _ => Visibility::Private,
        }
    }
}

/// Body of a knowledge node — title + Markdown body + optional abstract.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NodeContent {
    pub title: String,
    pub body: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub abstract_: Option<String>,
}

impl NodeContent {
    pub fn new(title: impl Into<String>, body: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            body: body.into(),
            abstract_: None,
        }
    }
}

/// A node in the knowledge graph.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KnowledgeNode {
    pub node_id: String,
    pub node_type: NodeType,
    pub content: NodeContent,
    #[serde(default)]
    pub properties: serde_json::Map<String, serde_json::Value>,
    pub metadata: MetadataEnvelope,
    pub status: NodeStatus,
    pub visibility: Visibility,
    pub created_at: DateTime<Utc>,
    pub modified_at: DateTime<Utc>,

    /// Meta-cognitive annotation (thinking trace, assumptions, etc.).
    /// `None` for ordinary nodes; populated for `reflection`, `hypothesis`,
    /// `insight`, etc., or any node augmented via `add_meta_cognition`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meta_cognition: Option<MetaCognition>,

    /// Lean 4 verification metadata. Populated when this node represents a
    /// theorem or has been associated with a formal proof.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lean: Option<LeanVerification>,
}

impl KnowledgeNode {
    /// Construct a new node with a fresh ULID + envelope.
    pub fn new(
        node_type: NodeType,
        content: NodeContent,
        owner: impl Into<String>,
    ) -> Self {
        let now = Utc::now();
        Self {
            node_id: Ulid::new().to_string(),
            node_type,
            content,
            properties: serde_json::Map::new(),
            metadata: MetadataEnvelope::new(owner, "1.0.0"),
            status: NodeStatus::Active,
            visibility: Visibility::Private,
            created_at: now,
            modified_at: now,
            meta_cognition: None,
            lean: None,
        }
    }

    /// Compute a deterministic content hash for sync / dedup.
    pub fn content_hash(&self) -> Result<String, serde_json::Error> {
        // Hash a stable subset of fields (excluding mutable timestamps).
        let stable = serde_json::json!({
            "node_id": self.node_id,
            "node_type": self.node_type,
            "content": self.content,
            "properties": self.properties,
            "status": self.status,
            "visibility": self.visibility,
        });
        let bytes = serde_json::to_vec(&stable)?;
        Ok(dt_core::sha3_256_hex(&bytes))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_node_new() {
        let n = KnowledgeNode::new(
            NodeType::Note,
            NodeContent::new("Hello", "World"),
            "did:dt:alice",
        );
        assert_eq!(n.status, NodeStatus::Active);
        assert_eq!(n.visibility, Visibility::Private);
        assert_eq!(n.content.title, "Hello");
        assert!(!n.node_id.is_empty());
    }

    #[test]
    fn test_node_type_roundtrip() {
        for t in &[
            NodeType::Note,
            NodeType::Task,
            NodeType::Concept,
            NodeType::Custom("app.purchase".into()),
        ] {
            let s = t.as_str().to_string();
            let parsed = NodeType::parse(&s);
            assert_eq!(parsed, *t);
        }
    }

    #[test]
    fn test_content_hash_deterministic() {
        let n1 = KnowledgeNode::new(NodeType::Note, NodeContent::new("a", "b"), "u");
        let mut n2 = n1.clone();
        n2.modified_at = chrono::Utc::now() + chrono::Duration::seconds(1);
        // mutable timestamps should not affect hash
        assert_eq!(n1.content_hash().unwrap(), n2.content_hash().unwrap());
    }

    #[test]
    fn test_serde_roundtrip() {
        let n = KnowledgeNode::new(NodeType::Task, NodeContent::new("t", "b"), "u");
        let s = serde_json::to_string(&n).unwrap();
        let n2: KnowledgeNode = serde_json::from_str(&s).unwrap();
        assert_eq!(n, n2);
    }
}
