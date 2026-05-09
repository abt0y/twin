//! End-to-end integration test: open a store, build a hash chain of events,
//! tamper with the on-disk DB, and verify detection.

use std::sync::Arc;

use dt_event::{
    EventBuilder, EventStore, EventStoreConfig, EventType, InMemoryProjection, Projection,
};
use serde_json::json;
use tempfile::TempDir;

fn make_cfg(dir: &std::path::Path) -> EventStoreConfig {
    EventStoreConfig {
        db_path: dir.join("db.sqlite"),
        cas_path: dir.join("cas"),
        log_path: dir.join("events.jsonl"),
        strict_causal_deps: true,
        strict_hash_chain: true,
    }
}

#[test]
fn end_to_end_hash_chain_with_projection() {
    let dir = TempDir::new().unwrap();
    let mut store = EventStore::open(make_cfg(dir.path())).unwrap();

    let proj = Arc::new(InMemoryProjection::new("e2e-counter"));
    store.register_projection(proj.clone());

    // Build a 10-event hash chain.
    let mut prev: Option<String> = None;
    let mut event_ids: Vec<String> = Vec::new();

    for i in 0..10 {
        let mut b = EventBuilder::new(EventType::KnowledgeCreate, "node-test", "did:dt:tester")
            .user("did:dt:tester")
            .payload(json!({"index": i, "msg": format!("event-{}", i)}));
        if let Some(p) = prev.clone() {
            b = b.prev_event(p);
        }
        let ev = b.build().unwrap();
        store.append(&ev).unwrap();
        prev = Some(ev.event_id.clone());
        event_ids.push(ev.event_id);
    }

    assert_eq!(store.count().unwrap(), 10);
    assert_eq!(proj.count_for("knowledge.create"), 10);
    assert_eq!(proj.total_events(), 10);

    // Verify the entire log
    assert_eq!(store.verify_all().unwrap(), 10);

    // List ULIDs are in chronological order
    let listed = store.list_ids(100).unwrap();
    assert_eq!(listed.len(), 10);
    assert_eq!(listed.first().unwrap(), event_ids.first().unwrap());

    // JSONL log contains 10 entries
    let log = std::fs::read_to_string(store.log_path()).unwrap();
    assert!(log.lines().count() >= 10);
}

#[test]
fn rejects_replayed_events() {
    let dir = TempDir::new().unwrap();
    let store = EventStore::open(make_cfg(dir.path())).unwrap();

    let ev = EventBuilder::new(EventType::AgentObservation, "n1", "did:u")
        .payload(json!({"obs": "first"}))
        .build()
        .unwrap();

    store.append(&ev).unwrap();
    let res = store.append(&ev);
    assert!(matches!(res, Err(dt_event::EventError::DuplicateEvent(_))));
}

#[test]
fn detects_db_tampering() {
    let dir = TempDir::new().unwrap();
    let cfg = make_cfg(dir.path());

    let ev_id;
    {
        let store = EventStore::open(cfg.clone()).unwrap();
        let ev = EventBuilder::new(EventType::KnowledgeCreate, "n1", "did:u")
            .payload(json!({"original": true}))
            .build()
            .unwrap();
        ev_id = ev.event_id.clone();
        store.append(&ev).unwrap();
    }

    // Open the SQLite DB directly and tamper with the payload (bypassing the API)
    {
        let conn = rusqlite::Connection::open(&cfg.db_path).unwrap();
        conn.execute(
            "UPDATE events SET payload_json = ?1 WHERE event_id = ?2",
            rusqlite::params![r#"{"tampered":true}"#, &ev_id],
        )
        .unwrap();
    }

    // verify_all must detect mismatch
    let store = EventStore::open(cfg).unwrap();
    let res = store.verify_all();
    assert!(matches!(res, Err(dt_event::EventError::HashMismatch { .. })));
}

#[test]
fn enforces_causal_dependencies() {
    let dir = TempDir::new().unwrap();
    let store = EventStore::open(make_cfg(dir.path())).unwrap();

    // Event A
    let ev_a = EventBuilder::new(EventType::KnowledgeCreate, "n1", "did:u")
        .payload(json!({"a": 1}))
        .build()
        .unwrap();
    store.append(&ev_a).unwrap();

    // Event B depends on A — should succeed
    let ev_b = EventBuilder::new(EventType::KnowledgeUpdate, "n1", "did:u")
        .causal_dep(ev_a.event_id.clone())
        .payload(json!({"b": 2}))
        .build()
        .unwrap();
    store.append(&ev_b).unwrap();

    // Event C depends on a non-existent event — must fail
    let ev_c = EventBuilder::new(EventType::KnowledgeUpdate, "n1", "did:u")
        .causal_dep("01HQNONEXISTENT0000000000")
        .payload(json!({"c": 3}))
        .build()
        .unwrap();
    let res = store.append(&ev_c);
    assert!(matches!(
        res,
        Err(dt_event::EventError::UnsatisfiedDependency(_))
    ));
}
