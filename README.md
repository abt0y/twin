# DT — Sovereign Digital Twin Platform

A **production-grade, local-first, AI-native Digital Twin** platform.
**Sovereign** (you own all data). **Offline-capable**. **Append-only** event-sourced.
**Schema-driven**. **Multi-agent ready**. Built to scale to **1B+ users**.

> Consolidated Architecture Spec: **v1.1.0**
> Implementation Status: **v0.1 — Local Node, Event Sourcing complete**

---

## Architecture (one screen)

```
            ┌──────────────────────────────────┐
            │           dt-cli  (dt)           │
            └──────────────────────────────────┘
                          │
            ┌──────────────────────────────────┐
            │        dt-event  (this layer)    │
            │  • Event envelope + metadata     │
            │  • Hash-chain + causal deps      │
            │  • Projections (materialized)    │
            │  • JSONL audit trail             │
            └──────────────────────────────────┘
              │              │              │
   ┌──────────┴────┐  ┌──────┴──────┐  ┌────┴─────┐
   │  dt-db        │  │  dt-core    │  │ dt-sync  │
   │  SQLite hot   │  │  CAS (SHA3) │  │ Vec clock│
   │  index +FTS   │  │  Crypto     │  │ CRDT     │
   └───────────────┘  └─────────────┘  └──────────┘
```

### Crates

| Crate         | Purpose                                                        |
|---------------|----------------------------------------------------------------|
| `dt-core`     | Shared types, CAS storage, crypto (SHA3-256, Blake3), ULID/UUID|
| `dt-db`       | SQLite layer: events, knowledge nodes, FTS5, embeddings, vss   |
| `dt-event`    | Append-only event sourcing engine                              |
| `dt-knowledge`| **Knowledge graph API** — projection over the event log        |
| `dt-sync`     | Hybrid vector clocks + CRDT primitives (LWW, OR-Set) + delta   |
| `dt-schema`   | Schema registry, JSON Schema validation                        |
| `dt-codegen`  | Codegen → Rust / Python / TypeScript from schemas              |
| `dt-agent`    | Agent IPC over Unix socket + CBOR, runtime registry, sandbox   |
| `dt-cli`      | The `dt` command-line tool                                     |

---

## Core Invariants (Non-negotiable)

1. **Local-first** — Device is source of truth. Cloud/mesh = sync peers.
2. **Append-only** — Events, once committed, are never modified or deleted.
3. **Content-addressed** — Every object's identity is its SHA3-256 hash.
4. **Hash-chain integrity** — Each event optionally links to `prev_event_id`.
5. **Causal ordering** — Hybrid vector clock + explicit `causal_deps`.
6. **Universal Metadata Envelope** — Every object carries provenance & lineage.
7. **CAS-backed** — Payload bytes deduplicated by content hash.

---

## Event Sourcing Engine (`dt-event`)

The **single source of truth** for the platform.

```rust
use dt_event::{EventBuilder, EventStore, EventStoreConfig, EventType};

// Open or create a store rooted at ~/.dt/
let store = EventStore::open(EventStoreConfig::from_dt_dir())?;

// Build, seal, and append an event (hash-chained on prev_event)
let ev = EventBuilder::new(EventType::KnowledgeCreate, "node-alpha", "did:dt:alice")
    .user("did:dt:alice")
    .payload(serde_json::json!({"title": "Hello", "body": "World"}))
    .build()?;          // ← computes content_hash

let content_hash = store.append(&ev)?;

// Replay/audit: retrieve, verify, count
let again = store.get(&ev.event_id)?.unwrap();
again.verify_content_hash()?;
assert_eq!(store.count()?, 1);

// Walk the entire log and verify every hash + chain link
store.verify_all()?;
```

### What the EventStore does on every `append`
1. Validates the event is **sealed** (has a `content_hash`).
2. **Verifies** the hash matches the canonical bytes.
3. Rejects **duplicates** (`event_id` is unique forever).
4. Verifies **hash-chain** (`prev_event_id` exists if set).
5. Verifies **causal deps** (all referenced events exist).
6. Persists payload bytes to **CAS** (deduplicated).
7. Persists the canonical event bytes to **CAS**.
8. Inserts the SQLite hot-index row.
9. Appends a **JSONL audit line** to `~/.dt/logs/events.jsonl`.
10. Fires registered **projections** (materialized views).

### Projections

Implement the `Projection` trait to keep your own materialized view in sync:

```rust
pub trait Projection: Send + Sync {
    fn apply(&self, event: &Event) -> Result<(), EventError>;
    fn name(&self) -> &str;
}
```

A reference `InMemoryProjection` is included that counts events per type.

---

## Universal Metadata Envelope

Every object carries this:

```rust
pub struct MetadataEnvelope {
    dt_id: String,                  // UUID v7, time-ordered
    dt_version: String,             // "1.0.0"
    dt_created_at: DateTime<Utc>,
    dt_modified_at: Option<DateTime<Utc>>,
    dt_schema_version: String,
    dt_schema_hash: Option<String>, // SHA3-256 of schema bytes
    dt_owner: String,               // DID
    dt_source_node: Option<String>,
    dt_lineage: Vec<Lineage>,       // provenance chain
    dt_tags: Vec<String>,
    dt_confidence: Option<f64>,
    dt_embeddings: Option<EmbeddingMeta>,
}
```

Use the fluent builder:

```rust
let m = MetadataEnvelope::builder("did:dt:alice", "1.0.0")
    .source_node("node-xyz")
    .confidence(0.92)
    .tags(["work", "important"])
    .build();
```

