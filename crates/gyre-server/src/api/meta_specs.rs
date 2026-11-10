//! Meta-spec reconciliation API endpoints (M32).
//!
//! GET  /api/v1/workspaces/{id}/meta-spec-set  — get workspace meta-spec set
//! PUT  /api/v1/workspaces/{id}/meta-spec-set  — update (Admin only)
//! GET  /api/v1/workspaces/{id}/meta-specs/rollout-policy — get rollout policy (§7)
//! PUT  /api/v1/workspaces/{id}/meta-specs/rollout-policy — update (Admin only, §7)
//! GET  /api/v1/meta-specs/{path}/blast-radius — blast radius for a meta-spec
//! POST /api/v1/workspaces/{id}/meta-specs/preview — preview a draft meta-spec change
//! GET  /api/v1/workspaces/{id}/meta-specs/preview/{preview_id} — poll preview status

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use gyre_common::Id;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Arc;

use crate::{auth::AuthenticatedAgent, AppState};

use super::error::ApiError;
use super::now_secs;

// ---------------------------------------------------------------------------
// Types — meta-spec set
// ---------------------------------------------------------------------------

/// A pinned meta-spec entry (path + SHA).
#[derive(Clone, Debug, Serialize, Deserialize)]
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
// Types — preview
// ---------------------------------------------------------------------------

/// A draft meta-spec (content not yet committed to the spec registry).
#[derive(Deserialize)]
pub struct DraftMetaSpec {
    pub kind: String,
    pub content: String,
}

/// Request body for POST /api/v1/workspaces/{id}/meta-specs/preview.
///
/// Accepts both the UI-style format (`persona_id` + `spec_paths`) and the
/// spec-defined format (`draft` + `targets`).
#[derive(Deserialize)]
pub struct PreviewMetaSpecRequest {
    /// Persona being previewed (UI flow: persona editor).
    #[serde(default)]
    pub persona_id: Option<String>,
    /// Draft content for the persona or meta-spec being tested.
    #[serde(default)]
    pub content: Option<String>,
    /// Spec paths (relative to workspace) to preview against (UI flow).
    #[serde(default)]
    pub spec_paths: Vec<String>,
    /// Draft meta-spec object (spec-defined format).
    #[serde(default)]
    pub draft: Option<DraftMetaSpec>,
}

/// Status of one spec in a preview run.
#[derive(Clone, Serialize, Deserialize)]
pub struct PreviewSpecItem {
    pub path: String,
    /// "running" while in progress; "complete" when done.
    pub status: String,
}

/// Summarised blast radius included in a preview response.
#[derive(Clone, Serialize, Deserialize)]
pub struct PreviewBlastRadius {
    pub affected_workspaces: Vec<AffectedWorkspace>,
    pub affected_repos: Vec<AffectedRepo>,
    /// Count of workspaces that currently bind meta-specs overlapping with the draft.
    pub affected_workspace_count: usize,
}

/// Structural impact summary included in a preview response.
#[derive(Clone, Serialize, Deserialize)]
pub struct StructuralImpact {
    /// Scope of the meta-spec ("workspace", "tenant", "repo").
    pub scope: String,
    /// Number of specs that would be affected by adopting this draft.
    pub affected_spec_count: usize,
}

/// Stored preview session (serialized to kv_store, namespace "meta_spec_previews").
#[derive(Serialize, Deserialize)]
struct PreviewRecord {
    pub preview_id: String,
    pub workspace_id: String,
    /// "running" → "complete" (currently set to "complete" immediately since
    /// agents are not spawned; designed for future async expansion).
    pub state: String,
    pub specs: Vec<PreviewSpecItem>,
    pub blast_radius: PreviewBlastRadius,
    pub structural_impact: StructuralImpact,
    pub created_at: u64,
}

/// Response for POST /api/v1/workspaces/{id}/meta-specs/preview (202 Accepted).
#[derive(Serialize)]
pub struct PreviewResponse {
    pub preview_id: String,
    pub state: String,
    pub specs: Vec<PreviewSpecItem>,
    pub blast_radius: PreviewBlastRadius,
    pub structural_impact: StructuralImpact,
}

