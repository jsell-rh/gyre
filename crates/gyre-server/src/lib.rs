#![recursion_limit = "256"]

pub(crate) mod abac;
pub mod abac_middleware;
pub mod api;
pub mod attestation;
pub mod audit_simulator;
pub(crate) mod auth;
pub mod commit_signatures;
pub(crate) mod constraint_check;
pub mod container_audit;
pub mod dep_staleness;
pub mod domain_events;
pub mod gate_executor;
pub(crate) mod git_http;
pub mod git_refs;
pub mod graph_extraction;
pub mod procfs_monitor;

pub(crate) mod health;
pub mod jobs;
pub mod llm_defaults;
pub mod llm_helpers;
pub mod llm_rate_limit;
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
pub(crate) mod signing;
pub(crate) mod snapshot;
pub(crate) mod spa;
pub mod spec_link_staleness;
pub mod spec_registry;
pub mod speculative_merge;
// sqlite.rs (rusqlite) removed — use gyre_adapters::SqliteStorage (Diesel) instead.
pub(crate) mod explorer_ws;
pub mod notifications;
pub mod otlp_receiver;
pub mod policy_engine;
pub mod stale_agents;
pub mod stale_peers;
pub mod telemetry;
pub mod trust_suggestion;
pub(crate) mod tty;
pub mod version_compute;
pub mod workload_attestation;
pub(crate) mod ws;

use axum::{routing::get, Router};
use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use gyre_common::message::{Destination, Message, MessageKind, MessageOrigin, TelemetryBuffer};
use gyre_common::Id;
use gyre_ports::{
    AgentCommitRepository, AgentRepository, AnalyticsRepository, ApiKeyRepository,
    AttestationRepository, AuditRepository, BreakingChangeRepository, BudgetRepository,
    BudgetUsageRepository, ComputeTargetRepository, ContainerAuditRepository,
    ConversationRepository, CostRepository, DependencyPolicyRepository, DependencyRepository,
    GateResultRepository, GitOpsPort, GraphPort, JjOpsPort, KvJsonStore, LlmConfigRepository,
    MergeQueueRepository, MergeRequestRepository, MetaSpecBindingRepository, MetaSpecRepository,
    MetaSpecSetRepository, NetworkPeerRepository, NotificationRepository, PersonaRepository,
    PolicyRepository, PreAcceptGate, ProcessHandle, PushGateRepository, QualityGateRepository,
    RepoRepository, ReviewRepository, SpawnLogRepository, SpecApprovalEventRepository,
    SpecApprovalRepository, SpecLedgerRepository, SpecPolicyRepository, TaskRepository,
    TeamRepository, TraceRepository, UserRepository, UserWorkspaceStateRepository,
    WorkspaceMembershipRepository, WorkspaceRepository, WorktreeRepository,
};
use jobs::JobRegistry;
use retention::RetentionStore;
use siem::SiemStore;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, Mutex, RwLock};

/// In-memory presence entry for a single browser-tab session.
#[derive(Clone)]
pub struct PresenceEntry {
    /// Workspace this session is viewing.
    pub workspace_id: String,
    /// Current view name (e.g. "inbox", "explorer", "specs").
    pub view: String,
    /// Client-supplied epoch milliseconds (informational only — NOT used for eviction).
    pub timestamp: u64,
    /// Server-recorded epoch milliseconds of the last `UserPresence` heartbeat.
    /// Used for idle eviction to prevent client clock manipulation.
    pub server_last_seen: u64,
    /// Connection ID — used to route `PresenceEvicted` to the right WS sender.
    pub connection_id: u64,
}

/// Configuration for the WireGuard coordination plane (M26.1).
#[derive(Clone)]
pub struct WireGuardConfig {
    /// Whether the WireGuard coordination plane is enabled (GYRE_WG_ENABLED).
    pub enabled: bool,
    /// CIDR pool for mesh IP allocation, e.g. "10.100.0.0/16" (GYRE_WG_CIDR).
    pub cidr: String,
    /// Server's WireGuard public key (GYRE_WG_SERVER_PUBKEY).
    pub server_pubkey: Option<String>,
    /// Server's WireGuard endpoint, e.g. "vpn.example.com:51820" (GYRE_WG_SERVER_ENDPOINT).
    pub server_endpoint: Option<String>,
    /// Seconds after which a peer is considered stale (GYRE_WG_PEER_TTL, default 300).
    pub peer_ttl_secs: u64,
    /// Monotonically increasing counter for mesh IP allocation (.2, .3, …).
    /// Counter value N allocates base_ip + N.
    pub ip_counter: Arc<std::sync::atomic::AtomicU32>,
}

impl WireGuardConfig {
    pub fn from_env() -> Self {
        let enabled = std::env::var("GYRE_WG_ENABLED")
            .map(|v| v == "true" || v == "1")
            .unwrap_or(false);
        let cidr = std::env::var("GYRE_WG_CIDR").unwrap_or_else(|_| "10.100.0.0/16".to_string());
        let server_pubkey = std::env::var("GYRE_WG_SERVER_PUBKEY").ok();
        let server_endpoint = std::env::var("GYRE_WG_SERVER_ENDPOINT").ok();
        let peer_ttl_secs = std::env::var("GYRE_WG_PEER_TTL")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(300);
        Self {
            enabled,
            cidr,
            server_pubkey,
            server_endpoint,
            peer_ttl_secs,
            ip_counter: Arc::new(std::sync::atomic::AtomicU32::new(2)),
        }
    }

