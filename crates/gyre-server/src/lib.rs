pub(crate) mod activity;
pub mod api;
pub mod audit_simulator;
pub(crate) mod auth;
pub mod domain_events;
pub(crate) mod git_http;
pub(crate) mod health;
pub mod jobs;
pub(crate) mod mcp;
pub(crate) mod mem;
pub mod merge_processor;
pub(crate) mod messages;
pub mod metrics;
pub mod middleware;
pub mod rate_limit;
pub(crate) mod rbac;
pub mod retention;
pub mod siem;
pub(crate) mod snapshot;
pub(crate) mod spa;
pub mod stale_agents;
pub mod telemetry;
pub(crate) mod ws;

use axum::{routing::get, Router};
use domain_events::DomainEvent;
use gyre_common::ActivityEventData;
use gyre_domain::AgentCard;
use gyre_ports::{
    AgentCommitRepository, AgentRepository, AnalyticsRepository, ApiKeyRepository, AuditRepository,
    CostRepository, GitOpsPort, JjOpsPort, MergeQueueRepository, MergeRequestRepository,
    NetworkPeerRepository, ProjectRepository, RepoRepository, ReviewRepository, TaskRepository,
    UserRepository, WorktreeRepository,
};
use jobs::JobRegistry;
use messages::AgentMessage;
use retention::RetentionStore;
use siem::SiemStore;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use tokio::sync::{broadcast, Mutex};

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
    pub jj_ops: Arc<dyn JjOpsPort>,
    pub agent_commits: Arc<dyn AgentCommitRepository>,
    pub worktrees: Arc<dyn WorktreeRepository>,
    pub activity_store: activity::ActivityStore,
    pub broadcast_tx: broadcast::Sender<ActivityEventData>,
    /// Domain event bus: broadcasts structured domain events to all WS clients.
    pub event_tx: broadcast::Sender<DomainEvent>,
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
    /// Server start time as Unix epoch seconds (for uptime calculation).
    pub started_at_secs: u64,
    /// A2A Agent Cards: agent_id -> AgentCard for discovery.
    pub agent_cards: Arc<Mutex<HashMap<String, AgentCard>>>,
    /// Compose sessions: compose_id -> list of agent_ids.
    pub compose_sessions: Arc<Mutex<HashMap<String, Vec<String>>>>,
    /// Data retention policies.
    pub retention_store: RetentionStore,
    /// Background job registry.
    pub job_registry: Arc<JobRegistry>,
    /// Product analytics event store.
    pub analytics: Arc<dyn AnalyticsRepository>,
    /// Cost tracking store.
    pub costs: Arc<dyn CostRepository>,
    /// Audit event store.
    pub audit: Arc<dyn AuditRepository>,
    /// SIEM target store and forwarding state.
    pub siem_store: SiemStore,
    /// Broadcast channel for live audit event SSE stream.
    pub audit_broadcast_tx: broadcast::Sender<String>,
    /// Admin-configured compute targets: id -> ComputeTargetConfig.
    pub compute_targets: Arc<Mutex<HashMap<String, api::compute::ComputeTargetConfig>>>,
    /// WireGuard network peer registry.
    pub network_peers: Arc<dyn NetworkPeerRepository>,
    /// Request rate limiter (requests/sec).
    pub rate_limiter: Arc<rate_limit::RateLimiter>,
}

/// Build the axum Router (extracted for testability).
pub fn build_router(state: Arc<AppState>) -> Router {
    use axum::extract::DefaultBodyLimit;
    use axum::routing::post;
    use tower_http::catch_panic::CatchPanicLayer;

    // Body size limit: configurable via GYRE_MAX_BODY_SIZE (bytes), default 10MB.
    let max_body_bytes: usize = std::env::var("GYRE_MAX_BODY_SIZE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(10 * 1024 * 1024);

    // CORS: configurable via GYRE_CORS_ORIGINS (comma-separated), default "*".
    let cors = build_cors_layer();

    Router::new()
        .route("/health", get(health::health_handler))
        .route("/healthz", get(health::healthz_handler))
        .route("/readyz", get(health::readyz_handler))
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
        // MCP (Model Context Protocol) endpoints
        .route("/mcp", post(mcp::mcp_handler))
        .route("/mcp/sse", get(mcp::mcp_sse_handler))
        .route("/", get(spa::spa_handler))
        .route("/*path", get(spa::spa_handler))
        .merge(api::api_router())
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            middleware::rate_limit_middleware,
        ))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            middleware::request_tracing,
        ))
        .layer(DefaultBodyLimit::max(max_body_bytes))
        .layer(cors)
        .layer(CatchPanicLayer::new())
        .with_state(state)
}