/// Response for GET /api/v1/workspaces/{id}/meta-specs/preview/{preview_id}.
#[derive(Serialize)]
pub struct PreviewStatusResponse {
    pub preview_id: String,
    pub state: String,
    pub specs: Vec<PreviewSpecItem>,
    pub blast_radius: PreviewBlastRadius,
    pub structural_impact: StructuralImpact,
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
    state
        .workspaces
        .find_by_id(&Id::new(&workspace_id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("workspace '{workspace_id}' not found")))?;

    let set = match state.meta_spec_sets.get(&Id::new(&workspace_id)).await? {
        Some(json) => serde_json::from_str::<MetaSpecSet>(&json)
            .map_err(|e| ApiError::Internal(anyhow::anyhow!("corrupt meta_spec_set: {e}")))?,
        None => MetaSpecSet {
            workspace_id: workspace_id.clone(),
            ..Default::default()
        },
    };
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
    state
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
            Some(Id::new(workspace_id.clone())),
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

    // List all workspaces and check each one for a meta-spec-set referencing spec_path.
    let workspaces = state.workspaces.list().await.unwrap_or_default();
    for workspace in &workspaces {
        let ws_id = workspace.id.as_str();
        let set_opt = state
            .meta_spec_sets
            .get(&workspace.id)
            .await
            .ok()
            .flatten()
            .and_then(|json| serde_json::from_str::<MetaSpecSet>(&json).ok());

        if let Some(set) = set_opt {
            let references_spec = set.personas.values().any(|e| e.path == spec_path)
                || set.principles.iter().any(|e| e.path == spec_path)
                || set.standards.iter().any(|e| e.path == spec_path)
                || set.process.iter().any(|e| e.path == spec_path);

            if references_spec {
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
                        reason: "workspace_binding".to_string(),
                    });
                }
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
// POST /api/v1/workspaces/{id}/meta-specs/preview  (Admin or Developer)
// ---------------------------------------------------------------------------

pub async fn post_meta_spec_preview(
    State(state): State<Arc<AppState>>,
    Path(workspace_id): Path<String>,
    auth: AuthenticatedAgent,
    Json(req): Json<PreviewMetaSpecRequest>,
) -> Result<(StatusCode, Json<PreviewResponse>), ApiError> {
    // Require Admin or Developer role: preview runs agents against real specs, so
    // read-only and agent callers must not trigger preview runs.
    let allowed = auth.roles.contains(&gyre_domain::UserRole::Admin)
        || auth.roles.contains(&gyre_domain::UserRole::Developer);
    if !allowed {
        crate::rbac::record_rbac_denied(
            &state,
            &auth,
            "Admin or Developer",
            "POST /api/v1/workspaces/:id/meta-specs/preview",
        )
        .await;
        return Err(ApiError::Forbidden(
            "Admin or Developer role required to run meta-spec preview".to_string(),
        ));
    }

    // Verify workspace exists.
    state
        .workspaces
        .find_by_id(&Id::new(&workspace_id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("workspace '{workspace_id}' not found")))?;

    // Collect spec paths from either field format.
    let spec_paths: Vec<String> = if !req.spec_paths.is_empty() {
        req.spec_paths
    } else if let Some(ref draft) = req.draft {
        // spec-format: extract path from draft kind for a synthetic target
        vec![format!("meta/{}.md", draft.kind.replace(':', "/"))]
    } else {
        vec![]
    };

    if spec_paths.is_empty() {
        return Err(ApiError::BadRequest(
            "at least one spec path must be provided in spec_paths or targets".to_string(),
        ));
    }

    // Compute blast radius from the workspace's current meta-spec-set.
    // This tells the caller which workspaces/repos currently bind meta-specs that
    // the draft would replace (structural impact prediction without LLM).
    let blast_radius = compute_preview_blast_radius(&state, &workspace_id).await;

    let affected_spec_count = spec_paths.len();

    let structural_impact = StructuralImpact {
        scope: "workspace".to_string(),
        affected_spec_count,
    };

    // Build spec items. Preview is non-blocking: specs are marked "complete"
    // immediately because no agents are spawned in this endpoint. The endpoint
    // provides structural impact analysis; actual agent-based preview runs are
    // a future extension (spawn agents on preview branches per §5).
    let specs: Vec<PreviewSpecItem> = spec_paths
        .iter()
        .map(|p| PreviewSpecItem {
            path: p.clone(),
            status: "complete".to_string(),
        })
        .collect();

    // Generate a stable preview ID and persist the record so the status endpoint
    // can look it up.
    let preview_id = uuid::Uuid::new_v4().to_string();

    let record = PreviewRecord {
        preview_id: preview_id.clone(),
        workspace_id: workspace_id.clone(),
        state: "complete".to_string(),
        specs: specs.clone(),
        blast_radius: blast_radius.clone(),
        structural_impact: structural_impact.clone(),
        created_at: now_secs(),
    };

    let record_json = serde_json::to_string(&record)
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("serialize preview record: {e}")))?;

    state
        .kv_store
        .kv_set("meta_spec_previews", &preview_id, record_json)
        .await
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("store preview record: {e}")))?;

    Ok((
        StatusCode::ACCEPTED,
        Json(PreviewResponse {
            preview_id,
            state: "complete".to_string(),
            specs,
            blast_radius,
            structural_impact,
        }),
    ))
}