    /// Parse the base IP from the CIDR and allocate the next available mesh IP.
    /// Returns None if the CIDR is malformed or the pool is exhausted.
    pub fn allocate_ip(&self) -> Option<String> {
        if !self.enabled {
            return None;
        }
        let cidr = self.cidr.split('/').next()?;
        let parts: Vec<u8> = cidr.split('.').filter_map(|p| p.parse().ok()).collect();
        if parts.len() != 4 {
            return None;
        }
        let base = u32::from_be_bytes([parts[0], parts[1], parts[2], parts[3]]);
        let offset = self
            .ip_counter
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let ip_num = base.checked_add(offset)?;
        let bytes = ip_num.to_be_bytes();
        Some(format!(
            "{}.{}.{}.{}",
            bytes[0], bytes[1], bytes[2], bytes[3]
        ))
    }
}

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
    /// In-memory ring buffer for Telemetry-tier messages (replaces ActivityStore).
    pub telemetry_buffer: Arc<TelemetryBuffer>,
    /// Unified broadcast channel for WebSocket message delivery (replaces broadcast_tx + event_tx).
    pub message_broadcast_tx: broadcast::Sender<Message>,
    /// Generic key-value JSON store for server-internal HashMap stores (M29.5B).
    /// Persists: agent_messages, agent_tokens, agent_cards, compute_targets,
    /// agent_stacks, repo_stack_policies, workload_attestations, workspace_repos,
    /// abac_policies — all keyed by (namespace, key).
    pub kv_store: Arc<dyn KvJsonStore>,
    /// Ed25519 signing key for Gyre's built-in OIDC provider (M18).
    /// Used to mint and verify agent JWTs returned by POST /api/v1/agents/spawn.
    pub agent_signing_key: Arc<auth::AgentSigningKey>,
    /// Agent JWT TTL in seconds. Configurable via GYRE_AGENT_JWT_TTL (default: 300, M27.5).
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
    /// WireGuard network peer registry.
    pub network_peers: Arc<dyn NetworkPeerRepository>,
    /// Cross-repo dependency graph (M22.4).
    pub dependencies: Arc<dyn DependencyRepository>,
    /// Breaking change records (M22.4 — TASK-020).
    pub breaking_changes: Arc<dyn BreakingChangeRepository>,
    /// Per-workspace dependency enforcement policies (M22.4 — TASK-020).
    pub dependency_policies: Arc<dyn DependencyPolicyRepository>,
    /// Request rate limiter (requests/sec).
    pub rate_limiter: Arc<rate_limit::RateLimiter>,
    /// Running agent processes: agent_id -> ProcessHandle.
    pub process_registry: Arc<Mutex<HashMap<String, ProcessHandle>>>,
    /// Per-agent log buffers: agent_id -> lines in "[ts] message" format.
    pub agent_logs: Arc<Mutex<HashMap<String, Vec<String>>>>,
    /// Per-agent broadcast channels for live log SSE streaming.
    pub agent_log_tx: Arc<Mutex<HashMap<String, broadcast::Sender<String>>>>,
    /// Quality gates per repository (persisted).
    pub quality_gates: Arc<dyn gyre_ports::QualityGateRepository>,
    /// Gate execution results (persisted).
    pub gate_results: Arc<dyn gyre_ports::GateResultRepository>,
    /// Pre-accept gate registry: built-in gate implementations.
    pub push_gate_registry: Arc<Vec<Box<dyn PreAcceptGate>>>,
    /// Per-repo active push gate names (persisted).
    pub repo_push_gates: Arc<dyn gyre_ports::PushGateRepository>,
    /// Speculative merge results: (repo_id, branch) -> SpeculativeResult (M13.5).
    pub speculative_results:
        Arc<Mutex<HashMap<(String, String), speculative_merge::SpeculativeResult>>>,
    /// Spawn log: persisted to DB for diagnostic recovery (M13.7).
    pub spawn_log: Arc<dyn SpawnLogRepository>,
    /// Base SQLite storage instance (the "default" tenant).
    pub db_storage: Option<Arc<gyre_adapters::SqliteStorage>>,
    /// Spec approval ledger (persisted).
    pub spec_approvals: Arc<dyn gyre_ports::SpecApprovalRepository>,
    /// Per-repo spec enforcement policies (persisted).
    pub spec_policies: Arc<dyn gyre_ports::SpecPolicyRepository>,
    /// Merge attestation bundles (persisted).
    pub attestation_store: Arc<dyn gyre_ports::AttestationRepository>,
    /// Authorization provenance attestation chains (TASK-006).
    pub chain_attestations: Arc<dyn gyre_ports::ChainAttestationRepository>,
    /// Ephemeral key bindings for authorization provenance (TASK-006).
    pub key_bindings: Arc<dyn gyre_ports::KeyBindingRepository>,
    /// Trust anchors for authorization provenance (TASK-006).
    pub trust_anchors: Arc<dyn gyre_ports::TrustAnchorRepository>,
    /// Trusted remote Gyre base URLs for cross-instance JWT federation (G11).
    /// Populated from `GYRE_TRUSTED_ISSUERS` (comma-separated list of base URLs).
    pub trusted_issuers: Vec<String>,
    /// Cached JWKS from trusted remote Gyre instances: issuer URL -> entry (G11).
    pub remote_jwks_cache: Arc<tokio::sync::RwLock<HashMap<String, auth::RemoteJwksEntry>>>,
    /// Commit signatures produced by jj squash (M13.8 Sigstore): commit_sha -> CommitSignature.
    pub commit_signatures: commit_signatures::CommitSignatureStore,
    /// Sigstore signing mode (local Ed25519 or Fulcio CA).  Set via `GYRE_SIGSTORE_MODE`.
    pub sigstore_mode: commit_signatures::SigstoreMode,
    /// Active SSH tunnels: tunnel_id -> TunnelRecord (G12).
    pub tunnel_store: Arc<Mutex<HashMap<String, api::compute::TunnelRecord>>>,
    /// Container audit records (persisted).
    pub container_audits: Arc<dyn gyre_ports::ContainerAuditRepository>,
    /// Spec registry ledger (persisted).
    pub spec_ledger: Arc<dyn gyre_ports::SpecLedgerRepository>,
    /// Spec approval event history (persisted).
    pub spec_approval_history: Arc<dyn gyre_ports::SpecApprovalEventRepository>,
    /// Spec links graph: all inter-spec links from manifests (M22.3).
    pub spec_links_store: spec_registry::SpecLinksStore,
    /// Budget limits per entity: entity_key -> BudgetConfig (M22.2).
    pub budget_configs: Arc<dyn BudgetRepository>,
    /// Real-time budget usage per entity: entity_key -> BudgetUsage (M22.2).
    pub budget_usages: Arc<dyn BudgetUsageRepository>,
    /// Full-text search index (M22.7).
    pub search: Arc<dyn gyre_ports::SearchPort>,
    /// Tenant repository (M34).
    pub tenants: Arc<dyn gyre_ports::TenantRepository>,
    /// Workspace repository (M22.1).
    pub workspaces: Arc<dyn WorkspaceRepository>,
    /// Persona repository (M22.1).
    pub personas: Arc<dyn PersonaRepository>,
    /// Full declarative ABAC policy engine (M22.6).
    pub policies: Arc<dyn PolicyRepository>,
    /// Workspace membership repository (M22.8).
    pub workspace_memberships: Arc<dyn WorkspaceMembershipRepository>,
    /// Team repository (M22.8).
    pub teams: Arc<dyn TeamRepository>,
    /// Notification repository (M22.8).
    pub notifications: Arc<dyn NotificationRepository>,
    /// WireGuard coordination plane configuration (M26.1).
    pub wg_config: WireGuardConfig,
    /// Knowledge graph store — nodes, edges, and architectural deltas (realized-model).
    pub graph_store: Arc<dyn GraphPort>,
    /// Saved explorer views (per-repo).
    pub saved_views: Arc<dyn gyre_ports::SavedViewRepository>,
    /// DB-backed meta-spec registry (agent-runtime spec §2).
    pub meta_specs: Arc<dyn MetaSpecRepository>,
    /// Meta-spec binding repository.
    pub meta_spec_bindings: Arc<dyn MetaSpecBindingRepository>,
    /// Workspace meta-spec sets persisted to DB (M34 Slice 5).
    pub meta_spec_sets: Arc<dyn MetaSpecSetRepository>,
    /// Message bus persistence (Directed + Event tier).
    pub messages: Arc<dyn gyre_ports::MessageRepository>,
    /// Bounded mpsc sender for background message consumer dispatch.
    pub message_dispatch_tx: tokio::sync::mpsc::Sender<gyre_common::message::Message>,
    /// Max unacked Directed messages per agent before 429. Configurable via GYRE_AGENT_INBOX_MAX.
    pub agent_inbox_max: u64,
    /// Per-user, per-workspace last-seen tracking (HSI §1).
    pub user_workspace_state: Arc<dyn UserWorkspaceStateRepository>,
    /// Debounce cache for last-seen middleware: (user_id, workspace_id) -> last upsert Instant.
    pub last_seen_debounce:
        Arc<std::sync::Mutex<std::collections::HashMap<(String, String), std::time::Instant>>>,
    /// Per-(user_id, workspace_id) sliding-window rate limiter for LLM endpoints.
    /// Enforces 10 requests/60 s per user per workspace (ui-layout.md §2).
    pub llm_rate_limiter: Arc<tokio::sync::Mutex<llm_rate_limit::LlmRateLimiterMap>>,
    /// Per-function, per-workspace LLM model configuration (LLM integration §4).
    pub llm_configs: Arc<dyn LlmConfigRepository>,
    /// In-memory presence map: (user_id, session_id) → PresenceEntry.
    /// Not persisted — evicted after 60 s of inactivity or on graceful disconnect.
    pub presence: Arc<RwLock<HashMap<(String, String), PresenceEntry>>>,
    /// Active WebSocket connections for targeted message delivery (e.g. PresenceEvicted).
    /// connection_id → mpsc::Sender<serialized WsMessage JSON>.
    pub ws_connections: Arc<RwLock<HashMap<u64, tokio::sync::mpsc::Sender<String>>>>,
    /// Monotonically increasing counter for assigning unique connection IDs.
    pub ws_connection_counter: Arc<std::sync::atomic::AtomicU64>,
    /// Workspace subscriptions per connection — used to scope UserPresence broadcasts.
    /// connection_id → list of subscribed workspace IDs.
    pub ws_connection_workspaces: Arc<RwLock<HashMap<u64, Vec<gyre_common::Id>>>>,
    /// Gate-time OTel trace capture repository (HSI §3a).
    pub traces: Arc<dyn TraceRepository>,
    /// OTLP receiver server configuration (HSI §3a). From GYRE_OTLP_* env vars.
    pub otlp_config: otlp_receiver::OtlpServerConfig,
    /// Conversation provenance repository (HSI §5).
    pub conversations: Arc<dyn gyre_ports::ConversationRepository>,
    /// Root directory for bare git repositories. Defaults to `./repos`.
    /// Stored here so unit tests can override it without touching env vars.
    pub repos_root: String,
    /// Prompt template repository (DB-backed, per-workspace + tenant defaults).
    pub prompt_templates: Arc<dyn gyre_ports::PromptRepository>,
    /// Compute target repository — tenant-scoped infrastructure targets for agent spawning.
    pub compute_targets: Arc<dyn ComputeTargetRepository>,
    /// LLM inference factory. None = LLM disabled (GYRE_VERTEX_PROJECT not set).
    ///
    /// When None, all LLM endpoints return HTTP 503 with a structured error.
    /// Tests wire `MockLlmPortFactory` so this is always `Some(...)` in tests.
    pub llm: Option<Arc<dyn gyre_ports::LlmPortFactory>>,
    /// Per-user notification preferences (HSI §12).
    pub user_notification_prefs: Arc<dyn gyre_ports::UserNotificationPreferenceRepository>,
    /// Per-user API tokens (HSI §12). Hashed at rest; plaintext never stored.
    pub user_tokens: Arc<dyn gyre_ports::UserTokenRepository>,
    /// Aggregated judgment ledger for user activity history (HSI §12).
    pub judgment_ledger: Arc<dyn gyre_ports::JudgmentLedgerRepository>,
    /// WebSocket ticket store: short-lived, single-use tokens for WS auth.
    /// Replaces the insecure ?token= query parameter pattern.
    pub ws_tickets: auth::WsTicketStore,
}

