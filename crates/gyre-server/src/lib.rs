pub(crate) mod activity;
pub mod api;
pub(crate) mod auth;
pub(crate) mod git_http;
pub(crate) mod health;
pub(crate) mod mem;
pub mod merge_processor;
pub(crate) mod messages;
pub mod metrics;
pub mod middleware;
pub(crate) mod spa;
pub mod telemetry;
pub(crate) mod ws;

use axum::{routing::get, Router};
use gyre_common::ActivityEventData;
use gyre_domain::AgentStatus;
use gyre_ports::{
    AgentCommitRepository, AgentRepository, ApiKeyRepository, GitOpsPort, MergeQueueRepository,
    MergeRequestRepository, ProjectRepository, RepoRepository, ReviewRepository, TaskRepository,
    UserRepository, WorktreeRepository,
};
use messages::AgentMessage;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use tokio::sync::{broadcast, Mutex};
use tracing::info;

/// Configuration for OIDC/JWT validation.
#[derive(Clone)]
pub struct JwtConfig {
    pub issuer: String,
    pub audience: Option<String>,
    /// kid → DecodingKey cache. Pre-populated in tests; lazily refreshed from JWKS in production.
    /// Uses `std::sync::RwLock` so it can be written from synchronous (test) code without
    /// requiring a tokio runtime or `block_in_place`.
    pub keys: Arc<std::sync::RwLock<HashMap<String, jsonwebtoken::DecodingKey>>>,
}

impl JwtConfig {
    pub fn new(issuer: impl Into<String>, audience: Option<String>) -> Self {
        Self {
            issuer: issuer.into(),
            audience,
            keys: Arc::new(std::sync::RwLock::new(HashMap::new())),
        }
    }

    /// Insert a decoding key directly (used in tests and initial JWKS load).
    pub fn insert_key(&self, kid: impl Into<String>, key: jsonwebtoken::DecodingKey) {
        self.keys.write().unwrap().insert(kid.into(), key);
    }
}

/// Shared application state available to all handlers.
#[derive(Clone)]
pub struct AppState {
    pub auth_token: String,
    /// Base URL for building clone URLs, e.g. "http://localhost:3000".
    pub base_url: String,
    pub projects: Arc<dyn ProjectRepository>,
    pub repos: Arc<dyn RepoRepository>,
    pub agents: Arc<dyn AgentRepository>,
    pub tasks: Arc<dyn TaskRepository>,
    pub merge_requests: Arc<dyn MergeRequestRepository>,
    pub reviews: Arc<dyn ReviewRepository>,
    pub merge_queue: Arc<dyn MergeQueueRepository>,
    pub git_ops: Arc<dyn GitOpsPort>,
    pub agent_commits: Arc<dyn AgentCommitRepository>,
    pub worktrees: Arc<dyn WorktreeRepository>,
    pub activity_store: activity::ActivityStore,
    pub broadcast_tx: broadcast::Sender<ActivityEventData>,
    /// Per-agent message inboxes: agent_id -> queued messages.
    pub agent_messages: Arc<Mutex<HashMap<String, VecDeque<AgentMessage>>>>,
    /// Auth tokens issued on agent registration: agent_id -> token.
    pub agent_tokens: Arc<Mutex<HashMap<String, String>>>,
    /// User repository for JWT/SSO user management.
    pub users: Arc<dyn UserRepository>,
    /// API key repository: key -> user_id.
    pub api_keys: Arc<dyn ApiKeyRepository>,
    /// OIDC/JWT configuration. None = JWT auth disabled (agent tokens only).
    pub jwt_config: Option<Arc<JwtConfig>>,
    /// HTTP client for OIDC JWKS fetching.
    pub http_client: reqwest::Client,
    /// Prometheus metrics.
    pub metrics: Arc<metrics::Metrics>,
}

/// Build the axum Router (extracted for testability).
pub fn build_router(state: Arc<AppState>) -> Router {
    use axum::routing::post;

    Router::new()
        .route("/health", get(health::health_handler))
        .route("/metrics", get(metrics::metrics_handler))
        .route("/ws", get(ws::ws_handler))
        // Git smart HTTP -- auth enforced per-handler via AuthenticatedAgent extractor.
        .route(
            "/git/:project/:repo/info/refs",
            get(git_http::git_info_refs),
        )
        .route(
            "/git/:project/:repo/git-upload-pack",
            post(git_http::git_upload_pack),
        )
        .route(
            "/git/:project/:repo/git-receive-pack",
            post(git_http::git_receive_pack),
        )
        .route("/", get(spa::spa_handler))
        .route("/*path", get(spa::spa_handler))
        .merge(api::api_router())
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            middleware::request_tracing,
        ))
        .with_state(state)
}

