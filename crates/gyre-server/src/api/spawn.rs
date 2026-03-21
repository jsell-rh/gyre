use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use gyre_common::Id;
use gyre_domain::{Agent, AgentStatus, AgentWorktree, AnalyticsEvent, MergeRequest, TaskStatus};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::instrument;

use crate::{auth::AuthenticatedAgent, git_refs, workload_attestation, AppState};

use super::agents::AgentResponse;
use super::error::ApiError;
use super::merge_requests::MrResponse;
use super::{new_id, now_secs};

// ── Request / Response types ──────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct SpawnAgentRequest {
    pub name: String,
    pub repo_id: String,
    pub task_id: String,
    pub branch: String,
    pub parent_id: Option<String>,
    /// Optional compute target to associate with this agent spawn.
    pub compute_target_id: Option<String>,
}

#[derive(Serialize)]
pub struct SpawnAgentResponse {
    pub agent: AgentResponse,
    pub token: String,
    pub worktree_path: String,
    pub clone_url: String,
    pub branch: String,
    pub compute_target_id: Option<String>,
    /// jj change ID created for this agent's worktree, if jj was successfully initialized.
    pub jj_change_id: Option<String>,
}

#[derive(Deserialize)]
pub struct CompleteAgentRequest {
    pub branch: String,
    pub title: String,
    pub target_branch: String,
}

// ── Handlers ──────────────────────────────────────────────────────────────────

