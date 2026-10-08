//! Post-merge recovery protocol endpoints (platform-model.md §6).
//!
//! GET  /api/v1/repos/:id/status            — main health + queue pause state
//! PUT  /api/v1/repos/:id/queue/pause       — manual merge queue pause
//! PUT  /api/v1/repos/:id/queue/resume      — manual merge queue resume
//! POST /api/v1/repos/:id/revert/:mr_id     — manual revert of a merged MR
//! GET  /api/v1/repos/:id/post-merge-gates  — list post-merge gate configs
//! PUT  /api/v1/repos/:id/post-merge-gates  — replace post-merge gate configs
//!
//! Post-merge gates live in the `quality_gates` store with
//! `gate_phase = PostMerge` (same schema as pre-merge gates; see
//! `gate_executor::run_post_merge_gates`).

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use gyre_common::Id;
use gyre_domain::{GatePhase, GateType, MrStatus, QualityGate};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::AppState;

use super::error::ApiError;

/// KV namespace for merge queue pause state (owned by merge_processor).
const MERGE_QUEUE_PAUSE_NS: &str = "merge_queue_pause";

// ---------------------------------------------------------------------------
// Request / Response
// ---------------------------------------------------------------------------

#[derive(Deserialize, Default)]
pub struct PauseRequest {
    /// Human-readable reason shown in events and notifications.
    pub reason: Option<String>,
}

#[derive(Serialize)]
pub struct QueueStatusResponse {
    pub repo_id: String,
    /// Whether the repo's merge queue is paused (broken main).
    pub queue_paused: bool,
    /// Pause reason, if paused.
    pub pause_reason: Option<String>,
    /// Whether the repo's default branch HEAD passes post-merge validation.
    /// None when no post-merge gates are configured.
    pub main_green: Option<bool>,
    /// Post-merge gate configs currently set for this repo.
    pub post_merge_gates: Vec<PostMergeGateDto>,
}

#[derive(Serialize)]
pub struct RevertResponse {
    pub repo_id: String,
    pub mr_id: String,
    pub revert_commit_sha: String,
}

/// Serializable view of a post-merge gate (`QualityGate` with
/// `gate_phase = PostMerge`).
#[derive(Serialize, Deserialize, Clone)]
pub struct PostMergeGateDto {
    pub name: String,
    pub gate_type: String,
    pub command: String,
    /// When false, a failing gate is advisory only — it does not trigger
    /// the recovery protocol.
    pub required: bool,
    /// Command timeout in seconds.
    pub timeout_secs: Option<u64>,
}

#[derive(Deserialize)]
pub struct SetPostMergeGatesRequest {
    /// Full post-merge gate config list (replaces existing).
    pub gates: Vec<PostMergeGateDto>,
}

#[derive(Serialize)]
pub struct PostMergeGatesResponse {
    pub repo_id: String,
    pub gates: Vec<PostMergeGateDto>,
}

fn gate_type_from_str(s: &str) -> Option<GateType> {
    Some(match s {
        "test_command" => GateType::TestCommand,
        "lint_command" => GateType::LintCommand,
        other => {
            if other.is_empty() {
                GateType::TestCommand
            } else {
                return None;
            }
        }
    })
}

fn gate_type_to_str(t: &GateType) -> &'static str {
    match t {
        GateType::TestCommand => "test_command",
        GateType::LintCommand => "lint_command",
        GateType::RequiredApprovals => "required_approvals",
        GateType::AgentReview => "agent_review",
        GateType::AgentValidation => "agent_validation",
        GateType::TraceCapture => "trace_capture",
    }
}

