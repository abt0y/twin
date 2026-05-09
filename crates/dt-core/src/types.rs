//! Core shared types: ContentHash, NodeId, EventId, etc.

use serde::{Deserialize, Serialize};

/// A validated content hash (SHA3-256 hex).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ContentHash(pub String);

impl ContentHash {
    pub fn new(s: String) -> Option<Self> {
        if s.len() == 64 && s.chars().all(|c| c.is_ascii_hexdigit()) {
            Some(ContentHash(s.to_lowercase()))
        } else {
            None
        }
    }
}

impl AsRef<str> for ContentHash {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

/// Node identity (ULID).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct NodeId(pub String);

/// Event identity (ULID).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EventId(pub String);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_content_hash_validation() {
        assert!(ContentHash::new("a".repeat(64)).is_some());
        assert!(ContentHash::new("g".repeat(64)).is_none());
        assert!(ContentHash::new("a".repeat(63)).is_none());
    }
}
