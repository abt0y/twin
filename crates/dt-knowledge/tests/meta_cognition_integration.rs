//! End-to-end tests for the meta-cognition + Lean verification + reasoning
//! layers. Exercises the full pipeline:
//!
//! `KnowledgeService` → event → `EventStore` → `KnowledgeProjection` →
//! SQLite → `KnowledgeRepository` → `ReasoningEngine`.

use std::sync::Arc;

use dt_core::cas::CasStore;
use dt_event::{EventStore, EventStoreConfig};
use dt_knowledge::export::GraphScene;
use dt_knowledge::reasoning::ReasoningEngine;
use dt_knowledge::service::NodePatch;
use dt_knowledge::{
    CertaintyType, KnowledgeDb, KnowledgeProjection, KnowledgeRepository, KnowledgeService,
    LeanProofStatus, MetaCognition, NeighborDirection, NodeContent, NodeType, Relation,
    StubLeanVerifier, ThinkingStep,
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
fn create_with_meta_persists_meta_cognition() {
    let h = setup();
    let mc = MetaCognition::new()
        .with_certainty(CertaintyType::Heuristic)
        .with_assumption("users hate latency")
        .with_thinking_step(ThinkingStep::now("observed slow tail"))
        .with_open_question("is it network or compute?")
        .with_derivation_depth(1);

    let n = h
        .service
        .create_with_meta(
            NodeType::Hypothesis,
            NodeContent::new("Latency hurts retention", ""),
            mc.clone(),
            Some(0.62),
        )
        .unwrap();

    let fetched = h.repo.get(&n.node_id).unwrap().unwrap();
    let stored = fetched.meta_cognition.expect("meta_cognition persisted");
    assert_eq!(stored.certainty_type, CertaintyType::Heuristic);
    assert_eq!(stored.assumptions, vec!["users hate latency".to_string()]);
    assert_eq!(stored.thinking_trace.len(), 1);
    assert_eq!(stored.open_questions.len(), 1);
    assert_eq!(stored.derivation_depth, 1);
    assert_eq!(fetched.metadata.dt_confidence, Some(0.62));
}

#[test]
fn set_meta_cognition_updates_existing_node() {
    let h = setup();
    let n = h
        .service
        .create(NodeType::Note, NodeContent::new("plain note", ""))
        .unwrap();
    assert!(h.repo.get(&n.node_id).unwrap().unwrap().meta_cognition.is_none());

    let mc = MetaCognition::new()
        .with_certainty(CertaintyType::Statistical)
        .with_assumption("sampling i.i.d.");
    h.service
        .set_meta_cognition(&n.node_id, mc.clone(), Some(0.85))
        .unwrap();

    let updated = h.repo.get(&n.node_id).unwrap().unwrap();
    assert_eq!(
        updated.meta_cognition.unwrap().certainty_type,
        CertaintyType::Statistical
    );
}

#[test]
fn update_patch_can_change_meta_cognition_and_confidence() {
    let h = setup();
    let n = h
        .service
        .create_with_meta(
            NodeType::Insight,
            NodeContent::new("insight A", ""),
            MetaCognition::new().with_certainty(CertaintyType::Heuristic),
            Some(0.5),
        )
        .unwrap();

    let patch = NodePatch {
        meta_cognition: Some(
            MetaCognition::new()
                .with_certainty(CertaintyType::Proof)
                .with_assumption("ZFC"),
        ),
        confidence: Some(0.99),
        ..Default::default()
    };
    h.service.update(&n.node_id, patch).unwrap();

    let n2 = h.repo.get(&n.node_id).unwrap().unwrap();
    assert_eq!(
        n2.meta_cognition.unwrap().certainty_type,
        CertaintyType::Proof
    );
    // The confidence column is updated; the metadata envelope on the node is
    // not refreshed by the patch path (envelope is event-scoped).
}

#[test]
fn lean_create_theorem_stores_source_in_cas() {
    let h = setup();
    let src = "theorem add_comm : ∀ a b : Nat, a + b = b + a := by intros; ring";
    let node = h.service.create_theorem("add_comm", src, &h.cas).unwrap();
    assert_eq!(node.node_type, NodeType::Theorem);
    let lean = node.lean.expect("theorem must have lean block");
    assert_eq!(lean.lean_proof_status, LeanProofStatus::Pending);
    let hash = lean.lean_theorem_hash.expect("theorem hash present");
    assert!(h.cas.contains(&hash));
    let bytes = h.cas.get(&hash).unwrap();
    assert_eq!(String::from_utf8(bytes).unwrap(), src);
}

#[test]
fn lean_verify_emits_verified_event_and_persists_status() {
    let h = setup();
    let src = "theorem t : 1 + 1 = 2 := by rfl";
    let node = h.service.create_theorem("t", src, &h.cas).unwrap();

    let verifier = StubLeanVerifier::new();
    let lean = h
        .service
        .verify_with_lean(&node.node_id, src, &verifier, &h.cas)
        .unwrap();
    assert!(lean.verified_by_lean);
    assert_eq!(lean.lean_proof_status, LeanProofStatus::Verified);
    assert!(lean.lean_proof_hash.is_some());

    let fetched = h.repo.get(&node.node_id).unwrap().unwrap();
    let lean2 = fetched.lean.unwrap();
    assert!(lean2.verified_by_lean);
    assert_eq!(lean2.lean_proof_status, LeanProofStatus::Verified);
    assert_eq!(lean2.verifier_version.as_deref(), Some("stub-0.1.0"));

    // The proof artifact lives in CAS too.
    let phash = lean2.lean_proof_hash.unwrap();
    assert!(h.cas.contains(&phash));
}

#[test]
fn lean_verify_emits_failed_event_for_sorry_proof() {
    let h = setup();
    let src = "theorem hard : True := sorry";
    let node = h.service.create_theorem("hard", src, &h.cas).unwrap();

    let verifier = StubLeanVerifier::new();
    let lean = h
        .service
        .verify_with_lean(&node.node_id, src, &verifier, &h.cas)
        .unwrap();
    assert!(!lean.verified_by_lean);
    assert_eq!(lean.lean_proof_status, LeanProofStatus::Failed);
    assert!(lean.last_error.unwrap().contains("sorry"));

    let fetched = h.repo.get(&node.node_id).unwrap().unwrap();
    let lean2 = fetched.lean.unwrap();
    assert_eq!(lean2.lean_proof_status, LeanProofStatus::Failed);

    // Repo helper finds it.
    let failed = h.repo.list_by_lean_status("failed", 10).unwrap();
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0].node_id, node.node_id);
}

