//! Per-repo spec enforcement policy — domain type moved from gyre-server.

use serde::{Deserialize, Serialize};

/// Per-repo spec enforcement policy.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct SpecPolicy {
    /// If true, MRs without a `spec_ref` field are blocked from merging.
    pub require_spec_ref: bool,
    /// If true, MRs whose `spec_ref` has no active approval in the ledger are blocked.
    pub require_approved_spec: bool,
    /// If true, emit a `StaleSpecWarning` domain event when an MR's spec_ref SHA is not
    /// the current HEAD blob SHA for that spec file.
    #[serde(default)]
    pub warn_stale_spec: bool,
    /// If true, block merging when an MR's spec_ref SHA is not the current HEAD blob SHA.
    #[serde(default)]
    pub require_current_spec: bool,
    /// If true, the active approval for the referenced spec SHA must carry a
    /// Sigstore signature (agent-gates.md §Forge Enforcement Policies:
    /// "Spec approvals must include Sigstore signature").
    #[serde(default)]
    pub require_signed_approval: bool,
    /// If true, reject pushes that add spec files under `specs/` without a
    /// corresponding entry in `specs/manifest.yaml`.
    /// spec-registry.md §Manifest Rules rule 1 + §Ledger Sync on Push step 4.
    #[serde(default)]
    pub enforce_manifest: bool,
}
