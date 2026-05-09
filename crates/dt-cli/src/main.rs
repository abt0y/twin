//! `dt` — Digital Twin CLI
//!
//! Entry point for the local-first DT platform.

use std::path::PathBuf;

use clap::{Parser, Subcommand};
use std::sync::Arc;

use dt_event::{
    Event, EventBuilder, EventStore, EventStoreConfig, EventType, MetadataEnvelope,
};
use dt_knowledge::{
    export::GraphScene, reasoning::ReasoningEngine, service::NodePatch, CertaintyType,
    ExternalLeanVerifier, KnowledgeDb, KnowledgeProjection, KnowledgeRepository, KnowledgeService,
    LeanVerifier, MetaCognition, NeighborDirection, NodeContent, NodeStatus, NodeType, Relation,
    StubLeanVerifier, ThinkingStep,
};
use dt_graph_ui::Dashboard;
use dt_core::cas::CasStore;

#[derive(Parser)]
#[command(name = "dt")]
#[command(about = "Digital Twin Platform CLI", long_about = None)]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Path to node.toml config (defaults to ~/.dt/node.toml)
    #[arg(short, long, global = true)]
    config: Option<PathBuf>,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize the local DT environment (~/.dt)
    Init {
        /// Force re-initialization
        #[arg(long)]
        force: bool,
    },

    /// Append, list, and verify events
    Event {
        #[command(subcommand)]
        cmd: EventCmd,
    },

    /// Manage the knowledge graph
    Knowledge {
        #[command(subcommand)]
        cmd: KnowledgeCmd,
    },

    /// Visualize / explore the knowledge graph (TUI + exporters)
    Graph {
        #[command(subcommand)]
        cmd: GraphCmd,
    },

    /// Run code generation from schemas
    Generate {
        /// Target language(s): rust, python, ts
        #[arg(short, long, value_delimiter = ',')]
        target: Vec<String>,
    },

    /// Validate all schemas in the registry
    ValidateSchemas,

    /// Status of the local node
    Status,
}

#[derive(Subcommand)]
enum EventCmd {
    /// Append a new event to the local log
    Append {
        /// Event type (e.g. knowledge.create, agent.action, custom.foo)
        #[arg(short = 't', long)]
        event_type: String,

        /// Source node ID (defaults to "local")
        #[arg(short, long, default_value = "local")]
        node_id: String,

        /// Owner DID (defaults to "did:dt:local")
        #[arg(short, long, default_value = "did:dt:local")]
        owner: String,

        /// Acting user DID (optional)
        #[arg(short, long)]
        user: Option<String>,

        /// JSON payload
        #[arg(short, long, default_value = "{}")]
        payload: String,

        /// Previous event ID (for hash chain)
        #[arg(long)]
        prev: Option<String>,
    },

    /// List recent events
    List {
        #[arg(short, long, default_value = "20")]
        limit: usize,
    },

    /// Get a specific event by id
    Get {
        event_id: String,
    },

    /// Verify the integrity of the entire event log
    Verify,

    /// Show total event count
    Count,
}

#[derive(Subcommand)]
enum KnowledgeCmd {
    /// Create a new knowledge node
    Create {
        /// Node type (note|task|project|person|concept|...)
        #[arg(short = 't', long, default_value = "note")]
        node_type: String,
        /// Title
        #[arg(short = 'T', long)]
        title: String,
        /// Body (Markdown)
        #[arg(short, long, default_value = "")]
        body: String,
    },

    /// Get a node by id
    Get { node_id: String },

    /// List recent nodes
    List {
        #[arg(short = 't', long)]
        node_type: Option<String>,
        #[arg(short, long, default_value = "20")]
        limit: usize,
    },

    /// Full-text search the knowledge graph
    Search {
        query: String,
        #[arg(short, long, default_value = "20")]
        limit: usize,
    },

    /// Patch an existing node
    Update {
        node_id: String,
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        body: Option<String>,
        #[arg(long)]
        status: Option<String>,
    },

