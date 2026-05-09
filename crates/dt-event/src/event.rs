//! Event envelope — the atomic unit of state change in DT Platform.
//!
//! An `Event` is **immutable**, **content-addressed**, and **causally ordered**.
//! Once committed via `EventStore::append`, it can never be changed.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;
use ulid::Ulid;

use crate::canonical;
use crate::error::EventError;
use crate::metadata::MetadataEnvelope;
use dt_sync::vector_clock::VectorClock;

/// Strongly-typed event categories per spec §2.
///
/// Custom types are also allowed via `Custom(String)` — but core projections
/// only react to known variants.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventType {
    #[serde(rename = "knowledge.create")]
    KnowledgeCreate,
    #[serde(rename = "knowledge.update")]
    KnowledgeUpdate,
    #[serde(rename = "knowledge.delete")]
    KnowledgeDelete,
    #[serde(rename = "knowledge.link")]
    KnowledgeLink,
    #[serde(rename = "knowledge.unlink")]
    KnowledgeUnlink,
    #[serde(rename = "agent.action")]
    AgentAction,
    #[serde(rename = "agent.observation")]
    AgentObservation,
    #[serde(rename = "sync.delta")]
    SyncDelta,
    #[serde(rename = "sync.full")]
    SyncFull,
    #[serde(rename = "sync.merge")]
    SyncMerge,
    #[serde(rename = "system.config_change")]
    SystemConfigChange,
    #[serde(rename = "system.schema_update")]
    SystemSchemaUpdate,
    #[serde(rename = "user.auth")]
    UserAuth,
    #[serde(rename = "user.session")]
    UserSession,
    /// Application-defined custom type (must match `^[a-z][a-z0-9_]*\.[a-z][a-z0-9_]*$`).
    #[serde(untagged)]
    Custom(String),
}

impl fmt::Display for EventType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            EventType::KnowledgeCreate => "knowledge.create",
            EventType::KnowledgeUpdate => "knowledge.update",
            EventType::KnowledgeDelete => "knowledge.delete",
            EventType::KnowledgeLink => "knowledge.link",
            EventType::KnowledgeUnlink => "knowledge.unlink",
            EventType::AgentAction => "agent.action",
            EventType::AgentObservation => "agent.observation",
            EventType::SyncDelta => "sync.delta",
            EventType::SyncFull => "sync.full",
            EventType::SyncMerge => "sync.merge",
            EventType::SystemConfigChange => "system.config_change",
            EventType::SystemSchemaUpdate => "system.schema_update",
            EventType::UserAuth => "user.auth",
            EventType::UserSession => "user.session",
            EventType::Custom(s) => s,
        };
        f.write_str(s)
    }
}

/// The Event envelope — append-only, content-addressed, causally ordered.
///
/// `content_hash` is computed AFTER all other fields are populated.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Event {
    /// ULID — lexicographically sortable, ~time-ordered.
    pub event_id: String,

    /// Strongly-typed event category.
    pub event_type: EventType,

    /// UTC physical timestamp (does NOT supersede vector clock for ordering).
    pub timestamp: DateTime<Utc>,

    /// Source node that emitted this event.
    pub node_id: String,

    /// DID of the acting user (optional — system events may have none).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,

    /// Event-specific payload (validated against schema by registry).
    pub payload: serde_json::Value,

    /// SHA3-256 of the payload schema definition (integrity).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload_schema_hash: Option<String>,

    /// Hybrid vector clock at the moment of creation.
    pub vector_clock: VectorClock,

    /// Hash-chain link to the previous event from this node (Git-like).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prev_event_id: Option<String>,

    /// Causal dependencies — events that MUST be present before this one applies.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub causal_deps: Vec<String>,

    /// Universal Metadata Envelope.
    pub metadata: MetadataEnvelope,

    /// Ed25519 signature of canonical event bytes (set during `seal`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,

    /// SHA3-256 of canonical event JSON (set during `seal`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_hash: Option<String>,
}

impl Event {
    /// Compute the canonical content hash by EXCLUDING the `content_hash` and
    /// `signature` fields from the hashed bytes (they are populated after).
    pub fn compute_content_hash(&self) -> Result<String, EventError> {
        let mut copy = self.clone();
        copy.content_hash = None;
        copy.signature = None;
        canonical::canonical_hash(&copy)
    }

    /// Seal the event: compute and set the content hash. Idempotent.
    pub fn seal(&mut self) -> Result<&str, EventError> {
        let h = self.compute_content_hash()?;
        self.content_hash = Some(h);
        Ok(self.content_hash.as_deref().unwrap())
    }

    /// Verify the event's content hash matches its bytes.
    pub fn verify_content_hash(&self) -> Result<(), EventError> {
        let stored = self
            .content_hash
            .as_ref()
            .ok_or_else(|| EventError::Invalid("event not sealed".into()))?
            .clone();
        let actual = self.compute_content_hash()?;
        if stored != actual {
            return Err(EventError::HashMismatch {
                expected: stored,
                actual,
            });
        }
        Ok(())
    }

    /// Is this event sealed (has a content hash)?
    pub fn is_sealed(&self) -> bool {
        self.content_hash.is_some()
    }
}