async fn post_merge_gates(state: &AppState, repo_id: &str) -> Result<Vec<PostMergeGateDto>, ApiError> {
    let gates = state
        .quality_gates
        .list_by_repo_id_and_phase(repo_id, GatePhase::PostMerge)
        .await?;
    Ok(gates
        .into_iter()
        .map(|g| PostMergeGateDto {
            name: g.name,
            gate_type: gate_type_to_str(&g.gate_type).to_string(),
            command: g.command.unwrap_or_default(),
            required: g.required,
            timeout_secs: g.timeout_secs,
        })
        .collect())
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// PUT /api/v1/repos/:id/queue/pause — manually pause the repo's merge queue.
pub async fn pause_queue(
    State(state): State<Arc<AppState>>,
    Path(repo_id): Path<String>,
    body: Option<Json<PauseRequest>>,
) -> Result<Json<QueueStatusResponse>, ApiError> {
    let repo = state
        .repos
        .find_by_id(&Id::new(&repo_id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("repo {repo_id} not found")))?;

    let reason = body
        .and_then(|Json(req)| req.reason)
        .unwrap_or_else(|| "manually paused".to_string());

    crate::merge_processor::pause_merge_queue(&state, &repo, &reason).await;

    let gates = post_merge_gates(&state, &repo_id).await?;

    Ok(Json(QueueStatusResponse {
        repo_id,
        queue_paused: true,
        pause_reason: Some(reason),
        main_green: None,
        post_merge_gates: gates,
    }))
}

/// PUT /api/v1/repos/:id/queue/resume — manually resume the repo's merge queue.
pub async fn resume_queue(
    State(state): State<Arc<AppState>>,
    Path(repo_id): Path<String>,
) -> Result<Json<QueueStatusResponse>, ApiError> {
    let repo = state
        .repos
        .find_by_id(&Id::new(&repo_id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("repo {repo_id} not found")))?;

    crate::merge_processor::resume_merge_queue(&state, &repo).await;

    let gates = post_merge_gates(&state, &repo_id).await?;

    Ok(Json(QueueStatusResponse {
        repo_id,
        queue_paused: false,
        pause_reason: None,
        main_green: None,
        post_merge_gates: gates,
    }))
}

/// GET /api/v1/repos/:id/status — main health and merge queue state.
///
/// Runs post-merge validation against the default branch HEAD to compute
/// `main_green` (None when no gates are configured).
pub async fn repo_status(
    State(state): State<Arc<AppState>>,
    Path(repo_id): Path<String>,
) -> Result<Json<QueueStatusResponse>, ApiError> {
    let repo = state
        .repos
        .find_by_id(&Id::new(&repo_id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("repo {repo_id} not found")))?;

    // Pause state from kv_store ({"paused": bool, "reason": string}).
    let paused_state = state
        .kv_store
        .kv_get(MERGE_QUEUE_PAUSE_NS, &repo_id)
        .await?
        .and_then(|v| serde_json::from_str::<serde_json::Value>(&v).ok());
    let queue_paused = paused_state
        .as_ref()
        .and_then(|v| v.get("paused"))
        .and_then(|p| p.as_bool())
        .unwrap_or(false);
    let pause_reason = paused_state
        .as_ref()
        .and_then(|v| v.get("reason"))
        .and_then(|r| r.as_str())
        .map(|s| s.to_string());

    let gates = post_merge_gates(&state, &repo_id).await?;

    // main_green: run post-merge gates against current default-branch HEAD.
    let main_green = if gates.is_empty() {
        None
    } else {
        let head_sha = crate::git_refs::resolve_ref(
            &repo.path,
            &format!("refs/heads/{}", repo.default_branch),
        )
        .await
        .unwrap_or_default();
        Some(
            crate::gate_executor::run_post_merge_gates(&state, &repo, &head_sha)
                .await
                .is_ok(),
        )
    };

    Ok(Json(QueueStatusResponse {
        repo_id,
        queue_paused,
        pause_reason,
        main_green,
        post_merge_gates: gates,
    }))
}

/// POST /api/v1/repos/:id/revert/:mr_id — manual revert of a merged MR.
///
/// Reverts the MR's merge commit on the target branch and applies the
/// recovery side effects (mark Reverted, notify author, remediation task,
/// invalidate gate results — platform-model.md §6 steps 2, 4–7).
pub async fn revert_mr(
    State(state): State<Arc<AppState>>,
    Path((repo_id, mr_id)): Path<(String, String)>,
) -> Result<(StatusCode, Json<RevertResponse>), ApiError> {
    let repo = state
        .repos
        .find_by_id(&Id::new(&repo_id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("repo {repo_id} not found")))?;

    let mr = state
        .merge_requests
        .find_by_id(&Id::new(&mr_id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("merge request {mr_id} not found")))?;

    if mr.status != MrStatus::Merged {
        return Err(ApiError::InvalidInput(format!(
            "merge request {mr_id} is not Merged (status: {:?})",
            mr.status
        )));
    }

    // Resolve the merge commit: the target branch HEAD (the merge landed it).
    let merge_sha = crate::git_refs::resolve_ref(
        &repo.path,
        &format!("refs/heads/{}", mr.target_branch),
    )
    .await
    .unwrap_or_default();

    let revert_commit_sha = state
        .git_ops
        .revert_commit(&repo.path, &repo.default_branch, &merge_sha)
        .await
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("revert failed: {e}")))?;

    // Steps 4–7 of the recovery protocol (mark Reverted, RevertNotification,
    // remediation task, gate-result invalidation).
    crate::merge_processor::apply_revert_side_effects(
        &state,
        &repo,
        &mr,
        &merge_sha,
        &revert_commit_sha,
        "manual revert",
    )
    .await;

    Ok((
        StatusCode::OK,
        Json(RevertResponse {
            repo_id,
            mr_id,
            revert_commit_sha,
        }),
    ))
}

