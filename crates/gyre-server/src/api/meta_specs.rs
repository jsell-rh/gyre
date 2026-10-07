//! Meta-spec reconciliation API endpoints (M32).
//!
//! GET  /api/v1/workspaces/{id}/meta-spec-set  — get workspace meta-spec set
//! PUT  /api/v1/workspaces/{id}/meta-spec-set  — update (Admin only)
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
use std::collections::HashMap;
use std::sync::Arc;

use crate::{auth::AuthenticatedAgent, AppState};

use super::error::ApiError;
use super::{new_id, now_secs};

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
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct MetaSpecSet {
    pub workspace_id: String,
    /// Named persona bindings: role -> pinned entry (e.g. "backend" -> path@sha).
    #[serde(default)]
    pub personas: HashMap<String, MetaSpecPinnedEntry>,
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
    pub personas: HashMap<String, MetaSpecPinnedEntry>,
    #[serde(default)]
    pub principles: Vec<MetaSpecPinnedEntry>,
    #[serde(default)]
    pub standards: Vec<MetaSpecPinnedEntry>,
    #[serde(default)]
    pub process: Vec<MetaSpecPinnedEntry>,
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

    Ok((StatusCode::OK, Json(set)))
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
fn require_preview_role(auth: &AuthenticatedAgent) -> Result<(), ApiError> {
    if auth.roles.contains(&UserRole::Admin) || auth.roles.contains(&UserRole::Developer) {
        Ok(())
    } else {
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
    require_preview_role(&auth)?;

    if req.draft.kind.trim().is_empty() || req.draft.content.trim().is_empty() {
        return Err(ApiError::BadRequest(
            "draft.kind and draft.content are both required".to_string(),
        ));
    }
    if req.targets.is_empty() {
        return Err(ApiError::BadRequest("targets must not be empty".to_string()));
    }

    // ── Pass 1: resolve, authorize and validate every target. Nothing is
    // created before this completes, so a rejected target cannot leave an
    // agent, branch or worktree behind.
    let mut resolved: Vec<ResolvedTarget> = Vec::with_capacity(req.targets.len());
    for target in &req.targets {
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
        // Global route: the run's scope is derived from the repo record and the
        // caller's identity — never from the request body.
        crate::abac::check_repo_abac(&state, &repo.id.to_string(), &auth)
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

    // ── Pass 2: budget.
    let requested = resolved.len() as u64;
    if let Some(max) = state.preview_budget_max_concurrent {
        let running = count_running_preview_agents(&state).await;
        if running.saturating_add(requested) > max {
            return Err(ApiError::TooManyRequests(format!(
                "preview budget exhausted: {running} preview agent(s) running, max_concurrent={max}, requested={requested}"
            )));
        }
    } else {
        // Unset: the target workspaces' own budgets govern (server-config.md).
        for rt in &resolved {
            super::budget::check_spawn_budget(&state, rt.workspace.id.as_str())
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
        match provision_preview_agent(&state, &auth, &preview_id, rt, &branch, now).await {
            Ok(p) => provisioned.push(p),
            Err(e) => {
                for p in &provisioned {
                    rollback_preview_agent(&state, &p.agent_ref).await;
                }
                return Err(e);
            }
        }
    }

    // Persist before launching: a fast agent can exit — and report — before the
    // first launch call returns.
    let record = PreviewRecord {
        preview_id: preview_id.clone(),
        requested_by: Some(auth.agent_id.clone()),
        created_at: now,
        agents: provisioned.iter().map(|p| p.agent_ref.clone()).collect(),
    };
    persist_preview(&state, &record).await?;

    tracing::info!(
        preview_id = %preview_id,
        draft_kind = %req.draft.kind,
        agents = provisioned.len(),
        requested_by = %auth.agent_id,
        "meta-spec preview started"
    );

    // ── Pass 4: launch. A target whose process never starts is reported failed
    // and its slot released; the rest of the run still proceeds.
    for p in provisioned.iter_mut() {
        if let Err(reason) = launch_preview_agent(&state, &req.draft, p).await {
            tracing::error!(
                preview_id = %preview_id,
                agent_id = %p.agent_ref.agent_id,
                "preview agent launch failed: {reason}"
            );
            mark_preview_agent_failed(&state, &preview_id, p).await;
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

    Ok((StatusCode::ACCEPTED, Json(PreviewResponse { preview_id, agents })))
}

// ---------------------------------------------------------------------------
// GET /api/v1/meta-specs/preview/{preview_id}
// ---------------------------------------------------------------------------

pub async fn get_meta_spec_preview_status(
    State(state): State<Arc<AppState>>,
    Path(preview_id): Path<String>,
    auth: AuthenticatedAgent,
) -> Result<Json<PreviewStatusResponse>, ApiError> {
    require_preview_role(&auth)?;
    let record = load_preview(&state, &preview_id).await?;
    require_preview_scope(&state, &auth, &record).await?;

    let mut agents = Vec::with_capacity(record.agents.len());
    let mut running = false;
    for agent_ref in &record.agents {
        let status = preview_agent_status(&state, agent_ref).await;
        if status == "running" {
            running = true;
        }
        agents.push(PreviewAgentStatus {
            agent_id: agent_ref.agent_id.clone(),
            repo_id: agent_ref.repo_id.clone(),
            spec_path: agent_ref.spec_path.clone(),
            branch: agent_ref.branch.clone(),
            diff: preview_diff(&state, agent_ref).await,
            status,
        });
    }

    Ok(Json(PreviewStatusResponse {
        preview_id: record.preview_id,
        created_at: record.created_at,
        ttl_seconds: state.preview_ttl_secs,
        state: if running { "running" } else { "complete" }.to_string(),
        agents,
    }))
}

// ---------------------------------------------------------------------------
// DELETE /api/v1/meta-specs/preview/{preview_id}
// ---------------------------------------------------------------------------

pub async fn delete_meta_spec_preview(
    State(state): State<Arc<AppState>>,
    Path(preview_id): Path<String>,
    auth: AuthenticatedAgent,
) -> Result<StatusCode, ApiError> {
    require_preview_role(&auth)?;
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
            tracing::warn!(agent_id, "preview agent lookup failed; treating as a normal agent: {e:#}");
            return false;
        }
    };
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
    stop_preview_agent_row(state, agent_id).await;
    revoke_preview_token(state, agent_id).await;
    remove_preview_worktree(state, &agent_ref).await;
    release_preview_slot(state, &agent_ref).await;
    true
}

/// True when `agent_id` belongs to a preview run.
pub(crate) async fn is_preview_agent(state: &AppState, agent_id: &str) -> bool {
    matches!(state.kv_store.kv_get(PREVIEW_AGENTS_NS, agent_id).await, Ok(Some(_)))
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
                let _ = state.kv_store.kv_remove(PREVIEW_NS, &key).await;
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
                let _ = state.kv_store.kv_remove(PREVIEW_AGENTS_NS, &agent_id).await;
                continue;
            }
        };
        let run_gone = !matches!(
            state.kv_store.kv_get(PREVIEW_NS, &agent_ref.preview_id).await,
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
        let _ = state.kv_store.kv_remove(PREVIEW_AGENTS_NS, &agent_id).await;
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
            tracing::warn!(preview_id, "preview record unavailable during update: {e:?}");
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
/// A repo deleted after the run started is skipped — the run is already
/// heading for teardown — but an unresolvable workspace is denied rather than
/// ridden on an assumed identity.
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
        let repo = match state.repos.find_by_id(&Id::new(&agent_ref.repo_id)).await? {
            Some(repo) => repo,
            None => continue,
        };
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
    Ok(())
}

/// Preview agents are `Active` ledger rows with a running process. Counted from
/// the per-agent index so the query is bounded by preview runs, not by all
/// agents.
async fn count_running_preview_agents(state: &AppState) -> u64 {
    let entries = match state.kv_store.kv_list(PREVIEW_AGENTS_NS).await {
        Ok(entries) => entries,
        Err(e) => {
            tracing::warn!("cannot enumerate preview agents for the budget check: {e:#}");
            return 0;
        }
    };
    let mut count = 0u64;
    for (agent_id, raw) in entries {
        // Only unreleased slots are live: a finished agent still has an index
        // entry (it is the process-exit discriminator) but holds no slot.
        if serde_json::from_str::<PreviewAgentRef>(&raw).map(|a| a.released).unwrap_or(true) {
            continue;
        }
        if let Ok(Some(a)) = state.agents.find_by_id(&Id::new(&agent_id)).await {
            if a.status == AgentStatus::Active {
                count += 1;
            }
        }
    }
    count
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
        Ok(Some(agent)) => {
            preview_agent_status_label(agent.status, agent_ref.released).to_string()
        }
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
fn preview_branch_name(preview_id: &str, spec_path: &str, used: &mut HashMap<String, u32>) -> String {
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
        slug = if qualified.is_empty() { slug } else { qualified };
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
    let repo = &target.repo;

    // Pin the branch to the default-branch tip so the diff a human reads is
    // exactly this agent's work: a default branch that moves afterwards can
    // never show up as someone else's deletions. An empty repo has nothing to
    // preview — there is no spec content to implement.
    let base_sha = crate::git_refs::resolve_ref(&repo.path, &repo.default_branch)
        .await
        .ok_or_else(|| ApiError::BadRequest(format!(
            "repo '{}' has no commits on '{}' — push an initial commit before previewing",
            repo.id, repo.default_branch
        )))?;

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
    state.agents.create(&agent).await?;

    // Throwaway branch checked out into its own worktree — the branch already
    // exists, so `create_worktree` checks it out instead of branching from HEAD.
    let worktree_path = format!("{}/worktrees/{}", repo.path, branch.replace('/', "-"));
    if let Err(e) = state
        .git_ops
        .create_worktree(&repo.path, &worktree_path, branch)
        .await
    {
        rollback_preview_agent(state, &PreviewAgentRef {
            preview_id: preview_id.to_string(),
            agent_id: agent.id.to_string(),
            repo_id: repo.id.to_string(),
            workspace_id: repo.workspace_id.to_string(),
            spec_path: target.spec_path.clone(),
            branch: branch.to_string(),
            base_sha,
            worktree_path,
            released: false,
        })
        .await;
        let msg = e.to_string().to_lowercase();
        if msg.contains("not a valid object") || msg.contains("bad default revision") {
            return Err(ApiError::BadRequest(format!(
                "cannot create preview worktree: repo '{}' has no commits yet (branch: {branch})",
                repo.id
            )));
        }
        return Err(ApiError::Internal(anyhow::anyhow!(
            "create preview worktree '{worktree_path}': {e:#}"
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
        let mut partial = preview_agent_ref(preview_id, &agent, repo, &target.spec_path, branch, base_sha, worktree_path);
        partial.released = true; // no slot was taken
        rollback_preview_agent(state, &partial).await;
        return Err(e);
    }

    // Short-lived token: a preview agent must not outlive its run. The preview
    // id takes the task slot of the claim set — there is no task to name.
    let token = match state.agent_signing_key.mint(
        &agent.id.to_string(),
        preview_id,
        &auth.agent_id,
        &state.base_url,
        state.preview_jwt_ttl_secs,
    ) {
        Ok(token) => token,
        Err(e) => {
            let mut partial = preview_agent_ref(preview_id, &agent, repo, &target.spec_path, branch, base_sha, worktree_path);
            partial.released = true;
            rollback_preview_agent(state, &partial).await;
            return Err(ApiError::Internal(anyhow::anyhow!("mint preview agent JWT: {e}")));
        }
    };
    if let Err(e) = state
        .kv_store
        .kv_set("agent_tokens", &agent.id.to_string(), token.clone())
        .await
    {
        let mut partial = preview_agent_ref(preview_id, &agent, repo, &target.spec_path, branch, base_sha, worktree_path);
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
        agent_ref: preview_agent_ref(preview_id, &agent, repo, &target.spec_path, branch, base_sha, worktree_path),
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
    let ct_entity: Option<ComputeTargetEntity> = if let Some(ct_id) = p
        .workspace
        .compute_target_id
        .clone()
    {
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
    extra_env.insert("GYRE_PREVIEW_ID".to_string(), p.agent_ref.preview_id.clone());
    extra_env.insert("GYRE_META_SPEC_DRAFT_KIND".to_string(), draft.kind.clone());
    extra_env.insert("GYRE_META_SPEC_DRAFT_CONTENT".to_string(), draft.content.clone());
    extra_env.insert("GYRE_TARGET_SPEC_PATH".to_string(), p.agent_ref.spec_path.clone());

    let clone_url = super::spawn::build_clone_url(state, Some(&p.workspace), &p.repo);
    let outcome = super::spawn::launch_agent_process(super::spawn::AgentLaunchParams {
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

    // `launch_agent_process` treats a failed process spawn as best-effort: it
    // logs and returns Ok. A preview whose process never started would sit
    // "running" until the GC sweep, so require the handle.
    let started = if target_type == "container" {
        outcome.container_id.is_some()
    } else {
        outcome.pid.is_some()
    };
    if !started {
        return Err(format!("{target_type} process never started"));
    }
    Ok(())
}

/// Launch failure: terminal agent, revoked token, released slot, worktree gone.
/// The run record survives so the failed agent is still visible in the status.
async fn mark_preview_agent_failed(state: &AppState, preview_id: &str, p: &mut Provisioned) {
    if let Ok(Some(mut agent)) = state.agents.find_by_id(&p.agent.id).await {
        if agent.status == AgentStatus::Active {
            let _ = agent.transition_status(AgentStatus::Failed);
            let _ = state.agents.update(&agent).await;
        }
    }
    revoke_preview_token(state, &p.agent_ref.agent_id).await;
    remove_preview_worktree(state, &p.agent_ref).await;
    if !p.agent_ref.released {
        p.agent_ref.released = true;
        super::budget::decrement_active_agents(state, &p.agent_ref.workspace_id).await;
    }
    let agent_id = p.agent_ref.agent_id.clone();
    mutate_preview(state, preview_id, |record| {
        if let Some(entry) = record
            .agents
            .iter_mut()
            .find(|a| a.agent_id == agent_id)
        {
            entry.released = true;
        }
    })
    .await;
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
    remove_preview_worktree(state, agent_ref).await;
    delete_preview_branch(state, agent_ref).await;
    revoke_preview_token(state, &agent_ref.agent_id).await;
    release_preview_slot(state, agent_ref).await;
}

/// Undo provisioning of an agent that never launched, for a POST being
/// rejected. Unlike teardown it also drops the agent row: nothing ever
/// referenced it, so leaving it would be ledger noise from a failed request.
async fn rollback_preview_agent(state: &AppState, agent_ref: &PreviewAgentRef) {
    remove_preview_worktree(state, agent_ref).await;
    delete_preview_branch(state, agent_ref).await;
    revoke_preview_token(state, &agent_ref.agent_id).await;
    if let Ok(wts) = state.worktrees.find_by_agent(&Id::new(&agent_ref.agent_id)).await {
        for wt in wts {
            let _ = state.worktrees.delete(&wt.id).await;
        }
    }
    if let Err(e) = state.agents.delete(&Id::new(&agent_ref.agent_id)).await {
        tracing::warn!(agent_id = %agent_ref.agent_id, "preview agent row cleanup failed: {e:#}");
    }
    if !agent_ref.released {
        super::budget::decrement_active_agents(state, &agent_ref.workspace_id).await;
    }
}

async fn kill_preview_process(state: &AppState, agent_id: &str) {
    let handle = state.process_registry.lock().await.remove(agent_id);
    match handle {
        Some(handle) => {
            if let Err(e) = super::spawn::kill_process_handle(&handle).await {
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
            let _ = agent.transition_status(AgentStatus::Stopped);
            let _ = state.agents.update(&agent).await;
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
    if let Ok(wts) = state.worktrees.find_by_agent(&Id::new(&agent_ref.agent_id)).await {
        for wt in wts {
            let _ = state.worktrees.delete(&wt.id).await;
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
async fn release_preview_slot(state: &AppState, agent_ref: &PreviewAgentRef) {
    if agent_ref.released {
        return;
    }
    super::budget::decrement_active_agents(state, &agent_ref.workspace_id).await;
    let agent_id = agent_ref.agent_id.clone();
    mutate_preview(state, &agent_ref.preview_id, |record| {
        if let Some(entry) = record
            .agents
            .iter_mut()
            .find(|a| a.agent_id == agent_id)
        {
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
            tokio::runtime::Handle::current().block_on(seed_builtin_policies(&state))
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
            tokio::runtime::Handle::current().block_on(seed_builtin_policies(&state))
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
// PUT    /api/v1/meta-specs-registry/:id       — update (new version, bumps version)
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
    _auth: AuthenticatedAgent,
    Query(q): Query<ListMetaSpecsQuery>,
) -> Result<Json<Vec<MetaSpec>>, ApiError> {
    let scope = match q.scope.as_deref() {
        None => None,
        Some(s) => Some(parse_scope(s)?),
    };
    let kind = match q.kind.as_deref() {
        None => None,
        Some(k) => Some(parse_kind(k)?),
    };
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
    let mut ms = state
        .meta_specs
        .get_by_id(&Id::new(&id))
        .await
        .map_err(ApiError::Internal)?
        .ok_or_else(|| ApiError::NotFound(format!("meta-spec '{id}' not found")))?;

    let now = now_secs();

    if let Some(name) = req.name {
        ms.name = name;
    }
    if let Some(prompt) = req.prompt {
        ms.content_hash = sha256_hex(&prompt);
        ms.prompt = prompt;
        // Spec §2: editing content resets approval to Pending until re-reviewed.
        ms.approval_status = MetaSpecApprovalStatus::Pending;
        ms.approved_by = None;
        ms.approved_at = None;
    }
    if let Some(required) = req.required {
        ms.required = required;
    }
    if let Some(ref status_str) = req.approval_status {
        let status = parse_approval_status(status_str)?;
        if status == MetaSpecApprovalStatus::Approved {
            ms.approved_by = Some(auth.agent_id.as_str().to_string());
            ms.approved_at = Some(now);
        }
        ms.approval_status = status;
    }

    ms.version += 1;
    ms.updated_at = now;

    state
        .meta_specs
        .update(&ms)
        .await
        .map_err(ApiError::Internal)?;
    Ok(Json(ms))
}

// ---------------------------------------------------------------------------
// DELETE /api/v1/meta-specs-registry/:id
// ---------------------------------------------------------------------------

pub async fn delete_meta_spec_registry(
    State(state): State<Arc<AppState>>,
    _auth: AuthenticatedAgent,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let rid = Id::new(&id);
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
}
