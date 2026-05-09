//! `EventStore` — append-only, content-addressed event log.
//!
//! Composes:
//! - **`dt-db`** SQLite events table (hot index)
//! - **`dt-core::CasStore`** (immutable canonical bytes)
//! - **`telemetry::JsonlLogger`** (structured audit trail)
//! - Pluggable **`Projection`s** (materialized views)
//!
//! ### Append-only guarantees
//! - `event_id` is unique; duplicate `append` returns `EventError::DuplicateEvent`.
//! - Stored `content_hash` is verified before commit.
//! - If `prev_event_id` is set, that event MUST exist (hash-chain integrity).
//! - Causal deps MUST exist before append succeeds.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use rusqlite::params;
use tracing::{debug, info, warn};

use dt_core::cas::CasStore;
use dt_db::connection::DbConnection;
use dt_db::events::{append_event as db_append, get_event_by_id as db_get, EventRecord};
use dt_db::schema::EVENTS_SQL;

use crate::canonical::to_canonical_bytes;
use crate::error::EventError;
use crate::event::Event;
use crate::projection::Projection;
use crate::telemetry::JsonlLogger;

/// Configuration for opening an `EventStore`.
#[derive(Debug, Clone)]
pub struct EventStoreConfig {
    /// SQLite database file path.
    pub db_path: PathBuf,
    /// CAS root directory.
    pub cas_path: PathBuf,
    /// JSONL log file path.
    pub log_path: PathBuf,
    /// If true, verify causal deps exist on append.
    pub strict_causal_deps: bool,
    /// If true, verify `prev_event_id` exists on append.
    pub strict_hash_chain: bool,
}

impl EventStoreConfig {
    /// Default config rooted at `~/.dt/`.
    pub fn from_dt_dir() -> Self {
        let root = dt_core::resolve_dt_dir();
        Self {
            db_path: root.join("db.sqlite"),
            cas_path: root.join("cas"),
            log_path: root.join("logs").join("events.jsonl"),
            strict_causal_deps: true,
            strict_hash_chain: true,
        }
    }
}

/// The append-only event store.
pub struct EventStore {
    db: DbConnection,
    cas: CasStore,
    logger: JsonlLogger,
    projections: Vec<Arc<dyn Projection>>,
    strict_causal_deps: bool,
    strict_hash_chain: bool,
}

impl EventStore {
    /// Open or create the event store.
    pub fn open(config: EventStoreConfig) -> Result<Self, EventError> {
        if let Some(p) = config.db_path.parent() {
            std::fs::create_dir_all(p)?;
        }
        let db = DbConnection::open(&config.db_path)?;
        db.execute_batch(EVENTS_SQL)?;

        let cas = CasStore::open(&config.cas_path)?;
        let logger = JsonlLogger::open(&config.log_path)?;

        info!(
            db = %config.db_path.display(),
            cas = %config.cas_path.display(),
            log = %config.log_path.display(),
            "EventStore opened"
        );

        Ok(Self {
            db,
            cas,
            logger,
            projections: Vec::new(),
            strict_causal_deps: config.strict_causal_deps,
            strict_hash_chain: config.strict_hash_chain,
        })
    }

    /// Open an in-memory store (tests / ephemeral nodes).
    pub fn open_in_memory<P: AsRef<Path>>(cas_path: P, log_path: P) -> Result<Self, EventError> {
        let db = DbConnection::open_in_memory()?;
        db.execute_batch(EVENTS_SQL)?;
        let cas = CasStore::open(cas_path.as_ref())?;
        let logger = JsonlLogger::open(log_path.as_ref())?;
        Ok(Self {
            db,
            cas,
            logger,
            projections: Vec::new(),
            strict_causal_deps: true,
            strict_hash_chain: true,
        })
    }

    /// Register a projection — invoked synchronously after each successful append.
    pub fn register_projection(&mut self, p: Arc<dyn Projection>) {
        info!(projection = p.name(), "projection registered");
        self.projections.push(p);
    }

