//! Rekor transparency-log publication for merge attestation bundles
//! (agent-gates.md §Part 3 Storage: "in Rekor transparency log for
//! non-repudiation"; task-264).

use anyhow::Result;
use async_trait::async_trait;
use gyre_domain::merge_attestation::MergeAttestationBundle;

/// Submission of merge attestation bundles to a transparency log.
///
/// The production implementation (gyre-adapters' `HttpRekorClient`, wired
/// since task-264) submits each bundle as a hashedrekord entry to the
/// configured Rekor instance (`GYRE_REKOR_URL`) and returns the log
/// entry's UUID. Failure semantics are explicit (task-264): an
/// unconfigured log and any Rekor error are `Err`s — never a silent
/// `Ok(None)` — so the caller can surface the failed/pending
/// non-repudiation state instead of reporting success.
#[async_trait]
#[allow(clippy::double_must_use)] // async_trait generates a must_use future for each method.
pub trait RekorClient: Send + Sync {
    /// Submit the signed bundle for inclusion in the transparency log.
    /// Returns the log entry's UUID when the submission was accepted;
    /// `Err` when no log is configured or the submission failed — never a
    /// silent `Ok(None)` (task-264).
    async fn submit(&self, bundle: &MergeAttestationBundle) -> Result<Option<String>>;

    /// Whether a transparency log backend is configured.
    fn is_configured(&self) -> bool;
}
