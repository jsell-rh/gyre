use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use gyre_common::Id;
use gyre_domain::{AgentCommit, AgentWorktree, Session};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::auth::AuthenticatedAgent;
use crate::AppState;

use super::error::ApiError;
use super::{new_id, now_secs};

// ── Request / Response types ──────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct RecordCommitRequest {
    pub agent_id: String,
    pub commit_sha: String,
    pub branch: String,
    /// Optional caller-asserted file list for the manual recording path,
    /// used only when the sha cannot be resolved against the repo's real
    /// object store (e.g. recording a commit that lives on another forge).
    /// The native push path always derives the list from
    /// `git diff-tree` against the real repo.
    #[serde(default)]
    pub files_changed: Option<Vec<String>>,
}

#[derive(Serialize)]
pub struct AgentCommitResponse {
    pub id: String,
    pub agent_id: String,
    pub repository_id: String,
    pub commit_sha: String,
    pub branch: String,
    pub timestamp: u64,
    /// The agent session (context window) that produced this commit —
    /// NULL when the pushing agent had no open session.
    pub session_id: Option<String>,
    /// Files changed by this commit, recorded at push time.
    pub files_changed: Vec<String>,
}

impl From<AgentCommit> for AgentCommitResponse {
    fn from(ac: AgentCommit) -> Self {
        Self {
            id: ac.id.to_string(),
            agent_id: ac.agent_id.to_string(),
            repository_id: ac.repository_id.to_string(),
            commit_sha: ac.commit_sha,
            branch: ac.branch,
            timestamp: ac.timestamp,
            session_id: ac.session_id,
            files_changed: ac.files_changed,
        }
    }
}

#[derive(Deserialize)]
pub struct CommitFilterQuery {
    pub agent_id: Option<String>,
}

/// A session hit from a commit-based query: the session, its agent, and
/// the commits that matched (source-control.md §Agent-to-Commit Tracking
/// — "find all agent sessions that touched this file/commit").
#[derive(Serialize)]
pub struct SessionHitResponse {
    pub session: AgentSessionResponse,
    pub agent_id: String,
    pub commit_shas: Vec<String>,
}

#[derive(Serialize)]
pub struct AgentSessionResponse {
    pub id: String,
    pub agent_id: String,
    pub repository_id: String,
    pub started_at: u64,
    pub ended_at: Option<u64>,
    /// Branch checked out at session start (spawn-time branch).
    pub initial_branch: String,
    /// Branch resolved in the agent's worktree at session end.
    pub final_branch: Option<String>,
    /// Whether the agent left uncommitted work at termination.
    /// `None` = not terminated yet, or the worktree state could not be
    /// determined (explicit unknown state — never a silent clean).
    pub dirty: Option<bool>,
    pub uncommitted_paths: Vec<String>,
    pub dirty_detail: Option<String>,
    pub cost: Option<f64>,
    pub tokens_used: Option<u64>,
    pub messages_read: Vec<String>,
}

impl From<Session> for AgentSessionResponse {
    fn from(s: Session) -> Self {
        Self {
            id: s.id.to_string(),
            agent_id: s.agent_id.to_string(),
            repository_id: s.repository_id.to_string(),
            started_at: s.started_at,
            ended_at: s.ended_at,
            initial_branch: s.initial_branch,
            final_branch: s.final_branch,
            dirty: s.dirty,
            uncommitted_paths: s.uncommitted_paths,
            dirty_detail: s.dirty_detail,
            cost: s.cost,
            tokens_used: s.tokens_used,
            messages_read: s.messages_read,
        }
    }
}

#[derive(Deserialize)]
pub struct SessionFilterQuery {
    pub agent_id: Option<String>,
}

#[derive(Deserialize)]
pub struct SessionsByFileQuery {
    pub path: String,
}

#[derive(Deserialize)]
pub struct SessionsByCommitQuery {
    pub sha: String,
}

/// Per-agent branch lifecycle view (source-control.md §Agent-to-Commit
/// Tracking): the initial (spawn-time) branch, the current branch
/// recorded at the latest termination, and all observed commits.
#[derive(Serialize)]
pub struct BranchLifecycleResponse {
    pub agent_id: String,
    pub repository_id: String,
    /// Branch at agent spawn (first session's initial branch).
    pub initial_branch: String,
    /// Current branch at the latest session termination (real worktree
    /// lookup at termination time — observable to differ from
    /// initial_branch when the agent moved branches mid-session).
    pub current_branch: Option<String>,
    pub sessions: Vec<AgentSessionResponse>,
    /// All commits recorded during the agent's lifetime (chronological).
    pub observed_commits: Vec<AgentCommitResponse>,
}

/// Time-travel view (source-control.md §Agent-to-Commit Tracking): what
/// the repo looked like while the session was active.
#[derive(Serialize)]
pub struct SessionStateResponse {
    pub session: AgentSessionResponse,
    /// Commits present in the repo during [started_at, ended_at] — a real
    /// `git log --after/--before` query against the object store.
    pub commits_during_session: Vec<crate::agent_sessions::WindowCommit>,
    /// The tree/commit state at the session's completion point, resolved
    /// from the refs/agents/{agent-id}/snapshots/{n} refs. `None` when the
    /// session is still open or no snapshot qualifies.
    pub completion_snapshot: Option<crate::agent_sessions::CompletionSnapshot>,
}

#[derive(Deserialize)]
pub struct CreateWorktreeRequest {
    pub agent_id: String,
    pub task_id: Option<String>,
    pub branch: String,
    pub path: String,
}

