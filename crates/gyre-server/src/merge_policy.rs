//! Per-workspace merge policy (meta-spec-reconciliation.md §9).
//!
//! The workspace merge policy holds the opt-in strict-mode switch for the
//! meta-spec drift merge gate. By default the gate only warns (§9: "Default
//! behavior: warn, don't block"); a workspace admin can set
//! `require_current_meta_spec_set: true` to convert the drift warning into a
//! hard block ("Opt-in strict mode (per workspace)").
//!
//! Storage: the policy is a small JSON document persisted through the
//! port-backed KV store (namespace `workspace_merge_policies`, key =
//! workspace id), the same durable-store pattern as the repo stack policies.
//! It is read by the merge processor on every merge-queue cycle.

use serde::{Deserialize, Serialize};
use tracing::warn;

use gyre_common::Id;

use crate::AppState;

/// KV namespace for per-workspace merge policies.
pub const MERGE_POLICY_KV_NAMESPACE: &str = "workspace_merge_policies";

/// Per-workspace merge policy (meta-spec-reconciliation.md §9).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceMergePolicy {
    /// Block merges whose provenance records a meta-spec set SHA that differs
    /// from the workspace's current set, instead of warning.
    /// Default: false (warn only — "velocity over strict consistency").
    #[serde(default)]
    pub require_current_meta_spec_set: bool,
}

/// Load the merge policy for a workspace.
///
/// A workspace with no stored policy gets the default (warn only). A corrupt
/// stored value is logged and treated as the default — the spec's opinionated
/// default is "warn always, block never", so a unreadable strict-mode flag
/// must not silently wedge every merge in the queue.
pub async fn get_merge_policy(
    state: &AppState,
    workspace_id: &Id,
) -> anyhow::Result<WorkspaceMergePolicy> {
    let raw = state
        .kv_store
        .kv_get(MERGE_POLICY_KV_NAMESPACE, workspace_id.as_str())
        .await?;
    match raw {
        None => Ok(WorkspaceMergePolicy::default()),
        Some(json) => serde_json::from_str::<WorkspaceMergePolicy>(&json).map_err(|e| {
            anyhow::anyhow!(
                "corrupt merge policy for workspace {}: {e}",
                workspace_id.as_str()
            )
        }),
    }
}

/// Persist the merge policy for a workspace.
pub async fn set_merge_policy(
    state: &AppState,
    workspace_id: &Id,
    policy: &WorkspaceMergePolicy,
) -> anyhow::Result<()> {
    let json = serde_json::to_string(policy)?;
    state
        .kv_store
        .kv_set(MERGE_POLICY_KV_NAMESPACE, workspace_id.as_str(), json)
        .await
}

/// Load the merge policy, falling back to the default on storage errors.
///
/// Used by the merge processor, where a policy-store outage should surface as
/// a logged degradation to the spec default rather than aborting the cycle.
pub(crate) async fn merge_policy_or_default(
    state: &AppState,
    workspace_id: &Id,
) -> WorkspaceMergePolicy {
    match get_merge_policy(state, workspace_id).await {
        Ok(p) => p,
        Err(e) => {
            warn!(
                workspace_id = workspace_id.as_str(),
                error = %e,
                "merge policy lookup failed; using default (warn only)"
            );
            WorkspaceMergePolicy::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mem::test_state;

    #[tokio::test]
    async fn missing_policy_defaults_to_warn_only() {
        let state = test_state();
        let policy = get_merge_policy(&state, &Id::new("ws-none")).await.unwrap();
        assert_eq!(policy, WorkspaceMergePolicy::default());
        assert!(!policy.require_current_meta_spec_set);
    }

    #[tokio::test]
    async fn policy_roundtrip_through_kv_store() {
        let state = test_state();
        let ws = Id::new("ws-mp-1");
        set_merge_policy(
            &state,
            &ws,
            &WorkspaceMergePolicy {
                require_current_meta_spec_set: true,
            },
        )
        .await
        .unwrap();

        let loaded = get_merge_policy(&state, &ws).await.unwrap();
        assert!(loaded.require_current_meta_spec_set);
    }

    #[tokio::test]
    async fn corrupt_policy_reports_error_and_default_falls_back() {
        let state = test_state();
        let ws = Id::new("ws-mp-2");
        state
            .kv_store
            .kv_set(
                MERGE_POLICY_KV_NAMESPACE,
                ws.as_str(),
                "not json".to_string(),
            )
            .await
            .unwrap();

        let err = get_merge_policy(&state, &ws).await;
        assert!(err.is_err(), "corrupt policy must surface an error");

        // The merge-processor path degrades to the spec default instead of
        // wedging the queue.
        let fallback = merge_policy_or_default(&state, &ws).await;
        assert_eq!(fallback, WorkspaceMergePolicy::default());
    }
}
