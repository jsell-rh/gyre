#![recursion_limit = "256"]

pub(crate) mod abac;
pub mod abac_middleware;
pub mod abandoned_branch;
pub mod agent_audit;
pub mod agent_sessions;
pub mod aibom_spec_registry;
pub mod api;
pub mod attestation;
pub mod audit_simulator;
pub(crate) mod auth;
pub mod commit_signatures;
pub(crate) mod constraint_check;
pub mod container_audit;
pub mod dep_staleness;
pub mod domain_events;
pub mod email;
pub mod gate_executor;
pub(crate) mod git_http;
pub mod git_refs;
pub mod graph_diff_core;
pub mod graph_extraction;
pub mod procfs_monitor;
pub mod sigstore;

pub(crate) mod health;
pub mod impersonation;
pub mod jobs;
pub mod llm_defaults;
pub mod llm_helpers;
pub mod llm_rate_limit;
pub mod log_compression;
pub(crate) mod mcp;
pub(crate) mod mem;
pub mod merge_attestation;
pub mod merge_policy;
pub mod merge_processor;
pub mod message_dispatcher;
pub mod metrics;
pub mod middleware;
pub mod mirror_sync;
pub(crate) mod oidc;
pub mod pr_hygiene;
pub mod pre_accept;
pub mod preview_env_gc;
pub mod prompt_assembly;
pub mod ralph_loop;
pub mod rate_limit;
pub(crate) mod rbac;
pub mod reconciliation;
pub mod retention;
pub mod search_indexer;
pub mod search_scope;
pub mod search_sync;
pub mod secret_rotation;
pub mod siem;
pub(crate) mod signing;
pub(crate) mod snapshot;
pub(crate) mod spa;
pub mod spec_lifecycle_patrol;
pub mod spec_link_staleness;
pub mod spec_patrol;
pub mod spec_registry;
pub mod speculative_merge;
// sqlite.rs (rusqlite) removed — use gyre_adapters::SqliteStorage (Diesel) instead.
pub(crate) mod explorer_ws;
pub mod notification_dispatcher;
pub mod notifications;
pub mod otlp_receiver;
pub mod policy_cache;
pub mod policy_engine;
pub mod signal_chain;
pub mod spiffe;
pub mod spiffe_federation;
pub mod stale_agents;
pub mod stale_peers;
pub mod telemetry;
pub mod trust_suggestion;
pub mod trusted_proxy;
pub(crate) mod tty;
pub mod version_compute;
pub mod workload_attestation;
pub mod workspace_ownership;
pub(crate) mod ws;

use axum::{routing::get, Router};
use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use gyre_common::message::{Destination, Message, MessageKind, MessageOrigin, TelemetryBuffer};
use gyre_common::Id;
use gyre_common::WsMessage;
use gyre_ports::{
    AgentCommitRepository, AgentRepository, AgentSessionRepository, AnalyticsRepository,
    ApiKeyRepository, AttestationRepository, AttestationStorage, AuditRepository,
    BreakingChangeRepository, BudgetCallRepository, BudgetRepository, BudgetUsageRepository,
    CascadeTestResultRepository, ComputeTargetRepository, ContainerAuditRepository,
    ConversationRepository, CostRepository, DependencyPolicyRepository, DependencyRepository,
    GateResultRepository, GitOpsPort, GraphPort, JjOpsPort, KvJsonStore, LlmConfigRepository,
    MergeQueueRepository, MergeRequestRepository, MetaSpecBindingRepository, MetaSpecRepository,
    MetaSpecSetRepository, NetworkPeerRepository, NotificationRepository, PersonaRepository,
    PolicyRepository, PreAcceptGate, ProcessHandle, PushGateRepository, QualityGateRepository,
    RekorClient, RepoAccessOverrideRepository, RepoRepository, ReviewRepository,
    RolloutPolicyRepository, SpawnLogRepository, SpecApprovalEventRepository,
    SpecApprovalRepository, SpecLedgerRepository, SpecLifecycleRepository, SpecPolicyRepository,
    TaskRepository, TeamRepository, TenantMetaSpecSetRepository, TraceRepository, UserRepository,
    UserWorkspaceStateRepository, WorkspaceMembershipRepository, WorkspaceRepository,
    WorktreeRepository,
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
    /// Optional entity the session is actively editing, e.g.
    /// `"spec:specs/system/payments.md"` (HSI §7). `None` when only navigating.
    pub editing_entity: Option<String>,
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
    /// SPIFFE workload-attestation service (identity-security.md Layer 1):
    /// trust-domain CA / external SPIRE Workload API, trust-bundle
    /// publication, and SVID issuance for agent sessions.
    pub spiffe: Arc<spiffe::SpiffeService>,
    /// SPIFFE federation (identity-security.md §Federation): trust-bundle
    /// exchange with peer Gyre instances — bundle endpoints keyed by trust
    /// domain (`GYRE_SPIFFE_FEDERATION_PEERS`), refreshed on interval.
    /// Independent of the OIDC-issuer federation (`trusted_issuers`).
    pub spiffe_federation: Arc<spiffe_federation::FederationBundles>,
    pub repos: Arc<dyn RepoRepository>,
    pub agents: Arc<dyn AgentRepository>,
    pub tasks: Arc<dyn TaskRepository>,
    pub merge_requests: Arc<dyn MergeRequestRepository>,
    pub reviews: Arc<dyn ReviewRepository>,
    pub merge_queue: Arc<dyn MergeQueueRepository>,
    pub git_ops: Arc<dyn GitOpsPort>,
    pub jj_ops: Arc<dyn JjOpsPort>,
    pub agent_commits: Arc<dyn AgentCommitRepository>,
    /// Agent sessions (one per context window) — source-control.md
    /// §Agent-to-Commit Tracking. Port-backed so sessions survive a
    /// server restart.
    pub agent_sessions: Arc<dyn AgentSessionRepository>,
    pub worktrees: Arc<dyn WorktreeRepository>,
    /// In-memory ring buffer for Telemetry-tier messages (replaces ActivityStore).
    pub telemetry_buffer: Arc<TelemetryBuffer>,
    /// Unified broadcast channel for WebSocket message delivery (replaces broadcast_tx + event_tx).
    pub message_broadcast_tx: broadcast::Sender<Message>,
    /// Generic key-value JSON store for server-internal HashMap stores (M29.5B).
    /// Persists: agent_tokens, agent_cards, compute_targets,
    /// agent_stacks, repo_stack_policies, workload_attestations, workspace_repos,
    /// abac_policies — all keyed by (namespace, key).
    pub kv_store: Arc<dyn KvJsonStore>,
    /// Ed25519 signing key for Gyre's built-in OIDC provider (M18).
    /// Used to mint and verify agent JWTs returned by POST /api/v1/agents/spawn.
    pub agent_signing_key: Arc<auth::AgentSigningKey>,
    /// Retained public material of every signing key ever active on this
    /// deployment (message-bus.md §Edge Cases "Signature key rotation"):
    /// `GET /.well-known/jwks.json` serves these entries — minus the
    /// active key's kid — so messages signed before a key rotation remain
    /// verifiable by key_id. Durable in DB-backed deployments; empty in
    /// pure in-memory mode, where the key itself is fresh per boot.
    pub signing_key_history: Vec<auth::HistoricalSigningKey>,
    /// Merge attestation bundle verification (agent-gates.md §Part 3
    /// Verification). Stateless — holds only the forge key handle.
    pub attestation_verifier: Arc<dyn gyre_ports::AttestationVerifier>,
    /// Agent JWT TTL in seconds. Configurable via GYRE_AGENT_JWT_TTL (default: 3600).
    pub agent_jwt_ttl_secs: u64,
    /// Lifetime of a meta-spec preview run before the GC sweep tears it down.
    /// Configurable via GYRE_META_SPEC_PREVIEW_TTL_HOURS (default: 24 h).
    pub preview_ttl_secs: u64,
    /// TTL of the short-lived agent JWTs minted for preview agents.
    /// Configurable via GYRE_PREVIEW_JWT_TTL_SECS (default: 1800 s).
    pub preview_jwt_ttl_secs: u64,
    /// Max concurrent preview agents. Configurable via
    /// GYRE_PREVIEW_BUDGET_MAX_CONCURRENT; `None` = the workspace's own
    /// budget governs preview runs (meta-spec-reconciliation §5).
    pub preview_budget_max_concurrent: Option<u64>,
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
    /// Explorer WebSocket session registry: per-user live-session tracking
    /// for the concurrent-session limit. Ephemeral by nature (connections
    /// cannot outlive the server instance), so it is intentionally not
    /// port-backed - see `explorer_ws::ExplorerSessionRegistry`.
    pub(crate) explorer_sessions: Arc<explorer_ws::ExplorerSessionRegistry>,
    /// Data retention policies.
    pub retention_store: RetentionStore,
    /// Background job registry.
    pub job_registry: Arc<JobRegistry>,
    /// Workspaces with a live budget-exhaustion grace timer running
    /// (agent-runtime.md §4 Enforcement Levels: 100% → 60s grace). Purely
    /// process-local synchronization — a restart drops the timer, and the
    /// next usage report re-enters enforcement because usage is still at
    /// or above the limit. Not durable state; deliberately not port-backed.
    pub budget_grace_in_progress: Arc<tokio::sync::Mutex<std::collections::HashSet<String>>>,
    /// Product analytics event store.
    pub analytics: Arc<dyn AnalyticsRepository>,
    /// Cost tracking store.
    pub costs: Arc<dyn CostRepository>,
    /// Audit event store.
    pub audit: Arc<dyn AuditRepository>,
    /// Merge attestation bundle storage (agent-gates.md §Part 3
    /// Storage): git note on the merge commit + `merge_attestation_bundle`
    /// audit event, queryable by MR id and commit SHA.
    pub attestation_storage: Arc<dyn AttestationStorage>,
    /// Rekor transparency log for merge attestation non-repudiation
    /// (no-op until a Rekor backend is configured).
    pub rekor: Arc<dyn RekorClient>,
    /// SIEM target store and forwarding state.
    pub siem_store: SiemStore,
    /// Broadcast channel for live audit event SSE stream.
    pub audit_broadcast_tx: broadcast::Sender<String>,
    /// Broadcast channel for live notification SSE stream
    /// (user-management.md §API > Notifications: `/notifications/stream`).
    /// Carries serialized `Notification` JSON; per-stream filtering by
    /// user/tenant/workspace happens in the SSE handler.
    pub notification_broadcast_tx: broadcast::Sender<String>,
    /// WireGuard network peer registry.
    pub network_peers: Arc<dyn NetworkPeerRepository>,
    /// Ephemeral preview environments (activity-dashboard.md §"Try it").
    /// Durable store: rows survive restarts; teardown/TTL keep the row as
    /// the audit record of what ran.
    pub preview_environments: Arc<dyn gyre_ports::PreviewEnvironmentRepository>,
    /// Cross-repo dependency graph (M22.4).
    pub dependencies: Arc<dyn DependencyRepository>,
    /// Breaking change records (M22.4 — TASK-020).
    pub breaking_changes: Arc<dyn BreakingChangeRepository>,
    /// Per-workspace dependency enforcement policies (M22.4 — TASK-020).
    pub dependency_policies: Arc<dyn DependencyPolicyRepository>,
    /// Durable cascade test results (dependency-graph.md §Cascade Testing,
    /// task-280): the persisted outcome of each cascade test run, surviving
    /// server restarts — the durable counterpart of the one-shot
    /// cascade_test_passed/failed workspace events.
    pub cascade_test_results: Arc<dyn CascadeTestResultRepository>,
    /// Request rate limiter (requests/sec).
    pub rate_limiter: Arc<rate_limit::RateLimiter>,
    /// Running agent processes: agent_id -> ProcessHandle.
    pub process_registry: Arc<Mutex<HashMap<String, ProcessHandle>>>,
    /// Compute backend each running agent was spawned on: agent_id ->
    /// backend owning its handle. Lets kill/is_alive reach the right
    /// orchestrator (docker/podman, remote SSH docker, or kubectl)
    /// instead of assuming a local pid (agent-runtime.md §3).
    ///
    /// Values are `Arc` so the exit monitor can clone the backend and drop
    /// the lock BEFORE probing liveness — an SSH probe can block for the
    /// TCP connect timeout, and holding this lock across it would stall
    /// admin kills and every new spawn.
    pub spawned_backends:
        Arc<Mutex<HashMap<String, std::sync::Arc<crate::api::spawn::SpawnBackend>>>>,
    /// Per-agent log buffers ("[ts] message" lines) with a zstd-compressed
    /// cold segment for lines older than the compression age
    /// (observability.md §Log Retention & Compression).
    pub agent_logs: Arc<log_compression::AgentLogBuffer>,
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
    /// Durable storage backend handle for infrastructure health probes
    /// (business-continuity.md §2). Covers SQLite and PostgreSQL; None in
    /// pure in-memory mode (no DB backend configured).
    pub storage: Option<Arc<dyn gyre_ports::storage::StoragePort>>,
    /// Spec approval ledger (persisted).
    pub spec_approvals: Arc<dyn gyre_ports::SpecApprovalRepository>,
    /// Per-repo spec enforcement policies (persisted).
    pub spec_policies: Arc<dyn gyre_ports::SpecPolicyRepository>,
    /// Per-repo spec lifecycle configuration (persisted).
    pub spec_lifecycle_configs: Arc<dyn gyre_ports::SpecLifecycleRepository>,
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
    /// Commit signatures produced by jj squash (M13.8, task-107): persisted
    /// via `CommitSignatureRepository`, keyed by `(repo_id, commit_sha)`.
    pub commit_signatures: Arc<dyn gyre_ports::commit_signature_repo::CommitSignatureRepository>,
    /// Signing configuration — mode plus the Fulcio/Rekor trust anchors
    /// (task-107 F3: verification resolves trust from here, never from the
    /// record being verified).
    pub signing_config: commit_signatures::SigningConfig,
    /// Trusted reverse-proxy networks for forensic source-IP resolution
    /// (task-102 F3 / CWE-348). Populated from `GYRE_TRUSTED_PROXIES`
    /// (comma-separated CIDRs / bare IPs). Empty (the default) = every
    /// connection is direct: the socket peer is always the source and
    /// forwarded headers are ignored.
    pub trusted_proxies: trusted_proxy::TrustedProxies,
    /// Active SSH tunnels: tunnel_id -> TunnelRecord (G12).
    pub tunnel_store: Arc<Mutex<HashMap<String, api::compute::TunnelRecord>>>,
    /// Container audit records (persisted).
    pub container_audits: Arc<dyn gyre_ports::ContainerAuditRepository>,
    /// Spec registry ledger (persisted).
    pub spec_ledger: Arc<dyn gyre_ports::SpecLedgerRepository>,
    /// Spec approval event history (persisted).
    pub spec_approval_history: Arc<dyn gyre_ports::SpecApprovalEventRepository>,
    /// Spec link graph repository (spec-links.md §Forge-Maintained Spec Graph).
    /// Durable source of truth; `spec_links_store` is the hot in-memory cache
    /// loaded from this at boot and written through on every mutation.
    pub spec_link_repo: Arc<dyn gyre_ports::SpecLinkRepository>,
    /// Persisted spec assertion check results (system-explorer.md §9).
    pub spec_assertion_results: Arc<dyn gyre_ports::SpecAssertionResultRepository>,
    /// In-memory hot cache of the spec-link graph, rebuilt from the repository
    /// at boot (see `load_spec_links_into_store`) and written through on every
    /// mutation. The SQL table is authoritative; this is the reader-facing view.
    pub spec_links_store: spec_registry::SpecLinksStore,
    /// Budget limits per entity: entity_key -> BudgetConfig (M22.2).
    pub budget_configs: Arc<dyn BudgetRepository>,
    /// Real-time budget usage per entity: entity_key -> BudgetUsage (M22.2).
    pub budget_usages: Arc<dyn BudgetUsageRepository>,
    /// Per-call LLM budget audit records (platform-model.md §Budget Tracking).
    pub budget_calls: Arc<dyn BudgetCallRepository>,
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
    /// Shared policy-set cache handle (§Performance): `policies` is wrapped
    /// in a read-through [`policy_cache::CachedPolicyRepository`]; this handle
    /// is the invalidation entry point for writers that persist policies
    /// WITHOUT the `PolicyRepository` port (`apply_trust_transition`).
    pub policy_cache: Arc<policy_cache::PolicyCache>,
    /// Workspace membership repository (M22.8).
    pub workspace_memberships: Arc<dyn WorkspaceMembershipRepository>,
    /// Per-repo access override repository (user-management.md §Repo Access).
    pub repo_access_overrides: Arc<dyn RepoAccessOverrideRepository>,
    /// Team repository (M22.8).
    pub teams: Arc<dyn TeamRepository>,
    /// Notification repository (M22.8).
    pub notifications: Arc<dyn NotificationRepository>,
    /// Tenant invitation repository (task-110, user-management.md).
    pub tenant_invitations: Arc<dyn gyre_ports::TenantInvitationRepository>,
    /// Workspace invitation repository (task-110, user-management.md).
    pub workspace_invitations: Arc<dyn gyre_ports::WorkspaceInvitationRepository>,
    /// SCIM Group resources and their membership sets (task-266,
    /// identity-security.md §SCIM Provisioning). Durable: SQLite/PG
    /// adapters over migration 000068.
    pub scim_groups: Arc<dyn gyre_ports::ScimGroupRepository>,
    /// Per-workspace IdP-group → workspace-role mappings (task-266,
    /// identity-security.md §Group-to-Workspace Mapping). Durable.
    pub scim_mappings: Arc<dyn gyre_ports::ScimMappingRepository>,
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
    /// Tenant-level meta-spec sets (§8): tenant-wide default bindings that
    /// workspaces inherit unless their own set overrides the pinned path.
    pub tenant_meta_spec_sets: Arc<dyn TenantMetaSpecSetRepository>,
    /// Per-workspace rollout policies (meta-spec-reconciliation.md §7).
    pub rollout_policies: Arc<dyn RolloutPolicyRepository>,
    /// Message bus persistence (Directed + Event tier).
    pub messages: Arc<dyn gyre_ports::MessageRepository>,
    /// Bounded mpsc sender for background message consumer dispatch.
    pub message_dispatch_tx: tokio::sync::mpsc::Sender<gyre_common::message::Message>,
    /// Receiver half of the dispatch channel. `None` once the consumer task
    /// took ownership — exactly one dispatcher may drain the bus.
    message_dispatch_rx:
        Arc<tokio::sync::Mutex<Option<tokio::sync::mpsc::Receiver<gyre_common::message::Message>>>>,
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
    /// Verified user identity per connection — used for user-targeted
    /// pushes (identity-security.md §Impersonation: notify the impersonated
    /// user when online). connection_id → verified user_id. Connections
    /// authenticated without a user identity (shared token, agent tokens)
    /// are absent. Ephemeral connection state, not a durable store.
    pub ws_connection_users: Arc<RwLock<HashMap<u64, String>>>,
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
    /// Per-user delivery-channel preferences (user-management.md §Delivery Channels).
    pub user_channel_prefs: Arc<dyn gyre_ports::UserChannelPreferenceRepository>,
    /// Per-user API tokens (HSI §12). Hashed at rest; plaintext never stored.
    pub user_tokens: Arc<dyn gyre_ports::UserTokenRepository>,
    /// Authenticated user sessions (user-management.md §Session Management).
    /// Created on successful API-key auth, one per (credential, device);
    /// revocable individually or in bulk ("sign out everywhere").
    pub sessions: Arc<dyn gyre_ports::SessionRepository>,
    /// Secret repository (platform-model.md §7): scoped credential storage,
    /// encrypted at rest in the adapter. Agent spawn resolves secrets through
    /// this and injects them as GYRE_CRED_* env vars for the cred-proxy sidecar.
    pub secrets: Arc<dyn gyre_ports::SecretRepository>,
    /// Aggregated judgment ledger for user activity history (HSI §12).
    pub judgment_ledger: Arc<dyn gyre_ports::JudgmentLedgerRepository>,
    /// Orchestrator registry (agent-runtime.md §1 Phases 2–3): per-workspace
    /// and per-repo processing locks implementing exactly-one-active
    /// semantics for the spec-approval signal chain. Process-local
    /// serialization only — agent liveness lives in the port-backed agents
    /// store.
    pub orchestrator_registry: signal_chain::OrchestratorRegistry,
    /// WebSocket ticket store: short-lived, single-use tokens for WS auth.
    /// Replaces the insecure ?token= query parameter pattern.
    pub ws_tickets: auth::WsTicketStore,
}