#[derive(Serialize)]
pub struct AgentWorktreeResponse {
    pub id: String,
    pub agent_id: String,
    pub repository_id: String,
    pub task_id: Option<String>,
    pub branch: String,
    pub path: String,
    pub created_at: u64,
}

impl From<AgentWorktree> for AgentWorktreeResponse {
    fn from(wt: AgentWorktree) -> Self {
        Self {
            id: wt.id.to_string(),
            agent_id: wt.agent_id.to_string(),
            repository_id: wt.repository_id.to_string(),
            task_id: wt.task_id.map(|id| id.to_string()),
            branch: wt.branch,
            path: wt.path,
            created_at: wt.created_at,
        }
    }
}

// ── Handlers ──────────────────────────────────────────────────────────────────

/// POST /api/v1/repos/:id/commits/record
///
/// Records an AgentCommit provenance row. Preview-agent callers are
/// refused (task-206 review F5): §5 forbids preview runs from leaving an
/// audit trail, and this handler persists a caller-supplied agent_id as a
/// provenance row — the MCP dispatch already refuses `conversation_upload`
/// for preview agents for exactly this reason, and this per-handler guard
/// closes the REST twin. The middleware maps the route to (repo, write),
/// which the Agent role passes via the builtin agent-scoped-access policy,
/// so this guard is the only refusal layer for a same-tenant
/// preview-agent JWT.
pub async fn record_commit(
    State(state): State<Arc<AppState>>,
    auth: AuthenticatedAgent,
    Path(repo_id): Path<String>,
    Json(req): Json<RecordCommitRequest>,
) -> Result<(StatusCode, Json<AgentCommitResponse>), ApiError> {
    if crate::api::meta_specs::caller_is_preview_agent(&state, &auth).await {
        return Err(ApiError::Forbidden(
            "preview agents may not record commit provenance — preview runs leave no \
             audit trail"
                .to_string(),
        ));
    }
    // Verify the repo exists (and keep it: the file list is derived from
    // its real object store when possible).
    let repo = state
        .repos
        .find_by_id(&Id::new(&repo_id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("repo {repo_id} not found")))?;

    // source-control.md §Agent-to-Commit Tracking: attribute the commit to
    // the pushing agent's currently-open session (context window). No open
    // session → NULL session_id — never a fabricated id. Same resolution as
    // the native push path (git_http record_pushed_commits).
    let session_id = crate::agent_sessions::open_session_id(&state, &Id::new(&req.agent_id)).await;

    // Files changed: derived from the repo's real object store when the
    // sha resolves there (git diff-tree); the caller-asserted list is a
    // fallback for commits that live outside this forge's storage.
    let mut files_changed =
        crate::agent_sessions::files_changed_for_commit(&repo.path, &req.commit_sha).await;
    if files_changed.is_empty() {
        files_changed = req.files_changed.unwrap_or_default();
    }

    let ac = AgentCommit::new(
        new_id(),
        Id::new(req.agent_id),
        Id::new(repo_id),
        req.commit_sha,
        req.branch,
        now_secs(),
    )
    .with_session_id(session_id)
    .with_files_changed(files_changed);
    state.agent_commits.record(&ac).await?;
    Ok((StatusCode::CREATED, Json(AgentCommitResponse::from(ac))))
}

/// GET /api/v1/repos/:id/commits?agent_id=<optional>
pub async fn list_commits(
    State(state): State<Arc<AppState>>,
    Path(repo_id): Path<String>,
    Query(params): Query<CommitFilterQuery>,
) -> Result<Json<Vec<AgentCommitResponse>>, ApiError> {
    // Verify the repo exists.
    state
        .repos
        .find_by_id(&Id::new(&repo_id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("repo {repo_id} not found")))?;

    let commits = if let Some(agent_id) = params.agent_id {
        // Filter by both repo AND agent: get agent's commits, keep matching repo.
        state
            .agent_commits
            .find_by_agent(&Id::new(agent_id))
            .await?
            .into_iter()
            .filter(|ac| ac.repository_id.as_str() == repo_id)
            .collect()
    } else {
        state.agent_commits.find_by_repo(&Id::new(&repo_id)).await?
    };

    Ok(Json(
        commits.into_iter().map(AgentCommitResponse::from).collect(),
    ))
}

/// Load a session scoped to the repo: NotFound when the session does not
/// exist or belongs to a different repository (scoped-entity check — the
/// repo path segment is the authorization scope, resolved by the ABAC
/// middleware's repo resource mapping).
async fn session_in_repo(
    state: &Arc<AppState>,
    repo_id: &str,
    session_id: &str,
) -> Result<Session, ApiError> {
    let session = state
        .agent_sessions
        .find_by_id(&Id::new(session_id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("session {session_id} not found")))?;
    if session.repository_id.as_str() != repo_id {
        // A session in another repo is indistinguishable from a missing
        // one from this route's scope.
        return Err(ApiError::NotFound(format!(
            "session {session_id} not found"
        )));
    }
    Ok(session)
}

