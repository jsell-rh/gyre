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
    pub created_at: u64,
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