/// Compute the SHA-256 hash of a workspace's EFFECTIVE meta-spec set
/// (meta-spec-reconciliation.md §8: tenant defaults merged with the
/// workspace's own overrides — workspace pin wins on conflict).
///
/// Returns the hex-encoded hash of the canonical serialization of the merged
/// set, or an empty string when the workspace binds nothing (no workspace set
/// and no tenant set) or the workspace does not exist. Used at spec approval
/// time (to populate `InputContent.meta_spec_set_sha`) and during constraint
/// evaluation (to populate `AgentContext.meta_spec_set_sha`).
///
/// For a workspace without a tenant set the merged serialization is
/// byte-identical to the stored workspace-set JSON, so SHAs recorded before
/// tenant sets existed remain valid (no false drift on upgrade).
pub(crate) async fn compute_meta_spec_set_sha(state: &AppState, workspace_id: &Id) -> String {
    use sha2::{Digest, Sha256};
    let ws = match state.workspaces.find_by_id(workspace_id).await {
        Ok(Some(ws)) => ws,
        _ => return String::new(),
    };
    let ws_set = state.meta_spec_sets.get(workspace_id).await.ok().flatten();
    let tenant_set = state
        .tenant_meta_spec_sets
        .get(&ws.tenant_id)
        .await
        .ok()
        .flatten();
    if ws_set.is_none() && tenant_set.is_none() {
        // Nothing is bound — no set to conform to.
        return String::new();
    }
    let effective = crate::reconciliation::resolve_effective_meta_spec_set(state, &ws)
        .await
        .map_err(|e| {
            tracing::warn!(
                workspace_id = %workspace_id,
                error = %e,
                "compute_meta_spec_set_sha: corrupt set JSON; treating as unbound"
            );
            e
        })
        .unwrap_or_default();
    let json = serde_json::to_string(&effective).unwrap_or_default();
    let mut hasher = Sha256::new();
    hasher.update(json.as_bytes());
    hex::encode(hasher.finalize())
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
    /// Persist a notification and fan it out to both realtime channels
    /// (user-management.md §API > Notifications): the `/notifications/stream`
    /// SSE subscribers and the live `/ws` connections authenticated as the
    /// notification's owner (the in-app bell).
    ///
    /// Single fan-out point for notification creation: every production
    /// emitter goes through here (or through the
    /// [`BroadcastingNotificationRepo`](crate::notifications::BroadcastingNotificationRepo)
    /// this delegates to, which background paths without an `&AppState` use)
    /// so a connected client sees every notification the moment it is
    /// stored. Fan-out is fire-and-forget (no subscribers, a lagged one, or
    /// an offline owner must never fail the persist).
    pub async fn create_notification(
        &self,
        notification: &gyre_common::Notification,
    ) -> anyhow::Result<()> {
        self.broadcasting_notifications().create(notification).await
    }

    /// Resolve the tenant scope for a server-originated message about a
    /// workspace: the workspace record's tenant. Skips the emission and
    /// logs when the workspace cannot be resolved (same contract as
    /// `emit_reconciliation_completed`): a message carrying a fabricated
    /// tenant re-targets delivery to whatever scope the id happens to
    /// collide with.
    async fn message_tenant_id(&self, workspace_id: &Id) -> Option<Id> {
        match self.workspaces.find_by_id(workspace_id).await {
            Ok(Some(ws)) => Some(ws.tenant_id),
            Ok(None) => {
                tracing::warn!(
                    workspace_id = %workspace_id,
                    "emit: workspace not found; skipping message (cannot resolve tenant scope)"
                );
                None
            }
            Err(e) => {
                tracing::warn!(
                    workspace_id = %workspace_id,
                    error = %e,
                    "emit: failed to resolve workspace tenant; skipping message"
                );
                None
            }
        }
    }

    /// Emit an Event-tier server-originated message: sign, persist, broadcast to WS clients,
    /// and dispatch to consumers. Best-effort: logs errors, never panics.
    ///
    /// The message's tenant is resolved from the workspace record — a
    /// server-originated message never rides a fabricated scope identity. If
    /// the owning tenant cannot be resolved, the message is dropped with a
    /// warning (skip-and-log, per the task-099 F1 remediation direction).
    pub async fn emit_event(
        &self,
        workspace_id: Id,
        to: Destination,
        kind: MessageKind,
        payload: Option<serde_json::Value>,
    ) {
        // F1: tenant scope resolved from the workspace record, never a
        // fabricated literal (skip-and-log on unresolvable).
        let Some(tenant_id) = self.message_tenant_id(&workspace_id).await else {
            return;
        };
        if let Err(e) = self
            .emit_scoped_event(tenant_id, Some(workspace_id), to, kind, payload)
            .await
        {
            tracing::warn!("emit_event: dropped message: {e}");
        }
    }

    /// Emit an Event-tier server-originated message scoped to a tenant whose
    /// Id the caller already holds (tenant-level domain events such as the
    /// tenant meta-spec set update). The scope is known by construction —
    /// derived from the authenticated caller's tenant in the handler — so it
    /// is carried directly instead of being derived from a workspace record;
    /// it is never fabricated. The message has no single workspace audience
    /// (`workspace_id: None`): the audience (the tenant's workspaces) is
    /// enumerated by the consumers of the event, not by this emitter.
    ///
    /// The destination MUST be Broadcast: a non-Broadcast message without a
    /// workspace scope is rejected by `emit_scoped_event` (the send-path
    /// scopeless-message guard) — tenant-level events fan out to the whole
    /// tenant, they are never stored or delivered scoped.
    pub async fn emit_tenant_event(
        &self,
        tenant_id: Id,
        to: Destination,
        kind: MessageKind,
        payload: Option<serde_json::Value>,
    ) {
        if let Err(e) = self
            .emit_scoped_event(tenant_id, None, to, kind, payload)
            .await
        {
            tracing::warn!("emit_tenant_event: dropped message: {e}");
        }
    }

    /// Shared body of the Event-tier emitters: build, sign, persist
    /// (non-Broadcast), broadcast to WS clients, and dispatch to consumers.
    /// Best-effort: logs errors, never panics.
    ///
    /// Scopeless-message guard (message-bus.md §Message Envelope — the
    /// send_message() helper validation): a message whose destination is not
    /// Broadcast MUST carry a workspace scope. Such a message is never
    /// constructed, stored, or broadcast — the emitter returns Err and logs
    /// at error level.
    async fn emit_scoped_event(
        &self,
        tenant_id: Id,
        workspace_id: Option<Id>,
        to: Destination,
        kind: MessageKind,
        payload: Option<serde_json::Value>,
    ) -> anyhow::Result<()> {
        use std::time::{SystemTime, UNIX_EPOCH};
        if !matches!(to, Destination::Broadcast) && workspace_id.is_none() {
            tracing::error!(
                destination = ?to,
                kind = %kind,
                "emit: refusing to construct non-Broadcast message without workspace scope"
            );
            return Err(anyhow::anyhow!(
                "non-Broadcast message requires a workspace scope (workspace_id is None)"
            ));
        }
        let created_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let id = Id::new(uuid::Uuid::new_v4().to_string());

        let mut msg = Message {
            id,
            tenant_id,
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

        // Persist (not Broadcast — those are never stored). Broadcast-only
        // kinds (DataSeeded/QueueUpdated) are storage-exempt regardless of
        // destination (message-bus.md §Broadcast-only kinds): they flow
        // through the in-memory broadcast channel only.
        if !matches!(msg.to, Destination::Broadcast) && !msg.kind.broadcast_only() {
            if let Err(e) = self.messages.store(&msg).await {
                tracing::warn!("emit_event: failed to persist message: {e}");
            }
        }
        // Broadcast to WebSocket clients.
        let _ = self.message_broadcast_tx.send(msg.clone());
        // Dispatch to consumers (notification system, etc.). Consumer drops under
        // backpressure are logged, never block the send path (message-bus.md
        // §Relationship to Notifications).
        match self.message_dispatch_tx.try_send(msg) {
            Ok(()) => {}
            Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {
                tracing::warn!(
                    "message dispatch channel full; consumer dropped a message (best-effort tier)"
                );
            }
            Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
                tracing::debug!("message dispatch channel closed; no consumers registered");
            }
        }
        Ok(())
    }

    /// Take ownership of the message-dispatch receiver. Returns `None` if the
    /// dispatcher already took it — only one consumer task may drain the bus.
    pub async fn take_message_dispatch_rx(
        &self,
    ) -> Option<tokio::sync::mpsc::Receiver<gyre_common::message::Message>> {
        self.message_dispatch_rx.lock().await.take()
    }

    /// Emit a Telemetry-tier message: push to TelemetryBuffer and broadcast to WS clients.
    /// Unsigned and not persisted.
    pub async fn emit_telemetry(
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

        // F1: tenant scope resolved from the workspace record.
        let Some(tenant_id) = self.message_tenant_id(&workspace_id).await else {
            return;
        };

        let msg = Message {
            id,
            tenant_id,
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

    // SCIM 2.0 provisioning endpoints (identity-security.md §SCIM
    // Provisioning) authenticate with the dedicated `GYRE_SCIM_TOKEN`
    // bearer token — a server-to-server credential separate from the agent
    // JWT / global-token ladder below. Passing a SCIM request through that
    // ladder rejected it 401 ("Invalid token") before the handler-level
    // `check_scim_auth` ever ran, so the specced authentication model did
    // not work end-to-end in the shipped server. The exemption is safe
    // because every /scim/v2 handler enforces `check_scim_auth` itself:
    // fail-closed 403 when GYRE_SCIM_TOKEN is unset, constant-time bearer
    // compare otherwise. ABAC middleware passes these routes through as
    // resolver-exempt (RouteResourceMapping::exempt entries), so no policy
    // evaluation is skipped by this bypass.
    if req.uri().path().starts_with("/scim/v2/") {
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
            // 1.5. Impersonation JWT (identity-security.md §Impersonation) —
            //     the same validation as the auth extractor
            //     (`FromRequestParts` step 1.5): EdDSA signature + active
            //     session. Without this branch the token is rejected here
            //     and impersonated calls never reach the API at all.
            //     Attribution happens downstream in `abac_middleware`.
            if t.starts_with("ey") {
                if let Some(claims) = state
                    .agent_signing_key
                    .validate_impersonation_opt(t, &state.base_url)
                {
                    return match crate::impersonation::resolve_impersonation_auth(&claims, &state)
                        .await
                    {
                        Some(_) => next.run(req).await,
                        None => (
                            axum::http::StatusCode::UNAUTHORIZED,
                            "Impersonation session is not active",
                        )
                            .into_response(),
                    };
                }
            }

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
            // task-104: AuthFailure audit event (observability.md §Audit
            // Event Types) — the global gate rejected the presented token;
            // the extractors never run for this request. Best-effort token
            // classification: "ey" prefix = JWT-shaped credential.
            let token_kind = if t.starts_with("ey") {
                "agent_jwt"
            } else {
                "unknown"
            };
            let source_ip = req
                .extensions()
                .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
                .map(|c| c.0.ip().to_string());
            let user_agent = req
                .headers()
                .get(axum::http::header::USER_AGENT)
                .and_then(|v| v.to_str().ok())
                .map(str::to_string);
            auth::record_auth_failure_for_middleware(
                &state,
                token_kind,
                "invalid token",
                source_ip,
                user_agent,
            )
            .await;
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
        // SPIFFE trust bundle (identity-security.md Layer 1): the trust
        // domain's X.509-SVID anchors in SPIFFE bundle format, at the
        // standard well-known location. Public like the JWKS endpoint —
        // any verifier must be able to check SVIDs.
        .route(
            spiffe::TRUST_BUNDLE_PATH,
            get(spiffe::trust_bundle_endpoint),
        )
        .route("/ws", get(ws::ws_handler))
        .route("/ws/agents/:id/tty", get(tty::tty_handler))
        // WebSocket ticket endpoint: issues short-lived, single-use tokens.
        // Prevents leaking real auth tokens in WebSocket URL query parameters.
        .route("/api/v1/ws-ticket", post(explorer_ws::issue_ws_ticket))
        // Invitation magic-link acceptance (task-110, user-management.md
        // §Tenant-Level User Onboarding): the 256-bit CSPRNG token in the
        // URL is the auth factor — the invitee has no credentials yet, so
        // these cannot run behind require_auth_middleware. The handler
        // validates the token hash, single-use status, and expiry.
        .route(
            "/api/v1/invite/:token/accept",
            post(api::invitations::accept_tenant_invitation),
        )
        .route(
            "/api/v1/invite/:token/decline",
            post(api::invitations::decline_tenant_invitation),
        )
        // Magic-link preview for the /invite/{token} accept page: same
        // token-as-auth-factor trust model, strictly read-only (no status
        // transition, no claim, no credentials) — see the handler.
        .route(
            "/api/v1/invite/:token/preview",
            get(api::invitations::preview_invitation),
        )
        .route(
            "/api/v1/workspaces/invitations/:token/accept",
            post(api::invitations::accept_workspace_invitation),
        )
        .route(
            "/api/v1/workspaces/invitations/:token/decline",
            post(api::invitations::decline_workspace_invitation),
        )
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

/// Build application state. Storage backend selection
/// (specs/development/architecture.md §Storage — "SQLite as the default
/// (simple, vertical scaling, zero ops)"):
///
/// - `GYRE_DATABASE_URL=memory` — explicit stateless opt-in: every port
///   falls back to an in-memory implementation and nothing persists
///   across restarts (the pre-default-SQLite behavior, now opt-in only);
/// - `postgres://…` / `postgresql://…` — PostgreSQL via Diesel;
/// - `sqlite://<path>` (or a bare path) — SQLite at that path;
/// - unset (the shipped default) — durable SQLite at the platform
///   default data path ([`default_sqlite_db_path`]), so a
///   zero-configuration `cargo run -p gyre-server` persists all entities
///   across restarts.
///
/// Used by both production (main) and integration tests.
/// Resolve the repositories root once against this process's working directory
/// so persisted repo paths and child processes see the same absolute path.
pub fn configured_repos_path() -> std::path::PathBuf {
    let cwd = std::env::current_dir().expect("Gyre requires a valid working directory");
    let configured = std::env::var_os("GYRE_REPOS_PATH")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| cwd.join("repos"));
    if configured.is_absolute() {
        configured
    } else {
        cwd.join(configured)
    }
}

/// KV namespace/key under which the forge Ed25519 signing key is persisted
/// so that local-mode commit-signature records — which survive restarts in
/// the `CommitSignatureRepository` — remain verifiable across boots
/// (task-107 F12). A fresh key per boot would make every pre-restart
/// local-mode record report `valid:false` after restart.
pub const AGENT_SIGNING_KEY_KV_NAMESPACE: &str = "server_identity";
pub const AGENT_SIGNING_KEY_KV_KEY: &str = "agent_signing_key_pkcs8_b64";

/// KV key (same `server_identity` namespace) holding the retained public
/// material of every signing key that has ever been active on this
/// deployment (message-bus.md §Edge Cases "Signature key rotation"):
/// a JSON array of `{kid, public_key_b64}` entries. JWKS serves every
/// entry whose kid is not the active key's, so messages signed before a
/// rotation remain verifiable by key_id. Only public material is stored
/// here — the private key slot (`AGENT_SIGNING_KEY_KV_KEY`) is untouched
/// by rotation bookkeeping.
pub const AGENT_SIGNING_KEY_HISTORY_KV_KEY: &str = "agent_signing_key_history";

/// Load the retained signing-key history from durable storage. An
/// unreadable or malformed entry is skipped with a warning (that kid stops
/// resolving in JWKS) rather than aborting the boot; a missing entry is a
/// first rotation-free boot and yields an empty history.
fn load_signing_key_history(
    sqlite_db: &Option<Arc<gyre_adapters::SqliteStorage>>,
    pg_db: &Option<Arc<gyre_adapters::PgStorage>>,
) -> Vec<auth::HistoricalSigningKey> {
    let raw: anyhow::Result<Option<String>> = if let Some(pg) = pg_db {
        pg.kv_get_sync(
            AGENT_SIGNING_KEY_KV_NAMESPACE,
            AGENT_SIGNING_KEY_HISTORY_KV_KEY,
        )
    } else if let Some(sqlite) = sqlite_db {
        sqlite.kv_get_sync(
            AGENT_SIGNING_KEY_KV_NAMESPACE,
            AGENT_SIGNING_KEY_HISTORY_KV_KEY,
        )
    } else {
        return Vec::new();
    };
    let raw = match raw {
        Ok(Some(v)) => v,
        Ok(None) => return Vec::new(),
        Err(e) => {
            tracing::warn!(
                error = %e,
                "could not read the signing-key history; JWKS will serve only the active key this boot"
            );
            return Vec::new();
        }
    };

    #[derive(serde::Deserialize)]
    struct HistoryEntry {
        kid: String,
        public_key_b64: String,
    }

    let entries: Vec<HistoryEntry> = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(
                error = %e,
                "signing-key history is malformed; JWKS will serve only the active key this boot"
            );
            return Vec::new();
        }
    };

    let mut history = Vec::with_capacity(entries.len());
    for entry in entries {
        let bytes = match B64
            .decode(entry.public_key_b64.trim())
            .map_err(|e| format!("base64: {e}"))
            .and_then(|b| {
                if b.len() == 32 {
                    Ok(b)
                } else {
                    Err(format!(
                        "expected 32 Ed25519 public-key bytes, got {}",
                        b.len()
                    ))
                }
            }) {
            Ok(b) => b,
            Err(e) => {
                tracing::warn!(
                    kid = %entry.kid,
                    error = %e,
                    "skipping unreadable signing-key history entry"
                );
                continue;
            }
        };
        let key = auth::HistoricalSigningKey::from_public_key_bytes(bytes);
        if key.kid != entry.kid {
            tracing::warn!(
                stored_kid = %entry.kid,
                derived_kid = %key.kid,
                "skipping signing-key history entry whose kid does not match its public key"
            );
            continue;
        }
        history.push(key);
    }
    history
}

/// Ensure the active key's public material is present in the retained
/// history and persist the updated history. Every key that has ever been
/// active ends up exactly once in this set; at serve time JWKS filters out
/// the active kid, so what remains is precisely the historical keys.
/// Rotation chains (K1 → K2 → K3) accumulate: boot K2 appends K2 to a set
/// that already holds K1, boot K3 appends K3 — JWKS on the K3 boot serves
/// K1 and K2 alongside the active K3. Best-effort with a loud warning on
/// write failure (old kids stop resolving after this boot's rotation).
fn retain_active_key_in_history(
    sqlite_db: &Option<Arc<gyre_adapters::SqliteStorage>>,
    pg_db: &Option<Arc<gyre_adapters::PgStorage>>,
    mut history: Vec<auth::HistoricalSigningKey>,
    active: &auth::AgentSigningKey,
) -> Vec<auth::HistoricalSigningKey> {
    if history.iter().any(|k| k.kid == active.kid) {
        return history;
    }
    history.push(auth::HistoricalSigningKey::from_public_key_bytes(
        active.public_key_bytes.clone(),
    ));

    #[derive(serde::Serialize)]
    struct HistoryEntry<'a> {
        kid: &'a str,
        public_key_b64: String,
    }
    let entries: Vec<HistoryEntry> = history
        .iter()
        .map(|k| HistoryEntry {
            kid: &k.kid,
            public_key_b64: B64.encode(&k.public_key_bytes),
        })
        .collect();
    let encoded = serde_json::to_string(&entries)
        .unwrap_or_else(|e| panic!("signing-key history must serialize (plain strings): {e}"));

    let store_result = if let Some(pg) = pg_db {
        pg.kv_set_sync(
            AGENT_SIGNING_KEY_KV_NAMESPACE,
            AGENT_SIGNING_KEY_HISTORY_KV_KEY,
            &encoded,
        )
    } else if let Some(sqlite) = sqlite_db {
        sqlite.kv_set_sync(
            AGENT_SIGNING_KEY_KV_NAMESPACE,
            AGENT_SIGNING_KEY_HISTORY_KV_KEY,
            &encoded,
        )
    } else {
        Ok(())
    };
    if let Err(e) = store_result {
        tracing::warn!(
            error = %e,
            "could not persist the signing-key history; signatures made before a rotation will not verify after a restart"
        );
    }
    history
}