/// GET /api/v1/repos/:id/agent-sessions?agent_id=<optional>
///
/// The agent's session list (source-control.md §Agent-to-Commit Tracking):
/// one session per context window, with start/end times.
pub async fn list_sessions(
    State(state): State<Arc<AppState>>,
    Path(repo_id): Path<String>,
    Query(params): Query<SessionFilterQuery>,
) -> Result<Json<Vec<AgentSessionResponse>>, ApiError> {
    state
        .repos
        .find_by_id(&Id::new(&repo_id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("repo {repo_id} not found")))?;

    let sessions = if let Some(agent_id) = params.agent_id {
        state
            .agent_sessions
            .find_by_agent(&Id::new(agent_id))
            .await?
            .into_iter()
            .filter(|s| s.repository_id.as_str() == repo_id)
            .collect()
    } else {
        state
            .agent_sessions
            .find_by_repo(&Id::new(&repo_id))
            .await?
    };
    Ok(Json(
        sessions
            .into_iter()
            .map(AgentSessionResponse::from)
            .collect(),
    ))
}

/// GET /api/v1/repos/:id/agent-sessions/:session_id/commits
///
/// The chronological commits-by-session query (source-control.md
/// §Agent-to-Commit Tracking — session-to-commit mapping): exactly the
/// commits recorded while this session was the agent's open context
/// window.
pub async fn session_commits(
    State(state): State<Arc<AppState>>,
    Path((repo_id, session_id)): Path<(String, String)>,
) -> Result<Json<Vec<AgentCommitResponse>>, ApiError> {
    let session = session_in_repo(&state, &repo_id, &session_id).await?;
    let commits = state
        .agent_commits
        .find_by_session(&session.id)
        .await?
        .into_iter()
        .map(AgentCommitResponse::from)
        .collect();
    Ok(Json(commits))
}

/// GET /api/v1/repos/:id/agent-sessions/by-file?path=<path>
///
/// "Find all agent sessions that touched this file": the sessions (and
/// agents) whose recorded commits changed the path. A file touched by no
/// recorded commit returns an empty list.
pub async fn sessions_by_file(
    State(state): State<Arc<AppState>>,
    Path(repo_id): Path<String>,
    Query(params): Query<SessionsByFileQuery>,
) -> Result<Json<Vec<SessionHitResponse>>, ApiError> {
    state
        .repos
        .find_by_id(&Id::new(&repo_id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("repo {repo_id} not found")))?;

    let commits = state
        .agent_commits
        .find_by_file(&Id::new(&repo_id), &params.path)
        .await?;

    // Group the matching commits by the session that produced them.
    let mut by_session: Vec<(String, Vec<String>)> = Vec::new();
    for ac in commits {
        let Some(sid) = ac.session_id.clone() else {
            continue;
        };
        match by_session.iter_mut().find(|(s, _)| *s == sid) {
            Some((_, shas)) => shas.push(ac.commit_sha.clone()),
            None => by_session.push((sid, vec![ac.commit_sha.clone()])),
        }
    }

    let mut hits = Vec::new();
    for (sid, shas) in by_session {
        if let Ok(Some(session)) = state.agent_sessions.find_by_id(&Id::new(&sid)).await {
            if session.repository_id.as_str() == repo_id {
                hits.push(SessionHitResponse {
                    agent_id: session.agent_id.to_string(),
                    session: AgentSessionResponse::from(session),
                    commit_shas: shas,
                });
            }
        }
    }
    Ok(Json(hits))
}

/// GET /api/v1/repos/:id/agent-sessions/by-commit?sha=<sha>
///
/// "Find the agent session(s) that touched this commit": the session
/// linked to the recorded commit mapping for the SHA. A SHA with no
/// recorded mapping (or one made outside any session) returns an empty
/// list.
pub async fn sessions_by_commit(
    State(state): State<Arc<AppState>>,
    Path(repo_id): Path<String>,
    Query(params): Query<SessionsByCommitQuery>,
) -> Result<Json<Vec<SessionHitResponse>>, ApiError> {
    state
        .repos
        .find_by_id(&Id::new(&repo_id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("repo {repo_id} not found")))?;

    let Some(ac) = state.agent_commits.find_by_commit(&params.sha).await? else {
        return Ok(Json(Vec::new()));
    };
    if ac.repository_id.as_str() != repo_id {
        // A commit recorded in another repo is not this repo's to report.
        return Ok(Json(Vec::new()));
    }
    let Some(sid) = ac.session_id.clone() else {
        return Ok(Json(Vec::new()));
    };
    let Ok(Some(session)) = state.agent_sessions.find_by_id(&Id::new(&sid)).await else {
        return Ok(Json(Vec::new()));
    };
    if session.repository_id.as_str() != repo_id {
        return Ok(Json(Vec::new()));
    }
    Ok(Json(vec![SessionHitResponse {
        agent_id: session.agent_id.to_string(),
        session: AgentSessionResponse::from(session),
        commit_shas: vec![ac.commit_sha],
    }]))
}