    /// Soft-delete a node
    Delete { node_id: String },

    /// Link two nodes
    Link {
        source: String,
        target: String,
        #[arg(short, long, default_value = "related_to")]
        relation: String,
        #[arg(short, long)]
        weight: Option<f64>,
    },

    /// Unlink (soft-delete an edge)
    Unlink { edge_id: String },

    /// List neighbors of a node
    Neighbors {
        node_id: String,
        #[arg(short, long, default_value = "both")]
        direction: String,
        #[arg(short, long, default_value = "20")]
        limit: usize,
    },

    /// Total live node count
    Count,

    // ── meta-cognition ──────────────────────────────────────────────────────
    /// Add or replace meta-cognition on an existing node
    Meta {
        node_id: String,
        #[arg(long)]
        certainty: Option<String>,
        #[arg(long)]
        assumption: Vec<String>,
        #[arg(long = "counter")]
        counter_argument: Vec<String>,
        #[arg(long = "question")]
        open_question: Vec<String>,
        #[arg(long = "thought")]
        thinking_step: Vec<String>,
        #[arg(long, default_value = "0")]
        derivation_depth: u32,
        #[arg(long)]
        confidence: Option<f64>,
    },

    /// List nodes whose `confidence` < threshold
    LowConfidence {
        #[arg(short, long, default_value = "0.5")]
        threshold: f64,
        #[arg(short, long, default_value = "20")]
        limit: usize,
    },

    /// List nodes carrying open meta-cognition questions
    OpenQuestions {
        #[arg(short, long, default_value = "20")]
        limit: usize,
    },

    // ── Lean 4 ──────────────────────────────────────────────────────────────
    /// Create a `theorem` node from Lean source (file or stdin)
    LeanCreate {
        title: String,
        /// Path to .lean file. Use '-' to read from stdin.
        #[arg(short, long)]
        file: String,
    },

    /// Verify a theorem node with the bundled stub Lean verifier
    LeanVerify {
        node_id: String,
        /// Path to .lean file. If omitted, uses the node's body.
        #[arg(short, long)]
        file: Option<String>,
        /// Use the external `lean` binary on PATH instead of the stub.
        #[arg(long)]
        external: bool,
    },

    /// List nodes by Lean proof status (verified|failed|pending|unknown)
    LeanStatus {
        status: String,
        #[arg(short, long, default_value = "20")]
        limit: usize,
    },

    // ── reasoning ───────────────────────────────────────────────────────────
    /// Find reasoning paths between two nodes (BFS, outgoing edges)
    ReasonPath {
        source: String,
        target: String,
        #[arg(short, long, default_value = "5")]
        max_depth: usize,
    },

    /// Find evidence chains supporting a node
    Evidence {
        node_id: String,
        #[arg(short, long, default_value = "5")]
        max_depth: usize,
    },

    /// Detect contradictions in the graph
    Contradictions {
        #[arg(short, long, default_value = "20")]
        limit: usize,
    },

    /// Validate graph consistency (orphans, lean inconsistencies, etc.)
    Validate,
}

#[derive(Subcommand)]
enum GraphCmd {
    /// Print headless dashboard stats as JSON
    Dashboard,

    /// Launch the interactive TUI
    Tui,

    /// Export the graph in a given format
    Export {
        /// One of: mermaid | dot | json
        #[arg(short, long, default_value = "mermaid")]
        format: String,
        /// Optional root node — exports a walked subgraph from this node.
        /// If omitted, exports the most recent N nodes.
        #[arg(short, long)]
        root: Option<String>,
        #[arg(short, long, default_value = "2")]
        depth: usize,
        #[arg(short, long, default_value = "100")]
        limit: usize,
    },
}

fn init_tracing() {
    let subscriber = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .json()
        .with_target(true)
        .with_current_span(false);
    let _ = subscriber.try_init();
}