/// Resolve the forge Ed25519 signing key for this boot, along with the
/// retained public-key history JWKS serves after rotations (see
/// `AGENT_SIGNING_KEY_HISTORY_KV_KEY`). Resolution order:
///
/// 1. `GYRE_AGENT_SIGNING_KEY` (base64 PKCS#8 DER, v1 or v2) — explicit
///    operator provisioning; takes precedence and the private material is
///    never written to the DB (only its public key joins the retained
///    history, which is the specced rotation-visibility behavior), so
///    rotation is a deliberate operator act. Invalid material is a
///    hard startup error: silently falling back to a different key would
///    break every signature the operator expects to verify.
/// 2. The key persisted in the durable KV store (PostgreSQL or SQLite,
///    whichever backend `build_state` selected) — the default path.
/// 3. A freshly generated key, persisted for the next boot. In pure
///    in-memory mode (no DB) the key is fresh per boot, matching the
///    process-lifetime scope of everything else in that mode.
///
/// The returned history contains every key ever active on this deployment
/// (including the active one); JWKS filters the active kid out at serve
/// time.
fn load_or_persist_agent_signing_key(
    sqlite_db: &Option<Arc<gyre_adapters::SqliteStorage>>,
    pg_db: &Option<Arc<gyre_adapters::PgStorage>>,
) -> (Arc<auth::AgentSigningKey>, Vec<auth::HistoricalSigningKey>) {
    // 1. Operator-provisioned key.
    if let Ok(encoded) = std::env::var("GYRE_AGENT_SIGNING_KEY") {
        let der = B64
            .decode(encoded.trim())
            .unwrap_or_else(|e| panic!("GYRE_AGENT_SIGNING_KEY is not valid base64: {e}"));
        let key = auth::AgentSigningKey::from_pkcs8(&der).unwrap_or_else(|e| {
            panic!("GYRE_AGENT_SIGNING_KEY is not valid Ed25519 PKCS#8 material: {e}")
        });
        tracing::info!(
            kid = %key.kid,
            "using operator-provisioned agent signing key from GYRE_AGENT_SIGNING_KEY"
        );
        // Rotation bookkeeping. The retained history normally already holds
        // the previous active key (every DB-backed boot adds its key). One
        // case does not: a deployment that predates history retention has
        // its previous key only in the current slot — fold that key into
        // the history so this rotation keeps its kid resolvable in JWKS.
        // Then make sure this boot's key joins the history too, so its kid
        // keeps resolving if the operator rotates onward later.
        let mut history = load_signing_key_history(sqlite_db, pg_db);
        let persisted_current: anyhow::Result<Option<String>> = if let Some(pg) = pg_db {
            pg.kv_get_sync(AGENT_SIGNING_KEY_KV_NAMESPACE, AGENT_SIGNING_KEY_KV_KEY)
        } else if let Some(sqlite) = sqlite_db {
            sqlite.kv_get_sync(AGENT_SIGNING_KEY_KV_NAMESPACE, AGENT_SIGNING_KEY_KV_KEY)
        } else {
            Ok(None)
        };
        match persisted_current {
            Ok(Some(encoded)) => {
                match B64
                    .decode(encoded.trim())
                    .map_err(|e| format!("base64: {e}"))
                    .and_then(|der| auth::AgentSigningKey::from_pkcs8(&der))
                {
                    Ok(previous) if previous.kid != key.kid => {
                        if !history.iter().any(|h| h.kid == previous.kid) {
                            tracing::info!(
                                kid = %previous.kid,
                                "retaining the previous signing key in JWKS history after rotation"
                            );
                            history.push(auth::HistoricalSigningKey::from_public_key_bytes(
                                previous.public_key_bytes.clone(),
                            ));
                        }
                    }
                    Ok(_) => { /* same key re-provisioned — no rotation */ }
                    Err(e) => {
                        tracing::warn!(
                            error = %e,
                            "persisted agent signing key is unreadable; its kid cannot be retained in JWKS history"
                        );
                    }
                }
            }
            Ok(None) => { /* first boot with this DB — nothing to retain */ }
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    "could not read the persisted agent signing key; the pre-rotation kid cannot be retained in JWKS history"
                );
            }
        }
        let history = retain_active_key_in_history(sqlite_db, pg_db, history, &key);
        return (Arc::new(key), history);
    }

    // 2. Key persisted in the durable KV store.
    let persisted: anyhow::Result<Option<String>> = if let Some(pg) = pg_db {
        pg.kv_get_sync(AGENT_SIGNING_KEY_KV_NAMESPACE, AGENT_SIGNING_KEY_KV_KEY)
    } else if let Some(sqlite) = sqlite_db {
        sqlite.kv_get_sync(AGENT_SIGNING_KEY_KV_NAMESPACE, AGENT_SIGNING_KEY_KV_KEY)
    } else {
        Ok(None)
    };

    match persisted {
        Ok(Some(encoded)) => match B64
            .decode(encoded.trim())
            .map_err(|e| format!("base64: {e}"))
            .and_then(|der| auth::AgentSigningKey::from_pkcs8(&der))
        {
            Ok(key) => {
                tracing::info!(
                    kid = %key.kid,
                    "loaded persisted agent signing key (local-mode commit signatures remain verifiable across restarts)"
                );
                let history = retain_active_key_in_history(
                    sqlite_db,
                    pg_db,
                    load_signing_key_history(sqlite_db, pg_db),
                    &key,
                );
                return (Arc::new(key), history);
            }
            Err(e) => {
                // Unreadable stored material: regenerate and overwrite below,
                // loudly — local-mode records signed by the old key will no
                // longer verify, which the operator must know about.
                tracing::error!(
                    error = %e,
                    "persisted agent signing key is unreadable; generating a replacement"
                );
            }
        },
        Ok(None) => { /* first boot with this DB — generate below */ }
        Err(e) => {
            // Read failed (transient DB trouble): keep this boot's key
            // process-local and do NOT overwrite storage (including the
            // key history), so a transient failure cannot destroy the
            // existing key.
            tracing::warn!(
                error = %e,
                "could not read persisted agent signing key; using a process-local key for this boot"
            );
            return (Arc::new(auth::AgentSigningKey::generate()), Vec::new());
        }
    }

    // 3. Fresh key, persisted for subsequent boots.
    let key = Arc::new(auth::AgentSigningKey::generate());
    let encoded = B64.encode(key.pkcs8_der());
    let store_result = if let Some(pg) = pg_db {
        pg.kv_set_sync(
            AGENT_SIGNING_KEY_KV_NAMESPACE,
            AGENT_SIGNING_KEY_KV_KEY,
            &encoded,
        )
    } else if let Some(sqlite) = sqlite_db {
        sqlite.kv_set_sync(
            AGENT_SIGNING_KEY_KV_NAMESPACE,
            AGENT_SIGNING_KEY_KV_KEY,
            &encoded,
        )
    } else {
        Ok(())
    };
    if let Err(e) = store_result {
        tracing::warn!(
            error = %e,
            "could not persist the agent signing key; local-mode commit signatures will not verify after a restart"
        );
    }
    let history = retain_active_key_in_history(
        sqlite_db,
        pg_db,
        load_signing_key_history(sqlite_db, pg_db),
        &key,
    );
    (key, history)
}

