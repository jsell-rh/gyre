//! Port trait for server-side pre-accept push validation.
//!
//! Pre-accept gates run synchronously in the receive-pack handler BEFORE the
//! ref is updated. A push is rejected (403) if any configured gate returns
//! `GateResult::Failed`.

/// Context provided to each gate for a single push.
pub struct PushContext {
    /// The repository UUID being pushed to.
    pub repo_id: String,
    /// The ref being updated, e.g. "refs/heads/feat/my-feature".
    pub refname: String,
    /// Short branch name derived from refname, e.g. "feat/my-feature".
    pub branch: String,
    /// Commit messages for all commits in this push (new commits only).
    pub commit_messages: Vec<String>,
    /// File paths changed across all commits in this push.
    pub changed_files: Vec<String>,
    /// ID of the agent performing the push (M14.2).
    pub agent_id: Option<String>,
    /// SHA-256 fingerprint of the pushing agent's registered stack (M14.2).
    /// None if the agent has not registered a stack.
    pub stack_fingerprint: Option<String>,
    /// SHA-256 fingerprint required by the repo's stack policy (M14.2).
    /// None if no policy is set for this repo.
    pub required_fingerprint: Option<String>,
}

/// Result of a single gate check.
pub enum GateOutcome {
    Passed,
    Failed(String),
}

/// A synchronous, in-process pre-accept gate.
///
/// Gates are registered at server startup and selected per-repo by name via
/// the `push_gates` API (`PUT /api/v1/repos/{id}/push-gates`).
pub trait PreAcceptGate: Send + Sync {
    /// Stable name used to reference this gate in the per-repo config.
    fn name(&self) -> &str;

    /// Run the gate check. Called synchronously in the receive-pack path.
    fn check(&self, ctx: &PushContext) -> GateOutcome;
}