fn open_event_store() -> anyhow::Result<EventStore> {
    let cfg = EventStoreConfig::from_dt_dir();
    let store = EventStore::open(cfg)?;
    Ok(store)
}

fn parse_event_type(s: &str) -> EventType {
    match serde_json::from_value::<EventType>(serde_json::Value::String(s.to_string())) {
        Ok(et) => et,
        Err(_) => EventType::Custom(s.to_string()),
    }
}

fn print_event(ev: &Event) {
    let val = serde_json::to_value(ev).unwrap_or(serde_json::Value::Null);
    println!("{}", serde_json::to_string_pretty(&val).unwrap_or_default());
}

fn cmd_init(force: bool) -> anyhow::Result<()> {
    let dt_dir = dt_core::resolve_dt_dir();
    if dt_dir.exists() && !force {
        println!("DT already initialized at {}", dt_dir.display());
        return Ok(());
    }
    std::fs::create_dir_all(&dt_dir)?;
    for sub in &[
        "events", "knowledge", "cas", "config", "schemas", "agents", "logs", "sync", "tmp",
    ] {
        std::fs::create_dir_all(dt_dir.join(sub))?;
    }
    // Open the event store once to materialize the SQLite DB and log file.
    let _ = open_event_store()?;
    println!("Initialized DT at {}", dt_dir.display());
    Ok(())
}

fn cmd_event_append(
    event_type: &str,
    node_id: &str,
    owner: &str,
    user: Option<&str>,
    payload: &str,
    prev: Option<&str>,
) -> anyhow::Result<()> {
    let store = open_event_store()?;

    let payload_val: serde_json::Value = serde_json::from_str(payload)
        .map_err(|e| anyhow::anyhow!("invalid payload JSON: {}", e))?;

    let mut builder = EventBuilder::new(parse_event_type(event_type), node_id, owner)
        .payload(payload_val);

    if let Some(u) = user {
        builder = builder.user(u);
    }
    if let Some(p) = prev {
        builder = builder.prev_event(p);
    }
    builder = builder.metadata(MetadataEnvelope::new(owner, "1.0.0"));

    let ev = builder.build()?;
    let hash = store.append(&ev)?;

    println!(
        "{}",
        serde_json::json!({
            "ok": true,
            "event_id": ev.event_id,
            "content_hash": hash,
            "event_type": ev.event_type.to_string(),
        })
    );
    Ok(())
}

fn cmd_event_list(limit: usize) -> anyhow::Result<()> {
    let store = open_event_store()?;
    let ids = store.list_ids(limit)?;
    for id in ids {
        if let Some(ev) = store.get(&id)? {
            println!(
                "{}\t{}\t{}\t{}",
                ev.event_id,
                ev.event_type,
                ev.timestamp.to_rfc3339(),
                ev.content_hash.as_deref().unwrap_or("(unsealed)")
            );
        }
    }
    Ok(())
}

fn cmd_event_get(event_id: &str) -> anyhow::Result<()> {
    let store = open_event_store()?;
    match store.get(event_id)? {
        Some(ev) => {
            print_event(&ev);
            Ok(())
        }
        None => {
            eprintln!("event not found: {}", event_id);
            std::process::exit(1);
        }
    }
}

fn cmd_event_verify() -> anyhow::Result<()> {
    let store = open_event_store()?;
    let n = store.verify_all()?;
    println!(
        "{}",
        serde_json::json!({"ok": true, "verified": n})
    );
    Ok(())
}

fn cmd_event_count() -> anyhow::Result<()> {
    let store = open_event_store()?;
    let n = store.count()?;
    println!("{}", n);
    Ok(())
}