// ---------------------------------------------------------------------------
// GET /api/v1/workspaces/{id}/meta-specs/preview/{preview_id}
// ---------------------------------------------------------------------------

pub async fn get_meta_spec_preview_status(
    State(state): State<Arc<AppState>>,
    Path((workspace_id, preview_id)): Path<(String, String)>,
    _auth: AuthenticatedAgent,
) -> Result<Json<PreviewStatusResponse>, ApiError> {
    // Verify workspace exists.
    state
        .workspaces
        .find_by_id(&Id::new(&workspace_id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("workspace '{workspace_id}' not found")))?;

    let record_json = state
        .kv_store
        .kv_get("meta_spec_previews", &preview_id)
        .await
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("kv lookup: {e}")))?
        .ok_or_else(|| ApiError::NotFound(format!("preview '{preview_id}' not found")))?;

    let record: PreviewRecord = serde_json::from_str(&record_json)
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("corrupt preview record: {e}")))?;

    // Validate that this preview belongs to the requested workspace.
    if record.workspace_id != workspace_id {
        return Err(ApiError::NotFound(format!(
            "preview '{preview_id}' not found"
        )));
    }

    Ok(Json(PreviewStatusResponse {
        preview_id: record.preview_id,
        state: record.state,
        specs: record.specs,
        blast_radius: record.blast_radius,
        structural_impact: record.structural_impact,
    }))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Compute a blast-radius summary for a workspace's current meta-spec-set.