    /// Append a sealed event. The single critical write path of the system.
    ///
    /// Returns the canonical content hash on success.
    pub fn append(&self, event: &Event) -> Result<String, EventError> {
        // 1. Validate seal + hash
        let content_hash = event
            .content_hash
            .clone()
            .ok_or_else(|| EventError::Invalid("event must be sealed before append".into()))?;
        event.verify_content_hash()?;

        // 2. Reject duplicates
        if self.exists(&event.event_id)? {
            return Err(EventError::DuplicateEvent(event.event_id.clone()));
        }

        // 3. Hash-chain check
        if self.strict_hash_chain {
            if let Some(prev) = &event.prev_event_id {
                if !self.exists(prev)? {
                    return Err(EventError::HashChainBroken {
                        prev: Some(prev.clone()),
                    });
                }
            }
        }

        // 4. Causal dependency check
        if self.strict_causal_deps {
            for dep in &event.causal_deps {
                if !self.exists(dep)? {
                    return Err(EventError::UnsatisfiedDependency(dep.clone()));
                }
            }
        }

        // 5. Persist payload to CAS (deduplicated by content)
        let payload_bytes = to_canonical_bytes(&event.payload)?;
        let payload_cas_hash = self
            .cas
            .put(&payload_bytes)
            .map_err(|e| EventError::Storage(format!("cas payload put: {}", e)))?;
        debug!(
            event_id = %event.event_id,
            payload_hash = %payload_cas_hash,
            "payload persisted to CAS"
        );

        // 6. Persist canonical event bytes to CAS
        let event_bytes = to_canonical_bytes(event)?;
        let event_cas_hash = self
            .cas
            .put(&event_bytes)
            .map_err(|e| EventError::Storage(format!("cas event put: {}", e)))?;
        debug!(
            event_id = %event.event_id,
            event_hash = %event_cas_hash,
            "event persisted to CAS"
        );

        // 7. Insert SQLite hot index row
        let record = self.event_to_record(event)?;
        db_append(self.db.inner(), &record)?;

        // 8. Audit log (JSONL)
        self.logger.log_event("info", "appended", event)?;

        // 9. Fire projections
        for proj in &self.projections {
            if let Err(e) = proj.apply(event) {
                // Projections are best-effort — log but don't fail the append.
                warn!(
                    projection = proj.name(),
                    event_id = %event.event_id,
                    error = %e,
                    "projection apply failed"
                );
                let _ = self.logger.log(
                    "warn",
                    "projection_apply_failed",
                    Some(serde_json::json!({
                        "projection": proj.name(),
                        "event_id": event.event_id,
                        "error": e.to_string(),
                    })),
                );
            }
        }

        info!(
            event_id = %event.event_id,
            event_type = %event.event_type,
            content_hash = %content_hash,
            "event appended"
        );

        Ok(content_hash)
    }

    /// Retrieve an event by its id.
    pub fn get(&self, event_id: &str) -> Result<Option<Event>, EventError> {
        let rec = match db_get(self.db.inner(), event_id)? {
            Some(r) => r,
            None => return Ok(None),
        };
        let ev = self.record_to_event(&rec)?;
        Ok(Some(ev))
    }