/// Helper: sign a bus message and return (base64_signature, key_id).
fn sign_bus_message(key: &auth::AgentSigningKey, msg: &Message) -> (String, String) {
    use sha2::{Digest, Sha256};

    let (from_type, from_id) = match &msg.from {
        MessageOrigin::Server => ("server", "".to_string()),
        MessageOrigin::Agent(id) => ("agent", id.as_str().to_string()),
        MessageOrigin::User(id) => ("user", id.as_str().to_string()),
    };
    let (to_type, to_id) = match &msg.to {
        Destination::Agent(id) => ("agent", id.as_str().to_string()),
        Destination::Workspace(id) => ("workspace", id.as_str().to_string()),
        Destination::Broadcast => ("broadcast", "".to_string()),
    };
    let ws_id = msg
        .workspace_id
        .as_ref()
        .map(|id| id.as_str().to_string())
        .unwrap_or_default();
    let payload_json = msg
        .payload
        .as_ref()
        .map(|v| serde_json::to_string(v).unwrap_or_default())
        .unwrap_or_default();
    let mut hasher = Sha256::new();
    hasher.update(payload_json.as_bytes());
    let payload_hash = format!("{:x}", hasher.finalize());
    let sign_input = format!(
        "{}\0{}\0{}\0{}\0{}\0{}\0{}\0{}\0{}",
        msg.id.as_str(),
        from_type,
        from_id,
        ws_id,
        to_type,
        to_id,
        msg.kind.as_str(),
        payload_hash,
        msg.created_at,
    );
    let sig_bytes = key.sign_bytes(sign_input.as_bytes());
    let sig_b64 = B64.encode(&sig_bytes);
    (sig_b64, key.kid.clone())
}

