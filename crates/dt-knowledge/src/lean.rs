//! Lean 4 verification hooks for the knowledge graph.
//!
//! ## Design
//!
//! Theorems live as `KnowledgeNode { node_type: NodeType::Theorem, ... }`.
//! The Lean source code (`.lean` text) is content-addressed in CAS, with the
//! resulting hash stored on the node (`lean_theorem_hash`). Verification is
//! performed by an implementation of [`LeanVerifier`] — either the in-process
//! [`StubLeanVerifier`] (for tests / offline mode) or [`ExternalLeanVerifier`]
//! (which shells out to the `lean` binary).
//!
//! ## Event emission
//!
//! After a verification attempt, the caller emits:
//! - `knowledge.lean.verified` if proof check succeeded
//! - `knowledge.lean.failed`   if the proof was rejected
//!
//! These events update the `lean_proof_status` column of the theorem node
//! through `KnowledgeProjection`.

use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::KnowledgeError;

/// Status of a Lean 4 proof attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
#[serde(rename_all = "snake_case")]
pub enum LeanProofStatus {
    /// No verification attempted yet.
    Unknown,
    /// Submitted to a verifier; waiting for a verdict.
    Pending,
    /// Lean accepted the proof.
    Verified,
    /// Lean rejected the proof.
    Failed,
}

impl LeanProofStatus {
    pub fn as_str(&self) -> &str {
        match self {
            LeanProofStatus::Unknown => "unknown",
            LeanProofStatus::Pending => "pending",
            LeanProofStatus::Verified => "verified",
            LeanProofStatus::Failed => "failed",
        }
    }
    pub fn parse(s: &str) -> Self {
        match s {
            "pending" => LeanProofStatus::Pending,
            "verified" => LeanProofStatus::Verified,
            "failed" => LeanProofStatus::Failed,
            _ => LeanProofStatus::Unknown,
        }
    }
}

impl Default for LeanProofStatus {
    fn default() -> Self {
        LeanProofStatus::Unknown
    }
}

/// Verification metadata stored on a theorem-bearing `KnowledgeNode`.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct LeanVerification {
    /// Convenience boolean — true iff `lean_proof_status == Verified`.
    #[serde(default)]
    pub verified_by_lean: bool,
    /// SHA3-256 of the `.lean` source bytes stored in CAS.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lean_theorem_hash: Option<String>,
    /// SHA3-256 of an optional `.olean` proof artifact.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lean_proof_hash: Option<String>,
    #[serde(default)]
    pub lean_proof_status: LeanProofStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verifier_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verified_at: Option<DateTime<Utc>>,
    /// Diagnostics from the last verification attempt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

impl LeanVerification {
    pub fn pending(theorem_hash: impl Into<String>) -> Self {
        Self {
            verified_by_lean: false,
            lean_theorem_hash: Some(theorem_hash.into()),
            lean_proof_hash: None,
            lean_proof_status: LeanProofStatus::Pending,
            verifier_version: None,
            verified_at: None,
            last_error: None,
        }
    }
}

/// Verdict from a verifier run.
#[derive(Debug, Clone, PartialEq)]
pub struct LeanVerdict {
    pub status: LeanProofStatus,
    pub verifier_version: String,
    /// Optional `.olean` artifact bytes (CAS-stored by caller).
    pub proof_artifact: Option<Vec<u8>>,
    pub diagnostics: Option<String>,
}

/// Pluggable verifier interface — the same trait is used by tests and by
/// the production binary; only the `verify` implementation differs.
pub trait LeanVerifier: Send + Sync {
    /// Verify a Lean 4 source string. Must be deterministic for tests.
    fn verify(&self, lean_source: &str) -> Result<LeanVerdict, KnowledgeError>;

    /// Implementation name (e.g. "stub", "lean-4.6.0").
    fn name(&self) -> &str;
}

/// In-process stub verifier. Accepts any source whose first non-empty,
/// non-comment line begins with `theorem` or `lemma`, and contains the
/// substring `:= by` or `:= sorry` (which we then reject).
///
/// Designed to be deterministic and useful in unit tests; never shells out.
pub struct StubLeanVerifier {
    pub version: String,
}

impl StubLeanVerifier {
    pub fn new() -> Self {
        Self {
            version: "stub-0.1.0".to_string(),
        }
    }
}

impl Default for StubLeanVerifier {
    fn default() -> Self {
        Self::new()
    }
}

impl LeanVerifier for StubLeanVerifier {
    fn verify(&self, lean_source: &str) -> Result<LeanVerdict, KnowledgeError> {
        let trimmed = lean_source.trim();
        if trimmed.is_empty() {
            return Ok(LeanVerdict {
                status: LeanProofStatus::Failed,
                verifier_version: self.version.clone(),
                proof_artifact: None,
                diagnostics: Some("empty Lean source".into()),
            });
        }

        // Find the first meaningful line.
        let header = trimmed
            .lines()
            .map(str::trim)
            .find(|l| !l.is_empty() && !l.starts_with("--"))
            .unwrap_or("");

        let starts_ok = header.starts_with("theorem ")
            || header.starts_with("lemma ")
            || header.starts_with("def ")
            || header.starts_with("example ");

        if !starts_ok {
            return Ok(LeanVerdict {
                status: LeanProofStatus::Failed,
                verifier_version: self.version.clone(),
                proof_artifact: None,
                diagnostics: Some(format!(
                    "expected 'theorem/lemma/def/example' at start, got: {}",
                    header.chars().take(80).collect::<String>()
                )),
            });
        }

        if trimmed.contains(":= sorry") || trimmed.contains(":=sorry") {
            return Ok(LeanVerdict {
                status: LeanProofStatus::Failed,
                verifier_version: self.version.clone(),
                proof_artifact: None,
                diagnostics: Some("proof body is `sorry` — incomplete".into()),
            });
        }

        // Treat any non-sorry body as accepted.
        let proof_artifact = format!("{{stub-olean for hash={}}}", short_hash(lean_source))
            .into_bytes();
        Ok(LeanVerdict {
            status: LeanProofStatus::Verified,
            verifier_version: self.version.clone(),
            proof_artifact: Some(proof_artifact),
            diagnostics: None,
        })
    }