---

## Knowledge Graph API (`dt-knowledge`)

The user-facing knowledge graph, **projected from the event log**. All writes
flow through `KnowledgeService` → `EventStore` → `KnowledgeProjection` → SQLite.

```rust
use std::sync::Arc;
use dt_event::{EventStore, EventStoreConfig};
use dt_knowledge::{
    service::NodePatch, KnowledgeDb, KnowledgeProjection, KnowledgeRepository,
    KnowledgeService, NeighborDirection, NodeContent, NodeType, Relation,
};

// Wire the stack
let cfg = EventStoreConfig::from_dt_dir();
let mut store = EventStore::open(cfg.clone())?;
let db = Arc::new(KnowledgeDb::open(&cfg.db_path)?);
store.register_projection(Arc::new(KnowledgeProjection::new(db.clone())?));
let store = Arc::new(store);
let svc = KnowledgeService::new(store, "node-alpha", "did:dt:alice");
let repo = KnowledgeRepository::new(db);

// Write
let n = svc.create(NodeType::Note, NodeContent::new("Hello", "Markdown body"))?;
svc.update(&n.node_id, NodePatch { title: Some("Hi".into()), ..Default::default() })?;
let edge = svc.link(&n.node_id, &other.node_id, Relation::References, Some(0.8))?;

// Read
let hits = repo.search("rust async", 10)?;
let neighbors = repo.neighbors(&n.node_id, NeighborDirection::Both, None, 50)?;
let subgraph = repo.walk(&n.node_id, 2, NeighborDirection::Outgoing)?;
```

### Why route writes through events?
- **Single source of truth.** SQLite tables are a *cache*. The truth is the log.
- **Audit + replay.** Rebuilding state == replaying events.
- **Sync-ready.** Events are the wire format for mesh sync.

---

## CLI Quickstart

```bash
# Build
cargo build -p dt-cli

# Initialize the data root (~/.dt)
./target/debug/dt init

# Append events
./target/debug/dt event append \
    --event-type knowledge.create \
    --node-id node-alpha \
    --owner did:dt:alice \
    --payload '{"title":"hello","body":"world"}'

# Append a follow-up linked via hash chain
./target/debug/dt event append \
    -t knowledge.update \
    -p '{"diff":"..."}' \
    --prev <event_id-from-step-above>

# Inspect
./target/debug/dt event count
./target/debug/dt event list --limit 50
./target/debug/dt event get <event_id>

# Verify the entire log (recomputes every hash + chain link)
./target/debug/dt event verify

# Knowledge graph
./target/debug/dt knowledge create -t note -T "Rust async" -b "tokio runtime"
./target/debug/dt knowledge create -t task -T "Read book" -b "Karpathy nano-gpt"
./target/debug/dt knowledge list -t note
./target/debug/dt knowledge search "rust"
./target/debug/dt knowledge link <source-id> <target-id> -r related_to
./target/debug/dt knowledge neighbors <node-id>
./target/debug/dt knowledge count

# Status
./target/debug/dt status
```

Every command emits **structured JSONL logs** to stderr and appends an audit
line to `~/.dt/logs/events.jsonl`.

---

## Filesystem Layout (`~/.dt/`)

```
~/.dt/
├── db.sqlite                  # Hot index: events, knowledge_nodes, FTS5
├── cas/                       # Content-addressable store (SHA3-256 keyed)
│   └── <2-hex>/<remaining-62-hex>
├── events/                    # (reserved: cold JSONL exports)
├── knowledge/                 # (reserved: Markdown + YAML frontmatter)
├── logs/
│   └── events.jsonl           # Append-only audit trail
├── config/
├── schemas/
├── agents/
└── sync/
```

---

## Testing

```bash
# All crates, all tests
cargo test --workspace

# Just the event engine
cargo test -p dt-event

# Just integration tests
cargo test -p dt-event --test integration
```

**Current status:** 80 tests passing across 9 crates.

| Crate         | Unit | Integration |
|---------------|-----:|------------:|
| dt-core       |    9 |           – |
| dt-db         |    7 |           – |
| dt-event      |   33 |           4 |
| dt-knowledge  |    7 |           8 |
| dt-sync       |    6 |           – |
| dt-schema     |    4 |           – |
| dt-codegen    |    1 |           – |
| dt-agent      |    1 |           – |

---

## Design Principles (Karpathy-clean)

- **No magic.** Every byte that gets hashed is in `canonical.rs` — one function.
- **Determinism.** Same input → same content hash → same CAS path. Always.
- **Testable.** Every public API has a unit test; the store has integration tests.
- **Observable.** Every append emits structured JSON to stderr and JSONL on disk.
- **Composable.** `dt-event` knows nothing about networking, agents, or UI.

---

## Roadmap

- [x] Workspace skeleton + schema registry
- [x] CAS storage (`dt-core::cas`)
- [x] SQLite layer with FTS5 + vector tables (`dt-db`)
- [x] Append-only event sourcing engine (`dt-event`)
- [x] **Knowledge Graph API (`dt-knowledge`) — CRUD, FTS5, edges, walk** ← *just shipped*
- [ ] Sync engine: QUIC transport + delta protocol
- [ ] Agent IPC daemon (`dtd`) with WASM sandbox
- [ ] Schema-driven SQL migration codegen
- [ ] Local embedding pipeline (nomic via Ollama / llama.cpp)

---

## License

MIT.
