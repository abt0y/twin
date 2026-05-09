//! Domain-specific errors for event sourcing operations.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum EventError {
    #[error("event with id {0} already exists (append-only violation)")]
    DuplicateEvent(String),

    #[error("hash chain broken: prev_event_id {prev:?} not found in store")]
    HashChainBroken { prev: Option<String> },

    #[error("content hash mismatch: expected {expected}, got {actual}")]
    HashMismatch { expected: String, actual: String },

    #[error("causal dependency not satisfied: missing event {0}")]
    UnsatisfiedDependency(String),

    #[error("invalid event: {0}")]
    Invalid(String),

    #[error("validation failed: {0}")]
    ValidationFailed(String),

    #[error("storage error: {0}")]
    Storage(String),

    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),

    #[error("dt-core error: {0}")]
    Core(#[from] dt_core::DTError),
}
