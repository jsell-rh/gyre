//! Durable pending-since signal for spec ledger entries
//! (spec-registry.md §Accountability Agent, task-339).
//!
//! The accountability patrol's pending-beyond-one-cycle check — "specs with
//! `requires_approval: true` that have been `pending` for more than one Ralph
//! loop cycle" — needs a real pending-since signal. `SpecLedgerEntry::
//! updated_at` cannot serve: it churns on every unrelated write (title
//! changes, link refreshes, scope backfills), so it measures last-touch, not
//! how long the spec has been waiting for approval.
//!
//! This module maintains a per-spec timestamp as equivalent durable state:
//! a record in the port-backed KV store (the `kv_store` table persisted by
//! the SQLite and PostgreSQL adapters), keyed by the ledger's own path key
//! (`system/x.md`). It is written exactly where `approval_status` (re-)
//! enters `pending` and cleared where the entry leaves it:
//!
//! - `sync_spec_ledger`: new spec registration, content invalidation,
//!   re-pending after deprecation, and extends-drift invalidation record it;
//!   manifest-removal deprecation clears it;
//! - `approve_spec` / `revoke_spec` / `reject_spec` in `api/specs.rs`:
//!   resolution transitions re-record or clear it via [`on_transition`].
//!
//! Failure semantics: writes are best-effort alongside the ledger save (the
//! same contract as the ledger saves themselves — a failed write is logged,
//! and the patrol then cannot evaluate that entry's age and skips it, never
//! fabricating a timestamp). Reads propagate errors: a patrol that cannot
//! read its inputs must not report "nothing wrong".

use gyre_domain::ApprovalStatus;
use std::sync::Arc;

/// KV namespace for the pending-since markers, keyed by ledger spec path.
pub const NAMESPACE: &str = "spec_pending_since";

/// Record that the spec entered `pending` at `now` (seconds since epoch).
///
/// Restarts any earlier pending period — every caller is a (re-)entry into
/// pending, so the timestamp names the start of the CURRENT wait.
pub async fn mark_pending(kv: &Arc<dyn gyre_ports::KvJsonStore>, spec_path: &str, now: u64) {
    let value = serde_json::json!({ "since": now }).to_string();
    if let Err(e) = kv.kv_set(NAMESPACE, spec_path, value).await {
        tracing::warn!(
            spec_path,
            error = %e,
            "spec_pending_since: failed to record pending-since marker"
        );
    }
}

/// Clear the marker: the spec left `pending` (approved / rejected / revoked /
/// deprecated).
pub async fn clear_pending(kv: &Arc<dyn gyre_ports::KvJsonStore>, spec_path: &str) {
    if let Err(e) = kv.kv_remove(NAMESPACE, spec_path).await {
        tracing::warn!(
            spec_path,
            error = %e,
            "spec_pending_since: failed to clear pending-since marker"
        );
    }
}

/// Read the recorded pending-since timestamp, if any.
///
/// Propagates storage errors (fail closed for readers). A present-but-
/// unparseable record reads as `None` with a warning — the signal cannot be
/// evaluated, and the patrol skips the entry rather than guessing.
pub async fn pending_since(
    kv: &Arc<dyn gyre_ports::KvJsonStore>,
    spec_path: &str,
) -> anyhow::Result<Option<u64>> {
    match kv.kv_get(NAMESPACE, spec_path).await? {
        None => Ok(None),
        Some(raw) => match serde_json::from_str::<serde_json::Value>(&raw) {
            Ok(v) => Ok(v.get("since").and_then(|s| s.as_u64())),
            Err(e) => {
                tracing::warn!(
                    spec_path,
                    error = %e,
                    "spec_pending_since: corrupt pending-since record — treating as absent"
                );
                Ok(None)
            }
        },
    }
}

/// Maintain the marker across an approval-status transition made by the
/// approve/revoke resolution paths: entering `pending` starts a new pending
/// period, leaving it ends the current one, and an unchanged status keeps
/// the existing period (a partial approval that leaves the spec pending
/// must not restart the clock).
pub async fn on_transition(
    kv: &Arc<dyn gyre_ports::KvJsonStore>,
    spec_path: &str,
    old_status: &ApprovalStatus,
    new_status: &ApprovalStatus,
    now: u64,
) {
    if old_status == new_status {
        return;
    }
    if *new_status == ApprovalStatus::Pending {
        mark_pending(kv, spec_path, now).await;
    } else {
        clear_pending(kv, spec_path).await;
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mem::MemKvStore;

    fn kv() -> Arc<dyn gyre_ports::KvJsonStore> {
        Arc::new(MemKvStore::default())
    }

    #[tokio::test]
    async fn mark_then_read_round_trips_the_timestamp() {
        let kv = kv();
        assert_eq!(pending_since(&kv, "system/x.md").await.unwrap(), None);

        mark_pending(&kv, "system/x.md", 1_700_000_000).await;
        assert_eq!(
            pending_since(&kv, "system/x.md").await.unwrap(),
            Some(1_700_000_000)
        );

        clear_pending(&kv, "system/x.md").await;
        assert_eq!(pending_since(&kv, "system/x.md").await.unwrap(), None);
    }

    /// A second `mark_pending` restarts the period: the marker always names
    /// the start of the CURRENT wait, not the first one.
    #[tokio::test]
    async fn remarking_restarts_the_period() {
        let kv = kv();
        mark_pending(&kv, "system/x.md", 100).await;
        mark_pending(&kv, "system/x.md", 500).await;
        assert_eq!(pending_since(&kv, "system/x.md").await.unwrap(), Some(500));
    }

    /// Transition semantics: Pending→Approved clears, Approved→Pending
    /// records, Pending→Pending (partial approval) keeps the original
    /// period.
    #[tokio::test]
    async fn transitions_mark_clear_and_preserve() {
        let kv = kv();
        let pending = ApprovalStatus::Pending;
        let approved = ApprovalStatus::Approved;

        on_transition(&kv, "system/x.md", &approved, &pending, 100).await;
        assert_eq!(pending_since(&kv, "system/x.md").await.unwrap(), Some(100));

        // Unchanged status (partial approval): clock keeps running.
        on_transition(&kv, "system/x.md", &pending, &pending, 999).await;
        assert_eq!(pending_since(&kv, "system/x.md").await.unwrap(), Some(100));

        on_transition(&kv, "system/x.md", &pending, &approved, 200).await;
        assert_eq!(pending_since(&kv, "system/x.md").await.unwrap(), None);

        // Demotion back to pending (revocation) starts a new period.
        on_transition(&kv, "system/x.md", &approved, &pending, 300).await;
        assert_eq!(pending_since(&kv, "system/x.md").await.unwrap(), Some(300));
    }
}
