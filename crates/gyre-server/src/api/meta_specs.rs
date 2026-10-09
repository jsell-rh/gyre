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
    let worktree_path_for_log = worktree_path.clone();
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
        let mut partial = preview_agent_ref(preview_id, &agent, repo, &target.spec_path, branch, base_sha, worktree_path);
        partial.released = true; // no slot was taken
        rollback_preview_agent(state, &partial).await;
        return Err(ApiError::Internal(e));
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
            &[
                "push",
                repo_dir.to_str().unwrap(),
                "HEAD:refs/heads/main",
            ],
        );

        let state = crate::mem::test_state_with_preview_config(
            Arc::new(gyre_adapters::Git2OpsAdapter::new()),
            ttl_secs,
            max_concurrent,
        );
        let app = crate::api::api_router().with_state(state.clone());

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

        (app, state, repo.id.to_string())
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
        app.clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/meta-specs/preview")
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap()
    }

    async fn get_preview_status(app: &Router, preview_id: &str) -> axum::response::Response {
        app.clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/meta-specs/preview/{preview_id}"))
                    .header("authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap()
    }

    async fn delete_preview(app: &Router, preview_id: &str) -> axum::response::Response {
        app.clone()
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/api/v1/meta-specs/preview/{preview_id}"))
                    .header("authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap()
    }

    /// Run `f` with `GYRE_AGENT_COMMAND` pointed at `command`. The env var is
    /// process-global and read at launch time, so the lock serializes every
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
    fn env_dump_script(dir: &std::path::Path, env_out: &std::path::Path) -> std::path::PathBuf {
        let script = dir.join("agent-entrypoint.sh");
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\nenv > {out}\nexec sleep 300\n",
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
                preview_body(&repo_id, &["specs/system/search.md", "specs/system/identity.md"]),
            )
            .await;
            assert_eq!(resp.status(), StatusCode::ACCEPTED, "preview must be accepted");
            let json = body_json(resp).await;

            let preview_id = json["preview_id"].as_str().unwrap().to_string();
            assert!(!preview_id.is_empty());
            let agents = json["agents"].as_array().unwrap();
            assert_eq!(agents.len(), 2, "one agent per target: {json}");

            let mut branches: Vec<&str> = agents.iter().map(|a| a["branch"].as_str().unwrap()).collect();
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
                assert!(a["spec_path"].as_str().unwrap().starts_with("specs/system/"));
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
            // shell creates the redirect target before `env` writes it, so
            // poll until the dump has content, not merely exists.
            let env = soon(|| {
                std::fs::read_to_string(&env_out).ok().filter(|s| !s.is_empty())
            })
            .await;
            assert!(
                env.contains("GYRE_META_SPEC_DRAFT_KIND=meta:persona"),
                "draft kind must be injected; env was:\n{env}"
            );
            assert!(
                env.contains("DRAFT-PERSONA-BODY: write exhaustive tests"),
                "draft content must be injected; env was:\n{env}"
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
            assert!(provenance.is_empty(), "no provenance recording: {provenance:?}");
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
            assert!(usage.active_agents >= 2, "two preview agents must be counted");

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
            let resp = post_preview(&app, preview_body(&repo_id, &["specs/system/search.md"])).await;
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

            // Branch gone, worktree gone, kv records gone.
            assert!(!state.git_ops.branch_exists(&repo.path, &branch).await.unwrap());
            assert!(!std::path::Path::new(&wt_path).exists());
            assert!(
                state
                    .kv_store
                    .kv_get("meta_spec_previews", &preview_id)
                    .await
                    .unwrap()
                    .is_none()
            );
            assert!(
                state
                    .kv_store
                    .kv_get("preview_agents", &agent_id)
                    .await
                    .unwrap()
                    .is_none()
            );
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
            format!(
                "#!/bin/sh\n\
                 cd \"${{GYRE_WORK_DIR:-$PWD}}\" || cd /workspace || exit 0\n\
                 echo 'search v2 under draft persona' >> specs/system/search.md\n\
                 git add -A\n\
                 git -c user.email=agent@gyre -c user.name=agent commit -m 'preview: apply draft' >/dev/null 2>&1\n\
                 git push origin HEAD:refs/heads/\"$GYRE_BRANCH\" >/dev/null 2>&1\n\
                 exit 0\n"
            ),
        )
        .unwrap();
        make_executable(&script_path);

        with_agent_command(script_path.to_str().unwrap(), || async {
            let resp = post_preview(&app, preview_body(&repo_id, &["specs/system/search.md"])).await;
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
            assert!(!std::path::Path::new(&wt_path).exists(), "worktree removed on finish");
            assert!(
                state.git_ops.branch_exists(&repo.path, &branch).await.unwrap(),
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

        // Unknown repo.
        let resp = post_preview(
            &app,
            preview_body("00000000-0000-0000-0000-000000000000", &["specs/system/search.md"]),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);

        // Spec path that does not exist in the repo.
        let resp = post_preview(&app, preview_body(&repo_id, &["specs/system/does-not-exist.md"])).await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        // Read-only caller → 403 (no agents spawned, so no env lock needed).
        use crate::abac_middleware::seed_builtin_policies;
        use crate::auth::test_helpers::{make_test_state_with_jwt, sign_test_jwt};
        let jwt_state = make_test_state_with_jwt();
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(seed_builtin_policies(&jwt_state))
        });
        let ro_token = sign_test_jwt(
            &serde_json::json!({
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

    /// The separate preview budget cap: with `max_concurrent=1` pinned, a
    /// second concurrent spawn is rejected with 429.
    #[tokio::test(flavor = "multi_thread")]
    async fn preview_budget_cap_rejects_overflow() {
        let dir = tempfile::tempdir().unwrap();
        let (app, _state, repo_id) = preview_app(dir.path(), 86_400, Some(1)).await;

        let env_out = dir.path().join("agent-env.txt");
        let script = env_dump_script(dir.path(), &env_out);

        with_agent_command(script.to_str().unwrap(), || async {
            let resp = post_preview(&app, preview_body(&repo_id, &["specs/system/search.md"])).await;
            assert_eq!(resp.status(), StatusCode::ACCEPTED);
            let first = body_json(resp).await;
            let preview_id = first["preview_id"].as_str().unwrap().to_string();

            // One preview agent already running; the cap is 1.
            let resp = post_preview(&app, preview_body(&repo_id, &["specs/system/identity.md"])).await;
            assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);

            delete_preview(&app, &preview_id).await;
        })
        .await;
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
            let resp = post_preview(&app, preview_body(&repo_id, &["specs/system/search.md"])).await;
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
            let resp = post_preview(&app, preview_body(&repo_id, &["specs/system/identity.md"])).await;
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
// Registry authorization (registry routes run without middleware ABAC —
// frozen exemption list — so every handler enforces per-handler authz)
// ---------------------------------------------------------------------------

/// Role gate for registry mutations: creating, editing, approving, or deleting
/// a meta-spec rewrites the rules agents operate under — same authority class
/// as the workspace meta-spec-set binding (Admin-only, NEW-26).
fn require_registry_admin(auth: &AuthenticatedAgent) -> Result<(), ApiError> {
    if auth.roles.contains(&UserRole::Admin) {
        Ok(())
    } else {
        Err(ApiError::Forbidden(
            "only Admin role may modify the meta-spec registry".to_string(),
        ))
    }
}

/// Tenant containment for a Workspace-scoped meta-spec: the named workspace
/// must exist and belong to the caller's tenant (Admin bypasses). Global
/// scope crosses tenants by definition, so only the role gate applies to it.
/// A `scope_id` that does not resolve to a workspace is denied rather than
/// ridden on an assumed identity (AGENTS.md — never fabricate a scope).
async fn require_registry_scope(
    state: &AppState,
    auth: &AuthenticatedAgent,
    scope: &MetaSpecScope,
    scope_id: &Option<String>,
) -> Result<(), ApiError> {
    if matches!(scope, MetaSpecScope::Global) {
        return Ok(());
    }
    let Some(ws_id) = scope_id else {
        return Err(ApiError::BadRequest(
            "workspace-scoped meta-spec requires scope_id".to_string(),
        ));
    };
    match state.workspaces.find_by_id(&Id::new(ws_id)).await? {
        Some(ws) if ws.tenant_id.as_str() == auth.tenant_id || auth.roles.contains(&UserRole::Admin) => Ok(()),
        Some(_) => Err(ApiError::Forbidden(format!(
            "workspace-scoped meta-spec '{ws_id}' is outside the caller's tenant"
        ))),
        None => Err(ApiError::NotFound(format!("workspace '{ws_id}' not found"))),
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
// GET /api/v1/meta-specs-registry
// ---------------------------------------------------------------------------

pub async fn list_meta_specs_registry(
    State(state): State<Arc<AppState>>,
    auth: AuthenticatedAgent,
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
// POST /api/v1/meta-specs-registry
// ---------------------------------------------------------------------------

pub async fn create_meta_spec_registry(
    State(state): State<Arc<AppState>>,
    auth: AuthenticatedAgent,
    Json(req): Json<CreateMetaSpecRequest>,
) -> Result<(StatusCode, Json<MetaSpec>), ApiError> {
    require_registry_admin(&auth)?;
    let kind = parse_kind(&req.kind)?;
    let scope = parse_scope(&req.scope)?;
    // Scope containment: `scope_id` is caller-supplied, so it is validated
    // against the caller's tenant, never trusted.
    require_registry_scope(&state, &auth, &scope, &req.scope_id).await?;
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
    auth: AuthenticatedAgent,
    Path(id): Path<String>,
) -> Result<Json<MetaSpec>, ApiError> {
    let ms = require_meta_spec_access(&state, &auth, &id).await?;
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
    require_registry_admin(&auth)?;
    let mut ms = require_meta_spec_access(&state, &auth, &id).await?;

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
    auth: AuthenticatedAgent,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    require_registry_admin(&auth)?;
    // Load the entity, derive scope from the record (never the request), and
    // check tenant containment before deleting.
    require_meta_spec_access(&state, &auth, &id).await?;
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
// GET /api/v1/meta-specs-registry/:id/versions/:version
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
            ApiError::NotFound(format!(
                "version {version} of meta-spec '{id}' not found"
            ))
        })?;
    Ok(Json(ver))
}
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
            .mint(&agent_id, "task-x", "system", &state.base_url, 300)
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
                "/api/v1/meta-specs-registry",
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
                "/api/v1/meta-specs-registry/00000000-0000-0000-0000-000000000000",
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
                "/api/v1/meta-specs-registry/00000000-0000-0000-0000-000000000000",
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
