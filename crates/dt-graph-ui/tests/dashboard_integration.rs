//! End-to-end dashboard tests against a real `KnowledgeService` + projection.

use std::sync::Arc;

use dt_core::cas::CasStore;
use dt_event::{EventStore, EventStoreConfig};
use dt_graph_ui::Dashboard;
use dt_knowledge::{
    CertaintyType, KnowledgeDb, KnowledgeProjection, KnowledgeRepository, KnowledgeService,
    MetaCognition, NodeContent, NodeType, StubLeanVerifier,
};
use tempfile::TempDir;

struct Harness {
    _tmp: TempDir,
    service: KnowledgeService,
    repo: KnowledgeRepository,
    cas: CasStore,
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
    let proj_db = Arc::new(KnowledgeDb::open(&cfg.db_path).unwrap());
    let projection = Arc::new(KnowledgeProjection::new(proj_db.clone()).unwrap());
    store.register_projection(projection);
    let store = Arc::new(store);
    let service = KnowledgeService::new(store, "node-test", "did:dt:tester");
    let repo = KnowledgeRepository::new(proj_db);
    let cas = CasStore::open(&cfg.cas_path).unwrap();
    Harness {
        _tmp: tmp,
        service,
        repo,
        cas,
    }
}

#[test]
fn dashboard_aggregates_meta_cognition_and_lean_state() {
    let h = setup();
    h.service
        .create(NodeType::Note, NodeContent::new("plain", ""))
        .unwrap();
    h.service
        .create_with_meta(
            NodeType::Hypothesis,
            NodeContent::new("h1", ""),
            MetaCognition::new()
                .with_certainty(CertaintyType::Heuristic)
                .with_open_question("really?"),
            Some(0.3),
        )
        .unwrap();
    h.service
        .create_with_meta(
            NodeType::Insight,
            NodeContent::new("i1", ""),
            MetaCognition::new().with_certainty(CertaintyType::Statistical),
            Some(0.9),
        )
        .unwrap();

    // Theorem + verified
    let v = StubLeanVerifier::new();
    let t = h
        .service
        .create_theorem("ok", "theorem ok : True := trivial", &h.cas)
        .unwrap();
    h.service
        .verify_with_lean(&t.node_id, "theorem ok : True := trivial", &v, &h.cas)
        .unwrap();

    // Theorem + failed
    let bad = h
        .service
        .create_theorem("bad", "theorem bad : True := sorry", &h.cas)
        .unwrap();
    h.service
        .verify_with_lean(&bad.node_id, "theorem bad : True := sorry", &v, &h.cas)
        .unwrap();

    let stats = Dashboard::new(&h.repo).compute(1000).unwrap();
    assert_eq!(stats.total_nodes, 5);
    assert_eq!(stats.lean_verified, 1);
    assert_eq!(stats.lean_failed, 1);
    assert_eq!(stats.lean_pending, 0);
    assert!(stats.total_meta_cognitive >= 4); // hypothesis + insight + 2 theorems
    assert_eq!(stats.open_questions, 1);
    // Confidence distribution: hypothesis=0.3 → bucket 1, insight=0.9 → bucket 4.
    assert_eq!(stats.confidence.buckets[1], 1);
    assert_eq!(stats.confidence.buckets[4], 1);
}
