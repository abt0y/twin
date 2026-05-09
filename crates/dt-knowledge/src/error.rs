//! Errors for the knowledge graph layer.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum KnowledgeError {
    #[error("knowledge node not found: {0}")]
    NodeNotFound(String),

    #[error("knowledge edge not found: {0}")]
    EdgeNotFound(String),

    #[error("invalid input: {0}")]
    Invalid(String),

    #[error("conflict (CRDT): {0}")]
    Conflict(String),

    #[error("storage error: {0}")]
    Storage(String),

    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),

    #[error("event error: {0}")]
    Event(#[from] dt_event::EventError),

    #[error("dt-core error: {0}")]
    Core(#[from] dt_core::DTError),
}
