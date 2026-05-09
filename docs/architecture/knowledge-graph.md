# Knowledge Graph — Implementation Notes

Companion to README's `dt-knowledge` section. Documents what was built, why,
and how it composes with `dt-event`.

## Why this layer?

Per the Consolidated Spec hierarchy: **Event Sourcing → Metadata → CAS →
Knowledge Graph → Agents**. The knowledge graph is the first user-facing
data type. Every higher-level feature (notes, tasks, agents acting on
"the user's context") consumes it.

Crucially, building it as a **projection over the event log** validates the
core spec invariant — *"State = materialized view of immutable events"* — and
proves out the `Projection` trait we shipped in `dt-event`.

## Three-layer architecture

```
   ┌──────────────────────────────────────────────┐
   │  KnowledgeService     (write API, emits)     │
   │      │                                        │
   │      ▼                                        │
   │  EventBuilder ─▶ Event ─▶ EventStore::append │
   │                                ┌───────┘     │
   │                                ▼              │
   │  KnowledgeProjection (registered, applies)   │
   │      │                                        │
   │      ▼                                        │
   │  SQLite materialized view                    │
   │      ▲                                        │
   │      │                                        │
   │  KnowledgeRepository (read-only API)         │
   └──────────────────────────────────────────────┘
```

- **`KnowledgeService`** — write API. Builds `EventType::KnowledgeCreate /
  Update / Delete / Link / Unlink` events, seals them, appends them.
- **`KnowledgeProjection`** — implements `dt_event::Projection`. Registered with
  the `EventStore`, fired synchronously after every append. Updates SQLite.
- **`KnowledgeRepository`** — read API. SQL + FTS5 + BFS over the materialized
  tables. Never mutates state.

All three share an `Arc<KnowledgeDb>`, which wraps the SQLite `Connection` in
a `Mutex` so it can be `Send + Sync` (required by `Projection`).

## Domain model

### `KnowledgeNode`
- `node_id`: ULID (chronological)
- `node_type`: typed enum (`note`, `task`, `project`, `person`, `concept`, …)
  + `Custom(String)` escape hatch
- `content`: `{title, body, abstract?}`
- `properties`: free-form `serde_json::Map`
- `metadata`: full `MetadataEnvelope`
- `status`: `active`/`archived`/`deleted`/`draft`/`pending`
- `visibility`: `private`/`shared`/`public`

### `KnowledgeEdge`
- `edge_id`: ULID
- `source_id`, `target_id`
- `relation`: typed enum (`references`, `child_of`, `depends_on`, …) + `Custom`
- `weight`: optional `f64` clamped to [0, 1]
- `metadata`: full `MetadataEnvelope`

## Event payload schemas

| Event type            | Payload (informal)                                                  |
|-----------------------|---------------------------------------------------------------------|
| `knowledge.create`    | `{node_id, node_type, content, properties, status, visibility}`     |
| `knowledge.update`    | `{node_id, content?, status?, visibility?, properties?}` (patch)    |
| `knowledge.delete`    | `{node_id}` — soft delete (tombstone preserved)                     |
| `knowledge.link`      | `{edge_id, source_id, target_id, relation, weight?}`                |
| `knowledge.unlink`    | `{edge_id}` — soft delete                                            |

The projection ignores any non-knowledge events (returns `Ok(())`).

## Tombstones, not hard deletes

Both nodes and edges use `deleted = 1` flags rather than physical deletion.
This is required for CRDT correctness: a remote peer that has not yet seen
the delete event would otherwise re-insert the node when its create event
arrives during sync. With tombstones, the projection ignores create events
for nodes already marked deleted (idempotency via `ON CONFLICT DO NOTHING`).

## FTS5 search

`knowledge_fts` is a separate FTS5 virtual table. The projection mirrors
`title` and `body` into it on `create`/`update` and removes the row on
`delete`. The repository wraps user queries with `"phrase"` quoting to be
safe against FTS5 metacharacters (`AND`, `OR`, `*`, `:`, etc.) and ranks
results with `bm25()`.

## Graph walks

`KnowledgeRepository::walk` is a simple BFS up to N hops. Direction is
configurable: `Outgoing`, `Incoming`, or `Both`. `Both` deduplicates by
node_id and follows edges in either direction.

For dense graphs this is O(V+E); for v0.1 it's sufficient. Future work:
materialize transitive closures or use a recursive-CTE query for deeper walks.

## Concurrency model

- `KnowledgeDb` wraps `Connection` in `Mutex<Connection>`.
- Every public method takes the lock, does its work, drops the lock.
- This serializes reads and writes through one connection — fine for v0.1
  single-user nodes. A future optimization is to use a connection pool
  (rusqlite + r2d2 or a custom `RwLock<Vec<Connection>>`).

## Testing

| Layer                     | Tests |
|---------------------------|------:|
| Node + Edge unit tests    |     7 |
| End-to-end integration    |     8 |

The integration suite exercises:
- Create → get roundtrip
- Partial updates (only patched fields change)
- Soft delete (visible via `get_including_deleted`, hidden by default)
- FTS5 search across multiple nodes
- Link / unlink cycles
- Self-loop rejection
- Multi-hop graph walks (depth 1, 2, full)
- List + count filtered by node_type

## Failure modes

| Condition                                | Behavior |
|------------------------------------------|----------|
| Event payload missing required field     | `EventError::Invalid` — projection skips |
| Linking a node to itself                 | `KnowledgeError::Invalid` — service rejects |
| Update on non-existent node              | No-op (no row affected) — by design |
| FTS5 search on empty query               | Returns empty `Vec` — no I/O |

## Open questions / future work

- **CRDT-aware merging.** Right now updates are last-writer-wins by SQL
  timestamp. We need to use the event's vector clock for proper LWW under
  sync. The hooks are in place (events carry `VectorClock`); just needs
  per-field clocks in the materialized table.
- **Schema-driven validation.** `dt-schema` should validate `knowledge.*`
  event payloads against the JSON Schemas before append. Currently we trust
  the service.
- **Event replay / rebuild.** Add a `rebuild()` API that drops the materialized
  tables and replays all `knowledge.*` events from the log. Critical for
  schema migrations and disaster recovery.
- **Embedding pipeline.** `metadata.dt_embeddings` is a stub. Wire to
  `nomic-embed-text` via Ollama and populate the `embeddings` table on create
  and update.