#[test]
fn list_by_lean_status_separates_verified_and_failed() {
    let h = setup();
    let v = StubLeanVerifier::new();
    let ok = h
        .service
        .create_theorem("ok", "theorem ok : True := trivial", &h.cas)
        .unwrap();
    h.service
        .verify_with_lean(&ok.node_id, "theorem ok : True := trivial", &v, &h.cas)
        .unwrap();

    let bad = h
        .service
        .create_theorem("bad", "theorem bad : True := sorry", &h.cas)
        .unwrap();
    h.service
        .verify_with_lean(&bad.node_id, "theorem bad : True := sorry", &v, &h.cas)
        .unwrap();

    let verified = h.repo.list_by_lean_status("verified", 10).unwrap();
    let failed = h.repo.list_by_lean_status("failed", 10).unwrap();
    assert_eq!(verified.len(), 1);
    assert_eq!(failed.len(), 1);
    assert_eq!(verified[0].node_id, ok.node_id);
    assert_eq!(failed[0].node_id, bad.node_id);
}

#[test]
fn list_low_confidence_surfaces_weak_claims() {
    let h = setup();
    h.service
        .create_with_meta(
            NodeType::Hypothesis,
            NodeContent::new("weak", ""),
            MetaCognition::new(),
            Some(0.2),
        )
        .unwrap();
    h.service
        .create_with_meta(
            NodeType::Hypothesis,
            NodeContent::new("strong", ""),
            MetaCognition::new(),
            Some(0.95),
        )
        .unwrap();

    let weak = h.repo.list_low_confidence(0.5, 10).unwrap();
    assert_eq!(weak.len(), 1);
    assert_eq!(weak[0].content.title, "weak");
}

