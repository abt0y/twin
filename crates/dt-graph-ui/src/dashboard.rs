//! Headless meta-cognition dashboard.
//!
//! Pure data-layer aggregations against a `KnowledgeRepository`. Used by
//! both the TUI and the CLI (`dt graph dashboard`).

use std::collections::BTreeMap;

use serde::Serialize;

use dt_knowledge::{KnowledgeError, KnowledgeRepository, NodeType};

/// Distribution of a numeric metric in 5 quantile buckets:
/// `[0.0–0.2), [0.2–0.4), [0.4–0.6), [0.6–0.8), [0.8–1.0]`.
#[derive(Debug, Clone, Serialize)]
pub struct ConfidenceDistribution {
    pub buckets: [u64; 5],
    pub none_count: u64,
    pub total: u64,
}

impl ConfidenceDistribution {
    pub fn empty() -> Self {
        Self {
            buckets: [0; 5],
            none_count: 0,
            total: 0,
        }
    }

    pub fn observe(&mut self, c: Option<f64>) {
        self.total += 1;
        match c {
            None => self.none_count += 1,
            Some(v) => {
                let idx = if v >= 0.8 {
                    4
                } else if v >= 0.6 {
                    3
                } else if v >= 0.4 {
                    2
                } else if v >= 0.2 {
                    1
                } else {
                    0
                };
                self.buckets[idx] += 1;
            }
        }
    }
}

/// Summary statistics for the meta-cognition dashboard.
#[derive(Debug, Clone, Serialize)]
pub struct DashboardStats {
    pub total_nodes: u64,
    pub total_meta_cognitive: u64,
    pub by_type: BTreeMap<String, u64>,
    pub confidence: ConfidenceDistribution,
    pub lean_pending: u64,
    pub lean_verified: u64,
    pub lean_failed: u64,
    pub open_questions: u64,
}

pub struct Dashboard<'a> {
    repo: &'a KnowledgeRepository,
}

impl<'a> Dashboard<'a> {
    pub fn new(repo: &'a KnowledgeRepository) -> Self {
        Self { repo }
    }

    /// Compute current dashboard statistics. `node_limit` caps the bulk-list
    /// scan (use `usize::MAX / 2` for "everything").
    pub fn compute(&self, node_limit: usize) -> Result<DashboardStats, KnowledgeError> {
        let nodes = self.repo.list(None, node_limit)?;

        let mut by_type: BTreeMap<String, u64> = BTreeMap::new();
        let mut conf = ConfidenceDistribution::empty();
        let mut total_meta_cognitive = 0u64;
        let mut open_q = 0u64;

        for n in &nodes {
            *by_type.entry(n.node_type.as_str().to_string()).or_insert(0) += 1;
            conf.observe(n.metadata.dt_confidence);
            if n.node_type.is_meta_cognitive() {
                total_meta_cognitive += 1;
            }
            if let Some(mc) = &n.meta_cognition {
                open_q += mc.open_questions.len() as u64;
            }
        }

        let lean_pending = self.repo.list_by_lean_status("pending", 100_000)?.len() as u64;
        let lean_verified = self.repo.list_by_lean_status("verified", 100_000)?.len() as u64;
        let lean_failed = self.repo.list_by_lean_status("failed", 100_000)?.len() as u64;

        Ok(DashboardStats {
            total_nodes: nodes.len() as u64,
            total_meta_cognitive,
            by_type,
            confidence: conf,
            lean_pending,
            lean_verified,
            lean_failed,
            open_questions: open_q,
        })
    }
}

/// Best-effort node-type filter that matches a substring against the typed
/// enum's `as_str()` representation. Returns `None` if no candidate matches.
pub fn parse_node_type_filter(s: &str) -> Option<NodeType> {
    let nt = NodeType::parse(s);
    // Reject the catch-all Custom result if it doesn't round-trip cleanly.
    if matches!(nt, NodeType::Custom(_)) {
        None
    } else {
        Some(nt)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confidence_distribution_buckets_correctly() {
        let mut d = ConfidenceDistribution::empty();
        d.observe(Some(0.05));
        d.observe(Some(0.25));
        d.observe(Some(0.55));
        d.observe(Some(0.75));
        d.observe(Some(0.95));
        d.observe(None);
        assert_eq!(d.total, 6);
        assert_eq!(d.none_count, 1);
        assert_eq!(d.buckets, [1, 1, 1, 1, 1]);
    }

    #[test]
    fn parse_node_type_filter_known() {
        assert_eq!(parse_node_type_filter("insight"), Some(NodeType::Insight));
        assert_eq!(parse_node_type_filter("note"), Some(NodeType::Note));
    }

    #[test]
    fn parse_node_type_filter_unknown_rejected() {
        assert_eq!(parse_node_type_filter("zorblax"), None);
    }
}
