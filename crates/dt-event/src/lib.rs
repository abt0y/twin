//! # dt-event — Append-only Event Sourcing Engine
//!
//! Implements the **single source of truth** for the DT Platform per
//! Consolidated Architecture Spec v1.1.0.
//!
//! ## Core Invariants (Non-negotiable)
//! 1. **Append-only**: Events, once committed, are never modified or deleted.
//! 2. **Content-addressed**: Every event has a deterministic SHA3-256 hash
//!    over its canonical JSON representation. The hash IS the identity.
//! 3. **Hash-chain integrity**: Each event optionally references `prev_event_id`,
//!    forming a tamper-evident chain (Git-like).
//! 4. **Causal ordering**: Each event carries a hybrid vector clock and
//!    optional causal dependencies; commits respect causality.
//! 5. **Metadata envelope**: Every event carries a Universal Metadata Envelope
//!    with provenance, lineage, schema version, and ownership.
//! 6. **CAS-backed**: Event payloads are stored in the content-addressable
//!    store; events themselves are also CAS'd for immutable retrieval.
//!
//! ## Core Flow
//! ```text
//! Command → EventBuilder → Event → EventStore::append()
//!                                       │
//!                                       ├─→ canonical JSON
//!                                       ├─→ SHA3-256 hash
//!                                       ├─→ CAS::put(payload)
//!                                       ├─→ CAS::put(event)
//!                                       ├─→ SQLite::insert
//!                                       ├─→ JSONL log append
//!                                       └─→ Projection::apply()
//! ```

pub mod canonical;
pub mod error;
pub mod event;
pub mod metadata;
pub mod projection;
pub mod store;
pub mod telemetry;

pub use error::EventError;
pub use event::{Event, EventBuilder, EventType};
pub use metadata::{Lineage, MetadataEnvelope, MetadataBuilder};
pub use projection::{InMemoryProjection, Projection};
pub use store::{EventStore, EventStoreConfig};

/// Result alias for event operations.
pub type Result<T> = std::result::Result<T, EventError>;