///
/// Returns the set of workspaces and repos that reference any of the meta-specs
/// currently bound to `workspace_id`. This gives a structural impact estimate:
/// if the draft replaces any of those specs, all of these workspaces/repos are
/// affected.
async fn compute_preview_blast_radius(
    state: &Arc<AppState>,
    workspace_id: &str,
) -> PreviewBlastRadius {
    // caller-scope:ok — correctly uses ws_id (each workspace's own ID) inside the loop, not the parameter
    // Collect the spec paths currently bound in this workspace's meta-spec-set.
    let bound_paths: Vec<String> = state
        .meta_spec_sets
        .get(&Id::new(workspace_id))
        .await
        .ok()
        .flatten()
        .and_then(|json| serde_json::from_str::<MetaSpecSet>(&json).ok())
        .map(|set| {
            let mut paths: Vec<String> = set.personas.values().map(|e| e.path.clone()).collect();
            paths.extend(set.principles.iter().map(|e| e.path.clone()));
            paths.extend(set.standards.iter().map(|e| e.path.clone()));
            paths.extend(set.process.iter().map(|e| e.path.clone()));
            paths
        })
        .unwrap_or_default();

    let mut affected_workspaces: Vec<AffectedWorkspace> = Vec::new();
    let mut affected_repos: Vec<AffectedRepo> = Vec::new();

    let workspaces = state.workspaces.list().await.unwrap_or_default();
    for workspace in &workspaces {
        let ws_id = workspace.id.as_str();
        let set_opt = state
            .meta_spec_sets
            .get(&workspace.id)
            .await
            .ok()
            .flatten()
            .and_then(|json| serde_json::from_str::<MetaSpecSet>(&json).ok());

        if let Some(set) = set_opt {
            let references_any_bound = bound_paths.iter().any(|bp| {
                set.personas.values().any(|e| &e.path == bp)
                    || set.principles.iter().any(|e| &e.path == bp)
                    || set.standards.iter().any(|e| &e.path == bp)
                    || set.process.iter().any(|e| &e.path == bp)
            });

            if references_any_bound {
                affected_workspaces.push(AffectedWorkspace {
                    id: ws_id.to_string(),
                });

                let repos = state
                    .repos
                    .list_by_workspace(&Id::new(ws_id))
                    .await
                    .unwrap_or_default();

                for repo in &repos {
                    affected_repos.push(AffectedRepo {
                        id: repo.id.to_string(),
                        workspace_id: ws_id.to_string(),
                        reason: "workspace_binding".to_string(),
                    });
                }
            }
        }
    }

    let affected_workspace_count = affected_workspaces.len();

    PreviewBlastRadius {
        affected_workspaces,
        affected_repos,
        affected_workspace_count,
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use crate::mem::test_state;
    use axum::{body::Body, Router};
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
    // Preview endpoint tests
    // -----------------------------------------------------------------------

    /// Helper: create a workspace and return its id string.
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
    async fn preview_returns_202_with_preview_id() {
        let app = app();
        let ws_id = create_workspace(&app, "preview-ws-1").await;

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/workspaces/{ws_id}/meta-specs/preview"))
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"spec_paths":["specs/system/search.md","specs/system/identity.md"]}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::ACCEPTED);
        let json = body_json(resp).await;
        assert!(
            json["preview_id"].as_str().is_some(),
            "preview_id must be present"
        );
        assert_eq!(json["state"].as_str().unwrap(), "complete");
        let specs = json["specs"].as_array().unwrap();
        assert_eq!(specs.len(), 2);
        assert_eq!(specs[0]["status"].as_str().unwrap(), "complete");
        assert_eq!(specs[1]["status"].as_str().unwrap(), "complete");
        assert!(json["blast_radius"].is_object());
        assert!(json["structural_impact"].is_object());
        assert_eq!(
            json["structural_impact"]["affected_spec_count"]
                .as_u64()
                .unwrap(),
            2
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn preview_status_endpoint_returns_stored_preview() {
        let app = app();
        let ws_id = create_workspace(&app, "preview-ws-2").await;

        // Create preview.
        let create_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/workspaces/{ws_id}/meta-specs/preview"))
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"spec_paths":["specs/system/auth.md"]}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(create_resp.status(), StatusCode::ACCEPTED);
        let create_json = body_json(create_resp).await;
        let preview_id = create_json["preview_id"].as_str().unwrap().to_string();

        // Poll status.
        let status_resp = app
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/api/v1/workspaces/{ws_id}/meta-specs/preview/{preview_id}"
                    ))
                    .header("authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(status_resp.status(), StatusCode::OK);
        let status_json = body_json(status_resp).await;
        assert_eq!(status_json["preview_id"].as_str().unwrap(), preview_id);
        assert_eq!(status_json["state"].as_str().unwrap(), "complete");
        let specs = status_json["specs"].as_array().unwrap();
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0]["path"].as_str().unwrap(), "specs/system/auth.md");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn preview_requires_at_least_one_spec() {
        let app = app();
        let ws_id = create_workspace(&app, "preview-ws-3").await;

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/workspaces/{ws_id}/meta-specs/preview"))
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"spec_paths":[]}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn preview_returns_404_for_unknown_workspace() {
        let resp = app()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/workspaces/00000000-0000-0000-0000-000000000000/meta-specs/preview")
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"spec_paths":["specs/system/auth.md"]}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn preview_status_returns_404_for_unknown_preview_id() {
        let app = app();
        let ws_id = create_workspace(&app, "preview-ws-4").await;

        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/api/v1/workspaces/{ws_id}/meta-specs/preview/00000000-0000-0000-0000-000000000000"
                    ))
                    .header("authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn preview_readonly_role_is_forbidden() {
        use crate::abac_middleware::seed_builtin_policies;
        use crate::auth::test_helpers::{make_test_state_with_jwt, sign_test_jwt};
        let state = make_test_state_with_jwt();
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current()
                .block_on(seed_builtin_policies(&state))
                .expect("seed built-in policies")
        });

        // Create workspace as admin.
        let ws_resp = crate::api::api_router()
            .with_state(state.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/workspaces")
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"name":"preview-ws-ro","slug":"preview-ws-ro"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(ws_resp.status(), StatusCode::CREATED);
        let ws_json = body_json(ws_resp).await;
        let ws_id = ws_json["id"].as_str().unwrap().to_string();

        // ReadOnly OIDC JWT.
        let ro_token = sign_test_jwt(
            &serde_json::json!({
                "sub": "ro-sub",
                "preferred_username": "readonly-user",
                "realm_access": { "roles": ["readonly"] }
            }),
            3600,
        );

        let resp = crate::api::api_router()
            .with_state(state)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/workspaces/{ws_id}/meta-specs/preview"))
                    .header("authorization", format!("Bearer {ro_token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"spec_paths":["specs/system/auth.md"]}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }
}

// ===========================================================================
// Meta-spec registry CRUD API (agent-runtime spec §2)
//
// GET    /api/v1/meta-specs-registry           — list (query: scope, scope_id, kind, required)
// POST   /api/v1/meta-specs-registry           — create
// GET    /api/v1/meta-specs-registry/:id       — get by id
// PUT    /api/v1/meta-specs-registry/:id       — update (content edit bumps version; approval-only PUT does not)
// DELETE /api/v1/meta-specs-registry/:id       — delete (409 if bindings)
// GET    /api/v1/meta-specs-registry/:id/versions       — list versions
// GET    /api/v1/meta-specs-registry/:id/versions/:ver  — get specific version
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
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn parse_kind(s: &str) -> Result<MetaSpecKind, ApiError> {
    MetaSpecKind::parse(s).ok_or_else(|| {
        ApiError::BadRequest(format!(
            "invalid kind '{s}'; must be one of: meta:persona, meta:principle, meta:standard, meta:process"
        ))
    })
}

fn parse_scope(s: &str) -> Result<MetaSpecScope, ApiError> {
    MetaSpecScope::parse(s).ok_or_else(|| {
        ApiError::BadRequest(format!(
            "invalid scope '{s}'; must be one of: Global, Workspace"
        ))
    })
}

fn parse_approval_status(s: &str) -> Result<MetaSpecApprovalStatus, ApiError> {
    MetaSpecApprovalStatus::parse(s).ok_or_else(|| {
        ApiError::BadRequest(format!(
            "invalid approval_status '{s}'; must be: Pending, Approved, or Rejected"
        ))
    })
}

// ---------------------------------------------------------------------------
// GET /api/v1/meta-specs-registry
// ---------------------------------------------------------------------------

pub async fn list_meta_specs_registry(
    State(state): State<Arc<AppState>>,
    auth: AuthenticatedAgent,
    Query(q): Query<ListMetaSpecsQuery>,
) -> Result<Json<Vec<MetaSpec>>, ApiError> {
    // Admin-only: the registry is tenant-wide governance configuration (it
    // defines the meta-specs that can be bound to any workspace). This route
    // is ABAC-exempt, so the handler is the only authorization surface —
    // any authenticated agent must not enumerate cross-workspace governance.
    if !auth.roles.contains(&gyre_domain::UserRole::Admin) {
        crate::rbac::record_rbac_denied(&state, &auth, "Admin", "GET /api/v1/meta-specs-registry")
            .await;
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
    // tokens, membership via the explorer-views pattern). Global-scope
    // registry entries are shared reference data, readable by any
    // authenticated principal (docs/api-reference.md §meta-specs-registry).
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
    let results = state
        .meta_specs
        .list(&filter)
        .await
        .map_err(ApiError::Internal)?;
    Ok(Json(results))
}

// ---------------------------------------------------------------------------
// POST /api/v1/meta-specs-registry
// ---------------------------------------------------------------------------

pub async fn create_meta_spec_registry(
    State(state): State<Arc<AppState>>,
    auth: AuthenticatedAgent,
    Json(req): Json<CreateMetaSpecRequest>,
) -> Result<(StatusCode, Json<MetaSpec>), ApiError> {
    // Admin-only (ABAC-exempt route — the handler is the authorization
    // surface): creating registry entries defines tenant-wide governance;
    // non-Admin callers must not mint meta-specs.
    if !auth.roles.contains(&gyre_domain::UserRole::Admin) {
        crate::rbac::record_rbac_denied(&state, &auth, "Admin", "POST /api/v1/meta-specs-registry")
            .await;
        return Err(ApiError::Forbidden(
            "only Admin role may create meta-specs registry entries".to_string(),
        ));
    }
    let kind = parse_kind(&req.kind)?;
    let scope = parse_scope(&req.scope)?;
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
    Ok((StatusCode::CREATED, Json(ms)))
}

// ---------------------------------------------------------------------------
// GET /api/v1/meta-specs-registry/:id
// ---------------------------------------------------------------------------

pub async fn get_meta_spec_registry(
    State(state): State<Arc<AppState>>,
    _auth: AuthenticatedAgent,
    Path(id): Path<String>,
) -> Result<Json<MetaSpec>, ApiError> {
    let ms = state
        .meta_specs
        .get_by_id(&Id::new(&id))
        .await
        .map_err(ApiError::Internal)?
        .ok_or_else(|| ApiError::NotFound(format!("meta-spec '{id}' not found")))?;
    Ok(Json(ms))
}

// ---------------------------------------------------------------------------
// PUT /api/v1/meta-specs-registry/:id
// ---------------------------------------------------------------------------

pub async fn update_meta_spec_registry(
    State(state): State<Arc<AppState>>,
    auth: AuthenticatedAgent,
    Path(id): Path<String>,
    Json(req): Json<UpdateMetaSpecRequest>,
) -> Result<Json<MetaSpec>, ApiError> {
    // Admin-only (docs: "Admin only for PUT/DELETE"; ABAC-exempt route —
    // the handler is the authorization surface). Without this gate, any
    // authenticated agent could edit governance prompts and reset approvals.
    if !auth.roles.contains(&gyre_domain::UserRole::Admin) {
        crate::rbac::record_rbac_denied(
            &state,
            &auth,
            "Admin",
            "PUT /api/v1/meta-specs-registry/:id",
        )
        .await;
        return Err(ApiError::Forbidden(
            "only Admin role may update meta-specs registry entries".to_string(),
        ));
    }
    let mut ms = state
        .meta_specs
        .get_by_id(&Id::new(&id))
        .await
        .map_err(ApiError::Internal)?
        .ok_or_else(|| ApiError::NotFound(format!("meta-spec '{id}' not found")))?;

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
        // The outer write gate already enforces this for every PUT.
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
        let workspaces = state.workspaces.list().await.unwrap_or_default();
        for ws in &workspaces {
            let set_json = state.meta_spec_sets.get(&ws.id).await.ok().flatten();
            let binds = set_json
                .as_deref()
                .and_then(|j| serde_json::from_str::<MetaSpecSet>(j).ok())
                .map(|s| {
                    s.personas.values().any(|e| e.path == spec_path)
                        || s.principles.iter().any(|e| e.path == spec_path)
                        || s.standards.iter().any(|e| e.path == spec_path)
                        || s.process.iter().any(|e| e.path == spec_path)
                })
                .unwrap_or(false);
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

    Ok(Json(ms))
}

// ---------------------------------------------------------------------------
// DELETE /api/v1/meta-specs-registry/:id
// ---------------------------------------------------------------------------
pub async fn delete_meta_spec_registry(
    State(state): State<Arc<AppState>>,
    auth: AuthenticatedAgent,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    // Admin-only (docs: "Admin only for PUT/DELETE"; ABAC-exempt route —
    // the handler is the authorization surface).
    if !auth.roles.contains(&gyre_domain::UserRole::Admin) {
        crate::rbac::record_rbac_denied(
            &state,
            &auth,
            "Admin",
            "DELETE /api/v1/meta-specs-registry/:id",
        )
        .await;
        return Err(ApiError::Forbidden(
            "only Admin role may delete meta-specs registry entries".to_string(),
        ));
    }
    let rid = Id::new(&id);
    // 404 before the binding check: deleting a nonexistent meta-spec is a
    // client error, not a silent no-op.
    state
        .meta_specs
        .get_by_id(&rid)
        .await
        .map_err(ApiError::Internal)?
        .ok_or_else(|| ApiError::NotFound(format!("meta-spec '{id}' not found")))?;
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
// GET /api/v1/meta-specs-registry/:id/versions
// ---------------------------------------------------------------------------

pub async fn list_meta_spec_versions(
    State(state): State<Arc<AppState>>,
    _auth: AuthenticatedAgent,
    Path(id): Path<String>,
) -> Result<Json<Vec<gyre_domain::MetaSpecVersion>>, ApiError> {
    // Ensure meta-spec exists.
    state
        .meta_specs
        .get_by_id(&Id::new(&id))
        .await
        .map_err(ApiError::Internal)?
        .ok_or_else(|| ApiError::NotFound(format!("meta-spec '{id}' not found")))?;

    let versions = state
        .meta_specs
        .list_versions(&Id::new(&id))
        .await
        .map_err(ApiError::Internal)?;
    Ok(Json(versions))
}

// ---------------------------------------------------------------------------
// GET /api/v1/meta-specs-registry/:id/versions/:version
// ---------------------------------------------------------------------------

pub async fn get_meta_spec_version(
    State(state): State<Arc<AppState>>,
    _auth: AuthenticatedAgent,
    Path((id, version)): Path<(String, u32)>,
) -> Result<Json<gyre_domain::MetaSpecVersion>, ApiError> {
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

    #[tokio::test(flavor = "multi_thread")]
    async fn create_and_get_meta_spec() {
        let app = app();
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/meta-specs-registry")
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
                    .uri(format!("/api/v1/meta-specs-registry/{id}"))
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
                    .uri("/api/v1/meta-specs-registry")
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
                    .uri("/api/v1/meta-specs-registry")
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
                    .uri(format!("/api/v1/meta-specs-registry/{id}"))
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
                    .uri("/api/v1/meta-specs-registry")
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
                    .uri(format!("/api/v1/meta-specs-registry/{id}"))
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
                    .uri(format!("/api/v1/meta-specs-registry/{id}"))
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
                    .uri("/api/v1/meta-specs-registry")
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
                    .uri(format!("/api/v1/meta-specs-registry/{id}"))
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
                    .uri(format!("/api/v1/meta-specs-registry/{id}"))
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
                    .uri(format!("/api/v1/meta-specs-registry/{id}"))
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
                    .uri(format!("/api/v1/meta-specs-registry/{id}"))
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
                    .uri("/api/v1/meta-specs-registry")
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
                    .uri(format!("/api/v1/meta-specs-registry/{id}"))
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
        // The meta-specs-registry routes are ABAC-exempt (legacy exemption
        // file), so the handler is the only authorization surface. A
        // Developer-role JWT must get 403 from every registry mutation and
        // from the list — governance configuration is Admin-only (docs:
        // "Admin only for PUT/DELETE"; list/create follow the same class).
        use crate::auth::test_helpers::{make_test_state_with_jwt, sign_test_jwt};
        let state = make_test_state_with_jwt();
        let router = crate::api::api_router().with_state(state);

        let dev_token = sign_test_jwt(
            &serde_json::json!({
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
                    .uri("/api/v1/meta-specs-registry")
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
                    .uri("/api/v1/meta-specs-registry")
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
                    .uri("/api/v1/meta-specs-registry/whatever")
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
                    .uri("/api/v1/meta-specs-registry/whatever")
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
                    .uri("/api/v1/meta-specs-registry/00000000-0000-0000-0000-000000000000")
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
                    .uri("/api/v1/meta-specs-registry")
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
                    .uri(format!("/api/v1/meta-specs-registry/{id}"))
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
                    .uri("/api/v1/meta-specs-registry")
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
                    .uri("/api/v1/meta-specs-registry")
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
                    .uri("/api/v1/meta-specs-registry")
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
                    .uri(format!("/api/v1/meta-specs-registry/{id}"))
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
                    .uri(format!("/api/v1/meta-specs-registry/{id}"))
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
                    .uri(format!("/api/v1/meta-specs-registry/{id}"))
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
                    .uri("/api/v1/meta-specs-registry")
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
                    .uri(format!("/api/v1/meta-specs-registry/{ms_id}"))
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
            .list_for_user(&user_id, Some(&ws_id), None, None, None, 50, 0)
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
            .list_for_user(&user_id, Some(&ws_id), None, None, None, 50, 0)
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
            "/api/v1/meta-specs-registry".to_string(),
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
                "/api/v1/meta-specs-registry".to_string(),
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
                format!("/api/v1/meta-specs-registry/{id}"),
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
                format!("/api/v1/meta-specs-registry/{id}"),
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
                format!("/api/v1/meta-specs-registry?scope=Workspace&scope_id={ws_id}"),
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
                format!("/api/v1/meta-specs-registry/{id}"),
                "test-token",
                Some(r#"{"approval_status":"Approved"}"#),
            )
            .await,
            StatusCode::OK
        );
    }
}
