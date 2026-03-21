pub(crate) mod abac;
pub(crate) mod activity;
pub mod api;
pub mod attestation;
pub mod audit_simulator;
pub(crate) mod auth;
pub mod commit_signatures;
pub mod domain_events;
pub mod gate_executor;
pub(crate) mod git_http;
pub mod git_refs;
pub mod procfs_monitor;

pub(crate) mod health;
pub mod jobs;
pub(crate) mod mcp;
pub(crate) mod mem;
pub mod merge_processor;
pub(crate) mod messages;
pub mod metrics;
pub mod middleware;
pub mod mirror_sync;
pub(crate) mod oidc;
pub mod pre_accept;
pub mod rate_limit;
pub(crate) mod rbac;
pub mod retention;
pub mod siem;
pub(crate) mod snapshot;
pub(crate) mod spa;
pub mod speculative_merge;
// sqlite.rs (rusqlite) removed — use gyre_adapters::SqliteStorage (Diesel) instead.
pub mod stale_agents;
pub mod telemetry;
pub(crate) mod tty;
pub mod version_compute;
pub mod workload_attestation;
pub(crate) mod ws;

use axum::{routing::get, Router};
use domain_events::DomainEvent;
use gyre_common::ActivityEventData;
use gyre_domain::AgentCard;
use gyre_ports::{
    AgentCommitRepository, AgentRepository, AnalyticsRepository, ApiKeyRepository, AuditRepository,
    CostRepository, GitOpsPort, JjOpsPort, MergeQueueRepository, MergeRequestRepository,
    NetworkPeerRepository, PreAcceptGate, ProcessHandle, ProjectRepository, RepoRepository,
    ReviewRepository, SpawnLogRepository, TaskRepository, UserRepository, WorktreeRepository,
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
    /// Ed25519 signing key for Gyre's built-in OIDC provider (M18).
    /// Used to mint and verify agent JWTs returned by POST /api/v1/agents/spawn.
    pub agent_signing_key: Arc<auth::AgentSigningKey>,
    /// Agent JWT TTL in seconds. Configurable via GYRE_AGENT_JWT_TTL (default: 3600).
    pub agent_jwt_ttl_secs: u64,
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
    /// Running agent processes: agent_id -> ProcessHandle.
    pub process_registry: Arc<Mutex<HashMap<String, ProcessHandle>>>,
    /// Per-agent log buffers: agent_id -> lines in "[ts] message" format.
    pub agent_logs: Arc<Mutex<HashMap<String, Vec<String>>>>,
    /// Per-agent broadcast channels for live log SSE streaming.
    pub agent_log_tx: Arc<Mutex<HashMap<String, broadcast::Sender<String>>>>,
    /// Quality gates per repository: gate_id -> QualityGate.
    pub quality_gates: Arc<Mutex<HashMap<String, gyre_domain::QualityGate>>>,
    /// Gate execution results: result_id -> GateResult.
    pub gate_results: Arc<Mutex<HashMap<String, gyre_domain::GateResult>>>,
    /// Pre-accept gate registry: built-in gate implementations.
    pub push_gate_registry: Arc<Vec<Box<dyn PreAcceptGate>>>,
    /// Per-repo active push gate names: repo_id -> list of gate names.
    pub repo_push_gates: Arc<Mutex<HashMap<String, Vec<String>>>>,
    /// Speculative merge results: (repo_id, branch) -> SpeculativeResult (M13.5).
    pub speculative_results:
        Arc<Mutex<HashMap<(String, String), speculative_merge::SpeculativeResult>>>,
    /// Spawn log: persisted to DB for diagnostic recovery (M13.7).
    pub spawn_log: Arc<dyn SpawnLogRepository>,
    /// Agent stack fingerprints: agent_id -> AgentStack (M14.1).
    pub agent_stacks: Arc<Mutex<HashMap<String, api::stack_attest::AgentStack>>>,
    /// Repo stack attestation policies: repo_id -> required fingerprint (M14.2).
    pub repo_stack_policies: Arc<Mutex<HashMap<String, String>>>,
    /// Base SQLite storage instance (the "default" tenant).
    pub db_storage: Option<Arc<gyre_adapters::SqliteStorage>>,
    /// Spec approval ledger: approval_id -> SpecApproval (agent-gates).
    pub spec_approvals: Arc<Mutex<HashMap<String, gyre_domain::SpecApproval>>>,
    /// Per-repo spec enforcement policies: repo_id -> SpecPolicy (M12.3).
    pub spec_policies: Arc<Mutex<HashMap<String, api::spec_policy::SpecPolicy>>>,
    /// Merge attestation bundles: mr_id -> AttestationBundle (G5).
    pub attestation_store: Arc<Mutex<HashMap<String, attestation::AttestationBundle>>>,
    /// Trusted remote Gyre base URLs for cross-instance JWT federation (G11).
    /// Populated from `GYRE_TRUSTED_ISSUERS` (comma-separated list of base URLs).
    pub trusted_issuers: Vec<String>,
    /// Cached JWKS from trusted remote Gyre instances: issuer URL -> entry (G11).
    pub remote_jwks_cache: Arc<tokio::sync::RwLock<HashMap<String, auth::RemoteJwksEntry>>>,
    /// Commit signatures produced by jj squash (M13.8 Sigstore): commit_sha -> CommitSignature.
    pub commit_signatures: commit_signatures::CommitSignatureStore,
    /// Sigstore signing mode (local Ed25519 or Fulcio CA).  Set via `GYRE_SIGSTORE_MODE`.
    pub sigstore_mode: commit_signatures::SigstoreMode,
    /// Per-repo ABAC policies: repo_id -> list of AbacPolicy (G6).
    pub abac_policies: Arc<Mutex<HashMap<String, Vec<abac::AbacPolicy>>>>,
    /// Workload attestation records: agent_id -> WorkloadAttestation (G10).
    pub workload_attestations:
        Arc<Mutex<HashMap<String, workload_attestation::WorkloadAttestation>>>,
    /// Active SSH tunnels: tunnel_id -> TunnelRecord (G12).
    pub tunnel_store: Arc<Mutex<HashMap<String, api::compute::TunnelRecord>>>,
}

