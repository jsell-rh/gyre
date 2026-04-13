//! Quality gate enum types shared across the crate boundary.
//!
//! `GateType` and `GateStatus` are pure value enums with no domain logic.
//! They live in `gyre-common` (not `gyre-domain`) so that types like
//! `GateAttestation` in the attestation module can reference them without
//! violating the hexagonal architecture boundary.

use serde::{Deserialize, Serialize};

/// Discriminant for a quality gate check type.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GateType {
    /// Run a test command; passes when exit code == 0.
    TestCommand,
    /// Run a lint command; passes when exit code == 0.
    LintCommand,
    /// Require N or more approved reviews.
    RequiredApprovals,
    /// Spawn a review agent that examines the MR diff and spec; passes when approved.
    AgentReview,
    /// Spawn a validation agent for domain-specific checks; passes when agent reports pass.
    AgentValidation,
    /// Observational gate: captures OTel spans from the integration test run.
    /// Always passes — trace capture is not a quality gate, it is observability.
    TraceCapture,
}

/// Execution status of one gate check for a specific MR.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GateStatus {
    Pending,
    Running,
    Passed,
    Failed,
}
