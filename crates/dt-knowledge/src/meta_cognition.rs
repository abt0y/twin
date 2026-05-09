//! Meta-cognition extensions for `KnowledgeNode`.
//!
//! These types capture *how* the twin is thinking — confidence, certainty
//! mode, the reasoning trace that produced a claim, the assumptions it rests
//! on, and counter-arguments that might unravel it.
//!
//! The data lives in dedicated SQLite columns (added by `KnowledgeProjection`
//! on first run) so the read path stays cheap, but every change still flows
//! through an append-only event for full auditability and replay.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// How a claim is justified.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Hash)]
#[serde(rename_all = "snake_case")]
pub enum CertaintyType {
    /// Rule of thumb / pattern-matched intuition.
    Heuristic,
    /// Backed by a Lean 4 (or other) formal proof.
    Proof,
    /// Backed by statistical / empirical evidence.
    Statistical,
    /// Anecdotal — single-instance evidence.
    Anecdotal,
    /// Direct testimony from a trusted source.
    Testimonial,
    /// Author cannot say.
    Unknown,
}

impl CertaintyType {
    pub fn as_str(&self) -> &str {
        match self {
            CertaintyType::Heuristic => "heuristic",
            CertaintyType::Proof => "proof",
            CertaintyType::Statistical => "statistical",
            CertaintyType::Anecdotal => "anecdotal",
            CertaintyType::Testimonial => "testimonial",
            CertaintyType::Unknown => "unknown",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "heuristic" => CertaintyType::Heuristic,
            "proof" => CertaintyType::Proof,
            "statistical" => CertaintyType::Statistical,
            "anecdotal" => CertaintyType::Anecdotal,
            "testimonial" => CertaintyType::Testimonial,
            _ => CertaintyType::Unknown,
        }
    }
}

impl Default for CertaintyType {
    fn default() -> Self {
        CertaintyType::Unknown
    }
}

/// Single step in a thinking trace (chain-of-thought entry).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ThinkingStep {
    pub at: DateTime<Utc>,
    /// Free-form thought / inference step.
    pub thought: String,
    /// Optional reference to an existing node that this step relies on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference_node_id: Option<String>,
}

impl ThinkingStep {
    pub fn now(thought: impl Into<String>) -> Self {
        Self {
            at: Utc::now(),
            thought: thought.into(),
            reference_node_id: None,
        }
    }
}

/// Rich meta-cognitive annotation for a knowledge node.
///
/// Stored in the `meta_cognition_json` SQLite column. Always optional — most
/// nodes (a plain note, a task) do not need this.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct MetaCognition {
    /// How is this claim justified?
    #[serde(default)]
    pub certainty_type: CertaintyType,

    /// Chronological reasoning trace.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub thinking_trace: Vec<ThinkingStep>,

    /// Assumptions this claim depends on.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub assumptions: Vec<String>,

    /// Known counter-arguments (steel-man your own thinking).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub counter_arguments: Vec<String>,

    /// How many inferential hops this claim sits from raw evidence.
    /// `0` = direct observation. Higher = more derived.
    #[serde(default)]
    pub derivation_depth: u32,

    /// Free-form open questions around this claim.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub open_questions: Vec<String>,
}

impl MetaCognition {
    /// New, empty meta-cognition envelope.
    pub fn new() -> Self {
        Self::default()
    }

    /// Builder: append a thinking step.
    pub fn with_thinking_step(mut self, step: ThinkingStep) -> Self {
        self.thinking_trace.push(step);
        self
    }

    /// Builder: add an assumption.
    pub fn with_assumption(mut self, a: impl Into<String>) -> Self {
        self.assumptions.push(a.into());
        self
    }

    /// Builder: add a counter-argument.
    pub fn with_counter_argument(mut self, c: impl Into<String>) -> Self {
        self.counter_arguments.push(c.into());
        self
    }

    /// Builder: add an open question.
    pub fn with_open_question(mut self, q: impl Into<String>) -> Self {
        self.open_questions.push(q.into());
        self
    }

    /// Builder: set certainty type.
    pub fn with_certainty(mut self, c: CertaintyType) -> Self {
        self.certainty_type = c;
        self
    }

    /// Builder: set derivation depth.
    pub fn with_derivation_depth(mut self, d: u32) -> Self {
        self.derivation_depth = d;
        self
    }

    /// True if this is effectively empty (default).
    pub fn is_empty(&self) -> bool {
        self.certainty_type == CertaintyType::Unknown
            && self.thinking_trace.is_empty()
            && self.assumptions.is_empty()
            && self.counter_arguments.is_empty()
            && self.derivation_depth == 0
            && self.open_questions.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_is_empty() {
        assert!(MetaCognition::default().is_empty());
    }

    #[test]
    fn test_builder() {
        let m = MetaCognition::new()
            .with_certainty(CertaintyType::Heuristic)
            .with_assumption("users want speed")
            .with_counter_argument("they may want correctness more")
            .with_thinking_step(ThinkingStep::now("observed slow page loads"))
            .with_open_question("is latency the bottleneck?")
            .with_derivation_depth(2);
        assert_eq!(m.certainty_type, CertaintyType::Heuristic);
        assert_eq!(m.assumptions.len(), 1);
        assert_eq!(m.counter_arguments.len(), 1);
        assert_eq!(m.thinking_trace.len(), 1);
        assert_eq!(m.open_questions.len(), 1);
        assert_eq!(m.derivation_depth, 2);
        assert!(!m.is_empty());
    }

    #[test]
    fn test_certainty_roundtrip() {
        for c in &[
            CertaintyType::Heuristic,
            CertaintyType::Proof,
            CertaintyType::Statistical,
            CertaintyType::Anecdotal,
            CertaintyType::Testimonial,
            CertaintyType::Unknown,
        ] {
            let s = c.as_str().to_string();
            assert_eq!(CertaintyType::parse(&s), *c);
        }
    }

    #[test]
    fn test_serde_roundtrip() {
        let m = MetaCognition::new()
            .with_certainty(CertaintyType::Proof)
            .with_assumption("ZFC");
        let s = serde_json::to_string(&m).unwrap();
        let m2: MetaCognition = serde_json::from_str(&s).unwrap();
        assert_eq!(m, m2);
    }
}
