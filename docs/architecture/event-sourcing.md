# Event Sourcing in DT — Implementation Notes

This is the deep-dive companion to the README's `dt-event` section. It documents
what was built, why, and the decisions you may want to revisit.

## Why event sourcing first?

The Consolidated Spec mandates: **"State = materialized view of immutable events."**
Every other module — knowledge graph projections, sync deltas, agent observations,
audit trail — reads from or writes to the event log. Building it correctly first
unblocks 100% of remaining work.

## Layering

```
EventBuilder ──▶ Event (sealed) ──▶ EventStore::append
                                          │
                                          ▼
            ┌───────────────────────────────────────────┐
            │  1. verify_content_hash()                 │
            │  2. exists()  → reject duplicate          │
            │  3. exists(prev_event_id)?                │
            │  4. exists(causal_deps)?                  │
            │  5. CAS::put(canonical(payload))          │
            │  6. CAS::put(canonical(event))            │
            │  7. SQLite INSERT                         │
            │  8. JsonlLogger::log_event                │
            │  9. Projection::apply for each registered │
            └───────────────────────────────────────────┘
```

## Canonical JSON

`canonical::to_canonical_bytes` is the **only** code path producing bytes that
get hashed. It:

- Sorts all object keys lexicographically (BTreeMap).
- Preserves array order (arrays are ordered).
- Uses compact serde_json output (no whitespace).

Any change to this function is a hard breaking change to all on-disk content
hashes. Treat it like a kernel ABI.

## Hash Chain (Git-like)

Each event optionally references `prev_event_id`. We do NOT enforce a single
chain per node — multiple branches are allowed (think Git). What we DO enforce
on append:

- If `prev_event_id` is set, that event MUST exist in the store.

`verify_all` walks the entire log and re-checks every link. This is O(n) — fine
for any single user's lifetime data.

## Causal Dependencies

`causal_deps` is an explicit list of event IDs this event logically depends on.
Used by sync to ensure remote events are applied in causally-correct order.

On append we require all deps exist. During sync (future work), `dt-sync` will
buffer events whose deps haven't arrived yet.

## Vector Clocks

Each event carries a `VectorClock` from `dt-sync`. The `EventBuilder` increments
the local node's counter at build time. Two events with concurrent vector clocks
indicate a sync conflict for downstream CRDTs to resolve.

## CAS-backed payload storage

Payloads are stored in the CAS keyed by SHA3-256 of canonical bytes. Two events
with **identical payloads** share a single CAS file. The SQLite row also stores
the payload JSON for fast queries — at the cost of duplication. We accept this
trade-off; a future optimization can make SQLite rows reference CAS hashes.

## JSONL Audit Trail

Every append writes one JSON line to `~/.dt/logs/events.jsonl`:

```json
{"ts":"2026-05-09T11:31:00Z","level":"info","target":"dt-event",
 "event_id":"01HQ...", "event_type":"knowledge.create",
 "node_id":"node-alpha",
 "content_hash":"927c1819...",
 "message":"appended"}
```

Tail-friendly, grep-friendly, ingestable by DuckDB or `jq`.

## Failure modes

| Condition | Behavior |
|-----------|----------|
| Unsealed event | `EventError::Invalid` — reject |
| Tampered event (hash mismatch) | `EventError::HashMismatch` — reject |
| Duplicate `event_id` | `EventError::DuplicateEvent` — reject |
| Missing `prev_event_id` | `EventError::HashChainBroken` — reject |
| Missing causal dep | `EventError::UnsatisfiedDependency` — reject |
| Projection failure | Log warning, do NOT roll back the append |

The last point is intentional: projections are **best-effort materialized views**.
The event log is the truth; projections can always be rebuilt by replay.

## Testing

- 33 unit tests cover canonical hashing, metadata builder, event seal/verify,
  projection idempotency, and store correctness.
- 4 integration tests cover end-to-end hash chains, replay rejection, on-disk
  tampering detection, and causal-dep enforcement.

## Open questions / future work

- **Signing.** `Event.signature` is a placeholder. We need to plug Ed25519 +
  the W3C DID key store before we ship multi-node sync.
- **Schema validation on append.** `payload_schema_hash` is recorded but the
  store doesn't currently look up the schema and validate. Wire `dt-schema` in.
- **Cold storage.** SQLite holds everything; we should periodically roll old
  events out to Parquet (DuckDB) for analytics.
- **Snapshotting.** Replaying 1B events is slow. Add periodic projection
  snapshots keyed by event_id.
