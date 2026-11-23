//! Per-workspace merge policy (meta-spec-reconciliation.md §9).
//!
//! The workspace merge policy is the specced `MetaSpecPolicy` struct in
//! full — `warn_on_drift` / `block_on_drift` / `drift_tolerance` — not a
//! single strict-mode boolean. The opinionated default is "warn always,
//! block never, reconcile in the background" (§9: velocity over strict
//! consistency); a workspace can opt into blocking with a version-count
//! tolerance (`drift_tolerance: 0` blocks on any drift, `1` blocks only
//! when a meta-spec is more than one version behind).
//!
//! Storage: the policy is a small JSON document persisted through the
//! port-backed KV store (namespace `workspace_merge_policies`, key =
//! workspace id), the same durable-store pattern as the repo stack policies.
//! It is read by the merge processor on every merge-queue cycle.
//!
//! Backward compatibility: documents written by the pre-§9-full shape held
//! a single `require_current_meta_spec_set` boolean. Reading such a
//! document maps `true` to `block_on_drift: true, drift_tolerance: 0` —
//! preserving the old block-on-any-drift semantics for existing strict
//! workspaces — and `false`/absent to the §9 default. New writes always
//! store the full shape (the legacy field is never re-persisted).

use serde::{Deserialize, Serialize};
use tracing::warn;

use gyre_common::Id;

use crate::AppState;

/// KV namespace for per-workspace merge policies.
pub const MERGE_POLICY_KV_NAMESPACE: &str = "workspace_merge_policies";

/// The §9 merge-gate drift policy (meta-spec-reconciliation.md §9),
/// persisted per workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MetaSpecPolicy {
    /// Warn on MRs produced under outdated meta-specs.
    /// Default: true. Always on in strict mode (`block_on_drift: true`)
    /// — the warning is recorded alongside the block regardless of this
    /// flag, so a blocked merge always names the drift that held it.
    #[serde(default = "default_warn_on_drift")]
    pub warn_on_drift: bool,

    /// Block MRs produced under outdated meta-specs.
    /// Default: false. Only enable for high-assurance workspaces.
    #[serde(default = "default_block_on_drift")]
    pub block_on_drift: bool,

    /// How many versions behind before blocking (if `block_on_drift` is
    /// true). Default: 1 (block only if more than 1 version behind).
    /// 0 = block on any drift.
    #[serde(default = "default_drift_tolerance")]
    pub drift_tolerance: u32,
}

fn default_warn_on_drift() -> bool {
    true
}

fn default_block_on_drift() -> bool {
    false
}

fn default_drift_tolerance() -> u32 {
    1
}

impl Default for MetaSpecPolicy {
    /// The opinionated §9 default: warn always, block never, tolerance 1.
    fn default() -> Self {
        Self {
            warn_on_drift: true,
            block_on_drift: false,
            drift_tolerance: 1,
        }
    }
}

impl MetaSpecPolicy {
    /// The tolerance-0 strict view of this policy — the legacy
    /// `require_current_meta_spec_set` semantics (block on ANY drift),
    /// still exposed by the merge-policy endpoint for old clients.
    pub fn blocks_on_any_drift(&self) -> bool {
        self.block_on_drift && self.drift_tolerance == 0
    }
}

/// Parse and validate a caller-supplied `drift_tolerance` JSON number.
///
/// The §9 field is a `u32`, but JSON numbers arrive untyped: a negative
/// or fractional value must be rejected with a 400 (not a serde 422),
/// and anything outside `u32` range cannot name a version count.
pub fn parse_drift_tolerance(value: serde_json::Number) -> Result<u32, String> {
    if let Some(n) = value.as_u64() {
        if n <= u32::MAX as u64 {
            return Ok(n as u32);
        }
    }
    Err(format!(
        "drift_tolerance must be an integer between 0 and {}",
        u32::MAX
    ))
}

/// Deserialization shim for the stored policy document, carrying the
/// legacy `require_current_meta_spec_set` field for read compatibility.
///
/// A stored `require_current_meta_spec_set: true` (the pre-§9-full strict
/// flag) maps to `block_on_drift: true, drift_tolerance: 0` — the old
/// flag meant "block on any drift", which is exactly tolerance-0
/// blocking. `false`/absent leaves the §9 shape as stored/defaulted.
#[derive(Debug, Clone, Copy, Deserialize)]
struct StoredPolicyDoc {
    #[serde(default)]
    require_current_meta_spec_set: bool,
    #[serde(default = "default_warn_on_drift")]
    warn_on_drift: bool,
    #[serde(default = "default_block_on_drift")]
    block_on_drift: bool,
    #[serde(default = "default_drift_tolerance")]
    drift_tolerance: u32,
}

impl From<StoredPolicyDoc> for MetaSpecPolicy {
    fn from(doc: StoredPolicyDoc) -> Self {
        if doc.require_current_meta_spec_set {
            // Legacy strict workspaces keep block-on-any-drift behavior:
            // tolerance 0, and the warning stays on (it was always
            // recorded under the old single-flag policy).
            MetaSpecPolicy {
                warn_on_drift: doc.warn_on_drift,
                block_on_drift: true,
                drift_tolerance: 0,
            }
        } else {
            MetaSpecPolicy {
                warn_on_drift: doc.warn_on_drift,
                block_on_drift: doc.block_on_drift,
                drift_tolerance: doc.drift_tolerance,
            }
        }
    }
}

