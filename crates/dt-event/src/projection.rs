//! Projections: materialized views over the event log.
//!
//! Per spec: "State = materialized view of immutable events." A `Projection`
//! receives every committed event and updates its own internal view.
//! Projections must be **idempotent** (replaying events produces the same view).

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use crate::error::EventError;
use crate::event::Event;

/// Implement this trait to build a custom materialized view.
pub trait Projection: Send + Sync {
    /// Apply a single event. MUST be idempotent on event_id.
    fn apply(&self, event: &Event) -> Result<(), EventError>;

    /// Stable name (for logging/debugging).
    fn name(&self) -> &str;
}

/// Simple in-memory projection: counts events per type and tracks last event id.
///
/// Useful for testing, dashboards, and as a reference implementation.
#[derive(Debug, Default)]
pub struct InMemoryProjection {
    name: String,
    counts: Arc<RwLock<HashMap<String, u64>>>,
    last_event_id: Arc<RwLock<Option<String>>>,
    seen: Arc<RwLock<std::collections::HashSet<String>>>,
}

impl InMemoryProjection {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            counts: Default::default(),
            last_event_id: Default::default(),
            seen: Default::default(),
        }
    }

    pub fn count_for(&self, event_type: &str) -> u64 {
        self.counts
            .read()
            .ok()
            .and_then(|c| c.get(event_type).copied())
            .unwrap_or(0)
    }

    pub fn last_event_id(&self) -> Option<String> {
        self.last_event_id.read().ok().and_then(|g| g.clone())
    }

    pub fn total_events(&self) -> u64 {
        self.counts.read().ok().map(|c| c.values().sum()).unwrap_or(0)
    }
}

impl Projection for InMemoryProjection {
    fn apply(&self, event: &Event) -> Result<(), EventError> {
        // Idempotency: skip if event_id already seen
        {
            let mut seen = self
                .seen
                .write()
                .map_err(|e| EventError::Storage(format!("projection seen poisoned: {}", e)))?;
            if !seen.insert(event.event_id.clone()) {
                return Ok(());
            }
        }
        {
            let mut counts = self
                .counts
                .write()
                .map_err(|e| EventError::Storage(format!("projection counts poisoned: {}", e)))?;
            *counts.entry(event.event_type.to_string()).or_insert(0) += 1;
        }
        {
            let mut last = self
                .last_event_id
                .write()
                .map_err(|e| EventError::Storage(format!("projection last poisoned: {}", e)))?;
            *last = Some(event.event_id.clone());
        }
        Ok(())
    }

    fn name(&self) -> &str {
        &self.name
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{EventBuilder, EventType};

    #[test]
    fn test_projection_counts() {
        let proj = InMemoryProjection::new("test");
        for _ in 0..3 {
            let ev = EventBuilder::new(EventType::KnowledgeCreate, "n1", "did:u")
                .build()
                .unwrap();
            proj.apply(&ev).unwrap();
        }
        let ev = EventBuilder::new(EventType::KnowledgeUpdate, "n1", "did:u")
            .build()
            .unwrap();
        proj.apply(&ev).unwrap();

        assert_eq!(proj.count_for("knowledge.create"), 3);
        assert_eq!(proj.count_for("knowledge.update"), 1);
        assert_eq!(proj.total_events(), 4);
    }

    #[test]
    fn test_projection_idempotent() {
        let proj = InMemoryProjection::new("test");
        let ev = EventBuilder::new(EventType::KnowledgeCreate, "n1", "did:u")
            .build()
            .unwrap();
        proj.apply(&ev).unwrap();
        proj.apply(&ev).unwrap();
        proj.apply(&ev).unwrap();
        assert_eq!(proj.total_events(), 1);
    }
}