/// Fluent builder for `Event`.
pub struct EventBuilder {
    event_type: EventType,
    node_id: String,
    user_id: Option<String>,
    payload: serde_json::Value,
    payload_schema_hash: Option<String>,
    vector_clock: VectorClock,
    prev_event_id: Option<String>,
    causal_deps: Vec<String>,
    metadata: MetadataEnvelope,
}

impl EventBuilder {
    /// Start a new event.
    pub fn new(
        event_type: EventType,
        node_id: impl Into<String>,
        owner_did: impl Into<String>,
    ) -> Self {
        let node_id = node_id.into();
        let owner_did = owner_did.into();
        Self {
            event_type,
            node_id: node_id.clone(),
            user_id: None,
            payload: serde_json::json!({}),
            payload_schema_hash: None,
            vector_clock: VectorClock::new(node_id),
            prev_event_id: None,
            causal_deps: Vec::new(),
            metadata: MetadataEnvelope::new(owner_did, "1.0.0"),
        }
    }

    pub fn user(mut self, did: impl Into<String>) -> Self {
        self.user_id = Some(did.into());
        self
    }

    pub fn payload(mut self, p: serde_json::Value) -> Self {
        self.payload = p;
        self
    }

    pub fn payload_schema_hash(mut self, h: impl Into<String>) -> Self {
        self.payload_schema_hash = Some(h.into());
        self
    }

    pub fn vector_clock(mut self, vc: VectorClock) -> Self {
        self.vector_clock = vc;
        self
    }

    pub fn prev_event(mut self, id: impl Into<String>) -> Self {
        self.prev_event_id = Some(id.into());
        self
    }

    pub fn causal_dep(mut self, id: impl Into<String>) -> Self {
        self.causal_deps.push(id.into());
        self
    }

    pub fn causal_deps<I, S>(mut self, ids: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.causal_deps.extend(ids.into_iter().map(Into::into));
        self
    }

    pub fn metadata(mut self, m: MetadataEnvelope) -> Self {
        self.metadata = m;
        self
    }

    /// Build and seal the event.
    pub fn build(mut self) -> Result<Event, EventError> {
        // Increment vector clock to mark this node's contribution.
        self.vector_clock.increment();

        let mut ev = Event {
            event_id: Ulid::new().to_string(),
            event_type: self.event_type,
            timestamp: Utc::now(),
            node_id: self.node_id,
            user_id: self.user_id,
            payload: self.payload,
            payload_schema_hash: self.payload_schema_hash,
            vector_clock: self.vector_clock,
            prev_event_id: self.prev_event_id,
            causal_deps: self.causal_deps,
            metadata: self.metadata,
            signature: None,
            content_hash: None,
        };
        ev.seal()?;
        Ok(ev)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample_event() -> Event {
        EventBuilder::new(
            EventType::KnowledgeCreate,
            "node-alpha",
            "did:dt:alice",
        )
        .user("did:dt:alice")
        .payload(json!({"title": "Hello", "body": "World"}))
        .build()
        .unwrap()
    }

    #[test]
    fn test_event_seal_and_verify() {
        let ev = sample_event();
        assert!(ev.is_sealed());
        ev.verify_content_hash().unwrap();
    }

    #[test]
    fn test_content_hash_deterministic() {
        // Two events with identical bytes (except event_id, timestamp, vector_clock) differ.
        // But two clones of the same event must hash the same.
        let ev = sample_event();
        let h1 = ev.compute_content_hash().unwrap();
        let h2 = ev.compute_content_hash().unwrap();
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 64);
    }

    #[test]
    fn test_tampering_breaks_verification() {
        let mut ev = sample_event();
        // Mutate payload after seal
        ev.payload = json!({"tampered": true});
        let res = ev.verify_content_hash();
        assert!(matches!(res, Err(EventError::HashMismatch { .. })));
    }

    #[test]
    fn test_event_type_serde() {
        let e = EventType::KnowledgeCreate;
        let s = serde_json::to_string(&e).unwrap();
        assert_eq!(s, r#""knowledge.create""#);
        let back: EventType = serde_json::from_str(&s).unwrap();
        assert_eq!(back, EventType::KnowledgeCreate);
    }

    #[test]
    fn test_custom_event_type() {
        let e = EventType::Custom("app.purchase".into());
        let s = serde_json::to_string(&e).unwrap();
        let back: EventType = serde_json::from_str(&s).unwrap();
        assert_eq!(back, EventType::Custom("app.purchase".into()));
    }

    #[test]
    fn test_event_builder_with_deps() {
        let ev = EventBuilder::new(
            EventType::KnowledgeUpdate,
            "node-1",
            "did:dt:user",
        )
        .causal_deps(["ev-prev-1", "ev-prev-2"])
        .prev_event("ev-prev-2")
        .build()
        .unwrap();
        assert_eq!(ev.causal_deps.len(), 2);
        assert_eq!(ev.prev_event_id.as_deref(), Some("ev-prev-2"));
    }

    #[test]
    fn test_vector_clock_incremented() {
        let ev = sample_event();
        assert_eq!(ev.vector_clock.counters.get("node-alpha"), Some(&1));
    }
}