    fn name(&self) -> &str {
        "stub"
    }
}

/// External Lean 4 verifier — shells out to the `lean` binary.
///
/// Stays a *stub-by-default* until `lean` is on PATH. Returns `Failed` with
/// a diagnostic explaining why if the binary is missing, so calling code
/// never panics on non-Lean machines.
pub struct ExternalLeanVerifier {
    /// Path to the `lean` executable. Defaults to `"lean"` (PATH lookup).
    pub binary: PathBuf,
}

impl ExternalLeanVerifier {
    pub fn new() -> Self {
        Self {
            binary: PathBuf::from("lean"),
        }
    }

    pub fn with_binary(binary: PathBuf) -> Self {
        Self { binary }
    }
}

impl Default for ExternalLeanVerifier {
    fn default() -> Self {
        Self::new()
    }
}

impl LeanVerifier for ExternalLeanVerifier {
    fn verify(&self, lean_source: &str) -> Result<LeanVerdict, KnowledgeError> {
        use std::io::Write;
        use std::process::Command;

        // Write source to a temp .lean file
        let tmp = tempfile_in()?;
        let lean_path = tmp.path().with_extension("lean");
        let mut f = std::fs::File::create(&lean_path)?;
        f.write_all(lean_source.as_bytes())?;

        let output = match Command::new(&self.binary).arg(&lean_path).output() {
            Ok(o) => o,
            Err(e) => {
                return Ok(LeanVerdict {
                    status: LeanProofStatus::Failed,
                    verifier_version: "external-not-available".into(),
                    proof_artifact: None,
                    diagnostics: Some(format!(
                        "could not invoke lean binary at {:?}: {}",
                        self.binary, e
                    )),
                });
            }
        };

        let _ = std::fs::remove_file(&lean_path);

        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        let combined = format!("{}\n{}", stdout, stderr);

        if output.status.success() {
            Ok(LeanVerdict {
                status: LeanProofStatus::Verified,
                verifier_version: "lean-external".into(),
                proof_artifact: None,
                diagnostics: if combined.trim().is_empty() {
                    None
                } else {
                    Some(combined)
                },
            })
        } else {
            Ok(LeanVerdict {
                status: LeanProofStatus::Failed,
                verifier_version: "lean-external".into(),
                proof_artifact: None,
                diagnostics: Some(combined),
            })
        }
    }

    fn name(&self) -> &str {
        "external"
    }
}

fn tempfile_in() -> Result<tempfile::NamedTempFile, KnowledgeError> {
    tempfile::NamedTempFile::new().map_err(KnowledgeError::Io)
}

fn short_hash(s: &str) -> String {
    let h = dt_core::sha3_256_hex(s.as_bytes());
    h.chars().take(12).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stub_accepts_simple_theorem() {
        let v = StubLeanVerifier::new();
        let src = "theorem t1 : 1 + 1 = 2 := by rfl";
        let verdict = v.verify(src).unwrap();
        assert_eq!(verdict.status, LeanProofStatus::Verified);
        assert!(verdict.proof_artifact.is_some());
    }

    #[test]
    fn stub_rejects_sorry() {
        let v = StubLeanVerifier::new();
        let src = "theorem hard : True := sorry";
        let verdict = v.verify(src).unwrap();
        assert_eq!(verdict.status, LeanProofStatus::Failed);
        assert!(verdict
            .diagnostics
            .as_deref()
            .unwrap_or("")
            .contains("sorry"));
    }

    #[test]
    fn stub_rejects_garbage() {
        let v = StubLeanVerifier::new();
        let verdict = v.verify("hello world").unwrap();
        assert_eq!(verdict.status, LeanProofStatus::Failed);
    }

    #[test]
    fn stub_rejects_empty() {
        let v = StubLeanVerifier::new();
        let verdict = v.verify("   \n  \n").unwrap();
        assert_eq!(verdict.status, LeanProofStatus::Failed);
    }

    #[test]
    fn proof_status_roundtrip() {
        for s in &[
            LeanProofStatus::Unknown,
            LeanProofStatus::Pending,
            LeanProofStatus::Verified,
            LeanProofStatus::Failed,
        ] {
            assert_eq!(LeanProofStatus::parse(s.as_str()), *s);
        }
    }

    #[test]
    fn external_handles_missing_binary() {
        let v = ExternalLeanVerifier::with_binary(PathBuf::from(
            "/nonexistent/path/to/lean-binary-xyz",
        ));
        let verdict = v.verify("theorem t : True := trivial").unwrap();
        assert_eq!(verdict.status, LeanProofStatus::Failed);
        assert!(verdict
            .diagnostics
            .as_deref()
            .unwrap_or("")
            .contains("could not invoke lean"));
    }
}
