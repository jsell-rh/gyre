//! Rekor transparency-log client adapters (agent-gates.md §Part 3
//! Storage: \"in Rekor transparency log for non-repudiation\").

use anyhow::Result;
use async_trait::async_trait;
use gyre_domain::merge_attestation::MergeAttestationBundle;
use gyre_ports::rekor::RekorClient;

/// No-op transparency-log client for deployments without Rekor
/// configured. Every submission is surfaced as a warning —
/// non-repudiation is visibly pending, never silently claimed — and
/// returns no log entry.
///
/// TODO (task-144 plan step 4): real Rekor API integration — submit the
/// bundle's canonical JSON as a hashedrekord to the configured Rekor
/// server (`GYRE_REKOR_URL`), with a latency-bounded HTTP client, and
/// return the inclusion entry id.
#[derive(Debug, Default)]
pub struct NoopRekorClient;

#[async_trait]
impl RekorClient for NoopRekorClient {
    async fn submit(&self, bundle: &MergeAttestationBundle) -> Result<Option<String>> {
        tracing::warn!(
            mr_id = %bundle.mr_id,
            merge_commit_sha = %bundle.merge_commit_sha,
            "Rekor transparency log not configured — merge attestation bundle not submitted for non-repudiation"
        );
        Ok(None)
    }

    fn is_configured(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The no-op client reports itself unconfigured, surfaces every
    /// submission with a warning, and returns no log entry — the
    /// not-configured state is visible, not silent.
    #[tokio::test]
    async fn noop_client_reports_unconfigured_and_returns_none() {
        let client = NoopRekorClient;
        assert!(!client.is_configured());
        let entry = client
            .submit(&MergeAttestationBundle {
                merge_attestation_version: "1.0".to_string(),
                mr_id: "mr-1".to_string(),
                merge_commit_sha: "abc".to_string(),
                merged_at: 0,
                spec_ref: None,
                spec_approval: None,
                author: None,
                gates: vec![],
                bundle_signature: None,
            })
            .await
            .unwrap();
        assert!(entry.is_none(), "no log entry without a configured log");
    }
}