fn cmd_generate(target: Vec<String>) -> anyhow::Result<()> {
    let schemas_dir = PathBuf::from("schemas");
    let registry = dt_schema::SchemaRegistry::load_from_dir(&schemas_dir)?;
    let out_base = PathBuf::from("codegen");

    let targets: Vec<dt_codegen::Target> = if target.is_empty() {
        vec![
            dt_codegen::Target::Rust,
            dt_codegen::Target::Python,
            dt_codegen::Target::TypeScript,
        ]
    } else {
        target
            .iter()
            .filter_map(|t| match t.as_str() {
                "rust" => Some(dt_codegen::Target::Rust),
                "python" => Some(dt_codegen::Target::Python),
                "ts" | "typescript" => Some(dt_codegen::Target::TypeScript),
                _ => {
                    eprintln!("Unknown target: {}", t);
                    None
                }
            })
            .collect()
    };

    for t in targets {
        let t_dir = out_base.join(match t {
            dt_codegen::Target::Rust => "rust",
            dt_codegen::Target::Python => "python",
            dt_codegen::Target::TypeScript => "ts",
        });
        std::fs::create_dir_all(&t_dir)?;
        let results = dt_codegen::generate_all(&registry, &t_dir)?;
        for (name, paths) in results {
            println!("Generated {} -> {:?}", name, paths);
        }
    }
    Ok(())
}

fn cmd_validate_schemas() -> anyhow::Result<()> {
    let schemas_dir = PathBuf::from("schemas");
    let registry = dt_schema::SchemaRegistry::load_from_dir(&schemas_dir)?;
    println!("Loaded {} schema(s)", registry.len());
    for name in registry.list_names() {
        println!("  - {}", name);
    }
    Ok(())
}

fn open_knowledge_stack() -> anyhow::Result<(Arc<EventStore>, Arc<KnowledgeDb>, KnowledgeService, KnowledgeRepository)> {
    let cfg = EventStoreConfig::from_dt_dir();
    let mut store = EventStore::open(cfg.clone())?;
    let db = Arc::new(KnowledgeDb::open(&cfg.db_path)?);
    let projection = Arc::new(KnowledgeProjection::new(db.clone())?);
    store.register_projection(projection);
    let store = Arc::new(store);
    let service = KnowledgeService::new(store.clone(), "local", "did:dt:local");
    let repo = KnowledgeRepository::new(db.clone());
    Ok((store, db, service, repo))
}

fn open_full_stack() -> anyhow::Result<(KnowledgeService, KnowledgeRepository, CasStore)> {
    let cfg = EventStoreConfig::from_dt_dir();
    let mut store = EventStore::open(cfg.clone())?;
    let db = Arc::new(KnowledgeDb::open(&cfg.db_path)?);
    let projection = Arc::new(KnowledgeProjection::new(db.clone())?);
    store.register_projection(projection);
    let store = Arc::new(store);
    let service = KnowledgeService::new(store, "local", "did:dt:local");
    let repo = KnowledgeRepository::new(db);
    let cas = CasStore::open(&cfg.cas_path)?;
    Ok((service, repo, cas))
}

fn read_lean_source(file: &str) -> anyhow::Result<String> {
    use std::io::Read;
    if file == "-" {
        let mut s = String::new();
        std::io::stdin().read_to_string(&mut s)?;
        Ok(s)
    } else {
        Ok(std::fs::read_to_string(file)?)
    }
}

fn cmd_knowledge_create(node_type: &str, title: &str, body: &str) -> anyhow::Result<()> {
    let (_, _, svc, _) = open_knowledge_stack()?;
    let n = svc.create(NodeType::parse(node_type), NodeContent::new(title, body))?;
    println!(
        "{}",
        serde_json::json!({
            "ok": true,
            "node_id": n.node_id,
            "node_type": n.node_type.as_str(),
            "title": n.content.title,
        })
    );
    Ok(())
}

fn cmd_knowledge_get(node_id: &str) -> anyhow::Result<()> {
    let (_, _, _, repo) = open_knowledge_stack()?;
    match repo.get(node_id)? {
        Some(n) => {
            println!("{}", serde_json::to_string_pretty(&n)?);
            Ok(())
        }
        None => {
            eprintln!("not found: {}", node_id);
            std::process::exit(1);
        }
    }
}

