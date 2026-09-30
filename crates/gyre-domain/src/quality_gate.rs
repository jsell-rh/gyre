//! Quality gate domain types for the merge queue.

use gyre_common::Id;
use serde::{Deserialize, Serialize};

// Re-export GateType and GateStatus from gyre-common so that existing
// `use gyre_domain::{GateType, GateStatus}` paths continue to resolve.
pub use gyre_common::{GateStatus, GateType};

/// A quality check that must pass before an MR can be merged.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct QualityGate {
    pub id: Id,
    /// Repository this gate applies to.
    pub repo_id: Id,
    /// Human-readable name, e.g. "unit tests".
    pub name: String,
    pub gate_type: GateType,
    /// Shell command to run (used by TestCommand and LintCommand).
    pub command: Option<String>,
    /// Minimum number of approvals required (used by RequiredApprovals).
    pub required_approvals: Option<u32>,
    /// Persona file path for AgentReview / AgentValidation gates.
    pub persona: Option<String>,
    /// When false, a failing gate is advisory only — it does not block the MR from merging.
    /// Defaults to true (blocking).
    #[serde(default = "default_required")]
    pub required: bool,
    /// When this gate runs: before merge (blocking, per-MR) or after merge
    /// (validation against the new default-branch HEAD). Defaults to pre-merge.
    #[serde(default)]
    pub gate_phase: GatePhase,
    /// Timeout in seconds for TestCommand/LintCommand execution.
    /// `None` uses the system default (300s).
    #[serde(default)]
    pub timeout_secs: Option<u64>,
    pub created_at: u64,
}

/// The phase a quality gate runs in (platform-model.md §6).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GatePhase {
    /// Runs before merge against the MR's speculative merge commit.
    PreMerge,
    /// Runs after merge against the new HEAD of the default branch.
    PostMerge,
}

impl Default for GatePhase {
    fn default() -> Self {
        Self::PreMerge
    }
}

impl GatePhase {
    /// Parses the wire/DB string representation.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "pre_merge" => Some(Self::PreMerge),
            "post_merge" => Some(Self::PostMerge),
            _ => None,
        }
    }

    /// Canonical snake_case string for DB storage.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::PreMerge => "pre_merge",
            Self::PostMerge => "post_merge",
        }
    }
}

fn default_required() -> bool {
    true
}

/// The result of running one quality gate against one MR.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GateResult {
    pub id: Id,
    pub gate_id: Id,
    pub mr_id: Id,
    pub status: GateStatus,
    /// Captured stdout/stderr (truncated to 4 KiB).
    pub output: Option<String>,
    pub started_at: Option<u64>,
    pub finished_at: Option<u64>,
}