impl AppState {
    /// Emit an Event-tier server-originated message: sign, persist, broadcast to WS clients,
    /// and dispatch to consumers. Best-effort: logs errors, never panics.
    pub async fn emit_event(
        &self,
        workspace_id: Option<Id>,
        to: Destination,
        kind: MessageKind,
        payload: Option<serde_json::Value>,
    ) {
        use std::time::{SystemTime, UNIX_EPOCH};
        let created_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let id = Id::new(uuid::Uuid::new_v4().to_string());

        let mut msg = Message {
            id,
            tenant_id: Id::new("default"),
            from: MessageOrigin::Server,
            workspace_id,
            to,
            kind,
            payload,
            created_at,
            signature: None,
            key_id: None,
            acknowledged: false,
        };

        let (sig, kid) = sign_bus_message(&self.agent_signing_key, &msg);
        msg.signature = Some(sig);
        msg.key_id = Some(kid);

        // Persist (not Broadcast — those are never stored).
        if !matches!(msg.to, Destination::Broadcast) {
            if let Err(e) = self.messages.store(&msg).await {
                tracing::warn!("emit_event: failed to persist message: {e}");
            }
        }
        // Broadcast to WebSocket clients.
        let _ = self.message_broadcast_tx.send(msg.clone());
        // Dispatch to consumers (notification system, etc.).
        let _ = self.message_dispatch_tx.try_send(msg);
    }

    /// Emit a Telemetry-tier message: push to TelemetryBuffer and broadcast to WS clients.
    /// Unsigned and not persisted.
    pub fn emit_telemetry(
        &self,
        workspace_id: Id,
        kind: MessageKind,
        payload: Option<serde_json::Value>,
    ) {
        use std::time::{SystemTime, UNIX_EPOCH};
        let created_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let id = Id::new(uuid::Uuid::new_v4().to_string());
        let msg = Message {
            id,
            tenant_id: Id::new("default"),
            from: MessageOrigin::Server,
            workspace_id: Some(workspace_id.clone()),
            to: Destination::Workspace(workspace_id),
            kind,
            payload,
            created_at,
            signature: None,
            key_id: None,
            acknowledged: false,
        };
        self.telemetry_buffer.push(msg.clone());
        let _ = self.message_broadcast_tx.send(msg);
    }
}