fn cmd_knowledge_list(node_type: Option<&str>, limit: usize) -> anyhow::Result<()> {
    let (_, _, _, repo) = open_knowledge_stack()?;
    let nt = node_type.map(NodeType::parse);
    let nodes = repo.list(nt.as_ref(), limit)?;
    for n in nodes {
        println!(
            "{}\t{}\t{}\t{}",
            n.node_id,
            n.node_type.as_str(),
            n.status.as_str(),
            n.content.title
        );
    }
    Ok(())
}

fn cmd_knowledge_search(query: &str, limit: usize) -> anyhow::Result<()> {
    let (_, _, _, repo) = open_knowledge_stack()?;
    let nodes = repo.search(query, limit)?;
    for n in nodes {
        println!(
            "{}\t{}\t{}",
            n.node_id,
            n.node_type.as_str(),
            n.content.title
        );
    }
    Ok(())
}

fn cmd_knowledge_update(
    node_id: &str,
    title: Option<&str>,
    body: Option<&str>,
    status: Option<&str>,
) -> anyhow::Result<()> {
    let (_, _, svc, _) = open_knowledge_stack()?;
    let patch = NodePatch {
        title: title.map(String::from),
        body: body.map(String::from),
        status: status.map(NodeStatus::parse),
        ..Default::default()
    };
    svc.update(node_id, patch)?;
    println!("{}", serde_json::json!({"ok": true, "node_id": node_id}));
    Ok(())
}

fn cmd_knowledge_delete(node_id: &str) -> anyhow::Result<()> {
    let (_, _, svc, _) = open_knowledge_stack()?;
    svc.delete(node_id)?;
    println!("{}", serde_json::json!({"ok": true, "node_id": node_id}));
    Ok(())
}

fn cmd_knowledge_link(
    source: &str,
    target: &str,
    relation: &str,
    weight: Option<f64>,
) -> anyhow::Result<()> {
    let (_, _, svc, _) = open_knowledge_stack()?;
    let edge = svc.link(source, target, Relation::parse(relation), weight)?;
    println!(
        "{}",
        serde_json::json!({
            "ok": true,
            "edge_id": edge.edge_id,
            "source": edge.source_id,
            "target": edge.target_id,
            "relation": edge.relation.as_str(),
        })
    );
    Ok(())
}

fn cmd_knowledge_unlink(edge_id: &str) -> anyhow::Result<()> {
    let (_, _, svc, _) = open_knowledge_stack()?;
    svc.unlink(edge_id)?;
    println!("{}", serde_json::json!({"ok": true, "edge_id": edge_id}));
    Ok(())
}

fn cmd_knowledge_neighbors(node_id: &str, direction: &str, limit: usize) -> anyhow::Result<()> {
    let (_, _, _, repo) = open_knowledge_stack()?;
    let dir = match direction {
        "outgoing" | "out" => NeighborDirection::Outgoing,
        "incoming" | "in" => NeighborDirection::Incoming,
        _ => NeighborDirection::Both,
    };
    let edges = repo.neighbors(node_id, dir, None, limit)?;
    for e in edges {
        println!(
            "{}\t{} -[{}]-> {}\t{:?}",
            e.edge_id,
            e.source_id,
            e.relation.as_str(),
            e.target_id,
            e.weight
        );
    }
    Ok(())
}