#[test]
fn list_with_open_questions_finds_meta_cognition_questions() {
    let h = setup();
    h.service
        .create_with_meta(
            NodeType::Insight,
            NodeContent::new("with q", ""),
            MetaCognition::new().with_open_question("does this generalize?"),
            None,
        )
        .unwrap();
    h.service
        .create_with_meta(
            NodeType::Insight,
            NodeContent::new("plain", ""),
            MetaCognition::new(),
            None,
        )
        .unwrap();

    let with_q = h.repo.list_with_open_questions(10).unwrap();
    assert_eq!(with_q.len(), 1);
    assert_eq!(with_q[0].content.title, "with q");
}

// ── reasoning engine ────────────────────────────────────────────────────────

#[test]
fn reason_path_finds_chain() {
    let h = setup();
    let a = h
        .service
        .create(NodeType::Evidence, NodeContent::new("E", "raw data"))
        .unwrap();
    let b = h
        .service
        .create(NodeType::Hypothesis, NodeContent::new("H", "claim"))
        .unwrap();
    let c = h
        .service
        .create(NodeType::Insight, NodeContent::new("I", "synthesis"))
        .unwrap();

    h.service
        .link(&a.node_id, &b.node_id, Relation::Custom("supports".into()), None)
        .unwrap();
    h.service
        .link(&b.node_id, &c.node_id, Relation::Custom("derives".into()), None)
        .unwrap();

    let engine = ReasoningEngine::new(&h.repo);
    let paths = engine
        .reason_path(&a.node_id, &c.node_id, 5, NeighborDirection::Outgoing)
        .unwrap();
    assert!(!paths.is_empty(), "expected at least one path");
    let chain = &paths[0];
    assert_eq!(chain.depth, 2);
    assert_eq!(chain.nodes.len(), 3);
    assert_eq!(chain.nodes[0].node_id, a.node_id);
    assert_eq!(chain.nodes[2].node_id, c.node_id);
}

#[test]
fn find_evidence_chains_returns_evidence_to_target() {
    let h = setup();
    let e1 = h
        .service
        .create(NodeType::Evidence, NodeContent::new("E1", ""))
        .unwrap();
    let e2 = h
        .service
        .create(NodeType::Evidence, NodeContent::new("E2", ""))
        .unwrap();
    let hyp = h
        .service
        .create(NodeType::Hypothesis, NodeContent::new("H", ""))
        .unwrap();
    h.service
        .link(&e1.node_id, &hyp.node_id, Relation::Custom("supports".into()), None)
        .unwrap();
    h.service
        .link(&e2.node_id, &hyp.node_id, Relation::Custom("supports".into()), None)
        .unwrap();

    let engine = ReasoningEngine::new(&h.repo);
    let chains = engine.find_evidence_chains(&hyp.node_id, 3).unwrap();
    assert_eq!(chains.len(), 2);
    for c in &chains {
        assert!(matches!(c.nodes[0].node_type, NodeType::Evidence));
        assert_eq!(c.nodes.last().unwrap().node_id, hyp.node_id);
    }
}