/// POST /api/v1/agents/spawn
///
/// Orchestrated agent provisioning in one call:
/// 1. Creates agent record (Active status, sets parent_id)
/// 2. Generates auth token
/// 3. Creates a git worktree on the repo for the agent's branch
/// 4. Assigns the task to the agent, advances task to InProgress
/// 5. Records the worktree in DB (linked to agent + task)
#[instrument(skip(state, auth, req), fields(agent_name = %req.name, branch = %req.branch))]
pub async fn spawn_agent(
    State(state): State<Arc<AppState>>,
    auth: AuthenticatedAgent,
    Json(req): Json<SpawnAgentRequest>,
) -> Result<(StatusCode, Json<SpawnAgentResponse>), ApiError> {
    // Verify repo exists
    let repo = state
        .repos
        .find_by_id(&Id::new(&req.repo_id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("repo {} not found", req.repo_id)))?;

    // G6: ABAC enforcement — check repo access policies against the caller's JWT claims.
    crate::abac::check_repo_abac(&state, &req.repo_id, &auth)
        .await
        .map_err(ApiError::Forbidden)?;

    // Verify task exists
    let mut task = state
        .tasks
        .find_by_id(&Id::new(&req.task_id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("task {} not found", req.task_id)))?;

    // Validate compute target if provided
    if let Some(ref ct_id) = req.compute_target_id {
        let store = state.compute_targets.lock().await;
        if !store.contains_key(ct_id.as_str()) {
            return Err(ApiError::NotFound(format!(
                "compute target {ct_id} not found"
            )));
        }
    }

    let now = now_secs();

    // Create agent with Active status
    let mut agent = Agent::new(new_id(), req.name, now);
    agent.parent_id = req.parent_id.map(Id::new);
    agent.spawned_by = Some(auth.agent_id.clone());
    agent.assign_task(Id::new(&req.task_id));
    agent
        .transition_status(AgentStatus::Active)
        .map_err(|e| ApiError::InvalidInput(e.to_string()))?;
    state.agents.create(&agent).await?;

    // JWT minting is deferred until after process spawn so workload claims
    // (PID, hostname, compute target) can be embedded.  We declare `token`
    // here and populate it below after the spawn block.
    // Using a temporary placeholder that will be replaced before the function returns.
    let token_placeholder = String::new();

    // Compute worktree path: {repo_path}/worktrees/{branch_slug}
    let branch_slug = req.branch.replace('/', "-");
    let worktree_path = format!("{}/worktrees/{}", repo.path, branch_slug);

    // Create git worktree (best effort -- failure is logged, not fatal)
    if let Err(e) = state
        .git_ops
        .create_worktree(&repo.path, &worktree_path, &req.branch)
        .await
    {
        tracing::warn!("create_worktree failed: {e}");
    }

    // Initialize jj in the worktree and create an initial change (best-effort).
    // Only attempted if the worktree directory exists on disk.
    let jj_change_id = if std::path::Path::new(&worktree_path).exists() {
        match state.jj_ops.jj_init(&worktree_path).await {
            Ok(()) => {
                let description = format!("Agent {}: task {}", agent.name, req.task_id);
                match state.jj_ops.jj_new(&worktree_path, &description).await {
                    Ok(change_id) => {
                        tracing::debug!(
                            agent_id = %agent.id,
                            change_id = %change_id,
                            "jj initialized in worktree"
                        );
                        Some(change_id)
                    }
                    Err(e) => {
                        tracing::debug!(agent_id = %agent.id, "jj new skipped: {e}");
                        None
                    }
                }
            }
            Err(e) => {
                tracing::debug!(agent_id = %agent.id, "jj init skipped: {e}");
                None
            }
        }
    } else {
        None
    };

    // Write custom ref namespaces (best-effort)
    if let Some(sha) = git_refs::resolve_ref(&repo.path, "HEAD").await {
        let agent_ref = format!("refs/agents/{}/head", agent.id);
        let ralph_ref = format!("refs/ralph/{}/implement", task.id);
        git_refs::write_ref(&repo.path, &agent_ref, &sha).await;
        git_refs::write_ref(&repo.path, &ralph_ref, &sha).await;
    }

    // Record worktree in DB linked to agent and task
    let wt = AgentWorktree::new(
        new_id(),
        agent.id.clone(),
        Id::new(&req.repo_id),
        Some(Id::new(&req.task_id)),
        req.branch.clone(),
        worktree_path.clone(),
        now,
    );
    state.worktrees.create(&wt).await?;

    // Assign task to agent and advance to InProgress
    task.assigned_to = Some(agent.id.clone());
    if task.status == TaskStatus::Backlog {
        let _ = task.transition_status(TaskStatus::InProgress);
    }
    task.updated_at = now;
    state.tasks.update(&task).await?;

    // Build clone URL: {base_url}/git/{project_id}/{repo_name}
    let clone_url = format!("{}/git/{}/{}", state.base_url, repo.project_id, repo.name);

    // Launch a real process via LocalTarget and monitor its lifecycle.
    // Capture the PID so we can embed it in the JWT and workload attestation.
    let spawned_pid: Option<u32>;
    let compute_target_label = req.compute_target_id.as_deref().unwrap_or("local");
    {
        // Command is server-controlled only — never from user input (C-1 RCE fix).
        let command = "echo".to_string();
        let args = vec![format!("Agent {} started", agent.id)];
        let effective_work_dir = if std::path::Path::new(&worktree_path).exists() {
            worktree_path.clone()
        } else {
            ".".to_string()
        };
        let spawn_config = gyre_ports::SpawnConfig {
            name: agent.name.clone(),
            command,
            args,
            env: std::collections::HashMap::new(),
            work_dir: effective_work_dir,
        };
        let local = gyre_adapters::compute::LocalTarget;
        match gyre_ports::ComputeTarget::spawn_process(&local, &spawn_config).await {
            Ok(handle) => {
                spawned_pid = handle.pid;
                let agent_id_str = agent.id.to_string();
                state
                    .process_registry
                    .lock()
                    .await
                    .insert(agent_id_str.clone(), handle.clone());

                // Background monitor: watch for process exit and update agent status.
                let state_mon = Arc::clone(&state);
                tokio::spawn(async move {
                    loop {
                        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                        let alive = gyre_ports::ComputeTarget::is_alive(
                            &gyre_adapters::compute::LocalTarget,
                            &handle,
                        )
                        .await
                        .unwrap_or(false);
                        if !alive {
                            state_mon
                                .process_registry
                                .lock()
                                .await
                                .remove(&agent_id_str);
                            if let Ok(Some(mut a)) =
                                state_mon.agents.find_by_id(&Id::new(&agent_id_str)).await
                            {
                                if a.status == AgentStatus::Active {
                                    let _ = a.transition_status(AgentStatus::Idle);
                                    let _ = state_mon.agents.update(&a).await;
                                }
                            }
                            break;
                        }
                    }
                });
            }
            Err(e) => {
                spawned_pid = None;
                tracing::warn!(agent_id = %agent.id, "process spawn failed (best-effort): {e}");
            }
        }
    }

    // G10: Create workload attestation now that we know the PID.
    let att = {
        // Retrieve the stack hash recorded by the agent (M14.1), if any.
        let stack_hash = {
            let stacks = state.agent_stacks.lock().await;
            stacks
                .get(&agent.id.to_string())
                .map(|s| s.fingerprint())
                .unwrap_or_default()
        };
        workload_attestation::attest_agent(
            &agent.id.to_string(),
            spawned_pid,
            compute_target_label,
            &stack_hash,
        )
    };
    let wl_hostname = Some(att.hostname.clone());
    let wl_compute_target = Some(att.compute_target.clone());
    let wl_stack_hash = if att.stack_fingerprint.is_empty() {
        None
    } else {
        Some(att.stack_fingerprint.clone())
    };
    state
        .workload_attestations
        .lock()
        .await
        .insert(agent.id.to_string(), att);

    // Mint a signed EdDSA JWT as the agent's auth token (M18 + G10).
    // Embeds workload attestation claims so external verifiers can reconstruct
    // workload identity from the JWT alone without calling the server.
    // Falls back to a UUID if JWT minting fails (defensive).
    let _ = token_placeholder; // consumed
    let token = state
        .agent_signing_key
        .mint_with_workload(
            &agent.id.to_string(),
            &task.id.to_string(),
            &auth.agent_id,
            &state.base_url,
            state.agent_jwt_ttl_secs,
            spawned_pid,
            wl_hostname,
            wl_compute_target,
            wl_stack_hash,
        )
        .unwrap_or_else(|e| {
            tracing::error!("JWT minting failed, falling back to UUID token: {e}");
            uuid::Uuid::new_v4().to_string()
        });
    state
        .agent_tokens
        .lock()
        .await
        .insert(agent.id.to_string(), token.clone());

    // Auto-track agent spawn
    let ev = AnalyticsEvent::new(
        new_id(),
        "agent.spawned",
        Some(agent.id.to_string()),
        serde_json::json!({ "task_id": req.task_id }),
        now,
    );
    let _ = state.analytics.record(&ev).await;

    Ok((
        StatusCode::CREATED,
        Json(SpawnAgentResponse {
            agent: AgentResponse::from(agent),
            token,
            worktree_path,
            clone_url,
            branch: req.branch,
            compute_target_id: req.compute_target_id,
            jj_change_id,
        }),
    ))
}