/// GET /api/v1/repos/:id/post-merge-gates — list post-merge gate configs.
pub async fn get_post_merge_gates(
    State(state): State<Arc<AppState>>,
    Path(repo_id): Path<String>,
) -> Result<Json<PostMergeGatesResponse>, ApiError> {
    state
        .repos
        .find_by_id(&Id::new(&repo_id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("repo {repo_id} not found")))?;

    let gates = post_merge_gates(&state, &repo_id).await?;

    Ok(Json(PostMergeGatesResponse { repo_id, gates }))
}

/// PUT /api/v1/repos/:id/post-merge-gates — replace post-merge gate configs.
pub async fn set_post_merge_gates(
    State(state): State<Arc<AppState>>,
    Path(repo_id): Path<String>,
    Json(req): Json<SetPostMergeGatesRequest>,
) -> Result<(StatusCode, Json<PostMergeGatesResponse>), ApiError> {
    state
        .repos
        .find_by_id(&Id::new(&repo_id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("repo {repo_id} not found")))?;

    // Validate configs: non-empty name and command.
    for gate in &req.gates {
        if gate.name.trim().is_empty() || gate.command.trim().is_empty() {
            return Err(ApiError::InvalidInput(
                "post-merge gate name and command must be non-empty".to_string(),
            ));
        }
    }

    let now = super::now_secs();

    // Replace-all semantics: delete existing post-merge gates for the repo,
    // then save the new list as QualityGates with gate_phase = PostMerge.
    let existing = state
        .quality_gates
        .list_by_repo_id_and_phase(&repo_id, GatePhase::PostMerge)
        .await?;
    for old in &existing {
        state.quality_gates.delete(old.id.as_str()).await?;
    }

    for (i, dto) in req.gates.iter().enumerate() {
        let gate_type = gate_type_from_str(&dto.gate_type).ok_or_else(|| {
            ApiError::InvalidInput(format!(
                "post-merge gate {} has unsupported gate_type '{}' (command gates only)",
                dto.name, dto.gate_type
            ))
        })?;
        let gate = QualityGate {
            id: Id::new(format!("pm-gate-{repo_id}-{i}")),
            repo_id: Id::new(repo_id.clone()),
            name: dto.name.clone(),
            gate_type,
            command: Some(dto.command.clone()),
            required_approvals: None,
            persona: None,
            required: dto.required,
            gate_phase: GatePhase::PostMerge,
            timeout_secs: dto.timeout_secs,
            created_at: now,
            position: 0,
        };
        state.quality_gates.save(&gate).await?;
    }

    let gates = post_merge_gates(&state, &repo_id).await?;

    Ok((StatusCode::OK, Json(PostMergeGatesResponse { repo_id, gates })))
}
