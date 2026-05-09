//! dt-core: Core types, content-addressable storage (CAS), crypto primitives,
//! and shared utilities for the DT Platform.

use std::path::PathBuf;

pub mod cas;
pub mod crypto;
pub mod id;
pub mod types;

/// Result type used across dt crates.
pub type Result<T> = std::result::Result<T, DTError>;

/// Top-level error type.
#[derive(Debug, thiserror::Error)]
pub enum DTError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("crypto error: {0}")]
    Crypto(String),
    #[error("CAS error: {0}")]
    Cas(String),
    #[error("invalid id: {0}")]
    InvalidId(String),
    #[error("general: {0}")]
    General(String),
}

/// Deterministic path resolution for the DT data directory.
pub fn resolve_dt_dir() -> PathBuf {
    dirs::home_dir()
        .expect("home directory must exist")
        .join(".dt")
}

/// Compute SHA3-256 digest of bytes, returning lowercase hex.
pub fn sha3_256_hex(data: &[u8]) -> String {
    use sha3::{Digest, Sha3_256};
    let mut hasher = Sha3_256::new();
    hasher.update(data);
    hex::encode(hasher.finalize())
}

/// Compute blake3 digest of bytes, returning lowercase hex.
pub fn blake3_hex(data: &[u8]) -> String {
    blake3::hash(data).to_hex().to_string()
}

/// Tracing helpers for structured JSONL logs.
pub mod telemetry {
    use serde::Serialize;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[derive(Serialize)]
    pub struct StructuredLog<'a> {
        pub ts: u64,
        pub level: &'a str,
        pub target: &'a str,
        pub message: &'a str,
        #[serde(flatten)]
        pub extra: Option<serde_json::Value>,
    }

    pub fn now_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
    }

    pub fn jsonl<'a>(level: &'a str, target: &'a str, message: &'a str) {
        let entry = StructuredLog {
            ts: now_ms(),
            level,
            target,
            message,
            extra: None,
        };
        if let Ok(s) = serde_json::to_string(&entry) {
            eprintln!("{}", s);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sha3_256_hex() {
        let h = sha3_256_hex(b"hello");
        assert_eq!(h.len(), 64);
    }

    #[test]
    fn test_blake3_hex() {
        let h = blake3_hex(b"hello");
        assert_eq!(h.len(), 64);
    }
}
