//! End-to-end integration tests for `dt-knowledge`.
//!
//! Exercises the full pipeline:
//! `KnowledgeService` → event → `EventStore` → `KnowledgeProjection` → SQLite → `KnowledgeRepository`.

use std::sync::Arc;

use dt_event::{EventStore, EventStoreConfig};
use dt_knowledge::service::NodePatch;
use dt_knowledge::{
    KnowledgeDb, KnowledgeProjection, KnowledgeRepository, KnowledgeService, NeighborDirection,
    NodeContent, NodeStatus, NodeType, Relation,
};
use tempfile::TempDir;

struct Harness {
    _tmp: TempDir,
    service: KnowledgeService,
    repo: KnowledgeRepository,
}

fn setup() -> Harness {
    let tmp = TempDir::new().unwrap();
    let cfg = EventStoreConfig {
        db_path: tmp.path().join("db.sqlite"),
        cas_path: tmp.path().join("cas"),
        log_path: tmp.path().join("events.jsonl"),
        strict_causal_deps: true,
        strict_hash_chain: true,
    };

    let mut store = EventStore::open(cfg.clone()).unwrap();

    // Open a separate connection to the same DB file for the projection + repo.
    let proj_db = Arc::new(KnowledgeDb::open(&cfg.db_path).unwrap());
    let projection = Arc::new(KnowledgeProjection::new(proj_db.clone()).unwrap());
    store.register_projection(projection);

    let store = Arc::new(store);
    let service = KnowledgeService::new(store.clone(), "node-test", "did:dt:tester");
    let repo = KnowledgeRepository::new(proj_db);

    Harness {
        _tmp: tmp,
        service,
        repo,
    }
}

#[test]
fn create_get_roundtrip() {
    let h = setup();
    let node = h
        .service
        .create(NodeType::Note, NodeContent::new("Hello", "World body"))
        .unwrap();

    let fetched = h.repo.get(&node.node_id).unwrap().unwrap();
    assert_eq!(fetched.node_id, node.node_id);
    assert_eq!(fetched.content.title, "Hello");
    assert_eq!(fetched.content.body, "World body");
    assert_eq!(fetched.status, NodeStatus::Active);
}

#[test]
fn update_partial_patches() {
    let h = setup();
    let node = h
        .service
        .create(NodeType::Note, NodeContent::new("Original", "Body"))
        .unwrap();

    h.service
        .update(
            &node.node_id,
            NodePatch {
                title: Some("Updated".into()),
                ..Default::default()
            },
        )
        .unwrap();

    let fetched = h.repo.get(&node.node_id).unwrap().unwrap();
    assert_eq!(fetched.content.title, "Updated");
    assert_eq!(fetched.content.body, "Body"); // body untouched
}

#[test]
fn delete_makes_invisible_to_get() {
    let h = setup();
    let node = h
        .service
        .create(NodeType::Task, NodeContent::new("Task", "Do it"))
        .unwrap();
    h.service.delete(&node.node_id).unwrap();
    assert!(h.repo.get(&node.node_id).unwrap().is_none());
    // Tombstone still present
    let with_tomb = h.repo.get_including_deleted(&node.node_id).unwrap().unwrap();
    assert_eq!(with_tomb.status, NodeStatus::Deleted);
}

#[test]
fn fts_search_finds_matches() {
    let h = setup();
    h.service
        .create(NodeType::Note, NodeContent::new("rust async", "tokio runtime"))
        .unwrap();
    h.service
        .create(NodeType::Note, NodeContent::new("python typing", "mypy"))
        .unwrap();
    h.service
        .create(
            NodeType::Note,
            NodeContent::new("about rust", "ownership rules"),
        )
        .unwrap();

    let results = h.repo.search("rust", 10).unwrap();
    assert_eq!(results.len(), 2);

    let none = h.repo.search("kotlin", 10).unwrap();
    assert!(none.is_empty());
}

#[test]
fn link_unlink_cycles() {
    let h = setup();
    let a = h
        .service
        .create(NodeType::Concept, NodeContent::new("A", ""))
        .unwrap();
    let b = h
        .service
        .create(NodeType::Concept, NodeContent::new("B", ""))
        .unwrap();
    let edge = h
        .service
        .link(&a.node_id, &b.node_id, Relation::References, Some(0.7))
        .unwrap();

    let outgoing = h
        .repo
        .neighbors(&a.node_id, NeighborDirection::Outgoing, None, 10)
        .unwrap();
    assert_eq!(outgoing.len(), 1);
    assert_eq!(outgoing[0].edge_id, edge.edge_id);
    assert_eq!(outgoing[0].relation, Relation::References);

    let incoming = h
        .repo
        .neighbors(&b.node_id, NeighborDirection::Incoming, None, 10)
        .unwrap();
    assert_eq!(incoming.len(), 1);

    h.service.unlink(&edge.edge_id).unwrap();
    let after = h
        .repo
        .neighbors(&a.node_id, NeighborDirection::Outgoing, None, 10)
        .unwrap();
    assert!(after.is_empty());
}

#[test]
fn rejects_self_loop() {
    let h = setup();
    let n = h
        .service
        .create(NodeType::Note, NodeContent::new("x", ""))
        .unwrap();
    let res = h.service.link(&n.node_id, &n.node_id, Relation::RelatedTo, None);
    assert!(res.is_err());
}

#[test]
fn walk_traverses_graph() {
    let h = setup();
    let a = h
        .service
        .create(NodeType::Concept, NodeContent::new("A", ""))
        .unwrap();
    let b = h
        .service
        .create(NodeType::Concept, NodeContent::new("B", ""))
        .unwrap();
    let c = h
        .service
        .create(NodeType::Concept, NodeContent::new("C", ""))
        .unwrap();
    let d = h
        .service
        .create(NodeType::Concept, NodeContent::new("D", ""))
        .unwrap();

    h.service
        .link(&a.node_id, &b.node_id, Relation::RelatedTo, None)
        .unwrap();
    h.service
        .link(&b.node_id, &c.node_id, Relation::RelatedTo, None)
        .unwrap();
    h.service
        .link(&c.node_id, &d.node_id, Relation::RelatedTo, None)
        .unwrap();

    let depth1 = h
        .repo
        .walk(&a.node_id, 1, NeighborDirection::Outgoing)
        .unwrap();
    assert_eq!(depth1.len(), 2); // A + B

    let depth2 = h
        .repo
        .walk(&a.node_id, 2, NeighborDirection::Outgoing)
        .unwrap();
    assert_eq!(depth2.len(), 3); // A + B + C

    let depth_full = h
        .repo
        .walk(&a.node_id, 5, NeighborDirection::Outgoing)
        .unwrap();
    assert_eq!(depth_full.len(), 4);
}

#[test]
fn count_and_list_filter_by_type() {
    let h = setup();
    h.service
        .create(NodeType::Note, NodeContent::new("n1", ""))
        .unwrap();
    h.service
        .create(NodeType::Note, NodeContent::new("n2", ""))
        .unwrap();
    h.service
        .create(NodeType::Task, NodeContent::new("t1", ""))
        .unwrap();

    assert_eq!(h.repo.count().unwrap(), 3);
    let notes = h.repo.list(Some(&NodeType::Note), 100).unwrap();
    assert_eq!(notes.len(), 2);
    let tasks = h.repo.list(Some(&NodeType::Task), 100).unwrap();
    assert_eq!(tasks.len(), 1);
}