fn cmd_knowledge_count() -> anyhow::Result<()> {
    let (_, _, _, repo) = open_knowledge_stack()?;
    println!("{}", repo.count()?);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn cmd_knowledge_meta(
    node_id: &str,
    certainty: Option<&str>,
    assumption: Vec<String>,
    counter_argument: Vec<String>,
    open_question: Vec<String>,
    thinking_step: Vec<String>,
    derivation_depth: u32,
    confidence: Option<f64>,
) -> anyhow::Result<()> {
    let (svc, _, _) = open_full_stack()?;
    let mut mc = MetaCognition::new()
        .with_derivation_depth(derivation_depth);
    if let Some(c) = certainty {
        mc = mc.with_certainty(CertaintyType::parse(c));
    }
    for a in assumption {
        mc = mc.with_assumption(a);
    }
    for c in counter_argument {
        mc = mc.with_counter_argument(c);
    }
    for q in open_question {
        mc = mc.with_open_question(q);
    }
    for s in thinking_step {
        mc = mc.with_thinking_step(ThinkingStep::now(s));
    }
    svc.set_meta_cognition(node_id, mc, confidence)?;
    println!("{}", serde_json::json!({"ok": true, "node_id": node_id}));
    Ok(())
}

fn cmd_knowledge_low_confidence(threshold: f64, limit: usize) -> anyhow::Result<()> {
    let (_, repo, _) = open_full_stack()?;
    for n in repo.list_low_confidence(threshold, limit)? {
        println!(
            "{}\t{}\t{:?}\t{}",
            n.node_id,
            n.node_type.as_str(),
            n.metadata.dt_confidence,
            n.content.title
        );
    }
    Ok(())
}

fn cmd_knowledge_open_questions(limit: usize) -> anyhow::Result<()> {
    let (_, repo, _) = open_full_stack()?;
    for n in repo.list_with_open_questions(limit)? {
        let qs = n
            .meta_cognition
            .as_ref()
            .map(|m| m.open_questions.clone())
            .unwrap_or_default();
        println!(
            "{}\t{}\t{}\t{}",
            n.node_id,
            n.node_type.as_str(),
            qs.len(),
            n.content.title
        );
        for q in qs {
            println!("    ? {}", q);
        }
    }
    Ok(())
}

fn cmd_knowledge_lean_create(title: &str, file: &str) -> anyhow::Result<()> {
    let src = read_lean_source(file)?;
    let (svc, _, cas) = open_full_stack()?;
    let n = svc.create_theorem(title, &src, &cas)?;
    let lean = n.lean.unwrap();
    println!(
        "{}",
        serde_json::json!({
            "ok": true,
            "node_id": n.node_id,
            "lean_theorem_hash": lean.lean_theorem_hash,
            "status": lean.lean_proof_status.as_str(),
        })
    );
    Ok(())
}

fn cmd_knowledge_lean_verify(node_id: &str, file: Option<&str>, external: bool) -> anyhow::Result<()> {
    let (svc, repo, cas) = open_full_stack()?;
    let src = match file {
        Some(f) => read_lean_source(f)?,
        None => repo
            .get(node_id)?
            .map(|n| n.content.body)
            .ok_or_else(|| anyhow::anyhow!("node not found"))?,
    };

    let stub = StubLeanVerifier::new();
    let ext = ExternalLeanVerifier::new();
    let verifier: &dyn LeanVerifier = if external { &ext } else { &stub };

    let lean = svc.verify_with_lean(node_id, &src, verifier, &cas)?;
    println!(
        "{}",
        serde_json::json!({
            "ok": lean.verified_by_lean,
            "node_id": node_id,
            "status": lean.lean_proof_status.as_str(),
            "verifier": verifier.name(),
            "diagnostics": lean.last_error,
            "proof_hash": lean.lean_proof_hash,
        })
    );
    Ok(())
}

fn cmd_knowledge_lean_status(status: &str, limit: usize) -> anyhow::Result<()> {
    let (_, repo, _) = open_full_stack()?;
    for n in repo.list_by_lean_status(status, limit)? {
        let lean = n.lean.unwrap_or_default();
        println!(
            "{}\t{}\t{}\t{}",
            n.node_id,
            lean.lean_proof_status.as_str(),
            lean.verifier_version.unwrap_or_default(),
            n.content.title
        );
    }
    Ok(())
}

fn cmd_knowledge_reason_path(source: &str, target: &str, max_depth: usize) -> anyhow::Result<()> {
    let (_, repo, _) = open_full_stack()?;
    let engine = ReasoningEngine::new(&repo);
    let paths = engine.reason_path(source, target, max_depth, NeighborDirection::Outgoing)?;
    if paths.is_empty() {
        println!("{}", serde_json::json!({"paths": []}));
        return Ok(());
    }
    for (i, p) in paths.iter().enumerate() {
        println!(
            "path #{} (depth={}, min_confidence={:?}):",
            i, p.depth, p.min_confidence
        );
        for n in &p.nodes {
            println!(
                "  {} [{}] {}",
                n.node_id,
                n.node_type.as_str(),
                n.content.title
            );
        }
    }
    Ok(())
}

fn cmd_knowledge_evidence(node_id: &str, max_depth: usize) -> anyhow::Result<()> {
    let (_, repo, _) = open_full_stack()?;
    let engine = ReasoningEngine::new(&repo);
    let chains = engine.find_evidence_chains(node_id, max_depth)?;
    if chains.is_empty() {
        println!("(no evidence chains)");
        return Ok(());
    }
    for (i, c) in chains.iter().enumerate() {
        println!("chain #{} (depth={}):", i, c.depth);
        for n in &c.nodes {
            println!(
                "  {} [{}] {}",
                n.node_id,
                n.node_type.as_str(),
                n.content.title
            );
        }
    }
    Ok(())
}

fn cmd_knowledge_contradictions(limit: usize) -> anyhow::Result<()> {
    let (_, repo, _) = open_full_stack()?;
    let engine = ReasoningEngine::new(&repo);
    let reports = engine.detect_contradictions(limit)?;
    for r in reports {
        println!(
            "{}\t{}\t{}\t{}",
            r.a.node_id, r.b.node_id, r.a.content.title, r.reason
        );
    }
    Ok(())
}

fn cmd_knowledge_validate() -> anyhow::Result<()> {
    let (_, repo, _) = open_full_stack()?;
    let engine = ReasoningEngine::new(&repo);
    let issues = engine.validate_consistency()?;
    if issues.is_empty() {
        println!("{}", serde_json::json!({"ok": true, "issues": []}));
    } else {
        for i in &issues {
            println!("{}", i);
        }
        std::process::exit(1);
    }
    Ok(())
}

fn cmd_graph_dashboard() -> anyhow::Result<()> {
    let (_, repo, _) = open_full_stack()?;
    let stats = Dashboard::new(&repo).compute(10_000)?;
    println!("{}", serde_json::to_string_pretty(&stats)?);
    Ok(())
}

fn cmd_graph_tui() -> anyhow::Result<()> {
    let (_, repo, _) = open_full_stack()?;
    dt_graph_ui::tui::run(&repo)?;
    Ok(())
}

fn cmd_graph_export(
    format: &str,
    root: Option<&str>,
    depth: usize,
    limit: usize,
) -> anyhow::Result<()> {
    let (_, repo, _) = open_full_stack()?;
    let scene = match root {
        Some(r) => GraphScene::from_walk(&repo, r, depth, NeighborDirection::Both)?,
        None => GraphScene::from_latest(&repo, limit)?,
    };
    let out = match format {
        "mermaid" => scene.to_mermaid(),
        "dot" | "graphviz" => scene.to_dot(),
        "json" => scene.to_json()?,
        other => anyhow::bail!("unknown format '{}'. use mermaid|dot|json", other),
    };
    println!("{}", out);
    Ok(())
}

fn cmd_status() -> anyhow::Result<()> {
    let dt_dir = dt_core::resolve_dt_dir();
    let store = open_event_store()?;
    let count = store.count()?;
    println!(
        "{}",
        serde_json::json!({
            "dt_dir": dt_dir.display().to_string(),
            "event_count": count,
            "log_path": store.log_path().display().to_string(),
        })
    );
    Ok(())
}

fn main() -> anyhow::Result<()> {
    init_tracing();
    let cli = Cli::parse();

    match cli.command {
        Commands::Init { force } => cmd_init(force),
        Commands::Event { cmd } => match cmd {
            EventCmd::Append {
                event_type,
                node_id,
                owner,
                user,
                payload,
                prev,
            } => cmd_event_append(
                &event_type,
                &node_id,
                &owner,
                user.as_deref(),
                &payload,
                prev.as_deref(),
            ),
            EventCmd::List { limit } => cmd_event_list(limit),
            EventCmd::Get { event_id } => cmd_event_get(&event_id),
            EventCmd::Verify => cmd_event_verify(),
            EventCmd::Count => cmd_event_count(),
        },
        Commands::Knowledge { cmd } => match cmd {
            KnowledgeCmd::Create { node_type, title, body } => {
                cmd_knowledge_create(&node_type, &title, &body)
            }
            KnowledgeCmd::Get { node_id } => cmd_knowledge_get(&node_id),
            KnowledgeCmd::List { node_type, limit } => {
                cmd_knowledge_list(node_type.as_deref(), limit)
            }
            KnowledgeCmd::Search { query, limit } => cmd_knowledge_search(&query, limit),
            KnowledgeCmd::Update { node_id, title, body, status } => cmd_knowledge_update(
                &node_id,
                title.as_deref(),
                body.as_deref(),
                status.as_deref(),
            ),
            KnowledgeCmd::Delete { node_id } => cmd_knowledge_delete(&node_id),
            KnowledgeCmd::Link { source, target, relation, weight } => {
                cmd_knowledge_link(&source, &target, &relation, weight)
            }
            KnowledgeCmd::Unlink { edge_id } => cmd_knowledge_unlink(&edge_id),
            KnowledgeCmd::Neighbors { node_id, direction, limit } => {
                cmd_knowledge_neighbors(&node_id, &direction, limit)
            }
            KnowledgeCmd::Count => cmd_knowledge_count(),
            KnowledgeCmd::Meta {
                node_id,
                certainty,
                assumption,
                counter_argument,
                open_question,
                thinking_step,
                derivation_depth,
                confidence,
            } => cmd_knowledge_meta(
                &node_id,
                certainty.as_deref(),
                assumption,
                counter_argument,
                open_question,
                thinking_step,
                derivation_depth,
                confidence,
            ),
            KnowledgeCmd::LowConfidence { threshold, limit } => {
                cmd_knowledge_low_confidence(threshold, limit)
            }
            KnowledgeCmd::OpenQuestions { limit } => cmd_knowledge_open_questions(limit),
            KnowledgeCmd::LeanCreate { title, file } => cmd_knowledge_lean_create(&title, &file),
            KnowledgeCmd::LeanVerify {
                node_id,
                file,
                external,
            } => cmd_knowledge_lean_verify(&node_id, file.as_deref(), external),
            KnowledgeCmd::LeanStatus { status, limit } => cmd_knowledge_lean_status(&status, limit),
            KnowledgeCmd::ReasonPath {
                source,
                target,
                max_depth,
            } => cmd_knowledge_reason_path(&source, &target, max_depth),
            KnowledgeCmd::Evidence { node_id, max_depth } => {
                cmd_knowledge_evidence(&node_id, max_depth)
            }
            KnowledgeCmd::Contradictions { limit } => cmd_knowledge_contradictions(limit),
            KnowledgeCmd::Validate => cmd_knowledge_validate(),
        },
        Commands::Graph { cmd } => match cmd {
            GraphCmd::Dashboard => cmd_graph_dashboard(),
            GraphCmd::Tui => cmd_graph_tui(),
            GraphCmd::Export {
                format,
                root,
                depth,
                limit,
            } => cmd_graph_export(&format, root.as_deref(), depth, limit),
        },
        Commands::Generate { target } => cmd_generate(target),
        Commands::ValidateSchemas => cmd_validate_schemas(),
        Commands::Status => cmd_status(),
    }
}
