//! Rekor transparency-log publication for merge attestation bundles
//! (agent-gates.md §Part 3 Storage: \"in Rekor transparency log for
//! non-repudiation\").

use anyhow::Result;
use async_trait::async_trait;
use gyre_domain::merge_attestation::MergeAttestationBundle;

/// Submission of merge attestation bundles to a transparency log.
///
/// The initial deployment ships a no-op implementation (the task-144
/// plan stages the Rekor API integration behind this port): while no
/// log is configured, submissions are surfaced with a warning —
/// non-repudiation is visibly pending, never silently claimed — and
/// return no log entry.
#[async_trait]
#[allow(clippy::double_must_use)] // async_trait generates a must_use future for each method.
pub trait RekorClient: Send + Sync {
    /// Submit the signed bundle for inclusion in the transparency log.
    /// Returns the log entry's identifier (Rekor entry UUID / URL) when
    /// the submission was accepted; `None` when no log is configured.
    async fn submit(&self, bundle: &MergeAttestationBundle) -> Result<Option<String>>;

    /// Whether a transparency log backend is configured.
    fn is_configured(&self) -> bool;
}
