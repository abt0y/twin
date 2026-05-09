//! Multi-hop reasoning + cognitive graph queries.
//!
//! Built on top of [`KnowledgeRepository`]; pure read-only logic. Used by the
//! Discovery Engine and the meta-cognition dashboard to surface evidence
//! chains, contradictions, and consistency violations.

use std::collections::{HashMap, HashSet, VecDeque};

use serde::{Deserialize, Serialize};

use crate::edge::Relation;
use crate::error::KnowledgeError;
use crate::node::{KnowledgeNode, NodeType};
use crate::repository::{KnowledgeRepository, NeighborDirection};

/// A path of nodes representing a reasoning chain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceChain {
    /// Nodes in chain order (root → leaf).
    pub nodes: Vec<KnowledgeNode>,
    /// Total chain length (number of edges).
    pub depth: usize,
    /// Minimum confidence along the chain (None if any node has no confidence).
    pub min_confidence: Option<f64>,
}

/// A pair of nodes whose claims appear to contradict each other.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContradictionReport {
    pub a: KnowledgeNode,
    pub b: KnowledgeNode,
    pub reason: String,
}

/// Read-only graph reasoning engine.
pub struct ReasoningEngine<'a> {
    repo: &'a KnowledgeRepository,
}

impl<'a> ReasoningEngine<'a> {
    pub fn new(repo: &'a KnowledgeRepository) -> Self {
        Self { repo }
    }

