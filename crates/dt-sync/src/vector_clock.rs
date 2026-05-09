//! Hybrid vector clocks: map node_id -> logical counter + physical timestamp.

use std::collections::HashMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct VectorClock {
    pub node_id: String,
    pub counters: HashMap<String, u64>,
    pub timestamp: String,
    pub hybrid_lamport: Option<u64>,
}

impl VectorClock {
    pub fn new(node_id: String) -> Self {
        Self {
            node_id,
            counters: HashMap::new(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            hybrid_lamport: Some(0),
        }
    }

    /// Increment this node's counter.
    pub fn increment(&mut self) {
        let entry = self.counters.entry(self.node_id.clone()).or_insert(0);
        *entry += 1;
        if let Some(ref mut lamport) = self.hybrid_lamport {
            *lamport += 1;
        }
        self.timestamp = chrono::Utc::now().to_rfc3339();
    }

    /// Merge another vector clock into this one (per-key max).
    pub fn merge(&mut self, other: &VectorClock) {
        for (node, counter) in &other.counters {
            let entry = self.counters.entry(node.clone()).or_insert(0);
            *entry = (*entry).max(*counter);
        }
        if let (Some(a), Some(b)) = (self.hybrid_lamport, other.hybrid_lamport) {
            self.hybrid_lamport = Some(a.max(b) + 1);
        }
    }

    /// Compare two vector clocks.
    /// Returns `None` if concurrent (incomparable).
    pub fn compare(&self, other: &VectorClock) -> Option<std::cmp::Ordering> {
        let mut all_le = true;
        let mut all_ge = true;
        let all_keys: std::collections::HashSet<_> = self
            .counters
            .keys()
            .chain(other.counters.keys())
            .collect();

        for key in all_keys {
            let a = self.counters.get(key).copied().unwrap_or(0);
            let b = other.counters.get(key).copied().unwrap_or(0);
            if a > b {
                all_le = false;
            }
            if a < b {
                all_ge = false;
            }
        }

        match (all_le, all_ge) {
            (true, true) => Some(std::cmp::Ordering::Equal),
            (true, false) => Some(std::cmp::Ordering::Less),
            (false, true) => Some(std::cmp::Ordering::Greater),
            (false, false) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_increment() {
        let mut vc = VectorClock::new("node-a".into());
        vc.increment();
        assert_eq!(vc.counters.get("node-a"), Some(&1));
    }

    #[test]
    fn test_merge() {
        let mut a = VectorClock::new("node-a".into());
        a.increment();
        let mut b = VectorClock::new("node-b".into());
        b.increment();
        a.merge(&b);
        assert_eq!(a.counters.get("node-a"), Some(&1));
        assert_eq!(a.counters.get("node-b"), Some(&1));
    }

    #[test]
    fn test_compare() {
        let mut a = VectorClock::new("node-a".into());
        a.increment();
        let mut b = VectorClock::new("node-a".into());
        b.increment();
        b.increment();
        assert_eq!(a.compare(&b), Some(std::cmp::Ordering::Less));
    }
}