/// Build a CORS layer from `GYRE_CORS_ORIGINS` env var.
fn build_cors_layer() -> tower_http::cors::CorsLayer {
    use tower_http::cors::{AllowHeaders, AllowMethods, AllowOrigin, CorsLayer};

    let origins_str = std::env::var("GYRE_CORS_ORIGINS").unwrap_or_else(|_| "*".to_string());

    if origins_str == "*" {
        CorsLayer::new()
            .allow_origin(AllowOrigin::any())
            .allow_methods(AllowMethods::any())
            .allow_headers(AllowHeaders::any())
    } else {
        let origins: Vec<axum::http::HeaderValue> = origins_str
            .split(',')
            .filter_map(|s| s.trim().parse().ok())
            .collect();
        CorsLayer::new()
            .allow_origin(AllowOrigin::list(origins))
            .allow_methods(AllowMethods::any())
            .allow_headers(AllowHeaders::any())
    }
}

/// Build application state with in-memory repositories and real git operations.
/// Used by both production (main) and integration tests.
pub fn build_state(
    auth_token: &str,
    base_url: &str,
    jwt_config: Option<Arc<JwtConfig>>,
) -> Arc<AppState> {
    let (broadcast_tx, _) = broadcast::channel(256);
    let (event_tx, _) = broadcast::channel(256);
    let (audit_broadcast_tx, _) = broadcast::channel(1024);
    let metrics = Arc::new(metrics::Metrics::new().expect("failed to create Prometheus metrics"));
    let started_at_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let rate_per_sec: u64 = std::env::var("GYRE_RATE_LIMIT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(100);
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
        jj_ops: Arc::new(gyre_adapters::JjOpsAdapter::new()),
        agent_commits: Arc::new(mem::MemAgentCommitRepository::default()),
        worktrees: Arc::new(mem::MemWorktreeRepository::default()),
        activity_store: activity::ActivityStore::new(),
        broadcast_tx,
        event_tx,
        agent_messages: Arc::new(Mutex::new(HashMap::new())),
        agent_tokens: Arc::new(Mutex::new(HashMap::new())),
        users: Arc::new(mem::MemUserRepository::default()),
        api_keys: Arc::new(mem::MemApiKeyRepository::default()),
        jwt_config,
        http_client: reqwest::Client::new(),
        metrics,
        started_at_secs,
        agent_cards: Arc::new(Mutex::new(HashMap::new())),
        compose_sessions: Arc::new(Mutex::new(HashMap::new())),
        retention_store: RetentionStore::new(),
        job_registry: Arc::new(JobRegistry::new()),
        analytics: Arc::new(mem::MemAnalyticsRepository::default()),
        costs: Arc::new(mem::MemCostRepository::default()),
        audit: Arc::new(mem::MemAuditRepository::default()),
        siem_store: SiemStore::new(),
        audit_broadcast_tx,
        compute_targets: Arc::new(Mutex::new(HashMap::new())),
        network_peers: Arc::new(mem::MemNetworkPeerRepository::default()),
        rate_limiter: rate_limit::RateLimiter::new(rate_per_sec),
    })
}

/// Delegate to keep backwards-compatibility. New code should use stale_agents::spawn_stale_agent_detector.
pub fn spawn_stale_agent_detector(state: Arc<AppState>) {
    stale_agents::spawn_stale_agent_detector(state);
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

    #[tokio::test]
    async fn healthz_returns_ok() {
        let app = test_app();
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/healthz")
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
    }

    #[tokio::test]
    async fn readyz_returns_ok() {
        let app = test_app();
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/readyz")
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
        assert!(json["checks"].is_object());
    }
}
