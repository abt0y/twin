//! CRDT stubs: LWW-register and OR-set primitives.
//!
//! These are the building blocks for knowledge node CRDT merging.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Last-Write-Wins register with vector-clock metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LwwRegister<V> {
    pub value: V,
    pub clock: crate::vector_clock::VectorClock,
}

impl<V: Clone + PartialEq> LwwRegister<V> {
    /// Merge two registers: the one with the greater vector clock wins.
    /// If concurrent, use lexicographic node_id tiebreaker.
    pub fn merge(&self, other: &Self) -> Self {
        match self.clock.compare(&other.clock) {
            Some(std::cmp::Ordering::Less) => other.clone(),
            Some(std::cmp::Ordering::Greater) => self.clone(),
            Some(std::cmp::Ordering::Equal) => self.clone(),
            None => {
                // Concurrent: deterministic tiebreaker by node_id
                if self.clock.node_id <= other.clock.node_id {
                    self.clone()
                } else {
                    other.clone()
                }
            }
        }
    }
}

/// Observed-Remove Set (OR-Set) using dot kernels.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OrSet<T: std::hash::Hash + Eq + Clone> {
    elements: HashMap<T, crate::vector_clock::VectorClock>,
}

impl<T: std::hash::Hash + Eq + Clone> OrSet<T> {
    pub fn new() -> Self {
        Self {
            elements: HashMap::new(),
        }
    }

    pub fn add(&mut self, value: T, clock: crate::vector_clock::VectorClock) {
        self.elements.insert(value, clock);
    }

    pub fn remove(&mut self, value: &T) {
        self.elements.remove(value);
    }

    pub fn contains(&self, value: &T) -> bool {
        self.elements.contains_key(value)
    }

    pub fn merge(&mut self, other: &Self) {
        for (value, clock) in &other.elements {
            let entry = self.elements.entry(value.clone()).or_insert_with(|| clock.clone());
            entry.merge(clock);
        }
    }

    pub fn values(&self) -> Vec<&T> {
        self.elements.keys().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lww_register_merge() {
        let mut c1 = crate::vector_clock::VectorClock::new("a".into());
        c1.increment();
        let r1 = LwwRegister {
            value: "hello".to_string(),
            clock: c1.clone(),
        };

        let mut c2 = crate::vector_clock::VectorClock::new("b".into());
        c2.increment();
        c2.increment();
        let r2 = LwwRegister {
            value: "world".to_string(),
            clock: c2,
        };

        let merged = r1.merge(&r2);
        // Concurrent: deterministic tiebreaker by node_id ("a" < "b" => r1 wins)
        assert_eq!(merged.value, "hello");
    }

    #[test]
    fn test_or_set() {
        let mut set = OrSet::new();
        let clock = crate::vector_clock::VectorClock::new("a".into());
        let key = "x".to_string();
        set.add(key.clone(), clock.clone());
        assert!(set.contains(&key));
        set.remove(&key);
        assert!(!set.contains(&key));
    }
}