/// Global authentication middleware for all `/api/v1/` routes.
///
/// Rejects any request without a valid `Authorization: Bearer <token>` header
/// with `401 Unauthorized`. The `/api/v1/version` endpoint is public.
/// ABAC middleware runs after this and enforces finer-grained policy checks.
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
            let valid = if let Ok(pairs) = state.kv_store.kv_list("agent_tokens").await {
                pairs.iter().any(|(_, v)| auth::tokens_equal(v.as_str(), t))
            } else {
                false
            };
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

    // Ensure the ABAC resource resolver is initialized (idempotent).
    abac_middleware::init_resolver();

    // Body size limit: configurable via GYRE_MAX_BODY_SIZE (bytes), default 10MB.
    let max_body_bytes: usize = std::env::var("GYRE_MAX_BODY_SIZE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(10 * 1024 * 1024);

    // CORS: configurable via GYRE_CORS_ORIGINS (comma-separated), default localhost only.
    let cors = build_cors_layer();

    // Apply global API auth middleware and ABAC middleware to all /api/v1/ routes.
    // Layer ordering (axum): later .layer() calls run FIRST.
    // So require_auth runs before last_seen_middleware runs before abac runs before the handler.
    let api = api::api_router()
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            abac_middleware::abac_middleware,
        ))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            middleware::last_seen_middleware,
        ))
        .layer(axum::middleware::from_fn_with_state(
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
        // WebSocket ticket endpoint: issues short-lived, single-use tokens.
        // Prevents leaking real auth tokens in WebSocket URL query parameters.
        .route("/api/v1/ws-ticket", post(explorer_ws::issue_ws_ticket))
        // Explorer WebSocket (explorer-implementation.md).
        // WebSocket upgrades cannot go through body-reading ABAC middleware,
        // so auth + repo-scoped tenant checks are enforced inside the handler.
        .route(
            "/api/v1/repos/:repo_id/explorer",
            get(explorer_ws::explorer_ws),
        )
        // Git smart HTTP -- auth enforced per-handler via AuthenticatedAgent extractor.
        // M34 Slice 6: workspace-slug/repo-name URL format.
        .route(
            "/git/:workspace_slug/:repo_name/info/refs",
            get(git_http::git_info_refs),
        )
        .route(
            "/git/:workspace_slug/:repo_name/git-upload-pack",
            post(git_http::git_upload_pack),
        )
        .route(
            "/git/:workspace_slug/:repo_name/git-receive-pack",
            post(git_http::git_receive_pack),
        )
        // MCP (Model Context Protocol) endpoints
        .route("/mcp", post(mcp::mcp_handler))
        .route("/mcp/sse", get(mcp::mcp_sse_handler))
        // Conversation provenance (HSI §5) — per-handler auth, ABAC-exempt from middleware.
        .route(
            "/api/v1/conversations/:sha",
            get(api::conversations::get_conversation),
        )
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
    let (message_broadcast_tx, _) = broadcast::channel(256);
    let (audit_broadcast_tx, _) = broadcast::channel(1024);
    let telemetry_buffer = Arc::new(TelemetryBuffer::new(
        std::env::var("GYRE_TELEMETRY_BUFFER_SIZE")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(10_000),
        std::env::var("GYRE_TELEMETRY_MAX_WORKSPACES")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(100),
    ));
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
        telemetry_buffer,
        message_broadcast_tx,
        kv_store: store!(dyn KvJsonStore, mem::MemKvStore::default()),
        agent_signing_key: Arc::new(auth::AgentSigningKey::generate()),
        // M27.5: Default reduced from 3600 to 300 s (5 min) to limit JWT exposure window.
        agent_jwt_ttl_secs: std::env::var("GYRE_AGENT_JWT_TTL")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(300),
        users: store!(dyn UserRepository, mem::MemUserRepository::default()),
        api_keys: store!(dyn ApiKeyRepository, mem::MemApiKeyRepository::default()),
        jwt_config,
        http_client: reqwest::Client::new(),
        metrics,
        started_at_secs,
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
        network_peers: store!(
            dyn NetworkPeerRepository,
            mem::MemNetworkPeerRepository::default()
        ),
        dependencies: Arc::new(mem::MemDependencyRepository::default()),
        breaking_changes: Arc::new(mem::MemBreakingChangeRepository::default()),
        dependency_policies: Arc::new(mem::MemDependencyPolicyRepository::default()),
        rate_limiter: rate_limit::RateLimiter::new(rate_per_sec),
        process_registry: Arc::new(Mutex::new(HashMap::new())),
        agent_logs: Arc::new(Mutex::new(HashMap::new())),
        agent_log_tx: Arc::new(Mutex::new(HashMap::new())),
        quality_gates: store!(
            dyn QualityGateRepository,
            mem::MemQualityGateRepository::default()
        ),
        gate_results: store!(
            dyn GateResultRepository,
            mem::MemGateResultRepository::default()
        ),
        push_gate_registry: Arc::new(pre_accept::builtin_gates()),
        repo_push_gates: store!(
            dyn PushGateRepository,
            mem::MemPushGateRepository::default()
        ),
        speculative_results: Arc::new(Mutex::new(HashMap::new())),
        spawn_log: store!(
            dyn SpawnLogRepository,
            mem::MemSpawnLogRepository::default()
        ),
        db_storage,
        spec_approvals: store!(
            dyn SpecApprovalRepository,
            mem::MemSpecApprovalRepository::default()
        ),
        spec_policies: store!(
            dyn SpecPolicyRepository,
            mem::MemSpecPolicyRepository::default()
        ),
        attestation_store: store!(
            dyn AttestationRepository,
            mem::MemAttestationRepository::default()
        ),
        chain_attestations: store!(
            dyn gyre_ports::ChainAttestationRepository,
            mem::MemChainAttestationRepository::default()
        ),
        key_bindings: store!(
            dyn gyre_ports::KeyBindingRepository,
            mem::MemKeyBindingRepository::default()
        ),
        trust_anchors: store!(
            dyn gyre_ports::TrustAnchorRepository,
            mem::MemTrustAnchorRepository::default()
        ),
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
        tunnel_store: Arc::new(Mutex::new(HashMap::new())),
        container_audits: store!(
            dyn ContainerAuditRepository,
            mem::MemContainerAuditRepository::default()
        ),
        spec_ledger: store!(
            dyn SpecLedgerRepository,
            mem::MemSpecLedgerRepository::default()
        ),
        spec_approval_history: store!(
            dyn SpecApprovalEventRepository,
            mem::MemSpecApprovalEventRepository::default()
        ),
        spec_links_store: Arc::new(Mutex::new(Vec::new())),
        budget_configs: store!(
            dyn BudgetRepository,
            mem::MemBudgetConfigRepository::default()
        ),
        budget_usages: store!(
            dyn BudgetUsageRepository,
            mem::MemBudgetUsageRepository::default()
        ),
        search: Arc::new(gyre_adapters::MemSearchAdapter::new()),
        tenants: store!(
            dyn gyre_ports::TenantRepository,
            mem::MemTenantRepository::default()
        ),
        workspaces: store!(
            dyn WorkspaceRepository,
            mem::MemWorkspaceRepository::default()
        ),
        personas: store!(dyn PersonaRepository, mem::MemPersonaRepository::default()),
        policies: store!(dyn PolicyRepository, mem::MemPolicyRepository::default()),
        workspace_memberships: Arc::new(mem::MemWorkspaceMembershipRepository::default()),
        teams: Arc::new(mem::MemTeamRepository::default()),
        notifications: store!(
            dyn NotificationRepository,
            mem::MemNotificationRepository::default()
        ),
        wg_config: WireGuardConfig::from_env(),
        graph_store: if let Some(ref d) = sqlite_db {
            Arc::clone(d) as Arc<dyn GraphPort>
        } else {
            Arc::new(gyre_adapters::MemGraphStore::new()) as Arc<dyn GraphPort>
        },
        saved_views: if let Some(ref d) = sqlite_db {
            Arc::clone(d) as Arc<dyn gyre_ports::SavedViewRepository>
        } else {
            Arc::new(gyre_adapters::MemSavedViewRepository::default())
                as Arc<dyn gyre_ports::SavedViewRepository>
        },
        meta_specs: store!(
            dyn MetaSpecRepository,
            mem::MemMetaSpecRepository::default()
        ),
        meta_spec_bindings: store!(
            dyn MetaSpecBindingRepository,
            mem::MemMetaSpecBindingRepository::default()
        ),
        meta_spec_sets: store!(
            dyn MetaSpecSetRepository,
            mem::MemMetaSpecSetRepository::default()
        ),
        messages: Arc::new(mem::MemMessageRepository::default()),
        message_dispatch_tx: {
            let (tx, rx) = tokio::sync::mpsc::channel(256);
            // Drain the channel so the receiver is not dropped.
            // ReconciliationCompleted → MetaSpecDrift notification creation happens
            // synchronously in emit_reconciliation_completed() helper, not here.
            tokio::spawn(async move {
                let mut rx = rx;
                while let Some(_msg) = rx.recv().await {
                    // No-op drain. Consumers can be wired via spawn_message_consumer().
                }
            });
            tx
        },
        agent_inbox_max: std::env::var("GYRE_AGENT_INBOX_MAX")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(1000),
        user_workspace_state: store!(
            dyn UserWorkspaceStateRepository,
            mem::MemUserWorkspaceStateRepository::default()
        ),
        last_seen_debounce: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
        llm_rate_limiter: Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new())),
        llm_configs: store!(
            dyn LlmConfigRepository,
            mem::MemLlmConfigRepository::default()
        ),
        presence: Arc::new(RwLock::new(HashMap::new())),
        ws_connections: Arc::new(RwLock::new(HashMap::new())),
        ws_connection_counter: Arc::new(std::sync::atomic::AtomicU64::new(1)),
        ws_connection_workspaces: Arc::new(RwLock::new(HashMap::new())),
        traces: store!(dyn TraceRepository, mem::MemTraceRepository::default()),
        otlp_config: otlp_receiver::OtlpServerConfig::from_env(),
        conversations: store!(
            dyn ConversationRepository,
            mem::MemConversationRepository::default()
        ),
        repos_root: std::env::var("GYRE_REPOS_PATH").unwrap_or_else(|_| "./repos".to_string()),
        prompt_templates: store!(
            dyn gyre_ports::PromptRepository,
            mem::MemPromptRepository::default()
        ),
        compute_targets: store!(
            dyn ComputeTargetRepository,
            mem::MemComputeTargetRepository::default()
        ),
        user_notification_prefs: store!(
            dyn gyre_ports::UserNotificationPreferenceRepository,
            mem::MemUserNotificationPreferenceRepository::default()
        ),
        user_tokens: store!(
            dyn gyre_ports::UserTokenRepository,
            mem::MemUserTokenRepository::default()
        ),
        judgment_ledger: store!(
            dyn gyre_ports::JudgmentLedgerRepository,
            mem::MemJudgmentLedgerRepository
        ),
        ws_tickets: auth::WsTicketStore::new(),
        llm: match std::env::var("GYRE_VERTEX_PROJECT") {
            Ok(_) => match gyre_adapters::RigVertexAiFactory::from_env() {
                Ok(factory) => Some(Arc::new(factory) as Arc<dyn gyre_ports::LlmPortFactory>),
                Err(e) => {
                    tracing::error!(
                        "Failed to initialize Vertex AI LLM adapter: {e}. \
                             LLM features are DISABLED."
                    );
                    None
                }
            },
            Err(_) => {
                tracing::warn!(
                    "GYRE_VERTEX_PROJECT is not set — LLM features are DISABLED. \
                     Endpoints POST /graph/predict, /briefing/ask, /specs/assist, \
                     /explorer-views/generate will return HTTP 503. \
                     Set GYRE_VERTEX_PROJECT (and GOOGLE_APPLICATION_CREDENTIALS) to enable."
                );
                None
            }
        },
    })
}

