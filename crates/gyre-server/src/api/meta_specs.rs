//! Meta-spec reconciliation API endpoints (M32).
//!
//! GET  /api/v1/workspaces/{id}/meta-spec-set  — get workspace meta-spec set
//! PUT  /api/v1/workspaces/{id}/meta-spec-set  — update (Admin only)
//! GET  /api/v1/workspaces/{id}/meta-specs/rollout-policy — get rollout policy (§7)
//! PUT  /api/v1/workspaces/{id}/meta-specs/rollout-policy — update (Admin only, §7)
//! GET  /api/v1/meta-specs/{path}/blast-radius — blast radius for a meta-spec
//! POST /api/v1/meta-specs/preview             — run a draft meta-spec through real agents
//! GET  /api/v1/meta-specs/preview/{id}        — poll preview status + produced diffs
//! DELETE /api/v1/meta-specs/preview/{id}      — tear a preview run down now

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use gyre_common::Id;
use gyre_domain::{
    Agent, AgentStatus, AgentWorktree, ComputeTargetEntity, ComputeTargetType, DiffResult,
    Repository, UserRole, Workspace,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use crate::{auth::AuthenticatedAgent, AppState};

use super::error::ApiError;
use super::{new_id, now_secs};

// ---------------------------------------------------------------------------
// Types — meta-spec set
// ---------------------------------------------------------------------------

/// A pinned meta-spec entry (path + SHA).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MetaSpecPinnedEntry {
    pub path: String,
    pub sha: String,
}

/// The bound collection of meta-specs active in a workspace.
///
/// `personas` is a `BTreeMap` (not a `HashMap`) so that serialization is
/// canonical: the same set of pinned entries always serializes to
/// byte-identical JSON. `compute_meta_spec_set_sha` hashes the stored JSON,
/// and the conformance sweep (meta-spec-reconciliation.md §10) plus the §9
/// constraint `agent.meta_spec_set_sha == input.meta_spec_set_sha` compare
/// those hashes — a HashMap's per-instance iteration order would flip the
/// SHA on every re-PUT of an identical set, false-drifting all provenance.
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct MetaSpecSet {
    pub workspace_id: String,
    /// Named persona bindings: role -> pinned entry (e.g. "backend" -> path@sha).
    #[serde(default)]
    pub personas: BTreeMap<String, MetaSpecPinnedEntry>,
    /// Ordered principle specs.
    #[serde(default)]
    pub principles: Vec<MetaSpecPinnedEntry>,
    /// Ordered coding standard specs.
    #[serde(default)]
    pub standards: Vec<MetaSpecPinnedEntry>,
    /// Ordered process specs.
    #[serde(default)]
    pub process: Vec<MetaSpecPinnedEntry>,
}

/// Request body for PUT /api/v1/workspaces/{id}/meta-spec-set.
#[derive(Deserialize)]
pub struct UpdateMetaSpecSetRequest {
    #[serde(default)]
    pub personas: BTreeMap<String, MetaSpecPinnedEntry>,
    #[serde(default)]
    pub principles: Vec<MetaSpecPinnedEntry>,
    #[serde(default)]
    pub standards: Vec<MetaSpecPinnedEntry>,
    #[serde(default)]
    pub process: Vec<MetaSpecPinnedEntry>,
}

/// The tenant-level default binding of meta-specs
/// (meta-spec-reconciliation.md §8 — Tenant-Scope Meta-Spec Changes).
///
/// Every workspace in the tenant inherits these pinned entries unless its
/// own meta-spec set overrides the pinned path — the workspace pin wins on
/// conflict. Same canonical-serialization rule as `MetaSpecSet` (BTreeMap
/// personas) so the effective-set SHA is stable across re-PUTs.
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct TenantMetaSpecSet {
    pub tenant_id: String,
    #[serde(default)]
    pub personas: BTreeMap<String, MetaSpecPinnedEntry>,
    #[serde(default)]
    pub principles: Vec<MetaSpecPinnedEntry>,
    #[serde(default)]
    pub standards: Vec<MetaSpecPinnedEntry>,
    #[serde(default)]
    pub process: Vec<MetaSpecPinnedEntry>,
}

/// Request body for PUT /api/v1/tenants/{id}/meta-specs/set.
#[derive(Deserialize)]
pub struct UpdateTenantMetaSpecSetRequest {
    #[serde(default)]
    pub personas: BTreeMap<String, MetaSpecPinnedEntry>,
    #[serde(default)]
    pub principles: Vec<MetaSpecPinnedEntry>,
    #[serde(default)]
    pub standards: Vec<MetaSpecPinnedEntry>,
    #[serde(default)]
    pub process: Vec<MetaSpecPinnedEntry>,
    /// Tenant-admin override (§8): propagate this change past workspace-level
    /// overrides — urgent compliance-driven changes that must be applied
    /// immediately. Without it, workspaces whose own set pins a changed path
    /// are skipped (their own rollout policy governs).
    #[serde(default)]
    pub force: bool,
}

/// Request body for PUT /api/v1/workspaces/{id}/meta-specs/rollout-policy
/// (meta-spec-reconciliation.md §7). `strategy` is required; the other
/// fields default to the workspace's current values so a partial update
/// (e.g. `{"strategy": "immediate"}`) keeps the rest of the policy.
#[derive(Deserialize)]
pub struct UpdateRolloutPolicyRequest {
    pub strategy: String,
    pub max_concurrent_agents: Option<u32>,
    pub priority: Option<String>,
    pub auto_merge: Option<bool>,
}

/// An affected repo entry in a blast radius response.
#[derive(Clone, Serialize, Deserialize)]
pub struct AffectedRepo {
    pub id: String,
    pub workspace_id: String,
    pub reason: String,
}

/// An affected workspace entry in a blast radius response.
#[derive(Clone, Serialize, Deserialize)]
pub struct AffectedWorkspace {
    pub id: String,
}

/// Blast radius response for a meta-spec change.
#[derive(Serialize)]
pub struct BlastRadiusResponse {
    pub spec_path: String,
    pub affected_repos: Vec<AffectedRepo>,
    pub affected_workspaces: Vec<AffectedWorkspace>,
}

// ---------------------------------------------------------------------------
// Types — preview (meta-spec-reconciliation.md §5)
// ---------------------------------------------------------------------------

/// Draft meta-spec under preview. Carried out-of-band in the preview agent's
/// environment: it has no SHA, is unapproved, and is committed nowhere.
#[derive(Clone, Debug, Deserialize)]
pub struct DraftMetaSpec {
    /// `meta:persona` | `meta:principle` | `meta:standard` | `meta:process`.
    pub kind: String,
    /// Full draft text, exactly as the human wrote it.
    pub content: String,
}

/// One preview target: an existing spec to re-implement under the draft.
#[derive(Clone, Debug, Deserialize)]
pub struct PreviewTarget {
    pub repo_id: String,
    pub spec_path: String,
}

/// Request body for POST /api/v1/meta-specs/preview.
#[derive(Debug, Deserialize)]
pub struct PreviewMetaSpecRequest {
    pub draft: DraftMetaSpec,
    /// Every target runs in parallel — one agent each.
    pub targets: Vec<PreviewTarget>,
}

/// One preview agent as reported by the 202 response.
#[derive(Clone, Debug, Serialize)]
pub struct PreviewAgentEntry {
    pub agent_id: String,
    pub repo_id: String,
    pub spec_path: String,
    /// Throwaway branch the agent works on: `preview/{preview_id}/{slug}`.
    pub branch: String,
}

/// Response for POST /api/v1/meta-specs/preview (202 Accepted).
#[derive(Serialize)]
pub struct PreviewResponse {
    pub preview_id: String,
    pub agents: Vec<PreviewAgentEntry>,
}

/// One preview agent with its live status and the diff it has produced.
#[derive(Clone, Debug, Serialize)]
pub struct PreviewAgentStatus {
    pub agent_id: String,
    pub repo_id: String,
    pub spec_path: String,
    pub branch: String,
    /// `running` | `complete` | `failed` | `stopped` | `dead` | `unknown`.
    pub status: String,
    /// Diff of the preview branch against the commit it was created from.
    /// `null` while the branch does not exist yet.
    pub diff: Option<DiffResult>,
}

/// Response for GET /api/v1/meta-specs/preview/{preview_id}.
#[derive(Serialize)]
pub struct PreviewStatusResponse {
    pub preview_id: String,
    pub created_at: u64,
    /// Seconds after which the GC sweep tears the run down.
    pub ttl_seconds: u64,
    /// `running` while any agent is still running, else `complete`.
    pub state: String,
    pub agents: Vec<PreviewAgentStatus>,
}

/// Per-agent bookkeeping for a preview run.
///
/// Persisted in two places: embedded in [`PreviewRecord`], and keyed by agent
/// id in the `preview_agents` kv namespace. That second index is the preview
/// discriminator consulted on every agent process exit, so a normal agent's
/// exit costs one kv lookup instead of a scan over all preview runs.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PreviewAgentRef {
    pub preview_id: String,
    pub agent_id: String,
    pub repo_id: String,
    pub workspace_id: String,
    pub spec_path: String,
    pub branch: String,
    /// Commit the preview branch was created from — the diff base. Pinned at
    /// provisioning so a moving default branch can never show up in the diff
    /// as someone else's deletions.
    pub base_sha: String,
    pub worktree_path: String,
    /// True once the token is revoked and the budget slot released. Makes the
    /// release idempotent across process-exit, DELETE and the GC sweep.
    #[serde(default)]
    pub released: bool,
}

/// Persisted preview run (kv namespace `meta_spec_previews`, key = preview_id).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PreviewRecord {
    pub preview_id: String,
    /// Caller identity, for audit. Deliberately NOT used as a scope: run scope
    /// is always derived from the repo/workspace records (AGENTS.md — never
    /// fabricate or reuse a caller-supplied scope identity).
    pub requested_by: Option<String>,
    /// Unix seconds of creation — the GC horizon.
    pub created_at: u64,
    pub agents: Vec<PreviewAgentRef>,
}

// ---------------------------------------------------------------------------
// GET /api/v1/workspaces/{id}/meta-spec-set
// ---------------------------------------------------------------------------

pub async fn get_meta_spec_set(
    State(state): State<Arc<AppState>>,
    _auth: AuthenticatedAgent,
    Path(workspace_id): Path<String>,
) -> Result<Json<MetaSpecSet>, ApiError> {
    // Verify workspace exists.
    let ws = state
        .workspaces
        .find_by_id(&Id::new(&workspace_id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("workspace '{workspace_id}' not found")))?;

    // §8 inheritance: the workspace's EFFECTIVE set — tenant defaults merged
    // with the workspace's own overrides (workspace pin wins on conflict).
    // A workspace with no set of its own inherits the tenant defaults.
    let set = crate::reconciliation::resolve_effective_meta_spec_set(&state, &ws).await?;
    Ok(Json(set))
}

// ---------------------------------------------------------------------------
// PUT /api/v1/workspaces/{id}/meta-spec-set  (Admin only)
// ---------------------------------------------------------------------------

pub async fn put_meta_spec_set(
    State(state): State<Arc<AppState>>,
    Path(workspace_id): Path<String>,
    auth: AuthenticatedAgent,
    Json(req): Json<UpdateMetaSpecSetRequest>,
) -> Result<(StatusCode, Json<MetaSpecSet>), ApiError> {
    // Admin-only: meta-spec-set bindings are governance controls that determine
    // which personas, principles, standards, and processes govern all agents in a
    // workspace. Allowing non-Admin callers (Developers, Agents) to modify these
    // bindings would let agents rewrite the rules they operate under (NEW-26).
    if !auth.roles.contains(&gyre_domain::UserRole::Admin) {
        crate::rbac::record_rbac_denied(
            &state,
            &auth,
            "Admin",
            "PUT /api/v1/workspaces/:id/meta-spec-set",
        )
        .await;
        return Err(ApiError::Forbidden(
            "only Admin role may update workspace meta-spec-set bindings".to_string(),
        ));
    }

    // Verify workspace exists.
    let ws_record = state
        .workspaces
        .find_by_id(&Id::new(&workspace_id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("workspace '{workspace_id}' not found")))?;

    // Load the OLD set before upsert — the diff between old and new pinned
    // SHAs is the reconciliation trigger (meta-spec-reconciliation.md §6:
    // "Reconciliation is triggered when a workspace's meta-spec set is
    // updated to reference a new version of a meta-spec").
    let old_set = match state.meta_spec_sets.get(&Id::new(&workspace_id)).await? {
        Some(json) => serde_json::from_str::<MetaSpecSet>(&json)
            .map_err(|e| ApiError::Internal(anyhow::anyhow!("corrupt meta_spec_set: {e}")))?,
        None => MetaSpecSet {
            workspace_id: workspace_id.clone(),
            ..Default::default()
        },
    };

    let set = MetaSpecSet {
        workspace_id: workspace_id.clone(),
        personas: req.personas,
        principles: req.principles,
        standards: req.standards,
        process: req.process,
    };

    let json = serde_json::to_string(&set)
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("serialize meta_spec_set: {e}")))?;
    state
        .meta_spec_sets
        .upsert(&Id::new(&workspace_id), &json)
        .await?;

    // §6: only version changes (new or re-pinned entries) trigger
    // reconciliation — an identical re-PUT is a no-op, and `run_reconciliation`
    // itself skips specs that are not yet Approved (a Pending bind is not a
    // rollout; the approval transition owns that wave).
    let changed_paths = crate::reconciliation::diff_meta_spec_set(&old_set, &set);
    if !changed_paths.is_empty() {
        // §11 MetaSpecSetUpdated: workspace meta-spec set binding changed.
        crate::reconciliation::emit_meta_spec_set_updated(
            &state,
            &Id::new(&workspace_id),
            &changed_paths,
        )
        .await;
        let summary = crate::reconciliation::run_reconciliation(
            &state,
            &Id::new(&workspace_id),
            &changed_paths,
        )
        .await;
        tracing::info!(
            workspace_id = %workspace_id,
            changed = changed_paths.len(),
            created = summary.tasks_created,
            skipped = summary.tasks_skipped,
            "meta-spec-set update triggered reconciliation"
        );
    }

    // Respond with the EFFECTIVE set (§8 inheritance): tenant defaults
    // merged with the just-written workspace overrides. The stored workspace
    // set is exactly `set`; the response shows what is now active.
    let effective =
        crate::reconciliation::resolve_effective_meta_spec_set(&state, &ws_record).await?;
    Ok((StatusCode::OK, Json(effective)))
}

// ---------------------------------------------------------------------------
// GET /api/v1/tenants/{id}/meta-specs/set
// ---------------------------------------------------------------------------

pub async fn get_tenant_meta_spec_set(
    State(state): State<Arc<AppState>>,
    Path(tenant_id): Path<String>,
    auth: AuthenticatedAgent,
) -> Result<Json<TenantMetaSpecSet>, ApiError> {
    // Tenant governance config, same authorization rule as tenant trust
    // anchors: Admin role + tenant containment (the caller's tenant scope
    // must match the path — a tenant admin manages only their own defaults).
    if !auth.roles.contains(&gyre_domain::UserRole::Admin) {
        return Err(ApiError::Forbidden(
            "only Admin role may view tenant meta-spec-set bindings".to_string(),
        ));
    }
    if auth.tenant_id != tenant_id {
        return Err(ApiError::Forbidden(
            "cannot access meta-spec-set for a different tenant".to_string(),
        ));
    }

    state
        .tenants
        .find_by_id(&Id::new(&tenant_id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("tenant '{tenant_id}' not found")))?;

    let set = match state
        .tenant_meta_spec_sets
        .get(&Id::new(&tenant_id))
        .await?
    {
        Some(json) => serde_json::from_str::<TenantMetaSpecSet>(&json).map_err(|e| {
            ApiError::Internal(anyhow::anyhow!("corrupt tenant_meta_spec_set: {e}"))
        })?,
        None => TenantMetaSpecSet {
            tenant_id: tenant_id.clone(),
            ..Default::default()
        },
    };
    Ok(Json(set))
}

// ---------------------------------------------------------------------------
// PUT /api/v1/tenants/{id}/meta-specs/set  (Admin only, tenant-scoped)
// ---------------------------------------------------------------------------

pub async fn put_tenant_meta_spec_set(
    State(state): State<Arc<AppState>>,
    Path(tenant_id): Path<String>,
    auth: AuthenticatedAgent,
    Json(req): Json<UpdateTenantMetaSpecSetRequest>,
) -> Result<(StatusCode, Json<TenantMetaSpecSet>), ApiError> {
    // Admin-only + tenant containment: the tenant set is the default binding
    // every workspace in the tenant inherits (§8) — the same NEW-26 governance
    // rule as the workspace set, scoped to the caller's own tenant.
    if !auth.roles.contains(&gyre_domain::UserRole::Admin) {
        return Err(ApiError::Forbidden(
            "only Admin role may update tenant meta-spec-set bindings".to_string(),
        ));
    }
    if auth.tenant_id != tenant_id {
        return Err(ApiError::Forbidden(
            "cannot update meta-spec-set for a different tenant".to_string(),
        ));
    }

    state
        .tenants
        .find_by_id(&Id::new(&tenant_id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("tenant '{tenant_id}' not found")))?;

    // Load the OLD set before upsert — the diff between old and new pinned
    // SHAs is the tenant propagation trigger (§8 step 5: the tenant binding
    // update is what rolls the change out to inheriting workspaces).
    let old_set = match state
        .tenant_meta_spec_sets
        .get(&Id::new(&tenant_id))
        .await?
    {
        Some(json) => serde_json::from_str::<TenantMetaSpecSet>(&json).map_err(|e| {
            ApiError::Internal(anyhow::anyhow!("corrupt tenant_meta_spec_set: {e}"))
        })?,
        None => TenantMetaSpecSet {
            tenant_id: tenant_id.clone(),
            ..Default::default()
        },
    };

    let set = TenantMetaSpecSet {
        tenant_id: tenant_id.clone(),
        personas: req.personas,
        principles: req.principles,
        standards: req.standards,
        process: req.process,
    };

    let json = serde_json::to_string(&set)
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("serialize tenant_meta_spec_set: {e}")))?;
    state
        .tenant_meta_spec_sets
        .upsert(&Id::new(&tenant_id), &json)
        .await?;

    // §8: changed pins propagate to every workspace in the tenant that does
    // not override them (each workspace then reconciles autonomously per its
    // own rollout policy — run_reconciliation). `force` (tenant-admin
    // override) bypasses the workspace-level overrides for urgent changes.
    let changed_paths = crate::reconciliation::diff_tenant_meta_spec_set(&old_set, &set);
    if !changed_paths.is_empty() {
        crate::reconciliation::emit_tenant_meta_spec_set_updated(
            &state,
            &Id::new(&tenant_id),
            &changed_paths,
        )
        .await;
        let summary = crate::reconciliation::run_tenant_reconciliation(
            &state,
            &Id::new(&tenant_id),
            &changed_paths,
            req.force,
        )
        .await;
        tracing::info!(
            tenant_id = %tenant_id,
            changed = changed_paths.len(),
            force = req.force,
            workspaces_scanned = summary.workspaces_scanned,
            workspaces_skipped_override = summary.workspaces_skipped_override,
            workspaces_skipped_error = summary.workspaces_skipped_error,
            created = summary.tasks_created,
            skipped = summary.tasks_skipped,
            "tenant meta-spec-set update triggered reconciliation"
        );
    }

    Ok((StatusCode::OK, Json(set)))
}

// ---------------------------------------------------------------------------
// GET /api/v1/workspaces/{id}/meta-specs/rollout-policy  (§7)
// ---------------------------------------------------------------------------

pub async fn get_rollout_policy(
    State(state): State<Arc<AppState>>,
    _auth: AuthenticatedAgent,
    Path(workspace_id): Path<String>,
) -> Result<Json<gyre_domain::RolloutPolicy>, ApiError> {
    // Verify workspace exists.
    state
        .workspaces
        .find_by_id(&Id::new(&workspace_id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("workspace '{workspace_id}' not found")))?;

    // Unset policy → spec default (§7: Rolling / Normal / auto_merge false).
    let policy =
        crate::reconciliation::get_workspace_rollout_policy(&state, &Id::new(&workspace_id)).await;
    Ok(Json(policy))
}

// ---------------------------------------------------------------------------
// PUT /api/v1/workspaces/{id}/meta-specs/rollout-policy  (Admin only, §7)
// ---------------------------------------------------------------------------

pub async fn put_rollout_policy(
    State(state): State<Arc<AppState>>,
    Path(workspace_id): Path<String>,
    auth: AuthenticatedAgent,
    Json(req): Json<UpdateRolloutPolicyRequest>,
) -> Result<(StatusCode, Json<gyre_domain::RolloutPolicy>), ApiError> {
    // Admin-only: the rollout policy governs how reconciliation proceeds for
    // the whole workspace — same governance class as meta-spec-set bindings
    // (NEW-26: non-Admin callers, including agents, must not be able to
    // rewrite the rules that govern their own rollout).
    if !auth.roles.contains(&gyre_domain::UserRole::Admin) {
        crate::rbac::record_rbac_denied(
            &state,
            &auth,
            "Admin",
            "PUT /api/v1/workspaces/:id/meta-specs/rollout-policy",
        )
        .await;
        return Err(ApiError::Forbidden(
            "only Admin role may update workspace rollout policy".to_string(),
        ));
    }

    // Verify workspace exists.
    state
        .workspaces
        .find_by_id(&Id::new(&workspace_id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("workspace '{workspace_id}' not found")))?;

    // Merge with the current (or default) policy so omitted fields keep
    // their present values.
    let current =
        crate::reconciliation::get_workspace_rollout_policy(&state, &Id::new(&workspace_id)).await;

    let strategy = gyre_domain::RolloutStrategy::parse(&req.strategy).ok_or_else(|| {
        ApiError::InvalidInput(format!(
            "unknown strategy '{}': expected immediate, rolling, or background",
            req.strategy
        ))
    })?;
    let priority = match req.priority.as_deref() {
        None => current.priority,
        Some(p) => gyre_domain::ReconciliationPriority::parse(p).ok_or_else(|| {
            ApiError::InvalidInput(format!(
                "unknown priority '{p}': expected high, normal, or low"
            ))
        })?,
    };
    let max_concurrent_agents = req
        .max_concurrent_agents
        .unwrap_or(current.max_concurrent_agents);
    if max_concurrent_agents == 0 {
        return Err(ApiError::InvalidInput(
            "max_concurrent_agents must be at least 1 — a rolling rollout with batch size 0 \
             would never create a task"
                .to_string(),
        ));
    }

    let policy = gyre_domain::RolloutPolicy {
        strategy,
        max_concurrent_agents,
        priority,
        auto_merge: req.auto_merge.unwrap_or(current.auto_merge),
    };

    let json = serde_json::to_string(&policy)
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("serialize rollout_policy: {e}")))?;
    state
        .rollout_policies
        .upsert(&Id::new(&workspace_id), &json)
        .await?;

    // §11-style observability: policy change is an auditable workspace event.
    state
        .emit_event(
            Id::new(workspace_id.clone()),
            gyre_common::message::Destination::Workspace(Id::new(workspace_id.clone())),
            gyre_common::message::MessageKind::Custom("rollout_policy_updated".to_string()),
            Some(serde_json::json!({
                "workspace_id": workspace_id,
                "strategy": policy.strategy.as_str(),
                "max_concurrent_agents": policy.max_concurrent_agents,
                "priority": policy.priority.as_str(),
                "auto_merge": policy.auto_merge,
            })),
        )
        .await;

    Ok((StatusCode::OK, Json(policy)))
}

// ---------------------------------------------------------------------------
// GET /api/v1/meta-specs/{path}/blast-radius
// ---------------------------------------------------------------------------

pub async fn get_meta_spec_blast_radius(
    State(state): State<Arc<AppState>>,
    _auth: AuthenticatedAgent,
    Path(spec_path): Path<String>,
) -> Json<BlastRadiusResponse> {
    let mut affected_workspaces: Vec<AffectedWorkspace> = Vec::new();
    let mut affected_repos: Vec<AffectedRepo> = Vec::new();

    // List all workspaces and check each one's EFFECTIVE meta-spec-set (§8:
    // tenant defaults merged with workspace overrides) for a pinned entry
    // referencing spec_path. A workspace with no set of its own is affected
    // when its tenant's set pins the path (tenant inheritance); a workspace
    // whose own set pins the path is affected via its own binding.
    let workspaces = state.workspaces.list().await.unwrap_or_default();
    for workspace in &workspaces {
        let ws_id = workspace.id.as_str();
        let own_set = state
            .meta_spec_sets
            .get(&workspace.id)
            .await
            .ok()
            .flatten()
            .and_then(|json| serde_json::from_str::<MetaSpecSet>(&json).ok());
        let tenant_set = state
            .tenant_meta_spec_sets
            .get(&workspace.tenant_id)
            .await
            .ok()
            .flatten()
            .and_then(|json| serde_json::from_str::<TenantMetaSpecSet>(&json).ok());

        if own_set.is_none() && tenant_set.is_none() {
            continue;
        }

        let effective = crate::reconciliation::merge_meta_spec_sets(
            tenant_set.as_ref().unwrap_or(&TenantMetaSpecSet::default()),
            own_set.as_ref().unwrap_or(&MetaSpecSet::default()),
        );
        let references_spec = effective.personas.values().any(|e| e.path == spec_path)
            || effective.principles.iter().any(|e| e.path == spec_path)
            || effective.standards.iter().any(|e| e.path == spec_path)
            || effective.process.iter().any(|e| e.path == spec_path);

        if references_spec {
            // Own binding wins the reason label: a workspace that pins the
            // path itself is affected by its own binding; otherwise the
            // affectation flows from the tenant default.
            let own_references = own_set
                .as_ref()
                .map(|s| {
                    s.personas.values().any(|e| e.path == spec_path)
                        || s.principles.iter().any(|e| e.path == spec_path)
                        || s.standards.iter().any(|e| e.path == spec_path)
                        || s.process.iter().any(|e| e.path == spec_path)
                })
                .unwrap_or(false);
            let reason = if own_references {
                "workspace_binding"
            } else {
                "tenant_inheritance"
            };

            affected_workspaces.push(AffectedWorkspace {
                id: ws_id.to_string(),
            });

            // Collect repos bound to this workspace via the database.
            let repos = state
                .repos
                .list_by_workspace(&Id::new(ws_id))
                .await
                .unwrap_or_default();
            for repo in &repos {
                affected_repos.push(AffectedRepo {
                    id: repo.id.to_string(),
                    workspace_id: ws_id.to_string(),
                    reason: reason.to_string(),
                });
            }
        }
    }

    Json(BlastRadiusResponse {
        spec_path,
        affected_repos,
        affected_workspaces,
    })
}

// ---------------------------------------------------------------------------
// Preview mode (meta-spec-reconciliation.md §5)
//
// One draft meta-spec, N throwaway agents, N throwaway branches. Everything a
// preview touches is deleted again: branch, worktree, token, budget slot, kv
// record. Nothing reaches the durable ledger — no task, no MR, no merge queue,
// no quality gates, no provenance, no `refs/agents/*` / `refs/tasks/*` writes.
// ---------------------------------------------------------------------------

/// kv namespace for preview runs, keyed by preview_id.
const PREVIEW_NS: &str = "meta_spec_previews";
/// kv namespace for preview agent records, keyed by agent_id.
const PREVIEW_AGENTS_NS: &str = "preview_agents";
/// All preview branches live under this prefix, so teardown can never name a
/// real branch.
pub(crate) const PREVIEW_BRANCH_PREFIX: &str = "preview";

/// Preview run lifetime before the GC sweep tears it down
/// (`GYRE_META_SPEC_PREVIEW_TTL_HOURS`, default 24 h).
pub fn preview_ttl_secs_from_env() -> u64 {
    positive_env("GYRE_META_SPEC_PREVIEW_TTL_HOURS", 24).saturating_mul(3600)
}

/// TTL of the JWTs minted for preview agents (`GYRE_PREVIEW_JWT_TTL_SECS`,
/// default 1800 s). Separate from `GYRE_AGENT_JWT_TTL` on purpose: a preview
/// agent must not outlive the run it was spawned for.
pub fn preview_jwt_ttl_secs_from_env() -> u64 {
    positive_env("GYRE_PREVIEW_JWT_TTL_SECS", 1800)
}

/// Interval between GC sweeps (`GYRE_META_SPEC_PREVIEW_GC_INTERVAL_SECS`).
pub fn preview_gc_interval_secs_from_env() -> u64 {
    positive_env("GYRE_META_SPEC_PREVIEW_GC_INTERVAL_SECS", 3600)
}

/// Preview concurrency cap (`GYRE_PREVIEW_BUDGET_MAX_CONCURRENT`). `None` =
/// the target workspaces' own budgets govern preview runs.
pub fn preview_budget_max_concurrent_from_env() -> Option<u64> {
    std::env::var("GYRE_PREVIEW_BUDGET_MAX_CONCURRENT")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|v| *v > 0)
}

fn positive_env(key: &str, default: u64) -> u64 {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(default)
}

/// A target after validation: the repo and workspace records it resolved to.
/// Later phases use these instead of re-trusting the caller's `repo_id`.
struct ResolvedTarget {
    repo: Repository,
    workspace: Workspace,
    spec_path: String,
}

/// A provisioned preview agent plus its minted token. The token deliberately
/// lives outside [`PreviewAgentRef`] so it can never leak into a response.
struct Provisioned {
    agent_ref: PreviewAgentRef,
    token: String,
    agent: Agent,
    repo: Repository,
    workspace: Workspace,
}

/// Role gate for every preview entry point: a preview run spends real compute
/// against real specs, so read-only and agent-role callers are excluded.
/// Denials are recorded through the RBAC audit trail like every other gate.
async fn require_preview_role(
    state: &AppState,
    auth: &AuthenticatedAgent,
    route: &str,
) -> Result<(), ApiError> {
    if auth.roles.contains(&UserRole::Admin) || auth.roles.contains(&UserRole::Developer) {
        Ok(())
    } else {
        crate::rbac::record_rbac_denied(state, auth, "Admin or Developer", route).await;
        Err(ApiError::Forbidden(
            "Admin or Developer role required to run meta-spec preview".to_string(),
        ))
    }
}

// ---------------------------------------------------------------------------
// POST /api/v1/meta-specs/preview  (Admin or Developer)
// ---------------------------------------------------------------------------

pub async fn post_meta_spec_preview(
    State(state): State<Arc<AppState>>,
    auth: AuthenticatedAgent,
    Json(req): Json<PreviewMetaSpecRequest>,
) -> Result<(StatusCode, Json<PreviewResponse>), ApiError> {
    require_preview_role(&state, &auth, "POST /api/v1/meta-specs/preview").await?;
    validate_preview_request(&req)?;
    let resolved = resolve_preview_targets(&state, &auth, &req.targets, None).await?;
    spawn_preview_run(&state, &auth, &req.draft, resolved).await
}

// ---------------------------------------------------------------------------
// POST /api/v1/workspaces/{id}/meta-specs/preview  (Admin or Developer)
// ---------------------------------------------------------------------------

/// Workspace-scoped twin of the global preview route: same body, same §5
/// machinery, one extra containment rule — every target repo must live in the
/// path's workspace, and that workspace must be inside the caller's tenant.
/// The run's scope still comes from the repo records (never the path alone):
/// each target passes the same repo ABAC and tenant checks as the global
/// route, so the workspace segment narrows, never replaces, authorization.
pub async fn post_workspace_meta_spec_preview(
    State(state): State<Arc<AppState>>,
    Path(workspace_id): Path<String>,
    auth: AuthenticatedAgent,
    Json(req): Json<PreviewMetaSpecRequest>,
) -> Result<(StatusCode, Json<PreviewResponse>), ApiError> {
    require_preview_role(
        &state,
        &auth,
        "POST /api/v1/workspaces/:id/meta-specs/preview",
    )
    .await?;
    let workspace = load_workspace_for_caller(&state, &auth, &workspace_id).await?;
    validate_preview_request(&req)?;
    let resolved = resolve_preview_targets(&state, &auth, &req.targets, Some(&workspace)).await?;
    spawn_preview_run(&state, &auth, &req.draft, resolved).await
}

/// Load the workspace named by a workspace-scoped preview path and contain it
/// to the caller: unknown ids are NotFound, and a workspace in another tenant
/// is Forbidden for every non-Admin caller.
async fn load_workspace_for_caller(
    state: &AppState,
    auth: &AuthenticatedAgent,
    workspace_id: &str,
) -> Result<Workspace, ApiError> {
    let workspace = state
        .workspaces
        .find_by_id(&Id::new(workspace_id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("workspace '{workspace_id}' not found")))?;
    if !auth.roles.contains(&UserRole::Admin) && workspace.tenant_id.as_str() != auth.tenant_id {
        return Err(ApiError::Forbidden(format!(
            "workspace '{workspace_id}' is outside the caller's tenant"
        )));
    }
    Ok(workspace)
}

/// Request validation shared by both POST surfaces: the draft must name a
/// known meta-spec kind with non-empty content, and at least one target.
fn validate_preview_request(req: &PreviewMetaSpecRequest) -> Result<(), ApiError> {
    if req.draft.kind.trim().is_empty() || req.draft.content.trim().is_empty() {
        return Err(ApiError::BadRequest(
            "draft.kind and draft.content are both required".to_string(),
        ));
    }
    // Kind-shape validation (documented as 400 for unknown shapes): the
    // preview runner treats the draft as one of the four meta-spec kinds, so
    // an arbitrary string would silently ride through as an uninterpretable
    // governing context.
    parse_kind(req.draft.kind.trim())?;
    if req.targets.is_empty() {
        return Err(ApiError::BadRequest(
            "targets must not be empty".to_string(),
        ));
    }
    Ok(())
}

/// Pass 1 of both POST surfaces: resolve, authorize and validate every
/// target. Nothing is created before this completes, so a rejected target
/// cannot leave an agent, branch or worktree behind. With `workspace_scope`
/// set (the workspace-scoped route), every target repo must additionally
/// live inside that workspace.
async fn resolve_preview_targets(
    state: &AppState,
    auth: &AuthenticatedAgent,
    targets: &[PreviewTarget],
    workspace_scope: Option<&Workspace>,
) -> Result<Vec<ResolvedTarget>, ApiError> {
    let mut resolved: Vec<ResolvedTarget> = Vec::with_capacity(targets.len());
    for target in targets {
        let repo = state
            .repos
            .find_by_id(&Id::new(&target.repo_id))
            .await?
            .ok_or_else(|| ApiError::NotFound(format!("repo '{}' not found", target.repo_id)))?;
        let workspace = state
            .workspaces
            .find_by_id(&repo.workspace_id)
            .await?
            .ok_or_else(|| {
                ApiError::NotFound(format!("workspace '{}' not found", repo.workspace_id))
            })?;
        // Workspace-scoped route: the path's workspace must own every target.
        if let Some(scope) = workspace_scope {
            if repo.workspace_id != scope.id {
                return Err(ApiError::Forbidden(format!(
                    "repo '{}' is not in workspace '{}'",
                    repo.id, scope.id
                )));
            }
        }
        // The run's scope is derived from the repo record and the caller's
        // identity — never from the request body or the path alone.
        crate::abac::check_repo_abac(state, &repo.id.to_string(), auth)
            .await
            .map_err(ApiError::Forbidden)?;
        if !auth.roles.contains(&UserRole::Admin) && workspace.tenant_id.as_str() != auth.tenant_id
        {
            return Err(ApiError::Forbidden(format!(
                "repo '{}' is outside the caller's tenant",
                repo.id
            )));
        }
        // The target spec must exist on the default branch: the agent is told to
        // read it, so a typo would burn a whole spawn to produce nothing.
        match state
            .git_ops
            .read_file(&repo.path, &repo.default_branch, &target.spec_path)
            .await
        {
            Ok(Some(_)) => {}
            Ok(None) => {
                return Err(ApiError::BadRequest(format!(
                    "spec path '{}' does not exist in repo '{}' on branch '{}'",
                    target.spec_path, repo.id, repo.default_branch
                )))
            }
            Err(e) => {
                return Err(ApiError::BadRequest(format!(
                    "cannot read spec path '{}' from repo '{}': {e:#}",
                    target.spec_path, repo.id
                )))
            }
        }
        resolved.push(ResolvedTarget {
            repo,
            workspace,
            spec_path: target.spec_path.clone(),
        });
    }
    Ok(resolved)
}

/// Passes 2-4 of both POST surfaces: budget, provision, launch. Shared so the
/// workspace-scoped route runs the exact same §5 machinery (preview budget,
/// per-workspace spawn budget, rollback-on-failure, persist-before-launch)
/// as the global route — only the target-resolution scope differs.
async fn spawn_preview_run(
    state: &Arc<AppState>,
    auth: &AuthenticatedAgent,
    draft: &DraftMetaSpec,
    resolved: Vec<ResolvedTarget>,
) -> Result<(StatusCode, Json<PreviewResponse>), ApiError> {
    // ── Pass 2: budget.
    let requested = resolved.len() as u64;
    if let Some(max) = state.preview_budget_max_concurrent {
        // Fail closed on the enumeration: a kv-store failure means the number
        // of running preview agents is unknown, and admitting the POST would
        // fail the concurrency cap open.
        let running = count_running_preview_agents(state).await.map_err(|e| {
            ApiError::Internal(anyhow::anyhow!(
                "cannot enumerate preview agents for the budget check: {e:#}"
            ))
        })?;
        if running.saturating_add(requested) > max {
            return Err(ApiError::TooManyRequests(format!(
                "preview budget exhausted: {running} preview agent(s) running, max_concurrent={max}, requested={requested}"
            )));
        }
    } else {
        // Unset: the target workspaces' own budgets govern (server-config.md).
        // Reserve every target's slot up front, grouped per workspace: a
        // single POST with N targets must not admit against a workspace whose
        // max_concurrent_agents < N just because the per-target check ran
        // before any increment landed.
        let mut per_workspace: HashMap<String, u32> = HashMap::new();
        for rt in &resolved {
            *per_workspace
                .entry(rt.workspace.id.as_str().to_string())
                .or_insert(0) += 1;
        }
        for (ws_id, requested) in per_workspace {
            super::budget::check_spawn_budget_for(state, &ws_id, requested)
                .await
                .map_err(ApiError::TooManyRequests)?;
        }
    }

    // ── Pass 3: provision. A failure rolls back everything provisioned so far.
    let preview_id = uuid::Uuid::new_v4().to_string();
    let now = now_secs();
    let mut used_slugs: HashMap<String, u32> = HashMap::new();
    let mut provisioned: Vec<Provisioned> = Vec::with_capacity(resolved.len());
    for rt in &resolved {
        let branch = preview_branch_name(&preview_id, &rt.spec_path, &mut used_slugs);
        match provision_preview_agent(state, auth, &preview_id, rt, &branch, now).await {
            Ok(p) => provisioned.push(p),
            Err(e) => {
                for p in &provisioned {
                    rollback_preview_agent(state, &p.agent_ref).await;
                }
                return Err(e);
            }
        }
    }

    // Persist before launching: a fast agent can exit — and report — before the
    // first launch call returns. A persist failure rolls back every provisioned
    // agent (same compensation as a provision failure): without it, N Active
    // agent rows, live tokens, held budget slots, branches and worktrees would
    // be orphaned with no preview record and no preview_agents index entries —
    // invisible to the GC orphan pass, which iterates that index.
    let record = PreviewRecord {
        preview_id: preview_id.clone(),
        requested_by: Some(auth.agent_id.clone()),
        created_at: now,
        agents: provisioned.iter().map(|p| p.agent_ref.clone()).collect(),
    };
    if let Err(e) = persist_preview(state, &record).await {
        for p in &provisioned {
            rollback_preview_agent(state, &p.agent_ref).await;
        }
        // A partial persist may have stored the run record before an index
        // write failed — drop it too, or a record whose agents were all
        // rolled back lingers until the GC sweep.
        if let Err(rm_err) = state.kv_store.kv_remove(PREVIEW_NS, &preview_id).await {
            tracing::warn!(
                preview_id = %preview_id,
                "failed to drop partially-persisted preview record: {rm_err}"
            );
        }
        return Err(e);
    }

    tracing::info!(
        preview_id = %preview_id,
        draft_kind = %draft.kind,
        agents = provisioned.len(),
        requested_by = %auth.agent_id,
        "meta-spec preview started"
    );

    // ── Pass 4: launch. A target whose process never starts is reported failed
    // and its slot released; the rest of the run still proceeds.
    for p in provisioned.iter_mut() {
        if let Err(reason) = launch_preview_agent(state, draft, p).await {
            tracing::error!(
                preview_id = %preview_id,
                agent_id = %p.agent_ref.agent_id,
                "preview agent launch failed: {reason}"
            );
            mark_preview_agent_failed(state, p).await;
        }
    }

    let agents = provisioned
        .iter()
        .map(|p| PreviewAgentEntry {
            agent_id: p.agent_ref.agent_id.clone(),
            repo_id: p.agent_ref.repo_id.clone(),
            spec_path: p.agent_ref.spec_path.clone(),
            branch: p.agent_ref.branch.clone(),
        })
        .collect();

    Ok((
        StatusCode::ACCEPTED,
        Json(PreviewResponse { preview_id, agents }),
    ))
}

// ---------------------------------------------------------------------------
// GET /api/v1/meta-specs/preview/{preview_id}
// ---------------------------------------------------------------------------

pub async fn get_meta_spec_preview_status(
    State(state): State<Arc<AppState>>,
    Path(preview_id): Path<String>,
    auth: AuthenticatedAgent,
) -> Result<Json<PreviewStatusResponse>, ApiError> {
    require_preview_role(&state, &auth, "GET /api/v1/meta-specs/preview/:preview_id").await?;
    let record = load_preview(&state, &preview_id).await?;
    require_preview_scope(&state, &auth, &record).await?;
    Ok(preview_status_response(&state, &record).await)
}

// ---------------------------------------------------------------------------
// GET /api/v1/workspaces/{id}/meta-specs/preview/{preview_id}
// ---------------------------------------------------------------------------

/// Workspace-scoped twin of the global status route: the run is visible only
/// through the workspace that owns its target repos — every repo of the run
/// must resolve inside the path's workspace (else NotFound), the workspace
/// must be inside the caller's tenant, and the same per-repo ABAC policy
/// checks as the global route apply.
pub async fn get_workspace_meta_spec_preview_status(
    State(state): State<Arc<AppState>>,
    Path((workspace_id, preview_id)): Path<(String, String)>,
    auth: AuthenticatedAgent,
) -> Result<Json<PreviewStatusResponse>, ApiError> {
    require_preview_role(
        &state,
        &auth,
        "GET /api/v1/workspaces/:id/meta-specs/preview/:preview_id",
    )
    .await?;
    let workspace = load_workspace_for_caller(&state, &auth, &workspace_id).await?;
    let record = load_preview(&state, &preview_id).await?;
    require_preview_in_workspace(&state, &record, &workspace).await?;
    require_preview_scope(&state, &auth, &record).await?;
    Ok(preview_status_response(&state, &record).await)
}

/// Build the status body shared by both GET surfaces: per-agent state derived
/// from the real agent records plus the produced branch diff.
async fn preview_status_response(
    state: &AppState,
    record: &PreviewRecord,
) -> Json<PreviewStatusResponse> {
    let mut agents = Vec::with_capacity(record.agents.len());
    let mut running = false;
    for agent_ref in &record.agents {
        let status = preview_agent_status(state, agent_ref).await;
        if status == "running" {
            running = true;
        }
        agents.push(PreviewAgentStatus {
            agent_id: agent_ref.agent_id.clone(),
            repo_id: agent_ref.repo_id.clone(),
            spec_path: agent_ref.spec_path.clone(),
            branch: agent_ref.branch.clone(),
            diff: preview_diff(state, agent_ref).await,
            status,
        });
    }

    Json(PreviewStatusResponse {
        preview_id: record.preview_id.clone(),
        created_at: record.created_at,
        ttl_seconds: state.preview_ttl_secs,
        state: if running { "running" } else { "complete" }.to_string(),
        agents,
    })
}

// ---------------------------------------------------------------------------
// DELETE /api/v1/meta-specs/preview/{preview_id}
// ---------------------------------------------------------------------------

pub async fn delete_meta_spec_preview(
    State(state): State<Arc<AppState>>,
    Path(preview_id): Path<String>,
    auth: AuthenticatedAgent,
) -> Result<StatusCode, ApiError> {
    require_preview_role(
        &state,
        &auth,
        "DELETE /api/v1/meta-specs/preview/:preview_id",
    )
    .await?;
    let record = load_preview(&state, &preview_id).await?;
    require_preview_scope(&state, &auth, &record).await?;
    teardown_preview(&state, &record).await;
    tracing::info!(
        preview_id = %record.preview_id,
        agents = record.agents.len(),
        requested_by = %auth.agent_id,
        "meta-spec preview torn down"
    );
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// DELETE /api/v1/workspaces/{id}/meta-specs/preview/{preview_id}
// ---------------------------------------------------------------------------

/// Workspace-scoped twin of the global teardown route: same scope rules as
/// the workspace status route (workspace containment, then per-repo ABAC),
/// then the same teardown of agents, branches, worktrees and the record.
pub async fn delete_workspace_meta_spec_preview(
    State(state): State<Arc<AppState>>,
    Path((workspace_id, preview_id)): Path<(String, String)>,
    auth: AuthenticatedAgent,
) -> Result<StatusCode, ApiError> {
    require_preview_role(
        &state,
        &auth,
        "DELETE /api/v1/workspaces/:id/meta-specs/preview/:preview_id",
    )
    .await?;
    let workspace = load_workspace_for_caller(&state, &auth, &workspace_id).await?;
    let record = load_preview(&state, &preview_id).await?;
    require_preview_in_workspace(&state, &record, &workspace).await?;
    require_preview_scope(&state, &auth, &record).await?;
    teardown_preview(&state, &record).await;
    tracing::info!(
        preview_id = %record.preview_id,
        workspace_id = %workspace.id,
        agents = record.agents.len(),
        requested_by = %auth.agent_id,
        "meta-spec preview torn down via workspace route"
    );
    Ok(StatusCode::NO_CONTENT)
}

/// Containment for the workspace-scoped status/teardown routes: the run is
/// addressable only through a workspace that owns every one of its target
/// repos. A repo deleted after the run started is skipped (the run is
/// already heading for teardown, mirroring [`require_preview_scope`]); a
/// resolvable repo outside the workspace makes the run NotFound in this
/// workspace's scope.
async fn require_preview_in_workspace(
    state: &AppState,
    record: &PreviewRecord,
    workspace: &Workspace,
) -> Result<(), ApiError> {
    let mut checked: Vec<String> = Vec::new();
    for agent_ref in &record.agents {
        if checked.iter().any(|r| r == &agent_ref.repo_id) {
            continue;
        }
        checked.push(agent_ref.repo_id.clone());
        let repo = match state.repos.find_by_id(&Id::new(&agent_ref.repo_id)).await? {
            Some(repo) => repo,
            None => continue,
        };
        if repo.workspace_id != workspace.id {
            return Err(ApiError::NotFound(format!(
                "preview '{}' not found in workspace '{}'",
                record.preview_id, workspace.id
            )));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Process-exit path
// ---------------------------------------------------------------------------

/// Completion path for a preview agent, called by every compute-target monitor
/// through `on_agent_process_exit`.
///
/// Returns true when `agent_id` is a preview agent, so the caller skips the
/// normal Active→Idle transition: a preview agent lands in `Stopped` with its
/// token revoked and its budget slot released — never `Idle`, because an Idle
/// preview agent could be re-targeted onto real work.
///
/// The throwaway worktree goes with it. The branch stays: the diff a human came
/// here to read lives on that branch until DELETE or the GC sweep removes it.
pub(crate) async fn finish_preview_agent(state: &AppState, agent_id: &str) -> bool {
    let raw = match state.kv_store.kv_get(PREVIEW_AGENTS_NS, agent_id).await {
        Ok(Some(raw)) => raw,
        Ok(None) => return false,
        Err(e) => {
            // Cannot tell what this agent is, so do not claim it: the caller
            // applies the normal transition, and the sweep still owns cleanup.
            tracing::warn!(
                agent_id,
                "preview agent lookup failed; treating as a normal agent: {e:#}"
            );
            return false;
        }
    };
    // message-bus.md §Delivery Guarantees (Agent completion cleanup): the
    // process exit IS the preview agent's completion (REST/MCP `complete` is
    // refused for preview agents — skip-ceremony), so bulk-ack any unacked
    // Directed messages in its inbox: retained for audit, never redelivered,
    // reclaimable by expire_acked_inboxes. Best-effort like every other
    // terminal path. Placed before the terminal flip so it runs on both the
    // normal and the corrupt-record branches below.
    crate::api::spawn::acknowledge_agent_inbox(state, &Id::new(agent_id), "agent_completed").await;
    let agent_ref = match serde_json::from_str::<PreviewAgentRef>(&raw) {
        Ok(a) => a,
        Err(e) => {
            // Our own record is unreadable. Still terminal-ize the agent: an
            // agent we cannot classify must not be handed real work.
            tracing::error!(agent_id, "preview agent record is corrupt: {e}");
            stop_preview_agent_row(state, agent_id).await;
            return true;
        }
    };

    tracing::info!(
        preview_id = %agent_ref.preview_id,
        agent_id,
        branch = %agent_ref.branch,
        "preview agent finished"
    );
    // Cleanup before the terminal status flip: the status row is the
    // observable "this agent is done" signal (status polling, GC, dashboards).
    // Making it the LAST mutation guarantees that a terminal row implies a
    // cleaned-up agent — no worktree, no live token, no held budget slot —
    // instead of a window where the row says Stopped but the cleanup is
    // still in flight.
    revoke_preview_token(state, agent_id).await;
    remove_preview_worktree(state, &agent_ref).await;
    release_preview_slot(state, &agent_ref).await;
    stop_preview_agent_row(state, agent_id).await;
    true
}

/// True when `agent_id` belongs to a preview run.
///
/// Fails CLOSED on a store error: every caller of this function is a
/// refusal gate (REST ceremony guards, MCP dispatch denials), so during a
/// kv outage the refusals must keep firing — returning `false` would let
/// every preview agent's calls silently pass while the store is down.
/// `finish_preview_agent`, which performs cleanup rather than refusal,
/// reads the store directly and handles `Err` explicitly.
pub(crate) async fn is_preview_agent(state: &AppState, agent_id: &str) -> bool {
    match state.kv_store.kv_get(PREVIEW_AGENTS_NS, agent_id).await {
        Ok(Some(_)) => true,
        Ok(None) => false,
        Err(e) => {
            tracing::warn!(
                agent_id,
                "preview registry unavailable; refusing the call as a preview agent (fail closed): {e:#}"
            );
            true
        }
    }
}

/// True when the caller authenticated via a Gyre-minted agent JWT whose
/// agent id belongs to a preview run. Gyre-minted agent JWTs serialize the
/// fine-grained `scope` claim as a NON-EMPTY JSON ARRAY
/// (`AgentJwtClaims.scope: Vec<String>`, populated by `mint_with_workload`
/// with `repo:{name}:write` — see auth.rs §1 Token Scoping), so the array
/// form is the discriminator; the legacy `scope: "agent"` string form is
/// accepted for tokens minted before §1 (mirrors `mcp.rs is_agent_jwt` and
/// `search_scope.rs is_agent_jwt`). Global tokens, API keys and Keycloak
/// JWTs never satisfy either form: they are not the preview agent itself —
/// and even a foreign array-scoped JWT falls through to the preview-agent
/// registry lookup below, which is keyed by Gyre-generated agent ids.
pub(crate) async fn caller_is_preview_agent(
    state: &AppState,
    auth: &crate::auth::AuthenticatedAgent,
) -> bool {
    let Some(scope) = auth.jwt_claims.as_ref().and_then(|c| c.get("scope")) else {
        return false;
    };
    let agent_jwt = match scope {
        serde_json::Value::Array(items) => !items.is_empty(),
        serde_json::Value::String(s) => s == "agent",
        _ => false,
    };
    agent_jwt && is_preview_agent(state, &auth.agent_id).await
}

// ---------------------------------------------------------------------------
// Background GC sweep
// ---------------------------------------------------------------------------

/// Expire preview runs past their TTL (`GYRE_META_SPEC_PREVIEW_TTL_HOURS`) and
/// drop agent records whose run is already gone. Registered as the
/// `meta_spec_preview_gc` job.
pub async fn run_once(state: &AppState) -> anyhow::Result<()> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let ttl = state.preview_ttl_secs;

    for (key, raw) in state.kv_store.kv_list(PREVIEW_NS).await? {
        let record = match serde_json::from_str::<PreviewRecord>(&raw) {
            Ok(r) => r,
            Err(e) => {
                // Nothing can address this run any longer; drop the pointer and
                // let the orphan pass below reclaim its agents.
                tracing::error!(preview_id = %key, "preview record is corrupt: {e}");
                if let Err(rm_err) = state.kv_store.kv_remove(PREVIEW_NS, &key).await {
                    tracing::warn!(
                        preview_id = %key,
                        "failed to drop corrupt preview record: {rm_err}"
                    );
                }
                continue;
            }
        };
        // saturating: a record stamped in the future (clock skew) must not
        // expire instantly.
        let age = now.saturating_sub(record.created_at);
        if age < ttl {
            continue;
        }
        tracing::info!(
            preview_id = %record.preview_id,
            age_secs = age,
            ttl_secs = ttl,
            agents = record.agents.len(),
            "expiring meta-spec preview run"
        );
        teardown_preview(state, &record).await;
    }

    // Orphan pass: an agent record whose run no longer exists would otherwise
    // pin a budget slot and hold a live token forever.
    for (agent_id, raw) in state.kv_store.kv_list(PREVIEW_AGENTS_NS).await? {
        let agent_ref = match serde_json::from_str::<PreviewAgentRef>(&raw) {
            Ok(a) => a,
            Err(e) => {
                tracing::error!(agent_id, "preview agent record is corrupt: {e}");
                stop_preview_agent_row(state, &agent_id).await;
                revoke_preview_token(state, &agent_id).await;
                if let Err(rm_err) = state.kv_store.kv_remove(PREVIEW_AGENTS_NS, &agent_id).await {
                    tracing::warn!(
                        agent_id,
                        "failed to drop corrupt preview agent record: {rm_err}"
                    );
                }
                continue;
            }
        };
        let run_gone = !matches!(
            state
                .kv_store
                .kv_get(PREVIEW_NS, &agent_ref.preview_id)
                .await,
            Ok(Some(_))
        );
        if !run_gone {
            continue;
        }
        tracing::warn!(
            preview_id = %agent_ref.preview_id,
            agent_id,
            "preview agent record has no run record; reclaiming it"
        );
        stop_preview_agent_row(state, &agent_id).await;
        revoke_preview_token(state, &agent_id).await;
        remove_preview_worktree(state, &agent_ref).await;
        delete_preview_branch(state, &agent_ref).await;
        release_preview_slot(state, &agent_ref).await;
        if let Err(rm_err) = state.kv_store.kv_remove(PREVIEW_AGENTS_NS, &agent_id).await {
            tracing::warn!(
                agent_id,
                "failed to drop orphaned preview agent record: {rm_err}"
            );
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Preview helpers
// ---------------------------------------------------------------------------

/// Load a preview run record.
async fn load_preview(state: &AppState, preview_id: &str) -> Result<PreviewRecord, ApiError> {
    let raw = state
        .kv_store
        .kv_get(PREVIEW_NS, preview_id)
        .await
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("preview lookup: {e:#}")))?
        .ok_or_else(|| ApiError::NotFound(format!("preview '{preview_id}' not found")))?;
    serde_json::from_str(&raw).map_err(|e| {
        ApiError::Internal(anyhow::anyhow!(
            "preview record '{preview_id}' is corrupt: {e}"
        ))
    })
}

/// Write the run record plus its per-agent index entries.
async fn persist_preview(state: &AppState, record: &PreviewRecord) -> Result<(), ApiError> {
    let json = serde_json::to_string(record)
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("serialize preview record: {e}")))?;
    state
        .kv_store
        .kv_set(PREVIEW_NS, &record.preview_id, json)
        .await
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("store preview record: {e:#}")))?;
    for agent_ref in &record.agents {
        let raw = serde_json::to_string(agent_ref).map_err(|e| {
            ApiError::Internal(anyhow::anyhow!("serialize preview agent record: {e}"))
        })?;
        state
            .kv_store
            .kv_set(PREVIEW_AGENTS_NS, &agent_ref.agent_id, raw)
            .await
            .map_err(|e| {
                ApiError::Internal(anyhow::anyhow!("store preview agent record: {e:#}"))
            })?;
    }
    Ok(())
}

/// Read-modify-write a run record so terminal flags land on whatever the store
/// currently holds rather than on a stale in-memory copy.
async fn mutate_preview<F>(state: &AppState, preview_id: &str, mutate: F)
where
    F: FnOnce(&mut PreviewRecord),
{
    let mut record = match load_preview(state, preview_id).await {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(
                preview_id,
                "preview record unavailable during update: {e:?}"
            );
            return;
        }
    };
    mutate(&mut record);
    if let Err(e) = persist_preview(state, &record).await {
        tracing::error!(preview_id, "failed to persist preview update: {e:?}");
    }
}

/// Containment for status/teardown: the run's own repos define its scope, so
/// every distinct repo must pass the caller's ABAC policy and tenant check.
/// A repo deleted after the run started cannot be ABAC-checked, but the
/// scope must not silently vanish with it — the agent refs carry the
/// workspace the run was provisioned in (captured from the real repo at
/// spawn time, never caller-supplied), so the tenant check still anchors a
/// run whose every target repo was deleted. Without this, deleting all of a
/// run's repos would leave it readable and tearable by any Developer in any
/// tenant. An unresolvable workspace is denied rather than ridden on an
/// assumed identity.
async fn require_preview_scope(
    state: &AppState,
    auth: &AuthenticatedAgent,
    record: &PreviewRecord,
) -> Result<(), ApiError> {
    let mut checked: Vec<String> = Vec::new();
    for agent_ref in &record.agents {
        if checked.iter().any(|r| r == &agent_ref.repo_id) {
            continue;
        }
        checked.push(agent_ref.repo_id.clone());
        match state.repos.find_by_id(&Id::new(&agent_ref.repo_id)).await? {
            Some(repo) => {
                crate::abac::check_repo_abac(state, &repo.id.to_string(), auth)
                    .await
                    .map_err(ApiError::Forbidden)?;
                if auth.roles.contains(&UserRole::Admin) {
                    continue;
                }
                match state.workspaces.find_by_id(&repo.workspace_id).await? {
                    Some(ws) if ws.tenant_id.as_str() == auth.tenant_id => {}
                    Some(_) => {
                        return Err(ApiError::Forbidden(format!(
                            "preview '{}' is outside the caller's tenant",
                            record.preview_id
                        )))
                    }
                    None => {
                        return Err(ApiError::Forbidden(format!(
                            "cannot determine the workspace scope of preview '{}'",
                            record.preview_id
                        )))
                    }
                }
            }
            None => {
                // Repo deleted mid-run: no ABAC policy can be evaluated
                // against a repo that no longer exists, so the recorded
                // workspace's tenant is the remaining scope anchor. Admins
                // stay cross-tenant by design; every other role must match
                // the run's recorded tenant.
                if auth.roles.contains(&UserRole::Admin) {
                    continue;
                }
                match state
                    .workspaces
                    .find_by_id(&Id::new(&agent_ref.workspace_id))
                    .await?
                {
                    Some(ws) if ws.tenant_id.as_str() == auth.tenant_id => {}
                    Some(_) => {
                        return Err(ApiError::Forbidden(format!(
                            "preview '{}' is outside the caller's tenant",
                            record.preview_id
                        )))
                    }
                    None => {
                        return Err(ApiError::Forbidden(format!(
                            "cannot determine the workspace scope of preview '{}'",
                            record.preview_id
                        )))
                    }
                }
            }
        }
    }
    Ok(())
}

/// Preview agents are `Active` ledger rows with a running process. Counted from
/// the per-agent index so the query is bounded by preview runs, not by all
/// agents. Fails closed: an enumeration error is surfaced to the caller so the
/// budget admission check rejects the POST instead of admitting unboundedly
/// against an unknown running count.
async fn count_running_preview_agents(state: &AppState) -> anyhow::Result<u64> {
    let entries = state.kv_store.kv_list(PREVIEW_AGENTS_NS).await?;
    let mut count = 0u64;
    for (agent_id, raw) in entries {
        // Only unreleased slots are live: a finished agent still has an index
        // entry (it is the process-exit discriminator) but holds no slot.
        if serde_json::from_str::<PreviewAgentRef>(&raw)
            .map(|a| a.released)
            .unwrap_or(true)
        {
            continue;
        }
        if let Ok(Some(a)) = state.agents.find_by_id(&Id::new(&agent_id)).await {
            if a.status == AgentStatus::Active {
                count += 1;
            }
        }
    }
    Ok(count)
}

/// Map a ledger status + the terminal `released` flag to the wire status.
fn preview_agent_status_label(status: AgentStatus, finished: bool) -> &'static str {
    match status {
        AgentStatus::Active => "running",
        AgentStatus::Failed => "failed",
        AgentStatus::Dead => "dead",
        // Preview agents are never parked in Idle: completion lands them in
        // Stopped with the slot released. Stopped/Idle without the flag was
        // stopped by something other than a finished run.
        AgentStatus::Idle | AgentStatus::Stopped if finished => "complete",
        AgentStatus::Idle | AgentStatus::Stopped => "stopped",
    }
}

async fn preview_agent_status(state: &AppState, agent_ref: &PreviewAgentRef) -> String {
    match state.agents.find_by_id(&Id::new(&agent_ref.agent_id)).await {
        Ok(Some(agent)) => preview_agent_status_label(agent.status, agent_ref.released).to_string(),
        // Reporting-only fallback: the agent row is gone (or unreadable), so the
        // honest answer is "unknown". No safety decision reads this value.
        Ok(None) => "unknown".to_string(),
        Err(e) => {
            tracing::warn!(agent_id = %agent_ref.agent_id, "agent status lookup failed: {e:#}");
            "unknown".to_string()
        }
    }
}

/// Diff of the preview branch against the commit it was created from — `None`
/// while the branch does not exist yet (or the repo/branch cannot be read).
async fn preview_diff(state: &AppState, agent_ref: &PreviewAgentRef) -> Option<DiffResult> {
    let repo = state
        .repos
        .find_by_id(&Id::new(&agent_ref.repo_id))
        .await
        .ok()
        .flatten()?;
    if !state
        .git_ops
        .branch_exists(&repo.path, &agent_ref.branch)
        .await
        .ok()?
    {
        return None;
    }
    match state
        .git_ops
        .diff(&repo.path, &agent_ref.base_sha, &agent_ref.branch)
        .await
    {
        Ok(diff) => Some(diff),
        Err(e) => {
            tracing::warn!(
                preview_id = %agent_ref.preview_id,
                branch = %agent_ref.branch,
                "preview diff unavailable: {e:#}"
            );
            None
        }
    }
}

/// `preview/{preview_id}/{slug}` — slug from the spec file stem, sanitized for
/// git. Duplicate stems inside one request are disambiguated with the full
/// path (`system-search.md` vs `specs/system/search.md`).
fn preview_branch_name(
    preview_id: &str,
    spec_path: &str,
    used: &mut HashMap<String, u32>,
) -> String {
    let stem = std::path::Path::new(spec_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("spec")
        .to_string();
    let mut slug = sanitize_branch_slug(&stem);
    if used.contains_key(&slug) {
        let qualified = sanitize_branch_slug(
            spec_path
                .trim_start_matches('/')
                .rsplit_once('.')
                .map(|(head, _)| head)
                .unwrap_or(spec_path),
        );
        slug = if qualified.is_empty() {
            slug
        } else {
            qualified
        };
    }
    let n = used.entry(slug.clone()).or_insert(0);
    *n += 1;
    if *n > 1 {
        slug = format!("{slug}-{}", *n - 1);
    }
    format!("{PREVIEW_BRANCH_PREFIX}/{preview_id}/{slug}")
}

fn sanitize_branch_slug(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .map(|c| {
            if c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_' | '-') {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let cleaned = cleaned.replace("..", "-");
    let trimmed = cleaned.trim_matches(|c| c == '-' || c == '.').to_string();
    if trimmed.is_empty() {
        return "spec".to_string();
    }
    // git refuses refs ending in `.lock`.
    if trimmed.ends_with(".lock") {
        return format!("{trimmed}x");
    }
    trimmed
}

/// Create the agent row, its token, the throwaway branch and worktree. No
/// process yet, and no budget slot taken until every step succeeded — so a
/// failure here needs no budget rollback.
async fn provision_preview_agent(
    state: &AppState,
    auth: &AuthenticatedAgent,
    preview_id: &str,
    target: &ResolvedTarget,
    branch: &str,
    now: u64,
) -> Result<Provisioned, ApiError> {
    // non-atomic-create:ok - the agent row is written before the worktree row
    // across two stores with no cross-repo transaction, but every failure
    // path after the first create routes through rollback_preview_agent,
    // which deletes the partially-provisioned agent row, worktree rows,
    // branch and index entry — and the caller applies the same rollback to
    // every already-provisioned target when a later step (provision or
    // persist_preview) fails. The compensation is proven by the
    // launch-failure and persist-failure tests. A create failure has no row
    // to roll back, but the branch already exists — it is deleted inline
    // below so it cannot orphan outside every GC pass (the orphan sweep
    // iterates the preview_agents index, which never saw a row that failed
    // to persist).
    let repo = &target.repo;

    // Pin the branch to the default-branch tip so the diff a human reads is
    // exactly this agent's work: a default branch that moves afterwards can
    // never show up as someone else's deletions. An empty repo has nothing to
    // preview — there is no spec content to implement.
    let base_sha = crate::git_refs::resolve_ref(&repo.path, &repo.default_branch)
        .await
        .ok_or_else(|| {
            ApiError::BadRequest(format!(
                "repo '{}' has no commits on '{}' — push an initial commit before previewing",
                repo.id, repo.default_branch
            ))
        })?;

    state
        .git_ops
        .create_branch(&repo.path, branch, &base_sha)
        .await
        .map_err(|e| {
            ApiError::Internal(anyhow::anyhow!(
                "create preview branch '{branch}' in repo '{}': {e:#}",
                repo.id
            ))
        })?;

    let slug = branch.rsplit('/').next().unwrap_or("preview").to_string();
    let mut agent = Agent::new(new_id(), format!("preview-{preview_id}-{slug}"), now);
    agent.spawned_by = Some(auth.agent_id.clone());
    // Real scope: `Agent::new`'s placeholder workspace must never survive — an
    // unscoped agent rides whatever identity a downstream consumer invents.
    agent.workspace_id = repo.workspace_id.clone();
    agent
        .transition_status(AgentStatus::Active)
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("preview agent status: {e}")))?;
    if let Err(e) = state.agents.create(&agent).await {
        // No agent row exists, so nothing routes through
        // rollback_preview_agent — but the branch was already created. Delete
        // it here or it orphans with no record: the GC orphan pass iterates
        // the preview_agents kv index, which has no entry for a row that
        // never persisted.
        if let Err(be) = state.git_ops.delete_branch(&repo.path, branch).await {
            tracing::warn!(
                repo_id = %repo.id,
                branch,
                "orphaned preview branch cleanup after agent-row create failed: {be:#}"
            );
        }
        return Err(ApiError::Internal(e));
    }

    // Throwaway branch checked out into its own worktree — the branch already
    // exists, so `create_worktree` checks it out instead of branching from HEAD.
    let worktree_path = format!("{}/worktrees/{}", repo.path, branch.replace('/', "-"));
    let worktree_path_for_log = worktree_path.clone();
    if let Err(e) = state
        .git_ops
        .create_worktree(&repo.path, &worktree_path, branch)
        .await
    {
        let mut partial = preview_agent_ref(
            preview_id,
            &agent,
            repo,
            &target.spec_path,
            branch,
            base_sha,
            worktree_path,
        );
        // No slot was taken: increment_active_agents runs only after every
        // provisioning step succeeded. A false flag here would make the
        // rollback decrement a workspace/tenant counter that was never
        // incremented — permanently biasing budget enforcement down and
        // letting later spawns over-admit past max_concurrent_agents.
        partial.released = true;
        rollback_preview_agent(state, &partial).await;
        let msg = e.to_string().to_lowercase();
        if msg.contains("not a valid object") || msg.contains("bad default revision") {
            return Err(ApiError::BadRequest(format!(
                "cannot create preview worktree: repo '{}' has no commits yet (branch: {branch})",
                repo.id
            )));
        }
        return Err(ApiError::Internal(anyhow::anyhow!(
            "create preview worktree '{worktree_path_for_log}': {e:#}"
        )));
    }

    // task_id: None — a preview agent has no task, and no task row is created
    // to give it one.
    let wt = AgentWorktree::new(
        new_id(),
        agent.id.clone(),
        repo.id.clone(),
        None,
        branch,
        worktree_path.clone(),
        now,
    );
    if let Err(e) = state.worktrees.create(&wt).await {
        let mut partial = preview_agent_ref(
            preview_id,
            &agent,
            repo,
            &target.spec_path,
            branch,
            base_sha,
            worktree_path,
        );
        partial.released = true; // no slot was taken
        rollback_preview_agent(state, &partial).await;
        return Err(ApiError::Internal(e));
    }

    // Short-lived token: a preview agent must not outlive its run. The preview
    // id takes the task slot of the claim set — there is no task to name.
    // Scope claims are derived from the loaded workspace/repo rows, never
    // caller-supplied (§1 Token Scoping).
    let token = match state.agent_signing_key.mint(
        &agent.id.to_string(),
        preview_id,
        &auth.agent_id,
        &state.base_url,
        state.preview_jwt_ttl_secs,
        target.workspace.tenant_id.as_str(),
        target.workspace.id.as_str(),
        repo.id.as_str(),
        &repo.name,
    ) {
        Ok(token) => token,
        Err(e) => {
            let mut partial = preview_agent_ref(
                preview_id,
                &agent,
                repo,
                &target.spec_path,
                branch,
                base_sha,
                worktree_path,
            );
            partial.released = true;
            rollback_preview_agent(state, &partial).await;
            return Err(ApiError::Internal(anyhow::anyhow!(
                "mint preview agent JWT: {e}"
            )));
        }
    };
    if let Err(e) = state
        .kv_store
        .kv_set("agent_tokens", &agent.id.to_string(), token.clone())
        .await
    {
        let mut partial = preview_agent_ref(
            preview_id,
            &agent,
            repo,
            &target.spec_path,
            branch,
            base_sha,
            worktree_path,
        );
        partial.released = true;
        rollback_preview_agent(state, &partial).await;
        return Err(ApiError::Internal(anyhow::anyhow!(
            "store preview agent token: {e:#}"
        )));
    }

    // Preview agents are real processes: count them like any other for
    // workspace/tenant occupancy, while the preview cap gates admission.
    super::budget::increment_active_agents(state, repo.workspace_id.as_str()).await;

    Ok(Provisioned {
        agent_ref: preview_agent_ref(
            preview_id,
            &agent,
            repo,
            &target.spec_path,
            branch,
            base_sha,
            worktree_path,
        ),
        token,
        agent,
        repo: repo.clone(),
        workspace: target.workspace.clone(),
    })
}

#[allow(clippy::too_many_arguments)]
fn preview_agent_ref(
    preview_id: &str,
    agent: &Agent,
    repo: &Repository,
    spec_path: &str,
    branch: &str,
    base_sha: String,
    worktree_path: String,
) -> PreviewAgentRef {
    PreviewAgentRef {
        preview_id: preview_id.to_string(),
        agent_id: agent.id.to_string(),
        repo_id: repo.id.to_string(),
        workspace_id: repo.workspace_id.to_string(),
        spec_path: spec_path.to_string(),
        branch: branch.to_string(),
        base_sha,
        worktree_path,
        released: false,
    }
}

/// Launch a provisioned preview agent on its compute target. `Err` carries the
/// reason; the caller marks the agent failed and releases its slot.
async fn launch_preview_agent(
    state: &Arc<AppState>,
    draft: &DraftMetaSpec,
    p: &mut Provisioned,
) -> Result<(), String> {
    // No compute target in the preview body: resolve the workspace assignment,
    // then the tenant default, exactly like a normal spawn.
    let ct_entity: Option<ComputeTargetEntity> =
        if let Some(ct_id) = p.workspace.compute_target_id.clone() {
            state.compute_targets.get_by_id(&ct_id).await.ok().flatten()
        } else {
            state
                .compute_targets
                .get_default_for_tenant(&p.workspace.tenant_id)
                .await
                .ok()
                .flatten()
        };
    let target_type = ct_entity
        .as_ref()
        .map(|e| match e.target_type {
            ComputeTargetType::Container => "container",
            ComputeTargetType::Ssh => "ssh",
            ComputeTargetType::Kubernetes => "kubernetes",
        })
        .unwrap_or("local")
        .to_string();
    let resolved_target_config = ct_entity.map(|e| super::compute::ComputeTargetConfig {
        id: e.id.to_string(),
        name: e.name.clone(),
        target_type: target_type.clone(),
        config: e.config.clone(),
    });

    // The draft travels out-of-band, in the environment only. `GYRE_TASK_ID` is
    // deliberately absent — a preview agent has no task.
    let mut extra_env = HashMap::new();
    extra_env.insert(
        "GYRE_PREVIEW_ID".to_string(),
        p.agent_ref.preview_id.clone(),
    );
    extra_env.insert("GYRE_META_SPEC_DRAFT_KIND".to_string(), draft.kind.clone());
    extra_env.insert(
        "GYRE_META_SPEC_DRAFT_CONTENT".to_string(),
        draft.content.clone(),
    );
    extra_env.insert(
        "GYRE_TARGET_SPEC_PATH".to_string(),
        p.agent_ref.spec_path.clone(),
    );

    let clone_url = super::spawn::build_clone_url(state, Some(&p.workspace), &p.repo);
    let outcome = super::spawn::launch_taskless_agent_process(super::spawn::AgentLaunchParams {
        state,
        agent: &p.agent,
        repo: &p.repo,
        workspace: Some(&p.workspace),
        resolved_target_config,
        token: &p.token,
        branch: &p.agent_ref.branch,
        repo_id: &p.agent_ref.repo_id,
        worktree_path: &p.agent_ref.worktree_path,
        task_id: None,
        clone_url: &clone_url,
        extra_env,
    })
    .await
    .map_err(|e| format!("{e:?}"))?;

    // `launch_taskless_agent_process` treats a failed process spawn as
    // best-effort: it logs and returns Ok. A preview whose process never
    // started would sit "running" until the GC sweep, so require an actual
    // launch. The dispatch's own `launched` flag is the authority — never
    // the handle's shape: SshDocker (ssh container_mode, the dispatch
    // default) and Kubernetes handles carry `pid: None` by design (the id
    // is a remote container / Pod name), so a pid-based guess reports every
    // successful remote launch as "never started", kills nothing, and
    // orphans the running remote process.
    if !outcome.launched {
        return Err(format!("{target_type} process never started"));
    }
    Ok(())
}

/// Launch failure: terminal agent, revoked token, released slot, worktree gone.
/// The run record survives so the failed agent is still visible in the status.
async fn mark_preview_agent_failed(state: &AppState, p: &mut Provisioned) {
    // Defense-in-depth: if anything registered a process for this agent
    // (a partially-completed dispatch, a stale registry entry), kill it
    // through the recorded handle/backend pair — the same routing as
    // DELETE and GC — BEFORE reclaiming the row, token and worktree.
    // Without this, a failure verdict on a launch that did leave a remote
    // container/Pod behind would orphan it with no agent pointing at it
    // — exactly what the pre-fix path did, which performed no kill at
    // all. No-op when nothing is registered.
    kill_preview_process(state, &p.agent_ref.agent_id).await;
    if let Ok(Some(mut agent)) = state.agents.find_by_id(&p.agent.id).await {
        if agent.status == AgentStatus::Active {
            // transition_status can only fail on an invalid transition; log
            // rather than discard so a stuck row is visible in operations.
            if let Err(te) = agent.transition_status(AgentStatus::Failed) {
                tracing::warn!(
                    agent_id = %p.agent.id,
                    "preview agent failure transition rejected: {te:?}"
                );
            }
            if let Err(ue) = state.agents.update(&agent).await {
                tracing::warn!(
                    agent_id = %p.agent.id,
                    "preview agent failure status write failed: {ue:#}"
                );
            }
        }
    }
    revoke_preview_token(state, &p.agent_ref.agent_id).await;
    remove_preview_worktree(state, &p.agent_ref).await;
    // Routed through the durable-guarded release: a generic terminal path
    // (stop/fail route, admin kill, budget stop) may have released this
    // slot between provisioning and the launch verdict — the snapshot flag
    // alone must not decide the decrement.
    release_preview_slot(state, &p.agent_ref).await;
    p.agent_ref.released = true;
}

/// Kill, unmount and forget every agent in a run, then drop its records.
/// Idempotent — safe from DELETE, the GC sweep and the POST rollback path.
pub(crate) async fn teardown_preview(state: &AppState, record: &PreviewRecord) {
    for agent_ref in &record.agents {
        teardown_preview_agent(state, agent_ref).await;
    }
    for agent_ref in &record.agents {
        let _ = state
            .kv_store
            .kv_remove(PREVIEW_AGENTS_NS, &agent_ref.agent_id)
            .await;
    }
    let _ = state
        .kv_store
        .kv_remove(PREVIEW_NS, &record.preview_id)
        .await;
}

/// Full teardown of one preview agent: process, worktree (before its branch —
/// git refuses to delete a checked-out branch), branch, token, budget slot.
async fn teardown_preview_agent(state: &AppState, agent_ref: &PreviewAgentRef) {
    kill_preview_process(state, &agent_ref.agent_id).await;
    // Stopped first: a monitor's late exit callback must not park it in Idle.
    stop_preview_agent_row(state, &agent_ref.agent_id).await;
    // Killed without completion (human DELETE or GC TTL expiry): same
    // orphaned-inbox handling as the stale-agent detector (message-bus.md
    // §Edge Cases). The kill removed the process-registry handle, so the
    // exit monitor will not run finish_preview_agent for this agent — the
    // ack must happen here or the inbox's unacked Directed messages are
    // redeliverable forever (expire_acked_inboxes only reclaims acked rows).
    crate::api::spawn::acknowledge_agent_inbox(
        state,
        &Id::new(&agent_ref.agent_id),
        "agent_orphaned",
    )
    .await;
    remove_preview_worktree(state, agent_ref).await;
    close_preview_branch_mrs(state, agent_ref).await;
    delete_preview_branch(state, agent_ref).await;
    revoke_preview_token(state, &agent_ref.agent_id).await;
    release_preview_slot(state, agent_ref).await;
}

/// Undo provisioning of an agent that never launched, for a POST being
/// rejected. Unlike teardown it also drops the agent row: nothing ever
/// referenced it, so leaving it would be ledger noise from a failed request.
/// Also drops the preview_agents index entry `persist_preview` may have
/// written before the request failed — otherwise the orphan would wait for
/// the hourly GC pass while holding its budget slot.
async fn rollback_preview_agent(state: &AppState, agent_ref: &PreviewAgentRef) {
    remove_preview_worktree(state, agent_ref).await;
    delete_preview_branch(state, agent_ref).await;
    revoke_preview_token(state, &agent_ref.agent_id).await;
    if let Err(e) = state
        .kv_store
        .kv_remove(PREVIEW_AGENTS_NS, &agent_ref.agent_id)
        .await
    {
        tracing::warn!(
            agent_id = %agent_ref.agent_id,
            "preview agent index cleanup failed: {e:#}"
        );
    }
    if let Ok(wts) = state
        .worktrees
        .find_by_agent(&Id::new(&agent_ref.agent_id))
        .await
    {
        for wt in wts {
            if let Err(e) = state.worktrees.delete(&wt.id).await {
                tracing::warn!(
                    worktree_id = %wt.id,
                    "preview rollback worktree row cleanup failed: {e:#}"
                );
            }
        }
    }
    if let Err(e) = state.agents.delete(&Id::new(&agent_ref.agent_id)).await {
        tracing::warn!(agent_id = %agent_ref.agent_id, "preview agent row cleanup failed: {e:#}");
    }
    // Same durable-guarded release: the snapshot's `released: true` covers
    // the no-slot-taken rollbacks, and the durable check covers a generic
    // terminal path that released the slot first.
    release_preview_slot(state, agent_ref).await;
}

/// Kill a preview agent's runtime through the backend recorded at spawn
/// time (`state.spawned_backends`) — the same invariant the admin kill path
/// (api/admin.rs) and the manifest gate teardown implement. Reconstructing
/// the target from the handle alone cannot reach two of the backends preview
/// agents launch on: an SshDocker handle's id is a remote container id that
/// `SshTarget::from_handle` refuses, and a Kubernetes handle carries no pid,
/// so a LocalTarget kill is a silent no-op while the Pod keeps running. Both
/// registry entries (handle and backend) are removed together so the exit
/// monitor's missing-handle break cannot leak the backend slot.
async fn kill_preview_process(state: &AppState, agent_id: &str) {
    let handle = state.process_registry.lock().await.remove(agent_id);
    let backend = state.spawned_backends.lock().await.remove(agent_id);
    match handle {
        Some(handle) => {
            let kill_result = match backend.as_deref() {
                Some(b) => gyre_ports::ComputeTarget::kill_process(b, &handle).await,
                // No recorded backend (pre-registration record): reconstruct
                // from the handle, which at least refuses malformed ssh ids
                // instead of killing an unrelated local pid.
                None => super::spawn::kill_process_handle(&handle).await,
            };
            if let Err(e) = kill_result {
                tracing::warn!(agent_id, "preview process kill failed: {e:#}");
            }
        }
        None => {
            // No registered handle: either it already exited, or it belongs to a
            // container/ssh target whose monitor owns the kill.
            tracing::debug!(agent_id, "no registered process handle for preview agent");
        }
    }
}

async fn stop_preview_agent_row(state: &AppState, agent_id: &str) {
    if let Ok(Some(mut agent)) = state.agents.find_by_id(&Id::new(agent_id)).await {
        if agent.status == AgentStatus::Active || agent.status == AgentStatus::Idle {
            // transition_status can only fail on an invalid transition; log
            // rather than discard so a stuck row is visible in operations.
            if let Err(te) = agent.transition_status(AgentStatus::Stopped) {
                tracing::warn!(agent_id, "preview agent stop transition rejected: {te:?}");
            }
            if let Err(ue) = state.agents.update(&agent).await {
                tracing::warn!(agent_id, "preview agent stop status write failed: {ue:#}");
            }
        }
    }
}

async fn revoke_preview_token(state: &AppState, agent_id: &str) {
    if let Err(e) = state.kv_store.kv_remove("agent_tokens", agent_id).await {
        tracing::warn!(agent_id, "preview token revocation failed: {e:#}");
    }
}

/// Force-remove the dirty throwaway worktree and unregister its rows. An
/// absent path is not an error: the caller asked for it to be gone.
async fn remove_preview_worktree(state: &AppState, agent_ref: &PreviewAgentRef) {
    if let Ok(Some(repo)) = state.repos.find_by_id(&Id::new(&agent_ref.repo_id)).await {
        if let Err(e) = state
            .git_ops
            .force_remove_worktree(&repo.path, &agent_ref.worktree_path)
            .await
        {
            tracing::warn!(
                preview_id = %agent_ref.preview_id,
                worktree = %agent_ref.worktree_path,
                "preview worktree removal failed: {e:#}"
            );
        }
    }
    if let Ok(wts) = state
        .worktrees
        .find_by_agent(&Id::new(&agent_ref.agent_id))
        .await
    {
        for wt in wts {
            if let Err(e) = state.worktrees.delete(&wt.id).await {
                tracing::warn!(
                    worktree_id = %wt.id,
                    "preview worktree row cleanup failed: {e:#}"
                );
            }
        }
    }
}

/// Close any open MR pointing at a preview branch. Preview runs create no
/// MRs and the REST/MCP surfaces refuse to create them for preview agents,
/// but a run torn down must not leave an open MR dangling off a branch that
/// no longer exists (belt-and-braces for any path that slipped past the
/// gates). `Closed` is terminal — already-closed MRs are left untouched.
async fn close_preview_branch_mrs(state: &AppState, agent_ref: &PreviewAgentRef) {
    let mrs = match state
        .merge_requests
        .list_by_repo(&Id::new(&agent_ref.repo_id))
        .await
    {
        Ok(mrs) => mrs,
        Err(e) => {
            tracing::warn!(
                preview_id = %agent_ref.preview_id,
                "MR listing failed during preview teardown: {e:#}"
            );
            return;
        }
    };
    for mut mr in mrs {
        if mr.source_branch != agent_ref.branch || mr.status == gyre_domain::MrStatus::Closed {
            continue;
        }
        if let Err(e) = mr.transition_status(gyre_domain::MrStatus::Closed) {
            tracing::warn!(
                preview_id = %agent_ref.preview_id,
                mr_id = %mr.id,
                "closing MR on preview branch failed: {e}"
            );
            continue;
        }
        mr.updated_at = now_secs();
        if let Err(e) = state.merge_requests.update(&mr).await {
            tracing::warn!(
                preview_id = %agent_ref.preview_id,
                mr_id = %mr.id,
                "MR close persist failed during preview teardown: {e:#}"
            );
        }
    }
}

async fn delete_preview_branch(state: &AppState, agent_ref: &PreviewAgentRef) {
    let repo = match state.repos.find_by_id(&Id::new(&agent_ref.repo_id)).await {
        Ok(Some(repo)) => repo,
        Ok(None) => return,
        Err(e) => {
            tracing::warn!(repo_id = %agent_ref.repo_id, "repo lookup failed during preview teardown: {e:#}");
            return;
        }
    };
    if let Err(e) = state
        .git_ops
        .delete_branch(&repo.path, &agent_ref.branch)
        .await
    {
        tracing::warn!(
            preview_id = %agent_ref.preview_id,
            branch = %agent_ref.branch,
            "preview branch deletion failed: {e:#}"
        );
    }
}

/// Release the budget slot exactly once across process-exit, DELETE and GC.
///
/// The caller's `agent_ref` snapshot may be stale: the generic terminal
/// paths — the stop/fail agent routes, the admin kill, budget enforcement
/// and the stale-agent detector — decrement the SAME workspace/tenant
/// counters for a preview agent and flag the record through
/// [`mark_preview_slot_released`]. The durable per-agent index, not the
/// snapshot, is therefore the release authority. A double release is not
/// recoverable: `decrement_active` is MAX(0, n-1), so each extra decrement
/// permanently biases the counter down and lets later spawns over-admit
/// past `max_concurrent_agents`.
async fn release_preview_slot(state: &AppState, agent_ref: &PreviewAgentRef) {
    if agent_ref.released || preview_slot_released(state, &agent_ref.agent_id).await {
        return;
    }
    super::budget::decrement_active_agents(state, &agent_ref.workspace_id).await;
    mark_preview_slot_released(state, &agent_ref.agent_id).await;
}

/// Read the durable `released` flag from the per-agent index — the
/// authority [`release_preview_slot`] re-checks before decrementing. A
/// missing or unreadable entry reads as unreleased (the caller then
/// releases, matching the pre-flag behavior); only an explicit `true`
/// suppresses the decrement.
async fn preview_slot_released(state: &AppState, agent_id: &str) -> bool {
    match state.kv_store.kv_get(PREVIEW_AGENTS_NS, agent_id).await {
        Ok(Some(raw)) => serde_json::from_str::<PreviewAgentRef>(&raw)
            .map(|a| a.released)
            .unwrap_or(false),
        _ => false,
    }
}

/// Flag a preview agent's budget slot as released WITHOUT decrementing it.
///
/// Called by the generic terminal paths — the stop/fail agent routes, the
/// admin kill, budget enforcement and the stale-agent detector — right
/// after they decremented the workspace/tenant counters themselves through
/// `budget::decrement_active_agents`. The durable flag is what keeps the
/// preview-owned release ([`release_preview_slot`], reached from process
/// exit, DELETE and the GC sweep) exactly-once: without it, the preview
/// path decrements the same slot a second time, and because
/// `decrement_active` is MAX(0, n-1) the over-release can never be earned
/// back — every occurrence permanently biases the counter down.
///
/// No-op for non-preview agents (no index entry) and already-released
/// slots. Best-effort on kv errors: the failure is logged, and the durable
/// re-check in `release_preview_slot` then still sees the stale flag —
/// one over-release under a failing kv store, never a silent one.
pub(crate) async fn mark_preview_slot_released(state: &AppState, agent_id: &str) {
    let raw = match state.kv_store.kv_get(PREVIEW_AGENTS_NS, agent_id).await {
        Ok(Some(raw)) => raw,
        Ok(None) => return, // not a preview agent — nothing to flag
        Err(e) => {
            tracing::warn!(agent_id, "cannot flag preview slot released: {e:#}");
            return;
        }
    };
    let mut agent_ref = match serde_json::from_str::<PreviewAgentRef>(&raw) {
        Ok(a) => a,
        Err(e) => {
            tracing::error!(
                agent_id,
                "preview agent record corrupt while flagging slot release: {e}"
            );
            return;
        }
    };
    if agent_ref.released {
        return;
    }
    agent_ref.released = true;
    match serde_json::to_string(&agent_ref) {
        Ok(json) => {
            if let Err(e) = state
                .kv_store
                .kv_set(PREVIEW_AGENTS_NS, agent_id, json)
                .await
            {
                tracing::warn!(agent_id, "preview slot flag persist failed: {e:#}");
            }
        }
        Err(e) => {
            tracing::error!(agent_id, "preview slot flag serialize failed: {e}");
        }
    }
    // Keep the run record's embedded copy in step: `mutate_preview`
    // rewrites the per-agent index from the record, so an unflagged record
    // entry would resurrect the stale flag on the next unrelated mutation.
    let preview_id = agent_ref.preview_id.clone();
    mutate_preview(state, &preview_id, |record| {
        if let Some(entry) = record.agents.iter_mut().find(|a| a.agent_id == agent_id) {
            entry.released = true;
        }
    })
    .await;
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::{
        kill_preview_process, mark_preview_agent_failed, new_id, now_secs, PreviewAgentRef,
        Provisioned,
    };
    use crate::mem::test_state;
    use axum::{body::Body, Router};
    use gyre_ports::ComputeTarget as _;
    use http::{Request, StatusCode};
    use tower::ServiceExt;

    fn app() -> Router {
        let state = test_state();
        crate::api::api_router().with_state(state)
    }

    async fn body_json(resp: axum::response::Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    /// Create a workspace through the REST surface (Admin token) and return
    /// its id — shared by the rollout-policy and preview tests.
    async fn create_workspace(app: &Router, name: &str) -> String {
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/workspaces")
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(format!(
                        r#"{{"name":"{name}","slug":"{name}"}}"#
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::CREATED,
            "workspace creation failed"
        );
        body_json(resp).await["id"].as_str().unwrap().to_string()
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn blast_radius_empty() {
        let resp = app()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/meta-specs/meta%2Fpersonas%2Fbackend.md/blast-radius")
                    .header("authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert!(json["affected_repos"].as_array().unwrap().is_empty());
        assert!(json["affected_workspaces"].as_array().unwrap().is_empty());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn put_meta_spec_set_requires_admin() {
        // A Developer-role JWT should be rejected with 403 (NEW-26 fix).
        use crate::abac_middleware::seed_builtin_policies;
        use crate::auth::test_helpers::{make_test_state_with_jwt, sign_test_jwt};
        let state = make_test_state_with_jwt();
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current()
                .block_on(seed_builtin_policies(&state))
                .expect("seed built-in policies")
        });

        // Create a workspace using the admin static token.
        let ws_resp = crate::api::api_router()
            .with_state(state.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/workspaces")
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"name":"ws26","slug":"ws26"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(ws_resp.status(), StatusCode::CREATED);
        let ws_json = body_json(ws_resp).await;
        let ws_id = ws_json["id"].as_str().unwrap().to_string();

        // Developer-role OIDC JWT.
        let dev_token = sign_test_jwt(
            &serde_json::json!({
                "tenant_id": "default",
                "sub": "dev-sub",
                "preferred_username": "developer-user",
                "realm_access": { "roles": ["developer"] }
            }),
            3600,
        );

        let resp = crate::api::api_router()
            .with_state(state)
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/workspaces/{ws_id}/meta-spec-set"))
                    .header("authorization", format!("Bearer {dev_token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"personas":{},"principles":[],"standards":[],"process":[]}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn meta_spec_set_not_found_for_unknown_workspace() {
        let resp = app()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/workspaces/00000000-0000-0000-0000-000000000000/meta-spec-set")
                    .header("authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    // -----------------------------------------------------------------------
    // Tenant meta-spec set endpoints (§8)
    // -----------------------------------------------------------------------

    #[tokio::test(flavor = "multi_thread")]
    async fn tenant_meta_spec_set_get_put_roundtrip() {
        let state = test_state();
        crate::bootstrap_default_tenant(&state).await;
        let app = crate::api::api_router().with_state(state.clone());

        // Empty default before anything is set.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/tenants/default/meta-specs/set")
                    .header("authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json["tenant_id"], "default");
        assert!(json["personas"].as_object().unwrap().is_empty());

        // PUT a tenant-level set.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/v1/tenants/default/meta-specs/set")
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "personas": { "worker": {"path": "backend-developer", "sha": "a1"} },
                            "principles": [{"path": "security", "sha": "s1"}],
                            "standards": [], "process": []
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // GET returns the stored set.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/tenants/default/meta-specs/set")
                    .header("authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json["personas"]["worker"]["path"], "backend-developer");
        assert_eq!(json["principles"][0]["path"], "security");

        // Persisted through the port (not an in-memory handler stand-in).
        let stored = state
            .tenant_meta_spec_sets
            .get(&gyre_common::Id::new("default"))
            .await
            .unwrap()
            .expect("tenant set must be persisted");
        assert!(stored.contains("backend-developer"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn tenant_meta_spec_set_requires_admin() {
        use crate::abac_middleware::seed_builtin_policies;
        use crate::auth::test_helpers::{make_test_state_with_jwt, sign_test_jwt};
        let state = make_test_state_with_jwt();
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current()
                .block_on(seed_builtin_policies(&state))
                .expect("seed built-in policies")
        });
        crate::bootstrap_default_tenant(&state).await;

        let dev_token = sign_test_jwt(
            &serde_json::json!({
                "sub": "dev-sub",
                "preferred_username": "developer-user",
                "realm_access": { "roles": ["developer"] }
            }),
            3600,
        );

        let resp = crate::api::api_router()
            .with_state(state)
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/v1/tenants/default/meta-specs/set")
                    .header("authorization", format!("Bearer {dev_token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "personas": {}, "principles": [], "standards": [], "process": []
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn tenant_meta_spec_set_tenant_containment() {
        let state = test_state();
        let app = crate::api::api_router().with_state(state.clone());

        // Create a second tenant (as the platform admin).
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/tenants")
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"name": "Other", "slug": "other"}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let other_id = body_json(resp).await["id"].as_str().unwrap().to_string();

        // The static admin token is scoped to tenant "default": writing
        // ANOTHER tenant's set must be forbidden (forged-scope containment).
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/tenants/{other_id}/meta-specs/set"))
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "personas": {}, "principles": [], "standards": [], "process": []
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);

        // GET is contained the same way.
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/tenants/{other_id}/meta-specs/set"))
                    .header("authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn tenant_meta_spec_set_unknown_tenant_404() {
        // The caller's own tenant id, but no tenant row exists (no
        // bootstrap) — containment passes, existence check 404s.
        let resp = app()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/tenants/default/meta-specs/set")
                    .header("authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    // -----------------------------------------------------------------------
    // Rollout policy endpoint tests (meta-spec-reconciliation.md §7)
    // -----------------------------------------------------------------------

    #[tokio::test(flavor = "multi_thread")]
    async fn get_rollout_policy_returns_spec_default() {
        let app = app();
        let ws_id = create_workspace(&app, "policy-default-ws").await;

        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/api/v1/workspaces/{ws_id}/meta-specs/rollout-policy"
                    ))
                    .header("authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        // §7: "Default: `Rolling` strategy, `Normal` priority, `auto_merge: false`".
        assert_eq!(json["strategy"], "rolling");
        assert_eq!(json["priority"], "normal");
        assert_eq!(json["auto_merge"], false);
        assert!(json["max_concurrent_agents"].as_u64().unwrap() >= 1);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn put_rollout_policy_round_trips() {
        let app = app();
        let ws_id = create_workspace(&app, "policy-put-ws").await;

        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/workspaces/{ws_id}/meta-specs/rollout-policy"))
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"strategy":"immediate","max_concurrent_agents":3,"priority":"high","auto_merge":true}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json["strategy"], "immediate");
        assert_eq!(json["max_concurrent_agents"], 3);
        assert_eq!(json["priority"], "high");
        assert_eq!(json["auto_merge"], true);

        // Read back through GET — the stored policy, not the request echo.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/api/v1/workspaces/{ws_id}/meta-specs/rollout-policy"
                    ))
                    .header("authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json["strategy"], "immediate");
        assert_eq!(json["max_concurrent_agents"], 3);
        assert_eq!(json["priority"], "high");
        assert_eq!(json["auto_merge"], true);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn put_rollout_policy_partial_update_keeps_other_fields() {
        let app = app();
        let ws_id = create_workspace(&app, "policy-partial-ws").await;

        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!(
                        "/api/v1/workspaces/{ws_id}/meta-specs/rollout-policy"
                    ))
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"strategy":"background"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        // Omitted fields fall back to the current (default) values.
        assert_eq!(json["strategy"], "background");
        assert_eq!(json["priority"], "normal");
        assert_eq!(json["auto_merge"], false);
        assert!(json["max_concurrent_agents"].as_u64().unwrap() >= 1);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn put_rollout_policy_requires_admin() {
        // A Developer-role JWT must be rejected with 403 — the rollout policy
        // governs reconciliation for the whole workspace (same governance
        // class as meta-spec-set bindings, NEW-26).
        use crate::abac_middleware::seed_builtin_policies;
        use crate::auth::test_helpers::{make_test_state_with_jwt, sign_test_jwt};
        let state = make_test_state_with_jwt();
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current()
                .block_on(seed_builtin_policies(&state))
                .expect("seed built-in policies")
        });

        let ws_resp = crate::api::api_router()
            .with_state(state.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/workspaces")
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"name":"policy-admin-ws","slug":"policy-admin-ws"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(ws_resp.status(), StatusCode::CREATED);
        let ws_id = body_json(ws_resp).await["id"].as_str().unwrap().to_string();

        let dev_token = sign_test_jwt(
            &serde_json::json!({
                "sub": "dev-policy-sub",
                "preferred_username": "policy-developer",
                "realm_access": { "roles": ["developer"] }
            }),
            3600,
        );

        let resp = crate::api::api_router()
            .with_state(state)
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!(
                        "/api/v1/workspaces/{ws_id}/meta-specs/rollout-policy"
                    ))
                    .header("authorization", format!("Bearer {dev_token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"strategy":"immediate"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn put_rollout_policy_rejects_invalid_values() {
        let app = app();
        let ws_id = create_workspace(&app, "policy-invalid-ws").await;

        for (body, what) in [
            (r#"{"strategy":"staged"}"#, "unknown strategy"),
            (
                r#"{"strategy":"rolling","priority":"urgent"}"#,
                "unknown priority",
            ),
            (
                r#"{"strategy":"rolling","max_concurrent_agents":0}"#,
                "zero batch size (rolling rollout would stall forever)",
            ),
        ] {
            let resp = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("PUT")
                        .uri(format!(
                            "/api/v1/workspaces/{ws_id}/meta-specs/rollout-policy"
                        ))
                        .header("authorization", "Bearer test-token")
                        .header("content-type", "application/json")
                        .body(Body::from(body))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::BAD_REQUEST, "must reject {what}");
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn rollout_policy_unknown_workspace_404() {
        let app = app();
        for method in ["GET", "PUT"] {
            let resp = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method(method)
                        .uri("/api/v1/workspaces/00000000-0000-0000-0000-000000000000/meta-specs/rollout-policy")
                        .header("authorization", "Bearer test-token")
                        .header("content-type", "application/json")
                        .body(Body::from(r#"{"strategy":"immediate"}"#))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                resp.status(),
                StatusCode::NOT_FOUND,
                "{method} on missing workspace"
            );
        }
    }

    // -----------------------------------------------------------------------
    // Preview mode (§5) — real-agent tests
    //
    // These exercise the production path end to end: real git repository on
    // disk (Git2OpsAdapter), real branch + worktree creation, real agent
    // rows, and a real spawned process whose environment is captured to a
    // file so the draft-injection contract is verified against what the
    // agent actually received — not against a field that was set.
    //
    // `GYRE_AGENT_COMMAND` is process-global, so every test that spawns a
    // process holds `ENV_LOCK` for its whole body.
    // -----------------------------------------------------------------------

    /// Serializes tests that touch `GYRE_AGENT_COMMAND` (read at launch time
    /// by `launch_agent_process` from the process environment).
    static ENV_LOCK: parking_lot::Mutex<()> = parking_lot::Mutex::new(());

    fn git(repo: &std::path::Path, args: &[&str]) -> String {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(args)
            .output()
            .expect("git must run");
        assert!(
            out.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    /// A real git repository with one spec file committed on `main`, plus the
    /// workspace and repo rows pointing at it. Returns the state-backed app
    /// and the repo id.
    ///
    /// `ttl_secs` / `max_concurrent` pin the preview knobs (the env-var
    /// variants are process-global and unsafe to mutate from a test).
    async fn preview_app(
        repos_root: &std::path::Path,
        ttl_secs: u64,
        max_concurrent: Option<u64>,
    ) -> (Router, std::sync::Arc<crate::AppState>, String) {
        let (state, repo_id, _ws_id) = preview_state(repos_root, ttl_secs, max_concurrent).await;
        let app = crate::api::api_router().with_state(state.clone());
        (app, state, repo_id)
    }

    /// The state half of [`preview_app`], split so tests that need to swap a
    /// store adapter (e.g. a failing kv namespace) can do so before the
    /// router is built. Returns (state, repo_id, workspace_id).
    async fn preview_state(
        repos_root: &std::path::Path,
        ttl_secs: u64,
        max_concurrent: Option<u64>,
    ) -> (std::sync::Arc<crate::AppState>, String, gyre_common::Id) {
        use gyre_domain::{Repository, Workspace};
        use std::sync::Arc;

        let repo_dir = repos_root.join("src-repo.git");
        std::fs::create_dir_all(&repo_dir).unwrap();
        git(&repo_dir, &["init", "--bare", "--initial-branch=main"]);

        // Build the initial commit in a temp clone, then push it up. This
        // exercises the same shape production repos have (bare, with a spec
        // tree on the default branch).
        let workdir = repos_root.join("seed-workdir");
        std::fs::create_dir_all(workdir.join("specs/system")).unwrap();
        git(&workdir, &["init", "--initial-branch=main"]);
        git(&workdir, &["config", "user.email", "test@gyre.local"]);
        git(&workdir, &["config", "user.name", "Gyre Test"]);
        std::fs::write(
            workdir.join("specs/system/search.md"),
            "# Search\n\nThe search spec under test.\n",
        )
        .unwrap();
        std::fs::write(
            workdir.join("specs/system/identity.md"),
            "# Identity\n\nThe identity spec under test.\n",
        )
        .unwrap();
        git(&workdir, &["add", "."]);
        git(&workdir, &["commit", "-m", "seed specs"]);
        git(
            &workdir,
            &["push", repo_dir.to_str().unwrap(), "HEAD:refs/heads/main"],
        );

        let state = crate::mem::test_state_with_preview_config(
            Arc::new(gyre_adapters::Git2OpsAdapter::new()),
            ttl_secs,
            max_concurrent,
        );

        let ws = Workspace::new(
            gyre_common::Id::new(uuid::Uuid::new_v4().to_string()),
            gyre_common::Id::new("default"),
            "preview-ws",
            format!("preview-ws-{}", uuid::Uuid::new_v4()),
            0,
        );
        state.workspaces.create(&ws).await.unwrap();

        let repo = Repository::new(
            gyre_common::Id::new(uuid::Uuid::new_v4().to_string()),
            ws.id.clone(),
            "src-repo",
            repo_dir.to_str().unwrap().to_string(),
            0,
        );
        state.repos.create(&repo).await.unwrap();

        (state, repo.id.to_string(), ws.id)
    }

    fn preview_body(repo_id: &str, spec_paths: &[&str]) -> serde_json::Value {
        serde_json::json!({
            "draft": {
                "kind": "meta:persona",
                "content": "DRAFT-PERSONA-BODY: write exhaustive tests for every change"
            },
            "targets": spec_paths
                .iter()
                .map(|p| serde_json::json!({ "repo_id": repo_id, "spec_path": p }))
                .collect::<Vec<_>>()
        })
    }

    async fn post_preview(app: &Router, body: serde_json::Value) -> axum::response::Response {
        post_preview_with_token(app, "test-token", body).await
    }

    /// POST with an explicit bearer token, for the JWT-based authorization
    /// tests (the default helper authenticates as the global admin token,
    /// which bypasses exactly the containment branches those tests guard).
    async fn post_preview_with_token(
        app: &Router,
        token: &str,
        body: serde_json::Value,
    ) -> axum::response::Response {
        app.clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/meta-specs/preview")
                    .header("authorization", format!("Bearer {token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap()
    }

    async fn get_preview_status(app: &Router, preview_id: &str) -> axum::response::Response {
        get_preview_status_with_token(app, preview_id, "test-token").await
    }

    async fn get_preview_status_with_token(
        app: &Router,
        preview_id: &str,
        token: &str,
    ) -> axum::response::Response {
        app.clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/meta-specs/preview/{preview_id}"))
                    .header("authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap()
    }

    async fn delete_preview(app: &Router, preview_id: &str) -> axum::response::Response {
        delete_preview_with_token(app, preview_id, "test-token").await
    }

    async fn delete_preview_with_token(
        app: &Router,
        preview_id: &str,
        token: &str,
    ) -> axum::response::Response {
        app.clone()
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/api/v1/meta-specs/preview/{preview_id}"))
                    .header("authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap()
    }

    /// POST a preview through the workspace-scoped route, authenticating as
    /// the global admin token.
    async fn post_workspace_preview(
        app: &Router,
        workspace_id: &str,
        body: serde_json::Value,
    ) -> axum::response::Response {
        post_workspace_preview_with_token(app, workspace_id, "test-token", body).await
    }

    /// POST a preview through the workspace-scoped route with an explicit
    /// bearer token (JWT-based containment tests).
    async fn post_workspace_preview_with_token(
        app: &Router,
        workspace_id: &str,
        token: &str,
        body: serde_json::Value,
    ) -> axum::response::Response {
        app.clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!(
                        "/api/v1/workspaces/{workspace_id}/meta-specs/preview"
                    ))
                    .header("authorization", format!("Bearer {token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap()
    }

    /// GET a preview status through the workspace-scoped route (admin token).
    async fn get_workspace_preview_status(
        app: &Router,
        workspace_id: &str,
        preview_id: &str,
    ) -> axum::response::Response {
        get_workspace_preview_status_with_token(app, workspace_id, preview_id, "test-token").await
    }

    /// GET a preview status through the workspace-scoped route with an
    /// explicit bearer token.
    async fn get_workspace_preview_status_with_token(
        app: &Router,
        workspace_id: &str,
        preview_id: &str,
        token: &str,
    ) -> axum::response::Response {
        app.clone()
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/api/v1/workspaces/{workspace_id}/meta-specs/preview/{preview_id}"
                    ))
                    .header("authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap()
    }

    /// DELETE a preview through the workspace-scoped route (admin token).
    async fn delete_workspace_preview(
        app: &Router,
        workspace_id: &str,
        preview_id: &str,
    ) -> axum::response::Response {
        app.clone()
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!(
                        "/api/v1/workspaces/{workspace_id}/meta-specs/preview/{preview_id}"
                    ))
                    .header("authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap()
    }

    /// Run `f` with `GYRE_AGENT_COMMAND` pointed at `command`. The env var is
    /// process-global and read at launch time, so the lock serializes every
    /// env-mutating test and MUST stay held across the awaited test body —
    /// dropping it early would let a concurrent test clobber the variable
    /// mid-run. Tests run single-threaded (`RUST_TEST_THREADS=1`), so the
    /// guard is never contended across the await.
    #[allow(clippy::await_holding_lock)]
    async fn with_agent_command<F, Fut>(command: &str, f: F)
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = ()>,
    {
        let _env = ENV_LOCK.lock();
        std::env::set_var("GYRE_AGENT_COMMAND", command);
        let result = std::panic::AssertUnwindSafe(f()).catch_unwind().await;
        std::env::remove_var("GYRE_AGENT_COMMAND");
        if let Err(panic) = result {
            std::panic::resume_unwind(panic);
        }
    }

    /// Write a fake agent-entrypoint script that dumps the environment it was
    /// handed (the injection contract) to `env_out` and then sleeps, so the
    /// process stays alive until the test kills it. Returns the script path.
    ///
    /// The dump must appear ATOMICALLY: `env`'s stdout is block-buffered when
    /// redirected to a file, so a bare `env > out` flushes in 4096-byte chunks
    /// and a poller that treats "file has content" as "file is complete"
    /// reads a torn dump (CI observed exactly the first stdio block, with
    /// GYRE_PREVIEW_ID/GYRE_AUTH_TOKEN/GYRE_SVID_* still unflushed). Two
    /// preview agents also run this script concurrently against the same
    /// `env_out`, so a bare redirect additionally interleaves two writers.
    /// Writing to a per-PID temp file and renaming it into place makes the
    /// dump observable only in its complete state; the last rename wins,
    /// which is fine — every agent of a preview shares the injected
    /// contract vars the assertions check.
    fn env_dump_script(dir: &std::path::Path, env_out: &std::path::Path) -> std::path::PathBuf {
        let script = dir.join("agent-entrypoint.sh");
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\nenv > {out}.tmp.$$\nmv {out}.tmp.$$ {out}\nexec sleep 300\n",
                out = env_out.display()
            ),
        )
        .unwrap();
        make_executable(&script);
        script
    }

    fn make_executable(path: &std::path::Path) {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(path).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(path, perms).unwrap();
    }

    /// Wait until `f` returns Some, polling briefly (git + process spawn are
    /// async relative to the handler's launch path).
    async fn soon<T>(mut f: impl FnMut() -> Option<T>) -> T {
        for _ in 0..100 {
            if let Some(v) = f() {
                return v;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        panic!("condition not met within 5s");
    }

    use futures_util::FutureExt as _;

    /// The full happy path: POST spawns one real agent per target on
    /// `preview/{preview_id}/{slug}` branches with real worktrees on disk,
    /// the 202 shape matches the spec, the draft reaches the agent process
    /// environment, and no task/MR/provenance is created.
    #[tokio::test(flavor = "multi_thread")]
    async fn preview_spawns_real_agents_with_branches_worktrees_and_draft() {
        let dir = tempfile::tempdir().unwrap();
        let (app, state, repo_id) = preview_app(dir.path(), 86_400, None).await;

        let env_out = dir.path().join("agent-env.txt");
        let script = env_dump_script(dir.path(), &env_out);

        with_agent_command(script.to_str().unwrap(), || async {
            let resp = post_preview(
                &app,
                preview_body(
                    &repo_id,
                    &["specs/system/search.md", "specs/system/identity.md"],
                ),
            )
            .await;
            assert_eq!(
                resp.status(),
                StatusCode::ACCEPTED,
                "preview must be accepted"
            );
            let json = body_json(resp).await;

            let preview_id = json["preview_id"].as_str().unwrap().to_string();
            assert!(!preview_id.is_empty());
            let agents = json["agents"].as_array().unwrap();
            assert_eq!(agents.len(), 2, "one agent per target: {json}");

            let mut branches: Vec<&str> = agents
                .iter()
                .map(|a| a["branch"].as_str().unwrap())
                .collect();
            branches.sort();
            assert_eq!(
                branches,
                vec![
                    format!("preview/{preview_id}/identity").as_str(),
                    format!("preview/{preview_id}/search").as_str(),
                ],
                "branches follow preview/<id>/<spec-stem>"
            );
            for a in agents {
                assert_eq!(a["repo_id"].as_str().unwrap(), repo_id);
                assert!(a["spec_path"]
                    .as_str()
                    .unwrap()
                    .starts_with("specs/system/"));
                assert!(!a["agent_id"].as_str().unwrap().is_empty());
            }

            // Real agent rows: Active, scoped to the repo's workspace.
            for a in agents {
                let agent = state
                    .agents
                    .find_by_id(&gyre_common::Id::new(a["agent_id"].as_str().unwrap()))
                    .await
                    .unwrap()
                    .expect("agent row must exist");
                assert_eq!(agent.status, gyre_domain::AgentStatus::Active);
                assert_eq!(
                    agent.workspace_id.as_str(),
                    state
                        .repos
                        .find_by_id(&gyre_common::Id::new(&repo_id))
                        .await
                        .unwrap()
                        .unwrap()
                        .workspace_id
                        .as_str()
                );
                assert!(
                    agent.current_task_id.is_none(),
                    "preview agents have no task"
                );
            }

            // Real branches + worktrees on disk.
            let repo = state
                .repos
                .find_by_id(&gyre_common::Id::new(&repo_id))
                .await
                .unwrap()
                .unwrap();
            for b in &branches {
                assert!(
                    state.git_ops.branch_exists(&repo.path, b).await.unwrap(),
                    "branch {b} must exist in the repo"
                );
                let wt_path = format!("{}/worktrees/{}", repo.path, b.replace('/', "-"));
                assert!(
                    std::path::Path::new(&wt_path).is_dir(),
                    "worktree {wt_path} must exist on disk"
                );
            }

            // The draft actually reached the agent process environment. The
            // dump is renamed into place atomically by the entrypoint script
            // (see `env_dump_script`), so once it exists it is complete —
            // poll until the dump has content, not merely exists.
            let env = soon(|| {
                std::fs::read_to_string(&env_out)
                    .ok()
                    .filter(|s| !s.is_empty())
            })
            .await;
            assert!(
                env.contains("GYRE_META_SPEC_DRAFT_KIND=meta:persona"),
                "draft kind must be injected; env was:\n{env}"
            );
            assert!(
                env.contains(
                    "GYRE_META_SPEC_DRAFT_CONTENT=DRAFT-PERSONA-BODY: write exhaustive tests"
                ),
                "draft content must be injected via the GYRE_META_SPEC_DRAFT_CONTENT env var; env was:\n{env}"
            );
            assert!(
                env.contains(&format!("GYRE_PREVIEW_ID={preview_id}")),
                "preview id must be injected; env was:\n{env}"
            );
            assert!(
                env.contains("GYRE_TARGET_SPEC_PATH=specs/system/search.md")
                    || env.contains("GYRE_TARGET_SPEC_PATH=specs/system/identity.md"),
                "target spec path must be injected; env was:\n{env}"
            );
            assert!(
                !env.contains("GYRE_TASK_ID="),
                "preview agents must NOT receive GYRE_TASK_ID; env was:\n{env}"
            );

            // Skip-ceremony: no MRs, no tasks, no provenance keys, no
            // refs/agents/* or refs/tasks/* writes.
            assert!(
                state.merge_requests.list().await.unwrap().is_empty(),
                "preview must not create MRs"
            );
            assert!(
                state.tasks.list().await.unwrap().is_empty(),
                "preview must not create tasks"
            );
            let provenance = state.kv_store.kv_list("agent_provenance").await.unwrap();
            assert!(
                provenance.is_empty(),
                "no provenance recording: {provenance:?}"
            );
            let agent_refs = crate::git_refs::count_refs_under(&repo.path, "refs/agents/").await;
            assert_eq!(agent_refs, 0, "no refs/agents/* writes in preview mode");
            let task_refs = crate::git_refs::count_refs_under(&repo.path, "refs/tasks/").await;
            assert_eq!(task_refs, 0, "no refs/tasks/* writes in preview mode");

            // Token minted and short-lived (preview JWT TTL, not the default).
            for a in agents {
                let tok = state
                    .kv_store
                    .kv_get("agent_tokens", a["agent_id"].as_str().unwrap())
                    .await
                    .unwrap();
                assert!(tok.is_some(), "preview agent token must be minted");
                let claims = state
                    .agent_signing_key
                    .validate(tok.unwrap().as_str(), &state.base_url)
                    .expect("preview agent token must be a valid JWT");
                let expected_exp = chrono_now_secs() + state.preview_jwt_ttl_secs;
                assert!(
                    claims.exp <= expected_exp,
                    "preview JWT must use the preview TTL ({:?} vs {claims:?})",
                    state.preview_jwt_ttl_secs
                );
            }

            // Budget slot taken (workspace accounting governs by default).
            let ws_key = crate::api::budget::workspace_key(repo.workspace_id.as_str());
            let usage = state
                .budget_usages
                .get_usage(&ws_key)
                .await
                .unwrap()
                .expect("usage row must exist after preview spawn");
            assert!(
                usage.active_agents >= 2,
                "two preview agents must be counted"
            );

            // Status reflects the real Active state while the process runs.
            let status_resp = get_preview_status(&app, &preview_id).await;
            assert_eq!(status_resp.status(), StatusCode::OK);
            let status = body_json(status_resp).await;
            assert_eq!(status["state"].as_str().unwrap(), "running");
            assert_eq!(status["agents"].as_array().unwrap().len(), 2);
            for a in status["agents"].as_array().unwrap() {
                assert_eq!(a["status"].as_str().unwrap(), "running");
            }

            // Cleanup so the sleep processes do not outlive the test.
            let del = delete_preview(&app, &preview_id).await;
            assert_eq!(del.status(), StatusCode::NO_CONTENT);
        })
        .await;
    }

    fn chrono_now_secs() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
    }

    /// DELETE tears the run down completely: processes killed, worktrees
    /// force-removed, branches deleted, kv record gone, budget released —
    /// and a subsequent GET status 404s.
    #[tokio::test(flavor = "multi_thread")]
    async fn preview_delete_tears_down_everything() {
        let dir = tempfile::tempdir().unwrap();
        let (app, state, repo_id) = preview_app(dir.path(), 86_400, None).await;

        let env_out = dir.path().join("agent-env.txt");
        let script = env_dump_script(dir.path(), &env_out);

        with_agent_command(script.to_str().unwrap(), || async {
            let resp =
                post_preview(&app, preview_body(&repo_id, &["specs/system/search.md"])).await;
            assert_eq!(resp.status(), StatusCode::ACCEPTED);
            let json = body_json(resp).await;
            let preview_id = json["preview_id"].as_str().unwrap().to_string();
            let agent_id = json["agents"][0]["agent_id"].as_str().unwrap().to_string();
            let branch = json["agents"][0]["branch"].as_str().unwrap().to_string();

            // Wait for the process to actually be running (registered).
            soon(|| {
                let reg = state.process_registry.try_lock().ok()?;
                reg.contains_key(&agent_id).then_some(())
            })
            .await;
            // Registration is complete only when the backend that owns the
            // handle is recorded too — kills must route through it.
            soon(|| {
                let backends = state.spawned_backends.try_lock().ok()?;
                backends.contains_key(&agent_id).then_some(())
            })
            .await;
            let agent_pid = {
                let reg = state.process_registry.lock().await;
                reg.get(&agent_id).and_then(|h| h.pid)
            };
            assert!(
                agent_pid.is_some(),
                "local preview spawn must record a pid so the kill is observable"
            );

            let repo = state
                .repos
                .find_by_id(&gyre_common::Id::new(&repo_id))
                .await
                .unwrap()
                .unwrap();
            let wt_path = format!("{}/worktrees/{}", repo.path, branch.replace('/', "-"));
            assert!(std::path::Path::new(&wt_path).is_dir());

            let del = delete_preview(&app, &preview_id).await;
            assert_eq!(del.status(), StatusCode::NO_CONTENT);

            // The runtime process is actually dead — DELETE must kill
            // through the recorded backend, not just forget the registry
            // entries (a forgotten-but-alive sleep process holds the pid
            // and the worktree CWD).
            let mut dead = false;
            for _ in 0..50 {
                let alive = gyre_ports::ComputeTarget::is_alive(
                    &gyre_adapters::compute::LocalTarget,
                    &gyre_ports::ProcessHandle {
                        id: agent_pid.unwrap().to_string(),
                        target_type: "local".to_string(),
                        pid: agent_pid,
                        options: std::collections::HashMap::new(),
                    },
                )
                .await
                .unwrap_or(false);
                if !alive {
                    dead = true;
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
            assert!(
                dead,
                "DELETE must terminate the preview agent process (pid {})",
                agent_pid.unwrap()
            );
            // Both lifecycle registries are purged together: a handle
            // removed without its backend leaks one Arc<SpawnBackend> per
            // killed agent for the lifetime of the server.
            assert!(!state.process_registry.lock().await.contains_key(&agent_id));
            assert!(!state.spawned_backends.lock().await.contains_key(&agent_id));

            // Branch gone, worktree gone, kv records gone.
            assert!(!state
                .git_ops
                .branch_exists(&repo.path, &branch)
                .await
                .unwrap());
            assert!(!std::path::Path::new(&wt_path).exists());
            assert!(state
                .kv_store
                .kv_get("meta_spec_previews", &preview_id)
                .await
                .unwrap()
                .is_none());
            assert!(state
                .kv_store
                .kv_get("preview_agents", &agent_id)
                .await
                .unwrap()
                .is_none());
            assert!(
                state
                    .kv_store
                    .kv_get("agent_tokens", &agent_id)
                    .await
                    .unwrap()
                    .is_none(),
                "preview agent token must be revoked on DELETE"
            );

            // Agent row terminal, not Active/Idle.
            let agent = state
                .agents
                .find_by_id(&gyre_common::Id::new(&agent_id))
                .await
                .unwrap()
                .unwrap();
            assert_ne!(agent.status, gyre_domain::AgentStatus::Active);
            assert_ne!(agent.status, gyre_domain::AgentStatus::Idle);

            // Status 404 afterwards.
            let status = get_preview_status(&app, &preview_id).await;
            assert_eq!(status.status(), StatusCode::NOT_FOUND);

            // Second DELETE is 404 (record is gone).
            let del2 = delete_preview(&app, &preview_id).await;
            assert_eq!(del2.status(), StatusCode::NOT_FOUND);
        })
        .await;
    }

    /// F1 regression: the preview kill must be dispatched through the
    /// backend recorded at spawn time (`spawned_backends`), never
    /// reconstructed from the handle's shape alone. Two production
    /// backends cannot be reconstructed: an SshDocker handle's id is a
    /// remote container id that `SshTarget::from_handle` refuses, and a
    /// Kubernetes handle carries no pid, so a LocalTarget reconstruction
    /// is a silent no-op while the Pod keeps running. No remote host is
    /// available in tests, so the contract is pinned with a REAL process
    /// registered behind an ssh-shaped handle (malformed for
    /// reconstruction) whose recorded backend is Local: the kill must
    /// reach the process. The pre-fix code path (`kill_process_handle`)
    /// refuses the malformed ssh id and leaves the process alive.
    #[tokio::test(flavor = "multi_thread")]
    async fn preview_kill_routes_through_recorded_backend_not_handle_shape() {
        let state = crate::mem::test_state();

        // A real process to kill.
        let spawn_cfg = gyre_ports::SpawnConfig {
            name: "preview-kill-routing".to_string(),
            command: "sleep".to_string(),
            args: vec!["60".to_string()],
            env: std::collections::HashMap::new(),
            work_dir: "/tmp".to_string(),
        };
        let handle = gyre_adapters::compute::LocalTarget
            .spawn_process(&spawn_cfg)
            .await
            .expect("spawning sleep should work on linux");
        let pid = handle.pid.expect("local spawn sets pid");

        // Register it the way a remote-container launch would: the handle
        // id is a remote container id an SshTarget cannot be reconstructed
        // from, and the backend that owns the handle is recorded alongside.
        let mislabeled = gyre_ports::ProcessHandle {
            id: "f00f00f00f00remote-container".to_string(),
            target_type: "ssh".to_string(),
            pid: Some(pid),
            options: std::collections::HashMap::new(),
        };
        state
            .process_registry
            .lock()
            .await
            .insert("agt-preview-kill-routing".to_string(), mislabeled);
        state.spawned_backends.lock().await.insert(
            "agt-preview-kill-routing".to_string(),
            std::sync::Arc::new(crate::api::spawn::SpawnBackend::Local),
        );

        kill_preview_process(&state, "agt-preview-kill-routing").await;

        // Both lifecycle entries are gone together...
        assert!(!state
            .process_registry
            .lock()
            .await
            .contains_key("agt-preview-kill-routing"));
        assert!(!state
            .spawned_backends
            .lock()
            .await
            .contains_key("agt-preview-kill-routing"));
        // ...and the process is actually dead.
        let mut dead = false;
        for _ in 0..50 {
            let alive = gyre_ports::ComputeTarget::is_alive(
                &gyre_adapters::compute::LocalTarget,
                &gyre_ports::ProcessHandle {
                    id: pid.to_string(),
                    target_type: "local".to_string(),
                    pid: Some(pid),
                    options: std::collections::HashMap::new(),
                },
            )
            .await
            .unwrap_or(false);
            if !alive {
                dead = true;
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        assert!(
            dead,
            "preview kill must reach the process through the recorded backend (pid {pid})"
        );
    }

    /// A fake `ssh` binary emulating a remote docker host: the remote
    /// command (`docker <subcmd> ...`, the last argument) is answered the
    /// way the remote daemon would, and markers are written next to the
    /// script so the test can observe exactly what the dispatch invoked.
    /// `docker run` reports a container id (spawn), `docker image inspect`
    /// a digest, `docker inspect` `true` (alive), `docker rm` the kill.
    fn fake_ssh_script(dir: &std::path::Path) -> std::path::PathBuf {
        let script = dir.join("fake-ssh");
        std::fs::write(
            &script,
            concat!(
                "#!/bin/sh\n",
                "dir=$(CDPATH= cd -- \"$(dirname -- \"$0\")\" && pwd)\n",
                "cmd=\n",
                "for arg in \"$@\"; do cmd=$arg; done\n",
                // The dispatch shell-quotes every remote docker argument
                // (SshDockerTarget::remote_docker), so the remote command
                // arrives as `docker 'run' '--detach' ...`. Strip the
                // quoting before matching the subcommand shape.
                "cmd=$(printf '%s' \"$cmd\" | tr -d \"'\")\n",
                "case $cmd in\n",
                "  'docker run '*) echo fakecontainer0123456789; echo run > \"$dir/ssh-saw-run\" ;;\n",
                "  'docker image inspect'*) echo sha256:fakedigest0000000000000000000000000000000000000000000000000000 ;;\n",
                "  'docker inspect '*) echo true ;;\n",
                "  'docker rm '*) echo rm > \"$dir/ssh-saw-rm\" ;;\n",
                "  *) echo \"fake ssh: unhandled remote command: $cmd\" >&2; exit 1 ;;\n",
                "esac\n",
            ),
        )
        .unwrap();
        make_executable(&script);
        script
    }

    /// A fake `kubectl` emulating a cluster: `apply` records the Pod
    /// manifest from stdin (so the test can assert what the agent would
    /// have run with), `wait` reports the Pod scheduled, `get` reports a
    /// Running phase / imageID, and `delete` records the kill.
    fn fake_kubectl_script(dir: &std::path::Path) -> std::path::PathBuf {
        let script = dir.join("fake-kubectl");
        std::fs::write(
            &script,
            concat!(
                "#!/bin/sh\n",
                "dir=$(CDPATH= cd -- \"$(dirname -- \"$0\")\" && pwd)\n",
                "case $1 in\n",
                "  apply) cat > \"$dir/kubectl-manifest.json\" ;;\n",
                "  wait)  : ;;\n",
                "  get)   case \"$*\" in\n",
                "            *imageID*) echo docker-pullable://gyre-agent@sha256:fakedigest ;;\n",
                "            *) echo Running ;;\n",
                "         esac ;;\n",
                "  delete) echo delete > \"$dir/kubectl-saw-delete\" ;;\n",
                "  *) echo \"fake kubectl: unhandled: $*\" >&2; exit 1 ;;\n",
                "esac\n",
            ),
        )
        .unwrap();
        make_executable(&script);
        script
    }

    /// Wire an Ssh/Kubernetes compute target to the preview workspace and
    /// return its id, so `launch_preview_agent` resolves it instead of the
    /// local fallback.
    async fn attach_compute_target(
        state: &std::sync::Arc<crate::AppState>,
        repo_id: &str,
        target_type: gyre_domain::ComputeTargetType,
        config: serde_json::Value,
    ) -> gyre_common::Id {
        let ct = gyre_domain::ComputeTargetEntity::new(
            new_id(),
            gyre_common::Id::new("default"),
            format!("preview-ct-{}", target_type),
            target_type,
            now_secs(),
        );
        let ct = gyre_domain::ComputeTargetEntity { config, ..ct };
        state.compute_targets.create(&ct).await.unwrap();
        let repo = state
            .repos
            .find_by_id(&gyre_common::Id::new(repo_id))
            .await
            .unwrap()
            .unwrap();
        let mut ws = state
            .workspaces
            .find_by_id(&repo.workspace_id)
            .await
            .unwrap()
            .unwrap();
        ws.compute_target_id = Some(ct.id.clone());
        state.workspaces.update(&ws).await.unwrap();
        ct.id
    }

    /// Review-round regression (01195dea): the preview launch's started
    /// check used to guess from the handle's shape (`pid.is_some()` for
    /// every non-container target), but SshDocker — ssh container_mode,
    /// the dispatch default — and Kubernetes handles carry `pid: None` by
    /// design (the id is a remote container / Pod name). A successful
    /// remote launch was therefore reported as "process never started":
    /// the agent transitioned Failed, token revoked, worktree removed,
    /// the actually-running remote container orphaned, no diff ever
    /// produced — §5 preview mode broken on two of the five compute
    /// backends. No remote host exists in tests, so the ssh-docker
    /// dispatch runs end to end against a fake `ssh` binary (wired via
    /// the target's `ssh_path` config, the `kubectl_path` pattern) that
    /// emulates the remote docker daemon; the contract under test is that
    /// a pid-less remote-container handle is a START, and that the run
    /// stays killable through the recorded backend.
    #[tokio::test(flavor = "multi_thread")]
    async fn preview_launch_on_ssh_docker_target_is_not_failed() {
        let dir = tempfile::tempdir().unwrap();
        let (app, state, repo_id) = preview_app(dir.path(), 86_400, None).await;

        // container_mode defaults to true in the dispatch — the spec
        // default ("SSH to remote host, docker run there"). The ssh
        // client is pinned to the fake via the target's `ssh_path` config
        // (same wiring pattern as the kubernetes sibling's `kubectl_path`,
        // no PATH shadowing — a PATH-wide `ssh` swap would leak into every
        // concurrently running test in this process).
        let ssh_bin = fake_ssh_script(dir.path());
        let fake_dir = ssh_bin.parent().unwrap().to_path_buf();
        attach_compute_target(
            &state,
            &repo_id,
            gyre_domain::ComputeTargetType::Ssh,
            serde_json::json!({
                "user": "ci",
                "host": "fake-remote",
                "ssh_path": ssh_bin.to_str().unwrap(),
            }),
        )
        .await;

        let resp = post_preview(&app, preview_body(&repo_id, &["specs/system/search.md"])).await;
        assert_eq!(
            resp.status(),
            StatusCode::ACCEPTED,
            "a successful remote-container launch must start the preview"
        );
        let json = body_json(resp).await;
        let preview_id = json["preview_id"].as_str().unwrap().to_string();
        let agent_id = json["agents"][0]["agent_id"].as_str().unwrap().to_string();

        // The remote docker really was asked to run a container.
        assert!(
            fake_dir.join("ssh-saw-run").exists(),
            "the ssh-docker dispatch must invoke the remote docker run"
        );

        // The registered handle is the pid-less remote-container shape
        // (target_type ssh, id = remote container id, no pid) and its
        // backend was recorded alongside — the exact shapes the
        // pre-fix check misread as \"never started\".
        let handle = state
            .process_registry
            .lock()
            .await
            .get(&agent_id)
            .cloned()
            .expect("remote-container handle must be registered");
        assert_eq!(handle.target_type, "ssh");
        assert!(
            handle.pid.is_none(),
            "SshDocker handles are pid-less by design — the shape the started check must not guess from"
        );
        assert!(state.spawned_backends.lock().await.contains_key(&agent_id));

        // THE regression: the agent is Active (pre-fix: Failed from the
        // first moment, with the remote container still running).
        let agent = state
            .agents
            .find_by_id(&gyre_common::Id::new(&agent_id))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            agent.status,
            gyre_domain::AgentStatus::Active,
            "a successfully started remote container must not be reported failed"
        );

        // Status derives from the real row: running, not failed.
        let status_resp = get_preview_status(&app, &preview_id).await;
        assert_eq!(status_resp.status(), StatusCode::OK);
        let status = body_json(status_resp).await;
        assert_eq!(status["state"].as_str().unwrap(), "running");
        assert_eq!(status["agents"][0]["status"].as_str().unwrap(), "running");

        // Teardown kills through the recorded SshDocker backend — the
        // remote daemon sees the `docker rm` — and purges both
        // lifecycle registries.
        let del = delete_preview(&app, &preview_id).await;
        assert_eq!(del.status(), StatusCode::NO_CONTENT);
        assert!(
            fake_dir.join("ssh-saw-rm").exists(),
            "DELETE must kill the remote container through the recorded backend"
        );
        assert!(!state.process_registry.lock().await.contains_key(&agent_id));
        assert!(!state.spawned_backends.lock().await.contains_key(&agent_id));
    }

    /// Kubernetes sibling of the ssh-docker regression above: a Pod handle
    /// (pid-less, id = pod name) is a START. Runs the kubernetes dispatch
    /// end to end against a fake `kubectl` (wired via the target's
    /// `kubectl_path` config, no PATH shadowing) — and additionally pins
    /// that the draft meta-spec reaches the Pod manifest the cluster was
    /// asked to apply.
    #[tokio::test(flavor = "multi_thread")]
    async fn preview_launch_on_kubernetes_target_is_not_failed() {
        let dir = tempfile::tempdir().unwrap();
        let (app, state, repo_id) = preview_app(dir.path(), 86_400, None).await;

        let kubectl = fake_kubectl_script(dir.path());
        attach_compute_target(
            &state,
            &repo_id,
            gyre_domain::ComputeTargetType::Kubernetes,
            serde_json::json!({"kubectl_path": kubectl.to_str().unwrap()}),
        )
        .await;

        let resp = post_preview(&app, preview_body(&repo_id, &["specs/system/search.md"])).await;
        assert_eq!(
            resp.status(),
            StatusCode::ACCEPTED,
            "a successfully scheduled Pod must start the preview"
        );
        let json = body_json(resp).await;
        let preview_id = json["preview_id"].as_str().unwrap().to_string();
        let agent_id = json["agents"][0]["agent_id"].as_str().unwrap().to_string();

        // The cluster was asked to apply a Pod manifest carrying the draft
        // meta-spec — the governing context the agent runs under. The
        // manifest serializes env as {"name","value"} objects, so assert
        // the name and the value separately.
        let manifest = std::fs::read_to_string(dir.path().join("kubectl-manifest.json"))
            .expect("kubectl apply must have received the Pod manifest");
        assert!(
            manifest.contains("\"GYRE_META_SPEC_DRAFT_KIND\"") && manifest.contains("meta:persona"),
            "draft kind must reach the Pod manifest; manifest was:\n{manifest}"
        );
        assert!(
            manifest.contains("\"GYRE_META_SPEC_DRAFT_CONTENT\"")
                && manifest.contains("DRAFT-PERSONA-BODY: write exhaustive tests"),
            "draft content must reach the Pod manifest via the GYRE_META_SPEC_DRAFT_CONTENT env var; manifest was:\n{manifest}"
        );
        assert!(
            manifest.contains("\"GYRE_PREVIEW_ID\"") && manifest.contains(&preview_id),
            "preview id must reach the Pod manifest; manifest was:\n{manifest}"
        );

        // The registered handle is the pid-less Pod shape.
        let handle = state
            .process_registry
            .lock()
            .await
            .get(&agent_id)
            .cloned()
            .expect("Pod handle must be registered");
        assert_eq!(handle.target_type, "kubernetes");
        assert!(
            handle.pid.is_none(),
            "Kubernetes handles are pid-less by design — the shape the started check must not guess from"
        );

        // THE regression: agent Active, status running (pre-fix: Failed
        // from the first poll while the Pod kept running).
        let agent = state
            .agents
            .find_by_id(&gyre_common::Id::new(&agent_id))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            agent.status,
            gyre_domain::AgentStatus::Active,
            "a successfully scheduled Pod must not be reported failed"
        );
        let status_resp = get_preview_status(&app, &preview_id).await;
        assert_eq!(status_resp.status(), StatusCode::OK);
        let status = body_json(status_resp).await;
        assert_eq!(status["agents"][0]["status"].as_str().unwrap(), "running");

        // Teardown deletes the Pod through the recorded backend.
        let del = delete_preview(&app, &preview_id).await;
        assert_eq!(del.status(), StatusCode::NO_CONTENT);
        assert!(
            dir.path().join("kubectl-saw-delete").exists(),
            "DELETE must delete the Pod through the recorded backend"
        );
        assert!(!state.process_registry.lock().await.contains_key(&agent_id));
    }

    /// The launch-failure path must not just reclaim the row, token and
    /// worktree — it must kill whatever process is registered for the
    /// agent, through the recorded handle/backend pair. Pinned as a
    /// regression because the pre-fix path performed no kill at all: a
    /// wrong \"never started\" verdict on a live remote process orphaned
    /// it with no agent pointing at it. A real process is registered the
    /// way a remote-container launch would (handle shape deliberately
    /// unreconstructable, backend recorded as Local) and must die.
    #[tokio::test(flavor = "multi_thread")]
    async fn mark_preview_agent_failed_kills_registered_process() {
        let dir = tempfile::tempdir().unwrap();
        let (state, repo_id, ws_id) = preview_state(dir.path(), 86_400, None).await;
        let repo = state
            .repos
            .find_by_id(&gyre_common::Id::new(&repo_id))
            .await
            .unwrap()
            .unwrap();
        let workspace = state.workspaces.find_by_id(&ws_id).await.unwrap().unwrap();

        let mut agent = gyre_domain::Agent::new(
            gyre_common::Id::new("agt-preview-failkill"),
            "preview-failkill",
            1000,
        );
        agent.workspace_id = repo.workspace_id.clone();
        agent
            .transition_status(gyre_domain::AgentStatus::Active)
            .unwrap();
        state.agents.create(&agent).await.unwrap();
        state
            .kv_store
            .kv_set("agent_tokens", &agent.id.to_string(), "tok".to_string())
            .await
            .unwrap();

        let agent_ref = PreviewAgentRef {
            preview_id: "preview-failkill".to_string(),
            agent_id: agent.id.to_string(),
            repo_id: repo.id.to_string(),
            workspace_id: repo.workspace_id.to_string(),
            spec_path: "specs/system/search.md".to_string(),
            branch: "preview/preview-failkill/search".to_string(),
            base_sha: "0".repeat(40),
            worktree_path: dir
                .path()
                .join("no-such-worktree")
                .to_str()
                .unwrap()
                .to_string(),
            released: false,
        };
        let mut p = Provisioned {
            agent_ref: agent_ref.clone(),
            token: "tok".to_string(),
            agent: agent.clone(),
            repo: repo.clone(),
            workspace,
        };

        // A real process behind an ssh-shaped handle (id malformed for
        // reconstruction) whose recorded backend is Local — the kill must
        // reach the process through that pair.
        let spawn_cfg = gyre_ports::SpawnConfig {
            name: "preview-failkill".to_string(),
            command: "sleep".to_string(),
            args: vec!["60".to_string()],
            env: std::collections::HashMap::new(),
            work_dir: "/tmp".to_string(),
        };
        let real = gyre_adapters::compute::LocalTarget
            .spawn_process(&spawn_cfg)
            .await
            .expect("spawning sleep should work on linux");
        let pid = real.pid.expect("local spawn sets pid");
        state.process_registry.lock().await.insert(
            agent.id.to_string(),
            gyre_ports::ProcessHandle {
                id: "f00f00f00f00remote-container".to_string(),
                target_type: "ssh".to_string(),
                pid: Some(pid),
                options: std::collections::HashMap::new(),
            },
        );
        state.spawned_backends.lock().await.insert(
            agent.id.to_string(),
            std::sync::Arc::new(crate::api::spawn::SpawnBackend::Local),
        );

        mark_preview_agent_failed(&state, &mut p).await;

        // The registered process is actually dead.
        let mut dead = false;
        for _ in 0..50 {
            let alive = gyre_ports::ComputeTarget::is_alive(
                &gyre_adapters::compute::LocalTarget,
                &gyre_ports::ProcessHandle {
                    id: pid.to_string(),
                    target_type: "local".to_string(),
                    pid: Some(pid),
                    options: std::collections::HashMap::new(),
                },
            )
            .await
            .unwrap_or(false);
            if !alive {
                dead = true;
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        assert!(
            dead,
            "mark_preview_agent_failed must kill the registered process (pid {pid})"
        );
        // Both lifecycle registries purged, row Failed, token revoked,
        // budget slot released.
        assert!(!state
            .process_registry
            .lock()
            .await
            .contains_key(&agent.id.to_string()));
        assert!(!state
            .spawned_backends
            .lock()
            .await
            .contains_key(&agent.id.to_string()));
        let row = state.agents.find_by_id(&agent.id).await.unwrap().unwrap();
        assert_eq!(row.status, gyre_domain::AgentStatus::Failed);
        assert!(state
            .kv_store
            .kv_get("agent_tokens", &agent.id.to_string())
            .await
            .unwrap()
            .is_none());
        assert!(p.agent_ref.released, "failure must release the budget slot");
    }

    /// kv wrapper whose writes to one namespace fail — forces the
    /// `persist_preview` failure path at a chosen step.
    struct NamespaceFailingKv {
        inner: std::sync::Arc<dyn gyre_ports::KvJsonStore>,
        fail_namespace: &'static str,
    }

    impl NamespaceFailingKv {
        fn new(
            inner: std::sync::Arc<dyn gyre_ports::KvJsonStore>,
            fail_namespace: &'static str,
        ) -> Self {
            Self {
                inner,
                fail_namespace,
            }
        }
    }

    #[axum::async_trait]
    impl gyre_ports::KvJsonStore for NamespaceFailingKv {
        async fn kv_set(&self, namespace: &str, _key: &str, _value: String) -> anyhow::Result<()> {
            if namespace == self.fail_namespace {
                anyhow::bail!("injected kv write failure for namespace {namespace}");
            }
            self.inner.kv_set(namespace, _key, _value).await
        }
        async fn kv_get(&self, namespace: &str, key: &str) -> anyhow::Result<Option<String>> {
            self.inner.kv_get(namespace, key).await
        }
        async fn kv_remove(&self, namespace: &str, key: &str) -> anyhow::Result<()> {
            self.inner.kv_remove(namespace, key).await
        }
        async fn kv_list(&self, namespace: &str) -> anyhow::Result<Vec<(String, String)>> {
            self.inner.kv_list(namespace).await
        }
        async fn kv_clear(&self, namespace: &str) -> anyhow::Result<()> {
            self.inner.kv_clear(namespace).await
        }
    }

    /// kv wrapper whose `kv_list` on one namespace fails — forces the
    /// preview-budget enumeration failure path, proving it fails closed
    /// (the POST is rejected, not admitted against an unknown count).
    struct ListFailingKv {
        inner: std::sync::Arc<dyn gyre_ports::KvJsonStore>,
        fail_namespace: &'static str,
    }

    impl ListFailingKv {
        fn new(
            inner: std::sync::Arc<dyn gyre_ports::KvJsonStore>,
            fail_namespace: &'static str,
        ) -> Self {
            Self {
                inner,
                fail_namespace,
            }
        }
    }

    #[axum::async_trait]
    impl gyre_ports::KvJsonStore for ListFailingKv {
        async fn kv_set(&self, namespace: &str, key: &str, value: String) -> anyhow::Result<()> {
            self.inner.kv_set(namespace, key, value).await
        }
        async fn kv_get(&self, namespace: &str, key: &str) -> anyhow::Result<Option<String>> {
            self.inner.kv_get(namespace, key).await
        }
        async fn kv_remove(&self, namespace: &str, key: &str) -> anyhow::Result<()> {
            self.inner.kv_remove(namespace, key).await
        }
        async fn kv_list(&self, namespace: &str) -> anyhow::Result<Vec<(String, String)>> {
            if namespace == self.fail_namespace {
                anyhow::bail!("injected kv list failure for namespace {namespace}");
            }
            self.inner.kv_list(namespace).await
        }
        async fn kv_clear(&self, namespace: &str) -> anyhow::Result<()> {
            self.inner.kv_clear(namespace).await
        }
    }

    /// kv wrapper whose `kv_get` on one namespace fails — forces the
    /// preview-agent discriminator lookup to error, proving the refusal
    /// gates fail closed during a store outage instead of silently
    /// stopping all preview containment.
    struct GetFailingKv {
        inner: std::sync::Arc<dyn gyre_ports::KvJsonStore>,
        fail_namespace: &'static str,
    }

    impl GetFailingKv {
        fn new(
            inner: std::sync::Arc<dyn gyre_ports::KvJsonStore>,
            fail_namespace: &'static str,
        ) -> Self {
            Self {
                inner,
                fail_namespace,
            }
        }
    }

    #[axum::async_trait]
    impl gyre_ports::KvJsonStore for GetFailingKv {
        async fn kv_set(&self, namespace: &str, key: &str, value: String) -> anyhow::Result<()> {
            self.inner.kv_set(namespace, key, value).await
        }
        async fn kv_get(&self, namespace: &str, _key: &str) -> anyhow::Result<Option<String>> {
            if namespace == self.fail_namespace {
                anyhow::bail!("injected kv get failure for namespace {namespace}");
            }
            self.inner.kv_get(namespace, _key).await
        }
        async fn kv_remove(&self, namespace: &str, key: &str) -> anyhow::Result<()> {
            self.inner.kv_remove(namespace, key).await
        }
        async fn kv_list(&self, namespace: &str) -> anyhow::Result<Vec<(String, String)>> {
            self.inner.kv_list(namespace).await
        }
        async fn kv_clear(&self, namespace: &str) -> anyhow::Result<()> {
            self.inner.kv_clear(namespace).await
        }
    }

    /// agents wrapper whose create fails for preview agents — forces the
    /// agent-row persistence failure path (before any worktree, token or
    /// budget slot exists, but after the branch was created).
    struct CreateFailingAgents {
        inner: std::sync::Arc<dyn gyre_ports::AgentRepository>,
    }

    #[axum::async_trait]
    impl gyre_ports::AgentRepository for CreateFailingAgents {
        async fn create(&self, agent: &gyre_domain::Agent) -> anyhow::Result<()> {
            if agent.name.starts_with("preview-") {
                anyhow::bail!("injected agent create failure for {}", agent.name);
            }
            self.inner.create(agent).await
        }
        async fn find_by_id(
            &self,
            id: &gyre_common::Id,
        ) -> anyhow::Result<Option<gyre_domain::Agent>> {
            self.inner.find_by_id(id).await
        }
        async fn find_by_name(&self, name: &str) -> anyhow::Result<Option<gyre_domain::Agent>> {
            self.inner.find_by_name(name).await
        }
        async fn list(&self) -> anyhow::Result<Vec<gyre_domain::Agent>> {
            self.inner.list().await
        }
        async fn list_by_status(
            &self,
            status: &gyre_domain::AgentStatus,
        ) -> anyhow::Result<Vec<gyre_domain::Agent>> {
            self.inner.list_by_status(status).await
        }
        async fn update(&self, agent: &gyre_domain::Agent) -> anyhow::Result<()> {
            self.inner.update(agent).await
        }
        async fn delete(&self, id: &gyre_common::Id) -> anyhow::Result<()> {
            self.inner.delete(id).await
        }
        async fn list_by_workspace(
            &self,
            workspace_id: &gyre_common::Id,
        ) -> anyhow::Result<Vec<gyre_domain::Agent>> {
            self.inner.list_by_workspace(workspace_id).await
        }
        async fn list_by_spawner(
            &self,
            spawned_by: &str,
        ) -> anyhow::Result<Vec<gyre_domain::Agent>> {
            self.inner.list_by_spawner(spawned_by).await
        }
        async fn update_status(
            &self,
            agent_id: &gyre_common::Id,
            status: gyre_domain::AgentStatus,
        ) -> anyhow::Result<()> {
            self.inner.update_status(agent_id, status).await
        }
        async fn record_usage(&self, usage: &gyre_domain::AgentUsage) -> anyhow::Result<()> {
            self.inner.record_usage(usage).await
        }
    }

    /// F3 regression: a `persist_preview` failure must roll back every
    /// provisioned agent. Without the rollback, N Active agent rows, live
    /// tokens, held budget slots, branches and worktrees are orphaned with
    /// no run record — invisible to the GC orphan pass, which iterates the
    /// `preview_agents` index that never finished writing. The failure is
    /// injected at the latest possible step (per-agent index writes, after
    /// the run record and every per-agent token landed).
    #[tokio::test(flavor = "multi_thread")]
    async fn preview_persist_failure_rolls_back_every_provisioned_agent() {
        let dir = tempfile::tempdir().unwrap();
        let (state, repo_id, ws_id) = preview_state(dir.path(), 86_400, None).await;

        let mut failing = (*state).clone();
        failing.kv_store = std::sync::Arc::new(NamespaceFailingKv::new(
            state.kv_store.clone(),
            "preview_agents",
        ));
        let app = crate::api::api_router().with_state(std::sync::Arc::new(failing));

        let resp = post_preview(
            &app,
            preview_body(
                &repo_id,
                &["specs/system/search.md", "specs/system/identity.md"],
            ),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);

        // No preview agent rows survive the failed persist...
        let agents = state.agents.list().await.unwrap();
        let leftovers: Vec<String> = agents
            .iter()
            .filter(|a| a.name.starts_with("preview-"))
            .map(|a| a.name.clone())
            .collect();
        assert!(
            leftovers.is_empty(),
            "persist failure must roll back every provisioned agent row, found {leftovers:?}"
        );
        // ...nor their tokens, index entries, run record, branches, worktrees
        // or budget slots.
        assert!(state
            .kv_store
            .kv_list("preview_agents")
            .await
            .unwrap()
            .is_empty());
        assert!(state
            .kv_store
            .kv_list("meta_spec_previews")
            .await
            .unwrap()
            .is_empty());
        assert!(state
            .kv_store
            .kv_list("agent_tokens")
            .await
            .unwrap()
            .is_empty());
        let repo = state
            .repos
            .find_by_id(&gyre_common::Id::new(&repo_id))
            .await
            .unwrap()
            .unwrap();
        let branches = state.git_ops.list_branches(&repo.path).await.unwrap();
        assert!(
            branches.iter().all(|b| !b.name.starts_with("preview/")),
            "persist failure must delete every preview branch"
        );
        let worktrees = state.git_ops.list_worktrees(&repo.path).await.unwrap();
        assert!(
            worktrees.iter().all(|w| !w.contains("preview")),
            "persist failure must remove every preview worktree"
        );
        let ws_key = crate::api::budget::workspace_key(ws_id.as_str());
        let active = state
            .budget_usages
            .get_usage(&ws_key)
            .await
            .unwrap()
            .map(|u| u.active_agents)
            .unwrap_or(0);
        assert_eq!(
            active, 0,
            "persist failure must release every held workspace budget slot"
        );
    }

    /// A worktree-creation failure must roll back without touching the
    /// budget: the workspace/tenant slots are incremented only after every
    /// provisioning step succeeded, so the rollback must not decrement
    /// counters that were never incremented. A phantom decrement permanently
    /// biases budget enforcement down (one lost slot per occurrence) and
    /// lets later spawns over-admit past max_concurrent_agents.
    #[tokio::test(flavor = "multi_thread")]
    async fn preview_worktree_failure_releases_no_untaken_budget_slot() {
        let dir = tempfile::tempdir().unwrap();
        let (state, repo_id, ws_id) = preview_state(dir.path(), 86_400, None).await;

        // A concurrent agent already holds one workspace slot — the invariant
        // under test is that a failed preview spawn leaves that count alone.
        crate::api::budget::increment_active_agents(&state, ws_id.as_str()).await;

        // Make worktree creation fail: `worktrees` exists as a regular file,
        // so every `git worktree add` beneath it is refused (ENOTDIR) before
        // any checkout happens.
        let repo = state
            .repos
            .find_by_id(&gyre_common::Id::new(&repo_id))
            .await
            .unwrap()
            .unwrap();
        std::fs::write(std::path::Path::new(&repo.path).join("worktrees"), b"").unwrap();

        let app = crate::api::api_router().with_state(state.clone());
        let resp = post_preview(&app, preview_body(&repo_id, &["specs/system/search.md"])).await;
        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);

        // The pre-existing slot is untouched — no phantom decrement of a slot
        // the failed spawn never took (workspace and tenant counters alike).
        let ws_key = crate::api::budget::workspace_key(ws_id.as_str());
        let active = state
            .budget_usages
            .get_usage(&ws_key)
            .await
            .unwrap()
            .map(|u| u.active_agents)
            .unwrap_or(0);
        assert_eq!(
            active, 1,
            "worktree failure must not decrement a budget slot it never took"
        );
        let tenant_key = crate::api::budget::tenant_key();
        let tenant_active = state
            .budget_usages
            .get_usage(tenant_key)
            .await
            .unwrap()
            .map(|u| u.active_agents)
            .unwrap_or(0);
        assert_eq!(
            tenant_active, 1,
            "worktree failure must not decrement the tenant-global slot either"
        );

        // The rollback still removed everything the failed spawn created:
        // branch, agent row, and nothing persisted.
        let branches = state.git_ops.list_branches(&repo.path).await.unwrap();
        assert!(
            branches.iter().all(|b| !b.name.starts_with("preview/")),
            "worktree failure must delete the provisioned preview branch"
        );
        let agents = state.agents.list().await.unwrap();
        assert!(
            agents.iter().all(|a| !a.name.starts_with("preview-")),
            "worktree failure must roll back the provisioned agent row"
        );
        assert!(state
            .kv_store
            .kv_list("meta_spec_previews")
            .await
            .unwrap()
            .is_empty());
    }

    /// An agent-row persistence failure must not orphan the just-created
    /// preview branch: no row ever referenced it, so the GC orphan pass
    /// (which iterates the preview_agents kv index) would never see it and
    /// the branch would linger forever. The branch is deleted inline before
    /// the error propagates.
    #[tokio::test(flavor = "multi_thread")]
    async fn preview_agent_row_failure_deletes_orphaned_branch() {
        let dir = tempfile::tempdir().unwrap();
        let (state, repo_id, _ws_id) = preview_state(dir.path(), 86_400, None).await;

        let mut failing = (*state).clone();
        failing.agents = std::sync::Arc::new(CreateFailingAgents {
            inner: state.agents.clone(),
        });
        let app = crate::api::api_router().with_state(std::sync::Arc::new(failing));

        let resp = post_preview(&app, preview_body(&repo_id, &["specs/system/search.md"])).await;
        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);

        let repo = state
            .repos
            .find_by_id(&gyre_common::Id::new(&repo_id))
            .await
            .unwrap()
            .unwrap();
        let branches = state.git_ops.list_branches(&repo.path).await.unwrap();
        assert!(
            branches.iter().all(|b| !b.name.starts_with("preview/")),
            "agent-row create failure must delete the branch it already created"
        );
        // No worktree, token, index entry or run record was ever written —
        // the failure predates all of them.
        let worktrees = state.git_ops.list_worktrees(&repo.path).await.unwrap();
        assert!(worktrees.iter().all(|w| !w.contains("preview")));
        assert!(state
            .kv_store
            .kv_list("agent_tokens")
            .await
            .unwrap()
            .is_empty());
        assert!(state
            .kv_store
            .kv_list("preview_agents")
            .await
            .unwrap()
            .is_empty());
        assert!(state
            .kv_store
            .kv_list("meta_spec_previews")
            .await
            .unwrap()
            .is_empty());
    }

    /// A preview agent that finishes lands in Stopped (never Idle), its
    /// worktree is removed, its token revoked, its budget slot released —
    /// and the produced diff shows up in the status endpoint.
    #[tokio::test(flavor = "multi_thread")]
    async fn preview_agent_finish_teardown_and_diff() {
        let dir = tempfile::tempdir().unwrap();
        let (app, state, repo_id) = preview_app(dir.path(), 86_400, None).await;

        // The "agent": commit a change on the preview branch, then exit. The
        // commit is what the diff endpoint must surface.
        let script_path = dir.path().join("agent-commit.sh");
        std::fs::write(
            &script_path,
            "#!/bin/sh\n\
             cd \"${GYRE_WORK_DIR:-$PWD}\" || cd /workspace || exit 0\n\
             echo 'search v2 under draft persona' >> specs/system/search.md\n\
             git add -A\n\
             git -c user.email=agent@gyre -c user.name=agent commit -m 'preview: apply draft' >/dev/null 2>&1\n\
             git push origin HEAD:refs/heads/\"$GYRE_BRANCH\" >/dev/null 2>&1\n\
             exit 0\n",
        )
        .unwrap();
        make_executable(&script_path);

        with_agent_command(script_path.to_str().unwrap(), || async {
            let resp =
                post_preview(&app, preview_body(&repo_id, &["specs/system/search.md"])).await;
            assert_eq!(resp.status(), StatusCode::ACCEPTED);
            let json = body_json(resp).await;
            let preview_id = json["preview_id"].as_str().unwrap().to_string();
            let agent_id = json["agents"][0]["agent_id"].as_str().unwrap().to_string();
            let branch = json["agents"][0]["branch"].as_str().unwrap().to_string();

            let repo = state
                .repos
                .find_by_id(&gyre_common::Id::new(&repo_id))
                .await
                .unwrap()
                .unwrap();

            // Wait for the monitor to observe the exit and run the finish
            // path (poll interval is 2s for local targets).
            let mut terminal = None;
            for _ in 0..150 {
                if let Ok(Some(a)) = state
                    .agents
                    .find_by_id(&gyre_common::Id::new(&agent_id))
                    .await
                {
                    if a.status != gyre_domain::AgentStatus::Active {
                        terminal = Some(a);
                        break;
                    }
                }
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
            let agent = terminal.expect("preview agent must leave Active after process exit");

            // Terminal state is Stopped — never Idle (§5: no idle state).
            assert_eq!(
                agent.status,
                gyre_domain::AgentStatus::Stopped,
                "finished preview agent must be Stopped, not {:?}",
                agent.status
            );

            // Worktree removed on completion; branch survives for diffing.
            let wt_path = format!("{}/worktrees/{}", repo.path, branch.replace('/', "-"));
            assert!(
                !std::path::Path::new(&wt_path).exists(),
                "worktree removed on finish"
            );
            assert!(
                state
                    .git_ops
                    .branch_exists(&repo.path, &branch)
                    .await
                    .unwrap(),
                "branch survives for diffing"
            );
            assert!(
                state
                    .kv_store
                    .kv_get("agent_tokens", &agent_id)
                    .await
                    .unwrap()
                    .is_none(),
                "token revoked on finish"
            );

            // Status reports complete + the produced diff.
            let status = body_json(get_preview_status(&app, &preview_id).await).await;
            assert_eq!(status["state"].as_str().unwrap(), "complete");
            let agents = status["agents"].as_array().unwrap();
            assert_eq!(agents[0]["status"].as_str().unwrap(), "complete");
            let diff = agents[0]["diff"].as_object().expect("diff must be present");
            assert!(
                diff["files_changed"].as_u64().unwrap() >= 1,
                "diff must show the agent's commit: {diff:?}"
            );
            let patches = diff["patches"].as_array().unwrap();
            assert!(
                patches
                    .iter()
                    .any(|p| p["path"].as_str().unwrap() == "specs/system/search.md"),
                "diff must include the changed spec: {patches:?}"
            );
        })
        .await;
    }

    /// message-bus.md §Delivery Guarantees (Agent completion cleanup)
    /// meets preview skip-ceremony: a preview agent's ONLY completion path
    /// is the process exit (`POST /agents/:id/complete` and the MCP
    /// equivalent refuse preview agents), and the DELETE/GC teardown kills
    /// without completion. Both terminal paths must bulk-ack the agent's
    /// unacked Directed inbox — otherwise the rows stay redeliverable
    /// forever: the stale-agent detector only acks agents it marks Dead
    /// (preview agents land in Stopped), and expire_acked_inboxes reclaims
    /// only acked rows. Fails on a finish/teardown path that skips
    /// acknowledge_agent_inbox.
    #[tokio::test(flavor = "multi_thread")]
    async fn preview_terminal_paths_bulk_ack_directed_inbox() {
        use gyre_common::message::{Destination, Message, MessageKind, MessageOrigin};

        let dir = tempfile::tempdir().unwrap();
        let (state, repo_id, ws_id) = preview_state(dir.path(), 86_400, None).await;
        let app = crate::api::api_router().with_state(state.clone());

        // A sleeping agent: the test drives both terminal paths explicitly
        // (the process-exit finish path directly, the DELETE kill via the
        // endpoint), so the runtime must still be alive when they run.
        let script_path = dir.path().join("agent-sleep.sh");
        std::fs::write(&script_path, "#!/bin/sh\nsleep 30\n").unwrap();
        make_executable(&script_path);

        with_agent_command(script_path.to_str().unwrap(), || async {
            let resp = post_preview(
                &app,
                preview_body(
                    &repo_id,
                    &["specs/system/search.md", "specs/system/identity.md"],
                ),
            )
            .await;
            assert_eq!(resp.status(), StatusCode::ACCEPTED);
            let json = body_json(resp).await;
            let preview_id = json["preview_id"].as_str().unwrap().to_string();
            let agent0 = json["agents"][0]["agent_id"].as_str().unwrap().to_string();
            let agent1 = json["agents"][1]["agent_id"].as_str().unwrap().to_string();

            // Both runtimes registered before the terminal paths run.
            soon(|| {
                let reg = state.process_registry.try_lock().ok()?;
                reg.contains_key(&agent0).then_some(())?;
                reg.contains_key(&agent1).then_some(())
            })
            .await;

            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64;
            // Aged past the reclamation cutoff below so the reason probe can
            // use a short window (mirrors complete_agent_bulk_acks_directed_inbox).
            let created_at = now - 10_000;
            let directed = |id: &str, agent: &str| Message {
                id: gyre_common::Id::new(id.to_string()),
                tenant_id: gyre_common::Id::new("default"),
                from: MessageOrigin::Server,
                workspace_id: Some(ws_id.clone()),
                to: Destination::Agent(gyre_common::Id::new(agent.to_string())),
                kind: MessageKind::TaskAssignment,
                payload: None,
                created_at,
                signature: None,
                key_id: None,
                acknowledged: false,
            };

            // Two unacked Directed messages in agent0's inbox, one in
            // agent1's — the sibling must survive the first terminal path.
            state
                .messages
                .store(&directed("preview-ack-a1", &agent0))
                .await
                .unwrap();
            state
                .messages
                .store(&directed("preview-ack-a2", &agent0))
                .await
                .unwrap();
            state
                .messages
                .store(&directed("preview-ack-b1", &agent1))
                .await
                .unwrap();
            assert_eq!(
                state
                    .messages
                    .count_unacked(&gyre_common::Id::new(&agent0))
                    .await
                    .unwrap(),
                2,
                "precondition: two unacked Directed messages before the exit path"
            );

            // Process-exit terminal path: this is what every compute-target
            // monitor calls when a preview agent's runtime goes away.
            assert!(
                super::finish_preview_agent(&state, &agent0).await,
                "agent0 is a preview agent"
            );

            assert_eq!(
                state
                    .messages
                    .count_unacked(&gyre_common::Id::new(&agent0))
                    .await
                    .unwrap(),
                0,
                "the process-exit finish path must bulk-ack the inbox"
            );
            for msg_id in ["preview-ack-a1", "preview-ack-a2"] {
                let msg = state
                    .messages
                    .find_by_id(&gyre_common::Id::new(msg_id))
                    .await
                    .unwrap()
                    .unwrap_or_else(|| panic!("{msg_id} must remain stored for audit"));
                assert!(msg.acknowledged, "{msg_id} must be acknowledged");
            }
            let sibling = state
                .messages
                .find_by_id(&gyre_common::Id::new("preview-ack-b1"))
                .await
                .unwrap()
                .expect("sibling preview agent's message must survive the exit path");
            assert!(!sibling.acknowledged);

            // The recorded reason is a lifecycle reason: expire_acked_inboxes
            // matches only agent_completed/agent_orphaned, so reclaiming the
            // two rows proves the finish path wrote one of them (not
            // "explicit", not nothing).
            let deleted = state
                .messages
                .expire_acked_inboxes(now - 1_000)
                .await
                .unwrap();
            assert_eq!(
                deleted, 2,
                "dead-inbox retention must reclaim exactly the finished agent's inbox"
            );

            // DELETE terminal path: the teardown kills without completion —
            // the exit monitor cannot run finish_preview_agent for a handle
            // the kill already removed, so the ack must happen in the
            // teardown itself.
            let del = delete_preview(&app, &preview_id).await;
            assert_eq!(del.status(), StatusCode::NO_CONTENT);
            assert_eq!(
                state
                    .messages
                    .count_unacked(&gyre_common::Id::new(&agent1))
                    .await
                    .unwrap(),
                0,
                "the DELETE teardown must bulk-ack the killed agent's inbox"
            );
            let killed = state
                .messages
                .find_by_id(&gyre_common::Id::new("preview-ack-b1"))
                .await
                .unwrap()
                .expect("killed agent's message must remain stored for audit");
            assert!(killed.acknowledged);
        })
        .await;
    }

    /// Regression (task-206 review round 05ab61cb — preview budget slot
    /// double-release): the generic stop/fail routes release a preview
    /// agent's workspace/tenant slot themselves, and every preview-owned
    /// release that follows — the process-exit finish path, then the
    /// human's DELETE — must not release the same slot again. Before the
    /// fix, neither the exit path nor DELETE consulted a flag the generic
    /// route had set, so one stopped preview agent biased `active_agents`
    /// down by one permanently (decrement is MAX(0, n-1)) while the
    /// background agent kept running — later spawns then over-admit past
    /// max_concurrent_agents, the exact blocking class the budget exists
    /// to prevent.
    ///
    /// Reproduction shape from the review: background agent B holds one
    /// slot; POST /meta-specs/preview adds two preview agents (active=3);
    /// the operator stops one and fails the other through the generic
    /// routes (active=1); each process later exits and finish_preview_agent
    /// runs (must stay 1 — the bug dropped the counter to 0 with B still
    /// live); the human clears the run via DELETE (must stay 1).
    #[tokio::test(flavor = "multi_thread")]
    async fn generic_stop_fail_release_preview_slot_exactly_once() {
        let dir = tempfile::tempdir().unwrap();
        let (state, repo_id, ws_id) = preview_state(dir.path(), 86_400, None).await;
        let app = crate::api::api_router().with_state(state.clone());

        // Long-lived agent processes: the generic stop/fail routes are
        // ledger transitions that do NOT kill the process, so the scripts
        // must outlive the route calls; the test then drives the exit
        // transition directly (the same `on_agent_process_exit` every
        // compute-target monitor calls after observing a real exit).
        let script = dir.path().join("agent-sleep.sh");
        std::fs::write(&script, "#!/bin/sh\nexec sleep 300\n").unwrap();
        make_executable(&script);

        with_agent_command(script.to_str().unwrap(), || async {
            async fn active_agents(state: &crate::AppState, key: &str) -> u32 {
                state
                    .budget_usages
                    .get_usage(key)
                    .await
                    .unwrap()
                    .map(|u| u.active_agents)
                    .unwrap_or(0)
            }
            let ws_key = crate::api::budget::workspace_key(ws_id.as_str());
            let tenant_key = crate::api::budget::tenant_key();

            // Background agent B holds one slot for the whole test.
            crate::api::budget::increment_active_agents(&state, ws_id.as_str()).await;
            assert_eq!(active_agents(&state, &ws_key).await, 1);

            // Two targets → two preview agents: one stopped, one failed
            // through the generic routes.
            let resp = post_preview(
                &app,
                preview_body(
                    &repo_id,
                    &["specs/system/search.md", "specs/system/identity.md"],
                ),
            )
            .await;
            assert_eq!(resp.status(), StatusCode::ACCEPTED);
            let json = body_json(resp).await;
            let preview_id = json["preview_id"].as_str().unwrap().to_string();
            let stopped_id = json["agents"][0]["agent_id"].as_str().unwrap().to_string();
            let failed_id = json["agents"][1]["agent_id"].as_str().unwrap().to_string();
            assert_eq!(active_agents(&state, &ws_key).await, 3);
            assert_eq!(active_agents(&state, tenant_key).await, 3);

            // Generic terminal routes — the exact production paths an
            // operator uses on any agent, preview or not.
            for (suffix, id) in [("/stop", &stopped_id), ("/fail", &failed_id)] {
                let resp = app
                    .clone()
                    .oneshot(
                        Request::builder()
                            .method("POST")
                            .uri(format!("/api/v1/agents/{id}{suffix}"))
                            .header("authorization", "Bearer test-token")
                            .body(Body::empty())
                            .unwrap(),
                    )
                    .await
                    .unwrap();
                assert_eq!(
                    resp.status(),
                    StatusCode::OK,
                    "generic {suffix} must succeed for a preview agent"
                );
            }
            // One decrement per agent, exactly — B plus nothing else live.
            assert_eq!(active_agents(&state, &ws_key).await, 1);
            assert_eq!(active_agents(&state, tenant_key).await, 1);

            // The processes later exit: the monitor's real transition. The
            // preview finish path must see the flags the generic routes
            // wrote and release nothing.
            crate::api::spawn::on_agent_process_exit(&state, &stopped_id).await;
            crate::api::spawn::on_agent_process_exit(&state, &failed_id).await;
            assert_eq!(
                active_agents(&state, &ws_key).await,
                1,
                "process exit must not re-release slots the generic routes released"
            );
            assert_eq!(active_agents(&state, tenant_key).await, 1);

            // Terminal rows stay terminal — no Idle resurrection.
            for (id, want) in [
                (&stopped_id, gyre_domain::AgentStatus::Stopped),
                (&failed_id, gyre_domain::AgentStatus::Failed),
            ] {
                let row = state
                    .agents
                    .find_by_id(&gyre_common::Id::new(id))
                    .await
                    .unwrap()
                    .unwrap();
                assert_eq!(row.status, want, "row must stay terminal after exit");
            }

            // The human clears the run (the UI "Clear" path from the
            // review repro): DELETE teardown must release nothing either.
            let resp = delete_preview(&app, &preview_id).await;
            assert_eq!(resp.status(), StatusCode::NO_CONTENT);
            assert_eq!(
                active_agents(&state, &ws_key).await,
                1,
                "DELETE must not re-release slots the generic routes released"
            );
            assert_eq!(active_agents(&state, tenant_key).await, 1);
        })
        .await;
    }

    /// The other half of the exactly-once contract: the admin kill and the
    /// budget-enforcement stop release the slot AND kill the process (they
    /// consume the registry handle), so the exit monitor fires the preview
    /// finish path and the 24h GC sweep later tears the run down — each
    /// must see the flag the generic path wrote and release nothing.
    /// Without the flag, either path double-decrements the same slot
    /// (decrement is MAX(0, n-1) — permanent downward bias, over-admission
    /// past max_concurrent_agents).
    #[tokio::test(flavor = "multi_thread")]
    async fn generic_kill_and_budget_stop_release_exactly_once_across_gc() {
        let dir = tempfile::tempdir().unwrap();
        let (state, repo_id, ws_id) = preview_state(dir.path(), 86_400, None).await;
        let app = crate::api::api_router().with_state(state.clone());

        let script = dir.path().join("agent-sleep.sh");
        std::fs::write(&script, "#!/bin/sh\nexec sleep 300\n").unwrap();
        make_executable(&script);

        with_agent_command(script.to_str().unwrap(), || async {
            async fn active_agents(state: &crate::AppState, key: &str) -> u32 {
                state
                    .budget_usages
                    .get_usage(key)
                    .await
                    .unwrap()
                    .map(|u| u.active_agents)
                    .unwrap_or(0)
            }
            let ws_key = crate::api::budget::workspace_key(ws_id.as_str());
            let tenant_key = crate::api::budget::tenant_key();

            // Background agent B holds one slot for the whole test.
            crate::api::budget::increment_active_agents(&state, ws_id.as_str()).await;

            let resp = post_preview(
                &app,
                preview_body(
                    &repo_id,
                    &["specs/system/search.md", "specs/system/identity.md"],
                ),
            )
            .await;
            assert_eq!(resp.status(), StatusCode::ACCEPTED);
            let json = body_json(resp).await;
            let preview_id = json["preview_id"].as_str().unwrap().to_string();
            let killed_id = json["agents"][0]["agent_id"].as_str().unwrap().to_string();
            let budget_stopped_id = json["agents"][1]["agent_id"].as_str().unwrap().to_string();
            assert_eq!(active_agents(&state, &ws_key).await, 3);

            // Admin kill through the production REST route: decrements the
            // slot, kills the process, consumes the registry handle.
            let resp = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri(format!("/api/v1/admin/agents/{killed_id}/kill"))
                        .header("authorization", "Bearer test-token")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::NO_CONTENT);
            assert_eq!(active_agents(&state, &ws_key).await, 2);

            // Budget-enforcement stop on the second agent (grace-expiry
            // hard stop): same release-and-kill shape.
            let row = state
                .agents
                .find_by_id(&gyre_common::Id::new(&budget_stopped_id))
                .await
                .unwrap()
                .unwrap();
            crate::api::budget::budget_stop_agent(&state, &row, "budget test stop").await;
            assert_eq!(active_agents(&state, &ws_key).await, 1);
            assert_eq!(active_agents(&state, tenant_key).await, 1);

            // The killed processes exit: the monitor's real transition for
            // both agents. The preview finish path must release nothing.
            crate::api::spawn::on_agent_process_exit(&state, &killed_id).await;
            crate::api::spawn::on_agent_process_exit(&state, &budget_stopped_id).await;
            assert_eq!(
                active_agents(&state, &ws_key).await,
                1,
                "process exit must not re-release slots the kill/budget-stop released"
            );
            assert_eq!(active_agents(&state, tenant_key).await, 1);

            // Age the run past its TTL and sweep: the GC teardown must
            // release nothing either, and the run is gone entirely.
            let raw = state
                .kv_store
                .kv_get("meta_spec_previews", &preview_id)
                .await
                .unwrap()
                .unwrap();
            let mut record: serde_json::Value = serde_json::from_str(&raw).unwrap();
            record["created_at"] = serde_json::json!(0);
            state
                .kv_store
                .kv_set("meta_spec_previews", &preview_id, record.to_string())
                .await
                .unwrap();
            super::run_once(&state).await.unwrap();

            assert!(
                state
                    .kv_store
                    .kv_get("meta_spec_previews", &preview_id)
                    .await
                    .unwrap()
                    .is_none(),
                "aged run must be swept"
            );
            assert_eq!(
                active_agents(&state, &ws_key).await,
                1,
                "GC must not re-release slots the kill/budget-stop released"
            );
            assert_eq!(active_agents(&state, tenant_key).await, 1);
        })
        .await;
    }

    /// The stale-agent-abort leg of the exactly-once slot release: when the
    /// heartbeat detector aborts an Active preview agent (Dead transition),
    /// it decrements the workspace slot itself and must flag the preview
    /// slot released (`mark_preview_slot_released`) so every preview-owned
    /// release that can still observe the run — the process-exit monitor,
    /// DELETE teardown and the GC sweep — releases nothing. Without the
    /// flag, each of them double-decrements the same slot (decrement is
    /// MAX(0, n-1): permanent downward bias, over-admission past
    /// max_concurrent_agents).
    #[tokio::test(flavor = "multi_thread")]
    async fn stale_abort_releases_preview_slot_exactly_once() {
        let dir = tempfile::tempdir().unwrap();
        let (state, repo_id, ws_id) = preview_state(dir.path(), 86_400, None).await;
        let app = crate::api::api_router().with_state(state.clone());

        // Long-lived processes: the stale transition must not kill them, so
        // the exit-monitor leg runs against a genuinely registered process.
        let script = dir.path().join("agent-sleep.sh");
        std::fs::write(&script, "#!/bin/sh\nexec sleep 300\n").unwrap();
        make_executable(&script);

        with_agent_command(script.to_str().unwrap(), || async {
            async fn active_agents(state: &crate::AppState, key: &str) -> u32 {
                state
                    .budget_usages
                    .get_usage(key)
                    .await
                    .unwrap()
                    .map(|u| u.active_agents)
                    .unwrap_or(0)
            }
            async fn released_flag(state: &crate::AppState, agent_id: &str) -> bool {
                let raw = state
                    .kv_store
                    .kv_get("preview_agents", agent_id)
                    .await
                    .unwrap()
                    .expect("preview agent index entry must exist");
                serde_json::from_str::<serde_json::Value>(&raw).unwrap()["released"]
                    .as_bool()
                    .unwrap()
            }
            let ws_key = crate::api::budget::workspace_key(ws_id.as_str());

            // Stale-abort an Active preview agent: Abort behavior + a
            // heartbeat aged past the detector's 60 s window.
            async fn stale_abort(state: &std::sync::Arc<crate::AppState>, agent_id: &str) {
                let mut aged = state
                    .agents
                    .find_by_id(&gyre_common::Id::new(agent_id))
                    .await
                    .unwrap()
                    .unwrap();
                aged.disconnected_behavior = gyre_domain::DisconnectedBehavior::Abort;
                aged.spawned_at = aged.spawned_at.saturating_sub(10_000);
                aged.last_heartbeat = None;
                state.agents.update(&aged).await.unwrap();
                crate::stale_agents::run_once(state).await.unwrap();
            }

            // Run A: exit monitor + DELETE after the stale abort.
            let resp =
                post_preview(&app, preview_body(&repo_id, &["specs/system/search.md"])).await;
            assert_eq!(resp.status(), StatusCode::ACCEPTED);
            let json = body_json(resp).await;
            let a_id = json["agents"][0]["agent_id"].as_str().unwrap().to_string();
            assert_eq!(active_agents(&state, &ws_key).await, 1);

            stale_abort(&state, &a_id).await;
            let row = state
                .agents
                .find_by_id(&gyre_common::Id::new(&a_id))
                .await
                .unwrap()
                .unwrap();
            assert_eq!(row.status, gyre_domain::AgentStatus::Dead);
            assert_eq!(
                active_agents(&state, &ws_key).await,
                0,
                "the stale abort must decrement the slot exactly once"
            );
            assert!(
                released_flag(&state, &a_id).await,
                "the stale abort must write the preview released flag (mark_preview_slot_released)"
            );

            // The process later exits: the monitor's real transition. The
            // preview finish path must see the flag and release nothing.
            crate::api::spawn::on_agent_process_exit(&state, &a_id).await;
            assert_eq!(
                active_agents(&state, &ws_key).await,
                0,
                "process exit must not re-release a slot the stale abort released"
            );

            // Human clears the run: DELETE teardown must release nothing.
            let resp = delete_preview(&app, json["preview_id"].as_str().unwrap()).await;
            assert_eq!(resp.status(), StatusCode::NO_CONTENT);
            assert_eq!(
                active_agents(&state, &ws_key).await,
                0,
                "DELETE must not re-release a slot the stale abort released"
            );

            // Run B: the GC sweep after the stale abort.
            let resp =
                post_preview(&app, preview_body(&repo_id, &["specs/system/identity.md"])).await;
            assert_eq!(resp.status(), StatusCode::ACCEPTED);
            let json = body_json(resp).await;
            let b_preview_id = json["preview_id"].as_str().unwrap().to_string();
            let b_id = json["agents"][0]["agent_id"].as_str().unwrap().to_string();
            assert_eq!(active_agents(&state, &ws_key).await, 1);

            stale_abort(&state, &b_id).await;
            assert_eq!(active_agents(&state, &ws_key).await, 0);
            assert!(released_flag(&state, &b_id).await);

            // Age the run past its TTL and sweep: the GC teardown must
            // release nothing and reclaim the run.
            let raw = state
                .kv_store
                .kv_get("meta_spec_previews", &b_preview_id)
                .await
                .unwrap()
                .unwrap();
            let mut record: serde_json::Value = serde_json::from_str(&raw).unwrap();
            record["created_at"] = serde_json::json!(0);
            state
                .kv_store
                .kv_set("meta_spec_previews", &b_preview_id, record.to_string())
                .await
                .unwrap();
            super::run_once(&state).await.unwrap();
            assert!(
                state
                    .kv_store
                    .kv_get("meta_spec_previews", &b_preview_id)
                    .await
                    .unwrap()
                    .is_none(),
                "aged run must be swept"
            );
            assert_eq!(
                active_agents(&state, &ws_key).await,
                0,
                "GC must not re-release a slot the stale abort released"
            );
        })
        .await;
    }

    /// Role and target validation: read-only caller 403, unknown repo 404,
    /// unknown spec path 400, empty targets 400, empty draft 400.
    #[tokio::test(flavor = "multi_thread")]
    async fn preview_validation_and_role_gates() {
        let dir = tempfile::tempdir().unwrap();
        let (app, _state, repo_id) = preview_app(dir.path(), 86_400, None).await;

        // Empty targets.
        let resp = post_preview(&app, preview_body(&repo_id, &[])).await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        // Empty draft content.
        let mut bad_draft = preview_body(&repo_id, &["specs/system/search.md"]);
        bad_draft["draft"]["content"] = serde_json::json!("  ");
        let resp = post_preview(&app, bad_draft).await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        // Unknown draft kind shape (documented 400): the runner can only
        // interpret the draft as one of the four meta-spec kinds.
        let mut bad_kind = preview_body(&repo_id, &["specs/system/search.md"]);
        bad_kind["draft"]["kind"] = serde_json::json!("feature:not-a-meta-kind");
        let resp = post_preview(&app, bad_kind).await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        // Unknown repo.
        let resp = post_preview(
            &app,
            preview_body(
                "00000000-0000-0000-0000-000000000000",
                &["specs/system/search.md"],
            ),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);

        // Spec path that does not exist in the repo.
        let resp = post_preview(
            &app,
            preview_body(&repo_id, &["specs/system/does-not-exist.md"]),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        // Read-only caller → 403 (no agents spawned, so no env lock needed).
        use crate::abac_middleware::seed_builtin_policies;
        use crate::auth::test_helpers::{make_test_state_with_jwt, sign_test_jwt};
        let jwt_state = make_test_state_with_jwt();
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current()
                .block_on(seed_builtin_policies(&jwt_state))
                .expect("seed built-in policies")
        });
        let ro_token = sign_test_jwt(
            &serde_json::json!({
                "tenant_id": "default",
                "sub": "ro-sub",
                "preferred_username": "readonly-user",
                "realm_access": { "roles": ["readonly"] }
            }),
            3600,
        );
        let ro_app = crate::api::api_router().with_state(jwt_state);
        let resp = ro_app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/meta-specs/preview")
                    .header("authorization", format!("Bearer {ro_token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        preview_body(&repo_id, &["specs/system/search.md"]).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);

        // Unknown preview id on GET/DELETE → 404.
        let resp = get_preview_status(&app, "00000000-0000-0000-0000-000000000000").await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        let resp = delete_preview(&app, "00000000-0000-0000-0000-000000000000").await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    /// Cross-tenant and repo-ABAC containment on the global POST route.
    ///
    /// The route is id-less (`POST /api/v1/meta-specs/preview` carries the
    /// targets in the body), so no resolver layer can derive a resource
    /// tenant for it — the per-target containment in `post_meta_spec_preview`
    /// (`check_repo_abac` plus the tenant check against the repo's
    /// workspace) is the only guard between a Developer from tenant A and
    /// preview compute against tenant B's repos. Both denial branches are
    /// asserted on their handler-visible error text, so a 403 from any other
    /// surface cannot satisfy the test, and an in-tenant Developer whose
    /// claims satisfy the repo's ABAC policy is the positive control proving
    /// the route works for the authorized caller (the 403s are containment,
    /// not a broken route).
    #[tokio::test(flavor = "multi_thread")]
    async fn preview_post_cross_tenant_and_abac_denial_are_forbidden() {
        use crate::auth::test_helpers::{make_jwt_config, sign_test_jwt};

        let dir = tempfile::tempdir().unwrap();
        let (state, repo_id, _ws_id) = preview_state(dir.path(), 86_400, None).await;
        // A JWT-enabled twin of the same state: the port stores are
        // Arc-shared, so both routers see the same repos, agents and kv
        // records, and the global test-token stays an Admin bypass on it.
        let mut jwt_state = (*state).clone();
        jwt_state.jwt_config = Some(make_jwt_config());
        let jwt_state = std::sync::Arc::new(jwt_state);
        let app = crate::api::api_router().with_state(jwt_state.clone());

        let dev_tenant_b = sign_test_jwt(
            &serde_json::json!({
                "tenant_id": "tenant-b",
                "sub": "dev-tenant-b",
                "preferred_username": "dev-tenant-b",
                "realm_access": { "roles": ["developer"] }
            }),
            3600,
        );
        let dev_tenant_default = sign_test_jwt(
            &serde_json::json!({
                "tenant_id": "default",
                "sub": "dev-default",
                "preferred_username": "dev-default",
                "realm_access": { "roles": ["developer"] }
            }),
            3600,
        );

        // 1. Developer from tenant B targeting tenant-default's repo: the
        //    tenant-containment branch must refuse before anything is
        //    provisioned. The repo carries no ABAC policy here, so
        //    check_repo_abac passes and the tenant check is the denial
        //    source.
        let resp = post_preview_with_token(
            &app,
            &dev_tenant_b,
            preview_body(&repo_id, &["specs/system/search.md"]),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
        let json = body_json(resp).await;
        assert_eq!(
            json["error"].as_str().unwrap(),
            format!("repo '{repo_id}' is outside the caller's tenant"),
            "the denial must come from the handler's tenant containment"
        );

        // 2. In-tenant Developer whose claims do not satisfy the repo's
        //    ABAC policy: check_repo_abac must refuse.
        let policy = serde_json::json!([{
            "resource_type": "repo",
            "resource_id": repo_id,
            "required_claims": { "team": "core" }
        }]);
        jwt_state
            .kv_store
            .kv_set("abac_policies", &repo_id, policy.to_string())
            .await
            .unwrap();
        let resp = post_preview_with_token(
            &app,
            &dev_tenant_default,
            preview_body(&repo_id, &["specs/system/search.md"]),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
        let json = body_json(resp).await;
        assert_eq!(
            json["error"].as_str().unwrap(),
            format!("ABAC denied: no policy satisfied for repo {repo_id}"),
            "the denial must come from check_repo_abac"
        );

        // Neither denial provisioned anything: no agent rows, no preview
        // record, no preview branches.
        assert!(
            jwt_state.agents.list().await.unwrap().is_empty(),
            "a denied preview must not leave agent rows behind"
        );
        assert!(
            jwt_state
                .kv_store
                .kv_list("meta_spec_previews")
                .await
                .unwrap()
                .is_empty(),
            "a denied preview must not leave a record behind"
        );
        let repo = jwt_state
            .repos
            .find_by_id(&gyre_common::Id::new(&repo_id))
            .await
            .unwrap()
            .unwrap();
        let branches = jwt_state.git_ops.list_branches(&repo.path).await.unwrap();
        assert!(
            !branches.iter().any(|b| b.name.starts_with("preview/")),
            "a denied preview must not leave branches behind: {branches:?}"
        );

        // 3. Positive control: the same in-tenant Developer with claims the
        //    policy satisfies passes both containment branches and really
        //    spawns (so the 403s above are containment, not something else).
        let dev_tenant_default_core = sign_test_jwt(
            &serde_json::json!({
                "tenant_id": "default",
                "sub": "dev-default-core",
                "preferred_username": "dev-default-core",
                "realm_access": { "roles": ["developer"] },
                "team": "core"
            }),
            3600,
        );
        let script = env_dump_script(dir.path(), &dir.path().join("agent-env.txt"));
        with_agent_command(script.to_str().unwrap(), || async {
            let resp = post_preview_with_token(
                &app,
                &dev_tenant_default_core,
                preview_body(&repo_id, &["specs/system/search.md"]),
            )
            .await;
            assert_eq!(
                resp.status(),
                StatusCode::ACCEPTED,
                "an in-tenant Developer satisfying the repo policy must be able to run a preview"
            );
            let json = body_json(resp).await;
            let preview_id = json["preview_id"].as_str().unwrap().to_string();
            let del = delete_preview(&app, &preview_id).await;
            assert_eq!(del.status(), StatusCode::NO_CONTENT);
        })
        .await;
    }

    /// Cross-tenant and repo-ABAC containment for GET status and DELETE:
    /// `require_preview_scope` derives the run's scope from its own repos,
    /// so a Developer from another tenant must be refused on both routes,
    /// an in-tenant Developer whose ABAC policy denies the repo must be
    /// refused too, and the refused DELETEs must tear nothing down — the
    /// authorized admin can still read and finally delete the run.
    #[tokio::test(flavor = "multi_thread")]
    async fn preview_status_and_delete_cross_tenant_are_forbidden() {
        use crate::auth::test_helpers::{make_jwt_config, sign_test_jwt};

        let dir = tempfile::tempdir().unwrap();
        let (state, repo_id, _ws_id) = preview_state(dir.path(), 86_400, None).await;
        let mut jwt_state = (*state).clone();
        jwt_state.jwt_config = Some(make_jwt_config());
        let jwt_state = std::sync::Arc::new(jwt_state);
        let app = crate::api::api_router().with_state(jwt_state.clone());

        let dev_tenant_b = sign_test_jwt(
            &serde_json::json!({
                "tenant_id": "tenant-b",
                "sub": "dev-tenant-b",
                "preferred_username": "dev-tenant-b",
                "realm_access": { "roles": ["developer"] }
            }),
            3600,
        );
        let dev_tenant_default = sign_test_jwt(
            &serde_json::json!({
                "tenant_id": "default",
                "sub": "dev-default",
                "preferred_username": "dev-default",
                "realm_access": { "roles": ["developer"] }
            }),
            3600,
        );

        let script = env_dump_script(dir.path(), &dir.path().join("agent-env.txt"));
        with_agent_command(script.to_str().unwrap(), || async {
            // Admin creates a real run to probe against.
            let resp =
                post_preview(&app, preview_body(&repo_id, &["specs/system/search.md"])).await;
            assert_eq!(resp.status(), StatusCode::ACCEPTED);
            let json = body_json(resp).await;
            let preview_id = json["preview_id"].as_str().unwrap().to_string();

            // Positive control: the in-tenant Developer can read the status.
            let resp = get_preview_status_with_token(&app, &preview_id, &dev_tenant_default).await;
            assert_eq!(
                resp.status(),
                StatusCode::OK,
                "an in-tenant Developer must be able to read the preview status"
            );

            // Cross-tenant Developer: GET refused by the tenant branch (the
            // repo carries no ABAC policy here, so the tenant check is the
            // denial source).
            let resp = get_preview_status_with_token(&app, &preview_id, &dev_tenant_b).await;
            assert_eq!(resp.status(), StatusCode::FORBIDDEN);
            let json = body_json(resp).await;
            assert_eq!(
                json["error"].as_str().unwrap(),
                format!("preview '{preview_id}' is outside the caller's tenant"),
                "the denial must come from require_preview_scope's tenant containment"
            );
            // ... and DELETE refused by the same containment.
            let resp = delete_preview_with_token(&app, &preview_id, &dev_tenant_b).await;
            assert_eq!(resp.status(), StatusCode::FORBIDDEN);

            // ABAC denial inside require_preview_scope: a repo policy the
            // in-tenant Developer does not satisfy refuses both routes.
            let policy = serde_json::json!([{
                "resource_type": "repo",
                "resource_id": repo_id,
                "required_claims": { "team": "core" }
            }]);
            jwt_state
                .kv_store
                .kv_set("abac_policies", &repo_id, policy.to_string())
                .await
                .unwrap();
            let resp = get_preview_status_with_token(&app, &preview_id, &dev_tenant_default).await;
            assert_eq!(resp.status(), StatusCode::FORBIDDEN);
            let json = body_json(resp).await;
            assert_eq!(
                json["error"].as_str().unwrap(),
                format!("ABAC denied: no policy satisfied for repo {repo_id}"),
                "the denial must come from require_preview_scope's repo ABAC check"
            );
            let resp = delete_preview_with_token(&app, &preview_id, &dev_tenant_default).await;
            assert_eq!(resp.status(), StatusCode::FORBIDDEN);
            jwt_state
                .kv_store
                .kv_remove("abac_policies", &repo_id)
                .await
                .unwrap();

            // The refused DELETEs tore nothing down: the record and the
            // status are still there for the authorized caller.
            let resp = get_preview_status(&app, &preview_id).await;
            assert_eq!(resp.status(), StatusCode::OK);
            assert!(
                jwt_state
                    .kv_store
                    .kv_get("meta_spec_previews", &preview_id)
                    .await
                    .unwrap()
                    .is_some(),
                "a refused DELETE must not remove the preview record"
            );

            // Admin cleanup (also kills the spawned process).
            let resp = delete_preview(&app, &preview_id).await;
            assert_eq!(resp.status(), StatusCode::NO_CONTENT);
        })
        .await;
    }

    /// Workspace-scoped POST containment: the path's workspace must own every
    /// target repo (403 otherwise), must exist (404), and must be inside the
    /// caller's tenant (403 for a foreign-tenant Developer). No denial may
    /// provision anything. The positive control really spawns through the
    /// workspace route and tears down through it too.
    #[tokio::test(flavor = "multi_thread")]
    async fn workspace_preview_post_scopes_targets_to_the_path_workspace() {
        use crate::auth::test_helpers::{make_jwt_config, sign_test_jwt};

        let dir = tempfile::tempdir().unwrap();
        let (state, repo_id, ws_id) = preview_state(dir.path(), 86_400, None).await;

        // A second workspace in the SAME tenant: same-tenant callers can see
        // it, but it does not own the seeded repo.
        let ws_b = gyre_domain::Workspace::new(
            gyre_common::Id::new(uuid::Uuid::new_v4().to_string()),
            gyre_common::Id::new("default"),
            "preview-ws-b",
            format!("preview-ws-b-{}", uuid::Uuid::new_v4()),
            0,
        );
        state.workspaces.create(&ws_b).await.unwrap();

        let mut jwt_state = (*state).clone();
        jwt_state.jwt_config = Some(make_jwt_config());
        let jwt_state = std::sync::Arc::new(jwt_state);
        let app = crate::api::api_router().with_state(jwt_state.clone());

        let dev_tenant_b = sign_test_jwt(
            &serde_json::json!({
                "tenant_id": "tenant-b",
                "sub": "dev-tenant-b",
                "preferred_username": "dev-tenant-b",
                "realm_access": { "roles": ["developer"] }
            }),
            3600,
        );

        // 1. Target repo owned by another workspace: refused before anything
        //    is provisioned.
        let resp = post_workspace_preview(
            &app,
            ws_b.id.as_str(),
            preview_body(&repo_id, &["specs/system/search.md"]),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
        let json = body_json(resp).await;
        assert_eq!(
            json["error"].as_str().unwrap(),
            format!("repo '{repo_id}' is not in workspace '{}'", ws_b.id),
            "the denial must come from the workspace-scope check"
        );

        // 2. Unknown workspace id.
        let resp = post_workspace_preview(
            &app,
            "no-such-workspace",
            preview_body(&repo_id, &["specs/system/search.md"]),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);

        // 3. Cross-tenant Developer: the workspace itself is outside the
        //    caller's tenant (the repo carries no ABAC policy here, so the
        //    workspace tenant check is the denial source).
        let resp = post_workspace_preview_with_token(
            &app,
            ws_id.as_str(),
            &dev_tenant_b,
            preview_body(&repo_id, &["specs/system/search.md"]),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
        let json = body_json(resp).await;
        assert_eq!(
            json["error"].as_str().unwrap(),
            format!("workspace '{}' is outside the caller's tenant", ws_id),
            "the denial must come from load_workspace_for_caller's tenant containment"
        );

        // None of the denials provisioned anything.
        assert!(jwt_state.agents.list().await.unwrap().is_empty());
        assert!(jwt_state
            .kv_store
            .kv_list("meta_spec_previews")
            .await
            .unwrap()
            .is_empty());
        let repo = jwt_state
            .repos
            .find_by_id(&gyre_common::Id::new(&repo_id))
            .await
            .unwrap()
            .unwrap();
        let branches = jwt_state.git_ops.list_branches(&repo.path).await.unwrap();
        assert!(
            !branches.iter().any(|b| b.name.starts_with("preview/")),
            "a denied workspace-scoped preview must not leave branches behind"
        );

        // 4. Positive control: the owning workspace really spawns through the
        //    workspace route (same machinery as the global one) and the run
        //    is teardown-addressable through the same route.
        let script = env_dump_script(dir.path(), &dir.path().join("agent-env.txt"));
        with_agent_command(script.to_str().unwrap(), || async {
            let resp = post_workspace_preview(
                &app,
                ws_id.as_str(),
                preview_body(&repo_id, &["specs/system/search.md"]),
            )
            .await;
            assert_eq!(
                resp.status(),
                StatusCode::ACCEPTED,
                "the owning workspace must be able to run a preview through the workspace route"
            );
            let json = body_json(resp).await;
            let preview_id = json["preview_id"].as_str().unwrap().to_string();
            let branch = json["agents"][0]["branch"].as_str().unwrap().to_string();
            assert!(
                branch.starts_with(&format!("preview/{preview_id}/")),
                "workspace-route spawns use the same preview branch scheme: {branch}"
            );

            assert_eq!(
                jwt_state.agents.list().await.unwrap().len(),
                1,
                "the workspace route must create a real agent record"
            );
            let branches = jwt_state.git_ops.list_branches(&repo.path).await.unwrap();
            assert!(
                branches.iter().any(|b| b.name == branch),
                "the preview branch must exist in the target repo"
            );

            let del = delete_workspace_preview(&app, ws_id.as_str(), &preview_id).await;
            assert_eq!(del.status(), StatusCode::NO_CONTENT);
        })
        .await;
    }

    /// Workspace-scoped GET/DELETE containment: a run is addressable only
    /// through a workspace that owns its target repos (404 through any other
    /// workspace, with nothing torn down), an unknown preview id is 404, and
    /// the authorized workspace route reads real state and really tears down.
    #[tokio::test(flavor = "multi_thread")]
    async fn workspace_preview_status_and_delete_scope_to_the_path_workspace() {
        let dir = tempfile::tempdir().unwrap();
        let (state, repo_id, ws_id) = preview_state(dir.path(), 86_400, None).await;

        let ws_b = gyre_domain::Workspace::new(
            gyre_common::Id::new(uuid::Uuid::new_v4().to_string()),
            gyre_common::Id::new("default"),
            "preview-ws-b",
            format!("preview-ws-b-{}", uuid::Uuid::new_v4()),
            0,
        );
        state.workspaces.create(&ws_b).await.unwrap();

        let app = crate::api::api_router().with_state(state.clone());

        let script = env_dump_script(dir.path(), &dir.path().join("agent-env.txt"));
        with_agent_command(script.to_str().unwrap(), || async {
            let resp =
                post_preview(&app, preview_body(&repo_id, &["specs/system/search.md"])).await;
            assert_eq!(resp.status(), StatusCode::ACCEPTED);
            let json = body_json(resp).await;
            let preview_id = json["preview_id"].as_str().unwrap().to_string();

            // Unknown preview id through a valid workspace.
            let resp = get_workspace_preview_status(&app, ws_id.as_str(), "no-such-preview").await;
            assert_eq!(resp.status(), StatusCode::NOT_FOUND);

            // The owning workspace reads real state.
            let resp = get_workspace_preview_status(&app, ws_id.as_str(), &preview_id).await;
            assert_eq!(resp.status(), StatusCode::OK);
            let json = body_json(resp).await;
            assert_eq!(json["preview_id"].as_str().unwrap(), preview_id);
            assert_eq!(json["agents"].as_array().unwrap().len(), 1);

            // Another same-tenant workspace: the run is not addressable
            // there, and the refusal leaks nothing about the run.
            let resp = get_workspace_preview_status(&app, ws_b.id.as_str(), &preview_id).await;
            assert_eq!(resp.status(), StatusCode::NOT_FOUND);
            let json = body_json(resp).await;
            assert_eq!(
                json["error"].as_str().unwrap(),
                format!(
                    "preview '{preview_id}' not found in workspace '{}'",
                    ws_b.id
                ),
                "the denial must come from require_preview_in_workspace"
            );

            // The wrong-workspace DELETE refuses AND tears nothing down.
            let resp = delete_workspace_preview(&app, ws_b.id.as_str(), &preview_id).await;
            assert_eq!(resp.status(), StatusCode::NOT_FOUND);
            assert!(
                state
                    .kv_store
                    .kv_get("meta_spec_previews", &preview_id)
                    .await
                    .unwrap()
                    .is_some(),
                "a wrong-workspace DELETE must not remove the preview record"
            );

            // The owning workspace really tears down: record and branch gone.
            let resp = delete_workspace_preview(&app, ws_id.as_str(), &preview_id).await;
            assert_eq!(resp.status(), StatusCode::NO_CONTENT);
            assert!(
                state
                    .kv_store
                    .kv_get("meta_spec_previews", &preview_id)
                    .await
                    .unwrap()
                    .is_none(),
                "the workspace DELETE must remove the preview record"
            );
            let repo = state
                .repos
                .find_by_id(&gyre_common::Id::new(&repo_id))
                .await
                .unwrap()
                .unwrap();
            let branches = state.git_ops.list_branches(&repo.path).await.unwrap();
            assert!(
                !branches.iter().any(|b| b.name.starts_with("preview/")),
                "the workspace DELETE must remove the preview branches"
            );
        })
        .await;
    }

    /// Task-206 review F6: deleting a run's target repos must not strip the
    /// run's scope. `require_preview_scope` used to `continue` past deleted
    /// repos, so a run whose EVERY target repo was deleted performed zero
    /// scope checks — any Developer in any tenant could read and tear down
    /// the record. The agent refs' recorded workspace (captured from the
    /// real repo at spawn time, never caller-supplied) anchors the tenant
    /// check instead.
    #[tokio::test(flavor = "multi_thread")]
    async fn preview_scope_survives_target_repo_deletion() {
        use crate::auth::test_helpers::{make_jwt_config, sign_test_jwt};

        let dir = tempfile::tempdir().unwrap();
        let (state, repo_id, _ws_id) = preview_state(dir.path(), 86_400, None).await;
        let mut jwt_state = (*state).clone();
        jwt_state.jwt_config = Some(make_jwt_config());
        let jwt_state = std::sync::Arc::new(jwt_state);
        let app = crate::api::api_router().with_state(jwt_state.clone());

        let dev_tenant_b = sign_test_jwt(
            &serde_json::json!({
                "tenant_id": "tenant-b",
                "sub": "dev-tenant-b",
                "preferred_username": "dev-tenant-b",
                "realm_access": { "roles": ["developer"] }
            }),
            3600,
        );
        let dev_tenant_default = sign_test_jwt(
            &serde_json::json!({
                "tenant_id": "default",
                "sub": "dev-default",
                "preferred_username": "dev-default",
                "realm_access": { "roles": ["developer"] }
            }),
            3600,
        );

        let script = env_dump_script(dir.path(), &dir.path().join("agent-env.txt"));
        with_agent_command(script.to_str().unwrap(), || async {
            // Admin creates a real run to probe against.
            let resp =
                post_preview(&app, preview_body(&repo_id, &["specs/system/search.md"])).await;
            assert_eq!(resp.status(), StatusCode::ACCEPTED);
            let json = body_json(resp).await;
            let preview_id = json["preview_id"].as_str().unwrap().to_string();

            // Delete the run's ONLY target repo — the scope anchor the
            // pre-fix code consulted. The run and its record remain.
            jwt_state
                .repos
                .delete(&gyre_common::Id::new(&repo_id))
                .await
                .unwrap();
            assert!(
                jwt_state
                    .kv_store
                    .kv_get("meta_spec_previews", &preview_id)
                    .await
                    .unwrap()
                    .is_some(),
                "the preview record must outlive its repo row"
            );

            // Cross-tenant Developer: GET and DELETE must STILL be refused
            // — with every repo deleted, this is exactly the input class
            // that used to skip all scope checks.
            let resp = get_preview_status_with_token(&app, &preview_id, &dev_tenant_b).await;
            assert_eq!(
                resp.status(),
                StatusCode::FORBIDDEN,
                "a deleted repo must not strip the run's tenant containment"
            );
            let resp = delete_preview_with_token(&app, &preview_id, &dev_tenant_b).await;
            assert_eq!(
                resp.status(),
                StatusCode::FORBIDDEN,
                "a deleted repo must not strip the run's teardown containment"
            );
            assert!(
                jwt_state
                    .kv_store
                    .kv_get("meta_spec_previews", &preview_id)
                    .await
                    .unwrap()
                    .is_some(),
                "the refused cross-tenant DELETE must not tear the run down"
            );

            // In-tenant Developer: still allowed — the recorded workspace's
            // tenant matches, so the fix is containment, not a broken route.
            let resp = get_preview_status_with_token(&app, &preview_id, &dev_tenant_default).await;
            assert_eq!(
                resp.status(),
                StatusCode::OK,
                "an in-tenant Developer must still read a run whose repo was deleted"
            );

            // Admin still reads and tears down the orphaned run.
            let resp = get_preview_status(&app, &preview_id).await;
            assert_eq!(resp.status(), StatusCode::OK);
            let resp = delete_preview(&app, &preview_id).await;
            assert_eq!(resp.status(), StatusCode::NO_CONTENT);
        })
        .await;
    }

    /// The separate preview budget cap: with `max_concurrent=1` pinned, a
    /// second concurrent spawn is rejected with 429.
    #[tokio::test(flavor = "multi_thread")]
    async fn preview_budget_cap_rejects_overflow() {
        let dir = tempfile::tempdir().unwrap();
        let (app, _state, repo_id) = preview_app(dir.path(), 86_400, Some(1)).await;

        let env_out = dir.path().join("agent-env.txt");
        let script = env_dump_script(dir.path(), &env_out);

        with_agent_command(script.to_str().unwrap(), || async {
            let resp =
                post_preview(&app, preview_body(&repo_id, &["specs/system/search.md"])).await;
            assert_eq!(resp.status(), StatusCode::ACCEPTED);
            let first = body_json(resp).await;
            let preview_id = first["preview_id"].as_str().unwrap().to_string();

            // One preview agent already running; the cap is 1.
            let resp =
                post_preview(&app, preview_body(&repo_id, &["specs/system/identity.md"])).await;
            assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);

            delete_preview(&app, &preview_id).await;
        })
        .await;
    }

    /// Fail-closed budget enumeration: with the preview cap set, a kv-store
    /// failure while counting running preview agents must REJECT the POST —
    /// not admit unboundedly against an unknown running count (the pre-fix
    /// behavior returned 0 from the count and let the request through).
    #[tokio::test(flavor = "multi_thread")]
    async fn preview_budget_enumeration_failure_fails_closed() {
        let dir = tempfile::tempdir().unwrap();
        let (state, repo_id, _ws_id) = preview_state(dir.path(), 86_400, Some(4)).await;

        let mut failing = (*state).clone();
        failing.kv_store =
            std::sync::Arc::new(ListFailingKv::new(state.kv_store.clone(), "preview_agents"));
        let app = crate::api::api_router().with_state(std::sync::Arc::new(failing));

        let resp = post_preview(&app, preview_body(&repo_id, &["specs/system/search.md"])).await;
        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);

        // Rejected before provisioning: no preview agent rows, no run record,
        // no held slots.
        let agents = state.agents.list().await.unwrap();
        assert!(
            agents.iter().all(|a| !a.name.starts_with("preview-")),
            "a rejected budget check must not leave preview agent rows behind"
        );
        assert!(state
            .kv_store
            .kv_list("meta_spec_previews")
            .await
            .unwrap()
            .is_empty());
    }

    /// GC TTL boundary: a run at/past the TTL is deleted, one just under it
    /// survives. Also covers the orphan pass (agent record whose run record
    /// is gone).
    #[tokio::test(flavor = "multi_thread")]
    async fn preview_gc_ttl_boundary() {
        let dir = tempfile::tempdir().unwrap();
        // TTL = 1000s.
        let (app, state, repo_id) = preview_app(dir.path(), 1000, None).await;

        let env_out = dir.path().join("agent-env.txt");
        let script = env_dump_script(dir.path(), &env_out);

        with_agent_command(script.to_str().unwrap(), || async {
            let resp =
                post_preview(&app, preview_body(&repo_id, &["specs/system/search.md"])).await;
            assert_eq!(resp.status(), StatusCode::ACCEPTED);
            let old = body_json(resp).await;
            let old_id = old["preview_id"].as_str().unwrap().to_string();

            // Age the run past the TTL by rewriting its created_at.
            let raw = state
                .kv_store
                .kv_get("meta_spec_previews", &old_id)
                .await
                .unwrap()
                .unwrap();
            let mut record: serde_json::Value = serde_json::from_str(&raw).unwrap();
            record["created_at"] = serde_json::json!(chrono_now_secs() - 1000);
            state
                .kv_store
                .kv_set("meta_spec_previews", &old_id, record.to_string())
                .await
                .unwrap();

            // A second run, left fresh (created_at = now ⇒ just under TTL).
            let resp =
                post_preview(&app, preview_body(&repo_id, &["specs/system/identity.md"])).await;
            assert_eq!(resp.status(), StatusCode::ACCEPTED);
            let fresh = body_json(resp).await;
            let fresh_id = fresh["preview_id"].as_str().unwrap().to_string();

            super::run_once(&state).await.unwrap();

            // Old run is gone entirely.
            assert!(
                state
                    .kv_store
                    .kv_get("meta_spec_previews", &old_id)
                    .await
                    .unwrap()
                    .is_none(),
                "run at/past TTL must be deleted"
            );
            let old_agent_id = old["agents"][0]["agent_id"].as_str().unwrap().to_string();
            assert!(
                state
                    .kv_store
                    .kv_get("preview_agents", &old_agent_id)
                    .await
                    .unwrap()
                    .is_none(),
                "aged run's agent index entry must be removed"
            );

            // Fresh run survives.
            assert!(
                state
                    .kv_store
                    .kv_get("meta_spec_previews", &fresh_id)
                    .await
                    .unwrap()
                    .is_some(),
                "run just under TTL must be retained"
            );

            // Orphan pass: an agent index entry whose run record is gone is
            // reclaimed (slot, token, branch).
            let fresh_agent = fresh["agents"][0]["agent_id"].as_str().unwrap().to_string();
            state
                .kv_store
                .kv_remove("meta_spec_previews", &fresh_id)
                .await
                .unwrap();
            super::run_once(&state).await.unwrap();
            assert!(
                state
                    .kv_store
                    .kv_get("preview_agents", &fresh_agent)
                    .await
                    .unwrap()
                    .is_none(),
                "orphaned preview agent record must be reclaimed"
            );
        })
        .await;
    }

    /// Direct unit regression for the dead-guard failure class (task-206
    /// review round 9ee3491d): Gyre-minted agent JWTs serialize `scope` as a
    /// NON-EMPTY JSON ARRAY (`AgentJwtClaims.scope: Vec<String>`, populated
    /// by `mint_with_workload` per §1 Token Scoping), so a containment guard
    /// keyed on `claims["scope"].as_str() == Some("agent")` matches nothing
    /// and every REST-surface preview-agent refusal silently stops firing
    /// (the integration tests below drive the router, this one pins the
    /// discriminator itself). The guard must recognize the claims a real
    /// minted preview token carries — rebuilt exactly as the auth extractor
    /// builds them (auth.rs per-agent token path:
    /// `serde_json::to_value(AgentJwtClaims)` → `jwt_claims`) — and must
    /// refuse every caller that is not the preview agent itself.
    #[tokio::test(flavor = "multi_thread")]
    async fn caller_is_preview_agent_detects_array_scoped_jwt() {
        let dir = tempfile::tempdir().unwrap();
        let (state, repo_id, _ws_id) = preview_state(dir.path(), 86_400, None).await;
        let app = crate::api::api_router().with_state(state.clone());

        let env_out = dir.path().join("agent-env.txt");
        let script = env_dump_script(dir.path(), &env_out);

        with_agent_command(script.to_str().unwrap(), || async {
            // Real preview run → real minted agent JWT (the production artifact).
            let resp =
                post_preview(&app, preview_body(&repo_id, &["specs/system/search.md"])).await;
            assert_eq!(resp.status(), StatusCode::ACCEPTED);
            let json = body_json(resp).await;
            let agent_id = json["agents"][0]["agent_id"].as_str().unwrap().to_string();
            let preview_id = json["preview_id"].as_str().unwrap().to_string();

            let token = state
                .kv_store
                .kv_get("agent_tokens", &agent_id)
                .await
                .unwrap()
                .expect("preview agent token must be live");
            let claims = state
                .agent_signing_key
                .validate(&token, &state.base_url)
                .expect("preview agent token must be a valid JWT");
            assert!(
                !claims.scope.is_empty(),
                "minted preview JWT must carry the §1 fine-grained scope list"
            );
            let claims_value = serde_json::to_value(&claims).unwrap();
            assert!(
                claims_value["scope"].is_array(),
                "scope must serialize as a JSON array — the dead-guard failure class"
            );

            let mk_auth = |agent_id: String, jwt_claims: Option<serde_json::Value>| {
                crate::auth::AuthenticatedAgent {
                    auth_mechanism: crate::auth::AuthMechanism::AgentToken,
                    agent_id,
                    user_id: None,
                    roles: vec![gyre_domain::UserRole::Agent],
                    tenant_id: "default".to_string(),
                    workspace_id: None,
                    repo_id: None,
                    scope: Vec::new(),
                    jwt_claims,
                    bearer_token: None,
                    deprecated_token_auth: false,
                    impersonation: None,
                }
            };

            // The preview agent's own array-scoped JWT → the guard MUST fire.
            // (This is the exact case the dead `as_str()` comparison missed.)
            assert!(
                super::caller_is_preview_agent(
                    &state,
                    &mk_auth(agent_id.clone(), Some(claims_value.clone()))
                )
                .await,
                "array-scoped preview-agent JWT must be recognized"
            );

            // Same JWT form but a DIFFERENT agent id → refused: the
            // preview-agent registry lookup, not the JWT shape, scopes the
            // containment to preview runs.
            assert!(
                !super::caller_is_preview_agent(
                    &state,
                    &mk_auth(
                        "not-a-preview-agent".to_string(),
                        Some(claims_value.clone())
                    )
                )
                .await,
                "array-scoped JWT of a NON-preview agent must not match"
            );

            // Global token / API key / legacy UUID token: no JWT claims.
            assert!(
                !super::caller_is_preview_agent(&state, &mk_auth(agent_id.clone(), None)).await,
                "claim-less caller must not match"
            );

            // Keycloak-style string scope (space-separated) must not match,
            // even for the preview agent id.
            let kc = mk_auth(
                agent_id.clone(),
                Some(serde_json::json!({ "scope": "openid profile" })),
            );
            assert!(
                !super::caller_is_preview_agent(&state, &kc).await,
                "space-separated string scope is not the agent-JWT discriminator"
            );

            // Empty scope array is not an agent-JWT signal.
            let empty_arr = mk_auth(agent_id.clone(), Some(serde_json::json!({ "scope": [] })));
            assert!(
                !super::caller_is_preview_agent(&state, &empty_arr).await,
                "empty scope array must not match"
            );

            // Legacy pre-§1 string form `scope: "agent"` is still honored.
            let legacy = mk_auth(
                agent_id.clone(),
                Some(serde_json::json!({ "scope": "agent" })),
            );
            assert!(
                super::caller_is_preview_agent(&state, &legacy).await,
                "legacy string scope \"agent\" for a preview agent must be honored"
            );

            let del = delete_preview(&app, &preview_id).await;
            assert_eq!(del.status(), StatusCode::NO_CONTENT);
        })
        .await;
    }

    /// Task-206 review F6: `is_preview_agent` must fail CLOSED on a store
    /// error. Every caller of it (and of `caller_is_preview_agent`) is a
    /// refusal gate, so during a kv outage the REST/MCP preview-agent
    /// refusals must keep firing — the pre-fix `matches!(Ok(Some(_)))`
    /// returned false on `Err` and silently stopped ALL preview containment
    /// for as long as the store was down.
    #[tokio::test(flavor = "multi_thread")]
    async fn preview_agent_guard_fails_closed_on_store_outage() {
        let dir = tempfile::tempdir().unwrap();
        let (state, _repo_id, _ws_id) = preview_state(dir.path(), 86_400, None).await;

        let mut failing = (*state).clone();
        failing.kv_store =
            std::sync::Arc::new(GetFailingKv::new(state.kv_store.clone(), "preview_agents"));
        let failing = std::sync::Arc::new(failing);

        let mk_auth = |agent_id: &str, jwt_claims: Option<serde_json::Value>| {
            crate::auth::AuthenticatedAgent {
                auth_mechanism: crate::auth::AuthMechanism::AgentToken,
                agent_id: agent_id.to_string(),
                user_id: None,
                roles: vec![gyre_domain::UserRole::Agent],
                tenant_id: "default".to_string(),
                workspace_id: None,
                repo_id: None,
                scope: Vec::new(),
                jwt_claims,
                bearer_token: None,
                deprecated_token_auth: false,
                impersonation: None,
            }
        };

        // Array-scoped agent JWT during an outage: the guard must fire —
        // the caller is treated as a preview agent and refused.
        assert!(
            super::caller_is_preview_agent(
                &failing,
                &mk_auth(
                    "any-agent",
                    Some(serde_json::json!({ "scope": ["repo:r:write"] }))
                )
            )
            .await,
            "kv outage must fail closed: the preview-agent refusal keeps firing"
        );

        // Legacy string scope form: same fail-closed behavior.
        assert!(
            super::caller_is_preview_agent(
                &failing,
                &mk_auth("any-agent", Some(serde_json::json!({ "scope": "agent" })))
            )
            .await,
            "legacy string scope must fail closed too"
        );

        // Claim-less principals (global token, API key) never reach the
        // store lookup: the outage must not widen the guard to them.
        assert!(
            !super::caller_is_preview_agent(&failing, &mk_auth("any-agent", None)).await,
            "claim-less caller must not match even during an outage"
        );
    }

    /// A preview agent must not be able to mint ceremony through the REST
    /// surface with its own live JWT (review finding: the §5 skip-ceremony
    /// invariant was enforced only in the MCP dispatch). POST
    /// /agents/{id}/complete and POST /merge-requests must both refuse the
    /// preview agent — no MR, no Idle, no provenance, no snapshot refs.
    /// Exercises the production route chain (ABAC middleware + seeded
    /// builtin policies) exactly as a live server would evaluate it.
    #[tokio::test(flavor = "multi_thread")]
    async fn preview_agent_rest_ceremony_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let (app, state, repo_id) = preview_app(dir.path(), 86_400, None).await;

        let env_out = dir.path().join("agent-env.txt");
        let script = env_dump_script(dir.path(), &env_out);

        with_agent_command(script.to_str().unwrap(), || async {
            let resp =
                post_preview(&app, preview_body(&repo_id, &["specs/system/search.md"])).await;
            assert_eq!(resp.status(), StatusCode::ACCEPTED);
            let json = body_json(resp).await;
            let preview_id = json["preview_id"].as_str().unwrap().to_string();
            let agent_id = json["agents"][0]["agent_id"].as_str().unwrap().to_string();
            let branch = json["agents"][0]["branch"].as_str().unwrap().to_string();

            // The preview agent's own live JWT, driven through the production
            // router (auth + ABAC middleware), mirroring the review probe.
            crate::abac_middleware::seed_builtin_policies(&state)
                .await
                .expect("seed built-in policies");
            let prod = crate::build_router(state.clone());
            let jwt = state
                .kv_store
                .kv_get("agent_tokens", &agent_id)
                .await
                .unwrap()
                .expect("preview agent token must be live");

            // POST /api/v1/agents/{id}/complete → 403, not 201-with-MR.
            let resp = prod
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri(format!("/api/v1/agents/{agent_id}/complete"))
                        .header("authorization", format!("Bearer {jwt}"))
                        .header("content-type", "application/json")
                        .body(Body::from(
                            serde_json::json!({
                                "branch": branch,
                                "title": "ceremony escape",
                                "target_branch": "main",
                                "conversation_sha": "0".repeat(64),
                            })
                            .to_string(),
                        ))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                resp.status(),
                StatusCode::FORBIDDEN,
                "preview agent must not complete via REST"
            );

            // POST /api/v1/merge-requests → 403, not 201-with-MR.
            let resp = prod
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/v1/merge-requests")
                        .header("authorization", format!("Bearer {jwt}"))
                        .header("content-type", "application/json")
                        .body(Body::from(
                            serde_json::json!({
                                "repository_id": repo_id,
                                "title": "ceremony escape",
                                "source_branch": branch,
                                "target_branch": "main",
                            })
                            .to_string(),
                        ))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                resp.status(),
                StatusCode::FORBIDDEN,
                "preview agent must not create MRs via REST"
            );

            // No ceremony side effects leaked through either refusal.
            assert!(
                state.merge_requests.list().await.unwrap().is_empty(),
                "no MR may exist after the refused calls"
            );
            let agent = state
                .agents
                .find_by_id(&gyre_common::Id::new(&agent_id))
                .await
                .unwrap()
                .unwrap();
            assert_ne!(agent.status, gyre_domain::AgentStatus::Idle);
            assert_eq!(
                agent.status,
                gyre_domain::AgentStatus::Active,
                "the refused complete call must not change the preview agent's state"
            );
            assert!(
                state
                    .kv_store
                    .kv_list("agent_provenance")
                    .await
                    .unwrap()
                    .is_empty(),
                "no provenance may be recorded for a preview agent"
            );
            let repo = state
                .repos
                .find_by_id(&gyre_common::Id::new(&repo_id))
                .await
                .unwrap()
                .unwrap();
            assert_eq!(
                crate::git_refs::count_refs_under(&repo.path, "refs/agents/").await,
                0,
                "no snapshot refs may be written for a preview agent"
            );

            // Positive control: the same MR request from an Admin caller is
            // accepted — the gate scopes to preview agents, not the route.
            let resp = prod
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/v1/merge-requests")
                        .header("authorization", "Bearer test-token")
                        .header("content-type", "application/json")
                        .body(Body::from(
                            serde_json::json!({
                                "repository_id": repo_id,
                                "title": "admin-created on preview branch",
                                "source_branch": branch,
                                "target_branch": "main",
                            })
                            .to_string(),
                        ))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                resp.status(),
                StatusCode::CREATED,
                "admin MR creation on a preview branch must still work"
            );
            let admin_mr = body_json(resp).await;
            let mr_id = admin_mr["id"].as_str().unwrap().to_string();

            // POST /api/v1/merge-requests/{id}/reviews → 403 (task-206
            // review F1): a preview agent's review would be a real Review
            // row counting toward real merge decisions — the same reason
            // approve_spec refuses preview-agent callers.
            let resp = prod
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri(format!("/api/v1/merge-requests/{mr_id}/reviews"))
                        .header("authorization", format!("Bearer {jwt}"))
                        .header("content-type", "application/json")
                        .body(Body::from(
                            serde_json::json!({
                                "reviewer_agent_id": agent_id,
                                "decision": "approve",
                                "body": "ceremony escape",
                            })
                            .to_string(),
                        ))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                resp.status(),
                StatusCode::FORBIDDEN,
                "preview agent must not submit MR reviews via REST"
            );
            let json = body_json(resp).await;
            assert!(
                json["error"]
                    .as_str()
                    .unwrap_or_default()
                    .contains("preview agents may not submit merge request reviews"),
                "the 403 must come from the preview guard, not middleware: {json}"
            );
            assert!(
                state
                    .reviews
                    .list_reviews(&gyre_common::Id::new(&mr_id))
                    .await
                    .unwrap()
                    .is_empty(),
                "no review may be recorded for the refused preview-agent call"
            );

            // POST /api/v1/workspaces/{ws}/messages → 403 (task-206 review
            // F2): a Directed-tier message from a preview agent lands in a
            // real agent's inbox and stays until acked — a durable
            // footprint and influence channel out of scratch work. The
            // message body is one the route would otherwise accept (the
            // preview agent is a workspace member), so the 403 can only
            // come from the preview guard, not incidental validation.
            let repo = state
                .repos
                .find_by_id(&gyre_common::Id::new(&repo_id))
                .await
                .unwrap()
                .unwrap();
            let ws_id = repo.workspace_id.to_string();
            let msg_body = serde_json::json!({
                "to": {"agent": agent_id},
                "kind": "status_update",
                "payload": {"status": "in_progress", "summary": "ceremony escape"},
            });
            let resp = prod
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri(format!("/api/v1/workspaces/{ws_id}/messages"))
                        .header("authorization", format!("Bearer {jwt}"))
                        .header("content-type", "application/json")
                        .body(Body::from(msg_body.to_string()))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                resp.status(),
                StatusCode::FORBIDDEN,
                "preview agent must not inject messages via REST"
            );
            let json = body_json(resp).await;
            assert!(
                json["error"]
                    .as_str()
                    .unwrap_or_default()
                    .contains("preview agents may not send messages"),
                "the 403 must come from the preview guard, not middleware: {json}"
            );

            // Positive control: the identical message from an Admin caller
            // is accepted — the guard scopes to preview agents, not the
            // route (and proves the 403 above was not validation).
            let resp = prod
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri(format!("/api/v1/workspaces/{ws_id}/messages"))
                        .header("authorization", "Bearer test-token")
                        .header("content-type", "application/json")
                        .body(Body::from(msg_body.to_string()))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                resp.status(),
                StatusCode::CREATED,
                "admin message send must still work (guard scopes to preview agents)"
            );

            let del = delete_preview(&app, &preview_id).await;
            assert_eq!(del.status(), StatusCode::NO_CONTENT);
        })
        .await;
    }

    /// Task-206 review repair (eb6937c4 F1): a preview agent's launch path
    /// issues the session SVID, so the runner's 15 s self-heartbeats —
    /// which present exactly `GYRE_SVID_PEM` + `GYRE_STACK_HASH` — verify
    /// cryptographically against the real trust bundle and keep a >60 s
    /// run Active. Before the fix the launch never issued an SVID, every
    /// heartbeat failed closed at `attest_session` ("session was never
    /// attested"), and the stale detector's default Pause arm flipped a
    /// still-running preview agent to Stopped ~60 s into every real run
    /// (spurious agent.failed, status reporting "complete" while agents
    /// were still working, the UI stopping its poll and never fetching the
    /// diff). This test drives the full causal chain: real launch, real
    /// env capture, real heartbeat through the production router, real
    /// stale-detector pass.
    #[tokio::test(flavor = "multi_thread")]
    async fn preview_agent_heartbeats_keep_long_runs_active() {
        let dir = tempfile::tempdir().unwrap();
        let (app, state, repo_id) = preview_app(dir.path(), 86_400, None).await;

        // The runner presents GYRE_SVID_PEM / GYRE_STACK_HASH verbatim from
        // its environment (agent-runner.mjs sendHeartbeat). A PEM contains
        // newlines, so capture the exact values to files — not a line-parsed
        // `env` dump — and verify the heartbeat with precisely what the
        // process received.
        let svid_out = dir.path().join("agent-svid.pem");
        let stack_out = dir.path().join("agent-stack.txt");
        let script = dir.path().join("agent-entrypoint.sh");
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\nprintf '%s' \"$GYRE_SVID_PEM\" > {svid}\nprintf '%s' \"$GYRE_STACK_HASH\" > {stack}\nexec sleep 300\n",
                svid = svid_out.display(),
                stack = stack_out.display(),
            ),
        )
        .unwrap();
        make_executable(&script);

        with_agent_command(script.to_str().unwrap(), || async {
            let resp =
                post_preview(&app, preview_body(&repo_id, &["specs/system/search.md"])).await;
            assert_eq!(resp.status(), StatusCode::ACCEPTED);
            let json = body_json(resp).await;
            let preview_id = json["preview_id"].as_str().unwrap().to_string();
            let agent_id = json["agents"][0]["agent_id"].as_str().unwrap().to_string();

            // The launched process received the session SVID and the stack
            // fingerprint bound at issuance (this fails on the old behavior:
            // no SVID was ever injected, so the file stays empty).
            let svid_pem = soon(|| {
                std::fs::read_to_string(&svid_out)
                    .ok()
                    .filter(|s| !s.is_empty())
            })
            .await;
            assert!(
                svid_pem.contains("BEGIN CERTIFICATE"),
                "GYRE_SVID_PEM must carry the issued chain; got: {svid_pem}"
            );
            let stack = soon(|| {
                std::fs::read_to_string(&stack_out)
                    .ok()
                    .filter(|s| !s.is_empty())
            })
            .await;
            assert!(
                stack.starts_with("sha256:"),
                "GYRE_STACK_HASH must carry the issuance-bound fingerprint; got: {stack}"
            );
            // The SVID record the heartbeat verifies against exists and is
            // keyed to this agent (the record `attest_session` loads).
            let record = state
                .kv_store
                .kv_get("spiffe_svids", &agent_id)
                .await
                .unwrap()
                .expect("launch must persist the session SVID record");
            assert!(
                record.contains(stack.as_str()),
                "record must bind the stack fingerprint"
            );

            // The runner's heartbeat — exactly what agent-runner.mjs sends
            // every 15 s — driven through the production router (auth +
            // ABAC middleware) with the preview agent's live JWT. Real
            // crypto: chain validation against the published trust bundle,
            // serial match, stack binding. On the old behavior this was a
            // 403 "session was never attested" for every preview run.
            crate::abac_middleware::seed_builtin_policies(&state)
                .await
                .expect("seed built-in policies");
            let prod = crate::build_router(state.clone());
            let jwt = state
                .kv_store
                .kv_get("agent_tokens", &agent_id)
                .await
                .unwrap()
                .expect("preview agent token must be live");
            let resp = prod
                .clone()
                .oneshot(
                    Request::builder()
                        .method("PUT")
                        .uri(format!("/api/v1/agents/{agent_id}/heartbeat"))
                        .header("authorization", format!("Bearer {jwt}"))
                        .header("content-type", "application/json")
                        .body(Body::from(
                            serde_json::json!({
                                "svid": svid_pem,
                                "stack_fingerprint": stack,
                            })
                            .to_string(),
                        ))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                resp.status(),
                StatusCode::NO_CONTENT,
                "the runner's self-heartbeat must verify for a preview agent"
            );

            // The heartbeat advanced the agent's liveness.
            let row = state
                .agents
                .find_by_id(&gyre_common::Id::new(&agent_id))
                .await
                .unwrap()
                .unwrap();
            assert!(
                row.last_heartbeat.is_some(),
                "a verified heartbeat must record last_heartbeat"
            );

            // A >60 s run is exactly this state: spawn older than the
            // detector's window, liveness kept fresh by heartbeats. Age the
            // spawn past the window and run the real detector — the
            // still-running preview agent must stay Active (on the old
            // behavior the missed heartbeats flipped it to Stopped with a
            // spurious agent.failed event).
            let mut aged = state
                .agents
                .find_by_id(&gyre_common::Id::new(&agent_id))
                .await
                .unwrap()
                .unwrap();
            aged.spawned_at = aged.spawned_at.saturating_sub(10_000);
            state.agents.update(&aged).await.unwrap();
            crate::stale_agents::run_once(&state).await.unwrap();
            let row = state
                .agents
                .find_by_id(&gyre_common::Id::new(&agent_id))
                .await
                .unwrap()
                .unwrap();
            assert_eq!(
                row.status,
                gyre_domain::AgentStatus::Active,
                "a heartbeating preview agent past the 60 s spawn window must stay Active"
            );

            // And the status surface reports the run as still running —
            // the signal the UI polls for diff updates (§5 step 4).
            let resp = get_preview_status(&app, &preview_id).await;
            assert_eq!(resp.status(), StatusCode::OK);
            let status = body_json(resp).await;
            assert_eq!(status["state"], "running", "status: {status}");
            assert_eq!(
                status["agents"][0]["status"], "running",
                "per-agent status must reflect the Active row: {status}"
            );

            // Cross-check the negative: a heartbeat presenting a tampered
            // SVID is refused — the 204 above was real verification, not a
            // bypass.
            let resp = prod
                .clone()
                .oneshot(
                    Request::builder()
                        .method("PUT")
                        .uri(format!("/api/v1/agents/{agent_id}/heartbeat"))
                        .header("authorization", format!("Bearer {jwt}"))
                        .header("content-type", "application/json")
                        .body(Body::from(
                            serde_json::json!({
                                "svid": "not-a-valid-svid",
                                "stack_fingerprint": stack,
                            })
                            .to_string(),
                        ))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                resp.status(),
                StatusCode::FORBIDDEN,
                "a tampered SVID presentation must be refused"
            );

            let del = delete_preview(&app, &preview_id).await;
            assert_eq!(del.status(), StatusCode::NO_CONTENT);
        })
        .await;
    }
    /// merge-queue routes too. A preview agent's live JWT authenticates as
    /// UserRole::Agent and the builtin agent-scoped-access policy grants
    /// 'write' on '*' — without per-handler guards it can create/update task
    /// rows and enqueue MRs, the same escape class closed for complete/MR.
    #[tokio::test(flavor = "multi_thread")]
    async fn preview_agent_rest_task_ceremony_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let (app, state, repo_id) = preview_app(dir.path(), 86_400, None).await;

        let env_out = dir.path().join("agent-env.txt");
        let script = env_dump_script(dir.path(), &env_out);

        with_agent_command(script.to_str().unwrap(), || async {
            let resp =
                post_preview(&app, preview_body(&repo_id, &["specs/system/search.md"])).await;
            assert_eq!(resp.status(), StatusCode::ACCEPTED);
            let json = body_json(resp).await;
            let preview_id = json["preview_id"].as_str().unwrap().to_string();
            let agent_id = json["agents"][0]["agent_id"].as_str().unwrap().to_string();

            crate::abac_middleware::seed_builtin_policies(&state)
                .await
                .expect("seed built-in policies");
            let prod = crate::build_router(state.clone());
            let jwt = state
                .kv_store
                .kv_get("agent_tokens", &agent_id)
                .await
                .unwrap()
                .expect("preview agent token must be live");

            // POST /api/v1/tasks → 403, not 201.
            let resp = prod
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/v1/tasks")
                        .header("authorization", format!("Bearer {jwt}"))
                        .header("content-type", "application/json")
                        .body(Body::from(
                            serde_json::json!({
                                "title": "preview task-ledger escape",
                            })
                            .to_string(),
                        ))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                resp.status(),
                StatusCode::FORBIDDEN,
                "preview agent must not create tasks via REST"
            );

            // PUT /api/v1/tasks/{id} → 403. An admin-created task provides the
            // row the preview agent attempts to mutate (the guard fires before
            // any lookup, so any id works, but a real row keeps the probe
            // honest about the route being reachable).
            let admin_task = serde_json::json!({"title": "admin-owned task"});
            let resp = prod
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/v1/tasks")
                        .header("authorization", "Bearer test-token")
                        .header("content-type", "application/json")
                        .body(Body::from(admin_task.to_string()))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                resp.status(),
                StatusCode::CREATED,
                "admin positive control: task creation must still work"
            );
            let created = body_json(resp).await;
            let task_id = created["id"].as_str().unwrap().to_string();

            let resp = prod
                .clone()
                .oneshot(
                    Request::builder()
                        .method("PUT")
                        .uri(format!("/api/v1/tasks/{task_id}"))
                        .header("authorization", format!("Bearer {jwt}"))
                        .header("content-type", "application/json")
                        .body(Body::from(
                            serde_json::json!({
                                "title": "mutated by preview agent",
                            })
                            .to_string(),
                        ))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                resp.status(),
                StatusCode::FORBIDDEN,
                "preview agent must not update tasks via REST"
            );
            let task = state
                .tasks
                .find_by_id(&gyre_common::Id::new(&task_id))
                .await
                .unwrap()
                .unwrap();
            assert_eq!(
                task.title, "admin-owned task",
                "the refused update must not have mutated the task"
            );

            // POST /api/v1/merge-queue → 403. Needs an existing MR for the
            // route to reach past its 404 lookup; an admin-created MR on the
            // preview branch supplies it.
            let branch = json["agents"][0]["branch"].as_str().unwrap().to_string();
            let repo = state
                .repos
                .find_by_id(&gyre_common::Id::new(&repo_id))
                .await
                .unwrap()
                .unwrap();
            let mr = gyre_domain::MergeRequest::new(
                gyre_common::Id::new(uuid::Uuid::new_v4().to_string()),
                gyre_common::Id::new(&repo_id),
                repo.workspace_id.clone(),
                "enqueue probe".to_string(),
                branch,
                "main".to_string(),
                chrono_now_secs(),
            );
            state.merge_requests.create(&mr).await.unwrap();
            let resp = prod
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/v1/merge-queue/enqueue")
                        .header("authorization", format!("Bearer {jwt}"))
                        .header("content-type", "application/json")
                        .body(Body::from(
                            serde_json::json!({
                                "merge_request_id": mr.id.to_string(),
                            })
                            .to_string(),
                        ))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                resp.status(),
                StatusCode::FORBIDDEN,
                "preview agent must not enqueue MRs via REST"
            );
            assert!(
                state.merge_queue.list_queue().await.unwrap().is_empty(),
                "no queue entry may exist after the refused enqueue"
            );

            // Only the admin's positive-control task exists — no task rows
            // leaked through the refused calls.
            let tasks = state.tasks.list().await.unwrap();
            assert_eq!(
                tasks.len(),
                1,
                "exactly the admin control task may exist after the refusals"
            );

            let del = delete_preview(&app, &preview_id).await;
            assert_eq!(del.status(), StatusCode::NO_CONTENT);
        })
        .await;
    }

    /// Review-round F1 regression: the §5 skip-ceremony must also hold on the
    /// status-transition and cancel routes. A preview agent's live JWT passes
    /// ABAC (Agent role, builtin agent-scoped-access allows `write` on '*'),
    /// so without per-handler guards it could transition task status — the
    /// dangerous one: Done/Cancelled on a reconciliation-labeled task fires
    /// `on_reconciliation_task_terminal` and advances a meta-spec rollout —
    /// flip agent status (itself back to Idle, defeating the killed-on-
    /// completion semantics), transition MR status, and cancel merge-queue
    /// entries: durable production effects from throwaway preview scratch.
    #[tokio::test(flavor = "multi_thread")]
    async fn preview_agent_rest_status_routes_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let (app, state, repo_id) = preview_app(dir.path(), 86_400, None).await;

        let env_out = dir.path().join("agent-env.txt");
        let script = env_dump_script(dir.path(), &env_out);

        with_agent_command(script.to_str().unwrap(), || async {
            let resp =
                post_preview(&app, preview_body(&repo_id, &["specs/system/search.md"])).await;
            assert_eq!(resp.status(), StatusCode::ACCEPTED);
            let json = body_json(resp).await;
            let preview_id = json["preview_id"].as_str().unwrap().to_string();
            let agent_id = json["agents"][0]["agent_id"].as_str().unwrap().to_string();

            crate::abac_middleware::seed_builtin_policies(&state)
                .await
                .expect("seed built-in policies");
            let prod = crate::build_router(state.clone());
            let jwt = state
                .kv_store
                .kv_get("agent_tokens", &agent_id)
                .await
                .unwrap()
                .expect("preview agent token must be live");

            // Admin-owned task the preview agent attempts to transition.
            let resp = prod
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/v1/tasks")
                        .header("authorization", "Bearer test-token")
                        .header("content-type", "application/json")
                        .body(Body::from(
                            serde_json::json!({
                                "title": "admin-owned transition target",
                                "labels": [crate::reconciliation::RECONCILIATION_LABEL],
                            })
                            .to_string(),
                        ))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::CREATED);
            let created = body_json(resp).await;
            let task_id = created["id"].as_str().unwrap().to_string();

            // PUT /api/v1/tasks/{id}/status → 403. The task carries the
            // reconciliation label, so a `done` transition would fire
            // on_reconciliation_task_terminal and advance a rollout —
            // exactly the durable footprint §5 forbids preview work to
            // leave.
            let resp = prod
                .clone()
                .oneshot(
                    Request::builder()
                        .method("PUT")
                        .uri(format!("/api/v1/tasks/{task_id}/status"))
                        .header("authorization", format!("Bearer {jwt}"))
                        .header("content-type", "application/json")
                        .body(Body::from(
                            serde_json::json!({"status": "in_progress"}).to_string(),
                        ))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                resp.status(),
                StatusCode::FORBIDDEN,
                "preview agent must not transition task status via REST"
            );
            let task = state
                .tasks
                .find_by_id(&gyre_common::Id::new(&task_id))
                .await
                .unwrap()
                .unwrap();
            assert_eq!(
                task.status,
                gyre_domain::TaskStatus::Backlog,
                "the refused transition must not have mutated the task"
            );

            // Positive control: the same transition from an Admin caller is
            // accepted — the guard scopes to preview agents, not the route.
            let resp = prod
                .clone()
                .oneshot(
                    Request::builder()
                        .method("PUT")
                        .uri(format!("/api/v1/tasks/{task_id}/status"))
                        .header("authorization", "Bearer test-token")
                        .header("content-type", "application/json")
                        .body(Body::from(
                            serde_json::json!({"status": "in_progress"}).to_string(),
                        ))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                resp.status(),
                StatusCode::OK,
                "admin task-status transition must still work"
            );

            // PUT /api/v1/agents/{id}/status → 403. The preview agent tries
            // to reset itself to Idle — the exact state finish_preview_agent
            // exists to prevent.
            let resp = prod
                .clone()
                .oneshot(
                    Request::builder()
                        .method("PUT")
                        .uri(format!("/api/v1/agents/{agent_id}/status"))
                        .header("authorization", format!("Bearer {jwt}"))
                        .header("content-type", "application/json")
                        .body(Body::from(
                            serde_json::json!({"status": "idle"}).to_string(),
                        ))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                resp.status(),
                StatusCode::FORBIDDEN,
                "preview agent must not change agent status via REST"
            );
            let agent = state
                .agents
                .find_by_id(&gyre_common::Id::new(&agent_id))
                .await
                .unwrap()
                .unwrap();
            assert_eq!(
                agent.status,
                gyre_domain::AgentStatus::Active,
                "the refused self-transition must not have idled the preview agent"
            );

            // Positive control: an admin-created agent can still be
            // transitioned by its operator.
            let resp = prod
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/v1/agents")
                        .header("authorization", "Bearer test-token")
                        .header("content-type", "application/json")
                        .body(Body::from(
                            serde_json::json!({"name": "status-route-control"}).to_string(),
                        ))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::CREATED);
            let created = body_json(resp).await;
            let control_agent_id = created["id"].as_str().unwrap().to_string();
            let resp = prod
                .clone()
                .oneshot(
                    Request::builder()
                        .method("PUT")
                        .uri(format!("/api/v1/agents/{control_agent_id}/status"))
                        .header("authorization", "Bearer test-token")
                        .header("content-type", "application/json")
                        .body(Body::from(
                            serde_json::json!({"status": "active"}).to_string(),
                        ))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                resp.status(),
                StatusCode::OK,
                "admin agent-status transition must still work"
            );

            // PUT /api/v1/merge-requests/{id}/status → 403 (not `approved` —
            // which would self-satisfy the supervised human-review escape —
            // and not `merged`, which would bypass the merge queue).
            let branch = json["agents"][0]["branch"].as_str().unwrap().to_string();
            let repo = state
                .repos
                .find_by_id(&gyre_common::Id::new(&repo_id))
                .await
                .unwrap()
                .unwrap();
            let mr = gyre_domain::MergeRequest::new(
                gyre_common::Id::new(uuid::Uuid::new_v4().to_string()),
                gyre_common::Id::new(&repo_id),
                repo.workspace_id.clone(),
                "status-route probe".to_string(),
                branch,
                "main".to_string(),
                chrono_now_secs(),
            );
            state.merge_requests.create(&mr).await.unwrap();
            let resp = prod
                .clone()
                .oneshot(
                    Request::builder()
                        .method("PUT")
                        .uri(format!("/api/v1/merge-requests/{}/status", mr.id))
                        .header("authorization", format!("Bearer {jwt}"))
                        .header("content-type", "application/json")
                        .body(Body::from(
                            serde_json::json!({"status": "closed"}).to_string(),
                        ))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                resp.status(),
                StatusCode::FORBIDDEN,
                "preview agent must not transition merge request status via REST"
            );
            let mr_now = state
                .merge_requests
                .find_by_id(&mr.id)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(
                mr_now.status,
                gyre_domain::MrStatus::Open,
                "the refused transition must not have mutated the MR"
            );

            // Positive control: an Admin caller transitions the same MR.
            let resp = prod
                .clone()
                .oneshot(
                    Request::builder()
                        .method("PUT")
                        .uri(format!("/api/v1/merge-requests/{}/status", mr.id))
                        .header("authorization", "Bearer test-token")
                        .header("content-type", "application/json")
                        .body(Body::from(
                            serde_json::json!({"status": "approved"}).to_string(),
                        ))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                resp.status(),
                StatusCode::OK,
                "admin MR-status transition must still work"
            );

            // DELETE /api/v1/merge-queue/{id} → 403. A queued entry (the
            // admin's own) must not be cancellable by preview scratch work.
            let entry = gyre_domain::MergeQueueEntry::new(
                gyre_common::Id::new(uuid::Uuid::new_v4().to_string()),
                mr.id.clone(),
                50,
                chrono_now_secs(),
            );
            state.merge_queue.enqueue(&entry).await.unwrap();
            let entry_id = entry.id.to_string();
            let resp = prod
                .clone()
                .oneshot(
                    Request::builder()
                        .method("DELETE")
                        .uri(format!("/api/v1/merge-queue/{entry_id}"))
                        .header("authorization", format!("Bearer {jwt}"))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                resp.status(),
                StatusCode::FORBIDDEN,
                "preview agent must not cancel merge-queue entries via REST"
            );
            let entry_now = state
                .merge_queue
                .find_by_id(&gyre_common::Id::new(&entry_id))
                .await
                .unwrap()
                .expect("the refused cancel must leave the queue entry in place");
            assert!(
                !entry_now.is_terminal(),
                "the refused cancel must not have terminated the entry"
            );

            // Positive control: the operator can still cancel the entry.
            let resp = prod
                .clone()
                .oneshot(
                    Request::builder()
                        .method("DELETE")
                        .uri(format!("/api/v1/merge-queue/{entry_id}"))
                        .header("authorization", "Bearer test-token")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                resp.status(),
                StatusCode::NO_CONTENT,
                "admin merge-queue cancel must still work"
            );

            let del = delete_preview(&app, &preview_id).await;
            assert_eq!(del.status(), StatusCode::NO_CONTENT);
        })
        .await;
    }

    /// Review-round F1 regression (repair 898642fe): the §5 skip-ceremony
    /// must also hold on the remaining control surfaces a live preview-agent
    /// JWT can reach. The preview agent's token is a valid Agent-role
    /// credential: it passes require_auth everywhere, the registry-exempt
    /// stop/fail routes run with NO policy evaluation at all, and the
    /// builtin agent-scoped-access Allow (write on '*') grants the
    /// non-repo-addressed compose routes. Without per-handler guards it
    /// could stop or fail ANY agent in ANY tenant (revoking tokens and task
    /// secrets, decrementing budgets, firing orchestrator death handling
    /// with replacement spawns), forge usage reports against other agents
    /// (triggering their budget enforcement), emit production
    /// SpecApprovalEvents, create task/agent rows via compose (bypassing
    /// the guarded POST /api/v1/tasks), tear down other runs' compose
    /// sessions, and mint a workspace orchestrator. Every probe drives the
    /// live JWT through the production router, asserts the ledger did not
    /// mutate, and carries an authorized positive control so each 403 is
    /// the guard, not a broken route.
    #[tokio::test(flavor = "multi_thread")]
    async fn preview_agent_control_routes_are_refused() {
        use crate::auth::test_helpers::{make_jwt_config, sign_test_jwt};

        let dir = tempfile::tempdir().unwrap();
        let (state, repo_id, ws_id) = preview_state(dir.path(), 86_400, None).await;
        let app = crate::api::api_router().with_state(state.clone());

        // A JWT-enabled twin of the same state (Arc-shared port stores), so
        // minted JWTs — a foreign-tenant Admin and an in-tenant Agent-role
        // caller — authenticate against the production router too.
        let mut jwt_state = (*state).clone();
        jwt_state.jwt_config = Some(make_jwt_config());
        let jwt_state = std::sync::Arc::new(jwt_state);

        // The orchestrator-spawn positive control needs the fail-closed
        // persona layers (platform-model §4.3): the built-in meta-spec set
        // and the pre-approved built-in personas, exactly like startup
        // seeding.
        crate::seed_builtin_meta_specs(&jwt_state).await;
        crate::seed_builtin_personas_for_tenant(&jwt_state, &gyre_common::Id::new("default")).await;

        // A spec ledger entry resolving to the preview's target repo, so
        // the spec/approve ABAC resource resolution points at the repo the
        // preview agent is scoped to (the finding's exact shape).
        let spec_sha = "b".repeat(40);
        jwt_state
            .spec_ledger
            .save(&gyre_domain::SpecLedgerEntry {
                path: "system/search.md".to_string(),
                title: "Search".to_string(),
                owner: "user:admin".to_string(),
                kind: None,
                current_sha: spec_sha.clone(),
                approval_mode: "human_only".to_string(),
                approval_status: gyre_domain::ApprovalStatus::Pending,
                linked_tasks: vec![],
                linked_mrs: vec![],
                drift_status: "unknown".to_string(),
                created_at: 1_700_000_000,
                updated_at: 1_700_000_000,
                repo_id: Some(repo_id.clone()),
                workspace_id: Some(ws_id.to_string()),
            })
            .await
            .unwrap();

        // Active victims in the preview workspace (a real workspace row in
        // tenant "default"), so tenant containment passes for in-tenant
        // callers and only the preview guard can refuse the preview agent.
        let now = chrono_now_secs();
        let mut victims = Vec::new();
        for name in ["stop-victim", "fail-victim", "containment-victim"] {
            let mut agent = gyre_domain::Agent::new(
                gyre_common::Id::new(uuid::Uuid::new_v4().to_string()),
                name,
                now,
            );
            agent.workspace_id = ws_id.clone();
            agent
                .transition_status(gyre_domain::AgentStatus::Active)
                .unwrap();
            state.agents.create(&agent).await.unwrap();
            victims.push(agent.id.to_string());
        }

        let env_out = dir.path().join("agent-env.txt");
        let script = env_dump_script(dir.path(), &env_out);

        with_agent_command(script.to_str().unwrap(), || async {
            let resp =
                post_preview(&app, preview_body(&repo_id, &["specs/system/search.md"])).await;
            assert_eq!(resp.status(), StatusCode::ACCEPTED);
            let json = body_json(resp).await;
            let preview_id = json["preview_id"].as_str().unwrap().to_string();
            let agent_id = json["agents"][0]["agent_id"].as_str().unwrap().to_string();

            crate::abac_middleware::seed_builtin_policies(&jwt_state)
                .await
                .expect("seed built-in policies");
            let prod = crate::build_router(jwt_state.clone());
            let jwt = state
                .kv_store
                .kv_get("agent_tokens", &agent_id)
                .await
                .unwrap()
                .expect("preview agent token must be live");

            // Minted probe credentials: an Admin in a foreign tenant (for
            // the stop route's cross-tenant containment) and an in-tenant
            // Agent-role caller that is NOT a preview agent (for the
            // orchestrator-spawn role gate).
            let foreign_admin = sign_test_jwt(
                &serde_json::json!({
                    "tenant_id": "tenant-b",
                    "sub": "admin-tenant-b",
                    "preferred_username": "admin-tenant-b",
                    "realm_access": { "roles": ["admin"] }
                }),
                3600,
            );
            let plain_agent = sign_test_jwt(
                &serde_json::json!({
                    "tenant_id": "default",
                    "sub": "plain-worker",
                    "preferred_username": "plain-worker",
                    "realm_access": { "roles": ["agent"] }
                }),
                3600,
            );

            // ── 1. POST /api/v1/agents/{id}/stop + /fail (registry-exempt:
            //    no middleware policy evaluation, the handler guards are the
            //    only authorization surface) ───────────────────────────
            for (verb, victim) in [("stop", &victims[0]), ("fail", &victims[1])] {
                let uri = format!("/api/v1/agents/{}/{}", victim, verb);
                let resp = prod
                    .clone()
                    .oneshot(
                        Request::builder()
                            .method("POST")
                            .uri(&uri)
                            .header("authorization", format!("Bearer {jwt}"))
                            .body(Body::empty())
                            .unwrap(),
                    )
                    .await
                    .unwrap();
                assert_eq!(
                    resp.status(),
                    StatusCode::FORBIDDEN,
                    "preview agent must not {verb} agents via REST"
                );
                let row = state
                    .agents
                    .find_by_id(&gyre_common::Id::new(victim))
                    .await
                    .unwrap()
                    .unwrap();
                assert_eq!(
                    row.status,
                    gyre_domain::AgentStatus::Active,
                    "the refused {verb} must not have mutated the victim"
                );
            }

            // Cross-tenant containment: an Admin JWT from tenant B cannot
            // stop tenant-default's agent — the per-handler comparison is
            // the only guard on this exempt route.
            let resp = prod
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri(format!("/api/v1/agents/{}/stop", victims[2]))
                        .header("authorization", format!("Bearer {foreign_admin}"))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                resp.status(),
                StatusCode::FORBIDDEN,
                "a foreign-tenant caller must not stop an agent outside its tenant"
            );

            // Usage self-report contract: the preview agent may not forge a
            // usage report against another agent (it would charge the
            // victim's budget and trigger its ≥100% enforcement).
            let resp = prod
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri(format!("/api/v1/agents/{}/usage", victims[2]))
                        .header("authorization", format!("Bearer {jwt}"))
                        .header("content-type", "application/json")
                        .body(Body::from(
                            serde_json::json!({
                                "tokens_input": 1,
                                "tokens_output": 1,
                                "cost_usd": 0.0
                            })
                            .to_string(),
                        ))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                resp.status(),
                StatusCode::FORBIDDEN,
                "a preview agent must not report usage for another agent"
            );

            // Positive controls: the operator (global Admin token, same
            // tenant) stops/fails the victims and reports usage on an
            // agent's behalf — the guards scope to preview/cross-tenant
            // callers, not the routes.
            for (verb, victim) in [("stop", &victims[0]), ("fail", &victims[1])] {
                let resp = prod
                    .clone()
                    .oneshot(
                        Request::builder()
                            .method("POST")
                            .uri(format!("/api/v1/agents/{}/{}", victim, verb))
                            .header("authorization", "Bearer test-token")
                            .body(Body::empty())
                            .unwrap(),
                    )
                    .await
                    .unwrap();
                assert_eq!(
                    resp.status(),
                    StatusCode::OK,
                    "admin {verb} must still work"
                );
            }
            let resp = prod
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri(format!("/api/v1/agents/{}/usage", victims[2]))
                        .header("authorization", "Bearer test-token")
                        .header("content-type", "application/json")
                        .body(Body::from(
                            serde_json::json!({
                                "tokens_input": 1,
                                "tokens_output": 1,
                                "cost_usd": 0.0
                            })
                            .to_string(),
                        ))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                resp.status(),
                StatusCode::OK,
                "admin usage reporting on behalf of an agent must still work"
            );

            // ── 2. POST /api/v1/specs/{path}/approve — §5 skip-ceremony row
            //    1 ("Spec approval check — Skipped") ──────────────────
            // Driven on the bare api_router (no ABAC middleware) so the
            // handler guard itself is the only refusal layer, and on the
            // production router for the realistic path.
            for router in [app.clone(), prod.clone()] {
                let resp = router
                    .oneshot(
                        Request::builder()
                            .method("POST")
                            .uri("/api/v1/specs/system%2Fsearch.md/approve")
                            .header("authorization", format!("Bearer {jwt}"))
                            .header("content-type", "application/json")
                            .body(Body::from(
                                serde_json::json!({ "sha": spec_sha }).to_string(),
                            ))
                            .unwrap(),
                    )
                    .await
                    .unwrap();
                assert_eq!(
                    resp.status(),
                    StatusCode::FORBIDDEN,
                    "preview agent must not approve specs"
                );
            }
            assert!(
                state
                    .spec_approval_history
                    .list_by_path("system/search.md")
                    .await
                    .unwrap()
                    .is_empty(),
                "the refused approval must not have recorded an event"
            );
            let entry = state
                .spec_ledger
                .find_by_path("system/search.md")
                .await
                .unwrap()
                .unwrap();
            assert_eq!(
                entry.approval_status,
                gyre_domain::ApprovalStatus::Pending,
                "the refused approval must not have transitioned the ledger"
            );
            // Positive control: the admin approves the same spec version.
            let resp = prod
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/v1/specs/system%2Fsearch.md/approve")
                        .header("authorization", "Bearer test-token")
                        .header("content-type", "application/json")
                        .body(Body::from(
                            serde_json::json!({ "sha": spec_sha }).to_string(),
                        ))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                resp.status(),
                StatusCode::CREATED,
                "admin spec approval must still work"
            );

            // ── 3. POST /api/v1/compose/apply — task/agent row creation is
            //    exactly the ceremony POST /api/v1/tasks already refuses
            //    for preview callers ─────────────────────────────────
            let compose_body = serde_json::json!({
                "version": "1",
                "repo_id": repo_id,
                "agents": [{
                    "name": "preview-compose-worker",
                    "role": "developer",
                    "parent": null,
                    "capabilities": [],
                    "task": {
                        "title": "preview compose task",
                        "description": null,
                        "priority": "Medium"
                    },
                    "branch": null,
                    "lifetime_secs": null
                }]
            });
            let agents_before = state.agents.list().await.unwrap().len();
            let tasks_before = state.tasks.list().await.unwrap().len();
            // Production router (realistic surface) and the bare api_router
            // (no ABAC middleware — the handler guard is the only refusal
            // layer), so the guard itself is proven, not just the policy set.
            for router in [prod.clone(), app.clone()] {
                let resp = router
                    .oneshot(
                        Request::builder()
                            .method("POST")
                            .uri("/api/v1/compose/apply")
                            .header("authorization", format!("Bearer {jwt}"))
                            .header("content-type", "application/json")
                            .body(Body::from(compose_body.to_string()))
                            .unwrap(),
                    )
                    .await
                    .unwrap();
                assert_eq!(
                    resp.status(),
                    StatusCode::FORBIDDEN,
                    "preview agent must not apply compose specs"
                );
            }
            assert_eq!(
                state.agents.list().await.unwrap().len(),
                agents_before,
                "the refused compose apply must not have created agent rows"
            );
            assert_eq!(
                state.tasks.list().await.unwrap().len(),
                tasks_before,
                "the refused compose apply must not have created task rows"
            );

            // Positive control: the admin applies the compose for real —
            // this is also the teardown target below.
            let resp = prod
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/v1/compose/apply")
                        .header("authorization", "Bearer test-token")
                        .header("content-type", "application/json")
                        .body(Body::from(compose_body.to_string()))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                resp.status(),
                StatusCode::CREATED,
                "admin compose apply must still work"
            );
            let compose_json = body_json(resp).await;
            let compose_id = compose_json["compose_id"].as_str().unwrap().to_string();
            let compose_agent_id = compose_json["agents"][0]["agent_id"]
                .as_str()
                .unwrap()
                .to_string();

            // ── 4. POST /api/v1/compose/teardown — killing another run's
            //    agents ────────────────────────────────────────────
            let resp = prod
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/v1/compose/teardown")
                        .header("authorization", format!("Bearer {jwt}"))
                        .header("content-type", "application/json")
                        .body(Body::from(
                            serde_json::json!({ "compose_id": compose_id }).to_string(),
                        ))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                resp.status(),
                StatusCode::FORBIDDEN,
                "preview agent must not tear down compose sessions via the production router"
            );
            // And on the bare api_router (no ABAC middleware) — the handler
            // guard itself is the only refusal layer.
            let resp = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/v1/compose/teardown")
                        .header("authorization", format!("Bearer {jwt}"))
                        .header("content-type", "application/json")
                        .body(Body::from(
                            serde_json::json!({ "compose_id": compose_id }).to_string(),
                        ))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                resp.status(),
                StatusCode::FORBIDDEN,
                "preview agent must not tear down compose sessions via the bare router"
            );
            let compose_agent = state
                .agents
                .find_by_id(&gyre_common::Id::new(&compose_agent_id))
                .await
                .unwrap()
                .unwrap();
            assert_eq!(
                compose_agent.status,
                gyre_domain::AgentStatus::Idle,
                "the refused teardown must not have killed the compose agents"
            );
            // Positive control: the operator tears the session down.
            let resp = prod
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/v1/compose/teardown")
                        .header("authorization", "Bearer test-token")
                        .header("content-type", "application/json")
                        .body(Body::from(
                            serde_json::json!({ "compose_id": compose_id }).to_string(),
                        ))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                resp.status(),
                StatusCode::NO_CONTENT,
                "admin compose teardown must still work"
            );

            // ── 5. POST /api/v1/workspaces/{id}/orchestrator/spawn ──────
            //    (ABAC-mapped to workspace/write — not repo-addressed, so the
            //    agent-repo-scope containment cannot fire and the builtin
            //    Agent-role Allow grants; the handler role gate is what refuses)
            let orch_uri = format!("/api/v1/workspaces/{}/orchestrator/spawn", ws_id);
            // The admin spec-approval positive control above legitimately
            // triggered the signal chain's server-side
            // ensure_workspace_orchestrator (task-115: SpecApproved →
            // exactly one live workspace orchestrator, spawned with no HTTP
            // caller). Snapshot the orchestrator ids present before the
            // refusal probes so the assert below attributes any NEW
            // orchestrator to the refused requests exactly.
            let orchestrators_before: std::collections::HashSet<String> = state
                .agents
                .list_by_workspace(&ws_id)
                .await
                .unwrap()
                .into_iter()
                .filter(|a| {
                    a.orchestrator_type == gyre_domain::OrchestratorType::WorkspaceOrchestrator
                })
                .map(|a| a.id.to_string())
                .collect();
            // The preview agent through the production router (realistic
            // surface), and the plain Agent-role caller through a bare
            // api_router built from the JWT twin (no ABAC middleware) so
            // the handler role gate itself is the only refusal layer.
            let bare_jwt_router = crate::api::api_router().with_state(jwt_state.clone());
            for (router, token, label) in [
                (&prod, &jwt, "preview agent"),
                (&bare_jwt_router, &plain_agent, "agent-role caller"),
            ] {
                let resp = router
                    .clone()
                    .oneshot(
                        Request::builder()
                            .method("POST")
                            .uri(&orch_uri)
                            .header("authorization", format!("Bearer {token}"))
                            .header("content-type", "application/json")
                            .body(Body::from(
                                serde_json::json!({ "name": "should-not-spawn" }).to_string(),
                            ))
                            .unwrap(),
                    )
                    .await
                    .unwrap();
                assert_eq!(
                    resp.status(),
                    StatusCode::FORBIDDEN,
                    "an {label} must not spawn a workspace orchestrator"
                );
            }
            let orchestrators_after: std::collections::HashSet<String> = state
                .agents
                .list_by_workspace(&ws_id)
                .await
                .unwrap()
                .into_iter()
                .filter(|a| {
                    a.orchestrator_type == gyre_domain::OrchestratorType::WorkspaceOrchestrator
                })
                .map(|a| a.id.to_string())
                .collect();
            assert!(
                orchestrators_after.is_subset(&orchestrators_before),
                "neither refusal may have minted a workspace orchestrator \
                 (before: {orchestrators_before:?}, after: {orchestrators_after:?})"
            );
            // The spawn core enforces one LIVE workspace orchestrator per
            // workspace (§3.2), so the signal chain's orchestrator from the
            // approval positive control must be retired before the admin
            // positive control can mint its own (a 409 there would be the
            // one-live rule working, not a refusal bug).
            for orch_id in &orchestrators_after {
                let resp = prod
                    .clone()
                    .oneshot(
                        Request::builder()
                            .method("POST")
                            .uri(format!("/api/v1/admin/agents/{orch_id}/kill"))
                            .header("authorization", "Bearer test-token")
                            .body(Body::empty())
                            .unwrap(),
                    )
                    .await
                    .unwrap();
                assert_eq!(
                    resp.status(),
                    StatusCode::NO_CONTENT,
                    "admin kill of the signal-chain orchestrator must work"
                );
            }
            // Positive control: the operator spawns one, then kills it
            // (terminating the launched process) so the test leaves nothing
            // behind.
            let resp = prod
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri(&orch_uri)
                        .header("authorization", "Bearer test-token")
                        .header("content-type", "application/json")
                        .body(Body::from(
                            serde_json::json!({ "name": "control-ws-orch" }).to_string(),
                        ))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                resp.status(),
                StatusCode::CREATED,
                "admin workspace-orchestrator spawn must still work"
            );
            let orch_json = body_json(resp).await;
            let orch_id = orch_json["agent"]["id"].as_str().unwrap().to_string();
            let resp = prod
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri(format!("/api/v1/admin/agents/{orch_id}/kill"))
                        .header("authorization", "Bearer test-token")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                resp.status(),
                StatusCode::NO_CONTENT,
                "admin kill cleanup must work"
            );

            let del = delete_preview(&app, &preview_id).await;
            assert_eq!(del.status(), StatusCode::NO_CONTENT);
        })
        .await;
    }

    /// Review round 2 regressions (F1–F5): the preview-agent containment
    /// must hold on the five REST routes the first guard pass missed —
    /// repo-tier orchestrator spawn (role gate + refusal), bare agent
    /// registration (identity laundering via a minted non-preview token),
    /// completing ANOTHER agent (caller-keyed, not target-keyed), spec save
    /// (auto-MR + notification ceremony), and commit provenance recording
    /// (audit trail). Each refusal is driven on the production router
    /// (auth + ABAC middleware — the builtin agent-scoped-access policy
    /// grants the Agent role the mapped action, so the middleware alone
    /// would let it through) and, where the route is not registry-exempt,
    /// also on the bare api_router so the per-handler guard itself is the
    /// only refusal layer being proven.
    #[tokio::test(flavor = "multi_thread")]
    async fn preview_agent_rest_identity_provenance_and_repo_orchestrator_are_refused() {
        use crate::auth::test_helpers::{make_jwt_config, sign_test_jwt};

        let dir = tempfile::tempdir().unwrap();
        let (state, repo_id, ws_id) = preview_state(dir.path(), 86_400, None).await;
        let app = crate::api::api_router().with_state(state.clone());

        // JWT-enabled twin of the same state (Arc-shared port stores), so a
        // minted plain Agent-role caller (not a preview agent) can drive the
        // bare router and prove the orchestrator role gate itself.
        let mut jwt_state = (*state).clone();
        jwt_state.jwt_config = Some(make_jwt_config());
        let jwt_state = std::sync::Arc::new(jwt_state);

        // The repo-orchestrator positive control needs the fail-closed
        // persona layers (platform-model §4.3), same as startup seeding.
        crate::seed_builtin_meta_specs(&jwt_state).await;
        crate::seed_builtin_personas_for_tenant(&jwt_state, &gyre_common::Id::new("default")).await;

        // A non-preview Active victim in the same workspace: F3's escape is
        // a preview agent completing ANOTHER agent (the target-keyed guard
        // alone passes because the victim is not a preview agent).
        let mut victim = gyre_domain::Agent::new(
            gyre_common::Id::new(uuid::Uuid::new_v4().to_string()),
            "complete-victim",
            chrono_now_secs(),
        );
        victim.workspace_id = ws_id.clone();
        victim
            .transition_status(gyre_domain::AgentStatus::Active)
            .unwrap();
        state.agents.create(&victim).await.unwrap();
        let victim_id = victim.id.to_string();

        let env_out = dir.path().join("agent-env.txt");
        let script = env_dump_script(dir.path(), &env_out);

        with_agent_command(script.to_str().unwrap(), || async {
            let resp =
                post_preview(&app, preview_body(&repo_id, &["specs/system/search.md"])).await;
            assert_eq!(resp.status(), StatusCode::ACCEPTED);
            let json = body_json(resp).await;
            let preview_id = json["preview_id"].as_str().unwrap().to_string();
            let agent_id = json["agents"][0]["agent_id"].as_str().unwrap().to_string();

            crate::abac_middleware::seed_builtin_policies(&jwt_state)
                .await
                .expect("seed built-in policies");
            let prod = crate::build_router(jwt_state.clone());
            let bare_jwt_router = crate::api::api_router().with_state(jwt_state.clone());
            let jwt = state
                .kv_store
                .kv_get("agent_tokens", &agent_id)
                .await
                .unwrap()
                .expect("preview agent token must be live");

            // In-tenant Agent-role caller that is NOT a preview agent: for
            // the repo-orchestrator role gate, this is the exact principal
            // the gate must refuse (a live Agent-role JWT that the builtin
            // policy would otherwise let through on both tiers).
            let plain_agent = sign_test_jwt(
                &serde_json::json!({
                    "tenant_id": "default",
                    "sub": "plain-worker",
                    "preferred_username": "plain-worker",
                    "realm_access": { "roles": ["agent"] }
                }),
                3600,
            );

            let post = |router: Router, uri: String, token: String, body: serde_json::Value| {
                async move {
                    router
                        .oneshot(
                            Request::builder()
                                .method("POST")
                                .uri(&uri)
                                .header("authorization", format!("Bearer {token}"))
                                .header("content-type", "application/json")
                                .body(Body::from(body.to_string()))
                                .unwrap(),
                        )
                        .await
                        .unwrap()
                }
            };

            // ── F1. POST /api/v1/repos/:id/orchestrator/spawn (repo tier):
            //    the preview agent through the production router, and the
            //    plain Agent-role caller through a bare api_router built
            //    from the JWT twin (no ABAC middleware) so the handler role
            //    gate itself is the only refusal layer.
            let orch_uri = format!("/api/v1/repos/{repo_id}/orchestrator/spawn");
            let orch_body = serde_json::json!({ "name": "should-not-spawn" });
            let resp = post(
                prod.clone(),
                orch_uri.clone(),
                jwt.clone(),
                orch_body.clone(),
            )
            .await;
            assert_eq!(
                resp.status(),
                StatusCode::FORBIDDEN,
                "a preview agent must not spawn a repo orchestrator"
            );
            let resp = post(
                bare_jwt_router.clone(),
                orch_uri.clone(),
                plain_agent.clone(),
                orch_body.clone(),
            )
            .await;
            assert_eq!(
                resp.status(),
                StatusCode::FORBIDDEN,
                "a plain Agent-role caller must not spawn a repo orchestrator"
            );
            assert!(
                state
                    .agents
                    .list_by_workspace(&ws_id)
                    .await
                    .unwrap()
                    .iter()
                    .all(|a| a.orchestrator_type
                        != gyre_domain::OrchestratorType::RepoOrchestrator),
                "neither refusal may have minted a repo orchestrator"
            );
            // Positive control: the operator (global Admin token) spawns
            // the repo orchestrator, then kills it so nothing lingers.
            let resp = post(
                prod.clone(),
                orch_uri.clone(),
                "test-token".to_string(),
                serde_json::json!({ "name": "control-repo-orch" }),
            )
            .await;
            assert_eq!(
                resp.status(),
                StatusCode::CREATED,
                "admin repo-orchestrator spawn must still work"
            );
            let orch_json = body_json(resp).await;
            let orch_id = orch_json["agent"]["id"].as_str().unwrap().to_string();
            let resp = prod
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri(format!("/api/v1/admin/agents/{orch_id}/kill"))
                        .header("authorization", "Bearer test-token")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                resp.status(),
                StatusCode::NO_CONTENT,
                "admin kill cleanup must work"
            );

            // ── F2. POST /api/v1/agents: registering an agent mints a fresh
            //    opaque token for a NEW agent principal that no preview
            //    guard indexes — one call launders a fresh identity past
            //    every caller-keyed refusal.
            let agents_before = state.agents.list().await.unwrap().len();
            let tokens_before = state
                .kv_store
                .kv_list("agent_tokens")
                .await
                .unwrap()
                .len();
            let reg_body = serde_json::json!({ "name": "laundered-identity" });
            for router in [prod.clone(), app.clone()] {
                let resp = post(
                    router,
                    "/api/v1/agents".to_string(),
                    jwt.clone(),
                    reg_body.clone(),
                )
                .await;
                assert_eq!(
                    resp.status(),
                    StatusCode::FORBIDDEN,
                    "a preview agent must not register agents (identity laundering)"
                );
            }
            assert_eq!(
                state.agents.list().await.unwrap().len(),
                agents_before,
                "the refused registration must not have created an agent row"
            );
            assert_eq!(
                state.kv_store.kv_list("agent_tokens").await.unwrap().len(),
                tokens_before,
                "the refused registration must not have minted an agent token"
            );
            // Positive control: the operator registers an agent.
            let resp = post(
                prod.clone(),
                "/api/v1/agents".to_string(),
                "test-token".to_string(),
                serde_json::json!({ "name": "operator-registered" }),
            )
            .await;
            assert_eq!(
                resp.status(),
                StatusCode::CREATED,
                "admin agent registration must still work"
            );

            // ── F3. POST /api/v1/agents/{id}/complete for ANOTHER agent:
            //    the caller-keyed guard must refuse the preview agent even
            //    though the target is not a preview agent (the target-keyed
            //    guard alone passes).
            let complete_body = serde_json::json!({
                "branch": "feat/victim",
                "title": "ceremony escape",
                "target_branch": "main",
            });
            for router in [prod.clone(), app.clone()] {
                let resp = post(
                    router,
                    format!("/api/v1/agents/{victim_id}/complete"),
                    jwt.clone(),
                    complete_body.clone(),
                )
                .await;
                assert_eq!(
                    resp.status(),
                    StatusCode::FORBIDDEN,
                    "a preview agent must not complete another agent"
                );
            }
            let row = state
                .agents
                .find_by_id(&gyre_common::Id::new(&victim_id))
                .await
                .unwrap()
                .unwrap();
            assert_eq!(
                row.status,
                gyre_domain::AgentStatus::Active,
                "the refused complete must not have transitioned the victim"
            );
            assert!(
                state.merge_requests.list().await.unwrap().is_empty(),
                "the refused complete must not have created an MR"
            );

            // ── F4. POST /api/v1/repos/:id/specs/save: auto-creates an MR
            //    plus a 'Spec pending approval' notification — MR-creation
            //    ceremony §5 forbids for preview runs.
            let save_body = serde_json::json!({
                "spec_path": "specs/system/search.md",
                "content": "# Search\n\nsmuggled edit\n",
                "message": "ceremony escape"
            });
            for router in [prod.clone(), app.clone()] {
                let resp = post(
                    router,
                    format!("/api/v1/repos/{repo_id}/specs/save"),
                    jwt.clone(),
                    save_body.clone(),
                )
                .await;
                assert_eq!(
                    resp.status(),
                    StatusCode::FORBIDDEN,
                    "a preview agent must not save specs (auto-MR ceremony)"
                );
            }
            assert!(
                state.merge_requests.list().await.unwrap().is_empty(),
                "the refused spec save must not have created an MR"
            );
            let repo = state
                .repos
                .find_by_id(&gyre_common::Id::new(&repo_id))
                .await
                .unwrap()
                .unwrap();
            assert!(
                state
                    .git_ops
                    .list_branches(&repo.path)
                    .await
                    .unwrap()
                    .iter()
                    .all(|b| !b.name.starts_with("spec-edit/")),
                "the refused spec save must not have created a spec-edit branch"
            );
            // Positive control: the operator saves the same spec.
            let resp = post(
                prod.clone(),
                format!("/api/v1/repos/{repo_id}/specs/save"),
                "test-token".to_string(),
                save_body.clone(),
            )
            .await;
            assert_eq!(
                resp.status(),
                StatusCode::CREATED,
                "admin spec save must still work"
            );

            // ── F5. POST /api/v1/repos/:id/commits/record: persists a
            //    caller-supplied agent_id as an AgentCommit provenance row —
            //    the audit trail §5 says preview runs must not leave.
            let record_body = serde_json::json!({
                "agent_id": agent_id,
                "commit_sha": "a".repeat(40),
                "branch": "preview-smuggled",
            });
            for router in [prod.clone(), app.clone()] {
                let resp = post(
                    router,
                    format!("/api/v1/repos/{repo_id}/commits/record"),
                    jwt.clone(),
                    record_body.clone(),
                )
                .await;
                assert_eq!(
                    resp.status(),
                    StatusCode::FORBIDDEN,
                    "a preview agent must not record commit provenance"
                );
            }
            assert!(
                state
                    .agent_commits
                    .find_by_repo(&gyre_common::Id::new(&repo_id))
                    .await
                    .unwrap()
                    .is_empty(),
                "the refused provenance recording must not have written a row"
            );
            // Positive control: the operator records a commit.
            let resp = post(
                prod.clone(),
                format!("/api/v1/repos/{repo_id}/commits/record"),
                "test-token".to_string(),
                record_body.clone(),
            )
            .await;
            assert_eq!(
                resp.status(),
                StatusCode::CREATED,
                "admin commit recording must still work"
            );

            let del = delete_preview(&app, &preview_id).await;
            assert_eq!(del.status(), StatusCode::NO_CONTENT);
        })
        .await;
    }

    /// F3 regression: the default workspace-budget path must reserve every
    /// requested slot up front — a single POST with more targets than the
    /// workspace's max_concurrent_agents is 429, not an over-admitted run.
    #[tokio::test(flavor = "multi_thread")]
    async fn preview_workspace_budget_caps_multi_target_request() {
        let dir = tempfile::tempdir().unwrap();
        // Separate preview budget unset: the workspace budget governs.
        let (app, state, repo_id) = preview_app(dir.path(), 86_400, None).await;

        let repo = state
            .repos
            .find_by_id(&gyre_common::Id::new(&repo_id))
            .await
            .unwrap()
            .unwrap();
        let ws_key = super::super::budget::workspace_key(repo.workspace_id.as_str());
        state
            .budget_configs
            .set_config(
                &ws_key,
                &gyre_domain::BudgetConfig {
                    max_concurrent_agents: Some(1),
                    ..Default::default()
                },
            )
            .await
            .unwrap();

        let env_out = dir.path().join("agent-env.txt");
        let script = env_dump_script(dir.path(), &env_out);

        with_agent_command(script.to_str().unwrap(), || async {
            // Two targets against max_concurrent_agents=1 → 429 before any
            // spawn (no rollback debris to clean up).
            let resp = post_preview(
                &app,
                preview_body(
                    &repo_id,
                    &["specs/system/search.md", "specs/system/identity.md"],
                ),
            )
            .await;
            assert_eq!(
                resp.status(),
                StatusCode::TOO_MANY_REQUESTS,
                "multi-target POST must reserve every slot up front"
            );
            // Nothing was provisioned: the refusal fires before Pass 3.
            assert!(
                state.agents.list().await.unwrap().is_empty(),
                "no agent may be provisioned by the over-budget request"
            );

            // Raise the workspace cap to 2 → the same request is admitted.
            state
                .budget_configs
                .set_config(
                    &ws_key,
                    &gyre_domain::BudgetConfig {
                        max_concurrent_agents: Some(2),
                        ..Default::default()
                    },
                )
                .await
                .unwrap();
            let resp = post_preview(
                &app,
                preview_body(
                    &repo_id,
                    &["specs/system/search.md", "specs/system/identity.md"],
                ),
            )
            .await;
            assert_eq!(resp.status(), StatusCode::ACCEPTED);
            let json = body_json(resp).await;
            let preview_id = json["preview_id"].as_str().unwrap().to_string();
            assert_eq!(json["agents"].as_array().unwrap().len(), 2);

            let del = delete_preview(&app, &preview_id).await;
            assert_eq!(del.status(), StatusCode::NO_CONTENT);
        })
        .await;
    }

    /// Tearing a preview run down must close MRs pointing at its branches —
    /// an open MR dangling off a deleted branch is ceremony debris preview
    /// mode promises not to leave behind.
    #[tokio::test(flavor = "multi_thread")]
    async fn preview_delete_closes_mrs_on_preview_branches() {
        let dir = tempfile::tempdir().unwrap();
        let (app, state, repo_id) = preview_app(dir.path(), 86_400, None).await;

        let env_out = dir.path().join("agent-env.txt");
        let script = env_dump_script(dir.path(), &env_out);

        with_agent_command(script.to_str().unwrap(), || async {
            let resp =
                post_preview(&app, preview_body(&repo_id, &["specs/system/search.md"])).await;
            assert_eq!(resp.status(), StatusCode::ACCEPTED);
            let json = body_json(resp).await;
            let preview_id = json["preview_id"].as_str().unwrap().to_string();
            let branch = json["agents"][0]["branch"].as_str().unwrap().to_string();

            // An open MR on the preview branch (e.g. created before a gate
            // existed, or by an admin).
            let repo = state
                .repos
                .find_by_id(&gyre_common::Id::new(&repo_id))
                .await
                .unwrap()
                .unwrap();
            let mut mr = gyre_domain::MergeRequest::new(
                gyre_common::Id::new(uuid::Uuid::new_v4().to_string()),
                gyre_common::Id::new(&repo_id),
                repo.workspace_id.clone(),
                "mr on preview branch".to_string(),
                branch.clone(),
                "main".to_string(),
                chrono_now_secs(),
            );
            mr.author_agent_id = Some(gyre_common::Id::new(
                json["agents"][0]["agent_id"].as_str().unwrap(),
            ));
            state.merge_requests.create(&mr).await.unwrap();
            assert_eq!(mr.status, gyre_domain::MrStatus::Open);

            let del = delete_preview(&app, &preview_id).await;
            assert_eq!(del.status(), StatusCode::NO_CONTENT);

            let closed = state
                .merge_requests
                .find_by_id(&mr.id)
                .await
                .unwrap()
                .expect("MR row must survive teardown (only the branch is deleted)");
            assert_eq!(
                closed.status,
                gyre_domain::MrStatus::Closed,
                "MR on a torn-down preview branch must be closed, not left open"
            );
        })
        .await;
    }
}

// ===========================================================================
// Meta-spec registry CRUD API (agent-runtime spec §2)
//
// GET    /api/v1/meta-specs           — list (query: scope, scope_id, kind, required)
// POST   /api/v1/meta-specs           — create
// GET    /api/v1/meta-specs/:id       — get by id
// PUT    /api/v1/meta-specs/:id       — update (content edit bumps version; approval-only PUT does not)
// DELETE /api/v1/meta-specs/:id       — delete (409 if bindings)
// GET    /api/v1/meta-specs/:id/versions       — list versions
// GET    /api/v1/meta-specs/:id/versions/:ver  — get specific version
// ===========================================================================

use axum::extract::Query;
use gyre_domain::meta_spec::{MetaSpec, MetaSpecApprovalStatus, MetaSpecKind, MetaSpecScope};
use gyre_ports::MetaSpecFilter;
use sha2::{Digest, Sha256};

fn sha256_hex(s: &str) -> String {
    let mut h = Sha256::new();
    h.update(s.as_bytes());
    hex::encode(h.finalize())
}

// ---------------------------------------------------------------------------
// Request / response types
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct ListMetaSpecsQuery {
    pub scope: Option<String>,
    pub scope_id: Option<String>,
    pub kind: Option<String>,
    pub required: Option<bool>,
}

#[derive(Deserialize)]
pub struct CreateMetaSpecRequest {
    pub kind: String,
    pub name: String,
    pub scope: String,
    pub scope_id: Option<String>,
    pub prompt: Option<String>,
    pub required: Option<bool>,
}

#[derive(Deserialize)]
pub struct UpdateMetaSpecRequest {
    pub name: Option<String>,
    pub prompt: Option<String>,
    pub required: Option<bool>,
    pub approval_status: Option<String>,
    /// Tenant-admin override (§8): when approving a Tenant-scoped meta-spec,
    /// propagate the change past workspace-level overrides (urgent
    /// compliance-driven changes). Only meaningful for Tenant-scoped specs;
    /// rejected otherwise. Restricted to admins of that tenant.
    #[serde(default)]
    pub force: Option<bool>,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn parse_kind(s: &str) -> Result<MetaSpecKind, ApiError> {
    // Case-insensitive: the §2 API table spells the query filter
    // `?kind=meta:persona` and `?scope=workspace` in either case; accept
    // any case, serialize canonically.
    MetaSpecKind::parse(&s.to_ascii_lowercase()).ok_or_else(|| {
        ApiError::BadRequest(format!(
            "invalid kind '{s}'; must be one of: meta:persona, meta:principle, meta:standard, meta:process"
        ))
    })
}

fn parse_scope(s: &str) -> Result<MetaSpecScope, ApiError> {
    // Case-insensitive (see parse_kind): `?scope=workspace` and
    // `?scope=Workspace` both name the Workspace scope. The domain
    // `MetaSpecScope::parse` is case-sensitive (canonical form only), so
    // the leniency lives here at the API edge.
    match s.to_ascii_lowercase().as_str() {
        "global" => Ok(MetaSpecScope::Global),
        "tenant" => Ok(MetaSpecScope::Tenant),
        "workspace" => Ok(MetaSpecScope::Workspace),
        _ => Err(ApiError::BadRequest(format!(
            "invalid scope '{s}'; must be one of: Global, Tenant, Workspace"
        ))),
    }
}

fn parse_approval_status(s: &str) -> Result<MetaSpecApprovalStatus, ApiError> {
    MetaSpecApprovalStatus::parse(s).ok_or_else(|| {
        ApiError::BadRequest(format!(
            "invalid approval_status '{s}'; must be: Pending, Approved, or Rejected"
        ))
    })
}

// ---------------------------------------------------------------------------
// Registry authorization (the per-handler gate for registry writes; reads
// and versions go through `require_meta_spec_access` below)
// ---------------------------------------------------------------------------

/// Scope-level write gate for registry mutations (agent-runtime §2
/// Required vs Optional: "Only admins at the meta-spec's own scope level —
/// tenant admins for tenant meta-specs, workspace admins (Owner/Admin
/// role) for workspace meta-specs"; §214 Registry Levels). Creating and
/// updating a registry entry — including flipping `required` in either
/// direction — is authorized by the meta-spec's OWN scope:
/// - Global / Tenant entries: tenant Admin only (for Tenant entries, an
///   Admin of the scope tenant — the §8 containment rule, same as the
///   force flag and DELETE);
/// - Workspace entries: that workspace's Owner/Admin members (the tenant
///   Admin bypass is retained, bounded to the workspace's own tenant).
///
/// A workspace Owner/Admin writing a Global/Tenant entry is the
/// privilege-escalation direction the spec warns about — 403.
///
/// `require_registry_scope` (containment) runs before this gate wherever
/// both apply, so a membership-based grant can never cross tenants. That
/// caller's Admin bypass admits an Admin of ANY tenant, though, so the
/// Workspace branch carries its own unconditional tenant-containment
/// check (mirroring `authorize_meta_spec_delete`): the tenant-Admin write
/// bypass is bounded to the workspace's own tenant — an Admin of tenant A
/// can never write (or flip `required` on) a meta-spec inside tenant B's
/// workspaces.
async fn authorize_registry_write(
    state: &AppState,
    auth: &AuthenticatedAgent,
    scope: &MetaSpecScope,
    scope_id: &Option<String>,
    route: &str,
) -> Result<(), ApiError> {
    let is_tenant_admin = auth.roles.contains(&UserRole::Admin);
    match scope {
        MetaSpecScope::Global => {
            if !is_tenant_admin {
                crate::rbac::record_rbac_denied(state, auth, "Admin", route).await;
                return Err(ApiError::Forbidden(
                    "only tenant Admin may write Global-scoped meta-specs".to_string(),
                ));
            }
        }
        MetaSpecScope::Tenant => {
            // §8 containment: only an Admin of the scope tenant may write —
            // an admin of another tenant must not write, or learn of, this
            // entry. A missing scope_id fails closed (no caller's tenant
            // scope matches the empty string).
            let scope_tenant = scope_id.as_deref().unwrap_or("");
            if !is_tenant_admin || auth.tenant_id != scope_tenant {
                crate::rbac::record_rbac_denied(state, auth, "Admin", route).await;
                return Err(ApiError::Forbidden(
                    "only an Admin of the scope tenant may write tenant-scoped meta-specs"
                        .to_string(),
                ));
            }
        }
        MetaSpecScope::Workspace => {
            let ws_id = scope_id.as_deref().ok_or_else(|| {
                ApiError::BadRequest("workspace-scoped meta-spec requires scope_id".to_string())
            })?;
            let wid = Id::new(ws_id);
            let ws = state
                .workspaces
                .find_by_id(&wid)
                .await
                .map_err(ApiError::Internal)?
                .ok_or_else(|| {
                    // Dangling scope reference — fail closed rather than
                    // write an entry whose scope cannot be verified.
                    // (`require_registry_scope`, which runs first in both
                    // write paths, already returns NotFound here; this is
                    // depth, not a second existence oracle.)
                    ApiError::NotFound(format!("workspace '{ws_id}' not found"))
                })?;
            // Tenant containment, unconditionally — no Admin bypass here:
            // the bypass in `require_registry_scope` admits an Admin of
            // ANY tenant, so this gate must bound it itself. An Admin of
            // tenant A writing (or flipping `required` on) a workspace
            // meta-spec inside tenant B is the same cross-tenant
            // escalation `authorize_meta_spec_delete` rejects (§8).
            if ws.tenant_id.as_str() != auth.tenant_id {
                crate::rbac::record_rbac_denied(state, auth, "Admin", route).await;
                return Err(ApiError::Forbidden(
                    "cross-tenant meta-spec access denied".to_string(),
                ));
            }
            // Workspace-registry entry: the caller must be an Owner/Admin
            // member of that workspace (or a tenant Admin — of THIS
            // tenant, per the containment check above). The membership
            // store is the only source — never a claim.
            let is_ws_admin = match &auth.user_id {
                Some(uid) => state
                    .workspace_memberships
                    .find_by_user_and_workspace(uid, &wid)
                    .await
                    .map_err(ApiError::Internal)?
                    .is_some_and(|m| {
                        matches!(
                            m.role,
                            gyre_domain::WorkspaceRole::Owner | gyre_domain::WorkspaceRole::Admin
                        )
                    }),
                None => false,
            };
            if !is_tenant_admin && !is_ws_admin {
                crate::rbac::record_rbac_denied(state, auth, "workspace Admin", route).await;
                return Err(ApiError::Forbidden(
                    "only workspace Owner/Admin (or tenant Admin) may write workspace meta-specs"
                        .to_string(),
                ));
            }
        }
    }
    Ok(())
}

/// Scope containment for a scoped meta-spec: the named entity must exist and
/// belong to the caller's tenant (Admin bypasses). Global scope crosses
/// tenants by definition, so only the role gate applies to it. A `scope_id`
/// that does not resolve to a real entity is denied rather than ridden on an
/// assumed identity (AGENTS.md — never fabricate a scope).
async fn require_registry_scope(
    state: &AppState,
    auth: &AuthenticatedAgent,
    scope: &MetaSpecScope,
    scope_id: &Option<String>,
) -> Result<(), ApiError> {
    match scope {
        MetaSpecScope::Global => Ok(()),
        MetaSpecScope::Tenant => {
            // Tenant-scoped meta-specs carry the tenant ID in scope_id
            // (mirroring the Workspace design); the tenant must exist so the
            // §8 tenant propagation trigger has a real scope to fan out to.
            let tenant_id = scope_id.as_deref().ok_or_else(|| {
                ApiError::BadRequest(
                    "scope_id (tenant id) is required for Tenant-scoped meta-specs".to_string(),
                )
            })?;
            match state.tenants.find_by_id(&Id::new(tenant_id)).await? {
                Some(t)
                    if t.id.as_str() == auth.tenant_id || auth.roles.contains(&UserRole::Admin) =>
                {
                    Ok(())
                }
                Some(_) => Err(ApiError::Forbidden(format!(
                    "tenant-scoped meta-spec '{tenant_id}' is outside the caller's tenant"
                ))),
                None => Err(ApiError::NotFound(format!(
                    "tenant '{tenant_id}' not found"
                ))),
            }
        }
        MetaSpecScope::Workspace => {
            let ws_id = scope_id.as_deref().ok_or_else(|| {
                ApiError::BadRequest("workspace-scoped meta-spec requires scope_id".to_string())
            })?;
            match state.workspaces.find_by_id(&Id::new(ws_id)).await? {
                Some(ws)
                    if ws.tenant_id.as_str() == auth.tenant_id
                        || auth.roles.contains(&UserRole::Admin) =>
                {
                    Ok(())
                }
                Some(_) => Err(ApiError::Forbidden(format!(
                    "workspace-scoped meta-spec '{ws_id}' is outside the caller's tenant"
                ))),
                None => Err(ApiError::NotFound(format!("workspace '{ws_id}' not found"))),
            }
        }
    }
}

/// Per-handler authorization for a registry entry point operating on an
/// existing meta-spec (get/update/delete/versions): load the entity, derive
/// its scope from the record — never from the request — and compare tenant.
async fn require_meta_spec_access(
    state: &AppState,
    auth: &AuthenticatedAgent,
    id: &str,
) -> Result<MetaSpec, ApiError> {
    let ms = state
        .meta_specs
        .get_by_id(&Id::new(id))
        .await
        .map_err(ApiError::Internal)?
        .ok_or_else(|| ApiError::NotFound(format!("meta-spec '{id}' not found")))?;
    require_registry_scope(state, auth, &ms.scope, &ms.scope_id).await?;
    Ok(ms)
}

// ---------------------------------------------------------------------------
// GET /api/v1/meta-specs
// ---------------------------------------------------------------------------

pub async fn list_meta_specs_registry(
    State(state): State<Arc<AppState>>,
    auth: AuthenticatedAgent,
    Query(q): Query<ListMetaSpecsQuery>,
) -> Result<Json<Vec<MetaSpec>>, ApiError> {
    // Admin-only: the registry is tenant-wide governance configuration (it
    // defines the meta-specs that can be bound to any workspace). This
    // per-handler gate is the role surface for the listing; the route is
    // also evaluated by the ABAC middleware (resolver mapping
    // /api/v1/meta-specs) — any authenticated agent must not enumerate
    // cross-workspace governance.
    if !auth.roles.contains(&gyre_domain::UserRole::Admin) {
        crate::rbac::record_rbac_denied(&state, &auth, "Admin", "GET /api/v1/meta-specs").await;
        return Err(ApiError::Forbidden(
            "only Admin role may list the meta-specs registry".to_string(),
        ));
    }
    let scope = match q.scope.as_deref() {
        None => None,
        Some(s) => Some(parse_scope(s)?),
    };
    let kind = match q.kind.as_deref() {
        None => None,
        Some(k) => Some(parse_kind(k)?),
    };

    // Per-handler authorization (route is ABAC-exempt): a Workspace-scoped
    // listing is a workspace membership view — the caller must belong to the
    // workspace whose entries they enumerate (tenant containment for user
    // tokens, membership via the explorer-views pattern). A Tenant-scoped
    // listing is a tenant governance view — the caller's tenant must match
    // the queried tenant. Global-scope registry entries are shared reference
    // data, readable by any authenticated principal (docs/api-reference.md
    // §meta-specs).
    if let (Some(MetaSpecScope::Tenant), Some(scope_id)) = (&scope, &q.scope_id) {
        if auth.tenant_id != *scope_id {
            return Err(ApiError::Forbidden(
                "Access denied: cannot list meta-specs for a different tenant".to_string(),
            ));
        }
    }
    if let (Some(MetaSpecScope::Workspace), Some(scope_id)) = (&scope, &q.scope_id) {
        let wid = Id::new(scope_id);
        match state.workspaces.find_by_id(&wid).await? {
            Some(ws) => {
                if ws.tenant_id.as_str() != auth.tenant_id {
                    return Err(ApiError::Forbidden(
                        "Access denied: workspace not in your tenant".to_string(),
                    ));
                }
                if let Some(user_id) = &auth.user_id {
                    match state
                        .workspace_memberships
                        .find_by_user_and_workspace(user_id, &wid)
                        .await
                    {
                        Ok(Some(_)) => {}
                        Ok(None) => {
                            return Err(ApiError::Forbidden(
                                "Access denied: not a member of this workspace".to_string(),
                            ));
                        }
                        Err(e) => return Err(ApiError::Internal(e)),
                    }
                }
            }
            None => {
                return Err(ApiError::NotFound(format!(
                    "workspace '{scope_id}' not found"
                )));
            }
        }
    }

    let filter = MetaSpecFilter {
        scope,
        scope_id: q.scope_id,
        kind,
        required: q.required,
    };
    let mut results = state
        .meta_specs
        .list(&filter)
        .await
        .map_err(ApiError::Internal)?;
    if !auth.roles.contains(&UserRole::Admin) {
        // A Workspace-scoped meta-spec is visible only when its scope names
        // one of the caller's own workspaces (tenant containment). Global
        // scope is visible to every authenticated caller. An unresolvable
        // scope_id is skipped, never guessed (never fabricate a scope).
        let mine: std::collections::HashSet<String> = state
            .workspaces
            .list_by_tenant(&Id::new(&auth.tenant_id))
            .await
            .map_err(ApiError::Internal)?
            .into_iter()
            .map(|ws| ws.id.to_string())
            .collect();
        results.retain(|ms| match ms.scope {
            MetaSpecScope::Global => true,
            // §8 tenant-scoped entry: visible to members of the scope
            // tenant. An unresolvable scope_id fails closed — it can never
            // match the caller's tenant, so a dangling row is invisible.
            MetaSpecScope::Tenant => ms
                .scope_id
                .as_deref()
                .map(|tenant_id| tenant_id == auth.tenant_id)
                .unwrap_or(false),
            MetaSpecScope::Workspace => ms
                .scope_id
                .as_deref()
                .map(|ws_id| mine.contains(ws_id))
                .unwrap_or(false),
        });
    }
    Ok(Json(results))
}

// ---------------------------------------------------------------------------
// POST /api/v1/meta-specs
// ---------------------------------------------------------------------------

pub async fn create_meta_spec_registry(
    State(state): State<Arc<AppState>>,
    auth: AuthenticatedAgent,
    Json(req): Json<CreateMetaSpecRequest>,
) -> Result<(StatusCode, Json<MetaSpec>), ApiError> {
    let kind = parse_kind(&req.kind)?;
    let scope = parse_scope(&req.scope)?;
    // Scope containment: `scope_id` is caller-supplied, so it is validated
    // against the caller's tenant, never trusted.
    require_registry_scope(&state, &auth, &scope, &req.scope_id).await?;
    // Scope-level write gate (§2 Required vs Optional): the entry's own
    // scope decides who may create it.
    authorize_registry_write(
        &state,
        &auth,
        &scope,
        &req.scope_id,
        "POST /api/v1/meta-specs",
    )
    .await?;
    let prompt = req.prompt.unwrap_or_default();
    let content_hash = sha256_hex(&prompt);
    let now = now_secs();

    let ms = MetaSpec {
        id: Id::new(uuid::Uuid::new_v4().to_string()),
        kind,
        name: req.name,
        scope,
        scope_id: req.scope_id,
        prompt,
        version: 1,
        content_hash,
        required: req.required.unwrap_or(false),
        approval_status: MetaSpecApprovalStatus::Pending,
        approved_by: None,
        approved_at: None,
        approved_content_hash: None,
        created_by: auth.agent_id.as_str().to_string(),
        created_at: now,
        updated_at: now,
    };

    state
        .meta_specs
        .create(&ms)
        .await
        .map_err(ApiError::Internal)?;
    record_meta_spec_publish(&state, &auth, &ms).await?;
    Ok((StatusCode::CREATED, Json(ms)))
}

// ---------------------------------------------------------------------------
// GET /api/v1/meta-specs/:id
// ---------------------------------------------------------------------------

pub async fn get_meta_spec_registry(
    State(state): State<Arc<AppState>>,
    auth: AuthenticatedAgent,
    Path(id): Path<String>,
) -> Result<Json<MetaSpec>, ApiError> {
    let ms = require_meta_spec_access(&state, &auth, &id).await?;
    Ok(Json(ms))
}

// ---------------------------------------------------------------------------
// PUT /api/v1/meta-specs/:id
// ---------------------------------------------------------------------------

pub async fn update_meta_spec_registry(
    State(state): State<Arc<AppState>>,
    auth: AuthenticatedAgent,
    Path(id): Path<String>,
    Json(req): Json<UpdateMetaSpecRequest>,
) -> Result<Json<MetaSpec>, ApiError> {
    // Load the entity and derive its scope from the record — never from
    // the request. Authorization ordering mirrors DELETE: a missing row
    // must not become an existence oracle for unprivileged callers
    // (403 precedes 404); only a tenant Admin — who may write at least
    // some scope — gets the truthful 404.
    let mut ms = match state
        .meta_specs
        .get_by_id(&Id::new(&id))
        .await
        .map_err(ApiError::Internal)?
    {
        Some(ms) => ms,
        None => {
            if !auth.roles.contains(&UserRole::Admin) {
                crate::rbac::record_rbac_denied(
                    &state,
                    &auth,
                    "Admin",
                    "PUT /api/v1/meta-specs/:id",
                )
                .await;
                return Err(ApiError::Forbidden("meta-spec access denied".to_string()));
            }
            return Err(ApiError::NotFound(format!("meta-spec '{id}' not found")));
        }
    };
    // Scope containment first (the record's own scope must be inside the
    // caller's tenant, or the caller is a bypassing Admin), then the
    // scope-level write gate below — a membership-based grant can never
    // cross tenants because containment runs first.
    require_registry_scope(&state, &auth, &ms.scope, &ms.scope_id).await?;
    // Scope-level write gate (§2 Required vs Optional): the meta-spec's
    // own scope decides who may write it — tenant Admin for Global/Tenant
    // entries, the workspace's Owner/Admin members for Workspace entries
    // (tenant Admin bypass retained, bounded to the workspace's own
    // tenant).
    authorize_registry_write(
        &state,
        &auth,
        &ms.scope,
        &ms.scope_id,
        "PUT /api/v1/meta-specs/:id",
    )
    .await?;

    // §8 tenant-admin force flag: only defined for Tenant-scoped specs, and
    // only an admin of THAT tenant may force propagation into its
    // workspaces (an admin of another tenant must not be able to stomp
    // workspace rollout policies outside their own scope).
    let force = req.force.unwrap_or(false);
    if force {
        if ms.scope != MetaSpecScope::Tenant {
            return Err(ApiError::BadRequest(
                "force is only valid when approving a Tenant-scoped meta-spec".to_string(),
            ));
        }
        match &ms.scope_id {
            Some(scope_tenant) if auth.tenant_id == *scope_tenant => {}
            Some(_) => {
                return Err(ApiError::Forbidden(
                    "only an admin of the scope tenant may force tenant-wide propagation"
                        .to_string(),
                ))
            }
            None => {
                return Err(ApiError::Internal(anyhow::anyhow!(
                    "Tenant-scoped meta-spec '{id}' has no scope_id"
                )))
            }
        }
    }

    let now = now_secs();

    // Content hash the CURRENT approval covers, before this request mutates
    // the record. The §6 reconciliation trigger compares it against the new
    // content hash at approval time: approving content that differs from
    // what was last approved is the trigger, regardless of whether the edit
    // and the approval arrived in the same request or in the two-step
    // edit-then-approve flow the shipped UI performs.
    let previously_approved_hash = ms.approved_content_hash.clone();

    if let Some(name) = req.name {
        ms.name = name;
    }
    if let Some(prompt) = &req.prompt {
        let prompt = prompt.clone();
        ms.content_hash = sha256_hex(&prompt);
        ms.prompt = prompt;
        // New content is a new version. Metadata-only updates (approval,
        // required flag, name) do NOT bump: the version names the content
        // the §6 reconciliation title pins ("... {name} v{version}"), and a
        // bump on every PUT would give adjacent triggers of one logical
        // rollout (bind → approve) different titles, defeating the
        // (workspace, repo, title) dedup.
        ms.version += 1;
        // Spec §2: editing content resets approval to Pending until re-reviewed.
        ms.approval_status = MetaSpecApprovalStatus::Pending;
        ms.approved_by = None;
        ms.approved_at = None;
    }
    if let Some(required) = req.required {
        // §2 Required vs Optional: only scope-level admins can change the
        // required flag (either direction — both alter the injected set).
        // authorize_registry_write above enforces exactly that per-scope
        // grant: tenant Admin for Global/Tenant entries, the workspace's
        // Owner/Admin members for Workspace entries. A workspace admin
        // marking a tenant/Global meta-spec required is rejected there
        // (privilege escalation, per the spec's warning).
        ms.required = required;
    }
    if let Some(status_str) = &req.approval_status {
        let status = parse_approval_status(status_str)?;
        if status == MetaSpecApprovalStatus::Approved {
            ms.approved_by = Some(auth.agent_id.as_str().to_string());
            ms.approved_at = Some(now);
            // Stamp the content this approval covers so the next approval
            // can tell whether the content changed in between.
            ms.approved_content_hash = Some(ms.content_hash.clone());
        }
        ms.approval_status = status;
    }

    ms.updated_at = now;

    state
        .meta_specs
        .update(&ms)
        .await
        .map_err(ApiError::Internal)?;
    record_meta_spec_publish(&state, &auth, &ms).await?;

    // §11 MetaSpecChanged: meta-spec content updated in the registry.
    if req.prompt.is_some() {
        crate::reconciliation::emit_meta_spec_changed(&state, None, &ms).await;
    }

    // Meta-spec change approved and rolled out (§6 trigger): the content
    // under approval differs from what the previous approval covered, and
    // this update leaves the spec Approved. Every workspace whose meta-spec
    // set binds this meta-spec reconciles.
    //
    // Comparison is against the hash stamped by the LAST approval — not
    // `req.prompt.is_some()` — so the two-step edit-then-approve flow the
    // shipped UI performs (saveEdit PUTs {prompt}, handleApprove PUTs
    // {approval_status}) triggers reconciliation exactly like the combined
    // single-request flow. Re-approving unchanged content is a no-op.
    let content_changed_since_last_approval =
        previously_approved_hash.as_deref() != Some(ms.content_hash.as_str());
    let newly_approved = ms.approval_status == MetaSpecApprovalStatus::Approved
        && content_changed_since_last_approval;
    if newly_approved {
        let spec_path = ms.name.clone();
        if ms.scope == MetaSpecScope::Tenant {
            // §8 Tenant-Scope Meta-Spec Changes: a tenant-level approval fans
            // out to every workspace in the tenant — workspaces that override
            // the meta-spec with their own pin are skipped (their rollout
            // policy governs) unless `force` (tenant-admin override for
            // urgent compliance-driven changes). Each non-overriding
            // workspace reconciles autonomously via run_reconciliation.
            if let Some(scope_tenant) = &ms.scope_id {
                let summary = crate::reconciliation::run_tenant_reconciliation(
                    &state,
                    &Id::new(scope_tenant),
                    std::slice::from_ref(&spec_path),
                    force,
                )
                .await;
                tracing::info!(
                    tenant_id = %scope_tenant,
                    meta_spec = %spec_path,
                    force,
                    workspaces_scanned = summary.workspaces_scanned,
                    workspaces_skipped_override = summary.workspaces_skipped_override,
                    workspaces_skipped_error = summary.workspaces_skipped_error,
                    created = summary.tasks_created,
                    skipped = summary.tasks_skipped,
                    "tenant meta-spec approval triggered reconciliation"
                );
            }
        } else {
            // Global / Workspace scope: every workspace whose EFFECTIVE
            // meta-spec set (own bindings plus inherited tenant defaults, §8)
            // binds this meta-spec reconciles.
            let workspaces = state.workspaces.list().await.unwrap_or_default();
            for ws in &workspaces {
                // Never fail open the override gate (standing repo rule, same
                // fail-closed-skip as the tenant controller in
                // run_tenant_reconciliation): a workspace whose set cannot
                // be read is skipped, not decoded as "binds nothing".
                // Propagating the error with `?` would 500 the approval
                // AFTER `state.meta_specs.update(&ms)` already persisted it
                // and after workspaces listed earlier already received their
                // reconciliation tasks — a persisted approval paired with a
                // partially executed, error-terminated fan-out.
                let resolved =
                    crate::reconciliation::resolve_effective_meta_spec_set(&state, ws).await;
                let effective = match resolved {
                    Ok(set) => set,
                    Err(e) => {
                        tracing::warn!(
                            workspace_id = %ws.id,
                            error = %e,
                            "meta-spec approval fan-out: failed to resolve effective meta-spec set; skipping workspace (fail closed)"
                        );
                        continue;
                    }
                };
                let binds = effective.personas.values().any(|e| e.path == spec_path)
                    || effective.principles.iter().any(|e| e.path == spec_path)
                    || effective.standards.iter().any(|e| e.path == spec_path)
                    || effective.process.iter().any(|e| e.path == spec_path);
                if binds {
                    let summary = crate::reconciliation::run_reconciliation(
                        &state,
                        &ws.id,
                        std::slice::from_ref(&spec_path),
                    )
                    .await;
                    tracing::info!(
                        workspace_id = %ws.id,
                        meta_spec = %spec_path,
                        created = summary.tasks_created,
                        skipped = summary.tasks_skipped,
                        "meta-spec approval triggered reconciliation"
                    );
                }
            }
        }
    }

    Ok(Json(ms))
}

/// HSI §12 Judgment Ledger: publishing a meta-spec version (initial create or
/// version bump) is human judgment — record it so it surfaces in the
/// publisher's ledger. Failure propagates: a silently dropped judgment event
/// is audit theatre.
async fn record_meta_spec_publish(
    state: &AppState,
    auth: &AuthenticatedAgent,
    ms: &MetaSpec,
) -> Result<(), ApiError> {
    let workspace_id = match ms.scope {
        MetaSpecScope::Workspace => ms.scope_id.as_ref().map(Id::new),
        // Global and Tenant scopes are not single-workspace judgments: a
        // Global entry applies platform-wide, a Tenant entry (§8) fans out to
        // every workspace in its tenant — neither belongs to one workspace's
        // ledger page, so the judgment carries no workspace attribution and
        // the ledger's `?workspace_id=` filter (which excludes NULL) surfaces
        // it only in the unfiltered cross-workspace view.
        MetaSpecScope::Global | MetaSpecScope::Tenant => None,
    };
    let event = gyre_domain::AuditEvent::new(
        Id::new(uuid::Uuid::new_v4().to_string()),
        gyre_domain::AuditEventType::MetaSpecPublish,
        None,
        auth.user_id.clone(),
        None,
        workspace_id,
        None,
        "meta_spec".to_string(),
        Some(ms.id.as_str().to_string()),
        gyre_domain::AuditOutcome::Success,
        serde_json::json!({
            "kind": ms.kind.as_str(),
            "name": ms.name,
            "version": ms.version,
            "content_hash": ms.content_hash,
        }),
        None,
        None,
        now_secs(),
    );
    state
        .audit
        .record(&event)
        .await
        .map_err(ApiError::Internal)?;
    let _ = state
        .audit_broadcast_tx
        .send(serde_json::to_string(&event).unwrap_or_default());
    Ok(())
}

// ---------------------------------------------------------------------------
// DELETE /api/v1/meta-specs/:id
// ---------------------------------------------------------------------------

/// Per-handler authorization (agent-runtime §214 "Registry Levels": only
/// admins at the meta-spec's own scope level manage registry entries — tenant
/// admins for Global meta-specs, workspace Owner/Admin for Workspace-scoped
/// ones; a workspace admin cannot touch tenant meta-specs, which would be
/// privilege escalation). The route is ABAC-exempt, so this body is the only
/// authorization surface.
async fn authorize_meta_spec_delete(
    state: &AppState,
    auth: &AuthenticatedAgent,
    ms: &MetaSpec,
) -> Result<(), ApiError> {
    match ms.scope {
        MetaSpecScope::Global => {
            // Tenant-registry entry: tenant admin only.
            if !auth.roles.contains(&gyre_domain::UserRole::Admin) {
                crate::rbac::record_rbac_denied(
                    state,
                    auth,
                    "Admin",
                    "DELETE /api/v1/meta-specs/:id",
                )
                .await;
                return Err(ApiError::Forbidden(
                    "only Admin role may delete tenant-level (Global) meta-specs".to_string(),
                ));
            }
        }
        MetaSpecScope::Tenant => {
            // §8 tenant-scoped entry: an Admin of the scope tenant only —
            // the same containment rule as the tenant force flag (an admin
            // of another tenant must not delete, or learn of, this entry).
            // A missing scope_id fails closed: no caller's tenant scope
            // matches the empty string, so a dangling row is undeletable.
            let scope_tenant = ms.scope_id.as_deref().unwrap_or("");
            if !auth.roles.contains(&gyre_domain::UserRole::Admin) || auth.tenant_id != scope_tenant
            {
                crate::rbac::record_rbac_denied(
                    state,
                    auth,
                    "Admin",
                    "DELETE /api/v1/meta-specs/:id",
                )
                .await;
                return Err(ApiError::Forbidden(
                    "only an Admin of the scope tenant may delete tenant-scoped meta-specs"
                        .to_string(),
                ));
            }
        }
        MetaSpecScope::Workspace => {
            let scope_id = ms.scope_id.as_deref().unwrap_or_default();
            let ws_id = Id::new(scope_id);
            let ws = state
                .workspaces
                .find_by_id(&ws_id)
                .await
                .map_err(ApiError::Internal)?
                .ok_or_else(|| {
                    // Dangling scope reference — treat as not found rather
                    // than letting a stale row be deleted by an unscoped
                    // caller.
                    ApiError::NotFound(format!(
                        "meta-spec '{id}' references missing workspace '{scope_id}'",
                        id = ms.id.as_str()
                    ))
                })?;
            // Tenant containment first: a caller scoped to another tenant
            // must not even learn the meta-spec exists.
            if ws.tenant_id.as_str() != auth.tenant_id {
                crate::rbac::record_rbac_denied(
                    state,
                    auth,
                    "Admin",
                    "DELETE /api/v1/meta-specs/:id",
                )
                .await;
                return Err(ApiError::Forbidden(
                    "cross-tenant meta-spec access denied".to_string(),
                ));
            }
            // Workspace-registry entry: the caller must be an Owner/Admin
            // member of that workspace (or a tenant Admin bypass).
            let is_tenant_admin = auth.roles.contains(&gyre_domain::UserRole::Admin);
            let is_ws_admin = match &auth.user_id {
                Some(uid) => match state
                    .workspace_memberships
                    .find_by_user_and_workspace(uid, &ws_id)
                    .await
                    .map_err(ApiError::Internal)?
                {
                    Some(m) => {
                        matches!(
                            m.role,
                            gyre_domain::WorkspaceRole::Owner | gyre_domain::WorkspaceRole::Admin
                        )
                    }
                    None => false,
                },
                None => false,
            };
            if !is_tenant_admin && !is_ws_admin {
                crate::rbac::record_rbac_denied(
                    state,
                    auth,
                    "workspace Admin",
                    "DELETE /api/v1/meta-specs/:id",
                )
                .await;
                return Err(ApiError::Forbidden(
                    "only workspace Owner/Admin (or tenant Admin) may delete workspace meta-specs"
                        .to_string(),
                ));
            }
        }
    }
    Ok(())
}

pub async fn delete_meta_spec_registry(
    State(state): State<Arc<AppState>>,
    auth: AuthenticatedAgent,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    // No blanket Admin gate here (unlike POST/PUT): delete authorization is
    // scope-derived — `authorize_meta_spec_delete` decides per entry
    // (tenant Admin for Global/Tenant entries, workspace Owner/Admin
    // membership for Workspace entries), so a workspace Owner may delete
    // their workspace's entries without holding the tenant Admin role.
    // Load the entity, derive scope from the record (never the request), and
    // check tenant containment before deleting. The containment check for
    // an EXISTING row lives inside `authorize_meta_spec_delete` (per-scope,
    // cross-tenant → 403) — a leading `require_meta_spec_access` here would
    // return its raw NotFound for a missing row and leak existence to
    // unprivileged callers before the ordering below runs.
    let rid = Id::new(&id);
    // Authorization ordering: a missing row must not become an existence
    // oracle for unprivileged callers (403 precedes 404). A tenant Admin —
    // who may delete Global entries unconditionally — gets the truthful
    // 404; any other caller is denied without confirming existence, since
    // whether they may delete at all depends on the row's scope.
    let ms = state
        .meta_specs
        .get_by_id(&rid)
        .await
        .map_err(ApiError::Internal)?;
    match ms {
        Some(ms) => authorize_meta_spec_delete(&state, &auth, &ms).await?,
        None => {
            if !auth.roles.contains(&gyre_domain::UserRole::Admin) {
                crate::rbac::record_rbac_denied(
                    &state,
                    &auth,
                    "Admin",
                    "DELETE /api/v1/meta-specs/:id",
                )
                .await;
                return Err(ApiError::Forbidden("meta-spec access denied".to_string()));
            }
            return Err(ApiError::NotFound(format!("meta-spec '{id}' not found")));
        }
    }
    let has_bindings = state
        .meta_spec_bindings
        .has_bindings_for(&rid)
        .await
        .map_err(ApiError::Internal)?;
    if has_bindings {
        return Err(ApiError::Conflict(format!(
            "cannot delete meta-spec '{id}': active bindings reference it"
        )));
    }
    state
        .meta_specs
        .delete(&rid)
        .await
        .map_err(ApiError::Internal)?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// GET /api/v1/meta-specs/:id/versions
// ---------------------------------------------------------------------------

pub async fn list_meta_spec_versions(
    State(state): State<Arc<AppState>>,
    auth: AuthenticatedAgent,
    Path(id): Path<String>,
) -> Result<Json<Vec<gyre_domain::MetaSpecVersion>>, ApiError> {
    // Load the entity, derive its scope from the record, tenant-check it.
    require_meta_spec_access(&state, &auth, &id).await?;

    let versions = state
        .meta_specs
        .list_versions(&Id::new(&id))
        .await
        .map_err(ApiError::Internal)?;
    Ok(Json(versions))
}

// ---------------------------------------------------------------------------
// GET /api/v1/meta-specs/:id/versions/:version
// ---------------------------------------------------------------------------

pub async fn get_meta_spec_version(
    State(state): State<Arc<AppState>>,
    auth: AuthenticatedAgent,
    Path((id, version)): Path<(String, u32)>,
) -> Result<Json<gyre_domain::MetaSpecVersion>, ApiError> {
    // Load the entity, derive its scope from the record, tenant-check it.
    require_meta_spec_access(&state, &auth, &id).await?;
    let ver = state
        .meta_specs
        .get_version(&Id::new(&id), version)
        .await
        .map_err(ApiError::Internal)?
        .ok_or_else(|| {
            ApiError::NotFound(format!("version {version} of meta-spec '{id}' not found"))
        })?;
    Ok(Json(ver))
}

// ---------------------------------------------------------------------------
// Spec-level binding management (agent-runtime spec §2 Spec-Level Binding)
//
// PUT    /api/v1/specs/:path/meta-spec-bindings — replace the binding set
// GET    /api/v1/specs/:path/meta-spec-bindings — list current bindings
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct SetSpecBindingsRequest {
    /// The desired binding set. Missing meta-specs are unbound.
    pub bindings: Vec<SetSpecBindingEntry>,
}

#[derive(Deserialize)]
pub struct SetSpecBindingEntry {
    pub meta_spec_id: String,
    pub pinned_version: u32,
}

/// Replace the full binding set for a spec (declarative, mirrors the UI's
/// Meta-spec bindings panel which shows the whole selection at once).
///
/// Validation per spec: pinned version must exist in the meta-spec's history
/// (or be the current version), and the meta-spec must exist. Required
/// meta-specs need no binding (auto-injected); a binding for one is accepted
/// but redundant — assembly dedups it.
///
/// Authorization: bindings are spec metadata AND prompt configuration —
/// band 3 of `assemble_prompt_set` injects every bound meta-spec into every
/// agent spawned for tasks under this spec, regardless of the caller's
/// workspace. The gate therefore checks workspace membership, not just the
/// JWT role: a global Admin may bind anything; otherwise the caller must be
/// an Owner/Admin/Developer member of the workspace owning the spec (per its
/// ledger entry). A spec with no workspace scope is admin-only. Each bound
/// meta-spec must additionally be visible from that workspace: Global
/// meta-specs always qualify, Workspace-scoped ones only in their own
/// workspace.
pub async fn put_spec_meta_spec_bindings(
    State(state): State<Arc<AppState>>,
    auth: AuthenticatedAgent,
    Path(spec_path): Path<String>,
    Json(req): Json<SetSpecBindingsRequest>,
) -> Result<(StatusCode, Json<Vec<gyre_domain::MetaSpecBinding>>), ApiError> {
    let spec_ws: Option<String> = state
        .spec_ledger
        .find_by_path(&spec_path)
        .await
        .map_err(ApiError::Internal)?
        .and_then(|e| e.workspace_id);

    let is_global_admin = auth.roles.contains(&gyre_domain::UserRole::Admin);
    if !is_global_admin {
        let ws_id = spec_ws.as_deref().ok_or_else(|| {
            ApiError::Forbidden(
                "spec is not scoped to a workspace; only tenant Admin may change its meta-spec bindings"
                    .to_string(),
            )
        })?;
        // Agent tokens have no user identity — never workspace members.
        let user_id = auth.user_id.as_ref().ok_or_else(|| {
            ApiError::Forbidden("agent tokens cannot change spec meta-spec bindings".to_string())
        })?;
        let member = state
            .workspace_memberships
            .find_by_user_and_workspace(user_id, &Id::new(ws_id))
            .await
            .map_err(ApiError::Internal)?;
        match member.map(|m| m.role) {
            Some(gyre_domain::WorkspaceRole::Owner)
            | Some(gyre_domain::WorkspaceRole::Admin)
            | Some(gyre_domain::WorkspaceRole::Developer) => {}
            _ => {
                return Err(ApiError::Forbidden(format!(
                    "spec bindings require Developer membership in workspace '{ws_id}' (the workspace owning spec '{spec_path}')"
                )));
            }
        }
    }

    let now = now_secs();

    // Validate every entry before mutating anything.
    for entry in &req.bindings {
        let ms_id = Id::new(&entry.meta_spec_id);
        let ms = state
            .meta_specs
            .get_by_id(&ms_id)
            .await
            .map_err(ApiError::Internal)?
            .ok_or_else(|| {
                ApiError::BadRequest(format!("meta-spec '{}' not found", entry.meta_spec_id))
            })?;
        // Visibility: a Workspace-scoped meta-spec may only bind within
        // its own workspace. Band 3 resolves bindings by spec_path alone,
        // so binding a ws-A meta-spec to a ws-B (or unscoped) spec would
        // inject its prompt into every agent for that spec — a
        // cross-workspace prompt-injection leak.
        if ms.scope == MetaSpecScope::Workspace && ms.scope_id.as_deref() != spec_ws.as_deref() {
            let ms_ws = ms.scope_id.as_deref().unwrap_or("?");
            let spec_ws_desc = match spec_ws.as_deref() {
                Some(w) => format!("workspace '{w}'"),
                None => "no workspace".to_string(),
            };
            return Err(ApiError::BadRequest(format!(
                "meta-spec '{}' is scoped to workspace '{}' and cannot bind to spec '{}' ({})",
                entry.meta_spec_id, ms_ws, spec_path, spec_ws_desc
            )));
        }
        if entry.pinned_version > ms.version {
            return Err(ApiError::BadRequest(format!(
                "pinned version {} exceeds current version {} of meta-spec '{}'",
                entry.pinned_version, ms.version, entry.meta_spec_id
            )));
        }
        if entry.pinned_version != ms.version {
            let exists = state
                .meta_specs
                .get_version(&ms_id, entry.pinned_version)
                .await
                .map_err(ApiError::Internal)?
                .is_some();
            if !exists {
                return Err(ApiError::BadRequest(format!(
                    "version {} of meta-spec '{}' not found in history",
                    entry.pinned_version, entry.meta_spec_id
                )));
            }
        }
    }

    // Replace the binding set: delete existing, insert the new set.
    let existing = state
        .meta_spec_bindings
        .list_by_spec_id(&spec_path)
        .await
        .map_err(ApiError::Internal)?;
    for old in existing {
        state
            .meta_spec_bindings
            .delete(&old.id)
            .await
            .map_err(ApiError::Internal)?;
    }

    let mut created = Vec::with_capacity(req.bindings.len());
    for entry in req.bindings {
        let binding = gyre_domain::MetaSpecBinding {
            id: Id::new(uuid::Uuid::new_v4().to_string()),
            spec_id: spec_path.clone(),
            meta_spec_id: Id::new(&entry.meta_spec_id),
            pinned_version: entry.pinned_version,
            created_at: now,
        };
        state
            .meta_spec_bindings
            .create(&binding)
            .await
            .map_err(ApiError::Internal)?;
        created.push(binding);
    }

    // Pin updates clear the stale-pin dedup keys for this spec so a subsequent
    // drift is re-notified (the human acted on the alert; a new drift deserves
    // a new notification). Keys are `stale_pin:{spec_id}:{meta_spec_id}`.
    let prefix = format!("stale_pin:{}:", spec_path);
    if let Ok(keys) = state.kv_store.kv_list("meta_spec_stale_pins").await {
        for (key, _) in keys {
            if key.starts_with(&prefix) {
                let _ = state.kv_store.kv_remove("meta_spec_stale_pins", &key).await;
            }
        }
    }

    Ok((StatusCode::OK, Json(created)))
}

/// GET /api/v1/specs/:path/meta-spec-bindings — list the spec's bindings.
pub async fn get_spec_meta_spec_bindings(
    State(state): State<Arc<AppState>>,
    _auth: AuthenticatedAgent,
    Path(spec_path): Path<String>,
) -> Result<Json<Vec<gyre_domain::MetaSpecBinding>>, ApiError> {
    let bindings = state
        .meta_spec_bindings
        .list_by_spec_id(&spec_path)
        .await
        .map_err(ApiError::Internal)?;
    Ok(Json(bindings))
}

// ---------------------------------------------------------------------------
// Registry-level tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod registry_tests {
    use crate::mem::test_state;
    use axum::{body::Body, Router};
    use gyre_domain::Workspace;
    use http::{Request, StatusCode};
    use tower::ServiceExt;

    fn app() -> Router {
        let state = test_state();
        crate::api::api_router().with_state(state)
    }

    async fn body_json(resp: axum::response::Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null)
    }

    /// A non-admin caller must not create, update, or delete registry entries
    /// — the registry rewrites the rules agents operate under, and these
    /// routes run without middleware ABAC (frozen exemption list), so the
    /// handler is the only gate. Regression test for the gap flagged by
    /// check-abac-exempt-handlers.sh.
    #[tokio::test(flavor = "multi_thread")]
    async fn registry_mutations_require_admin() {
        let state = test_state();
        let app = crate::api::api_router().with_state(state.clone());

        // Agent-role identity: a real agent JWT (not the global token), so
        // roles = [Agent], tenant = "default".
        let agent_id = "reg-authz-agent".to_string();
        state
            .agents
            .create(&gyre_domain::Agent::new(
                gyre_common::Id::new(&agent_id),
                "reg-authz",
                0,
            ))
            .await
            .unwrap();
        let token = state
            .agent_signing_key
            .mint(
                &agent_id,
                "task-x",
                "system",
                &state.base_url,
                300,
                "default",
                "ws-1",
                "r-1",
                "repo",
            )
            .unwrap();
        state
            .kv_store
            .kv_set("agent_tokens", &agent_id, token.clone())
            .await
            .unwrap();

        let req = |method: &str, uri: &str, body: &str| {
            Request::builder()
                .method(method)
                .uri(uri)
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap()
        };

        let resp = app
            .clone()
            .oneshot(req(
                "POST",
                "/api/v1/meta-specs",
                r#"{"kind":"meta:persona","name":"sneak","scope":"Global","prompt":"x"}"#,
            ))
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::FORBIDDEN,
            "create must be admin-only"
        );

        let resp = app
            .clone()
            .oneshot(req(
                "PUT",
                "/api/v1/meta-specs/00000000-0000-0000-0000-000000000000",
                r#"{"prompt":"x"}"#,
            ))
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::FORBIDDEN,
            "update must be admin-only"
        );

        let resp = app
            .clone()
            .oneshot(req(
                "DELETE",
                "/api/v1/meta-specs/00000000-0000-0000-0000-000000000000",
                "",
            ))
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::FORBIDDEN,
            "delete must be admin-only"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn create_and_get_meta_spec() {
        let app = app();
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/meta-specs")
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"kind":"meta:persona","name":"test-worker","scope":"Global","prompt":"You are a worker."}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let json = body_json(resp).await;
        let id = json["id"].as_str().unwrap().to_string();
        assert_eq!(json["name"].as_str().unwrap(), "test-worker");
        assert_eq!(json["version"].as_u64().unwrap(), 1);
        assert!(json["content_hash"].as_str().is_some());

        // GET by id — reuse same app instance so the in-memory store is shared
        let get_resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/meta-specs/{id}"))
                    .header("authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(get_resp.status(), StatusCode::OK);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn list_meta_specs_registry_empty() {
        let resp = app()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/meta-specs")
                    .header("authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert!(json.as_array().unwrap().is_empty());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn update_bumps_version() {
        let app = app();
        // Create
        let create_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/meta-specs")
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"kind":"meta:principle","name":"conventional-commits","scope":"Global","prompt":"Use CC."}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(create_resp.status(), StatusCode::CREATED);
        let create_json = body_json(create_resp).await;
        let id = create_json["id"].as_str().unwrap().to_string();

        // Update
        let update_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/meta-specs/{id}"))
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"prompt":"Updated CC prompt."}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(update_resp.status(), StatusCode::OK);
        let update_json = body_json(update_resp).await;
        assert_eq!(update_json["version"].as_u64().unwrap(), 2);
    }

    // -- regression: `required` field must persist (task-156 repair) -------

    /// PUT {"required": true} on a registry entry must persist the flag.
    /// Regression for the silent no-op dropped in 7aa240c0: the UI's
    /// handleRequiredToggle sends exactly this body and toasts success.
    #[tokio::test(flavor = "multi_thread")]
    async fn update_required_field_persists() {
        let app = app();
        let create_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/meta-specs")
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"kind":"meta:persona","name":"req-worker","scope":"Global","prompt":"p"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(create_resp.status(), StatusCode::CREATED);
        let create_json = body_json(create_resp).await;
        let id = create_json["id"].as_str().unwrap().to_string();
        assert!(
            !create_json["required"].as_bool().unwrap(),
            "created entry defaults to required=false"
        );

        // Toggle required — the exact body handleRequiredToggle sends.
        let put_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/meta-specs/{id}"))
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"required":true}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(put_resp.status(), StatusCode::OK);
        let put_json = body_json(put_resp).await;
        assert!(
            put_json["required"].as_bool().unwrap(),
            "PUT required:true must be reflected in the response"
        );

        // Re-fetch: the flag must be persisted, not just echoed.
        let get_resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/meta-specs/{id}"))
                    .header("authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(get_resp.status(), StatusCode::OK);
        let get_json = body_json(get_resp).await;
        assert!(
            get_json["required"].as_bool().unwrap(),
            "GET after PUT required:true must show required=true"
        );
    }

    /// The two-step edit-then-approve lifecycle the shipped UI performs
    /// (saveEdit PUTs {prompt}, handleApprove PUTs {approval_status}) must
    /// trigger the §6 reconciliation. Keying the trigger on
    /// `req.prompt.is_some()` missed the approval-only request; the trigger
    /// now compares the content hash stamped at the last approval.
    #[tokio::test(flavor = "multi_thread")]
    async fn two_step_edit_then_approve_triggers_reconciliation() {
        use crate::mem::test_state;

        let state = test_state();
        let app: Router = crate::api::api_router().with_state(state.clone());

        // Workspace + repo: reconciliation needs scope.
        let ws_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/workspaces")
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"name": "ws-2step", "slug": "ws-2step"}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(ws_resp.status(), StatusCode::CREATED);
        let ws_id = body_json(ws_resp).await["id"].as_str().unwrap().to_string();

        let repo = gyre_domain::Repository::new(
            gyre_common::Id::new("repo-2step"),
            gyre_common::Id::new(&ws_id),
            "repo-2step".to_string(),
            "/tmp/does-not-matter/repo-2step".to_string(),
            1_700_000_000,
        );
        state.repos.create(&repo).await.unwrap();

        // Registry meta-spec, bound via the workspace set.
        let create_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/meta-specs")
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"kind":"meta:persona","name":"2step-worker","scope":"Global","prompt":"v1"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(create_resp.status(), StatusCode::CREATED);
        let id = body_json(create_resp).await["id"]
            .as_str()
            .unwrap()
            .to_string();

        let set_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/workspaces/{ws_id}/meta-spec-set"))
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "personas": { "worker": { "path": "2step-worker", "sha": "a1" } },
                            "principles": [], "standards": [], "process": []
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(set_resp.status(), StatusCode::OK);

        let recon_count = || {
            let state = state.clone();
            let ws_id = ws_id.clone();
            async move {
                state
                    .tasks
                    .list_by_workspace(&gyre_common::Id::new(&ws_id))
                    .await
                    .unwrap()
                    .into_iter()
                    .filter(|t| t.labels.iter().any(|l| l == "meta-spec-reconciliation"))
                    .count()
            }
        };

        // Binding a never-approved (Pending) spec must NOT trigger
        // reconciliation: §6 gates the rollout on approval, so the
        // approval transition owns the wave. A trigger here would
        // double-fire with the approval below (one logical rollout,
        // two reconciliation tasks).
        assert_eq!(
            recon_count().await,
            0,
            "binding a Pending (never-approved) meta-spec must not trigger reconciliation"
        );

        // Initial approval (Pending → Approved): first approval of content
        // never approved before is a trigger.
        let approve1 = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/meta-specs/{id}"))
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"approval_status":"Approved"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(approve1.status(), StatusCode::OK);
        assert_eq!(
            recon_count().await,
            1,
            "first approval of never-approved content must trigger reconciliation"
        );
        // The wave's task names the version of the APPROVED content (v1).
        // An approval-only PUT must not bump the version: it does not
        // change content, and a bump here would retitle the wave's tasks
        // ("v2") so the (workspace, repo, title) dedup could never
        // collapse adjacent triggers of one logical rollout.
        {
            let tasks: Vec<_> = state
                .tasks
                .list_by_workspace(&gyre_common::Id::new(&ws_id))
                .await
                .unwrap()
                .into_iter()
                .filter(|t| t.labels.iter().any(|l| l == "meta-spec-reconciliation"))
                .collect();
            assert_eq!(tasks.len(), 1);
            assert_eq!(
                tasks[0].title,
                "Align code with updated meta:persona 2step-worker v1",
                "approval-only PUT must not bump the version (title pins approved content's version)"
            );
        }

        // Terminal-complete wave 1 so the dedup set is empty again.
        let tasks: Vec<_> = state
            .tasks
            .list_by_workspace(&gyre_common::Id::new(&ws_id))
            .await
            .unwrap()
            .into_iter()
            .filter(|t| t.labels.iter().any(|l| l == "meta-spec-reconciliation"))
            .collect();
        for mut t in tasks {
            t.transition_status(gyre_domain::TaskStatus::InProgress)
                .unwrap();
            t.transition_status(gyre_domain::TaskStatus::Review)
                .unwrap();
            t.transition_status(gyre_domain::TaskStatus::Done).unwrap();
            state.tasks.update(&t).await.unwrap();
        }

        // Step 1 of the UI flow: saveEdit PUTs only {prompt}. Editing
        // content resets approval to Pending (§2).
        let edit = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/meta-specs/{id}"))
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"prompt":"v2"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(edit.status(), StatusCode::OK);
        let edit_json = body_json(edit).await;
        assert_eq!(
            edit_json["approval_status"].as_str().unwrap(),
            "Pending",
            "content edit must reset approval to Pending (§2)"
        );
        assert_eq!(
            recon_count().await,
            1,
            "content edit alone (Pending) must not trigger reconciliation"
        );

        // Step 2 of the UI flow: handleApprove PUTs only
        // {approval_status: Approved} — no prompt in the request. This is
        // the request the old trigger condition missed.
        let approve2 = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/meta-specs/{id}"))
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"approval_status":"Approved"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(approve2.status(), StatusCode::OK);
        assert_eq!(
            recon_count().await,
            2,
            "approval after content edit must trigger reconciliation (two-step UI flow)"
        );

        // Re-approving unchanged content must NOT trigger again.
        let tasks: Vec<_> = state
            .tasks
            .list_by_workspace(&gyre_common::Id::new(&ws_id))
            .await
            .unwrap()
            .into_iter()
            .filter(|t| t.labels.iter().any(|l| l == "meta-spec-reconciliation"))
            .collect();
        for mut t in tasks {
            if !matches!(
                t.status,
                gyre_domain::TaskStatus::Done | gyre_domain::TaskStatus::Cancelled
            ) {
                t.transition_status(gyre_domain::TaskStatus::InProgress)
                    .unwrap();
                t.transition_status(gyre_domain::TaskStatus::Review)
                    .unwrap();
                t.transition_status(gyre_domain::TaskStatus::Done).unwrap();
                state.tasks.update(&t).await.unwrap();
            }
        }
        let reapprove = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/meta-specs/{id}"))
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"approval_status":"Approved"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(reapprove.status(), StatusCode::OK);
        assert_eq!(
            recon_count().await,
            2,
            "re-approval of unchanged content must not trigger reconciliation"
        );
    }

    /// Review repair (fail-closed fan-out): a Global/Workspace-scoped
    /// approval whose per-workspace set resolution hits an unreadable
    /// (corrupt JSON / storage error) workspace set must SKIP that
    /// workspace with a warn log and keep the approval response 200 —
    /// the approval is already persisted at that point, so one corrupt
    /// set anywhere in the deployment must not 500 every approval while
    /// leaving a partially executed fan-out. Mirrors the tenant
    /// controller's fail-closed override gate
    /// (`tenant_reconciliation_fails_closed_on_unreadable_workspace_set`).
    /// Killing test for the old `?` propagation: under it the approval
    /// returned 500 (while persisted) and the healthy workspace listed
    /// after the corrupt one never received its reconciliation task.
    #[tokio::test(flavor = "multi_thread")]
    async fn global_spec_approval_skips_unreadable_workspace_set() {
        let state = test_state();
        let app: Router = crate::api::api_router().with_state(state.clone());

        // Corrupt workspace, created FIRST so the fan-out reaches it before
        // the healthy one (the strongest ordering for the old bug: the `?`
        // aborted the loop before the healthy workspace was scanned).
        let ws_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/workspaces")
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"name": "ws-corrupt", "slug": "ws-corrupt"}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(ws_resp.status(), StatusCode::CREATED);
        let corrupt_ws = body_json(ws_resp).await["id"].as_str().unwrap().to_string();
        // Corrupt stored set: resolution must neither decode-fail-open into
        // "binds nothing" nor abort the whole approval.
        state
            .meta_spec_sets
            .upsert(&gyre_common::Id::new(&corrupt_ws), "{not valid json")
            .await
            .unwrap();

        // Healthy workspace: set binds the spec; repo gives reconciliation
        // scope.
        let ws_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/workspaces")
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"name": "ws-ok", "slug": "ws-ok"}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(ws_resp.status(), StatusCode::CREATED);
        let ok_ws = body_json(ws_resp).await["id"].as_str().unwrap().to_string();
        let repo = gyre_domain::Repository::new(
            gyre_common::Id::new("repo-fc-ok"),
            gyre_common::Id::new(&ok_ws),
            "repo-fc-ok".to_string(),
            "/tmp/does-not-matter/repo-fc-ok".to_string(),
            1_700_000_000,
        );
        state.repos.create(&repo).await.unwrap();

        let create_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/meta-specs")
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"kind":"meta:persona","name":"fc-worker","scope":"Global","prompt":"v1"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(create_resp.status(), StatusCode::CREATED);
        let id = body_json(create_resp).await["id"]
            .as_str()
            .unwrap()
            .to_string();

        let set_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/workspaces/{ok_ws}/meta-spec-set"))
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "personas": { "worker": { "path": "fc-worker", "sha": "a1" } },
                            "principles": [], "standards": [], "process": []
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(set_resp.status(), StatusCode::OK);

        // Approve: must succeed (200) even though one workspace's set is
        // unreadable — the corrupt workspace is skipped, not propagated as
        // a 500 after the approval already persisted.
        let approve = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/meta-specs/{id}"))
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"approval_status":"Approved"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            approve.status(),
            StatusCode::OK,
            "approval must not 500 because an unrelated workspace has an unreadable set"
        );
        assert_eq!(
            body_json(approve).await["approval_status"].as_str(),
            Some("Approved")
        );
        // The approval really persisted.
        let stored = state
            .meta_specs
            .get_by_id(&gyre_common::Id::new(&id))
            .await
            .unwrap()
            .expect("approved meta-spec must be persisted");
        assert_eq!(
            stored.approval_status,
            gyre_domain::MetaSpecApprovalStatus::Approved
        );

        let recon_count = |ws: String| {
            let state = state.clone();
            async move {
                state
                    .tasks
                    .list_by_workspace(&gyre_common::Id::new(&ws))
                    .await
                    .unwrap()
                    .into_iter()
                    .filter(|t| t.labels.iter().any(|l| l == "meta-spec-reconciliation"))
                    .count()
            }
        };

        // The healthy workspace (listed after the corrupt one) still got its
        // reconciliation task: the skip continues the fan-out, it does not
        // abort it.
        assert_eq!(
            recon_count(ok_ws.clone()).await,
            1,
            "healthy workspace must still reconcile when another workspace's set is unreadable"
        );
        // The unreadable workspace was skipped, not reconciled on a
        // fail-open empty set.
        assert_eq!(
            recon_count(corrupt_ws).await,
            0,
            "workspace with unreadable set must be skipped, not reconciled"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn update_prompt_emits_meta_spec_changed_event() {
        let state = test_state();
        let app: Router = crate::api::api_router().with_state(state.clone());

        // Workspace whose set binds the spec (so the event has a persisted
        // destination) + repo so the §6 reconciliation side effect has scope.
        let ws_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/workspaces")
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"name": "ws-changed", "slug": "ws-changed"}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(ws_resp.status(), StatusCode::CREATED);
        let ws_json = body_json(ws_resp).await;
        let ws_id = ws_json["id"].as_str().unwrap().to_string();

        let set_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/workspaces/{ws_id}/meta-spec-set"))
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "personas": { "worker": { "path": "event-worker", "sha": "a1" } },
                            "principles": [], "standards": [], "process": []
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(set_resp.status(), StatusCode::OK);

        let create_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/meta-specs")
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"kind":"meta:persona","name":"event-worker","scope":"Global","prompt":"v1 prompt"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(create_resp.status(), StatusCode::CREATED);
        let create_json = body_json(create_resp).await;
        let id = create_json["id"].as_str().unwrap().to_string();

        // Content update → meta_spec_changed fanned out to the binding workspace.
        let update_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/meta-specs/{id}"))
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"prompt":"v2 prompt"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(update_resp.status(), StatusCode::OK);

        let msgs = state
            .messages
            .list_by_workspace(
                &gyre_common::Id::new(&ws_id),
                Some("meta_spec_changed"),
                None,
                None,
                None,
                Some(50),
            )
            .await
            .unwrap();
        assert_eq!(
            msgs.len(),
            1,
            "content update on a bound meta-spec must emit meta_spec_changed to the binding workspace"
        );
        let payload = msgs[0].payload.as_ref().unwrap();
        assert_eq!(payload["name"].as_str().unwrap(), "event-worker");
        assert_eq!(payload["version"].as_u64().unwrap(), 2);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn registry_endpoints_reject_non_admin() {
        // The registry routes are resolver-mapped in the ABAC middleware,
        // and the per-handler gates below are the role surface: a
        // Developer-role JWT must get 403 from every registry mutation and
        // from the list — governance configuration is Admin-only (docs:
        // "Admin only for PUT/DELETE"; list/create follow the same class).
        use crate::auth::test_helpers::{make_test_state_with_jwt, sign_test_jwt};
        let state = make_test_state_with_jwt();
        let router = crate::api::api_router().with_state(state);

        let dev_token = sign_test_jwt(
            &serde_json::json!({
                "tenant_id": "default",
                "sub": "dev-sub",
                "preferred_username": "developer-user",
                "realm_access": { "roles": ["developer"] }
            }),
            3600,
        );
        let auth = format!("Bearer {dev_token}");

        // LIST
        let resp = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/meta-specs")
                    .header("authorization", &auth)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);

        // CREATE
        let resp = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/meta-specs")
                    .header("authorization", &auth)
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"kind":"meta:principle","name":"x","scope":"Global","prompt":"p"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);

        // UPDATE + DELETE against a known id shape (403 must precede 404 —
        // the gate runs before the store lookup).
        let resp = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/v1/meta-specs/whatever")
                    .header("authorization", &auth)
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"prompt":"p2"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);

        let resp = router
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri("/api/v1/meta-specs/whatever")
                    .header("authorization", &auth)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn get_not_found_returns_404() {
        let resp = app()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/meta-specs/00000000-0000-0000-0000-000000000000")
                    .header("authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn delete_meta_spec() {
        let app = app();
        // Create
        let create_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/meta-specs")
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"kind":"meta:standard","name":"test-coverage","scope":"Global"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(create_resp.status(), StatusCode::CREATED);
        let id = body_json(create_resp).await["id"]
            .as_str()
            .unwrap()
            .to_string();

        // Delete
        let del_resp = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/api/v1/meta-specs/{id}"))
                    .header("authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(del_resp.status(), StatusCode::NO_CONTENT);
    }

    /// §2 + NEW-26: registry writes are scope-admin-gated, not just the
    /// `required` flag. An agent token (Agent role, no user identity) must
    /// get 403 on create, update (prompt rewrite), and delete — an agent
    /// rewriting registry prompts could rewrite the rules it and every
    /// other agent operate under.
    #[tokio::test(flavor = "multi_thread")]
    async fn required_gate_rejects_non_admin() {
        let state = test_state();
        // Register an agent token — resolves to UserRole::Agent, no user_id.
        state
            .kv_store
            .kv_set(
                "agent_tokens",
                "agt-required-gate",
                "agt-secret-rq".to_string(),
            )
            .await
            .unwrap();
        let app = crate::api::api_router().with_state(state);

        // Create (no `required` flag at all) → 403: the write itself is
        // governance, not just the flag.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/meta-specs")
                    .header("authorization", "Bearer agt-secret-rq")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"kind":"meta:persona","name":"p","scope":"Global","prompt":"x"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);

        // Create with required: true → 403.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/meta-specs")
                    .header("authorization", "Bearer agt-secret-rq")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"kind":"meta:persona","name":"p","scope":"Global","prompt":"x","required":true}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);

        // Admin creates one; agent then attempts prompt rewrite → 403.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/meta-specs")
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"kind":"meta:persona","name":"p2","scope":"Global","prompt":"x"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let id = body_json(resp).await["id"].as_str().unwrap().to_string();

        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/meta-specs/{id}"))
                    .header("authorization", "Bearer agt-secret-rq")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"prompt":"injected rules"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);

        // Agent attempts delete → 403.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/api/v1/meta-specs/{id}"))
                    .header("authorization", "Bearer agt-secret-rq")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);

        // Admin (global token) CAN set required.
        let resp = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/meta-specs/{id}"))
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"required":true}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(body_json(resp).await["required"], true);
    }

    /// §2 Spec-Level Binding: PUT replaces the binding set, GET lists it,
    /// invalid pins (version beyond current / nonexistent meta-spec) are 400.
    #[tokio::test(flavor = "multi_thread")]
    async fn spec_bindings_put_get_and_validation() {
        let state = test_state();
        let app = crate::api::api_router().with_state(state.clone());

        // Create a meta-spec (admin) and bump it to v2.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/meta-specs")
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"kind":"meta:standard","name":"binding-std","scope":"Global","prompt":"v1"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let ms_id = body_json(resp).await["id"].as_str().unwrap().to_string();
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/meta-specs/{ms_id}"))
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"prompt":"v2"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // Pin v1 (historical) — valid.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/v1/specs/system%2Fauth.md/meta-spec-bindings")
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(format!(
                        r#"{{"bindings":[{{"meta_spec_id":"{ms_id}","pinned_version":1}}]}}"#
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json.as_array().unwrap().len(), 1);
        assert_eq!(json[0]["pinned_version"], 1);

        // GET lists it.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/specs/system%2Fauth.md/meta-spec-bindings")
                    .header("authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json.as_array().unwrap().len(), 1);

        // Pin v99 (beyond current v2) → 400.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/v1/specs/system%2Fauth.md/meta-spec-bindings")
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(format!(
                        r#"{{"bindings":[{{"meta_spec_id":"{ms_id}","pinned_version":99}}]}}"#
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        // Nonexistent meta-spec → 400.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/v1/specs/system%2Fauth.md/meta-spec-bindings")
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"bindings":[{"meta_spec_id":"00000000-0000-0000-0000-000000000000","pinned_version":1}]}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        // Replace with empty set → unbinds (GET returns []).
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/v1/specs/system%2Fauth.md/meta-spec-bindings")
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"bindings":[]}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/specs/system%2Fauth.md/meta-spec-bindings")
                    .header("authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(body_json(resp).await.as_array().unwrap().len(), 0);
    }

    /// §2 Stale Pin Detection: a spec pinning an old version produces a
    /// priority-6 MetaSpecDrift notification for workspace admins; the
    /// dedup key prevents duplicate notifications on the next run.
    #[tokio::test(flavor = "multi_thread")]
    async fn stale_pin_detection_creates_notification() {
        use gyre_domain::meta_spec::{
            MetaSpec, MetaSpecApprovalStatus, MetaSpecBinding, MetaSpecKind, MetaSpecScope,
        };

        let state = test_state();

        // Workspace + admin member to notify.
        let ws = gyre_domain::Workspace::new(
            gyre_common::Id::new("ws-stale-pin"),
            gyre_common::Id::new("tenant-stale-pin"),
            "ws-stale-pin".to_string(),
            "Stale Pin WS".to_string(),
            1000,
        );
        state.workspaces.create(&ws).await.unwrap();
        let member = gyre_domain::WorkspaceMembership::new(
            gyre_common::Id::new("mem-1"),
            gyre_common::Id::new("user-stale-pin"),
            gyre_common::Id::new("ws-stale-pin"),
            gyre_domain::WorkspaceRole::Admin,
            gyre_common::Id::new("user-stale-pin"),
            1000,
        );
        state.workspace_memberships.create(&member).await.unwrap();

        // Spec ledger entry so the binding's spec resolves to the workspace.
        let entry = gyre_domain::SpecLedgerEntry {
            path: "system/stale.md".to_string(),
            title: "Stale".to_string(),
            owner: "system".to_string(),
            kind: None,
            current_sha: "abc".to_string(),
            approval_mode: "standard".to_string(),
            approval_status: gyre_domain::ApprovalStatus::Approved,
            linked_tasks: vec![],
            linked_mrs: vec![],
            drift_status: "clean".to_string(),
            created_at: 1000,
            updated_at: 1000,
            repo_id: None,
            workspace_id: Some("ws-stale-pin".to_string()),
        };
        state.spec_ledger.save(&entry).await.unwrap();

        // Meta-spec at v2, spec pinned at v1.
        let ms = MetaSpec {
            id: gyre_common::Id::new("ms-stale"),
            kind: MetaSpecKind::Standard,
            name: "drifting-std".to_string(),
            scope: MetaSpecScope::Global,
            scope_id: None,
            prompt: "v2 content".to_string(),
            version: 2,
            content_hash: "h2".to_string(),
            required: false,
            approval_status: MetaSpecApprovalStatus::Approved,
            approved_by: None,
            approved_content_hash: None,
            approved_at: None,
            created_by: "system".to_string(),
            created_at: 1000,
            updated_at: 1000,
        };
        state.meta_specs.create(&ms).await.unwrap();
        state
            .meta_spec_bindings
            .create(&MetaSpecBinding {
                id: gyre_common::Id::new("bind-stale"),
                spec_id: "system/stale.md".to_string(),
                meta_spec_id: ms.id.clone(),
                pinned_version: 1,
                created_at: 1000,
            })
            .await
            .unwrap();

        // Run detection.
        crate::prompt_assembly::detect_stale_pins(&state)
            .await
            .unwrap();

        // Notification created for the workspace admin.
        let ws_id = gyre_common::Id::new("ws-stale-pin");
        let user_id = gyre_common::Id::new("user-stale-pin");
        let notifs = state
            .notifications
            .list_for_user(&user_id, Some(&ws_id), None, None, None, None, &[], 50, 0)
            .await
            .unwrap();
        let drift: Vec<_> = notifs
            .iter()
            .filter(|n| n.notification_type == gyre_common::NotificationType::MetaSpecDrift)
            .collect();
        assert_eq!(drift.len(), 1, "expected exactly one drift notification");
        assert_eq!(drift[0].priority_rank, 6);
        assert!(drift[0].title.contains("system/stale.md"));
        assert!(drift[0].title.contains("v1"));
        assert!(drift[0].title.contains("v2"));

        // Second run: dedup — no additional notification.
        crate::prompt_assembly::detect_stale_pins(&state)
            .await
            .unwrap();
        let notifs2 = state
            .notifications
            .list_for_user(&user_id, Some(&ws_id), None, None, None, None, &[], 50, 0)
            .await
            .unwrap();
        let drift2: Vec<_> = notifs2
            .iter()
            .filter(|n| n.notification_type == gyre_common::NotificationType::MetaSpecDrift)
            .collect();
        assert_eq!(
            drift2.len(),
            1,
            "dedup key must prevent duplicate notifications"
        );
    }

    /// Review round 6 finding: `put_spec_meta_spec_bindings` previously
    /// authorized only on the JWT role, with no check that the caller has
    /// membership in the workspace owning the target spec. A Developer-role
    /// JWT with no membership anywhere could bind a Global meta-spec to a
    /// spec owned by another workspace — and assembly band 3 injects bound
    /// prompts into every agent spawned for tasks under that spec. This test
    /// pins the membership gate: non-member Developer → 403, member
    /// Developer (of the owning workspace) → 200, agent token → 403.
    #[tokio::test(flavor = "multi_thread")]
    async fn spec_bindings_require_membership_in_owning_workspace() {
        use crate::auth::test_helpers::{make_test_state_with_jwt, sign_test_jwt};
        use gyre_domain::meta_spec::{
            MetaSpec, MetaSpecApprovalStatus, MetaSpecKind, MetaSpecScope,
        };

        let state = make_test_state_with_jwt();

        // Workspace owning the victim spec.
        let victim_ws = gyre_domain::Workspace::new(
            gyre_common::Id::new("ws-bind-victim"),
            gyre_common::Id::new("tenant-bind"),
            "Victim WS".to_string(),
            "bind-victim-slug".to_string(),
            1000,
        );
        state.workspaces.create(&victim_ws).await.unwrap();

        // Spec ledger entry scoped to the victim workspace.
        let entry = gyre_domain::SpecLedgerEntry {
            path: "system/victim-bind.md".to_string(),
            title: "Victim".to_string(),
            owner: "system".to_string(),
            kind: None,
            current_sha: "abc".to_string(),
            approval_mode: "standard".to_string(),
            approval_status: gyre_domain::ApprovalStatus::Approved,
            linked_tasks: vec![],
            linked_mrs: vec![],
            drift_status: "clean".to_string(),
            created_at: 1000,
            updated_at: 1000,
            repo_id: None,
            workspace_id: Some("ws-bind-victim".to_string()),
        };
        state.spec_ledger.save(&entry).await.unwrap();

        // Global meta-spec (bindable from any workspace per visibility rule).
        let ms = MetaSpec {
            id: gyre_common::Id::new("ms-bind-target"),
            kind: MetaSpecKind::Standard,
            name: "bindable-std".to_string(),
            scope: MetaSpecScope::Global,
            scope_id: None,
            prompt: "bindable content".to_string(),
            version: 1,
            content_hash: "h1".to_string(),
            required: false,
            approval_status: MetaSpecApprovalStatus::Approved,
            approved_by: None,
            approved_content_hash: None,
            approved_at: None,
            created_by: "system".to_string(),
            created_at: 1000,
            updated_at: 1000,
        };
        state.meta_specs.create(&ms).await.unwrap();

        // Attacker: Developer role, member of NO workspace.
        let mut attacker = gyre_domain::User::new(
            gyre_common::Id::new("user-bind-attacker"),
            "bind-attacker-sub",
            "bind-attacker".to_string(),
            1000,
        );
        attacker.roles = vec![gyre_domain::UserRole::Developer];
        state.users.create(&attacker).await.unwrap();

        let claims = serde_json::json!({
            "sub": "bind-attacker-sub",
            "preferred_username": "bind-attacker",
            "email": "bind-attacker@example.com",
            "realm_access": { "roles": ["developer"] }
        });
        let attacker_token = sign_test_jwt(&claims, 3600);

        let body = format!(
            r#"{{"bindings":[{{"meta_spec_id":"{}","pinned_version":1}}]}}"#,
            ms.id
        );
        let uri = "/api/v1/specs/system%2Fvictim-bind.md/meta-spec-bindings";

        let app = crate::api::api_router().with_state(state.clone());

        // Non-member Developer (role alone) → 403.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(uri)
                    .header("authorization", format!("Bearer {attacker_token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(body.clone()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::FORBIDDEN,
            "Developer role alone must not bind meta-specs to another workspace's spec"
        );

        // Agent token (no user identity) → 403.
        state
            .kv_store
            .kv_set("agent_tokens", "agt-bind", "agt-bind-secret".to_string())
            .await
            .unwrap();
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(uri)
                    .header("authorization", "Bearer agt-bind-secret")
                    .header("content-type", "application/json")
                    .body(Body::from(body.clone()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::FORBIDDEN,
            "agent tokens cannot change spec meta-spec bindings"
        );

        // Member Developer of the owning workspace → 200.
        state
            .workspace_memberships
            .create(&gyre_domain::WorkspaceMembership::new(
                gyre_common::Id::new("mem-bind-member"),
                gyre_common::Id::new("user-bind-member"),
                gyre_common::Id::new("ws-bind-victim"),
                gyre_domain::WorkspaceRole::Developer,
                gyre_common::Id::new("user-bind-member"),
                1000,
            ))
            .await
            .unwrap();
        let mut member = gyre_domain::User::new(
            gyre_common::Id::new("user-bind-member"),
            "bind-member-sub",
            "bind-member".to_string(),
            1000,
        );
        member.roles = vec![gyre_domain::UserRole::Developer];
        state.users.create(&member).await.unwrap();
        let claims = serde_json::json!({
            "sub": "bind-member-sub",
            "preferred_username": "bind-member",
            "email": "bind-member@example.com",
            "realm_access": { "roles": ["developer"] }
        });
        let member_token = sign_test_jwt(&claims, 3600);
        let resp = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(uri)
                    .header("authorization", format!("Bearer {member_token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "Developer member of the owning workspace must be able to set bindings"
        );
    }

    // -- per-handler authorization (ABAC-exempt routes) ---------------------

    /// Agent-role tokens must not write registry entries or approve content:
    /// meta-spec approval is human-only (meta-spec-reconciliation.md §1) and
    /// an approved content change is the §6 reconciliation trigger.
    #[tokio::test(flavor = "multi_thread")]
    async fn registry_writes_reject_non_admin() {
        use crate::auth::test_helpers::{make_test_state_with_jwt, sign_test_jwt};

        let state = make_test_state_with_jwt();
        let app: Router = crate::api::api_router().with_state(state.clone());

        // Developer-role OIDC JWT (no Admin role).
        let dev_token = sign_test_jwt(
            &serde_json::json!({
                "tenant_id": "default",
                "sub": "dev-registry",
                "preferred_username": "dev-registry",
                "realm_access": { "roles": ["developer"] }
            }),
            3600,
        );

        let authz = |method: &str, uri: String, token: &str, body: Option<&str>| {
            let app = app.clone();
            let method = method.to_string();
            let token = token.to_string();
            let body = body.map(|b| b.to_string());
            async move {
                let mut builder = Request::builder()
                    .method(method.as_str())
                    .uri(uri)
                    .header("authorization", format!("Bearer {token}"));
                if body.is_some() {
                    builder = builder.header("content-type", "application/json");
                }
                app.oneshot(builder.body(Body::from(body.unwrap_or_default())).unwrap())
                    .await
                    .unwrap()
                    .status()
            }
        };

        // Create as admin so a target exists for update/delete attempts.
        let created = authz(
            "POST",
            "/api/v1/meta-specs".to_string(),
            "test-token",
            Some(r#"{"kind":"meta:persona","name":"authz-worker","scope":"Global","prompt":"p"}"#),
        )
        .await;
        assert_eq!(created, StatusCode::CREATED);
        let all = state
            .meta_specs
            .list(&gyre_ports::MetaSpecFilter::default())
            .await
            .unwrap();
        let id = all
            .iter()
            .find(|ms| ms.name == "authz-worker")
            .expect("created meta-spec must exist")
            .id
            .to_string();

        // Non-admin create → 403.
        assert_eq!(
            authz(
                "POST",
                "/api/v1/meta-specs".to_string(),
                &dev_token,
                Some(r#"{"kind":"meta:standard","name":"forged","scope":"Global","prompt":"p"}"#),
            )
            .await,
            StatusCode::FORBIDDEN,
            "non-Admin create must be Forbidden"
        );

        // Non-admin approve (the §6 trigger) → 403, approval unchanged.
        assert_eq!(
            authz(
                "PUT",
                format!("/api/v1/meta-specs/{id}"),
                &dev_token,
                Some(r#"{"approval_status":"Approved"}"#),
            )
            .await,
            StatusCode::FORBIDDEN,
            "non-Admin approval must be Forbidden"
        );
        let ms = state
            .meta_specs
            .get_by_id(&gyre_common::Id::new(&id))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            ms.approval_status,
            gyre_domain::MetaSpecApprovalStatus::Pending
        );
        assert!(
            ms.approved_by.is_none(),
            "rejected approval must not record an approver"
        );

        // Non-admin delete → 403.
        assert_eq!(
            authz(
                "DELETE",
                format!("/api/v1/meta-specs/{id}"),
                &dev_token,
                None,
            )
            .await,
            StatusCode::FORBIDDEN,
            "non-Admin delete must be Forbidden"
        );

        // Non-admin workspace-scoped list of a workspace they are not in
        // → 403 (Global list stays authenticated-readable).
        let ws_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/workspaces")
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"name": "ws-authz", "slug": "ws-authz"}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(ws_resp.status(), StatusCode::CREATED);
        let ws_json = body_json(ws_resp).await;
        let ws_id = ws_json["id"].as_str().unwrap().to_string();
        assert_eq!(
            authz(
                "GET",
                format!("/api/v1/meta-specs?scope=Workspace&scope_id={ws_id}"),
                &dev_token,
                None,
            )
            .await,
            StatusCode::FORBIDDEN,
            "workspace-scoped list must require membership"
        );

        // Admin path still works end-to-end: approve as admin.
        assert_eq!(
            authz(
                "PUT",
                format!("/api/v1/meta-specs/{id}"),
                "test-token",
                Some(r#"{"approval_status":"Approved"}"#),
            )
            .await,
            StatusCode::OK
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn publish_writes_meta_spec_publish_audit_event() {
        use crate::auth::test_helpers::{make_test_state_with_jwt, sign_test_jwt};
        use gyre_ports::AuditQueryFilter;

        let state = make_test_state_with_jwt();
        let app = crate::api::api_router().with_state(state.clone());

        // The Workspace-scoped create validates scope containment against
        // the caller's tenant (scope_id must resolve to a real workspace
        // row), so the target workspace is seeded before the request.
        state
            .workspaces
            .create(&Workspace::new(
                gyre_common::Id::new("ws-pub-1"),
                gyre_common::Id::new("default"),
                "Publish WS",
                "publish-ws",
                0,
            ))
            .await
            .unwrap();

        // A human (OIDC JWT) publishes a Workspace-scoped meta-spec.
        let jwt = sign_test_jwt(
            &serde_json::json!({
                "sub": "ms-pub-sub",
                "preferred_username": "ms-publisher",
                "realm_access": { "roles": ["admin"] }
            }),
            3600,
        );
        let create_body = serde_json::json!({
            "kind": "meta:principle",
            "name": "conventional-commits",
            "scope": "Workspace",
            "scope_id": "ws-pub-1",
            "prompt": "Use CC."
        });
        let create_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/meta-specs")
                    .header("authorization", format!("Bearer {jwt}"))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&create_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(create_resp.status(), StatusCode::CREATED);
        let id = body_json(create_resp).await["id"]
            .as_str()
            .unwrap()
            .to_string();

        let events = state
            .audit
            .query(&AuditQueryFilter {
                event_type: Some("meta_spec_publish".to_string()),
                limit: 100,
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(events.len(), 1, "create must record one publish event");
        let ev = &events[0];
        let acting_user = state
            .users
            .find_by_external_id("ms-pub-sub")
            .await
            .unwrap()
            .expect("JWT auth must provision the acting user");
        assert_eq!(
            ev.user_id.as_ref().map(|u| u.as_str()),
            Some(acting_user.id.as_str())
        );
        assert_eq!(ev.resource_id.as_deref(), Some(id.as_str()));
        // Workspace attribution from the meta-spec's own scope.
        assert_eq!(
            ev.workspace_id.as_ref().map(|w| w.as_str()),
            Some("ws-pub-1")
        );
        assert_eq!(ev.detail["kind"], "meta:principle");
        assert_eq!(ev.detail["name"], "conventional-commits");
        assert_eq!(ev.detail["version"], 1);
        assert!(
            ev.detail["content_hash"].is_string(),
            "content_hash required"
        );

        // Version bump (update) records a second publish event.
        let update_resp = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/meta-specs/{id}"))
                    .header("authorization", format!("Bearer {jwt}"))
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"prompt":"Updated."}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(update_resp.status(), StatusCode::OK);
        let events2 = state
            .audit
            .query(&AuditQueryFilter {
                event_type: Some("meta_spec_publish".to_string()),
                limit: 100,
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(
            events2.len(),
            2,
            "update must record a second publish event"
        );
        // Both events may share the same wall-clock second, so assert the
        // version set rather than a specific order.
        let versions: Vec<u64> = events2
            .iter()
            .map(|e| e.detail["version"].as_u64().unwrap_or(0))
            .collect();
        assert!(
            versions.contains(&1) && versions.contains(&2),
            "versions: {versions:?}"
        );
        let ledger = crate::api::api_router()
            .with_state(state)
            .oneshot(
                Request::builder()
                    .uri("/api/v1/users/me/judgments?type=meta-spec")
                    .header("authorization", format!("Bearer {jwt}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(ledger.status(), StatusCode::OK);
        let ledger_json = body_json(ledger).await;
        let items = ledger_json["judgments"].as_array().unwrap();
        assert_eq!(
            items.len(),
            2,
            "ledger must surface both publishes: {items:?}"
        );
        assert_eq!(items[0]["judgment_type"], "meta-spec");
        assert_eq!(items[0]["entity_ref"], "conventional-commits");
        assert_eq!(items[0]["workspace_id"], "ws-pub-1");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn delete_requires_admin_for_global_meta_specs() {
        use crate::auth::test_helpers::{make_test_state_with_jwt, sign_test_jwt};

        let state = make_test_state_with_jwt();
        let app = crate::api::api_router().with_state(state.clone());
        // Create a Global meta-spec as the system admin.
        let create_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/meta-specs")
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"kind":"meta:persona","name":"global-p","scope":"Global","prompt":"p"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(create_resp.status(), StatusCode::CREATED);
        let id = body_json(create_resp).await["id"]
            .as_str()
            .unwrap()
            .to_string();

        // A non-admin human (developer role) must be forbidden.
        let dev_jwt = sign_test_jwt(
            &serde_json::json!({
                "sub": "del-dev-sub",
                "preferred_username": "del-dev",
                "realm_access": { "roles": ["developer"] }
            }),
            3600,
        );
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/api/v1/meta-specs/{id}"))
                    .header("authorization", format!("Bearer {dev_jwt}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);

        // The meta-spec must still exist (forbidden ≠ delete).
        let get_resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/meta-specs/{id}"))
                    .header("authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(get_resp.status(), StatusCode::OK);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn delete_tenant_scoped_meta_spec_requires_scope_tenant_admin() {
        use crate::auth::test_helpers::{make_test_state_with_jwt, sign_test_jwt};

        let state = make_test_state_with_jwt();
        let app = crate::api::api_router().with_state(state.clone());

        // A second tenant: the scope the meta-spec is pinned to. The system
        // admin's own tenant is "default", deliberately different.
        let other_tenant = gyre_domain::Tenant::new(
            gyre_common::Id::new("tenant-del-other"),
            "OtherDel",
            "other-del",
            0,
        );
        state.tenants.create(&other_tenant).await.unwrap();

        // Create a Tenant-scoped meta-spec as an Admin of the SCOPE
        // tenant (the scope-level write grant: only the scope tenant's
        // admins may create tenant-scoped entries — the system admin of
        // tenant "default" is cross-tenant here and would be rejected).
        let scope_admin_create_jwt = sign_test_jwt(
            &serde_json::json!({
                "sub": "del-create-admin-sub",
                "preferred_username": "del-create-admin",
                "tenant_id": "tenant-del-other",
                "realm_access": { "roles": ["admin"] }
            }),
            3600,
        );
        let create_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/meta-specs")
                    .header("authorization", format!("Bearer {scope_admin_create_jwt}"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "kind": "meta:persona", "name": "tenant-p",
                            "scope": "Tenant", "scope_id": "tenant-del-other"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(create_resp.status(), StatusCode::CREATED);
        let id = body_json(create_resp).await["id"]
            .as_str()
            .unwrap()
            .to_string();

        // A non-admin human of the scope tenant must be forbidden.
        let dev_jwt = sign_test_jwt(
            &serde_json::json!({
                "sub": "del-tenant-dev-sub",
                "preferred_username": "del-tenant-dev",
                "tenant_id": "tenant-del-other",
                "realm_access": { "roles": ["developer"] }
            }),
            3600,
        );
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/api/v1/meta-specs/{id}"))
                    .header("authorization", format!("Bearer {dev_jwt}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);

        // An Admin of a DIFFERENT tenant ("default") must be forbidden — the
        // §8 containment rule: only the scope tenant's admin may delete.
        let cross_admin_jwt = sign_test_jwt(
            &serde_json::json!({
                "sub": "del-tenant-admin-sub",
                "preferred_username": "del-tenant-admin",
                "tenant_id": "default",
                "realm_access": { "roles": ["admin"] }
            }),
            3600,
        );
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/api/v1/meta-specs/{id}"))
                    .header("authorization", format!("Bearer {cross_admin_jwt}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);

        // Still exists (forbidden ≠ delete).
        let get_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/meta-specs/{id}"))
                    .header("authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(get_resp.status(), StatusCode::OK);

        // An Admin of the scope tenant may delete, and the delete takes effect.
        let scope_admin_jwt = sign_test_jwt(
            &serde_json::json!({
                "sub": "del-scope-admin-sub",
                "preferred_username": "del-scope-admin",
                "tenant_id": "tenant-del-other",
                "realm_access": { "roles": ["admin"] }
            }),
            3600,
        );
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/api/v1/meta-specs/{id}"))
                    .header("authorization", format!("Bearer {scope_admin_jwt}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);

        let get_resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/meta-specs/{id}"))
                    .header("authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(get_resp.status(), StatusCode::NOT_FOUND);
    }

    /// §8 containment on the registry write gate: an Admin of tenant A
    /// must not PUT (any field, including `required`) or POST
    /// workspace-scoped meta-specs inside tenant B's workspaces — the
    /// tenant-Admin bypass in `authorize_registry_write` is bounded by the
    /// workspace's own tenant, mirroring the cross-admin DELETE rule
    /// (`delete_tenant_scoped_meta_spec_requires_scope_tenant_admin` and
    /// `authorize_meta_spec_delete`). Without the bound, flipping
    /// `required` would inject that prompt into every agent tenant B
    /// spawns in the workspace (prompt_assembly required-workspace band).
    #[tokio::test(flavor = "multi_thread")]
    async fn cross_tenant_admin_cannot_write_workspace_meta_spec() {
        use crate::auth::test_helpers::{make_test_state_with_jwt, sign_test_jwt};
        use gyre_domain::{Tenant, Workspace};

        let state = make_test_state_with_jwt();
        let app = crate::api::api_router().with_state(state.clone());

        // A second tenant with a workspace: the scope the meta-spec is
        // pinned to. The system admin's own tenant is "default",
        // deliberately different.
        let other_tenant = Tenant::new(
            gyre_common::Id::new("tenant-wr-other"),
            "OtherWr",
            "other-wr",
            0,
        );
        state.tenants.create(&other_tenant).await.unwrap();
        let ws = Workspace::new(
            gyre_common::Id::new("ws-wr-1"),
            gyre_common::Id::new("tenant-wr-other"),
            "WrWs",
            "wr-ws",
            0,
        );
        state.workspaces.create(&ws).await.unwrap();

        // An Admin of the SCOPE tenant creates a Workspace-scoped
        // meta-spec (the scope-level write grant: the scope tenant's
        // admins may write its workspaces' entries).
        let scope_admin_jwt = sign_test_jwt(
            &serde_json::json!({
                "sub": "wr-scope-admin-sub",
                "preferred_username": "wr-scope-admin",
                "tenant_id": "tenant-wr-other",
                "realm_access": { "roles": ["admin"] }
            }),
            3600,
        );
        let create_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/meta-specs")
                    .header("authorization", format!("Bearer {scope_admin_jwt}"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "kind": "meta:persona", "name": "wr-persona",
                            "scope": "Workspace", "scope_id": "ws-wr-1",
                            "prompt": "p"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(create_resp.status(), StatusCode::CREATED);
        let id = body_json(create_resp).await["id"]
            .as_str()
            .unwrap()
            .to_string();

        // An Admin of a DIFFERENT tenant ("default", the system admin)
        // PUTs required=true on the workspace meta-spec — must be 403 and
        // must not flip the flag. (require_registry_scope's Admin bypass
        // lets the request through containment; the write gate itself
        // must bound the bypass.)
        let put_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/meta-specs/{id}"))
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"required": true}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            put_resp.status(),
            StatusCode::FORBIDDEN,
            "an Admin of another tenant must not write workspace meta-specs"
        );
        let ms = state
            .meta_specs
            .get_by_id(&gyre_common::Id::new(&id))
            .await
            .unwrap()
            .unwrap();
        assert!(!ms.required, "the rejected PUT must not flip required");

        // The same cross-tenant Admin cannot POST a new workspace-scoped
        // entry into the other tenant's workspace either.
        let post_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/meta-specs")
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "kind": "meta:persona", "name": "wr-sneak",
                            "scope": "Workspace", "scope_id": "ws-wr-1",
                            "prompt": "p"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            post_resp.status(),
            StatusCode::FORBIDDEN,
            "an Admin of another tenant must not create workspace meta-specs"
        );

        // Positive control — the scope tenant's Admin PUT still succeeds:
        // the denials above are the cross-tenant bound, not removal of the
        // Admin bypass.
        let put_resp = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/meta-specs/{id}"))
                    .header("authorization", format!("Bearer {scope_admin_jwt}"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"required": true}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            put_resp.status(),
            StatusCode::OK,
            "an Admin of the workspace's own tenant must still write it"
        );
        let ms = state
            .meta_specs
            .get_by_id(&gyre_common::Id::new(&id))
            .await
            .unwrap()
            .unwrap();
        assert!(ms.required, "the scope-tenant Admin PUT must flip required");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn delete_workspace_meta_spec_scopes_to_membership() {
        use crate::auth::test_helpers::{make_test_state_with_jwt, sign_test_jwt};
        use gyre_domain::{Workspace, WorkspaceMembership, WorkspaceRole};

        let state = make_test_state_with_jwt();
        let app = crate::api::api_router().with_state(state.clone());

        // Seed a workspace in the caller's tenant ("default" for API-key/JWT
        // auth without a tenant_id claim).
        let ws = Workspace::new(
            gyre_common::Id::new("ws-del-1"),
            gyre_common::Id::new("default"),
            "DelWs",
            "del-ws",
            0,
        );
        state.workspaces.create(&ws).await.unwrap();

        // Create a Workspace-scoped meta-spec as the system admin.
        let create_body = serde_json::json!({
            "kind": "meta:standard",
            "name": "ws-only-standard",
            "scope": "Workspace",
            "scope_id": "ws-del-1",
            "prompt": "s"
        });
        let create_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/meta-specs")
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&create_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(create_resp.status(), StatusCode::CREATED);
        let id = body_json(create_resp).await["id"]
            .as_str()
            .unwrap()
            .to_string();

        // A developer who IS a workspace member — still forbidden (member,
        // not workspace admin).
        let member_jwt = sign_test_jwt(
            &serde_json::json!({
                "sub": "del-member-sub",
                "preferred_username": "del-member",
                "realm_access": { "roles": ["developer"] }
            }),
            3600,
        );
        let member_user = gyre_domain::User::new(
            gyre_common::Id::new("user-del-member"),
            "del-member-sub",
            "del-member",
            0,
        );
        state.users.create(&member_user).await.unwrap();
        state
            .workspace_memberships
            .create(&WorkspaceMembership::new(
                gyre_common::Id::new("m-del-1"),
                member_user.id.clone(),
                gyre_common::Id::new("ws-del-1"),
                WorkspaceRole::Developer,
                gyre_common::Id::new("admin"),
                0,
            ))
            .await
            .unwrap();
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/api/v1/meta-specs/{id}"))
                    .header("authorization", format!("Bearer {member_jwt}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);

        // A developer with NO membership at all — forbidden (not a member).
        let outsider_jwt = sign_test_jwt(
            &serde_json::json!({
                "sub": "del-outsider-sub",
                "preferred_username": "del-outsider",
                "realm_access": { "roles": ["developer"] }
            }),
            3600,
        );
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/api/v1/meta-specs/{id}"))
                    .header("authorization", format!("Bearer {outsider_jwt}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);

        // A workspace Owner member — allowed, and the delete takes effect.
        let owner_jwt = sign_test_jwt(
            &serde_json::json!({
                "sub": "del-owner-sub",
                "preferred_username": "del-owner",
                "realm_access": { "roles": ["developer"] }
            }),
            3600,
        );
        let owner_user = gyre_domain::User::new(
            gyre_common::Id::new("user-del-owner"),
            "del-owner-sub",
            "del-owner",
            0,
        );
        state.users.create(&owner_user).await.unwrap();
        state
            .workspace_memberships
            .create(&WorkspaceMembership::new(
                gyre_common::Id::new("m-del-2"),
                owner_user.id.clone(),
                gyre_common::Id::new("ws-del-1"),
                WorkspaceRole::Owner,
                gyre_common::Id::new("admin"),
                0,
            ))
            .await
            .unwrap();
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/api/v1/meta-specs/{id}"))
                    .header("authorization", format!("Bearer {owner_jwt}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);

        // Gone.
        let get_resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/meta-specs/{id}"))
                    .header("authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(get_resp.status(), StatusCode::NOT_FOUND);
    }

    /// §2 Required vs Optional + §214 Registry Levels: a workspace Owner
    /// (NOT a tenant Admin) may PUT `required: true` on a Workspace-scoped
    /// meta-spec — the specced workspace-admin authority that the old
    /// tenant-Admin-for-every-scope gate denied. The flag persists and the
    /// meta-spec is injected into the required-workspace band for agents
    /// spawned in that workspace (prompt_assembly band 2).
    #[tokio::test(flavor = "multi_thread")]
    async fn workspace_owner_sets_required_on_workspace_meta_spec() {
        use crate::auth::test_helpers::{make_test_state_with_jwt, sign_test_jwt};
        use gyre_domain::{Workspace, WorkspaceMembership, WorkspaceRole};

        let state = make_test_state_with_jwt();
        let app = crate::api::api_router().with_state(state.clone());

        // Workspace in the caller's tenant ("default" for JWT auth without
        // a tenant_id claim).
        let ws = Workspace::new(
            gyre_common::Id::new("ws-req-1"),
            gyre_common::Id::new("default"),
            "ReqWs",
            "req-ws",
            0,
        );
        state.workspaces.create(&ws).await.unwrap();

        // Admin creates a Workspace-scoped meta:persona and approves it
        // (injection only serves Approved entries).
        let create_body = serde_json::json!({
            "kind": "meta:persona",
            "name": "ws-owner-persona",
            "scope": "Workspace",
            "scope_id": "ws-req-1",
            "prompt": "Be the workspace persona."
        });
        let create_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/meta-specs")
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&create_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(create_resp.status(), StatusCode::CREATED);
        let id = body_json(create_resp).await["id"]
            .as_str()
            .unwrap()
            .to_string();
        let approve_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/meta-specs/{id}"))
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"approval_status": "Approved"}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(approve_resp.status(), StatusCode::OK);

        // The AC1 filtered listing at the specced path: only this entry
        // matches scope+scope_id+kind.
        let list_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/meta-specs?scope=workspace&scope_id=ws-req-1&kind=meta:persona")
                    .header("authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(list_resp.status(), StatusCode::OK);
        let list = body_json(list_resp).await;
        let arr = list.as_array().unwrap();
        assert_eq!(arr.len(), 1, "filtered list returns exactly the entry");
        assert_eq!(arr[0]["name"].as_str().unwrap(), "ws-owner-persona");

        // A workspace Owner member (Developer global role — NOT tenant
        // Admin) PUTs required=true. At the base tree this is 403 because
        // the write gate is tenant-Admin for every scope; the specced
        // scope-level grant must allow it.
        let owner_jwt = sign_test_jwt(
            &serde_json::json!({
                "sub": "req-owner-sub",
                "preferred_username": "req-owner",
                "realm_access": { "roles": ["developer"] }
            }),
            3600,
        );
        let owner_user = gyre_domain::User::new(
            gyre_common::Id::new("user-req-owner"),
            "req-owner-sub",
            "req-owner",
            0,
        );
        state.users.create(&owner_user).await.unwrap();
        state
            .workspace_memberships
            .create(&WorkspaceMembership::new(
                gyre_common::Id::new("m-req-1"),
                owner_user.id.clone(),
                gyre_common::Id::new("ws-req-1"),
                WorkspaceRole::Owner,
                gyre_common::Id::new("admin"),
                0,
            ))
            .await
            .unwrap();
        let put_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/meta-specs/{id}"))
                    .header("authorization", format!("Bearer {owner_jwt}"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"required": true}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            put_resp.status(),
            StatusCode::OK,
            "workspace Owner must be able to set required on a workspace meta-spec"
        );
        assert_eq!(body_json(put_resp).await["required"], true);

        // The flag persists in the store.
        let ms = state
            .meta_specs
            .get_by_id(&gyre_common::Id::new(&id))
            .await
            .unwrap()
            .unwrap();
        assert!(ms.required, "required must persist");

        // Injection: agents spawned in that workspace receive the meta-spec
        // in the required-workspace band (the same assembly spawn uses).
        let mut task = gyre_domain::Task::new(gyre_common::Id::new("t-req-1"), "T", 1);
        task.workspace_id = gyre_common::Id::new("ws-req-1");
        let set = crate::prompt_assembly::assemble_prompt_set(&state, &task).await;
        let section = set
            .sections
            .iter()
            .find(|s| s.name == "ws-owner-persona")
            .expect("required workspace meta-spec must be injected");
        assert_eq!(section.source, "required-workspace");
        assert!(section.used.required);

        // A workspace Admin member (also not tenant Admin) may write too.
        let ws_admin_jwt = sign_test_jwt(
            &serde_json::json!({
                "sub": "req-ws-admin-sub",
                "preferred_username": "req-ws-admin",
                "realm_access": { "roles": ["developer"] }
            }),
            3600,
        );
        let ws_admin_user = gyre_domain::User::new(
            gyre_common::Id::new("user-req-ws-admin"),
            "req-ws-admin-sub",
            "req-ws-admin",
            0,
        );
        state.users.create(&ws_admin_user).await.unwrap();
        state
            .workspace_memberships
            .create(&WorkspaceMembership::new(
                gyre_common::Id::new("m-req-2"),
                ws_admin_user.id.clone(),
                gyre_common::Id::new("ws-req-1"),
                WorkspaceRole::Admin,
                gyre_common::Id::new("admin"),
                0,
            ))
            .await
            .unwrap();
        let put_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/meta-specs/{id}"))
                    .header("authorization", format!("Bearer {ws_admin_jwt}"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"required": false}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            put_resp.status(),
            StatusCode::OK,
            "workspace Admin member must be able to write workspace meta-specs"
        );

        // A mere Developer member (not Owner/Admin) is still forbidden.
        let member_jwt = sign_test_jwt(
            &serde_json::json!({
                "sub": "req-member-sub",
                "preferred_username": "req-member",
                "realm_access": { "roles": ["developer"] }
            }),
            3600,
        );
        let member_user = gyre_domain::User::new(
            gyre_common::Id::new("user-req-member"),
            "req-member-sub",
            "req-member",
            0,
        );
        state.users.create(&member_user).await.unwrap();
        state
            .workspace_memberships
            .create(&WorkspaceMembership::new(
                gyre_common::Id::new("m-req-3"),
                member_user.id.clone(),
                gyre_common::Id::new("ws-req-1"),
                WorkspaceRole::Developer,
                gyre_common::Id::new("admin"),
                0,
            ))
            .await
            .unwrap();
        let put_resp = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/meta-specs/{id}"))
                    .header("authorization", format!("Bearer {member_jwt}"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"required": true}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            put_resp.status(),
            StatusCode::FORBIDDEN,
            "a plain workspace Developer member must not write registry entries"
        );
    }

    /// §2 Required vs Optional: the privilege-escalation direction — a
    /// workspace Owner/Admin writing a Global- or Tenant-scoped meta-spec
    /// (any field, including `required`) is rejected 403. Only tenant
    /// Admins write those scopes.
    #[tokio::test(flavor = "multi_thread")]
    async fn workspace_owner_cannot_write_global_or_tenant_meta_spec() {
        use crate::auth::test_helpers::{make_test_state_with_jwt, sign_test_jwt};
        use gyre_domain::{Workspace, WorkspaceMembership, WorkspaceRole};

        let state = make_test_state_with_jwt();
        // The Tenant-scoped POST below pins scope_id to the caller's own
        // tenant "default"; `require_registry_scope` fails closed (404)
        // when the scope tenant row is missing, and the mem test state
        // wires an empty tenant repo — materialize it like production
        // startup (main.rs) and the sibling reconciliation tests do.
        crate::bootstrap_default_tenant(&state).await;
        let app = crate::api::api_router().with_state(state.clone());

        // A workspace the owner belongs to (their authority exists, but
        // only over that workspace's scope).
        let ws = Workspace::new(
            gyre_common::Id::new("ws-esc-1"),
            gyre_common::Id::new("default"),
            "EscWs",
            "esc-ws",
            0,
        );
        state.workspaces.create(&ws).await.unwrap();
        let owner_jwt = sign_test_jwt(
            &serde_json::json!({
                "sub": "esc-owner-sub",
                "preferred_username": "esc-owner",
                "realm_access": { "roles": ["developer"] }
            }),
            3600,
        );
        let owner_user = gyre_domain::User::new(
            gyre_common::Id::new("user-esc-owner"),
            "esc-owner-sub",
            "esc-owner",
            0,
        );
        state.users.create(&owner_user).await.unwrap();
        state
            .workspace_memberships
            .create(&WorkspaceMembership::new(
                gyre_common::Id::new("m-esc-1"),
                owner_user.id.clone(),
                gyre_common::Id::new("ws-esc-1"),
                WorkspaceRole::Owner,
                gyre_common::Id::new("admin"),
                0,
            ))
            .await
            .unwrap();

        // Global meta-spec (admin-created).
        let global_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/meta-specs")
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "kind": "meta:principle",
                            "name": "global-esc-target",
                            "scope": "Global",
                            "prompt": "p"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(global_resp.status(), StatusCode::CREATED);
        let global_id = body_json(global_resp).await["id"]
            .as_str()
            .unwrap()
            .to_string();

        // Owner PUT (required) on the Global entry → 403.
        let put_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/meta-specs/{global_id}"))
                    .header("authorization", format!("Bearer {owner_jwt}"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"required": true}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            put_resp.status(),
            StatusCode::FORBIDDEN,
            "workspace Owner must not write Global-scoped meta-specs"
        );
        // Flag not flipped by the rejected request.
        let ms = state
            .meta_specs
            .get_by_id(&gyre_common::Id::new(&global_id))
            .await
            .unwrap()
            .unwrap();
        assert!(!ms.required);

        // Tenant-scoped meta-spec in the caller's OWN tenant: membership
        // authority over a workspace must not reach up to tenant scope.
        let tenant_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/meta-specs")
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "kind": "meta:standard",
                            "name": "tenant-esc-target",
                            "scope": "Tenant",
                            "scope_id": "default",
                            "prompt": "p"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(tenant_resp.status(), StatusCode::CREATED);
        let tenant_id = body_json(tenant_resp).await["id"]
            .as_str()
            .unwrap()
            .to_string();
        let put_resp = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/meta-specs/{tenant_id}"))
                    .header("authorization", format!("Bearer {owner_jwt}"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"required": true}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            put_resp.status(),
            StatusCode::FORBIDDEN,
            "workspace Owner must not write Tenant-scoped meta-specs (privilege escalation)"
        );
    }

    /// The registry CRUD moved to the specced flat routes
    /// (/api/v1/meta-specs, agent-runtime §2 API table) with NO silent
    /// alias: the old /api/v1/meta-specs-registry prefix is gone (404),
    /// and the blast-radius route still resolves under the shared prefix.
    #[tokio::test(flavor = "multi_thread")]
    async fn registry_old_prefix_removed_no_alias() {
        use crate::auth::test_helpers::make_test_state_with_jwt;

        let state = make_test_state_with_jwt();
        let app = crate::api::api_router().with_state(state);

        for (method, uri) in [
            ("GET", "/api/v1/meta-specs-registry"),
            ("POST", "/api/v1/meta-specs-registry"),
            (
                "GET",
                "/api/v1/meta-specs-registry/00000000-0000-0000-0000-000000000000",
            ),
            (
                "PUT",
                "/api/v1/meta-specs-registry/00000000-0000-0000-0000-000000000000",
            ),
            (
                "DELETE",
                "/api/v1/meta-specs-registry/00000000-0000-0000-0000-000000000000",
            ),
            (
                "GET",
                "/api/v1/meta-specs-registry/00000000-0000-0000-0000-000000000000/versions",
            ),
            (
                "GET",
                "/api/v1/meta-specs-registry/00000000-0000-0000-0000-000000000000/versions/1",
            ),
        ] {
            let resp = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method(method)
                        .uri(uri)
                        .header("authorization", "Bearer test-token")
                        .header("content-type", "application/json")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                resp.status(),
                StatusCode::NOT_FOUND,
                "{method} {uri} must be gone (no silent alias)"
            );
        }

        // The blast-radius route under the shared prefix still resolves
        // (a miss for an unknown path is its normal 404/405 shape — the
        // point is that the route is registered, not shadowed).
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/meta-specs/meta%2Fpersonas%2Fbackend.md/blast-radius")
                    .header("authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_ne!(
            resp.status(),
            StatusCode::NOT_FOUND,
            "blast-radius route must stay reachable under /api/v1/meta-specs"
        );
    }

    // -----------------------------------------------------------------------
    // Tenant scope (§8): creation validation + force flag
    // -----------------------------------------------------------------------

    #[tokio::test(flavor = "multi_thread")]
    async fn tenant_scoped_spec_requires_existing_tenant() {
        let app = app();
        // Missing scope_id → 400.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/meta-specs")
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "kind": "meta:persona", "name": "t-spec-1", "scope": "Tenant"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        // Nonexistent tenant → 404.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/meta-specs")
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "kind": "meta:persona", "name": "t-spec-2",
                            "scope": "Tenant", "scope_id": "no-such-tenant"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn force_rejected_for_non_tenant_scoped_spec() {
        let app = app();
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/meta-specs")
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "kind": "meta:persona", "name": "g-spec", "scope": "Global"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let id = body_json(resp).await["id"].as_str().unwrap().to_string();

        // force on a Global-scoped spec is not defined → 400, and the
        // update must not be applied.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/meta-specs/{id}"))
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "approval_status": "Approved", "force": true
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn force_requires_admin_of_the_scope_tenant() {
        use crate::auth::test_helpers::{make_test_state_with_jwt, sign_test_jwt};

        let state = make_test_state_with_jwt();
        let app = crate::api::api_router().with_state(state.clone());

        // A tenant OTHER than the caller's ("default").
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/tenants")
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"name": "Other", "slug": "other"}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let other_id = body_json(resp).await["id"].as_str().unwrap().to_string();

        // Tenant-scoped spec for that other tenant, created by an Admin of
        // the SCOPE tenant (the scope-level write grant — the "default"
        // system admin is cross-tenant here and would be rejected); force
        // is the tenant-scoped power under test below.
        let scope_admin_jwt = sign_test_jwt(
            &serde_json::json!({
                "sub": "force-scope-admin-sub",
                "preferred_username": "force-scope-admin",
                "tenant_id": other_id,
                "realm_access": { "roles": ["admin"] }
            }),
            3600,
        );
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/meta-specs")
                    .header("authorization", format!("Bearer {scope_admin_jwt}"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "kind": "meta:persona", "name": "other-tenant-spec",
                            "scope": "Tenant", "scope_id": other_id
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let id = body_json(resp).await["id"].as_str().unwrap().to_string();

        // Forcing propagation into ANOTHER tenant's workspaces is forbidden.
        let resp = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/meta-specs/{id}"))
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "approval_status": "Approved", "force": true
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }
}