#[test]
fn cognitive_neighborhood_filters_to_meta_nodes() {
    let h = setup();
    let n_note = h
        .service
        .create(NodeType::Note, NodeContent::new("ordinary", ""))
        .unwrap();
    let n_refl = h
        .service
        .create(NodeType::Reflection, NodeContent::new("thought", ""))
        .unwrap();
    let n_ins = h
        .service
        .create(NodeType::Insight, NodeContent::new("insight", ""))
        .unwrap();
    h.service
        .link(&n_note.node_id, &n_refl.node_id, Relation::References, None)
        .unwrap();
    h.service
        .link(&n_refl.node_id, &n_ins.node_id, Relation::References, None)
        .unwrap();

    let engine = ReasoningEngine::new(&h.repo);
    let cog = engine.cognitive_neighborhood(&n_note.node_id, 3).unwrap();
    let ids: Vec<_> = cog.iter().map(|n| n.node_id.as_str()).collect();
    assert!(ids.contains(&n_refl.node_id.as_str()));
    assert!(ids.contains(&n_ins.node_id.as_str()));
    assert!(!ids.contains(&n_note.node_id.as_str()));
}

#[test]
fn detect_contradictions_via_explicit_edge() {
    let h = setup();
    let a = h
        .service
        .create(NodeType::Hypothesis, NodeContent::new("A", "earth flat"))
        .unwrap();
    let b = h
        .service
        .create(NodeType::Hypothesis, NodeContent::new("B", "earth round"))
        .unwrap();
    h.service
        .link(
            &a.node_id,
            &b.node_id,
            Relation::Custom("contradicts".into()),
            None,
        )
        .unwrap();

    let engine = ReasoningEngine::new(&h.repo);
    let reports = engine.detect_contradictions(20).unwrap();
    assert_eq!(reports.len(), 1);
    assert!(reports[0].reason.contains("contradicts"));
}

#[test]
fn detect_contradictions_via_counter_argument_reference() {
    let h = setup();
    let a = h
        .service
        .create(NodeType::Insight, NodeContent::new("A", ""))
        .unwrap();
    let b = h
        .service
        .create_with_meta(
            NodeType::Insight,
            NodeContent::new("B", ""),
            MetaCognition::new().with_counter_argument(format!(
                "but node {} says otherwise",
                a.node_id
            )),
            None,
        )
        .unwrap();

    let engine = ReasoningEngine::new(&h.repo);
    let reports = engine.detect_contradictions(20).unwrap();
    assert!(reports
        .iter()
        .any(|r| (r.a.node_id == a.node_id && r.b.node_id == b.node_id)
            || (r.a.node_id == b.node_id && r.b.node_id == a.node_id)));
}

#[test]
fn validate_consistency_flags_inconsistent_lean_block() {
    let h = setup();
    // Create a theorem and corrupt its consistency by an out-of-band update
    // patch where verified_by_lean is true but status is failed.
    let src = "theorem t : True := trivial";
    let n = h.service.create_theorem("t", src, &h.cas).unwrap();
    let bad = dt_knowledge::LeanVerification {
        verified_by_lean: true,
        lean_theorem_hash: n.lean.as_ref().and_then(|l| l.lean_theorem_hash.clone()),
        lean_proof_hash: None,
        lean_proof_status: LeanProofStatus::Failed, // inconsistent!
        verifier_version: Some("stub".into()),
        verified_at: None,
        last_error: None,
    };
    h.service
        .update(
            &n.node_id,
            NodePatch {
                lean: Some(bad),
                ..Default::default()
            },
        )
        .unwrap();

    let engine = ReasoningEngine::new(&h.repo);
    let issues = engine.validate_consistency().unwrap();
    assert!(issues.iter().any(|i| i.contains("inconsistent lean fields")));
}

#[test]
fn validate_consistency_flags_orphaned_derivation() {
    let h = setup();
    h.service
        .create_with_meta(
            NodeType::Insight,
            NodeContent::new("derived from nothing", ""),
            MetaCognition::new().with_derivation_depth(3),
            None,
        )
        .unwrap();

    let engine = ReasoningEngine::new(&h.repo);
    let issues = engine.validate_consistency().unwrap();
    assert!(issues.iter().any(|i| i.contains("derivation_depth=3")));
}

// ── export ──────────────────────────────────────────────────────────────────

