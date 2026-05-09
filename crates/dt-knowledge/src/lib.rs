//! # dt-knowledge — Knowledge Graph API
//!
//! Implements the user-facing knowledge graph as a **projection** over the
//! append-only event log (`dt-event`).
//!
//! ## Architecture
//!
//! ```text
//!     User code / CLI / Agent
//!              │
//!              ▼
//!     ┌─────────────────────┐         ┌─────────────────────┐
//!     │ KnowledgeService    │  emits  │ EventStore (append) │
//!     │  (write API)        │────────▶│  • single source    │
//!     └─────────────────────┘         │    of truth         │
//!              │                      └──────────┬──────────┘
//!              │                                 │
//!              ▼                                 ▼
//!     ┌─────────────────────┐         ┌─────────────────────┐
//!     │ KnowledgeRepository │ reads   │ KnowledgeProjection │
//!     │  (read API + FTS)   │◀────────│ (materialized view) │
//!     └─────────────────────┘         └─────────────────────┘
//! ```
//!
//! All writes go through `KnowledgeService`, which builds a domain event,
//! seals it, and appends it to the `EventStore`. The `KnowledgeProjection`
//! (registered with the store) updates the SQLite materialized view.
//! Reads use `KnowledgeRepository` directly against the materialized tables.

pub mod db;
pub mod edge;
pub mod error;
pub mod node;
pub mod projection;
pub mod repository;
pub mod service;

pub use db::KnowledgeDb;

pub use edge::{KnowledgeEdge, Relation};
pub use error::KnowledgeError;
pub use node::{KnowledgeNode, NodeContent, NodeStatus, NodeType, Visibility};
pub use projection::KnowledgeProjection;
pub use repository::{KnowledgeRepository, NeighborDirection};
pub use service::KnowledgeService;

/// Result alias.
pub type Result<T> = std::result::Result<T, KnowledgeError>;