/// Spawn a background task that evicts stale presence entries every 30 seconds.
///
/// An entry is stale if server_last_seen is more than 60 seconds old.
/// The evicted connection (if still open) receives a `PresenceEvicted` message.
pub fn spawn_presence_eviction(state: Arc<AppState>) {
    use gyre_common::WsMessage;
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;

            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;

            let stale: Vec<((String, String), u64)> = {
                let map = state.presence.read().await;
                map.iter()
                    .filter(|(_, entry)| entry.server_last_seen + 60_000 < now_ms)
                    .map(|(key, entry)| (key.clone(), entry.connection_id))
                    .collect()
            };

            for ((user_id, session_id), conn_id) in stale {
                state
                    .presence
                    .write()
                    .await
                    .remove(&(user_id, session_id.clone()));

                let evict_msg = WsMessage::PresenceEvicted {
                    session_id: session_id.clone(),
                };
                if let Ok(payload) = serde_json::to_string(&evict_msg) {
                    let conns = state.ws_connections.read().await;
                    if let Some(tx) = conns.get(&conn_id) {
                        let _ = tx.try_send(payload);
                    }
                }
            }
        }
    });
}

/// Emit a `ReconciliationCompleted` Event-tier message and create priority-6
/// `MetaSpecDrift` notifications for all Admin/Developer/Owner workspace members (HSI §4).
///
/// This is the "MessageConsumer path" for ReconciliationCompleted: the spec says p6 is
/// "async acceptable", so we create notifications inline alongside the event emission.
/// This avoids the complexity of a separate consumer while meeting the spec requirement.
pub async fn emit_reconciliation_completed(
    state: &Arc<AppState>,
    workspace_id: Id,
    payload: Option<serde_json::Value>,
) {
    use gyre_common::{Notification, NotificationType};
    use gyre_domain::WorkspaceRole;

    // Emit Event-tier message.
    state
        .emit_event(
            Some(workspace_id.clone()),
            Destination::Workspace(workspace_id.clone()),
            MessageKind::ReconciliationCompleted,
            payload,
        )
        .await;

    // Resolve tenant_id from workspace record (avoid hardcoding "default").
    let tenant_id = match state.workspaces.find_by_id(&workspace_id).await {
        Ok(Some(ws)) => ws.tenant_id.to_string(),
        Ok(None) => {
            tracing::warn!("emit_reconciliation_completed: workspace {workspace_id} not found; skipping notifications");
            return;
        }
        Err(e) => {
            tracing::warn!(
                "emit_reconciliation_completed: failed to resolve workspace tenant: {e}"
            );
            return;
        }
    };

    // Create priority-6 MetaSpecDrift notifications for all Admin/Developer/Owner members.
    let members = match state
        .workspace_memberships
        .list_by_workspace(&workspace_id)
        .await
    {
        Ok(m) => m,
        Err(e) => {
            tracing::warn!("emit_reconciliation_completed: failed to list members: {e}");
            return;
        }
    };

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    for member in &members {
        if !matches!(
            member.role,
            WorkspaceRole::Admin | WorkspaceRole::Developer | WorkspaceRole::Owner
        ) {
            continue;
        }
        let notif_id = Id::new(uuid::Uuid::new_v4().to_string());
        let notif = Notification::new(
            notif_id,
            workspace_id.clone(),
            member.user_id.clone(),
            NotificationType::MetaSpecDrift,
            "Meta-spec reconciliation completed — workspace specs may have drifted",
            &tenant_id,
            now,
        );
        if let Err(e) = state.notifications.create(&notif).await {
            tracing::warn!(
                "emit_reconciliation_completed: failed to create MetaSpecDrift notification for {}: {e}",
                member.user_id
            );
        }
    }
}