/// KV key holding the persisted SPIFFE trust-domain CA (local mode).
pub const SPIFFE_CA_KV_KEY: &str = "spiffe_trust_domain_ca";

/// Resolve the SPIFFE workload-attestation service for this boot
/// (identity-security.md Layer 1):
///
/// - Trust domain: `GYRE_SPIFFE_TRUST_DOMAIN` (default `gyre.example.com`).
/// - External mode: `GYRE_SPIFFE_WORKLOAD_API_SOCKET` (UDS path to a SPIRE
///   Workload API) + `GYRE_SPIFFE_TRUST_BUNDLE_URL` (the trust domain's
///   bundle in SPIFFE bundle format, e.g. SPIRE's federation bundle
///   endpoint). SVIDs are then obtained from the Workload API and the
///   operator-configured bundle is the verification anchor.
/// - Local mode (default): Gyre operates a trust-domain CA, persisted in
///   the durable KV store like the agent signing key. `GYRE_SPIFFE_CA`
///   (JSON: base64 PKCS#8 key + CA certificate PEM) provisions the CA
///   explicitly and is never overwritten.
///
/// Invalid operator material is a hard startup error: silently falling
/// back to a different anchor would break every SVID the operator expects
/// to verify.
fn load_spiffe_service(
    sqlite_db: &Option<Arc<gyre_adapters::SqliteStorage>>,
    pg_db: &Option<Arc<gyre_adapters::PgStorage>>,
) -> Arc<spiffe::SpiffeService> {
    let trust_domain = std::env::var("GYRE_SPIFFE_TRUST_DOMAIN")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| spiffe::DEFAULT_TRUST_DOMAIN.to_string());
    // Validate: the trust domain must be usable in a SPIFFE ID.
    if gyre_spiffe::SpiffeId::agent(&trust_domain, "worker", "probe").is_err() {
        panic!("GYRE_SPIFFE_TRUST_DOMAIN {trust_domain:?} is not a valid SPIFFE trust domain");
    }
    let svid_ttl_secs = std::env::var("GYRE_SPIFFE_SVID_TTL")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|ttl: &u64| *ttl > 0)
        .unwrap_or(gyre_spiffe::BASE_SVID_TTL_SECS);

    if let Ok(socket) = std::env::var("GYRE_SPIFFE_WORKLOAD_API_SOCKET") {
        let Ok(bundle_url) = std::env::var("GYRE_SPIFFE_TRUST_BUNDLE_URL") else {
            panic!(
                "GYRE_SPIFFE_WORKLOAD_API_SOCKET requires GYRE_SPIFFE_TRUST_BUNDLE_URL \
                 (the trust domain's SPIFFE bundle in SPIFFE bundle format)"
            );
        };
        let service =
            spiffe::SpiffeService::external(trust_domain, socket, bundle_url, svid_ttl_secs)
                .expect("external SPIFFE service construction must succeed");
        tracing::info!(
            trust_domain = %service.trust_domain(),
            "SPIFFE workload attestation: external SPIRE Workload API mode"
        );
        return Arc::new(service);
    }

    // 1. Operator-provisioned CA.
    if let Ok(encoded) = std::env::var("GYRE_SPIFFE_CA") {
        let serialized: gyre_spiffe::ca::SerializedCa = serde_json::from_str(encoded.trim())
            .unwrap_or_else(|e| panic!("GYRE_SPIFFE_CA is not valid CA material: {e}"));
        let ca = gyre_spiffe::TrustDomainCa::from_serialized(&serialized)
            .unwrap_or_else(|e| panic!("GYRE_SPIFFE_CA is not usable CA material: {e}"));
        tracing::info!(
            trust_domain = %trust_domain,
            "using operator-provisioned SPIFFE trust-domain CA from GYRE_SPIFFE_CA"
        );
        return Arc::new(
            spiffe::SpiffeService::local(trust_domain, ca, svid_ttl_secs)
                .expect("local SPIFFE service construction must succeed"),
        );
    }

    // 2. CA persisted in the durable KV store.
    let persisted: anyhow::Result<Option<String>> = if let Some(pg) = pg_db {
        pg.kv_get_sync(AGENT_SIGNING_KEY_KV_NAMESPACE, SPIFFE_CA_KV_KEY)
    } else if let Some(sqlite) = sqlite_db {
        sqlite.kv_get_sync(AGENT_SIGNING_KEY_KV_NAMESPACE, SPIFFE_CA_KV_KEY)
    } else {
        Ok(None)
    };
    match persisted {
        Ok(Some(json)) => match serde_json::from_str::<gyre_spiffe::ca::SerializedCa>(&json)
            .map_err(|e| format!("json: {e}"))
            .and_then(|serialized| {
                gyre_spiffe::TrustDomainCa::from_serialized(&serialized)
                    .map_err(|e| format!("ca: {e}"))
            }) {
            Ok(ca) => {
                tracing::info!(
                    trust_domain = %trust_domain,
                    "loaded persisted SPIFFE trust-domain CA (issued SVIDs and the trust bundle survive restarts)"
                );
                return Arc::new(
                    spiffe::SpiffeService::local(trust_domain, ca, svid_ttl_secs)
                        .expect("local SPIFFE service construction must succeed"),
                );
            }
            Err(e) => {
                // Unreadable stored material: regenerate and overwrite below,
                // loudly — SVIDs issued by the old CA will no longer verify,
                // which the operator must know about.
                tracing::error!(
                    error = %e,
                    "persisted SPIFFE CA is unreadable; generating a replacement"
                );
            }
        },
        Ok(None) => { /* first boot with this DB — generate below */ }
        Err(e) => {
            // Read failed (transient DB trouble): keep this boot's CA
            // process-local and do NOT overwrite storage, so a transient
            // failure cannot destroy the existing CA.
            tracing::warn!(
                error = %e,
                "could not read the persisted SPIFFE CA; using a process-local CA for this boot"
            );
            let ca = gyre_spiffe::TrustDomainCa::generate(&trust_domain)
                .expect("CA generation must succeed");
            return Arc::new(
                spiffe::SpiffeService::local(trust_domain, ca, svid_ttl_secs)
                    .expect("local SPIFFE service construction must succeed"),
            );
        }
    }

    // 3. Fresh CA, persisted for subsequent boots.
    let ca =
        gyre_spiffe::TrustDomainCa::generate(&trust_domain).expect("CA generation must succeed");
    let serialized = serde_json::to_string(&ca.serialize()).expect("CA serialization");
    let store_result = if let Some(pg) = pg_db {
        pg.kv_set_sync(
            AGENT_SIGNING_KEY_KV_NAMESPACE,
            SPIFFE_CA_KV_KEY,
            &serialized,
        )
    } else if let Some(sqlite) = sqlite_db {
        sqlite.kv_set_sync(
            AGENT_SIGNING_KEY_KV_NAMESPACE,
            SPIFFE_CA_KV_KEY,
            &serialized,
        )
    } else {
        Ok(())
    };
    if let Err(e) = store_result {
        tracing::warn!(
            error = %e,
            "could not persist the SPIFFE trust-domain CA; issued SVIDs will not verify after a restart"
        );
    }
    Arc::new(
        spiffe::SpiffeService::local(trust_domain, ca, svid_ttl_secs)
            .expect("local SPIFFE service construction must succeed"),
    )
}

/// Platform-appropriate absolute default SQLite database path
/// (specs/development/architecture.md §Storage — SQLite is the default
/// durable backend, zero ops).
///
/// `<data-dir>/gyre/gyre.db` where `<data-dir>` is the XDG data home on
/// Linux (`$XDG_DATA_HOME`, default `~/.local/share`),
/// `~/Library/Application Support` on macOS and `%APPDATA%` on Windows
/// (via the `dirs` crate). ALWAYS absolute — a bare relative filename
/// would resolve against whatever cwd the server happens to start in and
/// silently point different boots at different databases
/// (scripts/check-relative-path-defaults.sh). Parent directories are
/// auto-created by `build_state` on first boot.
pub fn default_sqlite_db_path() -> String {
    let data_dir = dirs::data_dir().unwrap_or_else(|| {
        panic!(
            "cannot determine the platform data directory for the default SQLite database \
             (is $HOME set?); set GYRE_DATABASE_URL to an explicit sqlite://<path> URL"
        )
    });
    data_dir.join("gyre").join("gyre.db").display().to_string()
}

