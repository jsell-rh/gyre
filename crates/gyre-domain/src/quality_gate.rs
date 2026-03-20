//! Quality gate domain types for the merge queue.

use gyre_common::Id;
use serde::{Deserialize, Serialize};

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
    pub created_at: u64,
}

/// Discriminant for a quality gate's check type.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GateType {
    /// Run a test command; passes when exit code == 0.
    TestCommand,
    /// Run a lint command; passes when exit code == 0.
    LintCommand,
    /// Require N or more approved reviews.
    RequiredApprovals,
}

/// Execution status of one gate check for a specific MR.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GateStatus {
    Pending,
    Running,
    Passed,
    Failed,
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