/// POST /api/v1/agents/{id}/complete
///
/// Agent signals it has finished its task:
/// 1. Creates a MergeRequest (source->target)
/// 2. Transitions task status to Review
/// 3. Transitions agent status to Idle
#[instrument(skip(state, req), fields(agent_id = %id, branch = %req.branch))]
pub async fn complete_agent(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<CompleteAgentRequest>,
) -> Result<(StatusCode, Json<MrResponse>), ApiError> {
    let mut agent = state
        .agents
        .find_by_id(&Id::new(&id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("agent {id} not found")))?;

    // Get repository_id from agent's worktree
    let worktrees = state.worktrees.find_by_agent(&agent.id).await?;
    let repo_id = worktrees
        .first()
        .map(|wt| wt.repository_id.clone())
        .ok_or_else(|| ApiError::InvalidInput("agent has no associated worktree".to_string()))?;

    // Idempotent complete: if an MR already exists for this agent+branch, return 202.
    {
        let existing_mrs = state.merge_requests.list_by_repo(&repo_id).await?;
        let found = existing_mrs.into_iter().find(|m| {
            m.source_branch == req.branch
                && m.author_agent_id.as_ref() == Some(&agent.id)
                && m.status != gyre_domain::MrStatus::Closed
        });
        if let Some(existing) = found {
            return Ok((StatusCode::ACCEPTED, Json(MrResponse::from(existing))));
        }
    }

    let now = now_secs();

    // Create MergeRequest
    let mut mr = MergeRequest::new(
        new_id(),
        repo_id,
        req.title,
        req.branch,
        req.target_branch,
        now,
    );
    mr.author_agent_id = Some(agent.id.clone());
    state.merge_requests.create(&mr).await?;

    // Transition task to Review (navigate through intermediate states as needed)
    if let Some(task_id) = &agent.current_task_id {
        if let Ok(Some(mut task)) = state.tasks.find_by_id(task_id).await {
            if task.status == TaskStatus::Backlog {
                let _ = task.transition_status(TaskStatus::InProgress);
            }
            if task.status == TaskStatus::Blocked {
                let _ = task.transition_status(TaskStatus::InProgress);
            }
            let _ = task.transition_status(TaskStatus::Review);
            task.updated_at = now;
            let _ = state.tasks.update(&task).await;
        }
    }

    // Transition agent to Idle
    let _ = agent.transition_status(AgentStatus::Idle);
    state.agents.update(&agent).await?;

    // Revoke the agent's token — completed agents must not continue to authenticate (N-1).
    state.agent_tokens.lock().await.remove(&id);

    // Create a jj bookmark for the agent's branch in their worktree (best-effort).
    // This persists the branch tip in jj's bookmark namespace for traceability.
    if let Some(wt) = worktrees.first() {
        if std::path::Path::new(&wt.path).exists() {
            if let Err(e) = state
                .jj_ops
                .jj_bookmark_create(&wt.path, &mr.source_branch, "@")
                .await
            {
                tracing::debug!(agent_id = %agent.id, "jj bookmark skipped: {e}");
            } else {
                tracing::debug!(
                    agent_id = %agent.id,
                    branch = %mr.source_branch,
                    "jj bookmark created on complete"
                );
            }
        }
    }

    // Write snapshot ref for this agent (best-effort)
    if let Ok(Some(repo)) = state.repos.find_by_id(&mr.repository_id).await {
        let snap_prefix = format!("refs/agents/{}/snapshots/", agent.id);
        let n = git_refs::count_refs_under(&repo.path, &snap_prefix).await;
        let snap_ref = format!("refs/agents/{}/snapshots/{}", agent.id, n);
        let branch_ref = format!("refs/heads/{}", mr.source_branch);
        if let Some(sha) = git_refs::resolve_ref(&repo.path, &branch_ref).await {
            git_refs::write_ref(&repo.path, &snap_ref, &sha).await;
        }
    }

    // Auto-track agent completion
    let ev = AnalyticsEvent::new(
        new_id(),
        "agent.completed",
        Some(agent.id.to_string()),
        serde_json::json!({ "mr_id": mr.id.to_string() }),
        now,
    );
    let _ = state.analytics.record(&ev).await;

    // Auto-track MR creation
    let ev = AnalyticsEvent::new(
        new_id(),
        "mr.created",
        Some(agent.id.to_string()),
        serde_json::json!({ "mr_id": mr.id.to_string(), "source_branch": mr.source_branch }),
        now,
    );
    let _ = state.analytics.record(&ev).await;

    Ok((StatusCode::CREATED, Json(MrResponse::from(mr))))
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use crate::mem::test_state;
    use axum::{body::Body, Router};
    use http::{Request, StatusCode};
    use tower::ServiceExt;

    fn app() -> Router {
        crate::api::api_router().with_state(test_state())
    }

    async fn body_json(resp: axum::response::Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    async fn create_repo(app: Router) -> (Router, String) {
        let body = serde_json::json!({"project_id": "proj-1", "name": "test-repo"});
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/repos")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let json = body_json(resp).await;
        (app, json["id"].as_str().unwrap().to_string())
    }

    async fn create_task(app: Router, title: &str) -> (Router, String) {
        let body = serde_json::json!({"title": title});
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/tasks")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let json = body_json(resp).await;
        (app, json["id"].as_str().unwrap().to_string())
    }

    async fn do_spawn(
        app: Router,
        repo_id: &str,
        task_id: &str,
        branch: &str,
    ) -> (Router, serde_json::Value) {
        let body = serde_json::json!({
            "name": "worker-1",
            "repo_id": repo_id,
            "task_id": task_id,
            "branch": branch,
        });
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/agents/spawn")
                    .header("content-type", "application/json")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED, "spawn should succeed");
        let json = body_json(resp).await;
        (app, json)
    }

    #[tokio::test]
    async fn spawn_creates_agent_active_with_token() {
        let app = app();
        let (app, repo_id) = create_repo(app).await;
        let (app, task_id) = create_task(app, "Build feature").await;
        let (_, json) = do_spawn(app, &repo_id, &task_id, "feat/build").await;

        assert_eq!(json["agent"]["status"], "active");
        assert!(!json["agent"]["id"].as_str().unwrap().is_empty());
        assert!(!json["token"].as_str().unwrap().is_empty());
        assert_eq!(json["branch"], "feat/build");
        // jj_change_id is present in the response (null when worktree doesn't exist on disk)
        assert!(json.get("jj_change_id").is_some());
    }

    #[tokio::test]
    async fn spawn_response_includes_jj_change_id_field() {
        // Verifies the jj_change_id field is present in the spawn response JSON.
        // It will be null if the worktree path doesn't exist on disk (test env),
        // but the field must always be serialized.
        let app = app();
        let (app, repo_id) = create_repo(app).await;
        let (app, task_id) = create_task(app, "jj field task").await;
        let (_, json) = do_spawn(app, &repo_id, &task_id, "feat/jj-test").await;

        assert!(
            json.get("jj_change_id").is_some(),
            "spawn response must include jj_change_id field: {json}"
        );
    }

    #[tokio::test]
    async fn spawn_creates_worktree_record() {
        let app = app();
        let (app, repo_id) = create_repo(app).await;
        let (app, task_id) = create_task(app, "WT task").await;
        let (app, _) = do_spawn(app, &repo_id, &task_id, "feat/wt-test").await;

        let wt_resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/repos/{repo_id}/worktrees"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(wt_resp.status(), StatusCode::OK);
        let wt_json = body_json(wt_resp).await;
        assert_eq!(wt_json.as_array().unwrap().len(), 1);
        assert_eq!(wt_json[0]["branch"], "feat/wt-test");
    }

    #[tokio::test]
    async fn spawn_assigns_task_to_agent() {
        let app = app();
        let (app, repo_id) = create_repo(app).await;
        let (app, task_id) = create_task(app, "Assigned task").await;
        let (app, json) = do_spawn(app, &repo_id, &task_id, "feat/assign").await;
        let agent_id = json["agent"]["id"].as_str().unwrap().to_string();

        let task_resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/tasks/{task_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let task_json = body_json(task_resp).await;
        assert_eq!(task_json["assigned_to"].as_str().unwrap(), &agent_id);
        assert_eq!(task_json["status"], "in_progress");
    }

    #[tokio::test]
    async fn spawn_returns_correct_clone_url_format() {
        let app = app();
        let (app, repo_id) = create_repo(app).await;
        let (app, task_id) = create_task(app, "URL task").await;
        let (_, json) = do_spawn(app, &repo_id, &task_id, "feat/url-test").await;

        let clone_url = json["clone_url"].as_str().unwrap();
        assert!(
            clone_url.contains("/git/"),
            "clone_url should contain /git/: {clone_url}"
        );
        assert!(
            clone_url.contains("proj-1"),
            "clone_url should contain project id: {clone_url}"
        );
        assert!(
            clone_url.contains("test-repo"),
            "clone_url should contain repo name: {clone_url}"
        );
    }

    #[tokio::test]
    async fn spawn_branch_slashes_become_dashes_in_worktree_path() {
        let app = app();
        let (app, repo_id) = create_repo(app).await;
        let (app, task_id) = create_task(app, "Slash task").await;
        let (_, json) = do_spawn(app, &repo_id, &task_id, "feat/sub/feature").await;

        let wt_path = json["worktree_path"].as_str().unwrap();
        assert!(
            !wt_path.ends_with("feat/sub/feature"),
            "worktree path should not contain raw branch slashes: {wt_path}"
        );
    }

    #[tokio::test]
    async fn spawn_with_parent_id() {
        let app = app();
        let (app, repo_id) = create_repo(app).await;
        let (app, task_id) = create_task(app, "Child task").await;

        let body = serde_json::json!({
            "name": "child-worker",
            "repo_id": repo_id,
            "task_id": task_id,
            "branch": "feat/child",
            "parent_id": "parent-agent-123",
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/agents/spawn")
                    .header("content-type", "application/json")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let json = body_json(resp).await;
        assert_eq!(json["agent"]["parent_id"], "parent-agent-123");
    }

    #[tokio::test]
    async fn spawn_requires_valid_auth_token() {
        let app = app();
        let body = serde_json::json!({
            "name": "worker",
            "repo_id": "r",
            "task_id": "t",
            "branch": "feat/x",
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/agents/spawn")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn spawn_bad_token_rejected() {
        let app = app();
        let body = serde_json::json!({
            "name": "worker",
            "repo_id": "r",
            "task_id": "t",
            "branch": "feat/x",
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/agents/spawn")
                    .header("content-type", "application/json")
                    .header("Authorization", "Bearer bad-token")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn spawn_repo_not_found() {
        let app = app();
        let body = serde_json::json!({
            "name": "worker",
            "repo_id": "no-such-repo",
            "task_id": "task-1",
            "branch": "feat/x",
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/agents/spawn")
                    .header("content-type", "application/json")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn spawn_task_not_found() {
        let app = app();
        let (app, repo_id) = create_repo(app).await;
        let body = serde_json::json!({
            "name": "worker",
            "repo_id": repo_id,
            "task_id": "no-such-task",
            "branch": "feat/x",
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/agents/spawn")
                    .header("content-type", "application/json")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn complete_creates_merge_request() {
        let app = app();
        let (app, repo_id) = create_repo(app).await;
        let (app, task_id) = create_task(app, "Feature task").await;
        let (app, spawn_json) = do_spawn(app, &repo_id, &task_id, "feat/complete-test").await;
        let agent_id = spawn_json["agent"]["id"].as_str().unwrap().to_string();

        let body = serde_json::json!({
            "branch": "feat/complete-test",
            "title": "Add my feature",
            "target_branch": "main",
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/agents/{agent_id}/complete"))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let mr_json = body_json(resp).await;
        assert_eq!(mr_json["source_branch"], "feat/complete-test");
        assert_eq!(mr_json["target_branch"], "main");
        assert_eq!(mr_json["title"], "Add my feature");
        assert_eq!(mr_json["status"], "open");
    }

    #[tokio::test]
    async fn complete_transitions_task_to_review() {
        let app = app();
        let (app, repo_id) = create_repo(app).await;
        let (app, task_id) = create_task(app, "Review task").await;
        let (app, spawn_json) = do_spawn(app, &repo_id, &task_id, "feat/review-test").await;
        let agent_id = spawn_json["agent"]["id"].as_str().unwrap().to_string();

        let body = serde_json::json!({
            "branch": "feat/review-test",
            "title": "Done",
            "target_branch": "main",
        });
        app.clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/agents/{agent_id}/complete"))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        let task_resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/tasks/{task_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let task_json = body_json(task_resp).await;
        assert_eq!(task_json["status"], "review");
    }

    #[tokio::test]
    async fn complete_transitions_agent_to_idle() {
        let app = app();
        let (app, repo_id) = create_repo(app).await;
        let (app, task_id) = create_task(app, "Idle task").await;
        let (app, spawn_json) = do_spawn(app, &repo_id, &task_id, "feat/idle-test").await;
        let agent_id = spawn_json["agent"]["id"].as_str().unwrap().to_string();

        let body = serde_json::json!({
            "branch": "feat/idle-test",
            "title": "Done",
            "target_branch": "main",
        });
        app.clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/agents/{agent_id}/complete"))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        let agent_resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/agents/{agent_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let agent_json = body_json(agent_resp).await;
        assert_eq!(agent_json["status"], "idle");
    }

    #[tokio::test]
    async fn complete_agent_not_found() {
        let body = serde_json::json!({
            "branch": "feat/x",
            "title": "Done",
            "target_branch": "main",
        });
        let resp = app()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/agents/no-such/complete")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn complete_no_worktree_returns_bad_request() {
        let app = app();
        // Create agent directly (no worktree)
        let body = serde_json::json!({"name": "bare-agent"});
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/agents")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let json = body_json(resp).await;
        let agent_id = json["id"].as_str().unwrap().to_string();

        let body = serde_json::json!({
            "branch": "main",
            "title": "Done",
            "target_branch": "main",
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/agents/{agent_id}/complete"))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn complete_mr_sets_author_agent_id() {
        let app = app();
        let (app, repo_id) = create_repo(app).await;
        let (app, task_id) = create_task(app, "Author task").await;
        let (app, spawn_json) = do_spawn(app, &repo_id, &task_id, "feat/author-test").await;
        let agent_id = spawn_json["agent"]["id"].as_str().unwrap().to_string();

        let body = serde_json::json!({
            "branch": "feat/author-test",
            "title": "Feature",
            "target_branch": "main",
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/agents/{agent_id}/complete"))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let mr_json = body_json(resp).await;
        assert_eq!(mr_json["author_agent_id"].as_str().unwrap(), &agent_id);
    }

    #[tokio::test]
    async fn complete_revokes_agent_token() {
        let app = app();
        let (app, repo_id) = create_repo(app).await;
        let (app, task_id) = create_task(app, "Revoke task").await;
        let (app, spawn_json) = do_spawn(app, &repo_id, &task_id, "feat/revoke-test").await;
        let agent_id = spawn_json["agent"]["id"].as_str().unwrap().to_string();
        let agent_token = spawn_json["token"].as_str().unwrap().to_string();

        // Complete the agent
        let body = serde_json::json!({
            "branch": "feat/revoke-test",
            "title": "Done",
            "target_branch": "main",
        });
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/agents/{agent_id}/complete"))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);

        // The agent's token must now be rejected (401) — it was revoked on complete.
        let spawn_body = serde_json::json!({
            "name": "should-fail",
            "repo_id": repo_id,
            "task_id": task_id,
            "branch": "feat/should-fail",
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/agents/spawn")
                    .header("content-type", "application/json")
                    .header("Authorization", format!("Bearer {agent_token}"))
                    .body(Body::from(serde_json::to_vec(&spawn_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::UNAUTHORIZED,
            "agent token must be invalid after complete"
        );
    }

    #[tokio::test]
    async fn complete_idempotent_returns_202_on_double_complete() {
        let app = app();
        let (app, repo_id) = create_repo(app).await;
        let (app, task_id) = create_task(app, "Idempotent task").await;
        let (app, spawn_json) = do_spawn(app, &repo_id, &task_id, "feat/idempotent-test").await;
        let agent_id = spawn_json["agent"]["id"].as_str().unwrap().to_string();

        let body = serde_json::json!({
            "branch": "feat/idempotent-test",
            "title": "Idempotent Feature",
            "target_branch": "main",
        });

        // First complete — should return 201 CREATED
        let resp1 = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/agents/{agent_id}/complete"))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp1.status(), StatusCode::CREATED);
        let mr_json1 = body_json(resp1).await;
        let mr_id = mr_json1["id"].as_str().unwrap().to_string();

        // Second complete — should return 202 ACCEPTED with the same MR id
        let resp2 = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/agents/{agent_id}/complete"))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp2.status(), StatusCode::ACCEPTED);
        let mr_json2 = body_json(resp2).await;
        assert_eq!(mr_json2["id"].as_str().unwrap(), &mr_id);
    }
}