/// Delegate to keep backwards-compatibility. New code should use stale_agents::spawn_stale_agent_detector.
pub fn spawn_stale_agent_detector(state: Arc<AppState>) {
    stale_agents::spawn_stale_agent_detector(state);
}

/// Start the WireGuard stale peer detector background task (M26.4).
pub fn spawn_stale_peer_detector(state: Arc<AppState>) {
    stale_peers::spawn_stale_peer_detector(state);
}

/// Auto-register the default `gyre-agent-default` container compute target on startup (M25).
///
/// If Docker or Podman is available and no target named `gyre-agent-default` exists yet,
/// registers a container target pointing at `gyre-agent:latest` with bridge networking
/// (agents need server access for clone/heartbeat/complete).  This makes agent spawning
/// zero-config: operators can spawn agents without first creating a compute target.
pub async fn register_default_compute_target(state: &Arc<AppState>) {
    const DEFAULT_NAME: &str = "gyre-agent-default";
    const DEFAULT_IMAGE: &str = "gyre-agent:latest";

    // Only register if Docker or Podman is reachable (same check as ContainerTarget::detect).
    let docker_ok = tokio::process::Command::new("which")
        .arg("docker")
        .output()
        .await
        .map(|o| o.status.success())
        .unwrap_or(false);
    let podman_ok = tokio::process::Command::new("which")
        .arg("podman")
        .output()
        .await
        .map(|o| o.status.success())
        .unwrap_or(false);
    let runtime_available = docker_ok || podman_ok;
    if !runtime_available {
        tracing::debug!("no docker/podman on PATH; skipping default compute target registration");
        return;
    }

    // Idempotent: skip if any target with this name already exists.
    if let Ok(Some(_)) = state.kv_store.kv_get("compute_targets", DEFAULT_NAME).await {
        tracing::debug!("default compute target '{DEFAULT_NAME}' already registered");
        return;
    }

    let ct = api::compute::ComputeTargetConfig {
        id: DEFAULT_NAME.to_string(),
        name: DEFAULT_NAME.to_string(),
        target_type: "container".to_string(),
        config: serde_json::json!({
            "image": DEFAULT_IMAGE,
            "network": "bridge",
            "command": "/gyre/entrypoint.sh"
        }),
    };

    if let Ok(json) = serde_json::to_string(&ct) {
        let _ = state
            .kv_store
            .kv_set("compute_targets", DEFAULT_NAME, json)
            .await;
        tracing::info!(
            name = DEFAULT_NAME,
            image = DEFAULT_IMAGE,
            "registered default container compute target (M25)"
        );
    }
}