/// GET /api/v1/repos/:id/agents/:agent_id/branch-lifecycle
///
/// Per-agent branch lifecycle view (source-control.md §Agent-to-Commit
/// Tracking): the initial (spawn-time) branch, the current branch
/// recorded at termination (a real worktree lookup at termination time —
/// observable to differ after the agent moved branches mid-session), and
/// all observed commits.
pub async fn branch_lifecycle(
    State(state): State<Arc<AppState>>,
    Path((repo_id, agent_id)): Path<(String, String)>,
) -> Result<Json<BranchLifecycleResponse>, ApiError> {
    state
        .repos
        .find_by_id(&Id::new(&repo_id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("repo {repo_id} not found")))?;
    state
        .agents
        .find_by_id(&Id::new(&agent_id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("agent {agent_id} not found")))?;

    let sessions: Vec<Session> = state
        .agent_sessions
        .find_by_agent(&Id::new(&agent_id))
        .await?
        .into_iter()
        .filter(|s| s.repository_id.as_str() == repo_id)
        .collect();

    // Initial branch: the first session's spawn-time branch; fall back to
    // the worktree record (the spawn-time branch source) when no session
    // was ever recorded.
    let initial_branch = match sessions.first() {
        Some(s) => s.initial_branch.clone(),
        None => crate::agent_sessions::worktree_for_agent(&state, &Id::new(&agent_id))
            .await
            .map(|wt| wt.branch)
            .unwrap_or_default(),
    };

    // Current branch: recorded at the latest termination (real worktree
    // lookup at that time — never the spawn-time value copied forward).
    let current_branch = sessions
        .iter()
        .rev()
        .find(|s| s.ended_at.is_some())
        .and_then(|s| s.final_branch.clone());

    // All observed commits: commits recorded during the agent's lifetime,
    // chronological (oldest first). Recorded timestamps have second
    // granularity, so commits recorded within the same second tie; the
    // producing session's start time breaks those ties (a durable
    // chronology signal: one agent's sessions are sequential context
    // windows, so session order never contradicts recorded-time order),
    // with the id only as the final deterministic fallback. Sorting on
    // the random-UUID id alone would order same-second commits
    // arbitrarily — observable as a reversed lifecycle view.
    let session_start: std::collections::HashMap<&str, u64> = sessions
        .iter()
        .map(|s| (s.id.as_str(), s.started_at))
        .collect();
    let session_key = |ac: &AgentCommit| {
        ac.session_id
            .as_deref()
            .and_then(|sid| session_start.get(sid).copied())
            .unwrap_or(ac.timestamp)
    };
    let mut observed: Vec<AgentCommit> = state
        .agent_commits
        .find_by_agent(&Id::new(&agent_id))
        .await?
        .into_iter()
        .filter(|ac| ac.repository_id.as_str() == repo_id)
        .collect();
    observed.sort_by(|a, b| {
        a.timestamp
            .cmp(&b.timestamp)
            .then_with(|| session_key(a).cmp(&session_key(b)))
            .then_with(|| a.id.as_str().cmp(b.id.as_str()))
    });

    Ok(Json(BranchLifecycleResponse {
        agent_id,
        repository_id: repo_id,
        initial_branch,
        current_branch,
        sessions: sessions
            .into_iter()
            .map(AgentSessionResponse::from)
            .collect(),
        observed_commits: observed
            .into_iter()
            .map(AgentCommitResponse::from)
            .collect(),
    }))
}

/// GET /api/v1/repos/:id/agent-sessions/:session_id/state
///
/// Time-travel (source-control.md §Agent-to-Commit Tracking): what the
/// repo looked like while the session was active — the commits present
/// during [started_at, ended_at] (a real `git log --after/--before` query
/// against the object store) plus the tree/commit state at the session's
/// completion point (resolved from the refs/agents/{agent-id}/snapshots
/// refs). Real queries against real git state, never current HEAD.
pub async fn session_state(
    State(state): State<Arc<AppState>>,
    Path((repo_id, session_id)): Path<(String, String)>,
) -> Result<Json<SessionStateResponse>, ApiError> {
    let session = session_in_repo(&state, &repo_id, &session_id).await?;
    let repo = state
        .repos
        .find_by_id(&Id::new(&repo_id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("repo {repo_id} not found")))?;

    // An open session's window extends to now; the completion snapshot
    // exists only once the session has terminated.
    let ended_at = session.ended_at.unwrap_or_else(now_secs);
    let commits_during_session =
        crate::agent_sessions::commits_in_window(&repo.path, session.started_at, ended_at)
            .await
            .map_err(|e| {
                ApiError::Internal(anyhow::anyhow!(
                    "time-travel query for session {session_id} failed: {e}"
                ))
            })?;
    let completion_snapshot = match session.ended_at {
        Some(ended) => {
            crate::agent_sessions::completion_snapshot(
                &repo.path,
                &session.agent_id.to_string(),
                ended,
            )
            .await
        }
        None => None,
    };

    Ok(Json(SessionStateResponse {
        session: AgentSessionResponse::from(session),
        commits_during_session,
        completion_snapshot,
    }))
}

/// POST /api/v1/repos/:id/worktrees
pub async fn create_worktree(
    State(state): State<Arc<AppState>>,
    Path(repo_id): Path<String>,
    auth: AuthenticatedAgent,
    Json(req): Json<CreateWorktreeRequest>,
) -> Result<(StatusCode, Json<AgentWorktreeResponse>), ApiError> {
    // Verify the repo exists.
    let repo = state
        .repos
        .find_by_id(&Id::new(&repo_id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("repo {repo_id} not found")))?;

    // G6: ABAC enforcement — check repo access policies against the caller's JWT claims.
    if let Err(reason) = crate::abac::check_repo_abac(&state, &repo_id, &auth).await {
        // task-104: AbacDenied audit event (observability.md §Audit Event
        // Types) — the per-handler repo-level ABAC check rejected the
        // request outside the ABAC middleware.
        crate::abac_middleware::record_abac_denied(
            &state,
            &auth,
            "repo",
            "write",
            None,
            "POST /api/v1/repos/:id/worktrees",
        )
        .await;
        return Err(ApiError::Forbidden(reason));
    }

    // Create the worktree via git ops.
    if let Err(e) = state
        .git_ops
        .create_worktree(&repo.path, &req.path, &req.branch)
        .await
    {
        tracing::warn!("git worktree add failed: {e}");
    }

    let wt = AgentWorktree::new(
        new_id(),
        Id::new(req.agent_id),
        Id::new(repo_id),
        req.task_id.map(Id::new),
        req.branch,
        req.path,
        now_secs(),
    );
    state.worktrees.create(&wt).await?;
    Ok((StatusCode::CREATED, Json(AgentWorktreeResponse::from(wt))))
}