#[test]
fn export_mermaid_and_dot_for_walked_scene() {
    let h = setup();
    let a = h
        .service
        .create(NodeType::Evidence, NodeContent::new("E", ""))
        .unwrap();
    let b = h
        .service
        .create(NodeType::Insight, NodeContent::new("I", ""))
        .unwrap();
    h.service
        .link(&a.node_id, &b.node_id, Relation::References, None)
        .unwrap();

    let scene = GraphScene::from_walk(&h.repo, &a.node_id, 3, NeighborDirection::Outgoing).unwrap();
    assert_eq!(scene.nodes.len(), 2);
    assert_eq!(scene.edges.len(), 1);

    let mermaid = scene.to_mermaid();
    assert!(mermaid.starts_with("graph LR"));
    assert!(mermaid.contains("[insight]"));

    let dot = scene.to_dot();
    assert!(dot.starts_with("digraph"));
    assert!(dot.contains(&a.node_id));
    assert!(dot.contains(&b.node_id));
}

// ── End-to-end: hypothesis → reflection → Lean verification → insight ──────

#[test]
fn end_to_end_hypothesis_to_verified_insight() {
    let h = setup();

    // 1. Form a hypothesis with thinking trace.
    let hyp = h
        .service
        .create_with_meta(
            NodeType::Hypothesis,
            NodeContent::new("Sorting via merge is O(n log n)", ""),
            MetaCognition::new()
                .with_certainty(CertaintyType::Heuristic)
                .with_thinking_step(ThinkingStep::now("recurrence T(n)=2T(n/2)+n"))
                .with_assumption("comparison-based model")
                .with_open_question("is the constant tight?"),
            Some(0.7),
        )
        .unwrap();

    // 2. Capture a reflection on the hypothesis.
    let refl = h
        .service
        .create(
            NodeType::Reflection,
            NodeContent::new("Reflection", "Master theorem applies."),
        )
        .unwrap();
    h.service
        .link(&refl.node_id, &hyp.node_id, Relation::References, None)
        .unwrap();

    // 3. Express it as a theorem and verify with the Lean stub.
    let lean_src = "theorem mergesort_complexity : True := trivial";
    let theorem = h.service.create_theorem("MergeSortComplexity", lean_src, &h.cas).unwrap();
    h.service
        .link(&hyp.node_id, &theorem.node_id, Relation::Custom("formalized_as".into()), None)
        .unwrap();
    let v = StubLeanVerifier::new();
    let lean = h
        .service
        .verify_with_lean(&theorem.node_id, lean_src, &v, &h.cas)
        .unwrap();
    assert!(lean.verified_by_lean);

    // 4. Promote to an insight, citing the verified theorem.
    let insight = h
        .service
        .create_with_meta(
            NodeType::Insight,
            NodeContent::new(
                "MergeSort runs in Θ(n log n)",
                "Confirmed via formal proof.",
            ),
            MetaCognition::new()
                .with_certainty(CertaintyType::Proof)
                .with_derivation_depth(2),
            Some(0.99),
        )
        .unwrap();
    h.service
        .link(&theorem.node_id, &insight.node_id, Relation::Custom("supports".into()), None)
        .unwrap();

    // 5. Reason from evidence-side back: insight should be reachable from hypothesis.
    let engine = ReasoningEngine::new(&h.repo);
    let paths = engine
        .reason_path(&hyp.node_id, &insight.node_id, 5, NeighborDirection::Outgoing)
        .unwrap();
    assert!(!paths.is_empty());
    let chain = &paths[0];
    assert!(chain.nodes.iter().any(|n| n.node_id == theorem.node_id));

    // 6. Cognitive neighborhood from `refl` reaches the insight.
    let cog = engine.cognitive_neighborhood(&refl.node_id, 4).unwrap();
    assert!(cog.iter().any(|n| n.node_id == insight.node_id));

    // 7. No consistency issues.
    let issues = engine.validate_consistency().unwrap();
    assert!(issues.is_empty(), "expected zero issues, got {:?}", issues);
}