/// Load the merge policy for a workspace.
///
/// A workspace with no stored policy gets the §9 default (warn only). A
/// corrupt stored value is an error — callers decide whether to surface
/// it (API) or degrade to the default (merge processor, see
/// [`merge_policy_or_default`]).
pub async fn get_merge_policy(
    state: &AppState,
    workspace_id: &Id,
) -> anyhow::Result<MetaSpecPolicy> {
    let raw = state
        .kv_store
        .kv_get(MERGE_POLICY_KV_NAMESPACE, workspace_id.as_str())
        .await?;
    match raw {
        None => Ok(MetaSpecPolicy::default()),
        Some(json) => serde_json::from_str::<StoredPolicyDoc>(&json)
            .map(MetaSpecPolicy::from)
            .map_err(|e| {
                anyhow::anyhow!(
                    "corrupt merge policy for workspace {}: {e}",
                    workspace_id.as_str()
                )
            }),
    }
}

/// Persist the merge policy for a workspace (full §9 shape).
pub async fn set_merge_policy(
    state: &AppState,
    workspace_id: &Id,
    policy: &MetaSpecPolicy,
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
pub(crate) async fn merge_policy_or_default(state: &AppState, workspace_id: &Id) -> MetaSpecPolicy {
    match get_merge_policy(state, workspace_id).await {
        Ok(p) => p,
        Err(e) => {
            warn!(
                workspace_id = workspace_id.as_str(),
                error = %e,
                "merge policy lookup failed; using default (warn only)"
            );
            MetaSpecPolicy::default()
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
        assert_eq!(policy, MetaSpecPolicy::default());
        assert!(policy.warn_on_drift);
        assert!(!policy.block_on_drift);
        assert_eq!(policy.drift_tolerance, 1);
        assert!(!policy.blocks_on_any_drift());
    }

    #[tokio::test]
    async fn policy_roundtrip_through_kv_store() {
        let state = test_state();
        let ws = Id::new("ws-mp-1");
        set_merge_policy(
            &state,
            &ws,
            &MetaSpecPolicy {
                warn_on_drift: true,
                block_on_drift: true,
                drift_tolerance: 1,
            },
        )
        .await
        .unwrap();

        let loaded = get_merge_policy(&state, &ws).await.unwrap();
        assert_eq!(
            loaded,
            MetaSpecPolicy {
                warn_on_drift: true,
                block_on_drift: true,
                drift_tolerance: 1
            }
        );
    }

    #[tokio::test]
    async fn legacy_strict_flag_maps_to_block_with_tolerance_zero() {
        // A workspace stored under the pre-§9-full single-boolean shape
        // with strict mode ON keeps block-on-any-drift semantics:
        // block_on_drift: true, drift_tolerance: 0 (§9 compat mapping).
        let state = test_state();
        let ws = Id::new("ws-mp-legacy-strict");
        state
            .kv_store
            .kv_set(
                MERGE_POLICY_KV_NAMESPACE,
                ws.as_str(),
                r#"{"require_current_meta_spec_set": true}"#.to_string(),
            )
            .await
            .unwrap();

        let loaded = get_merge_policy(&state, &ws).await.unwrap();
        assert!(loaded.block_on_drift);
        assert_eq!(loaded.drift_tolerance, 0);
        assert!(loaded.warn_on_drift, "legacy strict mode always warned");
        assert!(loaded.blocks_on_any_drift());
    }

    #[tokio::test]
    async fn legacy_non_strict_flag_maps_to_default() {
        let state = test_state();
        let ws = Id::new("ws-mp-legacy-warn");
        state
            .kv_store
            .kv_set(
                MERGE_POLICY_KV_NAMESPACE,
                ws.as_str(),
                r#"{"require_current_meta_spec_set": false}"#.to_string(),
            )
            .await
            .unwrap();

        let loaded = get_merge_policy(&state, &ws).await.unwrap();
        assert_eq!(loaded, MetaSpecPolicy::default());
    }

    #[tokio::test]
    async fn new_write_replaces_legacy_strict_document() {
        // After a full-shape write, the legacy flag is gone from storage:
        // the stored document IS the new policy, so a later relaxed
        // tolerance is not overridden by a stale strict flag.
        let state = test_state();
        let ws = Id::new("ws-mp-rewrite");
        state
            .kv_store
            .kv_set(
                MERGE_POLICY_KV_NAMESPACE,
                ws.as_str(),
                r#"{"require_current_meta_spec_set": true}"#.to_string(),
            )
            .await
            .unwrap();

        let mut policy = get_merge_policy(&state, &ws).await.unwrap();
        policy.drift_tolerance = 2;
        set_merge_policy(&state, &ws, &policy).await.unwrap();

        let loaded = get_merge_policy(&state, &ws).await.unwrap();
        assert_eq!(
            loaded,
            MetaSpecPolicy {
                warn_on_drift: true,
                block_on_drift: true,
                drift_tolerance: 2
            }
        );
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
        assert_eq!(fallback, MetaSpecPolicy::default());
    }

    #[test]
    fn parse_drift_tolerance_rejects_invalid_numbers() {
        assert_eq!(
            parse_drift_tolerance(serde_json::json!(0).as_number().unwrap().clone()).unwrap(),
            0
        );
        assert_eq!(
            parse_drift_tolerance(serde_json::json!(3).as_number().unwrap().clone()).unwrap(),
            3
        );
        // Negative and fractional tolerances are 400-grade input errors.
        assert!(parse_drift_tolerance(serde_json::json!(-1).as_number().unwrap().clone()).is_err());
        assert!(
            parse_drift_tolerance(serde_json::json!(1.5).as_number().unwrap().clone()).is_err()
        );
        // Beyond u32 range cannot name a version count.
        assert!(parse_drift_tolerance(
            serde_json::json!(u32::MAX as u64 + 1)
                .as_number()
                .unwrap()
                .clone()
        )
        .is_err());
    }
}