/// GET /api/v1/repos/:id/worktrees
pub async fn list_worktrees(
    State(state): State<Arc<AppState>>,
    Path(repo_id): Path<String>,
) -> Result<Json<Vec<AgentWorktreeResponse>>, ApiError> {
    state
        .repos
        .find_by_id(&Id::new(&repo_id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("repo {repo_id} not found")))?;

    let worktrees = state.worktrees.find_by_repo(&Id::new(&repo_id)).await?;
    Ok(Json(
        worktrees
            .into_iter()
            .map(AgentWorktreeResponse::from)
            .collect(),
    ))
}

/// DELETE /api/v1/repos/:id/worktrees/:wt_id
pub async fn delete_worktree(
    State(state): State<Arc<AppState>>,
    Path((repo_id, wt_id)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    let repo = state
        .repos
        .find_by_id(&Id::new(&repo_id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("repo {repo_id} not found")))?;

    // jj workspace teardown before deleting the record (source-control.md §4).
    if let Ok(wts) = state.worktrees.find_by_repo(&repo.id).await {
        if let Some(wt) = wts.iter().find(|wt| wt.id.as_str() == wt_id) {
            let agent_id = wt.agent_id.to_string();
            crate::api::spawn::cleanup_jj_workspace(
                &state,
                &agent_id,
                &repo.path,
                &wt.path,
                &format!("agent-{agent_id}"),
            )
            .await;
        }
    }

    state.worktrees.delete(&Id::new(&wt_id)).await?;
    Ok(StatusCode::NO_CONTENT)
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
        let body = serde_json::json!({"workspace_id": "ws-1", "name": "test-repo"});
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
        let repo_id = json["id"].as_str().unwrap().to_string();
        (app, repo_id)
    }

    #[tokio::test]
    async fn record_commit_and_list() {
        let (app, repo_id) = create_repo(app()).await;

        let body = serde_json::json!({
            "agent_id": "agent-1",
            "commit_sha": "abc123",
            "branch": "main"
        });
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/repos/{repo_id}/commits/record"))
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);

        let list_resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/repos/{repo_id}/agent-commits"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(list_resp.status(), StatusCode::OK);
        let json = body_json(list_resp).await;
        assert_eq!(json.as_array().unwrap().len(), 1);
        assert_eq!(json[0]["commit_sha"], "abc123");
    }

    #[tokio::test]
    async fn list_commits_filter_by_agent() {
        let (app, repo_id) = create_repo(app()).await;

        for (agent, sha) in [("agent-1", "sha1"), ("agent-2", "sha2")] {
            let body = serde_json::json!({
                "agent_id": agent,
                "commit_sha": sha,
                "branch": "main"
            });
            app.clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri(format!("/api/v1/repos/{repo_id}/commits/record"))
                        .header("authorization", "Bearer test-token")
                        .header("content-type", "application/json")
                        .body(Body::from(serde_json::to_vec(&body).unwrap()))
                        .unwrap(),
                )
                .await
                .unwrap();
        }

        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/api/v1/repos/{repo_id}/agent-commits?agent_id=agent-1"
                    ))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json.as_array().unwrap().len(), 1);
        assert_eq!(json[0]["agent_id"], "agent-1");
    }

    #[tokio::test]
    async fn record_commit_repo_not_found() {
        let app = app();
        let body = serde_json::json!({
            "agent_id": "agent-1",
            "commit_sha": "abc",
            "branch": "main"
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/repos/no-such/commits/record")
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn create_and_list_worktrees() {
        let (app, repo_id) = create_repo(app()).await;

        let body = serde_json::json!({
            "agent_id": "agent-1",
            "branch": "feat/x",
            "path": "/tmp/wt/agent-1"
        });
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/repos/{repo_id}/worktrees"))
                    .header("content-type", "application/json")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let created = body_json(resp).await;
        let wt_id = created["id"].as_str().unwrap().to_string();

        let list_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/repos/{repo_id}/worktrees"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(list_resp.status(), StatusCode::OK);
        let json = body_json(list_resp).await;
        assert_eq!(json.as_array().unwrap().len(), 1);
        assert_eq!(json[0]["branch"], "feat/x");

        // Delete the worktree.
        let del_resp = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/api/v1/repos/{repo_id}/worktrees/{wt_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(del_resp.status(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn create_worktree_with_task_id() {
        let (app, repo_id) = create_repo(app()).await;

        let body = serde_json::json!({
            "agent_id": "agent-1",
            "task_id": "task-42",
            "branch": "feat/task42",
            "path": "/tmp/wt/task42"
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/repos/{repo_id}/worktrees"))
                    .header("content-type", "application/json")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let json = body_json(resp).await;
        assert_eq!(json["task_id"], "task-42");
    }

    #[tokio::test]
    async fn list_worktrees_empty() {
        let (app, repo_id) = create_repo(app()).await;
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/repos/{repo_id}/worktrees"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert!(json.as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn list_worktrees_repo_not_found() {
        let resp = app()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/repos/no-such/worktrees")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    // ── Agent-session tracking (source-control.md §Agent-to-Commit
    // Tracking) ─────────────────────────────────────────────────────

    fn state_app() -> (std::sync::Arc<crate::AppState>, Router) {
        let state = crate::mem::test_state();
        (state.clone(), crate::api::api_router().with_state(state))
    }

    async fn make_agent(state: &crate::AppState, id: &str) {
        let agent = gyre_domain::Agent::new(gyre_common::Id::new(id), format!("a-{id}"), 1000);
        state.agents.create(&agent).await.unwrap();
    }

    async fn open_session(
        state: &crate::AppState,
        id: &str,
        agent_id: &str,
        repo_id: &str,
        started_at: u64,
        initial_branch: &str,
    ) {
        state
            .agent_sessions
            .create(&gyre_domain::Session::new(
                gyre_common::Id::new(id),
                gyre_common::Id::new(agent_id),
                gyre_common::Id::new(repo_id),
                started_at,
                initial_branch,
            ))
            .await
            .unwrap();
    }

    async fn end_session(
        state: &crate::AppState,
        id: &str,
        ended_at: u64,
        final_branch: Option<&str>,
    ) {
        let mut s = state
            .agent_sessions
            .find_by_id(&gyre_common::Id::new(id))
            .await
            .unwrap()
            .unwrap();
        s.ended_at = Some(ended_at);
        s.final_branch = final_branch.map(str::to_string);
        state.agent_sessions.update(&s).await.unwrap();
    }

    async fn record(
        app: &Router,
        repo_id: &str,
        agent_id: &str,
        sha: &str,
        files: Option<Vec<String>>,
    ) -> serde_json::Value {
        let mut body = serde_json::json!({
            "agent_id": agent_id,
            "commit_sha": sha,
            "branch": "feat/x",
        });
        if let Some(files) = files {
            body["files_changed"] = serde_json::json!(files);
        }
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/repos/{repo_id}/commits/record"))
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        body_json(resp).await
    }

    async fn get_json(app: &Router, uri: String) -> serde_json::Value {
        let resp = app
            .clone()
            .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        body_json(resp).await
    }

    /// Run git in `dir`, optionally with a pinned author/committer date
    /// (epoch seconds) so commit timestamps are deterministic relative to
    /// session windows (wall-clock second granularity would make window
    /// membership flaky).
    fn git_at(dir: &std::path::Path, args: &[&str], epoch: Option<u64>) {
        let mut cmd = std::process::Command::new("git");
        cmd.arg("-C").arg(dir).args(args);
        cmd.env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@t")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@t");
        if let Some(t) = epoch {
            let date = format!("{t} +0000");
            cmd.env("GIT_AUTHOR_DATE", &date)
                .env("GIT_COMMITTER_DATE", &date);
        }
        let ok = cmd.status().unwrap().success();
        assert!(ok, "git {:?} failed in {}", args, dir.display());
    }

    /// Two-session attribution: commit A lands in session 1, the Ralph-loop
    /// respawn (fresh context) opens session 2, commit B lands in session 2.
    /// The commits-by-session query must return exactly [A] and [B] —
    /// session granularity, not agent granularity. A fabricated constant
    /// session id would land both commits in one session and fail this.
    #[tokio::test]
    async fn two_session_attribution() {
        let (state, app) = state_app();
        let (app, repo_id) = create_repo(app).await;
        make_agent(&state, "agent-1").await;

        // Session 1 (spawn): open, commit A pushed, then terminated.
        open_session(&state, "sess-1", "agent-1", &repo_id, 1000, "feat/x").await;
        let a = record(&app, &repo_id, "agent-1", "AAAA", None).await;
        assert_eq!(a["session_id"], "sess-1");
        end_session(&state, "sess-1", 1500, Some("feat/x")).await;

        // Session 2 (respawn with fresh context): commit B.
        open_session(&state, "sess-2", "agent-1", &repo_id, 2000, "feat/x").await;
        let b = record(&app, &repo_id, "agent-1", "BBBB", None).await;
        assert_eq!(b["session_id"], "sess-2");
        end_session(&state, "sess-2", 2500, Some("feat/y")).await;

        // The agent's session list shows two sessions with start/end times.
        let sessions = get_json(
            &app,
            format!("/api/v1/repos/{repo_id}/agent-sessions?agent_id=agent-1"),
        )
        .await;
        let arr = sessions.as_array().unwrap();
        assert_eq!(arr.len(), 2, "two sessions, one per context window");
        assert_eq!(arr[0]["id"], "sess-1");
        assert_eq!(arr[0]["started_at"], 1000);
        assert_eq!(arr[0]["ended_at"], 1500);
        assert_eq!(arr[1]["id"], "sess-2");
        assert_eq!(arr[1]["ended_at"], 2500);

        // Commits-by-session: exactly [A] for session 1, [B] for session 2.
        let s1 = get_json(
            &app,
            format!("/api/v1/repos/{repo_id}/agent-sessions/sess-1/commits"),
        )
        .await;
        let s1_shas: Vec<&str> = s1
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["commit_sha"].as_str().unwrap())
            .collect();
        assert_eq!(s1_shas, vec!["AAAA"], "session 1 owns exactly commit A");

        let s2 = get_json(
            &app,
            format!("/api/v1/repos/{repo_id}/agent-sessions/sess-2/commits"),
        )
        .await;
        let s2_shas: Vec<&str> = s2
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["commit_sha"].as_str().unwrap())
            .collect();
        assert_eq!(s2_shas, vec!["BBBB"], "session 2 owns exactly commit B");

        // A commit recorded outside any session carries a NULL session id
        // (never a fabricated one) and appears in neither session's list.
        end_session(&state, "sess-2", 2500, None).await; // ensure no open session
        let orphan = record(&app, &repo_id, "agent-1", "ORPH", None).await;
        assert!(orphan["session_id"].is_null());
        let s1 = get_json(
            &app,
            format!("/api/v1/repos/{repo_id}/agent-sessions/sess-1/commits"),
        )
        .await;
        assert_eq!(s1.as_array().unwrap().len(), 1);
    }

    /// By-file and by-commit session queries: after pushing a commit that
    /// touches src/lib.rs, the by-file query returns the producing session;
    /// an untouched file returns an empty list; the by-commit query for the
    /// SHA returns its session.
    #[tokio::test]
    async fn by_file_and_by_commit_queries() {
        let (state, app) = state_app();
        let (app, repo_id) = create_repo(app).await;
        make_agent(&state, "agent-1").await;
        open_session(&state, "sess-1", "agent-1", &repo_id, 1000, "feat/x").await;

        record(
            &app,
            &repo_id,
            "agent-1",
            "F11E",
            Some(vec!["src/lib.rs".to_string(), "README.md".to_string()]),
        )
        .await;
        end_session(&state, "sess-1", 1500, None).await;

        // By file: the producing session.
        let hits = get_json(
            &app,
            format!("/api/v1/repos/{repo_id}/agent-sessions/by-file?path=src/lib.rs"),
        )
        .await;
        let arr = hits.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["session"]["id"], "sess-1");
        assert_eq!(arr[0]["agent_id"], "agent-1");
        assert_eq!(arr[0]["commit_shas"], serde_json::json!(["F11E"]));

        // A file touched by no recorded commit: empty list.
        let none = get_json(
            &app,
            format!("/api/v1/repos/{repo_id}/agent-sessions/by-file?path=src/main.rs"),
        )
        .await;
        assert!(none.as_array().unwrap().is_empty());

        // A path that is a substring of a recorded path must not match.
        let substr = get_json(
            &app,
            format!("/api/v1/repos/{repo_id}/agent-sessions/by-file?path=lib.rs"),
        )
        .await;
        assert!(substr.as_array().unwrap().is_empty());

        // By commit SHA: the linked session.
        let by_commit = get_json(
            &app,
            format!("/api/v1/repos/{repo_id}/agent-sessions/by-commit?sha=F11E"),
        )
        .await;
        let arr = by_commit.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["session"]["id"], "sess-1");

        // An unrecorded SHA: empty list.
        let none = get_json(
            &app,
            format!("/api/v1/repos/{repo_id}/agent-sessions/by-commit?sha=NOPE"),
        )
        .await;
        assert!(none.as_array().unwrap().is_empty());
    }

    /// Branch lifecycle view: initial (spawn-time) branch, current branch
    /// recorded at termination (differs after the agent moved branches),
    /// and all observed commits.
    #[tokio::test]
    async fn branch_lifecycle_view() {
        let (state, app) = state_app();
        let (app, repo_id) = create_repo(app).await;
        make_agent(&state, "agent-1").await;

        // Session 1 spawns on feat/x, moves to feat/moved before ending.
        open_session(&state, "sess-1", "agent-1", &repo_id, 1000, "feat/x").await;
        record(&app, &repo_id, "agent-1", "AAAA", None).await;
        end_session(&state, "sess-1", 1500, Some("feat/moved")).await;

        // Session 2 on the moved branch.
        open_session(&state, "sess-2", "agent-1", &repo_id, 2000, "feat/moved").await;
        record(&app, &repo_id, "agent-1", "BBBB", None).await;
        end_session(&state, "sess-2", 2500, Some("feat/moved-2")).await;

        let view = get_json(
            &app,
            format!("/api/v1/repos/{repo_id}/agents/agent-1/branch-lifecycle"),
        )
        .await;
        assert_eq!(view["initial_branch"], "feat/x", "spawn-time branch");
        assert_eq!(
            view["current_branch"], "feat/moved-2",
            "current branch recorded at the latest termination"
        );
        let commits: Vec<&str> = view["observed_commits"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["commit_sha"].as_str().unwrap())
            .collect();
        assert_eq!(commits, vec!["AAAA", "BBBB"], "all observed commits");
        assert_eq!(view["sessions"].as_array().unwrap().len(), 2);

        // Unknown agent: 404.
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/api/v1/repos/{repo_id}/agents/ghost/branch-lifecycle"
                    ))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    /// Same-second commit ordering: recorded timestamps have second
    /// granularity, so two commits recorded within one second tie. The
    /// tie must be broken by the producing session's window (durable
    /// chronology at session granularity), never by the random-UUID row
    /// id — a bare id tiebreak orders the tie arbitrarily and the
    /// lifecycle view flips (the CI failure this regresses against).
    /// The commits are written directly with identical timestamps so the
    /// tie is pinned deterministically, not by wall-clock coincidence.
    #[tokio::test]
    async fn branch_lifecycle_same_second_tie_ordered_by_session() {
        let (state, app) = state_app();
        let (app, repo_id) = create_repo(app).await;
        make_agent(&state, "agent-1").await;

        open_session(&state, "sess-1", "agent-1", &repo_id, 1000, "feat/x").await;
        open_session(&state, "sess-2", "agent-1", &repo_id, 2000, "feat/y").await;

        // Both commits recorded in the same wall-clock second (identical
        // timestamps), each stamped to its open session — exactly what
        // record_commit produces for a same-second pair.
        let same_second = 1_800_000_000u64;
        for (sha, session) in [("CCCC", "sess-1"), ("DDDD", "sess-2")] {
            let ac = gyre_domain::AgentCommit::new(
                gyre_common::Id::new(format!("ac-{sha}")),
                gyre_common::Id::new("agent-1"),
                gyre_common::Id::new(&repo_id),
                sha,
                "feat/x",
                same_second,
            )
            .with_session_id(Some(session.to_string()));
            state.agent_commits.record(&ac).await.unwrap();
        }

        let view = get_json(
            &app,
            format!("/api/v1/repos/{repo_id}/agents/agent-1/branch-lifecycle"),
        )
        .await;
        let commits: Vec<&str> = view["observed_commits"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["commit_sha"].as_str().unwrap())
            .collect();
        assert_eq!(
            commits,
            vec!["CCCC", "DDDD"],
            "same-second commits order by session window, not random id"
        );
    }

    /// Time-travel: the session state view returns the repo state during
    /// the session's active window and the completion snapshot — a real
    /// `git log --after/--before` query against a real object store, so a
    /// commit pushed after the session ended is excluded and the result
    /// differs from current HEAD.
    #[tokio::test]
    async fn session_state_time_travel() {
        let state = crate::mem::test_state();
        let app = crate::api::api_router().with_state(state.clone());

        // A real git repo on disk (the query surface is the real object
        // store — a mocked stand-in would prove nothing).
        let dir = tempfile::tempdir().unwrap();
        let repo_path = dir.path().join("demo.git");
        std::fs::create_dir_all(&repo_path).unwrap();
        let run = |args: &[&str]| git_at(repo_path.as_path(), args, None);
        run(&["init", "--bare", "-b", "main"]);
        let work = tempfile::tempdir().unwrap();
        let run_work = |args: &[&str], date: Option<u64>| git_at(work.path(), args, date);
        run_work(&["init", "-b", "main"], None);
        std::fs::write(work.path().join("base.txt"), "base\n").unwrap();
        run_work(&["add", "base.txt"], None);
        run_work(&["commit", "-m", "base"], None);
        run_work(
            &["push", repo_path.to_str().unwrap(), "HEAD:refs/heads/main"],
            None,
        );

        let repo = gyre_domain::Repository::new(
            gyre_common::Id::new("repo-tt"),
            gyre_common::Id::new("ws-1"),
            "tt",
            repo_path.to_str().unwrap().to_string(),
            0,
        );
        state.repos.create(&repo).await.unwrap();
        make_agent(&state, "agent-tt").await;

        // Deterministic session window: [now-3000, now-2000].
        let now = crate::api::now_secs();
        let started_at = now - 3000;
        let ended_at = now - 2000;

        // A commit dated INSIDE the window, pushed to the repo.
        std::fs::write(work.path().join("during.txt"), "during\n").unwrap();
        run_work(&["add", "during.txt"], Some(started_at + 100));
        run_work(&["commit", "-m", "during session"], Some(started_at + 100));
        run_work(
            &["push", repo_path.to_str().unwrap(), "HEAD:refs/heads/main"],
            None,
        );
        let during_sha = String::from_utf8_lossy(
            &std::process::Command::new("git")
                .arg("-C")
                .arg(work.path())
                .args(["rev-parse", "HEAD"])
                .output()
                .unwrap()
                .stdout,
        )
        .trim()
        .to_string();

        open_session(&state, "sess-tt", "agent-tt", "repo-tt", started_at, "main").await;

        // Snapshot ref at the completion point, then end the session.
        run(&[
            "update-ref",
            "refs/agents/agent-tt/snapshots/0",
            &during_sha,
        ]);
        end_session(&state, "sess-tt", ended_at, Some("main")).await;

        // A commit dated AFTER the session ended — must be excluded from
        // the session's window, and is the current HEAD (so the view
        // demonstrably differs from current HEAD).
        std::fs::write(work.path().join("after.txt"), "after\n").unwrap();
        run_work(&["add", "after.txt"], Some(ended_at + 1000));
        run_work(&["commit", "-m", "after session"], Some(ended_at + 1000));
        run_work(
            &["push", repo_path.to_str().unwrap(), "HEAD:refs/heads/main"],
            None,
        );
        let after_sha = String::from_utf8_lossy(
            &std::process::Command::new("git")
                .arg("-C")
                .arg(work.path())
                .args(["rev-parse", "HEAD"])
                .output()
                .unwrap()
                .stdout,
        )
        .trim()
        .to_string();

        let view = get_json(
            &app,
            "/api/v1/repos/repo-tt/agent-sessions/sess-tt/state".to_string(),
        )
        .await;

        let shas: Vec<&str> = view["commits_during_session"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["sha"].as_str().unwrap())
            .collect();
        assert!(
            shas.contains(&during_sha.as_str()),
            "commit pushed during the session must be present"
        );
        assert!(
            !shas.contains(&after_sha.as_str()),
            "commit pushed after the session ended must be excluded"
        );

        let snap = view["completion_snapshot"].as_object().expect("snapshot");
        assert_eq!(snap["commit_sha"].as_str().unwrap(), during_sha);
        assert_eq!(
            snap["ref_name"].as_str().unwrap(),
            "refs/agents/agent-tt/snapshots/0"
        );
        let files: Vec<&str> = snap["files"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| f.as_str().unwrap())
            .collect();
        assert!(
            files.contains(&"during.txt"),
            "snapshot tree files: {files:?}"
        );
        assert!(
            !files.contains(&"after.txt"),
            "snapshot must predate the after-commit"
        );

        // A session in another repo is not visible from this route.
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/repos/repo-other/agent-sessions/sess-tt/state")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }
}
