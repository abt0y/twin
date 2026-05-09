//! Export the knowledge graph into formats consumable by external tools.
//!
//! Supports:
//! - **Mermaid** (`graph LR`) — render in any Markdown viewer.
//! - **Graphviz DOT** — pipe through `dot -Tsvg`.
//! - **JSON** — `{ nodes: [...], edges: [...] }` for the future web UI.

use serde::Serialize;

use crate::edge::KnowledgeEdge;
use crate::error::KnowledgeError;
use crate::node::KnowledgeNode;
use crate::repository::{KnowledgeRepository, NeighborDirection};

/// Snapshot of the graph as an exportable scene.
#[derive(Debug, Clone, Serialize)]
pub struct GraphScene {
    pub nodes: Vec<KnowledgeNode>,
    pub edges: Vec<KnowledgeEdge>,
}

impl GraphScene {
    /// Build a scene by walking from `root_id` up to `depth` hops.
    pub fn from_walk(
        repo: &KnowledgeRepository,
        root_id: &str,
        depth: usize,
        direction: NeighborDirection,
    ) -> Result<Self, KnowledgeError> {
        let nodes = repo.walk(root_id, depth, direction)?;
        let mut edges = Vec::new();
        for n in &nodes {
            for e in repo.neighbors(&n.node_id, NeighborDirection::Outgoing, None, 256)? {
                // include only edges between scene nodes
                if nodes.iter().any(|x| x.node_id == e.target_id) {
                    edges.push(e);
                }
            }
        }
        Ok(Self { nodes, edges })
    }

    /// Build a scene from the most-recent N nodes in the repository.
    pub fn from_latest(
        repo: &KnowledgeRepository,
        limit: usize,
    ) -> Result<Self, KnowledgeError> {
        let nodes = repo.list(None, limit)?;
        let alive: std::collections::HashSet<String> =
            nodes.iter().map(|n| n.node_id.clone()).collect();
        let mut edges = Vec::new();
        for n in &nodes {
            for e in repo.neighbors(&n.node_id, NeighborDirection::Outgoing, None, 256)? {
                if alive.contains(&e.target_id) {
                    edges.push(e);
                }
            }
        }
        Ok(Self { nodes, edges })
    }

    /// Export as Mermaid `graph LR`.
    pub fn to_mermaid(&self) -> String {
        let mut out = String::from("graph LR\n");
        for n in &self.nodes {
            let label = mermaid_escape(&n.content.title);
            let badge = node_badge(n);
            out.push_str(&format!(
                "    {}[\"{}{}\"]\n",
                mermaid_id(&n.node_id),
                label,
                badge
            ));
        }
        for e in &self.edges {
            out.push_str(&format!(
                "    {} -- {} --> {}\n",
                mermaid_id(&e.source_id),
                mermaid_escape(e.relation.as_str()),
                mermaid_id(&e.target_id),
            ));
        }
        out
    }

    /// Export as Graphviz DOT.
    pub fn to_dot(&self) -> String {
        let mut out = String::from("digraph dt_knowledge {\n  rankdir=LR;\n  node [shape=box,fontname=\"Helvetica\"];\n");
        for n in &self.nodes {
            let title = dot_escape(&n.content.title);
            let kind = n.node_type.as_str();
            let conf = n
                .metadata
                .dt_confidence
                .map(|c| format!("\\nconf={:.2}", c))
                .unwrap_or_default();
            let lean = n
                .lean
                .as_ref()
                .map(|l| format!("\\nlean={}", l.lean_proof_status.as_str()))
                .unwrap_or_default();
            out.push_str(&format!(
                "  \"{id}\" [label=\"{title}\\n[{kind}]{conf}{lean}\"];\n",
                id = n.node_id,
                title = title,
                kind = kind,
                conf = conf,
                lean = lean,
            ));
        }
        for e in &self.edges {
            out.push_str(&format!(
                "  \"{}\" -> \"{}\" [label=\"{}\"];\n",
                e.source_id,
                e.target_id,
                dot_escape(e.relation.as_str())
            ));
        }
        out.push_str("}\n");
        out
    }

    /// Export as compact JSON.
    pub fn to_json(&self) -> Result<String, KnowledgeError> {
        Ok(serde_json::to_string(self)?)
    }
}

fn node_badge(n: &KnowledgeNode) -> String {
    let mut parts = Vec::new();
    parts.push(format!(" [{}]", n.node_type.as_str()));
    if let Some(c) = n.metadata.dt_confidence {
        parts.push(format!(" c={:.2}", c));
    }
    if let Some(lean) = &n.lean {
        parts.push(format!(" ⊢{}", lean.lean_proof_status.as_str()));
    }
    parts.join("")
}

fn mermaid_id(s: &str) -> String {
    // Mermaid node ids must be alnum-friendly; ULIDs already are.
    s.replace('-', "_")
}

fn mermaid_escape(s: &str) -> String {
    s.replace('"', "'")
}

fn dot_escape(s: &str) -> String {
    s.replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::{NodeContent, NodeType};

    #[test]
    fn empty_scene_renders() {
        let scene = GraphScene {
            nodes: vec![],
            edges: vec![],
        };
        assert!(scene.to_mermaid().starts_with("graph LR"));
        assert!(scene.to_dot().contains("digraph"));
        assert_eq!(scene.to_json().unwrap(), "{\"nodes\":[],\"edges\":[]}");
    }

    #[test]
    fn single_node_renders() {
        let n = KnowledgeNode::new(
            NodeType::Insight,
            NodeContent::new("Async is fast", "for IO"),
            "u",
        );
        let scene = GraphScene {
            nodes: vec![n.clone()],
            edges: vec![],
        };
        let m = scene.to_mermaid();
        assert!(m.contains(&n.node_id.replace('-', "_")));
        assert!(m.contains("Async is fast"));
        let d = scene.to_dot();
        assert!(d.contains("[insight]"));
    }
}