/// Explicitly select the stateless in-memory storage mode for this
/// process — the in-process equivalent of the `GYRE_DATABASE_URL=memory`
/// sentinel (specs/development/architecture.md §Storage).
///
/// The default (unset) mode is durable SQLite at
/// [`default_sqlite_db_path`], so in-process harnesses (integration test
/// binaries, `#[cfg(test)]` suites inside the crate) that want the
/// stateless behavior must opt in explicitly: without this call, building
/// state would open the operator's real default database. Writes the
/// sentinel exactly once per process (`std::sync::Once`), so concurrent
/// callers race nothing and every later `build_state` in the process sees
/// the sentinel.
///
/// Test binaries that also point `GYRE_DATABASE_URL` at real databases
/// per test must NOT use this (the sentinel is sticky for the process);
/// they keep their own serialized env-var discipline instead.
pub fn select_in_memory_storage() {
    static SET_SENTINEL: std::sync::Once = std::sync::Once::new();
    SET_SENTINEL.call_once(|| {
        // SAFETY: the environment write happens exactly once per process,
        // serialized by `Once`; the only reader of this variable in the
        // process (`build_state`) runs after its caller's `call_once`
        // returns, which orders it after the write.
        unsafe { std::env::set_var("GYRE_DATABASE_URL", "memory") };
    });
}

/// Which durable backend `build_state` opens, derived from
/// `GYRE_DATABASE_URL` (specs/development/architecture.md §Storage).
#[derive(Debug, Clone, PartialEq, Eq)]
enum StorageSelection {
    /// `GYRE_DATABASE_URL=memory` — explicit stateless opt-in.
    InMemory,
    /// `postgres://` / `postgresql://` URL — PostgreSQL via Diesel.
    Postgres(String),
    /// `sqlite://<path>` (or bare path) URL — SQLite at that path.
    SqliteExplicit(String),
    /// Unset — durable SQLite at the platform default data path
    /// ([`default_sqlite_db_path`]).
    SqliteDefault(String),
}

impl StorageSelection {
    fn resolve(db_url: Option<&str>) -> Self {
        match db_url {
            // The stateless sentinel: filtered out BEFORE the sqlite-path
            // branch so it is never parsed as a sqlite file path.
            Some("memory") => Self::InMemory,
            Some(u) if u.starts_with("postgres://") || u.starts_with("postgresql://") => {
                Self::Postgres(u.to_string())
            }
            Some(u) => Self::SqliteExplicit(u.strip_prefix("sqlite://").unwrap_or(u).to_string()),
            None => Self::SqliteDefault(default_sqlite_db_path()),
        }
    }
}

/// Open (or create) the SQLite database at `path`, running the Diesel
/// migrations. Failures abort startup: silently continuing without the
/// configured durable store would turn every restart into data loss.
fn open_sqlite(path: &str) -> Arc<gyre_adapters::SqliteStorage> {
    Arc::new(
        gyre_adapters::SqliteStorage::new(path)
            .unwrap_or_else(|e| panic!("Failed to open SQLite at {path}: {e}")),
    )
}