/// Spawn a background task that evicts stale LLM rate-limiter entries every 60 seconds.
///
/// Prevents unbounded map growth: entries for users who have been idle for a full window
/// are removed. Safe to call once on server startup.
pub fn spawn_llm_rate_limiter_cleanup(state: Arc<AppState>) {
    use llm_rate_limit::{evict_stale_entries, LLM_WINDOW_SECS};
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(LLM_WINDOW_SECS)).await;
            let mut limiter = state.llm_rate_limiter.lock().await;
            evict_stale_entries(&mut limiter, LLM_WINDOW_SECS);
        }
    });
}

/// Spawn a background task that resets budget daily counters at midnight UTC (M22.2).
pub fn spawn_budget_daily_reset(state: Arc<AppState>) {
    tokio::spawn(async move {
        loop {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let secs_until_midnight = 86400 - (now % 86400);
            tokio::time::sleep(tokio::time::Duration::from_secs(secs_until_midnight)).await;
            api::budget::reset_daily_counters(&state).await;
        }
    });
}

/// Seed built-in meta-specs on first startup.
///
/// Seeds a set of well-known Global meta-specs (approval_status=Approved) if
/// the meta_specs table is empty. This is idempotent — re-running on a DB that
/// already has rows is a no-op.
pub async fn seed_builtin_meta_specs(state: &Arc<AppState>) {
    use gyre_domain::meta_spec::{MetaSpec, MetaSpecApprovalStatus, MetaSpecKind, MetaSpecScope};
    use gyre_ports::MetaSpecFilter;
    use sha2::{Digest, Sha256};

    fn sha256_hex(s: &str) -> String {
        let mut h = Sha256::new();
        h.update(s.as_bytes());
        hex::encode(h.finalize())
    }

    let existing = state
        .meta_specs
        .list(&MetaSpecFilter::default())
        .await
        .unwrap_or_default();
    if !existing.is_empty() {
        return;
    }

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let seeds: &[(&str, MetaSpecKind, bool, &str)] = &[
        // (name, kind, required, prompt_stub)
        (
            "default-worker",
            MetaSpecKind::Persona,
            false,
            "You are a diligent software engineer agent.",
        ),
        (
            "workspace-orchestrator",
            MetaSpecKind::Persona,
            false,
            "You are a workspace-level orchestrator agent.",
        ),
        (
            "repo-orchestrator",
            MetaSpecKind::Persona,
            false,
            "You are a repo-level orchestrator agent.",
        ),
        (
            "spec-reviewer",
            MetaSpecKind::Persona,
            false,
            "You are a spec reviewer agent.",
        ),
        (
            "accountability",
            MetaSpecKind::Persona,
            false,
            "You are an accountability agent.",
        ),
        (
            "security",
            MetaSpecKind::Persona,
            false,
            "You are a security review agent.",
        ),
        (
            "reconciliation",
            MetaSpecKind::Persona,
            false,
            "You are a meta-spec reconciliation agent.",
        ),
        (
            "conventional-commits",
            MetaSpecKind::Principle,
            true,
            "All commits must follow Conventional Commits specification.",
        ),
        (
            "test-coverage",
            MetaSpecKind::Standard,
            false,
            "All new code must have adequate test coverage.",
        ),
    ];

    for (name, kind, required, prompt) in seeds {
        let ms = MetaSpec {
            id: Id::new(uuid::Uuid::new_v4().to_string()),
            kind: kind.clone(),
            name: name.to_string(),
            scope: MetaSpecScope::Global,
            scope_id: None,
            prompt: prompt.to_string(),
            version: 1,
            content_hash: sha256_hex(prompt),
            required: *required,
            approval_status: MetaSpecApprovalStatus::Approved,
            approved_by: Some("system".to_string()),
            approved_at: Some(now),
            created_by: "system".to_string(),
            created_at: now,
            updated_at: now,
        };
        if let Err(e) = state.meta_specs.create(&ms).await {
            tracing::warn!("failed to seed meta-spec '{}': {e}", name);
        }
    }
    tracing::info!("seeded {} built-in meta-specs", seeds.len());
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
