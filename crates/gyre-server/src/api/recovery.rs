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
    // Fail CLOSED (task-095 R3-F3): when the HEAD ref does not resolve there
    // is no tree to validate — report `main_green: false` with the failure in
    // `pause_reason` context, never a green reading from an unvalidated tree.
    let main_green = if gates.is_empty() {
        None
    } else {
        match crate::git_refs::resolve_ref(
            &repo.path,
            &format!("refs/heads/{}", repo.default_branch),
        )
        .await
        {
            Some(head_sha) => Some(
                crate::gate_executor::run_post_merge_gates(&state, &repo, &head_sha)
                    .await
                    .is_ok(),
            ),
            None => Some(false),
        }
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

    // The MR must belong to the repo in the path: a mismatch would revert
    // one repo's history while marking another repo's MR (task-095 R3-F1).
    if mr.repository_id != repo.id {
        return Err(ApiError::Forbidden(format!(
            "merge request {mr_id} does not belong to repo {repo_id}"
        )));
    }

    // Revert the MR's OWN merge commit, recorded at merge time (task-095
    // R3-F1). The current target-branch HEAD is NOT the MR's merge commit
    // once any later merge or revert landed.
    let Some(merge_sha) = mr.merge_commit_sha.clone() else {
        return Err(ApiError::Conflict(format!(
            "merge request {mr_id} has no recorded merge commit (merged before task-095 R3-F1?) — \
             cannot identify which commit to revert"
        )));
    };

    let revert_commit_sha = state
        .git_ops
        .revert_commit(&repo.path, &repo.default_branch, &merge_sha)
        .await
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("revert failed: {e}")))?;

    // Manual reverts count toward the circuit breaker (task-095 R3-F1):
    // the breaker keyed on the resubmission-stable key fires after 3
    // reverts of the same work no matter who initiated them.
    if let Ok(revert_count) =
        crate::merge_processor::increment_revert_count(&state, &mr).await
    {
        if revert_count >= 3 {
            if let Err(e) =
                crate::merge_processor::trip_circuit_breaker(&state, repo.id.as_str(), &mr)
                    .await
            {
                tracing::error!(mr_id = %mr.id, error = %e, "failed to trip circuit breaker");
            }
        }
    }

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
        };
        state.quality_gates.save(&gate).await?;
    }

    let gates = post_merge_gates(&state, &repo_id).await?;

    Ok((StatusCode::OK, Json(PostMergeGatesResponse { repo_id, gates })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mem::{test_state, test_state_with_git_ops, ConfigurableGitOps};
    use axum::body::Body;
    use axum::Router;
    use http::{Request, StatusCode};
    use gyre_domain::{MergeQueueEntry, MergeQueueEntryStatus, MergeRequest, Repository};
    use std::sync::Arc;
    use tower::ServiceExt;

    fn app(state: Arc<AppState>) -> Router {
        crate::api::api_router().with_state(state)
    }

    async fn body_json(resp: axum::response::Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    async fn seed_repo(state: &Arc<AppState>, name: &str) -> Repository {
        let repo = Repository::new(
            Id::new(uuid::Uuid::new_v4().to_string()),
            Id::new("ws-1"),
            name,
            format!("/tmp/gyre-recovery-tests/{name}.git"),
            0,
        );
        state.repos.create(&repo).await.unwrap();
        repo
    }

    /// MR driven to `Merged` through the real state machine, with an
    /// optional recorded merge commit (task-095 R3-F1).
    async fn seed_merged_mr(
        state: &Arc<AppState>,
        repo_id: &Id,
        mr_id: &str,
        merge_sha: Option<&str>,
    ) -> MergeRequest {
        let mut mr = MergeRequest::new(
            Id::new(mr_id),
            repo_id.clone(),
            "Merged MR",
            "feat/x",
            "main",
            1000,
        );
        mr.workspace_id = Id::new("ws-1");
        mr.transition_status(MrStatus::Approved).unwrap();
        mr.transition_status(MrStatus::Merged).unwrap();
        mr.merge_commit_sha = merge_sha.map(|s| s.to_string());
        state.merge_requests.create(&mr).await.unwrap();
        mr
    }

    async fn post_revert(app: &Router, repo_id: &str, mr_id: &str) -> axum::response::Response {
        app.clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/repos/{repo_id}/revert/{mr_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap()
    }

    /// R3-F1: the manual revert must revert the MR's OWN recorded merge
    /// commit — not whatever the target branch HEAD happens to be after
    /// later merges landed.
    #[tokio::test]
    async fn manual_revert_reverts_recorded_merge_commit() {
        let git = Arc::new(ConfigurableGitOps::default());
        let recorded = git.revert_calls.clone();
        let state = test_state_with_git_ops(git);
        let repo = seed_repo(&state, "revert-sha").await;
        seed_merged_mr(&state, &repo.id, "mr-sha", Some("a1b2c3d4e5f60718293a4b5c6d7e8f9011223344")).await;

        let resp = post_revert(&app(state.clone()), repo.id.as_str(), "mr-sha").await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_json(resp).await;
        assert_eq!(
            body["revert_commit_sha"],
            "revert-of-a1b2c3d4e5f60718293a4b5c6d7e8f9011223344"
        );
        // The double saw exactly the MR's merge commit as the revert target.
        assert_eq!(
            *recorded.lock(),
            vec!["a1b2c3d4e5f60718293a4b5c6d7e8f9011223344".to_string()],
            "revert_commit must be called with the recorded merge sha"
        );

        // §6 step 4: side effects applied — MR marked Reverted.
        let mr = state
            .merge_requests
            .find_by_id(&Id::new("mr-sha"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(mr.status, MrStatus::Reverted);
        assert_eq!(
            mr.revert_commit_sha.as_deref(),
            Some("revert-of-a1b2c3d4e5f60718293a4b5c6d7e8f9011223344")
        );
    }

    /// R3-F1: without a recorded merge commit there is no valid commit to
    /// revert — reject explicitly instead of reverting a guessed HEAD.
    #[tokio::test]
    async fn manual_revert_without_recorded_merge_commit_is_conflict() {
        let git = Arc::new(ConfigurableGitOps::default());
        let recorded = git.revert_calls.clone();
        let state = test_state_with_git_ops(git);
        let repo = seed_repo(&state, "revert-nosha").await;
        seed_merged_mr(&state, &repo.id, "mr-nosha", None).await;

        let resp = post_revert(&app(state.clone()), repo.id.as_str(), "mr-nosha").await;
        assert_eq!(resp.status(), StatusCode::CONFLICT);
        assert!(recorded.lock().is_empty(), "no revert may be attempted");
    }

    /// R3-F1: an MR belonging to another repo must not be revertable
    /// through this repo's path (would revert repo A's history while
    /// marking repo B's MR).
    #[tokio::test]
    async fn manual_revert_rejects_foreign_repo_mr() {
        let git = Arc::new(ConfigurableGitOps::default());
        let recorded = git.revert_calls.clone();
        let state = test_state_with_git_ops(git);
        let repo_a = seed_repo(&state, "revert-a").await;
        let repo_b = seed_repo(&state, "revert-b").await;
        seed_merged_mr(&state, &repo_a.id, "mr-foreign", Some("aabbccddeeff00112233445566778899aabbccdd")).await;

        let resp = post_revert(&app(state.clone()), repo_b.id.as_str(), "mr-foreign").await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
        assert!(recorded.lock().is_empty(), "no revert may be attempted");
    }

    /// R3-F1: manual reverts count toward the circuit breaker. Previously
    /// the breaker only incremented in the automatic recovery path, so 3
    /// manual reverts never tripped it.
    #[tokio::test]
    async fn manual_revert_counts_toward_circuit_breaker() {
        let git = Arc::new(ConfigurableGitOps::default());
        let state = test_state_with_git_ops(git);
        let repo = seed_repo(&state, "revert-breaker").await;
        let mr = seed_merged_mr(&state, &repo.id, "mr-brk", Some("deadbeefdeadbeefdeadbeefdeadbeefdeadbeef")).await;

        // Two prior reverts of the same work already counted (namespace
        // literal mirrors merge_processor::REVERT_COUNTS_NS).
        state
            .kv_store
            .kv_set("revert_counts", "mr:mr-brk", "2".to_string())
            .await
            .unwrap();
        // A queue entry already waiting — a trip must remove it.
        state
            .merge_queue
            .enqueue(&MergeQueueEntry::new(
                Id::new("entry-brk"),
                mr.id.clone(),
                100,
                1000,
            ))
            .await
            .unwrap();

        let resp = post_revert(&app(state.clone()), repo.id.as_str(), "mr-brk").await;
        assert_eq!(resp.status(), StatusCode::OK);

        // Third revert → breaker tripped: queue entry removed permanently…
        let entry = state
            .merge_queue
            .find_by_id(&Id::new("entry-brk"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            entry.status,
            MergeQueueEntryStatus::Cancelled,
            "breaker trip must remove the MR's queue entries"
        );
        // …and a Critical human escalation exists.
        let tasks = state.tasks.list_by_repo(&repo.id).await.unwrap();
        assert!(
            tasks
                .iter()
                .any(|t| t.priority == gyre_domain::TaskPriority::Critical
                    && t.labels.contains(&"circuit-breaker".to_string())),
            "manual 3rd revert must create the circuit-breaker escalation task, got {:?}",
            tasks.iter().map(|t| &t.title).collect::<Vec<_>>()
        );
    }

    /// R3-F3: main_green is the boolean a human uses to decide whether to
    /// resume the queue. When the HEAD ref cannot be resolved there is no
    /// tree to validate — it must read `false`, never a green default from
    /// the server-cwd gate fallback.
    #[tokio::test]
    async fn repo_status_main_green_fails_closed_when_head_unresolvable() {
        let state = test_state();
        let dir = tempfile::tempdir().unwrap();
        // A plain directory — NOT a git repo — so refs/heads/main cannot
        // resolve, while a gate would still "pass" in the server cwd.
        let path = dir.path().join("unresolvable.git");
        std::fs::create_dir_all(&path).unwrap();

        let repo = Repository::new(
            Id::new(uuid::Uuid::new_v4().to_string()),
            Id::new("ws-1"),
            "unresolvable",
            path.to_str().unwrap(),
            0,
        );
        state.repos.create(&repo).await.unwrap();

        // A gate whose command always exits 0: if it ever RAN (server-cwd
        // fallback), main would read green.
        state
            .quality_gates
            .save(&QualityGate {
                id: Id::new("gate-failclosed"),
                repo_id: repo.id.clone(),
                name: "always-pass".to_string(),
                gate_type: GateType::TestCommand,
                command: Some("true".to_string()),
                required_approvals: None,
                persona: None,
                required: true,
                gate_phase: GatePhase::PostMerge,
                timeout_secs: Some(10),
                created_at: 1000,
            })
            .await
            .unwrap();

        let resp = app(state.clone())
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/api/v1/repos/{}/status", repo.id))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_json(resp).await;
        assert_eq!(
            body["main_green"],
            serde_json::json!(false),
            "unresolvable HEAD must report main_green: false (fail closed), got {body}"
        );
    }
}