    /// Find all reasoning paths (BFS) from `start_id` to `target_id` up to
    /// `max_depth` hops, traversing in the given direction. Returns paths
    /// ordered shortest first.
    pub fn reason_path(
        &self,
        start_id: &str,
        target_id: &str,
        max_depth: usize,
        direction: NeighborDirection,
    ) -> Result<Vec<EvidenceChain>, KnowledgeError> {
        let mut paths: Vec<EvidenceChain> = Vec::new();
        if start_id == target_id {
            if let Some(n) = self.repo.get(start_id)? {
                let conf = n.metadata.dt_confidence;
                paths.push(EvidenceChain {
                    nodes: vec![n],
                    depth: 0,
                    min_confidence: conf,
                });
            }
            return Ok(paths);
        }

        // BFS over node ids; track parent pointers per discovered path.
        let mut queue: VecDeque<Vec<String>> = VecDeque::new();
        queue.push_back(vec![start_id.to_string()]);

        while let Some(prefix) = queue.pop_front() {
            if prefix.len() > max_depth + 1 {
                continue;
            }
            let last = prefix.last().unwrap().clone();
            if last == target_id && prefix.len() > 1 {
                paths.push(self.materialize_chain(&prefix)?);
                continue;
            }
            if prefix.len() > max_depth {
                continue;
            }
            let edges = self.repo.neighbors(&last, direction, None, 256)?;
            for e in edges {
                let next = match direction {
                    NeighborDirection::Outgoing => e.target_id,
                    NeighborDirection::Incoming => e.source_id,
                    NeighborDirection::Both => {
                        if e.source_id == last {
                            e.target_id
                        } else {
                            e.source_id
                        }
                    }
                };
                if prefix.contains(&next) {
                    continue; // avoid cycles
                }
                let mut extended = prefix.clone();
                extended.push(next);
                queue.push_back(extended);
            }
        }

        // Sort by depth (shortest first), then by min_confidence desc.
        paths.sort_by(|a, b| {
            a.depth.cmp(&b.depth).then_with(|| {
                b.min_confidence
                    .partial_cmp(&a.min_confidence)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
        });
        Ok(paths)
    }

    /// Find all evidence chains supporting `node_id` — i.e. paths *into* the
    /// node from any node of type `Evidence`. Useful for surfacing why a
    /// hypothesis or insight is believed.
    pub fn find_evidence_chains(
        &self,
        node_id: &str,
        max_depth: usize,
    ) -> Result<Vec<EvidenceChain>, KnowledgeError> {
        // BFS backwards (incoming edges) and collect chains that originate at
        // any Evidence node.
        let mut chains: Vec<EvidenceChain> = Vec::new();
        let mut queue: VecDeque<Vec<String>> = VecDeque::new();
        queue.push_back(vec![node_id.to_string()]);

        while let Some(prefix) = queue.pop_front() {
            if prefix.len() > max_depth + 1 {
                continue;
            }
            let head = prefix.last().unwrap().clone();
            if let Some(n) = self.repo.get(&head)? {
                if matches!(n.node_type, NodeType::Evidence) && head != node_id {
                    let mut nodes_chain = prefix.clone();
                    nodes_chain.reverse(); // root (evidence) → target
                    chains.push(self.materialize_chain(&nodes_chain)?);
                    continue;
                }
            }
            if prefix.len() > max_depth {
                continue;
            }
            let edges = self.repo.neighbors(&head, NeighborDirection::Incoming, None, 256)?;
            for e in edges {
                if prefix.contains(&e.source_id) {
                    continue;
                }
                let mut extended = prefix.clone();
                extended.push(e.source_id.clone());
                queue.push_back(extended);
            }
        }
        chains.sort_by_key(|c| c.depth);
        Ok(chains)
    }

    /// Find all *cognitive* nodes (reflection / hypothesis / insight / pattern)
    /// reachable from `node_id` within `depth` hops.
    pub fn cognitive_neighborhood(
        &self,
        node_id: &str,
        depth: usize,
    ) -> Result<Vec<KnowledgeNode>, KnowledgeError> {
        let walked = self
            .repo
            .walk(node_id, depth, NeighborDirection::Both)?;
        let filtered = walked
            .into_iter()
            .filter(|n| n.node_type.is_meta_cognitive())
            .collect();
        Ok(filtered)
    }

    /// Detect basic contradictions: pairs of nodes that
    /// (a) share at least one tag,
    /// (b) are connected by a `contradicts` / `Custom("contradicts")` edge,
    ///     OR
    /// (c) have meta-cognition counter-arguments referencing each other's
    ///     `node_id`.
    ///
    /// Returns one report per offending pair.
    pub fn detect_contradictions(
        &self,
        limit: usize,
    ) -> Result<Vec<ContradictionReport>, KnowledgeError> {
        // Use the bulk-list of all live nodes (capped) for case (c) and (a),
        // and rely on graph edges for case (b).
        let nodes = self.repo.list(None, limit.max(50))?;
        let nodes_by_id: HashMap<String, KnowledgeNode> = nodes
            .iter()
            .map(|n| (n.node_id.clone(), n.clone()))
            .collect();
        let mut reports: Vec<ContradictionReport> = Vec::new();
        let mut seen_pairs: HashSet<(String, String)> = HashSet::new();

        // (b) explicit contradicts edges
        for n in &nodes {
            for direction in [NeighborDirection::Outgoing, NeighborDirection::Incoming] {
                let edges = self.repo.neighbors(&n.node_id, direction, None, 64)?;
                for e in edges {
                    let is_contradicts = matches!(&e.relation,
                        Relation::Custom(s) if s == "contradicts");
                    if !is_contradicts {
                        continue;
                    }
                    let (a_id, b_id) = if e.source_id == n.node_id {
                        (e.source_id.clone(), e.target_id.clone())
                    } else {
                        (e.target_id.clone(), e.source_id.clone())
                    };
                    let key = ordered_pair(&a_id, &b_id);
                    if seen_pairs.insert(key.clone()) {
                        if let (Some(a), Some(b)) =
                            (nodes_by_id.get(&a_id), nodes_by_id.get(&b_id))
                        {
                            reports.push(ContradictionReport {
                                a: a.clone(),
                                b: b.clone(),
                                reason: format!(
                                    "explicit contradicts edge ({})",
                                    e.edge_id
                                ),
                            });
                            if reports.len() >= limit {
                                return Ok(reports);
                            }
                        }
                    }
                }
            }
        }

        // (c) meta-cognition counter-arguments referencing other node_ids
        for n in &nodes {
            if let Some(mc) = &n.meta_cognition {
                for ca in &mc.counter_arguments {
                    for other in &nodes {
                        if other.node_id == n.node_id {
                            continue;
                        }
                        if ca.contains(&other.node_id) {
                            let key = ordered_pair(&n.node_id, &other.node_id);
                            if seen_pairs.insert(key) {
                                reports.push(ContradictionReport {
                                    a: n.clone(),
                                    b: other.clone(),
                                    reason: format!(
                                        "counter-argument on '{}' references '{}'",
                                        n.node_id, other.node_id
                                    ),
                                });
                                if reports.len() >= limit {
                                    return Ok(reports);
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(reports)
    }

    /// Validate basic graph consistency. Returns the list of issues; empty
    /// vector means the graph is healthy.
    pub fn validate_consistency(&self) -> Result<Vec<String>, KnowledgeError> {
        let mut issues = Vec::new();

        // Issue 1: dangling edges — edges referencing missing or deleted nodes.
        let nodes = self.repo.list(None, 10_000)?;
        let alive: HashSet<String> = nodes.iter().map(|n| n.node_id.clone()).collect();

        for n in &nodes {
            for e in self
                .repo
                .neighbors(&n.node_id, NeighborDirection::Outgoing, None, 1024)?
            {
                if !alive.contains(&e.target_id) {
                    issues.push(format!(
                        "edge {} points to missing/deleted node {}",
                        e.edge_id, e.target_id
                    ));
                }
            }
        }

        // Issue 2: theorems whose Lean status is `Verified` but
        // `verified_by_lean = false` (or vice versa).
        for n in &nodes {
            if let Some(lean) = &n.lean {
                let claims_verified = lean.verified_by_lean;
                let status_verified =
                    lean.lean_proof_status == crate::lean::LeanProofStatus::Verified;
                if claims_verified != status_verified {
                    issues.push(format!(
                        "theorem {} has inconsistent lean fields (verified_by_lean={}, status={})",
                        n.node_id,
                        claims_verified,
                        lean.lean_proof_status.as_str()
                    ));
                }
            }
        }

        // Issue 3: meta-cognition with derivation_depth > 0 but no incoming
        // edges (claim to be derived from nothing).
        for n in &nodes {
            if let Some(mc) = &n.meta_cognition {
                if mc.derivation_depth > 0 {
                    let inc = self
                        .repo
                        .neighbors(&n.node_id, NeighborDirection::Incoming, None, 1)?;
                    if inc.is_empty() {
                        issues.push(format!(
                            "node {} declares derivation_depth={} but has no incoming edges",
                            n.node_id, mc.derivation_depth
                        ));
                    }
                }
            }
        }

        Ok(issues)
    }

    fn materialize_chain(&self, ids: &[String]) -> Result<EvidenceChain, KnowledgeError> {
        let mut nodes = Vec::with_capacity(ids.len());
        let mut min_conf: Option<f64> = None;
        for id in ids {
            if let Some(n) = self.repo.get(id)? {
                if let Some(c) = n.metadata.dt_confidence {
                    min_conf = Some(min_conf.map(|m| m.min(c)).unwrap_or(c));
                }
                nodes.push(n);
            }
        }
        let depth = nodes.len().saturating_sub(1);
        Ok(EvidenceChain {
            nodes,
            depth,
            min_confidence: min_conf,
        })
    }
}

fn ordered_pair(a: &str, b: &str) -> (String, String) {
    if a < b {
        (a.to_string(), b.to_string())
    } else {
        (b.to_string(), a.to_string())
    }
}