/// Global authentication middleware for all `/api/v1/` routes.
///
/// Rejects any request without a valid `Authorization: Bearer <token>` header
/// with `401 Unauthorized`. The `/api/v1/version` endpoint is public.
/// Per-handler extractors (`AuthenticatedAgent`, `AdminOnly`, etc.) still
/// enforce finer-grained role checks on top of this.
async fn require_auth_middleware(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    use axum::response::IntoResponse;

    // /api/v1/version is intentionally public.
    if req.uri().path() == "/api/v1/version" {
        return next.run(req).await;
    }

    let token = req
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));

    match token {
        Some(t) if auth::tokens_equal(t, &state.auth_token) => next.run(req).await,
        Some(t) => {
            // Check per-agent tokens using constant-time compare.
            let agent_tokens = state.agent_tokens.lock().await;
            let valid = agent_tokens
                .values()
                .any(|v| auth::tokens_equal(v.as_str(), t));
            drop(agent_tokens);
            if valid {
                return next.run(req).await;
            }
            // Check API keys via the hashed key repository.
            if let Ok(Some(_)) = state.api_keys.find_user_id(&auth::hash_api_key(t)).await {
                return next.run(req).await;
            }
            // Check JWT (Keycloak/OIDC) if configured.
            if let Some(jwt_cfg) = &state.jwt_config {
                if auth::validate_jwt_middleware(t, jwt_cfg, &state)
                    .await
                    .is_ok()
                {
                    return next.run(req).await;
                }
            }
            // Check federation JWT from trusted remote Gyre instances (G11).
            if t.starts_with("ey")
                && !state.trusted_issuers.is_empty()
                && auth::validate_federated_jwt_middleware(t, &state)
                    .await
                    .is_ok()
            {
                return next.run(req).await;
            }
            (axum::http::StatusCode::UNAUTHORIZED, "Invalid token").into_response()
        }
        None => (axum::http::StatusCode::UNAUTHORIZED, "Missing Bearer token").into_response(),
    }
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

    // CORS: configurable via GYRE_CORS_ORIGINS (comma-separated), default localhost only.
    let cors = build_cors_layer();

    // Apply global API auth middleware to all /api/v1/ routes.
    let api = api::api_router().layer(axum::middleware::from_fn_with_state(
        state.clone(),
        require_auth_middleware,
    ));

    Router::new()
        .route("/health", get(health::health_handler))
        .route("/healthz", get(health::healthz_handler))
        .route("/readyz", get(health::readyz_handler))
        .route("/metrics", get(metrics::metrics_handler))
        // OIDC discovery endpoints (M18) — no auth required.
        .route(
            "/.well-known/openid-configuration",
            get(oidc::openid_configuration),
        )
        .route("/.well-known/jwks.json", get(oidc::jwks))
        .route("/ws", get(ws::ws_handler))
        .route("/ws/agents/:id/tty", get(tty::tty_handler))
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
        .merge(api)
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
///
/// When `GYRE_CORS_ORIGINS` is not set, the default list includes the server's
/// own port (from `GYRE_PORT`) so that the dashboard works on non-default ports
/// without requiring explicit CORS configuration.
fn build_cors_layer() -> tower_http::cors::CorsLayer {
    use tower_http::cors::{AllowHeaders, AllowMethods, AllowOrigin, CorsLayer};

    let origins_str = std::env::var("GYRE_CORS_ORIGINS").unwrap_or_else(|_| {
        // Always include the server's own port so preflight requests succeed
        // when running on a non-default port (e.g. GYRE_PORT=2223).
        let server_port: u16 = std::env::var("GYRE_PORT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(3000);
        let server_origin = format!("http://localhost:{server_port}");
        let mut defaults = vec![
            "http://localhost:2222".to_string(),
            "http://localhost:3000".to_string(),
            "http://localhost:5173".to_string(),
        ];
        if !defaults.contains(&server_origin) {
            defaults.push(server_origin);
        }
        defaults.join(",")
    });

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

/// Build application state. When `GYRE_DATABASE_URL` is set (e.g. `sqlite://gyre.db`),
/// uses SQLite-backed repositories; otherwise falls back to in-memory stores.
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

    let db_url = std::env::var("GYRE_DATABASE_URL").ok();
    let sqlite_db: Option<Arc<gyre_adapters::SqliteStorage>> = db_url
        .as_deref()
        .filter(|u| !u.starts_with("postgres://") && !u.starts_with("postgresql://"))
        .map(|url| {
            let path = url.strip_prefix("sqlite://").unwrap_or(url).to_string();
            Arc::new(
                gyre_adapters::SqliteStorage::new(&path)
                    .unwrap_or_else(|e| panic!("Failed to open SQLite at {path}: {e}")),
            )
        });
    let pg_db: Option<Arc<gyre_adapters::PgStorage>> = db_url
        .as_deref()
        .filter(|u| u.starts_with("postgres://") || u.starts_with("postgresql://"))
        .map(|url| {
            Arc::new(
                gyre_adapters::PgStorage::new(url)
                    .unwrap_or_else(|e| panic!("Failed to connect to PostgreSQL: {e}")),
            )
        });
    let db_storage = sqlite_db.clone();

    macro_rules! store {
        ($trait:ty, $mem:expr) => {
            if let Some(ref d) = pg_db {
                Arc::clone(d) as Arc<$trait>
            } else if let Some(ref d) = sqlite_db {
                Arc::clone(d) as Arc<$trait>
            } else {
                Arc::new($mem) as Arc<$trait>
            }
        };
    }

    Arc::new(AppState {
        auth_token: auth_token.to_string(),
        base_url: base_url.to_string(),
        projects: store!(dyn ProjectRepository, mem::MemProjectRepository::default()),
        repos: store!(dyn RepoRepository, mem::MemRepoRepository::default()),
        agents: store!(dyn AgentRepository, mem::MemAgentRepository::default()),
        tasks: store!(dyn TaskRepository, mem::MemTaskRepository::default()),
        merge_requests: store!(dyn MergeRequestRepository, mem::MemMrRepository::default()),
        reviews: store!(dyn ReviewRepository, mem::MemReviewRepository::default()),
        merge_queue: store!(
            dyn MergeQueueRepository,
            mem::MemMergeQueueRepository::default()
        ),
        git_ops: Arc::new(gyre_adapters::Git2OpsAdapter::new()),
        jj_ops: Arc::new(gyre_adapters::JjOpsAdapter::new()),
        agent_commits: store!(
            dyn AgentCommitRepository,
            mem::MemAgentCommitRepository::default()
        ),
        worktrees: store!(
            dyn WorktreeRepository,
            mem::MemWorktreeRepository::default()
        ),
        activity_store: activity::ActivityStore::new(),
        broadcast_tx,
        event_tx,
        agent_messages: Arc::new(Mutex::new(HashMap::new())),
        agent_tokens: Arc::new(Mutex::new(HashMap::new())),
        agent_signing_key: Arc::new(auth::AgentSigningKey::generate()),
        agent_jwt_ttl_secs: std::env::var("GYRE_AGENT_JWT_TTL")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(3600),
        users: store!(dyn UserRepository, mem::MemUserRepository::default()),
        api_keys: store!(dyn ApiKeyRepository, mem::MemApiKeyRepository::default()),
        jwt_config,
        http_client: reqwest::Client::new(),
        metrics,
        started_at_secs,
        agent_cards: Arc::new(Mutex::new(HashMap::new())),
        compose_sessions: Arc::new(Mutex::new(HashMap::new())),
        retention_store: RetentionStore::new(),
        job_registry: Arc::new(JobRegistry::new()),
        analytics: store!(
            dyn AnalyticsRepository,
            mem::MemAnalyticsRepository::default()
        ),
        costs: store!(dyn CostRepository, mem::MemCostRepository::default()),
        audit: store!(dyn AuditRepository, mem::MemAuditRepository::default()),
        siem_store: SiemStore::new(),
        audit_broadcast_tx,
        compute_targets: Arc::new(Mutex::new(HashMap::new())),
        network_peers: store!(
            dyn NetworkPeerRepository,
            mem::MemNetworkPeerRepository::default()
        ),
        rate_limiter: rate_limit::RateLimiter::new(rate_per_sec),
        process_registry: Arc::new(Mutex::new(HashMap::new())),
        agent_logs: Arc::new(Mutex::new(HashMap::new())),
        agent_log_tx: Arc::new(Mutex::new(HashMap::new())),
        quality_gates: Arc::new(Mutex::new(HashMap::new())),
        gate_results: Arc::new(Mutex::new(HashMap::new())),
        push_gate_registry: Arc::new(pre_accept::builtin_gates()),
        repo_push_gates: Arc::new(Mutex::new(HashMap::new())),
        speculative_results: Arc::new(Mutex::new(HashMap::new())),
        spawn_log: store!(
            dyn SpawnLogRepository,
            mem::MemSpawnLogRepository::default()
        ),
        agent_stacks: Arc::new(Mutex::new(HashMap::new())),
        repo_stack_policies: Arc::new(Mutex::new(HashMap::new())),
        db_storage,
        spec_approvals: Arc::new(Mutex::new(HashMap::new())),
        spec_policies: Arc::new(Mutex::new(HashMap::new())),
        attestation_store: Arc::new(Mutex::new(HashMap::new())),
        trusted_issuers: std::env::var("GYRE_TRUSTED_ISSUERS")
            .ok()
            .map(|v| {
                v.split(',')
                    .map(|s| s.trim().trim_end_matches('/').to_string())
                    .filter(|s| !s.is_empty())
                    .collect()
            })
            .unwrap_or_default(),
        remote_jwks_cache: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
        commit_signatures: Arc::new(Mutex::new(HashMap::new())),
        sigstore_mode: commit_signatures::SigstoreMode::from_env(),
        abac_policies: Arc::new(Mutex::new(HashMap::new())),
        workload_attestations: Arc::new(Mutex::new(HashMap::new())),
        tunnel_store: Arc::new(Mutex::new(HashMap::new())),
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