/// Build application state with in-memory repositories and real git operations.
/// Used by both production (main) and integration tests.
pub fn build_state(auth_token: &str, base_url: &str) -> Arc<AppState> {
    let (broadcast_tx, _) = broadcast::channel(256);
    let metrics = Arc::new(metrics::Metrics::new().expect("failed to create Prometheus metrics"));
    Arc::new(AppState {
        auth_token: auth_token.to_string(),
        base_url: base_url.to_string(),
        projects: Arc::new(mem::MemProjectRepository::default()),
        repos: Arc::new(mem::MemRepoRepository::default()),
        agents: Arc::new(mem::MemAgentRepository::default()),
        tasks: Arc::new(mem::MemTaskRepository::default()),
        merge_requests: Arc::new(mem::MemMrRepository::default()),
        reviews: Arc::new(mem::MemReviewRepository::default()),
        merge_queue: Arc::new(mem::MemMergeQueueRepository::default()),
        git_ops: Arc::new(gyre_adapters::Git2OpsAdapter::new()),
        agent_commits: Arc::new(mem::MemAgentCommitRepository::default()),
        worktrees: Arc::new(mem::MemWorktreeRepository::default()),
        activity_store: activity::ActivityStore::new(),
        broadcast_tx,
        agent_messages: Arc::new(Mutex::new(HashMap::new())),
        agent_tokens: Arc::new(Mutex::new(HashMap::new())),
        users: Arc::new(mem::MemUserRepository::default()),
        api_keys: Arc::new(mem::MemApiKeyRepository::default()),
        jwt_config: None,
        http_client: reqwest::Client::new(),
        metrics,
    })
}

/// Periodically scan for agents that have stopped sending heartbeats and mark them Dead.
/// When an agent is marked Dead:
///   - Its worktrees are cleaned up (git remove + DB delete)
///   - Its assigned task (if any) is transitioned to Blocked
///   - An ActivityEvent is recorded
pub fn spawn_stale_agent_detector(state: Arc<AppState>) {
    const CHECK_INTERVAL_SECS: u64 = 30;
    const HEARTBEAT_TIMEOUT_SECS: u64 = 60;

    tokio::spawn(async move {
        let mut interval =
            tokio::time::interval(tokio::time::Duration::from_secs(CHECK_INTERVAL_SECS));
        loop {
            interval.tick().await;
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();

            match state.agents.list().await {
                Ok(agents) => {
                    for mut agent in agents {
                        if agent.status != AgentStatus::Dead
                            && !agent.is_alive(now, HEARTBEAT_TIMEOUT_SECS)
                        {
                            info!(agent_id = %agent.id, agent_name = %agent.name,
                                  "marking stale agent as dead");
                            let _ = agent.transition_status(AgentStatus::Dead);
                            let _ = state.agents.update(&agent).await;

                            // Clean up worktrees
                            if let Ok(worktrees) = state.worktrees.find_by_agent(&agent.id).await {
                                for wt in worktrees {
                                    if let Ok(Some(repo)) =
                                        state.repos.find_by_id(&wt.repository_id).await
                                    {
                                        if let Err(e) = state
                                            .git_ops
                                            .remove_worktree(&repo.path, &wt.path)
                                            .await
                                        {
                                            tracing::warn!(
                                                "remove_worktree failed for agent {}: {e}",
                                                agent.id
                                            );
                                        }
                                    }
                                    let _ = state.worktrees.delete(&wt.id).await;
                                }
                            }

                            // Block the assigned task
                            if let Some(task_id) = &agent.current_task_id {
                                if let Ok(Some(mut task)) = state.tasks.find_by_id(task_id).await {
                                    use gyre_domain::TaskStatus;
                                    if task.status == TaskStatus::InProgress {
                                        let _ = task.transition_status(TaskStatus::Blocked);
                                        task.updated_at = now;
                                        let _ = state.tasks.update(&task).await;
                                    }
                                }
                            }

                            // Record ActivityEvent
                            state.activity_store.record(ActivityEventData {
                                event_id: uuid::Uuid::new_v4().to_string(),
                                agent_id: agent.id.to_string(),
                                event_type: "agent.dead".to_string(),
                                description: format!(
                                    "Agent {} marked dead (no heartbeat)",
                                    agent.name
                                ),
                                timestamp: now,
                            });
                        }
                    }
                }
                Err(e) => {
                    tracing::error!("stale agent check failed: {e}");
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, Router};
    use http::{Request, StatusCode};
    use tower::ServiceExt;

    fn test_app() -> Router {
        build_router(mem::test_state())
    }

    #[tokio::test]
    async fn integration_health_endpoint() {
        let app = test_app();
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["status"], "ok");
        assert_eq!(json["version"], "0.1.0");
    }
}