    /// Check existence by id (cheap, no decoding).
    pub fn exists(&self, event_id: &str) -> Result<bool, EventError> {
        let count: i64 = self.db.inner().query_row(
            "SELECT count(*) FROM events WHERE event_id = ?1",
            params![event_id],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    /// Total number of committed events.
    pub fn count(&self) -> Result<u64, EventError> {
        let n: i64 = self
            .db
            .inner()
            .query_row("SELECT count(*) FROM events", [], |row| row.get(0))?;
        Ok(n as u64)
    }

    /// Verify the entire log: walks every event, recomputes its content hash,
    /// and confirms hash-chain links resolve. Returns the count of verified events.
    pub fn verify_all(&self) -> Result<u64, EventError> {
        let mut stmt = self
            .db
            .inner()
            .prepare("SELECT event_id FROM events ORDER BY event_id")?;
        let ids: Vec<String> = stmt
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        let mut verified: u64 = 0;
        for id in ids {
            let ev = self
                .get(&id)?
                .ok_or_else(|| EventError::Invalid(format!("missing event during verify: {}", id)))?;
            ev.verify_content_hash()?;
            if let Some(prev) = &ev.prev_event_id {
                if !self.exists(prev)? {
                    return Err(EventError::HashChainBroken {
                        prev: Some(prev.clone()),
                    });
                }
            }
            verified += 1;
        }
        info!(verified, "log verification complete");
        Ok(verified)
    }

    /// List event IDs in chronological order (ULID natural order). Useful for sync.
    pub fn list_ids(&self, limit: usize) -> Result<Vec<String>, EventError> {
        let mut stmt = self
            .db
            .inner()
            .prepare("SELECT event_id FROM events ORDER BY event_id LIMIT ?1")?;
        let ids: Vec<String> = stmt
            .query_map(params![limit as i64], |r| r.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(ids)
    }

    /// Path to the JSONL audit log.
    pub fn log_path(&self) -> &Path {
        self.logger.path()
    }

    // ---- internals ----------------------------------------------------------

    fn event_to_record(&self, ev: &Event) -> Result<EventRecord, EventError> {
        Ok(EventRecord {
            event_id: ev.event_id.clone(),
            event_type: ev.event_type.to_string(),
            timestamp: ev.timestamp.to_rfc3339(),
            node_id: ev.node_id.clone(),
            user_id: ev.user_id.clone(),
            payload_json: serde_json::to_string(&ev.payload)?,
            payload_schema_hash: ev.payload_schema_hash.clone(),
            vector_clock_json: serde_json::to_string(&ev.vector_clock)?,
            prev_event_id: ev.prev_event_id.clone(),
            causal_deps_json: if ev.causal_deps.is_empty() {
                None
            } else {
                Some(serde_json::to_string(&ev.causal_deps)?)
            },
            metadata_json: serde_json::to_string(&ev.metadata)?,
            signature: ev.signature.clone(),
            content_hash: ev
                .content_hash
                .clone()
                .unwrap_or_else(|| "0".repeat(64)),
        })
    }

    fn record_to_event(&self, r: &EventRecord) -> Result<Event, EventError> {
        let payload: serde_json::Value = serde_json::from_str(&r.payload_json)?;
        let vector_clock: dt_sync::vector_clock::VectorClock =
            serde_json::from_str(&r.vector_clock_json)?;
        let causal_deps: Vec<String> = match &r.causal_deps_json {
            Some(s) if !s.is_empty() => serde_json::from_str(s)?,
            _ => Vec::new(),
        };
        let metadata: crate::metadata::MetadataEnvelope = serde_json::from_str(&r.metadata_json)?;
        let event_type: crate::event::EventType =
            serde_json::from_value(serde_json::Value::String(r.event_type.clone()))?;
        let timestamp = chrono::DateTime::parse_from_rfc3339(&r.timestamp)
            .map_err(|e| EventError::Invalid(format!("bad timestamp: {}", e)))?
            .with_timezone(&chrono::Utc);

        Ok(Event {
            event_id: r.event_id.clone(),
            event_type,
            timestamp,
            node_id: r.node_id.clone(),
            user_id: r.user_id.clone().filter(|s| !s.is_empty()),
            payload,
            payload_schema_hash: r.payload_schema_hash.clone().filter(|s| !s.is_empty()),
            vector_clock,
            prev_event_id: r.prev_event_id.clone().filter(|s| !s.is_empty()),
            causal_deps,
            metadata,
            signature: r.signature.clone().filter(|s| !s.is_empty()),
            content_hash: Some(r.content_hash.clone()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{EventBuilder, EventType};
    use crate::projection::InMemoryProjection;
    use serde_json::json;
    use tempfile::TempDir;

    fn store_in_dir(dir: &Path) -> EventStore {
        let cfg = EventStoreConfig {
            db_path: dir.join("db.sqlite"),
            cas_path: dir.join("cas"),
            log_path: dir.join("events.jsonl"),
            strict_causal_deps: true,
            strict_hash_chain: true,
        };
        EventStore::open(cfg).unwrap()
    }

    fn make_event(node: &str, prev: Option<String>) -> Event {
        let mut b = EventBuilder::new(EventType::KnowledgeCreate, node, "did:dt:u")
            .payload(json!({"k": "v"}));
        if let Some(p) = prev {
            b = b.prev_event(p);
        }
        b.build().unwrap()
    }

    #[test]
    fn test_append_and_get() {
        let dir = TempDir::new().unwrap();
        let store = store_in_dir(dir.path());

        let ev = make_event("n1", None);
        let h = store.append(&ev).unwrap();
        assert_eq!(h.len(), 64);

        let fetched = store.get(&ev.event_id).unwrap().unwrap();
        assert_eq!(fetched.event_id, ev.event_id);
        assert_eq!(fetched.payload, ev.payload);
        fetched.verify_content_hash().unwrap();
    }

    #[test]
    fn test_duplicate_rejected() {
        let dir = TempDir::new().unwrap();
        let store = store_in_dir(dir.path());
        let ev = make_event("n1", None);
        store.append(&ev).unwrap();
        let res = store.append(&ev);
        assert!(matches!(res, Err(EventError::DuplicateEvent(_))));
    }

    #[test]
    fn test_hash_chain_broken_rejected() {
        let dir = TempDir::new().unwrap();
        let store = store_in_dir(dir.path());
        let ev = make_event("n1", Some("01HQNONEXISTENT0000000000".to_string()));
        let res = store.append(&ev);
        assert!(matches!(res, Err(EventError::HashChainBroken { .. })));
    }

    #[test]
    fn test_hash_chain_valid_chain() {
        let dir = TempDir::new().unwrap();
        let store = store_in_dir(dir.path());
        let ev1 = make_event("n1", None);
        store.append(&ev1).unwrap();
        let ev2 = make_event("n1", Some(ev1.event_id.clone()));
        store.append(&ev2).unwrap();
        assert_eq!(store.count().unwrap(), 2);
    }

    #[test]
    fn test_causal_deps_missing_rejected() {
        let dir = TempDir::new().unwrap();
        let store = store_in_dir(dir.path());
        let ev = EventBuilder::new(EventType::KnowledgeUpdate, "n1", "did:u")
            .causal_dep("01HQNONEXISTENT0000000000")
            .build()
            .unwrap();
        let res = store.append(&ev);
        assert!(matches!(res, Err(EventError::UnsatisfiedDependency(_))));
    }

    #[test]
    fn test_tampered_event_rejected() {
        let dir = TempDir::new().unwrap();
        let store = store_in_dir(dir.path());
        let mut ev = make_event("n1", None);
        // Mutate payload after seal
        ev.payload = json!({"tampered": true});
        let res = store.append(&ev);
        assert!(matches!(res, Err(EventError::HashMismatch { .. })));
    }

    #[test]
    fn test_unsealed_event_rejected() {
        let dir = TempDir::new().unwrap();
        let store = store_in_dir(dir.path());
        let mut ev = make_event("n1", None);
        ev.content_hash = None;
        let res = store.append(&ev);
        assert!(matches!(res, Err(EventError::Invalid(_))));
    }

    #[test]
    fn test_projection_invoked() {
        let dir = TempDir::new().unwrap();
        let mut store = store_in_dir(dir.path());
        let proj = Arc::new(InMemoryProjection::new("counter"));
        store.register_projection(proj.clone());

        for _ in 0..3 {
            let ev = EventBuilder::new(EventType::KnowledgeCreate, "n1", "did:u")
                .build()
                .unwrap();
            store.append(&ev).unwrap();
        }
        assert_eq!(proj.count_for("knowledge.create"), 3);
    }

    #[test]
    fn test_verify_all() {
        let dir = TempDir::new().unwrap();
        let store = store_in_dir(dir.path());
        let mut prev: Option<String> = None;
        for _ in 0..5 {
            let ev = make_event("n1", prev.clone());
            store.append(&ev).unwrap();
            prev = Some(ev.event_id);
        }
        assert_eq!(store.verify_all().unwrap(), 5);
    }

    #[test]
    fn test_jsonl_log_written() {
        let dir = TempDir::new().unwrap();
        let store = store_in_dir(dir.path());
        let ev = make_event("n1", None);
        store.append(&ev).unwrap();

        let log = std::fs::read_to_string(store.log_path()).unwrap();
        assert!(log.lines().count() >= 1);
        let line: serde_json::Value = serde_json::from_str(log.lines().next().unwrap()).unwrap();
        assert_eq!(line["event_type"], "knowledge.create");
    }

    #[test]
    fn test_cas_dedup_for_identical_payloads() {
        let dir = TempDir::new().unwrap();
        let store = store_in_dir(dir.path());
        // Two events with identical payloads should reuse the CAS slot.
        let ev1 = EventBuilder::new(EventType::KnowledgeCreate, "n1", "did:u")
            .payload(json!({"same": "payload"}))
            .build()
            .unwrap();
        let ev2 = EventBuilder::new(EventType::KnowledgeCreate, "n1", "did:u")
            .payload(json!({"same": "payload"}))
            .prev_event(ev1.event_id.clone())
            .build()
            .unwrap();
        store.append(&ev1).unwrap();
        store.append(&ev2).unwrap();
        // Both events hash differently, but the payload CAS file is shared.
        let payload_hash = dt_event_canonical_hash_payload(&ev1.payload);
        assert!(store.cas.contains(&payload_hash));
    }

    fn dt_event_canonical_hash_payload(p: &serde_json::Value) -> String {
        let bytes = crate::canonical::to_canonical_bytes(p).unwrap();
        dt_core::sha3_256_hex(&bytes)
    }

    #[test]
    fn test_persistence_across_reopen() {
        let dir = TempDir::new().unwrap();
        let cfg = EventStoreConfig {
            db_path: dir.path().join("db.sqlite"),
            cas_path: dir.path().join("cas"),
            log_path: dir.path().join("events.jsonl"),
            strict_causal_deps: true,
            strict_hash_chain: true,
        };
        let ev_id;
        {
            let store = EventStore::open(cfg.clone()).unwrap();
            let ev = make_event("n1", None);
            store.append(&ev).unwrap();
            ev_id = ev.event_id;
        }
        // Reopen
        let store = EventStore::open(cfg).unwrap();
        assert!(store.exists(&ev_id).unwrap());
        let fetched = store.get(&ev_id).unwrap().unwrap();
        fetched.verify_content_hash().unwrap();
    }
}