pub fn build_state(
    auth_token: &str,
    base_url: &str,
    jwt_config: Option<Arc<JwtConfig>>,
) -> Arc<AppState> {
    let (message_dispatch_tx, message_dispatch_rx) = tokio::sync::mpsc::channel(256);
    let (message_broadcast_tx, _) = broadcast::channel(256);
    let (audit_broadcast_tx, _) = broadcast::channel(1024);
    let (notification_broadcast_tx, _) = broadcast::channel(1024);
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
    let selection = StorageSelection::resolve(db_url.as_deref());
    let sqlite_db: Option<Arc<gyre_adapters::SqliteStorage>> = match &selection {
        StorageSelection::InMemory => {
            tracing::info!(
                "storage: in-memory (GYRE_DATABASE_URL=memory; state is lost on restart)"
            );
            None
        }
        StorageSelection::Postgres(_) => None,
        StorageSelection::SqliteExplicit(path) => {
            tracing::info!(path, "storage: SQLite (explicit GYRE_DATABASE_URL)");
            Some(open_sqlite(path))
        }
        StorageSelection::SqliteDefault(path) => {
            tracing::info!(
                path,
                "storage: SQLite (default data path; durable across restarts)"
            );
            // Auto-create the default data directory so a zero-configuration
            // first boot works; an explicit path keeps its existing behavior
            // (a missing parent dir surfaces the open error below).
            if let Some(parent) = std::path::Path::new(path).parent() {
                std::fs::create_dir_all(parent).unwrap_or_else(|e| {
                    panic!(
                        "Failed to create default SQLite data directory {}: {e}",
                        parent.display()
                    )
                });
            }
            Some(open_sqlite(path))
        }
    };
    let pg_db: Option<Arc<gyre_adapters::PgStorage>> = match &selection {
        StorageSelection::Postgres(url) => {
            tracing::info!(url, "storage: PostgreSQL (GYRE_DATABASE_URL)");
            Some(Arc::new(gyre_adapters::PgStorage::new(url).unwrap_or_else(
                |e| panic!("Failed to connect to PostgreSQL: {e}"),
            )))
        }
        _ => None,
    };
    let db_storage = sqlite_db.clone();

    // Health-probe handle to whichever durable backend is configured
    // (business-continuity.md §2): DB connectivity and migration state are
    // evaluated against the real pool, SQLite or PostgreSQL alike.
    let storage: Option<Arc<dyn gyre_ports::storage::StoragePort>> = sqlite_db
        .clone()
        .map(|s| s as Arc<dyn gyre_ports::storage::StoragePort>)
        .or_else(|| {
            pg_db
                .clone()
                .map(|p| p as Arc<dyn gyre_ports::storage::StoragePort>)
        });

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

    // In pure in-memory mode (no DB), the workspace and policy repos must share a
    // single policy store so `apply_trust_transition` writes are visible to the
    // ABAC engine (which reads through `state.policies`). In DB-backed mode both
    // come from the same storage struct, so this store is unused.
    let mem_policy_store = Arc::new(Mutex::new(HashMap::new()));
    // Shared policy-set cache (§Performance): assigned into AppState both as
    // the `policies` wrapper's backing cache and as the `policy_cache`
    // invalidation handle.
    let policy_cache = Arc::new(policy_cache::PolicyCache::default());
    // Shared workspace store for pure-mem mode: `MemComputeTargetRepository`
    // pairs with `MemWorkspaceRepository` so `has_workspace_references`
    // (agent-runtime.md §3 delete guard) sees real workspace assignments
    // instead of a hardcoded empty set.
    let mem_workspace_store =
        Arc::new(Mutex::new(HashMap::<String, gyre_domain::Workspace>::new()));

    // In pure in-memory mode, the meta-spec and meta-spec-binding repos must
    // share a single binding store so the delete guard in
    // `MemMetaSpecRepository` sees the bindings the binding repo writes
    // (port contract: delete fails when bindings reference the meta-spec).
    // In DB-backed mode both ports come from the same storage struct, so
    // this store is unused.
    let mem_meta_spec_binding_store =
        Arc::new(RwLock::new(Vec::<gyre_domain::MetaSpecBinding>::new()));

    // In pure in-memory mode (no DB), the judgment ledger (HSI §12) aggregates
    // the SAME audit / spec-approval / spec-ledger stores these repos write
    // through. In DB-backed mode these stores are unused.
    let mem_audit = Arc::new(mem::MemAuditRepository::default());
    let mem_spec_approvals = Arc::new(mem::MemSpecApprovalRepository::default());
    let mem_spec_ledger = Arc::new(mem::MemSpecLedgerRepository::default());

    // Same as `store!` but the mem branch shares a pre-constructed store.
    macro_rules! store_shared {
        ($trait:ty, $arc:expr) => {
            if let Some(d) = &pg_db {
                Arc::clone(d) as Arc<$trait>
            } else if let Some(d) = &sqlite_db {
                Arc::clone(d) as Arc<$trait>
            } else {
                Arc::clone(&$arc) as Arc<$trait>
            }
        };
    }

    // search.md §Index Updates: entity repositories are wrapped in
    // search-sync decorators so every write path (REST, MCP, background
    // jobs) keeps the search index in sync. The search port must be resolved
    // first because the decorators hold a clone of it. It is wrapped in the
    // tenant-resolution decorator (search_scope) so every indexed document
    // carries its tenant scope — the mem fallback has no workspace table to
    // derive it from, so resolution happens server-side, uniformly for every
    // index path (§Access Scoping step 1).
    let workspaces: Arc<dyn WorkspaceRepository> = store!(
        dyn WorkspaceRepository,
        mem::MemWorkspaceRepository::with_policy_store(Arc::clone(&mem_policy_store))
            .with_workspace_store(Arc::clone(&mem_workspace_store))
    );
    let search: Arc<dyn gyre_ports::SearchPort> = search_scope::with_tenant_resolution(
        store!(
            dyn gyre_ports::SearchPort,
            gyre_adapters::MemSearchAdapter::new()
        ),
        Arc::clone(&workspaces),
    );
    let agents: Arc<dyn AgentRepository> = search_sync::wrap_agents(
        store!(dyn AgentRepository, mem::MemAgentRepository::default()),
        Arc::clone(&search),
    );
    // Shared task store: the persona repo's atomic
    // `apply_approval_reset_with_task` (persona approval reset + approval
    // task in one write) must land in the same store as every normal task
    // write. DB-backed modes use one storage object for both ports, so the
    // share only matters for the mem fallback.
    let mem_task_store: Arc<
        tokio::sync::Mutex<std::collections::HashMap<String, gyre_domain::Task>>,
    > = Arc::default();
    let tasks: Arc<dyn TaskRepository> = search_sync::wrap_tasks(
        store!(
            dyn TaskRepository,
            mem::MemTaskRepository::from_store(Arc::clone(&mem_task_store))
        ),
        Arc::clone(&search),
    );
    let merge_requests: Arc<dyn MergeRequestRepository> = search_sync::wrap_mrs(
        store!(dyn MergeRequestRepository, mem::MemMrRepository::default()),
        Arc::clone(&search),
    );
    let reviews: Arc<dyn ReviewRepository> = search_sync::wrap_reviews(
        store!(dyn ReviewRepository, mem::MemReviewRepository::default()),
        Arc::clone(&merge_requests),
        Arc::clone(&search),
    );
    let personas: Arc<dyn PersonaRepository> = search_sync::wrap_personas(
        store!(
            dyn PersonaRepository,
            mem::MemPersonaRepository::with_task_store(Arc::default(), Arc::clone(&mem_task_store))
        ),
        Arc::clone(&search),
    );
    let analytics: Arc<dyn AnalyticsRepository> = search_sync::wrap_analytics(
        store!(
            dyn AnalyticsRepository,
            mem::MemAnalyticsRepository::default()
        ),
        Arc::clone(&search),
    );
    // The audit repo keeps both concerns: search-sync indexing on every
    // recorded event (task-202) AND store sharing with the judgment ledger
    // in pure-mem mode (task-209, HSI §12) — the decorator wraps the shared
    // store instead of constructing its own.
    let audit: Arc<dyn AuditRepository> = search_sync::wrap_audit(
        store_shared!(dyn AuditRepository, mem_audit),
        Arc::clone(&search),
    );

    // agent-gates.md §Part 3 Storage: merge attestation bundles stored
    // as git notes on the merge commit + `merge_attestation_bundle`
    // audit events (queryable by MR id and commit SHA). Wraps the same
    // audit repository so bundles flow through the identical store
    // (SQLite/PG/mem) and search-sync path as every other audit event.
    let attestation_storage: Arc<dyn AttestationStorage> = Arc::new(
        gyre_adapters::MergeAttestationStorage::new(Arc::clone(&audit)),
    );

    // Spec-link graph persistence (spec-links.md §Forge-Maintained Spec Graph).
    // The SQL table is authoritative; the in-memory store is a hot cache,
    // loaded here at boot so the graph survives restarts without re-push.
    let spec_link_repo: Arc<dyn gyre_ports::SpecLinkRepository> = store!(
        dyn gyre_ports::SpecLinkRepository,
        mem::MemSpecLinkRepository::default()
    );
    let spec_links_store: spec_registry::SpecLinksStore = Arc::new(Mutex::new(Vec::new()));
    load_spec_links_into_store(&spec_link_repo, &spec_links_store);

    // Load or persist the forge's Ed25519 signing key first: both the
    // AppState field and the attestation verifier (which re-derives the
    // gate agents' keys from the same material) share it. The retained
    // public-key history keeps pre-rotation signatures verifiable via
    // JWKS (message-bus.md §Edge Cases "Signature key rotation").
    let (agent_signing_key, signing_key_history) =
        load_or_persist_agent_signing_key(&sqlite_db, &pg_db);
    // identity-security.md §Task-Scoped JWT Claims: aud is `gyre`, or
    // GYRE_OIDC_AUDIENCE when the operator configured one. Carried on
    // the key so every mint and every validation agree on the audience.
    // The audience is mint/validation metadata, not key material, so the
    // override keeps the kid (and the retained history) consistent.
    let agent_signing_key = {
        let audience = std::env::var("GYRE_OIDC_AUDIENCE")
            .ok()
            .filter(|a| !a.is_empty())
            .unwrap_or_else(|| crate::auth::DEFAULT_AGENT_JWT_AUDIENCE.to_string());
        if agent_signing_key.audience() == audience {
            agent_signing_key
        } else {
            Arc::new((*agent_signing_key).clone().with_audience(audience))
        }
    };

    // Rekor transparency-log publication for merge attestation bundles
    // (task-264): the REAL HTTP client replaces the old no-op stub. It is
    // configured only when GYRE_REKOR_URL is set — an unconfigured log
    // makes every submission fail explicitly (surfaced as a warning by
    // the storage path), never a silent Ok(None).
    let rekor: Arc<dyn RekorClient> = Arc::new(gyre_adapters::HttpRekorClient::new(
        std::env::var("GYRE_REKOR_URL")
            .ok()
            .map(|u| u.trim().to_string())
            .filter(|u| !u.is_empty()),
        agent_signing_key.public_key_pem(),
    ));

    // SPIFFE workload attestation (identity-security.md Layer 1): resolve
    // the trust domain and anchor before wiring AppState.
    let spiffe_service = load_spiffe_service(&sqlite_db, &pg_db);

    // SPIFFE federation (identity-security.md §Federation): exchanged
    // trust bundles for peer trust domains, keyed by trust domain.
    let spiffe_federation = Arc::new(spiffe_federation::FederationBundles::from_env(
        spiffe_service.trust_domain(),
    ));

    Arc::new(AppState {
        auth_token: auth_token.to_string(),
        base_url: base_url.to_string(),
        spiffe: spiffe_service,
        spiffe_federation,
        repos: store!(dyn RepoRepository, mem::MemRepoRepository::default()),
        agents,
        tasks,
        merge_requests,
        reviews,
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
        agent_sessions: store!(
            dyn AgentSessionRepository,
            mem::MemAgentSessionRepository::default()
        ),
        worktrees: store!(
            dyn WorktreeRepository,
            mem::MemWorktreeRepository::default()
        ),
        telemetry_buffer,
        message_broadcast_tx,
        kv_store: store!(dyn KvJsonStore, mem::MemKvStore::default()),
        agent_signing_key: agent_signing_key.clone(),
        // Retained signing-key history (see the loader): public material of
        // every key ever active on this deployment; JWKS serves the entries
        // whose kid is not the active key's.
        signing_key_history,
        // Attestation bundle verification protocol (agent-gates.md §Part 3
        // Verification): the forge-key verifier backing
        // POST /api/v1/attestations/verify and `gyre verify-attestation`.
        attestation_verifier: Arc::new(crate::merge_attestation::ForgeAttestationVerifier::new(
            agent_signing_key.clone(),
        )),
        // Agent JWT TTL (identity-security.md §Task-Scoped JWT Claims:
        // configurable via GYRE_AGENT_JWT_TTL, default 3600 s). The earlier
        // 300 s default was an undocumented M27.5 deviation from the claims
        // table; the spec default is restored here.
        agent_jwt_ttl_secs: std::env::var("GYRE_AGENT_JWT_TTL")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(3600),
        // Meta-spec preview mode (meta-spec-reconciliation.md §5).
        preview_ttl_secs: api::meta_specs::preview_ttl_secs_from_env(),
        preview_jwt_ttl_secs: api::meta_specs::preview_jwt_ttl_secs_from_env(),
        preview_budget_max_concurrent: api::meta_specs::preview_budget_max_concurrent_from_env(),
        users: store!(dyn UserRepository, mem::MemUserRepository::default()),
        api_keys: store!(dyn ApiKeyRepository, mem::MemApiKeyRepository::default()),
        jwt_config,
        http_client: reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("valid outbound HTTP client configuration"),
        metrics,
        started_at_secs,
        compose_sessions: Arc::new(Mutex::new(HashMap::new())),
        explorer_sessions: Arc::new(explorer_ws::ExplorerSessionRegistry::new()),
        retention_store: RetentionStore::new(),
        job_registry: Arc::new(JobRegistry::new()),
        budget_grace_in_progress: Arc::new(tokio::sync::Mutex::new(
            std::collections::HashSet::new(),
        )),
        analytics,
        costs: store!(dyn CostRepository, mem::MemCostRepository::default()),
        audit,
        attestation_storage,
        rekor,
        siem_store: SiemStore::new(),
        audit_broadcast_tx,
        notification_broadcast_tx,
        network_peers: store!(
            dyn NetworkPeerRepository,
            mem::MemNetworkPeerRepository::default()
        ),
        // Ephemeral preview environments (activity-dashboard.md §"Try it"
        // links) — durable rows through the port (durable-state invariant),
        // never an in-memory AppState map.
        preview_environments: store!(
            dyn gyre_ports::PreviewEnvironmentRepository,
            mem::MemPreviewEnvironmentRepository::default()
        ),
        dependencies: store!(
            dyn DependencyRepository,
            mem::MemDependencyRepository::default()
        ),
        breaking_changes: store!(
            dyn BreakingChangeRepository,
            mem::MemBreakingChangeRepository::default()
        ),
        dependency_policies: store!(
            dyn DependencyPolicyRepository,
            mem::MemDependencyPolicyRepository::default()
        ),
        cascade_test_results: store!(
            dyn CascadeTestResultRepository,
            mem::MemCascadeTestResultRepository::default()
        ),
        rate_limiter: rate_limit::RateLimiter::new(rate_per_sec),
        process_registry: Arc::new(Mutex::new(HashMap::new())),
        spawned_backends: Arc::new(Mutex::new(HashMap::new())),
        agent_logs: Arc::new(log_compression::AgentLogBuffer::new()),
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
        storage,
        spec_approvals: store_shared!(dyn SpecApprovalRepository, mem_spec_approvals),
        spec_policies: store!(
            dyn SpecPolicyRepository,
            mem::MemSpecPolicyRepository::default()
        ),
        spec_lifecycle_configs: store!(
            dyn SpecLifecycleRepository,
            mem::MemSpecLifecycleRepository::default()
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
        commit_signatures: store!(
            dyn gyre_ports::commit_signature_repo::CommitSignatureRepository,
            mem::MemCommitSignatureRepository::default()
        ),
        signing_config: commit_signatures::SigningConfig::from_env(),
        remote_jwks_cache: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
        trusted_proxies: trusted_proxy::TrustedProxies::from_env(),
        tunnel_store: Arc::new(Mutex::new(HashMap::new())),
        container_audits: store!(
            dyn ContainerAuditRepository,
            mem::MemContainerAuditRepository::default()
        ),
        spec_ledger: store_shared!(dyn SpecLedgerRepository, mem_spec_ledger),
        spec_approval_history: store!(
            dyn SpecApprovalEventRepository,
            mem::MemSpecApprovalEventRepository::default()
        ),
        spec_link_repo,
        spec_assertion_results: store!(
            dyn gyre_ports::SpecAssertionResultRepository,
            mem::MemSpecAssertionResultRepository::default()
        ),
        spec_links_store,
        budget_configs: store!(
            dyn BudgetRepository,
            mem::MemBudgetConfigRepository::default()
        ),
        tenant_invitations: store!(
            dyn gyre_ports::TenantInvitationRepository,
            mem::MemTenantInvitationRepository::default()
        ),
        workspace_invitations: store!(
            dyn gyre_ports::WorkspaceInvitationRepository,
            mem::MemWorkspaceInvitationRepository::default()
        ),
        // SCIM groups and group-to-workspace mappings are durable
        // (task-266): SQLite/PostgreSQL adapters implement the ports over
        // migration 000068; mem mode keeps the in-code store.
        scim_groups: store!(
            dyn gyre_ports::ScimGroupRepository,
            mem::MemScimGroupRepository::default()
        ),
        scim_mappings: store!(
            dyn gyre_ports::ScimMappingRepository,
            mem::MemScimMappingRepository::default()
        ),
        budget_usages: store!(
            dyn BudgetUsageRepository,
            mem::MemBudgetUsageRepository::default()
        ),
        budget_calls: store!(
            dyn BudgetCallRepository,
            mem::MemBudgetCallRepository::default()
        ),
        search,
        tenants: store!(
            dyn gyre_ports::TenantRepository,
            mem::MemTenantRepository::default()
        ),
        workspaces,
        personas,
        // §Performance: the policy repository is wrapped in a read-through
        // cache — policy evaluation happens on every request and must not
        // hit the store each time. Mutations through the port invalidate
        // the cache; `apply_trust_transition` (the one out-of-band policy
        // writer) is invalidated explicitly by its handlers via
        // `policy_cache`.
        policies: Arc::new(policy_cache::CachedPolicyRepository::new(
            store!(
                dyn PolicyRepository,
                mem::MemPolicyRepository::from_store(Arc::clone(&mem_policy_store))
            ),
            Arc::clone(&policy_cache),
        )) as Arc<dyn PolicyRepository>,
        policy_cache,
        // Workspace memberships are durable: SQLite/PostgreSQL adapters
        // implement the port (M22.8 schema); mem mode keeps the in-code
        // store. Before task-121 this was unconditionally the mem adapter,
        // so a DB-backed server silently lost every membership on restart.
        workspace_memberships: store!(
            dyn WorkspaceMembershipRepository,
            mem::MemWorkspaceMembershipRepository::default()
        ),
        repo_access_overrides: store!(
            dyn RepoAccessOverrideRepository,
            mem::MemRepoAccessOverrideRepository::default()
        ),
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
            mem::MemMetaSpecRepository::with_binding_store(Arc::clone(
                &mem_meta_spec_binding_store
            ))
        ),
        meta_spec_bindings: store!(
            dyn MetaSpecBindingRepository,
            mem::MemMetaSpecBindingRepository::with_store(Arc::clone(&mem_meta_spec_binding_store))
        ),
        meta_spec_sets: store!(
            dyn MetaSpecSetRepository,
            mem::MemMetaSpecSetRepository::default()
        ),
        tenant_meta_spec_sets: store!(
            dyn TenantMetaSpecSetRepository,
            mem::MemTenantMetaSpecSetRepository::default()
        ),
        rollout_policies: store!(
            dyn RolloutPolicyRepository,
            mem::MemRolloutPolicyRepository::default()
        ),
        messages: store!(
            dyn gyre_ports::MessageRepository,
            mem::MemMessageRepository::default()
        ),
        message_dispatch_tx: message_dispatch_tx.clone(),
        message_dispatch_rx: std::sync::Arc::new(tokio::sync::Mutex::new(Some(
            message_dispatch_rx,
        ))),
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
        ws_connection_users: Arc::new(RwLock::new(HashMap::new())),
        traces: store!(dyn TraceRepository, mem::MemTraceRepository::default()),
        otlp_config: otlp_receiver::OtlpServerConfig::from_env(),
        conversations: store!(
            dyn ConversationRepository,
            mem::MemConversationRepository::default()
        ),
        repos_root: configured_repos_path().to_string_lossy().into_owned(),
        prompt_templates: store!(
            dyn gyre_ports::PromptRepository,
            mem::MemPromptRepository::default()
        ),
        compute_targets: store!(
            dyn ComputeTargetRepository,
            mem::MemComputeTargetRepository::with_workspace_store(Arc::clone(&mem_workspace_store))
        ),
        user_tokens: store!(
            dyn gyre_ports::UserTokenRepository,
            mem::MemUserTokenRepository::default()
        ),
        user_notification_prefs: store!(
            dyn gyre_ports::UserNotificationPreferenceRepository,
            mem::MemUserNotificationPreferenceRepository::default()
        ),
        sessions: store!(
            dyn gyre_ports::SessionRepository,
            mem::MemSessionRepository::default()
        ),
        user_channel_prefs: store!(
            dyn gyre_ports::UserChannelPreferenceRepository,
            mem::MemUserChannelPreferenceRepository::default()
        ),
        secrets: store!(
            dyn gyre_ports::SecretRepository,
            mem::MemSecretRepository::default()
        ),
        judgment_ledger: store!(
            dyn gyre_ports::JudgmentLedgerRepository,
            mem::MemJudgmentLedgerRepository::with_sources(
                &mem_audit,
                &mem_spec_approvals,
                &mem_spec_ledger
            )
        ),
        orchestrator_registry: signal_chain::OrchestratorRegistry::new(),
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

/// Load the persisted spec-link graph into the in-memory hot cache
/// (spec-links.md §Forge-Maintained Spec Graph).
///
/// Runs on a dedicated thread with its own single-threaded runtime: callers
/// are sync constructors (`build_state`) that may already be inside a tokio
/// runtime, where `Handle::block_on` would panic. On read failure the cache
/// stays empty and the error is surfaced — an unreadable graph must not
/// silently masquerade as "no links" to staleness checks and gates.
pub fn load_spec_links_into_store(
    repo: &Arc<dyn gyre_ports::SpecLinkRepository>,
    store: &spec_registry::SpecLinksStore,
) {
    let repo = Arc::clone(repo);
    let store = Arc::clone(store);
    let handle = std::thread::spawn(move || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build runtime for spec link load")
            .block_on(async {
                match repo.list_all().await {
                    Ok(persisted) => {
                        if !persisted.is_empty() {
                            tracing::info!(
                                count = persisted.len(),
                                "spec-links: loaded persisted spec link graph"
                            );
                        }
                        *store.lock().await = persisted;
                    }
                    Err(e) => {
                        tracing::error!(
                            "spec-links: failed to load persisted spec link graph: {e}"
                        );
                    }
                }
            });
    })
    .join();
    if let Err(e) = handle {
        tracing::error!("spec-links: spec link load thread panicked: {e:?}");
    }
}

/// Evict stale presence entries (server_last_seen older than 60 seconds).
///
/// An entry is stale if `server_last_seen` is more than 60 seconds old.
/// The evicted connection (if still open) receives a `PresenceEvicted` message,
/// and other workspace subscribers receive a departure `UserPresence`
/// (HSI §7 Presence Awareness — every removal path must notify others).
pub(crate) async fn evict_stale_presence(state: &Arc<AppState>) {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;

    let stale: Vec<((String, String), u64, String)> = {
        let map = state.presence.read().await;
        map.iter()
            .filter(|(_, entry)| entry.server_last_seen + 60_000 < now_ms)
            .map(|(key, entry)| (key.clone(), entry.connection_id, entry.workspace_id.clone()))
            .collect()
    };

    for ((user_id, session_id), conn_id, workspace_id) in stale {
        state
            .presence
            .write()
            .await
            .remove(&(user_id.clone(), session_id.clone()));

        let evict_msg = WsMessage::PresenceEvicted {
            session_id: session_id.clone(),
        };
        if let Ok(payload) = serde_json::to_string(&evict_msg) {
            let conns = state.ws_connections.read().await;
            if let Some(tx) = conns.get(&conn_id) {
                let _ = tx.try_send(payload);
            }
        }

        crate::ws::broadcast_presence_departure(state, &user_id, &session_id, &workspace_id).await;
    }
}

/// Spawn a background task that evicts stale presence entries every 30 seconds.
pub fn spawn_presence_eviction(state: Arc<AppState>) {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;
            evict_stale_presence(&state).await;
        }
    });
}

