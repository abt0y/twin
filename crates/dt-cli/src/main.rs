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
    service::NodePatch, KnowledgeDb, KnowledgeProjection, KnowledgeRepository, KnowledgeService,
    NeighborDirection, NodeContent, NodeStatus, NodeType, Relation,
};

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
        },
        Commands::Generate { target } => cmd_generate(target),
        Commands::ValidateSchemas => cmd_validate_schemas(),
        Commands::Status => cmd_status(),
    }
}