/// Emit a `ReconciliationCompleted` Event-tier message (HSI §4 /
/// meta-spec-reconciliation.md §11).
///
/// The priority-6 `MetaSpecDrift` notifications are created downstream by the
/// notification bridge (`NotificationBridge` in `message_dispatcher.rs`), per
/// HSI §8 p6: "Via `MessageConsumer` consuming `ReconciliationCompleted`
/// events". This function only emits the event.
pub async fn emit_reconciliation_completed(
    state: &Arc<AppState>,
    workspace_id: Id,
    payload: Option<serde_json::Value>,
) {
    state
        .emit_event(
            workspace_id.clone(),
            Destination::Workspace(workspace_id),
            MessageKind::ReconciliationCompleted,
            payload,
        )
        .await;
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

/// Materialize the default Tenant entity on first startup
/// (hierarchy-enforcement.md §1 Bootstrap Behavior).
///
/// On an empty database the platform has no Tenant row backing the `"default"`
/// tenant scope that every tenant-filtered adapter query resolves against —
/// the scope would be a phantom. This creates it with the spec-mandated
/// field values: name "Default", slug "default", no OIDC issuer, unlimited
/// budget (`BudgetConfig::default()`), no workspace cap. The row's id is
/// `"default"` — the operative single-tenant scope — so it is the real
/// tenant existing data references, not an orphaned UUID (the spec's
/// `deterministic_uuid` form is deferred multi-tenant work, §68-70).
///
/// Idempotent: a no-op when the default tenant already exists.
pub async fn bootstrap_default_tenant(state: &Arc<AppState>) {
    use gyre_domain::budget::BudgetConfig;
    use gyre_domain::tenant::Tenant;

    let default_id = Id::new("default");
    if let Ok(Some(_)) = state.tenants.find_by_id(&default_id).await {
        return; // already bootstrapped
    }

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let mut tenant = Tenant::new(default_id, "Default", "default", now);
    tenant.budget = Some(BudgetConfig::default());

    if let Err(e) = state.tenants.create(&tenant).await {
        tracing::warn!("failed to bootstrap default tenant: {e}");
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

/// Spawn the budget rolling-window background job (agent-runtime.md §4
/// Budget Reset): runs hourly and re-syncs every entity's token/cost
/// counters to the per-call records inside the last 24 hours, so
/// time-based budgets (`max_tokens_per_day`, `max_cost_per_day`) reset on
/// a rolling window. Replaces the pre-task-119 midnight-UTC counter reset.
/// The same cycle enforces the per-agent lifetime ceilings
/// (`max_agent_lifetime_secs`, repo override → effective workspace limit)
/// so agents that stopped reporting usage are still bounded.
pub fn spawn_budget_daily_reset(state: Arc<AppState>) {
    tokio::spawn(async move {
        state
            .job_registry
            .mark_scheduled("spawn_budget_reset")
            .await;
        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(3600)).await;
            let started_at = jobs::now_secs();
            let result = api::budget::apply_rolling_24h_window(&state).await;
            if let Err(error) = &result {
                tracing::error!(%error, "failed to apply budget rolling 24h window");
            }
            // Lifetime enforcement is best-effort alongside the reset —
            // its failures are logged inside, never fail the job cycle.
            api::budget::enforce_agent_lifetimes(&state).await;
            state
                .job_registry
                .record_cycle("spawn_budget_reset", started_at, &result)
                .await;
        }
    });
}

/// Spawn a background task that deletes long-expired user sessions
/// (user-management.md §Session Management — task-111 plan §6).
///
/// Sessions whose `expires_at` is more than 30 days in the past are
/// deleted. Revoked rows are NEVER deleted, whatever their age (the
/// `SessionRepository::delete_expired_before` port contract): a revoked row
/// is the durable record that keeps a signed-out credential rejected
/// ("sign out everywhere") — deleting one would silently re-authorize the
/// API key it belonged to.
pub fn spawn_session_cleanup(state: Arc<AppState>) {
    const INTERVAL_SECS: u64 = 24 * 3600; // daily
    const RETENTION_PAST_EXPIRY_SECS: u64 = 30 * 24 * 3600; // 30 days
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(INTERVAL_SECS)).await;
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let cutoff = now.saturating_sub(RETENTION_PAST_EXPIRY_SECS);
            match state.sessions.delete_expired_before(cutoff).await {
                Ok(n) if n > 0 => {
                    tracing::info!(
                        deleted = n,
                        "session retention cleanup removed expired sessions"
                    );
                }
                Ok(_) => {}
                Err(e) => {
                    tracing::warn!("session retention cleanup failed: {e}");
                }
            }
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
            approved_content_hash: Some(sha256_hex(prompt)),
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

/// Seed the four built-in personas (platform-model.md §2) into every
/// existing tenant. Idempotent: an existing persona with the same slug at
/// Tenant scope is never overwritten — the user may have customized it, and
/// the spec allows workspace/repo-scope overrides to shadow it.
///
/// Called at server startup, after migrations and tenant bootstrap.
pub async fn seed_builtin_personas(state: &Arc<AppState>) {
    let tenants = match state.tenants.list().await {
        Ok(t) => t,
        Err(e) => {
            tracing::warn!("seed_builtin_personas: failed to list tenants: {e}");
            return;
        }
    };
    for tenant in &tenants {
        seed_builtin_personas_for_tenant(state, &tenant.id).await;
    }
}

/// Seed the built-in personas for one tenant — the shared tail of startup
/// seeding and tenant creation (`POST /api/v1/tenants`), so every tenant
/// gets the spec §2 table the moment it exists. Idempotent per slug+scope.
pub async fn seed_builtin_personas_for_tenant(state: &Arc<AppState>, tenant_id: &Id) {
    use gyre_domain::{builtin_personas, PersonaScope};

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    for persona in builtin_personas(tenant_id, now) {
        let scope = PersonaScope::Tenant(tenant_id.clone());
        match state
            .personas
            .find_by_slug_and_scope(&persona.slug, &scope)
            .await
        {
            Ok(Some(_)) => {} // already exists → idempotent
            Ok(None) => match state.personas.create(&persona).await {
                Ok(()) => tracing::debug!(
                    slug = %persona.slug,
                    tenant_id = %tenant_id,
                    "Seeded built-in persona"
                ),
                Err(e) => tracing::warn!(
                    slug = %persona.slug,
                    tenant_id = %tenant_id,
                    err = %e,
                    "Failed to seed built-in persona"
                ),
            },
            Err(e) => tracing::warn!(
                slug = %persona.slug,
                tenant_id = %tenant_id,
                err = %e,
                "Error checking built-in persona existence"
            ),
        }
    }
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

    #[test]
    fn storage_selection_matrix() {
        // The stateless sentinel — filtered out before the sqlite branch so
        // it is never parsed as a sqlite file path.
        assert_eq!(
            StorageSelection::resolve(Some("memory")),
            StorageSelection::InMemory
        );
        // PostgreSQL URLs (both spellings) select PostgreSQL.
        assert_eq!(
            StorageSelection::resolve(Some("postgres://u:p@h/db")),
            StorageSelection::Postgres("postgres://u:p@h/db".to_string())
        );
        assert_eq!(
            StorageSelection::resolve(Some("postgresql://h/db")),
            StorageSelection::Postgres("postgresql://h/db".to_string())
        );
        // Explicit sqlite:// URLs (and bare paths) select that exact path.
        assert_eq!(
            StorageSelection::resolve(Some("sqlite://tmp/x.db")),
            StorageSelection::SqliteExplicit("tmp/x.db".to_string())
        );
        assert_eq!(
            StorageSelection::resolve(Some("/abs/path.db")),
            StorageSelection::SqliteExplicit("/abs/path.db".to_string())
        );
        // Unset — the shipped default — is durable SQLite at the platform
        // default data path, which must ALWAYS be absolute (a bare relative
        // filename would resolve against the process cwd and silently point
        // different boots at different databases).
        match StorageSelection::resolve(None) {
            StorageSelection::SqliteDefault(path) => assert!(
                std::path::Path::new(&path).is_absolute(),
                "default SQLite path must be absolute, got {path}"
            ),
            other => {
                panic!("unset GYRE_DATABASE_URL must select the default SQLite path, got {other:?}")
            }
        }
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
    async fn healthz_503_until_required_jobs_running() {
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
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["status"], "error");
        // In-memory mode: no DB backend configured — check passes honestly.
        assert_eq!(json["checks"]["database"], "not_configured");
        assert_eq!(json["checks"]["merge_processor"], "not_scheduled");
    }

    // readyz must gate on the merge processor loop being real: a router
    // whose scheduler never ran must report 503 (unauthenticated — reaching
    // the probe at all, with 503 not 401, proves no auth is required).
    #[tokio::test]
    async fn readyz_503_until_merge_processor_running() {
        let state = mem::test_state();
        jobs::start_job_registry(state.clone()).await;
        let app = build_router(state);
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/readyz")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["status"], "error");
        assert_eq!(json["checks"]["merge_processor"], "not_scheduled");
    }

    #[tokio::test]
    async fn readyz_200_when_merge_processor_running() {
        let state = mem::test_state();
        jobs::start_job_registry(state.clone()).await;
        state.job_registry.mark_scheduled("merge_processor").await;
        state
            .job_registry
            .record_cycle(
                "merge_processor",
                jobs::now_secs(),
                &Ok::<(), anyhow::Error>(()),
            )
            .await;
        let app = build_router(state);
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
        assert_eq!(json["checks"]["merge_processor"], "ok");
        assert_eq!(json["checks"]["database"], "not_configured");
        assert_eq!(json["checks"]["migrations"], "not_configured");
    }

    /// Serialize env-mutating tests: GYRE_REPOS_PATH is process-global and
    /// parallel test threads race on it.
    static REPOS_PATH_ENV_LOCK: parking_lot::Mutex<()> = parking_lot::Mutex::new(());

    /// F6 pin: a relative GYRE_REPOS_PATH (e.g. "repos" or "./repos") must
    /// resolve to an ABSOLUTE path anchored at the process cwd —
    /// `repos_root` is persisted into `repo.path` rows and handed to jj/git
    /// child processes, which resolve paths against the COMMAND's cwd, not
    /// the server's. A relative root silently breaks every child-process
    /// operation in default deployments.
    #[test]
    fn configured_repos_path_makes_relative_config_absolute() {
        let _guard = REPOS_PATH_ENV_LOCK.lock();
        let cwd = std::env::current_dir().unwrap();

        std::env::set_var("GYRE_REPOS_PATH", "repos");
        let p = configured_repos_path();
        assert!(
            p.is_absolute(),
            "relative GYRE_REPOS_PATH must resolve absolute, got {p:?}"
        );
        assert_eq!(p, cwd.join("repos"));

        std::env::set_var("GYRE_REPOS_PATH", "./repos");
        let p = configured_repos_path();
        assert_eq!(
            p,
            cwd.join("repos"),
            "'./repos' must normalize to <cwd>/repos"
        );

        std::env::remove_var("GYRE_REPOS_PATH");
    }

    /// F6 pin (default): with no GYRE_REPOS_PATH configured, the root
    /// defaults to an absolute <cwd>/repos — never the bare relative
    /// "./repos" the default deployment used before the fix.
    #[test]
    fn configured_repos_path_default_is_absolute() {
        let _guard = REPOS_PATH_ENV_LOCK.lock();
        std::env::remove_var("GYRE_REPOS_PATH");

        let p = configured_repos_path();
        let cwd = std::env::current_dir().unwrap();
        assert_eq!(p, cwd.join("repos"));
        assert!(p.is_absolute());
    }

    /// F6 pin (absolute passthrough): an absolute GYRE_REPOS_PATH must be
    /// used verbatim.
    #[test]
    fn configured_repos_path_absolute_passthrough() {
        let _guard = REPOS_PATH_ENV_LOCK.lock();
        std::env::set_var("GYRE_REPOS_PATH", "/srv/gyre/repos");
        assert_eq!(
            configured_repos_path(),
            std::path::PathBuf::from("/srv/gyre/repos")
        );
        std::env::remove_var("GYRE_REPOS_PATH");
    }

    /// platform-model.md §2: startup seeding creates the four built-in
    /// personas at tenant level, pre-approved, and is idempotent.
    #[tokio::test]
    async fn seed_builtin_personas_creates_four_and_is_idempotent() {
        use gyre_domain::{PersonaApprovalStatus, PersonaScope, Tenant};

        let state = mem::test_state();
        // Startup seeding enumerates tenants — create one first (the way
        // migrations/bootstrap would have).
        let tenant = Tenant::new(Id::new("t-seed"), "Seed Tenant", "seed-tenant", 1);
        state.tenants.create(&tenant).await.unwrap();

        seed_builtin_personas(&state).await;

        let scope = PersonaScope::Tenant(Id::new("t-seed"));
        let seeded = state.personas.list_by_scope(&scope).await.unwrap();
        let mut slugs: Vec<String> = seeded.iter().map(|p| p.slug.clone()).collect();
        slugs.sort();
        assert_eq!(
            slugs,
            vec![
                "accountability",
                "repo-orchestrator",
                "security",
                "workspace-orchestrator"
            ],
            "all four built-in personas must be seeded at tenant scope"
        );
        for p in &seeded {
            assert_eq!(
                p.approval_status,
                PersonaApprovalStatus::Approved,
                "built-in {} must be pre-approved",
                p.slug
            );
            assert_eq!(p.approved_by.as_deref(), Some("system"));
            assert_eq!(p.version, 1);
        }

        // Second run (server restart): no duplicates.
        seed_builtin_personas(&state).await;
        let after = state.personas.list_by_scope(&scope).await.unwrap();
        assert_eq!(after.len(), 4, "re-seeding must not duplicate personas");
    }

    /// Task plan: "If the persona already exists, do not overwrite" — a
    /// customized persona survives re-seeding untouched.
    #[tokio::test]
    async fn seed_builtin_personas_preserves_existing() {
        use gyre_domain::{Persona, PersonaApprovalStatus, PersonaScope};

        let state = mem::test_state();
        let tenant_id = Id::new("t-custom");
        // A user-customized security persona already exists at tenant scope.
        let mut custom = Persona::new(
            Id::new("custom-sec"),
            "Custom Security",
            "security",
            PersonaScope::Tenant(tenant_id.clone()),
            "hand-edited prompt",
            42,
        );
        custom.system_prompt = "hand-edited prompt".to_string();
        state.personas.create(&custom).await.unwrap();

        seed_builtin_personas_for_tenant(&state, &tenant_id).await;

        let scope = PersonaScope::Tenant(tenant_id.clone());
        let personas = state.personas.list_by_scope(&scope).await.unwrap();
        let sec: Vec<_> = personas.iter().filter(|p| p.slug == "security").collect();
        assert_eq!(sec.len(), 1, "no duplicate security persona");
        assert_eq!(sec[0].id, Id::new("custom-sec"));
        assert_eq!(sec[0].system_prompt, "hand-edited prompt");
        assert_eq!(sec[0].approval_status, PersonaApprovalStatus::Pending);
        // The other three were seeded around it.
        assert_eq!(personas.len(), 4);
    }

    /// Seeding for one tenant never leaks personas into another tenant.
    #[tokio::test]
    async fn seed_builtin_personas_scoped_to_own_tenant() {
        use gyre_domain::PersonaScope;

        let state = mem::test_state();
        let a = Id::new("t-a");
        let b = Id::new("t-b");
        seed_builtin_personas_for_tenant(&state, &a).await;
        seed_builtin_personas_for_tenant(&state, &b).await;

        let in_a = state
            .personas
            .list_by_scope(&PersonaScope::Tenant(a.clone()))
            .await
            .unwrap();
        let in_b = state
            .personas
            .list_by_scope(&PersonaScope::Tenant(b.clone()))
            .await
            .unwrap();
        assert_eq!(in_a.len(), 4);
        assert_eq!(in_b.len(), 4);
        for p in &in_a {
            assert_eq!(p.scope, PersonaScope::Tenant(a.clone()));
        }
    }

    /// task-197 / hierarchy-enforcement §1 Bootstrap Behavior: startup must
    /// materialize the default Tenant row on an empty database, with the
    /// spec field values, and be idempotent across restarts.
    ///
    /// Exercises the real SQLite `TenantRepository` against the `tenants`
    /// table (not the mem repo) so the assertions verify the durable row
    /// production startup writes, and so "not wired" fails visibly: without
    /// the bootstrap call, `find_by_id("default")` returns None.
    #[tokio::test]
    async fn bootstrap_default_tenant_materializes_row_and_is_idempotent() {
        use gyre_common::Id;
        use gyre_domain::budget::BudgetConfig;

        let tmp = tempfile::NamedTempFile::new().unwrap();
        let storage = gyre_adapters::SqliteStorage::new(tmp.path().to_str().unwrap()).unwrap();
        // Wire the SQLite adapter into state.tenants exactly as build_state
        // does in DB-backed mode (store! macro path).
        let mut base = (*mem::test_state()).clone();
        base.tenants = Arc::new(storage);
        let state = Arc::new(base);

        // Empty database: no default tenant yet.
        assert!(
            state
                .tenants
                .find_by_id(&Id::new("default"))
                .await
                .unwrap()
                .is_none(),
            "fresh DB must not contain a default tenant before bootstrap"
        );

        bootstrap_default_tenant(&state).await;

        let tenant = state
            .tenants
            .find_by_id(&Id::new("default"))
            .await
            .unwrap()
            .expect("default tenant materialized after bootstrap");
        assert_eq!(tenant.id, Id::new("default"));
        assert_eq!(tenant.name, "Default");
        assert_eq!(tenant.slug, "default");
        assert_eq!(tenant.oidc_issuer, None);
        assert_eq!(tenant.max_workspaces, None);
        assert!(tenant.created_at > 0);
        // BudgetConfig has no PartialEq — compare serialized form against
        // Some(BudgetConfig::default()) (all limits unset = unlimited).
        assert_eq!(
            serde_json::to_value(tenant.budget).unwrap(),
            serde_json::to_value(Some(BudgetConfig::default())).unwrap()
        );

        // Restart against the populated DB: exactly one default tenant remains.
        bootstrap_default_tenant(&state).await;
        let tenants = state.tenants.list().await.unwrap();
        assert_eq!(
            tenants.iter().filter(|t| t.slug == "default").count(),
            1,
            "second bootstrap call must not create a duplicate default tenant"
        );
    }

    // ── Scopeless-message guard (message-bus.md §Message Envelope) ──────────────
    //
    // The send_message() helper validation: a non-Broadcast message with
    // workspace_id: None must be rejected with Err + an error-level log record,
    // and never constructed, stored, or broadcast.

    /// tracing field visitor capturing an event's `message` field.
    struct CaptureVisit(String);
    impl tracing::field::Visit for CaptureVisit {
        fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
            if field.name() == "message" {
                self.0 = format!("{value:?}");
            }
        }
    }

    /// Capturing tracing layer: records (level, message) pairs while a scoped
    /// default subscriber is active.
    struct CaptureLayer(Arc<std::sync::Mutex<Vec<(tracing::Level, String)>>>);
    impl<S> tracing_subscriber::Layer<S> for CaptureLayer
    where
        S: tracing::Subscriber,
    {
        fn on_event(
            &self,
            event: &tracing::Event<'_>,
            _ctx: tracing_subscriber::layer::Context<'_, S>,
        ) {
            let mut visit = CaptureVisit(String::new());
            event.record(&mut visit);
            self.0
                .lock()
                .unwrap()
                .push((*event.metadata().level(), visit.0));
        }
    }

    /// MessageRepository wrapper counting store() calls, delegating to the mem
    /// adapter — proves whether the emitter reached the persistence port.
    struct CountingMessages {
        inner: mem::MemMessageRepository,
        stores: std::sync::atomic::AtomicUsize,
    }

    #[async_trait::async_trait]
    impl gyre_ports::MessageRepository for CountingMessages {
        async fn store(&self, message: &Message) -> anyhow::Result<()> {
            self.stores
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            self.inner.store(message).await
        }
        async fn find_by_id(&self, id: &Id) -> anyhow::Result<Option<Message>> {
            self.inner.find_by_id(id).await
        }
        async fn list_after(
            &self,
            agent_id: &Id,
            after_ts: u64,
            after_id: Option<&Id>,
            limit: usize,
        ) -> anyhow::Result<Vec<Message>> {
            self.inner
                .list_after(agent_id, after_ts, after_id, limit)
                .await
        }
        async fn list_unacked(&self, agent_id: &Id, limit: usize) -> anyhow::Result<Vec<Message>> {
            self.inner.list_unacked(agent_id, limit).await
        }
        async fn count_unacked(&self, agent_id: &Id) -> anyhow::Result<u64> {
            self.inner.count_unacked(agent_id).await
        }
        async fn acknowledge(&self, message_id: &Id, agent_id: &Id) -> anyhow::Result<()> {
            self.inner.acknowledge(message_id, agent_id).await
        }
        async fn acknowledge_all(&self, agent_id: &Id, reason: &str) -> anyhow::Result<u64> {
            self.inner.acknowledge_all(agent_id, reason).await
        }
        async fn list_by_workspace(
            &self,
            workspace_id: &Id,
            kind: Option<&str>,
            since: Option<u64>,
            before_ts: Option<u64>,
            before_id: Option<&Id>,
            limit: Option<usize>,
        ) -> anyhow::Result<Vec<Message>> {
            self.inner
                .list_by_workspace(workspace_id, kind, since, before_ts, before_id, limit)
                .await
        }
        async fn expire_events(&self, older_than: u64) -> anyhow::Result<u64> {
            self.inner.expire_events(older_than).await
        }
        async fn expire_acked_inboxes(&self, older_than: u64) -> anyhow::Result<u64> {
            self.inner.expire_acked_inboxes(older_than).await
        }
        async fn expire_for_agents(
            &self,
            agent_ids: &[Id],
            older_than: u64,
        ) -> anyhow::Result<u64> {
            self.inner.expire_for_agents(agent_ids, older_than).await
        }
    }

    #[tokio::test]
    async fn emit_scoped_event_rejects_scopeless_non_broadcast_message() {
        use gyre_common::message::Destination;
        use tracing_subscriber::prelude::*;

        let counting = Arc::new(CountingMessages {
            inner: mem::MemMessageRepository::default(),
            stores: std::sync::atomic::AtomicUsize::new(0),
        });
        let state = mem::test_state_with_messages(counting.clone());
        let mut bus_rx = state.message_broadcast_tx.subscribe();
        let mut dispatch_rx = state
            .take_message_dispatch_rx()
            .await
            .expect("dispatcher not yet taken");

        let logs = Arc::new(std::sync::Mutex::new(Vec::new()));
        let subscriber = tracing_subscriber::registry().with(CaptureLayer(logs.clone()));
        // current_thread test runtime: the whole future (and every await resume)
        // runs on this thread, so the thread-local default subscriber captures
        // everything the emitter logs.
        let _guard = tracing::subscriber::set_default(subscriber);

        // Workspace destination + workspace_id None → Err, error-level log,
        // nothing stored / broadcast / dispatched.
        let result = state
            .emit_scoped_event(
                Id::new("tenant-1"),
                None,
                Destination::Workspace(Id::new("ws-x")),
                MessageKind::TaskCreated,
                None,
            )
            .await;
        assert!(
            result.is_err(),
            "scopeless Workspace message must be rejected"
        );

        // Agent destination + workspace_id None → same rejection.
        let result = state
            .emit_scoped_event(
                Id::new("tenant-1"),
                None,
                Destination::Agent(Id::new("agent-x")),
                MessageKind::TaskAssignment,
                None,
            )
            .await;
        assert!(result.is_err(), "scopeless Agent message must be rejected");

        // Clone the captured records out so the MutexGuard is confined to
        // this statement — the test body contains later awaits, and holding
        // a lock guard across an await point is the clippy::await_holding_lock
        // defect class (a guard held while suspended can deadlock the
        // capturing layer on another thread).
        let captured: Vec<(tracing::Level, String)> = logs.lock().unwrap().clone();
        assert!(
            captured
                .iter()
                .any(|(level, msg)| *level == tracing::Level::ERROR
                    && msg.contains("workspace scope")),
            "the guard must log at error level, captured: {captured:?}"
        );

        assert_eq!(
            counting.stores.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "no message may reach store()"
        );
        assert!(
            bus_rx.try_recv().is_err(),
            "no message may reach the broadcast channel"
        );
        assert!(
            dispatch_rx.try_recv().is_err(),
            "no message may reach the dispatch consumer"
        );

        // Control 1 — Broadcast is the specced carve-out: workspace_id None is
        // valid for Broadcast (never stored, but broadcast + dispatched).
        let result = state
            .emit_scoped_event(
                Id::new("tenant-1"),
                None,
                Destination::Broadcast,
                MessageKind::TaskCreated,
                None,
            )
            .await;
        assert!(result.is_ok(), "Broadcast without workspace scope is valid");
        assert_eq!(counting.stores.load(std::sync::atomic::Ordering::SeqCst), 0);
        let bus_msg = bus_rx.try_recv().expect("Broadcast must be broadcast");
        assert!(matches!(bus_msg.to, Destination::Broadcast));
        let _ = dispatch_rx.try_recv();

        // Control 2 — a scoped (Some(workspace_id)) Workspace message passes the
        // guard, is stored once, and is broadcast.
        let result = state
            .emit_scoped_event(
                Id::new("tenant-1"),
                Some(Id::new("ws-x")),
                Destination::Workspace(Id::new("ws-x")),
                MessageKind::TaskCreated,
                None,
            )
            .await;
        assert!(result.is_ok(), "scoped message must pass the guard");
        assert_eq!(
            counting.stores.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "scoped message must be stored exactly once"
        );
        let bus_msg = bus_rx.try_recv().expect("scoped message must be broadcast");
        assert_eq!(bus_msg.workspace_id, Some(Id::new("ws-x")));
    }

    /// message-bus.md §Broadcast-only kinds: `DataSeeded` and `QueueUpdated`
    /// are storage-exempt — "never hitting `MessageRepository::store()`" —
    /// REGARDLESS of destination. The internal emitter (`emit_scoped_event`)
    /// must short-circuit storage for these kinds by kind, not only by
    /// destination: emitted to a Workspace destination, the message must
    /// still reach the broadcast channel while no row is persisted. Against
    /// the prior destination-only guard this test fails (the row is stored).
    #[tokio::test]
    async fn emit_event_broadcast_only_kind_never_stored_for_workspace_destination() {
        let state = mem::test_state();
        mem::ensure_test_workspace(&state, "ws-bo-emit").await;
        let mut rx = state.message_broadcast_tx.subscribe();

        state
            .emit_event(
                Id::new("ws-bo-emit"),
                Destination::Workspace(Id::new("ws-bo-emit")),
                MessageKind::DataSeeded,
                None,
            )
            .await;
        state
            .emit_event(
                Id::new("ws-bo-emit"),
                Destination::Workspace(Id::new("ws-bo-emit")),
                MessageKind::QueueUpdated,
                None,
            )
            .await;

        // Both reached the broadcast channel (subscribers see them).
        for expected in [MessageKind::DataSeeded, MessageKind::QueueUpdated] {
            let msg = rx
                .try_recv()
                .unwrap_or_else(|e| panic!("{expected:?} must reach broadcast channel: {e}"));
            assert_eq!(msg.kind, expected);
            assert!(matches!(msg.from, MessageOrigin::Server));
            // Storage exemption is kind-based: no row for either kind.
            assert!(
                state.messages.find_by_id(&msg.id).await.unwrap().is_none(),
                "{expected:?} must never hit MessageRepository::store()"
            );
        }

        // The workspace listing is empty — neither kind was persisted even
        // though the destination was a Workspace.
        let listed = state
            .messages
            .list_by_workspace(&Id::new("ws-bo-emit"), None, None, None, None, Some(100))
            .await
            .unwrap();
        assert!(
            listed.is_empty(),
            "broadcast-only kinds must never hit MessageRepository::store(), \
             even addressed to a Workspace destination"
        );
    }
}
