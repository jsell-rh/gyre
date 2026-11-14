//! HTTP REST client for the Gyre server API.

use anyhow::{Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};

pub struct GyreClient {
    base_url: String,
    token: String,
    client: Client,
}

// ── Response types (mirrors server response shapes) ───────────────────────────

#[derive(Deserialize, Debug, Clone)]
pub struct AgentResponse {
    pub id: String,
    pub name: String,
    pub status: String,
    #[serde(default)]
    pub current_task_id: Option<String>,
    #[serde(default)]
    pub last_heartbeat: Option<u64>,
    /// When the agent was spawned (UNIX seconds) — the recency signal for
    /// `gyre search --since` on agent results (agents have no updated_at).
    #[serde(default)]
    pub spawned_at: u64,
}

#[derive(Deserialize, Debug)]
pub struct RegisterAgentResponse {
    pub id: String,
    pub name: String,
    pub status: String,
    pub auth_token: String,
}

#[derive(Deserialize, Debug, Clone)]
#[allow(dead_code)]
pub struct TaskResponse {
    pub id: String,
    pub title: String,
    pub status: String,
    pub priority: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub assigned_to: Option<String>,
    #[serde(default)]
    pub labels: Vec<String>,
    /// Last-modified timestamp (UNIX seconds) — maintained by the server on
    /// task update and status transitions. Used by `gyre search --since`.
    #[serde(default)]
    pub updated_at: u64,
}

#[derive(Deserialize, Debug, Clone)]
#[allow(dead_code)]
pub struct MrResponse {
    pub id: String,
    pub repository_id: String,
    pub title: String,
    #[serde(default)]
    pub source_branch: String,
    #[serde(default)]
    pub target_branch: String,
    pub status: String,
    /// Last-modified timestamp (UNIX seconds) — maintained by the server on
    /// MR status transitions. Used by `gyre search --since`.
    #[serde(default)]
    pub updated_at: u64,
}

// ── Budget response types (mirror server api/budget.rs shapes) ────────────────

/// Budget limits for an entity (workspace or tenant). Mirrors
/// `gyre_domain::BudgetConfig` as serialized by GET/PUT
/// /api/v1/workspaces/:id/budget.
#[derive(Deserialize, Serialize, Debug, Clone, Default, PartialEq)]
pub struct BudgetConfig {
    pub max_tokens_per_day: Option<u64>,
    pub max_cost_per_day: Option<f64>,
    pub max_concurrent_agents: Option<u32>,
    pub max_agent_lifetime_secs: Option<u64>,
}

/// Real-time budget usage snapshot. Mirrors `gyre_domain::BudgetUsage`.
#[allow(dead_code)]
#[derive(Deserialize, Debug, Clone)]
pub struct BudgetUsage {
    pub entity_type: String,
    pub entity_id: String,
    pub tokens_used_today: u64,
    pub cost_today: f64,
    pub active_agents: u32,
    pub period_start: u64,
}

/// Response of GET/PUT /api/v1/workspaces/:id/budget — config + live usage.
#[derive(Deserialize, Debug, Clone)]
pub struct BudgetResponse {
    pub entity_type: String,
    pub entity_id: String,
    pub config: BudgetConfig,
    pub usage: BudgetUsage,
}

/// Request body of PUT /api/v1/workspaces/:id/budget. The server replaces
/// the whole config with this body (omitted fields become unset), so the CLI
/// fetch-merges before sending (see `gyre budget set` in main.rs).
#[derive(Deserialize, Serialize, Debug, Clone, Default, PartialEq)]
pub struct SetBudgetRequest {
    pub max_tokens_per_day: Option<u64>,
    pub max_cost_per_day: Option<f64>,
    pub max_concurrent_agents: Option<u32>,
    pub max_agent_lifetime_secs: Option<u64>,
}

/// Response of GET /api/v1/budget/summary (Admin only) — tenant config,
/// tenant usage, and per-workspace breakdown.
#[derive(Deserialize, Debug, Clone)]
pub struct TenantBudgetSummary {
    pub tenant_config: BudgetConfig,
    pub tenant_usage: BudgetUsage,
    pub workspaces: Vec<BudgetResponse>,
}

// ── Bootstrap response types (platform-model.md §8) ──────────────────────────

#[derive(Deserialize, Debug, Clone)]
#[allow(dead_code)]
pub struct TenantResponse {
    pub id: String,
    pub name: String,
    pub slug: String,
    #[serde(default)]
    pub oidc_issuer: Option<String>,
}

#[derive(Deserialize, Debug, Clone)]
#[allow(dead_code)]
pub struct WorkspaceResponse {
    pub id: String,
    pub name: String,
    pub slug: String,
}

#[derive(Deserialize, Debug, Clone)]
#[allow(dead_code)]
pub struct RepoResponse {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub clone_url: Option<String>,
    #[serde(default)]
    pub default_branch: Option<String>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct PersonaResponse {
    pub id: String,
    pub slug: String,
    /// "Pending" | "Approved" | "Deprecated" — the resume path reads it:
    /// a Pending persona from an interrupted run must be approved, not
    /// skipped (spawn 409s on it).
    pub approval_status: String,
}

#[derive(Deserialize, Debug, Clone)]
pub struct CreateUserResponse {
    pub user: CreateUserUser,
    pub api_key: CreatedApiKey,
}

/// Error from [`GyreClient::create_user`]: a 409 duplicate (the user
/// already exists — bootstrap's resume path) or any other failure. The
/// split exists so the resume logic cannot mistake a 403 (non-admin
/// token), a 400 (validation), or a network error for "already exists"
/// and silently adopt a saved credential.
#[derive(Debug)]
pub enum CreateUserError {
    /// HTTP 409: duplicate external_id or username. Carries the response
    /// body (names the conflicting user).
    Duplicate(String),
    Other(anyhow::Error),
}

impl std::fmt::Display for CreateUserError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CreateUserError::Duplicate(body) => write!(f, "user already exists: {body}"),
            CreateUserError::Other(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for CreateUserError {}

/// GET /api/v1/users/me — the principal the current token authenticates
/// as. `username` and `tenant_id` drive the bootstrap resume verification.
#[derive(Deserialize, Debug, Clone)]
pub struct CurrentUser {
    pub username: String,
    #[serde(default)]
    pub tenant_id: Option<String>,
}

/// One row of GET /api/v1/repos/:id/gates — the name is all the bootstrap
/// resume path needs (skip gates already configured for the repo).
#[derive(Deserialize, Debug, Clone)]
pub struct GateSummary {
    pub name: String,
}

#[derive(Deserialize, Debug, Clone)]
pub struct CreateUserUser {
    pub id: String,
    pub username: String,
    #[serde(default)]
    #[allow(dead_code)]
    pub tenant_id: Option<String>,
}

#[derive(Deserialize, Debug, Clone)]
#[allow(dead_code)]
pub struct CreatedApiKey {
    /// Plaintext key - returned exactly once.
    pub key: String,
    pub name: String,
}

#[derive(Deserialize, Debug, Clone)]
pub struct SpawnOrchestratorResponse {
    /// Flattened agent summary (id, name, orchestrator_type, ...).
    pub agent: SpawnOrchestratorAgent,
    /// Truthful process-launch outcome (task-099 F3): "running" or
    /// "launch_failed". A persisted agent row is not a running orchestrator.
    #[serde(default = "default_launch_status")]
    pub launch_status: String,
    /// Failure reason when launch_status == "launch_failed".
    #[serde(default)]
    pub launch_detail: Option<String>,
}

fn default_launch_status() -> String {
    "unknown".to_string()
}

#[derive(Deserialize, Debug, Clone)]
pub struct SpawnOrchestratorAgent {
    pub id: String,
    #[allow(dead_code)]
    pub name: String,
    #[serde(default)]
    #[allow(dead_code)]
    pub orchestrator_type: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    pub repo_id: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    pub restart_on_failure: Option<bool>,
    #[serde(default)]
    #[allow(dead_code)]
    pub persona_id: Option<String>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct SyncSpecsResponse {
    /// Number of ledger entries present after the sync.
    pub registered: usize,
    /// Default-branch HEAD the sync ran against.
    pub head_sha: String,
}

// ── Secret admin types (mirrors /api/v1/secrets, platform-model.md §7) ───────

/// Secret metadata as returned by the Admin API. The server never includes
/// the plaintext value in any response, so neither does this type.
#[derive(Deserialize, Debug, Clone)]
pub struct SecretResponse {
    pub id: String,
    pub name: String,
    pub scope: String,
    pub scope_id: String,
    pub secret_type: String,
    #[allow(dead_code)]
    pub created_by: String,
    pub created_at: u64,
    pub expires_at: Option<u64>,
    pub last_rotated_at: Option<u64>,
    #[allow(dead_code)]
    pub tenant_id: String,
}

/// POST /api/v1/secrets request body (value included here — it is write-only
/// and never appears in a response).
#[derive(Serialize, Debug, Clone)]
pub struct CreateSecretRequest {
    pub name: String,
    pub scope: String,
    pub scope_id: Option<String>,
    pub secret_type: String,
    pub value: String,
    pub expires_at: Option<u64>,
}

// ── ABAC policy types (mirrors /api/v1/policies, abac-policy-engine.md) ──────

/// One condition of a policy: `{attribute, operator, value}`. Mirrors the
/// server's ConditionRequest (POST /api/v1/policies). `value` is the raw
/// JSON side of the comparison (string, number, bool, list, or null).
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PolicyConditionRequest {
    pub attribute: String,
    pub operator: String,
    pub value: serde_json::Value,
}

/// POST /api/v1/policies request body. Mirrors the server's
/// CreatePolicyRequest field-for-field (Admin only server-side; reserved
/// `trust:`/`builtin:` name prefixes are rejected there).
#[derive(Serialize, Debug, Clone)]
pub struct CreatePolicyRequest {
    pub name: String,
    pub description: Option<String>,
    /// "tenant" | "workspace" | "repo"
    pub scope: String,
    /// Id of the scoped entity; None = applies to ALL entities at the scope.
    pub scope_id: Option<String>,
    pub priority: u32,
    /// "allow" | "deny"
    pub effect: String,
    pub conditions: Vec<PolicyConditionRequest>,
    pub actions: Vec<String>,
    pub resource_types: Vec<String>,
    pub enabled: Option<bool>,
}

/// A policy as returned by GET/POST /api/v1/policies. Mirrors
/// `gyre_domain::Policy` as serialized by the server (scope/effect are
/// snake_case strings; conditions and audit fields are present in the
/// payload but not needed for CLI display, so they are not modeled).
#[derive(Deserialize, Debug, Clone)]
pub struct PolicyResponse {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub scope: String,
    #[serde(default)]
    pub scope_id: Option<String>,
    pub priority: u32,
    pub effect: String,
    pub enabled: bool,
    #[serde(default)]
    pub built_in: bool,
}

/// POST /api/v1/policies/evaluate request body. Mirrors the server's
/// EvaluateRequest; the CLI fills the subject type/id and resource
/// type/id from `--subject user:jsell` / `--resource spec:identity`.
#[derive(Serialize, Debug)]
pub struct EvaluatePolicyRequest {
    pub subject: EvaluateSubject,
    pub action: String,
    pub resource: EvaluateResource,
}

#[derive(Serialize, Debug)]
pub struct EvaluateSubject {
    pub r#type: String,
    pub id: String,
}

#[derive(Serialize, Debug)]
pub struct EvaluateResource {
    pub r#type: String,
    pub id: Option<String>,
}

/// POST /api/v1/policies/evaluate response.
#[derive(Deserialize, Debug)]
pub struct EvaluatePolicyResponse {
    /// "allow" | "deny"
    pub decision: String,
    pub matched_policy: Option<String>,
    pub evaluated_policies: u32,
    pub evaluation_ms: f64,
}

/// One audit record from GET /api/v1/policies/decisions. Mirrors
/// `gyre_domain::PolicyDecision` as serialized by the server.
#[derive(Deserialize, Debug, Clone)]
pub struct PolicyDecisionResponse {
    #[allow(dead_code)]
    pub request_id: String,
    pub subject_id: String,
    pub subject_type: String,
    pub action: String,
    pub resource_type: String,
    #[serde(default)]
    pub resource_id: String,
    /// "allow" | "deny"
    pub decision: String,
    pub matched_policy: Option<String>,
    #[allow(dead_code)]
    pub evaluated_policies: u32,
    #[allow(dead_code)]
    pub evaluation_ms: f64,
    pub evaluated_at: u64,
}

/// One row of GET /api/v1/policies/effective: the decision for one sampled
/// action on the queried resource type.
#[derive(Deserialize, Debug, Clone)]
pub struct EffectivePermissionResponse {
    pub action: String,
    pub resource_type: String,
    /// "allow" | "deny"
    pub decision: String,
    pub matched_policy: Option<String>,
}

// ── Search response types (mirrors GET /api/v1/search) ───────────────────────

/// One search result row. `facets` mirrors the server's result facets
/// (status, priority, repo_id, ...).
#[derive(Deserialize, Debug, Clone)]
#[allow(dead_code)]
pub struct SearchResult {
    pub entity_type: String,
    pub entity_id: String,
    pub title: String,
    #[serde(default)]
    pub snippet: String,
    #[serde(default)]
    pub score: f64,
    #[serde(default)]
    pub facets: std::collections::HashMap<String, String>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct SearchResponse {
    pub query: String,
    pub total: usize,
    #[serde(default)]
    pub results: Vec<SearchResult>,
}

/// Percent-encode a spec path for use as a single URL path segment.
/// Encodes `/` as `%2F` so axum receives the full path in one `:path` param.
fn encode_spec_path(path: &str) -> String {
    path.chars()
        .map(|c| match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => c.to_string(),
            _ => format!("%{:02X}", c as u32),
        })
        .collect()
}

// ── Client implementation ─────────────────────────────────────────────────────

impl GyreClient {
    pub fn new(base_url: String, token: String) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            token,
            client: Client::new(),
        }
    }

    /// The bearer token this client authenticates with (needed when the
    /// bootstrap flow shells out to `git push` against the server).
    pub fn token(&self) -> &str {
        &self.token
    }

    fn auth_header(&self) -> String {
        format!("Bearer {}", self.token)
    }

    /// Register a new agent with the server; returns agent info + auth_token.
    pub async fn register_agent(&self, name: &str) -> Result<RegisterAgentResponse> {
        let body = serde_json::json!({ "name": name });
        let resp = self
            .client
            .post(format!("{}/api/v1/agents", self.base_url))
            .header("Authorization", self.auth_header())
            .json(&body)
            .send()
            .await
            .context("connecting to Gyre server")?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            anyhow::bail!("register agent failed (HTTP {status}): {text}");
        }
        serde_json::from_str(&text).context("parsing register response")
    }

    /// Fetch a single agent by ID.
    pub async fn get_agent(&self, agent_id: &str) -> Result<AgentResponse> {
        let resp = self
            .client
            .get(format!("{}/api/v1/agents/{agent_id}", self.base_url))
            .header("Authorization", self.auth_header())
            .send()
            .await
            .context("connecting to Gyre server")?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            anyhow::bail!("get agent failed (HTTP {status}): {text}");
        }
        serde_json::from_str(&text).context("parsing agent response")
    }

    /// GET /api/v1/tasks/:id — live task detail. Search index facets are
    /// frozen at create time, so `gyre search --status/--since` resolves
    /// current task state through this endpoint.
    pub async fn get_task(&self, task_id: &str) -> Result<TaskResponse> {
        let resp = self
            .client
            .get(format!("{}/api/v1/tasks/{task_id}", self.base_url))
            .header("Authorization", self.auth_header())
            .send()
            .await
            .context("connecting to Gyre server")?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            anyhow::bail!("get task failed (HTTP {status}): {text}");
        }
        serde_json::from_str(&text).context("parsing task response")
    }

    /// GET /api/v1/merge-requests/:id — live MR detail. See `get_task` for
    /// why `gyre search --status/--since` needs the live record.
    pub async fn get_mr(&self, mr_id: &str) -> Result<MrResponse> {
        let resp = self
            .client
            .get(format!("{}/api/v1/merge-requests/{mr_id}", self.base_url))
            .header("Authorization", self.auth_header())
            .send()
            .await
            .context("connecting to Gyre server")?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            anyhow::bail!("get MR failed (HTTP {status}): {text}");
        }
        serde_json::from_str(&text).context("parsing MR response")
    }

    /// List tasks with optional filters.
    pub async fn list_tasks(
        &self,
        status_filter: Option<&str>,
        assigned_to: Option<&str>,
    ) -> Result<Vec<TaskResponse>> {
        let mut req = self
            .client
            .get(format!("{}/api/v1/tasks", self.base_url))
            .header("Authorization", self.auth_header());
        if let Some(s) = status_filter {
            req = req.query(&[("status", s)]);
        }
        if let Some(a) = assigned_to {
            req = req.query(&[("assigned_to", a)]);
        }
        let resp = req.send().await.context("connecting to Gyre server")?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            anyhow::bail!("list tasks failed (HTTP {status}): {text}");
        }
        serde_json::from_str(&text).context("parsing tasks response")
    }

    /// Assign a task to an agent (updates `assigned_to` field).
    pub async fn assign_task(&self, task_id: &str, agent_id: &str) -> Result<TaskResponse> {
        let body = serde_json::json!({ "assigned_to": agent_id });
        let resp = self
            .client
            .put(format!("{}/api/v1/tasks/{task_id}", self.base_url))
            .header("Authorization", self.auth_header())
            .json(&body)
            .send()
            .await
            .context("connecting to Gyre server")?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            anyhow::bail!("assign task failed (HTTP {status}): {text}");
        }
        serde_json::from_str(&text).context("parsing task response")
    }

    /// Transition a task's status.
    pub async fn transition_task_status(
        &self,
        task_id: &str,
        new_status: &str,
    ) -> Result<TaskResponse> {
        let body = serde_json::json!({ "status": new_status });
        let resp = self
            .client
            .put(format!("{}/api/v1/tasks/{task_id}/status", self.base_url))
            .header("Authorization", self.auth_header())
            .json(&body)
            .send()
            .await
            .context("connecting to Gyre server")?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            anyhow::bail!("transition task status failed (HTTP {status}): {text}");
        }
        serde_json::from_str(&text).context("parsing task response")
    }

    /// Create a merge request.
    pub async fn create_mr(
        &self,
        repo_id: &str,
        title: &str,
        source_branch: &str,
        target_branch: &str,
        author_agent_id: Option<&str>,
    ) -> Result<MrResponse> {
        let mut body = serde_json::json!({
            "repository_id": repo_id,
            "title": title,
            "source_branch": source_branch,
            "target_branch": target_branch,
        });
        if let Some(id) = author_agent_id {
            body["author_agent_id"] = serde_json::Value::String(id.to_string());
        }
        let resp = self
            .client
            .post(format!("{}/api/v1/merge-requests", self.base_url))
            .header("Authorization", self.auth_header())
            .json(&body)
            .send()
            .await
            .context("connecting to Gyre server")?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            anyhow::bail!("create MR failed (HTTP {status}): {text}");
        }
        serde_json::from_str(&text).context("parsing MR response")
    }

    /// GET /api/v1/search — full-text search across all entities.
    ///
    /// `q` is passed to the server as-is (the server's current engine is
    /// AND-of-terms substring matching; the facet query language of
    /// search.md §Query Language is task-154). `entity_type` and
    /// `workspace_id` are real server-side filters. The CLI's
    /// `--status`/`--since` filters are applied client-side against live
    /// entity state (see `gyre search` in main.rs) because search results
    /// carry no timestamps and index facets freeze at create time.
    pub async fn search(
        &self,
        q: &str,
        entity_type: Option<&str>,
        workspace_id: Option<&str>,
        limit: usize,
    ) -> Result<SearchResponse> {
        let mut req = self
            .client
            .get(format!("{}/api/v1/search", self.base_url))
            .header("Authorization", self.auth_header());
        if !q.is_empty() {
            req = req.query(&[("q", q)]);
        }
        if let Some(t) = entity_type {
            req = req.query(&[("entity_type", t)]);
        }
        if let Some(w) = workspace_id {
            req = req.query(&[("workspace_id", w)]);
        }
        req = req.query(&[("limit", limit.to_string())]);
        let resp = req.send().await.context("connecting to Gyre server")?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            anyhow::bail!("search failed (HTTP {status}): {text}");
        }
        serde_json::from_str(&text).context("parsing search response")
    }

    /// GET /api/v1/workspaces — list all accessible workspaces.
    pub async fn list_workspaces(&self) -> Result<Vec<serde_json::Value>> {
        let resp = self
            .client
            .get(format!("{}/api/v1/workspaces", self.base_url))
            .header("Authorization", self.auth_header())
            .send()
            .await
            .context("connecting to Gyre server")?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            anyhow::bail!("list workspaces failed (HTTP {status}): {text}");
        }
        serde_json::from_str(&text).context("parsing workspaces response")
    }

    /// Resolve a workspace slug to its ID. Returns the first match.
    pub async fn resolve_workspace_slug(&self, slug: &str) -> Result<String> {
        let resp = self
            .client
            .get(format!("{}/api/v1/workspaces", self.base_url))
            .header("Authorization", self.auth_header())
            .query(&[("slug", slug)])
            .send()
            .await
            .context("connecting to Gyre server")?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            anyhow::bail!("list workspaces failed (HTTP {status}): {text}");
        }
        let workspaces: Vec<serde_json::Value> =
            serde_json::from_str(&text).context("parsing workspaces response")?;
        workspaces
            .first()
            .and_then(|w| w["id"].as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| anyhow::anyhow!("no workspace found with slug '{slug}'"))
    }

    /// GET /api/v1/workspaces/:workspace_id/briefing
    pub async fn get_briefing(
        &self,
        workspace_id: &str,
        since: Option<u64>,
    ) -> Result<serde_json::Value> {
        let mut req = self
            .client
            .get(format!(
                "{}/api/v1/workspaces/{workspace_id}/briefing",
                self.base_url
            ))
            .header("Authorization", self.auth_header());
        if let Some(epoch) = since {
            req = req.query(&[("since", epoch.to_string())]);
        }
        let resp = req.send().await.context("connecting to Gyre server")?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            anyhow::bail!("get briefing failed (HTTP {status}): {text}");
        }
        serde_json::from_str(&text).context("parsing briefing response")
    }

    /// GET /api/v1/users/me/notifications
    pub async fn get_notifications(
        &self,
        workspace_id: Option<&str>,
        min_priority: Option<u8>,
        max_priority: Option<u8>,
        notification_type: Option<&str>,
    ) -> Result<serde_json::Value> {
        let mut req = self
            .client
            .get(format!("{}/api/v1/users/me/notifications", self.base_url))
            .header("Authorization", self.auth_header());
        if let Some(ws) = workspace_id {
            req = req.query(&[("workspace_id", ws)]);
        }
        if let Some(min) = min_priority {
            req = req.query(&[("min_priority", min.to_string())]);
        }
        if let Some(max) = max_priority {
            req = req.query(&[("max_priority", max.to_string())]);
        }
        if let Some(nt) = notification_type {
            req = req.query(&[("notification_type", nt)]);
        }
        let resp = req.send().await.context("connecting to Gyre server")?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            anyhow::bail!("get notifications failed (HTTP {status}): {text}");
        }
        serde_json::from_str(&text).context("parsing notifications response")
    }

    /// POST /api/v1/notifications/:id/dismiss
    pub async fn dismiss_notification(&self, id: &str) -> Result<()> {
        let resp = self
            .client
            .post(format!(
                "{}/api/v1/notifications/{id}/dismiss",
                self.base_url
            ))
            .header("Authorization", self.auth_header())
            .send()
            .await
            .context("connecting to Gyre server")?;
        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await?;
            anyhow::bail!("dismiss notification failed (HTTP {status}): {text}");
        }
        Ok(())
    }

    /// POST /api/v1/notifications/:id/resolve
    pub async fn resolve_notification(&self, id: &str) -> Result<()> {
        let resp = self
            .client
            .post(format!(
                "{}/api/v1/notifications/{id}/resolve",
                self.base_url
            ))
            .header("Authorization", self.auth_header())
            .json(&serde_json::json!({}))
            .send()
            .await
            .context("connecting to Gyre server")?;
        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await?;
            anyhow::bail!("resolve notification failed (HTTP {status}): {text}");
        }
        Ok(())
    }

    /// POST /api/v1/secrets — create or update a secret (Admin role
    /// required server-side). The plaintext `value` is sent in the request
    /// and never appears in the response.
    pub async fn create_secret(&self, req: &CreateSecretRequest) -> Result<SecretResponse> {
        let resp = self
            .client
            .post(format!("{}/api/v1/secrets", self.base_url))
            .header("Authorization", self.auth_header())
            .json(req)
            .send()
            .await
            .context("connecting to Gyre server")?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            anyhow::bail!("create secret failed (HTTP {status}): {text}");
        }
        serde_json::from_str(&text).context("parsing create secret response")
    }

    /// GET /api/v1/secrets?scope=...&scope_id=... — list secret metadata
    /// for a scope. The server never returns values.
    pub async fn list_secrets(
        &self,
        scope: &str,
        scope_id: Option<&str>,
    ) -> Result<Vec<SecretResponse>> {
        let mut req = self
            .client
            .get(format!("{}/api/v1/secrets", self.base_url))
            .header("Authorization", self.auth_header())
            .query(&[("scope", scope)]);
        if let Some(sid) = scope_id {
            req = req.query(&[("scope_id", sid)]);
        }
        let resp = req.send().await.context("connecting to Gyre server")?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            anyhow::bail!("list secrets failed (HTTP {status}): {text}");
        }
        serde_json::from_str(&text).context("parsing secrets list response")
    }

    /// DELETE /api/v1/secrets/:id — delete a secret (204 on success).
    pub async fn delete_secret(&self, id: &str) -> Result<()> {
        let resp = self
            .client
            .delete(format!("{}/api/v1/secrets/{id}", self.base_url))
            .header("Authorization", self.auth_header())
            .send()
            .await
            .context("connecting to Gyre server")?;
        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await?;
            anyhow::bail!("delete secret failed (HTTP {status}): {text}");
        }
        Ok(())
    }

    /// POST /api/v1/secrets/:id/rotate — replace a secret's value and
    /// update `last_rotated_at`.
    pub async fn rotate_secret(
        &self,
        id: &str,
        new_value: &str,
        expires_at: Option<u64>,
    ) -> Result<SecretResponse> {
        let body = serde_json::json!({ "new_value": new_value, "expires_at": expires_at });
        let resp = self
            .client
            .post(format!("{}/api/v1/secrets/{id}/rotate", self.base_url))
            .header("Authorization", self.auth_header())
            .json(&body)
            .send()
            .await
            .context("connecting to Gyre server")?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            anyhow::bail!("rotate secret failed (HTTP {status}): {text}");
        }
        serde_json::from_str(&text).context("parsing rotate secret response")
    }

    /// GET /api/v1/policies[?scope=&scope_id=] — list ABAC policies.
    /// `scope` filters by level (tenant/workspace/repo); `scope_id`
    /// restricts to one entity at that level.
    pub async fn list_policies(
        &self,
        scope: Option<&str>,
        scope_id: Option<&str>,
    ) -> Result<Vec<PolicyResponse>> {
        let mut req = self
            .client
            .get(format!("{}/api/v1/policies", self.base_url))
            .header("Authorization", self.auth_header());
        if let Some(s) = scope {
            req = req.query(&[("scope", s)]);
        }
        if let Some(sid) = scope_id {
            req = req.query(&[("scope_id", sid)]);
        }
        let resp = req.send().await.context("connecting to Gyre server")?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            anyhow::bail!("list policies failed (HTTP {status}): {text}");
        }
        serde_json::from_str(&text).context("parsing policies list response")
    }

    /// POST /api/v1/policies — create a policy (Admin only server-side;
    /// reserved `trust:`/`builtin:` name prefixes rejected there with 400).
    pub async fn create_policy(&self, req: &CreatePolicyRequest) -> Result<PolicyResponse> {
        let resp = self
            .client
            .post(format!("{}/api/v1/policies", self.base_url))
            .header("Authorization", self.auth_header())
            .json(req)
            .send()
            .await
            .context("connecting to Gyre server")?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            anyhow::bail!("create policy failed (HTTP {status}): {text}");
        }
        serde_json::from_str(&text).context("parsing create policy response")
    }

    /// POST /api/v1/policies/evaluate — dry-run the live policy set for a
    /// hypothetical subject/action/resource.
    pub async fn evaluate_policy(
        &self,
        req: &EvaluatePolicyRequest,
    ) -> Result<EvaluatePolicyResponse> {
        let resp = self
            .client
            .post(format!("{}/api/v1/policies/evaluate", self.base_url))
            .header("Authorization", self.auth_header())
            .json(req)
            .send()
            .await
            .context("connecting to Gyre server")?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            anyhow::bail!("evaluate policy failed (HTTP {status}): {text}");
        }
        serde_json::from_str(&text).context("parsing evaluate policy response")
    }

    /// GET /api/v1/policies/decisions — the decision audit log, newest
    /// first. Filters (subject_id, resource_type, limit) are applied
    /// server-side; outcome/age filtering is the caller's.
    pub async fn list_policy_decisions(
        &self,
        subject_id: Option<&str>,
        resource_type: Option<&str>,
        limit: usize,
    ) -> Result<Vec<PolicyDecisionResponse>> {
        let mut req = self
            .client
            .get(format!("{}/api/v1/policies/decisions", self.base_url))
            .header("Authorization", self.auth_header())
            .query(&[("limit", limit)]);
        if let Some(sid) = subject_id {
            req = req.query(&[("subject_id", sid)]);
        }
        if let Some(rt) = resource_type {
            req = req.query(&[("resource_type", rt)]);
        }
        let resp = req.send().await.context("connecting to Gyre server")?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            anyhow::bail!("list policy decisions failed (HTTP {status}): {text}");
        }
        serde_json::from_str(&text).context("parsing policy decisions response")
    }

    /// GET /api/v1/policies/effective?subject=&resource_type= — the
    /// effective-permissions answer for a real subject (the server loads
    /// the subject's actual attributes from the stores).
    pub async fn effective_permissions(
        &self,
        subject: &str,
        resource_type: Option<&str>,
    ) -> Result<Vec<EffectivePermissionResponse>> {
        let mut req = self
            .client
            .get(format!("{}/api/v1/policies/effective", self.base_url))
            .header("Authorization", self.auth_header())
            .query(&[("subject", subject)]);
        if let Some(rt) = resource_type {
            req = req.query(&[("resource_type", rt)]);
        }
        let resp = req.send().await.context("connecting to Gyre server")?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            anyhow::bail!("effective permissions failed (HTTP {status}): {text}");
        }
        serde_json::from_str(&text).context("parsing effective permissions response")
    }

    /// GET /api/v1/repos/:id/graph/concept/:name or
    /// GET /api/v1/workspaces/:id/graph/concept/:name
    ///
    /// Concept view projection (realized-model.md §4): the concept is resolved
    /// from the repo's specs/manifest.yaml and the knowledge graph is projected
    /// through its include patterns. A concept not defined in the manifest
    /// 404s. (The `?concept=` substring filter on GET /graph is a distinct
    /// surface.)
    pub async fn get_graph_concept(
        &self,
        concept: &str,
        repo_id: Option<&str>,
        workspace_id: Option<&str>,
    ) -> Result<serde_json::Value> {
        // Percent-encode the concept for use as a URL path segment.
        // Concept names are typically identifiers (e.g., "UserRepository"), but
        // we encode spaces and special characters for safety.
        let encoded_concept: String = concept
            .chars()
            .map(|c| match c {
                'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => c.to_string(),
                _ => format!("%{:02X}", c as u32),
            })
            .collect();
        let url = if let Some(rid) = repo_id {
            format!(
                "{}/api/v1/repos/{rid}/graph/concept/{encoded_concept}",
                self.base_url
            )
        } else if let Some(wid) = workspace_id {
            format!(
                "{}/api/v1/workspaces/{wid}/graph/concept/{encoded_concept}",
                self.base_url
            )
        } else {
            anyhow::bail!("either repo_id or workspace_id must be provided for concept search");
        };
        let resp = self
            .client
            .get(&url)
            .header("Authorization", self.auth_header())
            .send()
            .await
            .context("connecting to Gyre server")?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            anyhow::bail!("get graph concept failed (HTTP {status}): {text}");
        }
        serde_json::from_str(&text).context("parsing graph response")
    }

    /// GET /api/v1/workspaces/:workspace_id/repos — list repos in a workspace.
    pub async fn list_workspace_repos(&self, workspace_id: &str) -> Result<Vec<serde_json::Value>> {
        let resp = self
            .client
            .get(format!(
                "{}/api/v1/workspaces/{workspace_id}/repos",
                self.base_url
            ))
            .header("Authorization", self.auth_header())
            .send()
            .await
            .context("connecting to Gyre server")?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            anyhow::bail!("list workspace repos failed (HTTP {status}): {text}");
        }
        serde_json::from_str(&text).context("parsing repos response")
    }

    /// Resolve a repo name to its ID within a workspace.
    /// Lists repos in the workspace and matches by name.
    pub async fn resolve_repo_name(&self, workspace_id: &str, repo_name: &str) -> Result<String> {
        let resp = self
            .client
            .get(format!(
                "{}/api/v1/workspaces/{workspace_id}/repos",
                self.base_url
            ))
            .header("Authorization", self.auth_header())
            .send()
            .await
            .context("connecting to Gyre server")?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            anyhow::bail!("list workspace repos failed (HTTP {status}): {text}");
        }
        let repos: Vec<serde_json::Value> =
            serde_json::from_str(&text).context("parsing repos response")?;
        repos
            .iter()
            .find(|r| r["name"].as_str() == Some(repo_name))
            .and_then(|r| r["id"].as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| anyhow::anyhow!("no repo found with name '{repo_name}' in workspace"))
    }

    /// GET /api/v1/workspaces/:id/budget — workspace budget limits + live usage.
    pub async fn get_workspace_budget(&self, workspace_id: &str) -> Result<BudgetResponse> {
        let resp = self
            .client
            .get(format!(
                "{}/api/v1/workspaces/{workspace_id}/budget",
                self.base_url
            ))
            .header("Authorization", self.auth_header())
            .send()
            .await
            .context("connecting to Gyre server")?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            anyhow::bail!("get budget failed (HTTP {status}): {text}");
        }
        serde_json::from_str(&text).context("parsing budget response")
    }

    /// PUT /api/v1/workspaces/:id/budget — set workspace budget limits
    /// (Admin only). The server replaces the whole config with this body and
    /// enforces the tenant cascade (HTTP 400 when a limit exceeds the tenant
    /// ceiling — the message is surfaced verbatim by the caller).
    pub async fn set_workspace_budget(
        &self,
        workspace_id: &str,
        req: &SetBudgetRequest,
    ) -> Result<BudgetResponse> {
        let resp = self
            .client
            .put(format!(
                "{}/api/v1/workspaces/{workspace_id}/budget",
                self.base_url
            ))
            .header("Authorization", self.auth_header())
            .json(req)
            .send()
            .await
            .context("connecting to Gyre server")?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            anyhow::bail!("set budget failed (HTTP {status}): {text}");
        }
        serde_json::from_str(&text).context("parsing budget response")
    }

    /// GET /api/v1/budget/summary — tenant-wide budget picture (Admin only).
    pub async fn budget_summary(&self) -> Result<TenantBudgetSummary> {
        let resp = self
            .client
            .get(format!("{}/api/v1/budget/summary", self.base_url))
            .header("Authorization", self.auth_header())
            .send()
            .await
            .context("connecting to Gyre server")?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            anyhow::bail!("budget summary failed (HTTP {status}): {text}");
        }
        serde_json::from_str(&text).context("parsing budget summary response")
    }

    /// GET /api/v1/merge-requests/:id/trace
    pub async fn get_mr_trace(&self, mr_id: &str) -> Result<serde_json::Value> {
        let resp = self
            .client
            .get(format!(
                "{}/api/v1/merge-requests/{mr_id}/trace",
                self.base_url
            ))
            .header("Authorization", self.auth_header())
            .send()
            .await
            .context("connecting to Gyre server")?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            anyhow::bail!("get MR trace failed (HTTP {status}): {text}");
        }
        serde_json::from_str(&text).context("parsing trace response")
    }

    /// POST /api/v1/repos/:repo_id/specs/assist (SSE stream → collected `{diff, explanation}` responses)
    pub async fn spec_assist(
        &self,
        repo_id: &str,
        spec_path: &str,
        instruction: &str,
    ) -> Result<Vec<serde_json::Value>> {
        let body = serde_json::json!({
            "spec_path": spec_path,
            "instruction": instruction,
        });
        let resp = self
            .client
            .post(format!(
                "{}/api/v1/repos/{repo_id}/specs/assist",
                self.base_url
            ))
            .header("Authorization", self.auth_header())
            .json(&body)
            .send()
            .await
            .context("connecting to Gyre server")?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            anyhow::bail!("spec assist failed (HTTP {status}): {text}");
        }
        // Parse SSE stream: track event type and extract data from "complete" and "error" events.
        // The stream contains "event: partial" (incremental text chunks),
        // "event: complete" (final response with {diff, explanation}), and
        // "event: error" (LLM validation failures with {error, ...}).
        let mut ops = Vec::new();
        let mut current_event = String::new();
        for line in text.lines() {
            if let Some(event_type) = line.strip_prefix("event: ") {
                current_event = event_type.trim().to_string();
            } else if let Some(data) = line.strip_prefix("data: ") {
                if data == "[DONE]" {
                    break;
                }
                if current_event == "complete" {
                    if let Ok(val) = serde_json::from_str::<serde_json::Value>(data) {
                        ops.push(val);
                    }
                } else if current_event == "error" {
                    // Capture error events so the display code can surface them.
                    if let Ok(val) = serde_json::from_str::<serde_json::Value>(data) {
                        ops.push(val);
                    }
                } else if current_event == "partial" {
                    // Print partial progress to stderr for user feedback
                    if let Ok(val) = serde_json::from_str::<serde_json::Value>(data) {
                        if let Some(text) = val.get("text").and_then(|t| t.as_str()) {
                            eprint!("{text}");
                        }
                    }
                }
            }
        }
        Ok(ops)
    }

    // ── Spec link endpoints ─────────────────────────────────────────────────

    /// GET /api/v1/specs/:path/links — outbound and inbound links for one spec.
    pub async fn get_spec_links(&self, spec_path: &str) -> Result<Vec<serde_json::Value>> {
        let encoded = encode_spec_path(spec_path);
        let resp = self
            .client
            .get(format!("{}/api/v1/specs/{encoded}/links", self.base_url))
            .header("Authorization", self.auth_header())
            .send()
            .await
            .context("connecting to Gyre server")?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            anyhow::bail!("get spec links failed (HTTP {status}): {text}");
        }
        serde_json::from_str(&text).context("parsing spec links response")
    }

    /// GET /api/v1/specs/:path/dependents — specs that depend on this one.
    pub async fn get_spec_dependents(&self, spec_path: &str) -> Result<Vec<serde_json::Value>> {
        let encoded = encode_spec_path(spec_path);
        let resp = self
            .client
            .get(format!(
                "{}/api/v1/specs/{encoded}/dependents",
                self.base_url
            ))
            .header("Authorization", self.auth_header())
            .send()
            .await
            .context("connecting to Gyre server")?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            anyhow::bail!("get spec dependents failed (HTTP {status}): {text}");
        }
        serde_json::from_str(&text).context("parsing spec dependents response")
    }

    /// GET /api/v1/specs/graph — full tenant-wide spec dependency graph.
    pub async fn get_spec_graph(&self) -> Result<serde_json::Value> {
        let resp = self
            .client
            .get(format!("{}/api/v1/specs/graph", self.base_url))
            .header("Authorization", self.auth_header())
            .send()
            .await
            .context("connecting to Gyre server")?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            anyhow::bail!("get spec graph failed (HTTP {status}): {text}");
        }
        serde_json::from_str(&text).context("parsing spec graph response")
    }

    /// GET /api/v1/specs/stale-links — all stale links across the tenant.
    pub async fn get_stale_spec_links(&self) -> Result<Vec<serde_json::Value>> {
        let resp = self
            .client
            .get(format!("{}/api/v1/specs/stale-links", self.base_url))
            .header("Authorization", self.auth_header())
            .send()
            .await
            .context("connecting to Gyre server")?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            anyhow::bail!("get stale spec links failed (HTTP {status}): {text}");
        }
        serde_json::from_str(&text).context("parsing stale spec links response")
    }

    /// GET /api/v1/specs/conflicts — all active conflicts.
    pub async fn get_spec_conflicts(&self) -> Result<Vec<serde_json::Value>> {
        let resp = self
            .client
            .get(format!("{}/api/v1/specs/conflicts", self.base_url))
            .header("Authorization", self.auth_header())
            .send()
            .await
            .context("connecting to Gyre server")?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            anyhow::bail!("get spec conflicts failed (HTTP {status}): {text}");
        }
        serde_json::from_str(&text).context("parsing spec conflicts response")
    }

    // ── Dependency graph endpoints ──────────────────────────────────────────

    /// GET /api/v1/repos/:id/dependencies — outgoing deps from this repo.
    pub async fn list_dependencies(&self, repo_id: &str) -> Result<Vec<serde_json::Value>> {
        let resp = self
            .client
            .get(format!(
                "{}/api/v1/repos/{repo_id}/dependencies",
                self.base_url
            ))
            .header("Authorization", self.auth_header())
            .send()
            .await
            .context("connecting to Gyre server")?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            anyhow::bail!("list dependencies failed (HTTP {status}): {text}");
        }
        serde_json::from_str(&text).context("parsing dependencies response")
    }

    /// GET /api/v1/repos/:id/dependents — repos that depend on this repo.
    pub async fn list_dependents(&self, repo_id: &str) -> Result<Vec<serde_json::Value>> {
        let resp = self
            .client
            .get(format!(
                "{}/api/v1/repos/{repo_id}/dependents",
                self.base_url
            ))
            .header("Authorization", self.auth_header())
            .send()
            .await
            .context("connecting to Gyre server")?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            anyhow::bail!("list dependents failed (HTTP {status}): {text}");
        }
        serde_json::from_str(&text).context("parsing dependents response")
    }

    /// GET /api/v1/dependencies/graph — tenant-wide dependency graph.
    pub async fn get_dependency_graph(&self) -> Result<serde_json::Value> {
        let resp = self
            .client
            .get(format!("{}/api/v1/dependencies/graph", self.base_url))
            .header("Authorization", self.auth_header())
            .send()
            .await
            .context("connecting to Gyre server")?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            anyhow::bail!("get dependency graph failed (HTTP {status}): {text}");
        }
        serde_json::from_str(&text).context("parsing graph response")
    }

    /// GET /api/v1/repos/:id/blast-radius — transitive dependents.
    pub async fn get_blast_radius(&self, repo_id: &str) -> Result<serde_json::Value> {
        let resp = self
            .client
            .get(format!(
                "{}/api/v1/repos/{repo_id}/blast-radius",
                self.base_url
            ))
            .header("Authorization", self.auth_header())
            .send()
            .await
            .context("connecting to Gyre server")?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            anyhow::bail!("get blast radius failed (HTTP {status}): {text}");
        }
        serde_json::from_str(&text).context("parsing blast radius response")
    }

    /// GET /api/v1/dependencies/stale — all stale dependency edges.
    pub async fn list_stale_dependencies(
        &self,
        workspace_id: Option<&str>,
    ) -> Result<Vec<serde_json::Value>> {
        let mut req = self
            .client
            .get(format!("{}/api/v1/dependencies/stale", self.base_url))
            .header("Authorization", self.auth_header());
        if let Some(ws) = workspace_id {
            req = req.query(&[("workspace_id", ws)]);
        }
        let resp = req.send().await.context("connecting to Gyre server")?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            anyhow::bail!("list stale dependencies failed (HTTP {status}): {text}");
        }
        serde_json::from_str(&text).context("parsing stale dependencies response")
    }

    /// GET /api/v1/dependencies/breaking — unacknowledged breaking changes.
    pub async fn list_breaking_changes(&self) -> Result<Vec<serde_json::Value>> {
        let resp = self
            .client
            .get(format!("{}/api/v1/dependencies/breaking", self.base_url))
            .header("Authorization", self.auth_header())
            .send()
            .await
            .context("connecting to Gyre server")?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            anyhow::bail!("list breaking changes failed (HTTP {status}): {text}");
        }
        serde_json::from_str(&text).context("parsing breaking changes response")
    }

    /// POST /api/v1/repos/:id/dependencies — add a manual dependency.
    pub async fn add_dependency(
        &self,
        repo_id: &str,
        target_repo_id: &str,
        dep_type: &str,
    ) -> Result<serde_json::Value> {
        let body = serde_json::json!({
            "target_repo_id": target_repo_id,
            "dependency_type": dep_type,
        });
        let resp = self
            .client
            .post(format!(
                "{}/api/v1/repos/{repo_id}/dependencies",
                self.base_url
            ))
            .header("Authorization", self.auth_header())
            .json(&body)
            .send()
            .await
            .context("connecting to Gyre server")?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            anyhow::bail!("add dependency failed (HTTP {status}): {text}");
        }
        serde_json::from_str(&text).context("parsing add dependency response")
    }

    /// POST /api/v1/dependencies/breaking/:id/acknowledge
    pub async fn acknowledge_breaking_change(&self, id: &str) -> Result<()> {
        let resp = self
            .client
            .post(format!(
                "{}/api/v1/dependencies/breaking/{id}/acknowledge",
                self.base_url
            ))
            .header("Authorization", self.auth_header())
            .json(&serde_json::json!({}))
            .send()
            .await
            .context("connecting to Gyre server")?;
        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await?;
            anyhow::bail!("acknowledge breaking change failed (HTTP {status}): {text}");
        }
        Ok(())
    }

    /// Call POST /api/v1/release/prepare and return the JSON response.
    pub async fn release_prepare(
        &self,
        repo_id: &str,
        branch: Option<&str>,
        from: Option<&str>,
        create_mr: bool,
    ) -> Result<serde_json::Value> {
        let body = serde_json::json!({
            "repo_id": repo_id,
            "branch": branch,
            "from": from,
            "create_mr": create_mr,
        });
        let resp = self
            .client
            .post(format!("{}/api/v1/release/prepare", self.base_url))
            .header("Authorization", self.auth_header())
            .json(&body)
            .send()
            .await
            .context("connecting to Gyre server")?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            anyhow::bail!("release prepare failed (HTTP {status}): {text}");
        }
        serde_json::from_str(&text).context("parsing release prepare response")
    }

    // ── Bootstrap (platform-model.md §8) ─────────────────────────────────────

    /// GET /health - no auth; verifies the server is up before bootstrap.
    pub async fn health(&self) -> Result<serde_json::Value> {
        let resp = self
            .client
            .get(format!("{}/health", self.base_url))
            .send()
            .await
            .context("connecting to Gyre server")?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            anyhow::bail!("health check failed (HTTP {status}): {text}");
        }
        serde_json::from_str(&text).context("parsing health response")
    }

    /// POST /api/v1/tenants - create the bootstrap tenant.
    pub async fn create_tenant(
        &self,
        name: &str,
        slug: &str,
        oidc_issuer: Option<&str>,
    ) -> Result<TenantResponse> {
        let body = serde_json::json!({
            "name": name,
            "slug": slug,
            "oidc_issuer": oidc_issuer,
        });
        let resp = self
            .client
            .post(format!("{}/api/v1/tenants", self.base_url))
            .header("Authorization", self.auth_header())
            .json(&body)
            .send()
            .await
            .context("connecting to Gyre server")?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            anyhow::bail!("create tenant failed (HTTP {status}): {text}");
        }
        serde_json::from_str(&text).context("parsing tenant response")
    }

    /// GET /api/v1/tenants - find an existing tenant by slug (resume path:
    /// bootstrap re-runs skip creation when the tenant already exists).
    pub async fn find_tenant_by_slug(&self, slug: &str) -> Result<Option<TenantResponse>> {
        let resp = self
            .client
            .get(format!("{}/api/v1/tenants", self.base_url))
            .header("Authorization", self.auth_header())
            .send()
            .await
            .context("connecting to Gyre server")?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            anyhow::bail!("list tenants failed (HTTP {status}): {text}");
        }
        let tenants: Vec<TenantResponse> =
            serde_json::from_str(&text).context("parsing tenant list")?;
        Ok(tenants.into_iter().find(|t| t.slug == slug))
    }

    /// GET /api/v1/workspaces?tenant_id=... - find an existing workspace by
    /// name within a tenant (resume path for bootstrap re-runs).
    pub async fn find_workspace_by_name(
        &self,
        tenant_id: &str,
        name: &str,
    ) -> Result<Option<WorkspaceResponse>> {
        let resp = self
            .client
            .get(format!(
                "{}/api/v1/workspaces?tenant_id={}",
                self.base_url, tenant_id
            ))
            .header("Authorization", self.auth_header())
            .send()
            .await
            .context("connecting to Gyre server")?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            anyhow::bail!("list workspaces failed (HTTP {status}): {text}");
        }
        let workspaces: Vec<WorkspaceResponse> =
            serde_json::from_str(&text).context("parsing workspace list")?;
        Ok(workspaces.into_iter().find(|w| w.name == name))
    }

    /// GET /api/v1/repos?workspace_id=... - find an existing repo by name
    /// within a workspace (resume path for bootstrap re-runs).
    pub async fn find_repo_by_name(
        &self,
        workspace_id: &str,
        name: &str,
    ) -> Result<Option<RepoResponse>> {
        let resp = self
            .client
            .get(format!(
                "{}/api/v1/repos?workspace_id={}",
                self.base_url, workspace_id
            ))
            .header("Authorization", self.auth_header())
            .send()
            .await
            .context("connecting to Gyre server")?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            anyhow::bail!("list repos failed (HTTP {status}): {text}");
        }
        let repos: Vec<RepoResponse> = serde_json::from_str(&text).context("parsing repo list")?;
        Ok(repos.into_iter().find(|r| r.name == name))
    }

    /// POST /api/v1/repos/:id/sync-specs - re-run the spec-ledger sync
    /// against the repo's default-branch HEAD (bootstrap step 6 registers
    /// the specs in the platform registry at first-run time).
    pub async fn sync_specs(&self, repo_id: &str) -> Result<SyncSpecsResponse> {
        let resp = self
            .client
            .post(format!(
                "{}/api/v1/repos/{repo_id}/sync-specs",
                self.base_url
            ))
            .header("Authorization", self.auth_header())
            .send()
            .await
            .context("connecting to Gyre server")?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            anyhow::bail!("spec sync failed (HTTP {status}): {text}");
        }
        serde_json::from_str(&text).context("parsing spec sync response")
    }

    /// GET /api/v1/personas?scope=tenant&scope_id=<tenant> - find an existing
    /// persona by slug within the bootstrap tenant (resume path: bootstrap
    /// re-runs skip re-registering built-in personas).
    ///
    /// The scope filter is mandatory, not cosmetic: the unscoped endpoint
    /// lists personas across ALL tenants, so an unscoped lookup makes the
    /// second tenant's bootstrap skip persona registration ("already
    /// registered") and then fail at spawn with a persona 404 (round-3
    /// finding 3 — cross-tenant resume defect).
    pub async fn find_persona_by_slug_in_tenant(
        &self,
        slug: &str,
        tenant_id: &str,
    ) -> Result<Option<PersonaResponse>> {
        let resp = self
            .client
            .get(format!(
                "{}/api/v1/personas?scope=tenant&scope_id={tenant_id}",
                self.base_url
            ))
            .header("Authorization", self.auth_header())
            .send()
            .await
            .context("connecting to Gyre server")?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            anyhow::bail!("list personas failed (HTTP {status}): {text}");
        }
        let personas: Vec<PersonaResponse> =
            serde_json::from_str(&text).context("parsing persona list")?;
        Ok(personas.into_iter().find(|p| p.slug == slug))
    }

    /// POST /api/v1/users - create the admin user bound to a tenant; the
    /// response contains the API key exactly once. `tenant_id` is REQUIRED
    /// by the server (task-099 F1: the user is scoped at creation).
    ///
    /// A 409 is reported as [`CreateUserError::Duplicate`] (the user already
    /// exists — bootstrap's resume path); every other failure is
    /// [`CreateUserError::Other`] so the caller cannot mistake a 403 or a
    /// validation error for "already exists".
    pub async fn create_user(
        &self,
        username: &str,
        tenant_id: &str,
    ) -> Result<CreateUserResponse, CreateUserError> {
        let body = serde_json::json!({ "username": username, "tenant_id": tenant_id });
        let resp = self
            .client
            .post(format!("{}/api/v1/users", self.base_url))
            .header("Authorization", self.auth_header())
            .json(&body)
            .send()
            .await
            .context("connecting to Gyre server")
            .map_err(CreateUserError::Other)?;
        let status = resp.status();
        let text = resp
            .text()
            .await
            .context("reading create user response")
            .map_err(CreateUserError::Other)?;
        if status == reqwest::StatusCode::CONFLICT {
            return Err(CreateUserError::Duplicate(text));
        }
        if !status.is_success() {
            return Err(CreateUserError::Other(anyhow::anyhow!(
                "create user failed (HTTP {status}): {text}"
            )));
        }
        serde_json::from_str(&text)
            .context("parsing create user response")
            .map_err(CreateUserError::Other)
    }

    /// GET /api/v1/users/me - resolve the caller the token authenticates
    /// as. Used by the bootstrap resume path to VERIFY a saved credential
    /// before adopting it: a matching server URL in ~/.gyre/config says
    /// nothing about which principal the token belongs to (the config is
    /// also written by `gyre register` with agent tokens).
    pub async fn me(&self) -> Result<CurrentUser> {
        let resp = self
            .client
            .get(format!("{}/api/v1/users/me", self.base_url))
            .header("Authorization", self.auth_header())
            .send()
            .await
            .context("connecting to Gyre server")?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            anyhow::bail!("whoami failed (HTTP {status}): {text}");
        }
        serde_json::from_str(&text).context("parsing whoami response")
    }

    /// GET /api/v1/repos/:id/gates - list the repo's configured quality
    /// gates (bootstrap resume path: step 7 skips names already configured
    /// so a re-run cannot duplicate the gate set).
    pub async fn list_gates(&self, repo_id: &str) -> Result<Vec<GateSummary>> {
        let resp = self
            .client
            .get(format!("{}/api/v1/repos/{repo_id}/gates", self.base_url))
            .header("Authorization", self.auth_header())
            .send()
            .await
            .context("connecting to Gyre server")?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            anyhow::bail!("list gates failed (HTTP {status}): {text}");
        }
        serde_json::from_str(&text).context("parsing gate list")
    }

    /// POST /api/v1/workspaces - create a workspace under a tenant.
    pub async fn create_workspace(&self, tenant_id: &str, name: &str) -> Result<WorkspaceResponse> {
        let body = serde_json::json!({
            "tenant_id": tenant_id,
            "name": name,
        });
        let resp = self
            .client
            .post(format!("{}/api/v1/workspaces", self.base_url))
            .header("Authorization", self.auth_header())
            .json(&body)
            .send()
            .await
            .context("connecting to Gyre server")?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            anyhow::bail!("create workspace failed (HTTP {status}): {text}");
        }
        serde_json::from_str(&text).context("parsing workspace response")
    }

    /// POST /api/v1/repos - register a repo in a workspace.
    pub async fn create_repo(&self, workspace_id: &str, name: &str) -> Result<RepoResponse> {
        let body = serde_json::json!({
            "workspace_id": workspace_id,
            "name": name,
        });
        let resp = self
            .client
            .post(format!("{}/api/v1/repos", self.base_url))
            .header("Authorization", self.auth_header())
            .json(&body)
            .send()
            .await
            .context("connecting to Gyre server")?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            anyhow::bail!("create repo failed (HTTP {status}): {text}");
        }
        serde_json::from_str(&text).context("parsing repo response")
    }

    /// POST /api/v1/personas - register a built-in persona.
    pub async fn create_persona(
        &self,
        name: &str,
        slug: &str,
        tenant_id: &str,
        system_prompt: &str,
        capabilities: &[&str],
        protocols: &[&str],
    ) -> Result<PersonaResponse> {
        let body = serde_json::json!({
            "name": name,
            "slug": slug,
            "scope": { "kind": "Tenant", "id": tenant_id },
            "system_prompt": system_prompt,
            "capabilities": capabilities,
            "protocols": protocols,
        });
        let resp = self
            .client
            .post(format!("{}/api/v1/personas", self.base_url))
            .header("Authorization", self.auth_header())
            .json(&body)
            .send()
            .await
            .context("connecting to Gyre server")?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            anyhow::bail!("create persona failed (HTTP {status}): {text}");
        }
        serde_json::from_str(&text).context("parsing persona response")
    }

    /// POST /api/v1/personas/:id/approve - pre-approve a registered persona.
    pub async fn approve_persona(&self, persona_id: &str) -> Result<()> {
        let resp = self
            .client
            .post(format!(
                "{}/api/v1/personas/{persona_id}/approve",
                self.base_url
            ))
            .header("Authorization", self.auth_header())
            .json(&serde_json::json!({}))
            .send()
            .await
            .context("connecting to Gyre server")?;
        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await?;
            anyhow::bail!("approve persona failed (HTTP {status}): {text}");
        }
        Ok(())
    }

    /// POST /api/v1/repos/:id/gates - configure a default quality gate.
    pub async fn create_gate(
        &self,
        repo_id: &str,
        name: &str,
        gate_type: &str,
        command: &str,
    ) -> Result<serde_json::Value> {
        let body = serde_json::json!({
            "name": name,
            "gate_type": gate_type,
            "command": command,
        });
        let resp = self
            .client
            .post(format!("{}/api/v1/repos/{repo_id}/gates", self.base_url))
            .header("Authorization", self.auth_header())
            .json(&body)
            .send()
            .await
            .context("connecting to Gyre server")?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            anyhow::bail!("create gate failed (HTTP {status}): {text}");
        }
        serde_json::from_str(&text).context("parsing gate response")
    }

    /// POST /api/v1/repos/:id/orchestrator/spawn - spawn the repo-tier
    /// orchestrator for the bootstrapped repo (platform-model.md §8 step 8,
    /// endpoint from task-093). 409 when a live one already exists.
    pub async fn spawn_repo_orchestrator(
        &self,
        repo_id: &str,
        name: Option<&str>,
    ) -> Result<SpawnRepoOrchestratorOutcome> {
        let mut body = serde_json::json!({});
        if let Some(n) = name {
            body["name"] = serde_json::Value::String(n.to_string());
        }
        let resp = self
            .client
            .post(format!(
                "{}/api/v1/repos/{repo_id}/orchestrator/spawn",
                self.base_url
            ))
            .header("Authorization", self.auth_header())
            .json(&body)
            .send()
            .await
            .context("connecting to Gyre server")?;
        let status = resp.status();
        let text = resp.text().await?;
        if status == reqwest::StatusCode::CONFLICT {
            // Two distinct 409s: a live orchestrator already holds the
            // repo slot (resume path — keep it), OR the repo-orchestrator
            // persona resolves but is not Approved. The latter is NOT a
            // resume success: no orchestrator exists, and pretending one
            // is active reports fake completion (the server's message
            // names the persona and its status; the caller surfaces it).
            if text.contains("approve it before spawning") {
                return Ok(SpawnRepoOrchestratorOutcome::PersonaPending(text));
            }
            return Ok(SpawnRepoOrchestratorOutcome::AlreadyLive);
        }
        if status == reqwest::StatusCode::BAD_REQUEST && text.contains("already exists") {
            // Name collision with a LIVE agent outside this repo's scope
            // (stopped predecessors are released server-side). The caller
            // retries with a distinct name; not a resume blocker.
            return Ok(SpawnRepoOrchestratorOutcome::NameTaken);
        }
        if !status.is_success() {
            anyhow::bail!("spawn repo orchestrator failed (HTTP {status}): {text}");
        }
        let parsed: SpawnOrchestratorResponse =
            serde_json::from_str(&text).context("parsing repo orchestrator response")?;
        Ok(SpawnRepoOrchestratorOutcome::Spawned(parsed))
    }

    /// GET /api/v1/repos/:id/status — main health + merge queue state.
    pub async fn repo_status(&self, repo_id: &str) -> Result<serde_json::Value> {
        let resp = self
            .client
            .get(format!("{}/api/v1/repos/{repo_id}/status", self.base_url))
            .header("Authorization", self.auth_header())
            .send()
            .await
            .context("connecting to Gyre server")?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            anyhow::bail!("repo status failed (HTTP {status}): {text}");
        }
        serde_json::from_str(&text).context("parsing repo status response")
    }

    /// PUT /api/v1/repos/:id/queue/pause — manually pause the merge queue.
    pub async fn pause_queue(&self, repo_id: &str, reason: Option<&str>) -> Result<()> {
        let body = serde_json::json!({ "reason": reason });
        let resp = self
            .client
            .put(format!(
                "{}/api/v1/repos/{repo_id}/queue/pause",
                self.base_url
            ))
            .header("Authorization", self.auth_header())
            .json(&body)
            .send()
            .await
            .context("connecting to Gyre server")?;
        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await?;
            anyhow::bail!("pause queue failed (HTTP {status}): {text}");
        }
        Ok(())
    }

    /// PUT /api/v1/repos/:id/queue/resume — manually resume the merge queue.
    pub async fn resume_queue(&self, repo_id: &str) -> Result<()> {
        let resp = self
            .client
            .put(format!(
                "{}/api/v1/repos/{repo_id}/queue/resume",
                self.base_url
            ))
            .header("Authorization", self.auth_header())
            .send()
            .await
            .context("connecting to Gyre server")?;
        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await?;
            anyhow::bail!("resume queue failed (HTTP {status}): {text}");
        }
        Ok(())
    }

    /// POST /api/v1/repos/:id/revert/:mr_id — manual revert of a merged MR.
    pub async fn revert_mr(&self, repo_id: &str, mr_id: &str) -> Result<serde_json::Value> {
        let resp = self
            .client
            .post(format!(
                "{}/api/v1/repos/{repo_id}/revert/{mr_id}",
                self.base_url
            ))
            .header("Authorization", self.auth_header())
            .send()
            .await
            .context("connecting to Gyre server")?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            anyhow::bail!("revert MR failed (HTTP {status}): {text}");
        }
        serde_json::from_str(&text).context("parsing revert response")
    }
}

/// Outcome of `spawn_repo_orchestrator`: fresh spawn, a live orchestrator
/// already exists for the repo (HTTP 409 "already active" — bootstrap
/// resume path), the requested name is held by a LIVE agent elsewhere
/// (HTTP 400 name collision — agent names are unique server-wide, so a
/// second tenant bootstrapping a same-named repo hits this; the caller
/// retries with a suffixed name), or the repo-orchestrator persona
/// resolves but is Pending (HTTP 409 with the persona detail — a
/// bootstrap interrupted between persona creation and approval hits
/// this; it is a failure, not a resume success).
pub enum SpawnRepoOrchestratorOutcome {
    Spawned(SpawnOrchestratorResponse),
    AlreadyLive,
    NameTaken,
    /// The repo-orchestrator persona exists but is not Approved; carries
    /// the server's conflict body verbatim.
    PersonaPending(String),
}

/// Test-only helpers for spinning up real loopback HTTP servers that
/// capture the exact request line, headers, and body the client emits.
/// Shared by the client tests here and the command-flow tests in main.rs.
#[cfg(test)]
pub(crate) mod test_http {
    use std::io::{Read, Write};
    use std::sync::{Arc, Mutex};

    #[derive(Debug, Clone)]
    pub(crate) struct CapturedRequest {
        pub(crate) request_line: String,
        pub(crate) headers: String,
        pub(crate) body: String,
    }

    fn find_double_crlf(buf: &[u8]) -> Option<usize> {
        buf.windows(4).position(|w| w == b"\r\n\r\n")
    }

    /// Read one full HTTP request (headers + Content-Length body) from the
    /// stream.
    fn read_request(stream: &mut std::net::TcpStream) -> CapturedRequest {
        let mut buf = Vec::new();
        let mut tmp = [0u8; 4096];
        loop {
            let n = stream.read(&mut tmp).unwrap();
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&tmp[..n]);
            if let Some(header_end) = find_double_crlf(&buf) {
                let headers = String::from_utf8_lossy(&buf[..header_end]).to_string();
                let content_length = headers
                    .lines()
                    .find_map(|l| {
                        let lower = l.to_ascii_lowercase();
                        lower
                            .strip_prefix("content-length:")
                            .and_then(|v| v.trim().parse::<usize>().ok())
                    })
                    .unwrap_or(0);
                if buf.len() >= header_end + 4 + content_length {
                    break;
                }
            }
        }
        let header_end = find_double_crlf(&buf).unwrap_or(buf.len());
        let headers = String::from_utf8_lossy(&buf[..header_end]).to_string();
        let request_line = headers.lines().next().unwrap_or("").to_string();
        let body = String::from_utf8_lossy(&buf[header_end + 4..]).to_string();
        CapturedRequest {
            request_line,
            headers,
            body,
        }
    }

    fn write_response(stream: &mut std::net::TcpStream, status: &str, reply_body: &str) {
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply_body}",
            reply_body.len()
        );
        stream.write_all(response.as_bytes()).unwrap();
        stream.flush().unwrap();
    }

    /// Spawn a one-shot HTTP server on 127.0.0.1:0. Serves exactly one
    /// request, captures it, and replies with `status_line` + `response_body`.
    pub(crate) fn spawn_mock_http_server(
        status_line: &str,
        response_body: &str,
    ) -> (String, std::sync::mpsc::Receiver<CapturedRequest>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let status = status_line.to_string();
        let reply_body = response_body.to_string();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let req = read_request(&mut stream);
            let _ = tx.send(req);
            write_response(&mut stream, &status, &reply_body);
        });
        (format!("http://{addr}"), rx)
    }

    /// Spawn a sequential HTTP server on 127.0.0.1:0. Serves one request
    /// per entry of `responses` (in order), capturing every request into a
    /// shared vec readable while the server runs. Fewer requests than
    /// scripted responses is fine (the server simply keeps waiting), so a
    /// test can assert that rejected input produced no requests at all.
    pub(crate) fn spawn_mock_http_server_sequence(
        responses: Vec<(String, String)>,
    ) -> (String, Arc<Mutex<Vec<CapturedRequest>>>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let captured: Arc<Mutex<Vec<CapturedRequest>>> = Arc::new(Mutex::new(Vec::new()));
        let sink = captured.clone();
        std::thread::spawn(move || {
            for (status, reply_body) in responses {
                let Ok((mut stream, _)) = listener.accept() else {
                    return;
                };
                let req = read_request(&mut stream);
                sink.lock().unwrap().push(req);
                write_response(&mut stream, &status, &reply_body);
            }
        });
        (format!("http://{addr}"), captured)
    }
}

#[cfg(test)]
mod tests {
    use super::test_http::spawn_mock_http_server;
    use super::*;

    #[test]
    fn client_trims_trailing_slash() {
        let c = GyreClient::new("http://localhost:3333/".to_string(), "tok".to_string());
        assert_eq!(c.base_url, "http://localhost:3333");
    }

    #[test]
    fn spawn_orchestrator_response_parses() {
        // Exact shape from POST /api/v1/repos/:id/orchestrator/spawn after
        // the server-side fix: flattened AgentResponse (with repo_id left
        // None to avoid a duplicate key) + orchestrator fields + token.
        let body = r#"{
            "agent": {
                "id": "0eea0f49-8faa-4b16-a020-6ec24913c197",
                "name": "probe3",
                "status": "active",
                "parent_id": null,
                "current_task_id": null,
                "spawned_at": 1790770653,
                "last_heartbeat": null,
                "workspace_id": "ws",
                "orchestrator_type": "repo_orchestrator",
                "repo_id": "repo",
                "restart_on_failure": true
            },
            "token": "jwt"
        }"#;
        let parsed: SpawnOrchestratorResponse = serde_json::from_str(body).unwrap();
        assert_eq!(parsed.agent.id, "0eea0f49-8faa-4b16-a020-6ec24913c197");
        assert_eq!(
            parsed.agent.orchestrator_type.as_deref(),
            Some("repo_orchestrator")
        );
        assert_eq!(parsed.agent.repo_id.as_deref(), Some("repo"));
        assert_eq!(parsed.agent.restart_on_failure, Some(true));
    }
    #[test]
    fn auth_header_format() {
        let c = GyreClient::new("http://localhost:3333".to_string(), "mytoken".to_string());
        assert_eq!(c.auth_header(), "Bearer mytoken");
    }

    // ── Bootstrap resume classification tests ─────────────────────────
    //
    // The resume path must not mistake one server signal for another:
    // a 409 from the orchestrator spawn endpoint is either "a live
    // orchestrator already holds the repo slot" (resume success) or "the
    // repo-orchestrator persona is Pending" (NO orchestrator exists —
    // reporting success there was fake completion), and a create_user
    // failure is only resumable when it is a 409 duplicate (a 403 from a
    // non-admin token is not "already exists").

    #[tokio::test]
    async fn spawn_repo_orchestrator_pending_persona_409_is_not_already_live() {
        let (base, _rx) = spawn_mock_http_server(
            "409 Conflict",
            "persona 'repo-orchestrator' resolves in scope Tenant(\"t-1\") but is Pending; \
             approve it before spawning its orchestrator",
        );
        let api = GyreClient::new(base, "tok".to_string());
        let outcome = api
            .spawn_repo_orchestrator("r-1", Some("main-orchestrator"))
            .await
            .unwrap();
        assert!(
            matches!(outcome, SpawnRepoOrchestratorOutcome::PersonaPending(_)),
            "a Pending-persona 409 must not be classified as AlreadyLive \
             (no orchestrator exists — that was the fake-completion bug)"
        );
    }

    #[tokio::test]
    async fn spawn_repo_orchestrator_live_slot_409_is_already_live() {
        let (base, _rx) = spawn_mock_http_server(
            "409 Conflict",
            "a repo orchestrator is already active for repo r-1",
        );
        let api = GyreClient::new(base, "tok".to_string());
        let outcome = api
            .spawn_repo_orchestrator("r-1", Some("main-orchestrator"))
            .await
            .unwrap();
        assert!(matches!(outcome, SpawnRepoOrchestratorOutcome::AlreadyLive));
    }

    #[tokio::test]
    async fn create_user_classifies_409_as_duplicate() {
        let (base, _rx) = spawn_mock_http_server(
            "409 Conflict",
            "user with external id local:jsell already exists (id u-1)",
        );
        let api = GyreClient::new(base, "tok".to_string());
        let err = api.create_user("jsell", "t-1").await.unwrap_err();
        match err {
            CreateUserError::Duplicate(body) => {
                assert!(body.contains("already exists"), "body: {body}")
            }
            other => panic!("409 must classify as Duplicate, got: {other}"),
        }
    }

    #[tokio::test]
    async fn create_user_non_duplicate_error_is_not_resumable() {
        // A 403 (non-admin token) is NOT "already exists": the resume
        // path must fail the run, not adopt a saved credential.
        let (base, _rx) =
            spawn_mock_http_server("403 Forbidden", "only Admin role may create users");
        let api = GyreClient::new(base, "tok".to_string());
        let err = api.create_user("jsell", "t-1").await.unwrap_err();
        match err {
            CreateUserError::Other(e) => {
                let msg = format!("{e:#}");
                assert!(
                    msg.contains("HTTP 403") && msg.contains("only Admin role"),
                    "error should carry the server detail: {msg}"
                );
            }
            CreateUserError::Duplicate(_) => {
                panic!("403 must not classify as Duplicate")
            }
        }
    }

    #[test]
    fn encode_spec_path_simple() {
        assert_eq!(encode_spec_path("identity.md"), "identity.md");
    }

    #[test]
    fn encode_spec_path_with_slash() {
        assert_eq!(
            encode_spec_path("system/identity-security.md"),
            "system%2Fidentity-security.md"
        );
    }

    #[test]
    fn encode_spec_path_with_spaces() {
        assert_eq!(
            encode_spec_path("my spec/file name.md"),
            "my%20spec%2Ffile%20name.md"
        );
    }

    #[test]
    fn encode_spec_path_preserves_unreserved() {
        assert_eq!(encode_spec_path("a-b_c.d~e"), "a-b_c.d~e");
    }

    // ── Budget wiring tests ────────────────────────────────────────────────
    //
    // A one-shot mock HTTP server (see `test_http` above) captures the exact
    // request line, headers, and body the client methods emit, so these tests
    // fail if the method, URL, or serialized body drifts from the real
    // endpoints registered in gyre-server api/mod.rs
    // (GET/PUT /api/v1/workspaces/:id/budget,
    // GET /api/v1/budget/summary).

    const BUDGET_RESPONSE_JSON: &str = r#"{
        "entity_type": "workspace",
        "entity_id": "ws-123",
        "config": {
            "max_tokens_per_day": 500000,
            "max_cost_per_day": 100.0,
            "max_concurrent_agents": 10,
            "max_agent_lifetime_secs": 3600
        },
        "usage": {
            "entity_type": "workspace",
            "entity_id": "ws-123",
            "tokens_used_today": 123456,
            "cost_today": 12.5,
            "active_agents": 3,
            "period_start": 1790000000
        }
    }"#;

    #[tokio::test]
    async fn get_workspace_budget_hits_real_endpoint() {
        let (base, rx) = spawn_mock_http_server("200 OK", BUDGET_RESPONSE_JSON);
        let api = GyreClient::new(base, "tok".to_string());
        let budget = api.get_workspace_budget("ws-123").await.unwrap();
        let req = rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
        assert_eq!(
            req.request_line,
            "GET /api/v1/workspaces/ws-123/budget HTTP/1.1"
        );
        assert!(req
            .headers
            .to_ascii_lowercase()
            .contains("authorization: bearer tok"));
        // Response must parse into the server's BudgetResponse shape.
        assert_eq!(budget.config.max_tokens_per_day, Some(500000));
        assert_eq!(budget.config.max_cost_per_day, Some(100.0));
        assert_eq!(budget.usage.tokens_used_today, 123456);
        assert_eq!(budget.usage.cost_today, 12.5);
        assert_eq!(budget.usage.active_agents, 3);
    }

    #[tokio::test]
    async fn set_workspace_budget_puts_exact_url_and_body() {
        let (base, rx) = spawn_mock_http_server("200 OK", BUDGET_RESPONSE_JSON);
        let api = GyreClient::new(base, "tok".to_string());
        let req_body = SetBudgetRequest {
            max_tokens_per_day: Some(500000),
            max_cost_per_day: Some(100.0),
            max_concurrent_agents: Some(10),
            max_agent_lifetime_secs: Some(3600),
        };
        let budget = api.set_workspace_budget("ws-123", &req_body).await.unwrap();
        let req = rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
        assert_eq!(
            req.request_line,
            "PUT /api/v1/workspaces/ws-123/budget HTTP/1.1"
        );
        assert!(req
            .headers
            .to_ascii_lowercase()
            .contains("authorization: bearer tok"));
        // Exact serialized body of SetBudgetRequest (server api/budget.rs shape).
        assert_eq!(
            req.body,
            "{\"max_tokens_per_day\":500000,\"max_cost_per_day\":100.0,\"max_concurrent_agents\":10,\"max_agent_lifetime_secs\":3600}"
        );
        assert_eq!(budget.entity_id, "ws-123");
    }

    #[tokio::test]
    async fn set_workspace_budget_surfaces_server_error_verbatim() {
        // Tenant-cascade violation: the server returns 400 with the reason in
        // the body; the CLI must surface it verbatim.
        let (base, rx) = spawn_mock_http_server(
            "400 Bad Request",
            "{\"error\":\"workspace max_tokens_per_day (999) exceeds tenant limit (500)\"}",
        );
        let api = GyreClient::new(base, "tok".to_string());
        let req_body = SetBudgetRequest {
            max_tokens_per_day: Some(999),
            ..Default::default()
        };
        let err = api
            .set_workspace_budget("ws-123", &req_body)
            .await
            .unwrap_err();
        let msg = format!("{err:#}");
        assert!(
            msg.contains("HTTP 400"),
            "error should carry the status: {msg}"
        );
        assert!(
            msg.contains("workspace max_tokens_per_day (999) exceeds tenant limit (500)"),
            "error should surface the server message verbatim: {msg}"
        );
        let _ = rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
    }

    #[tokio::test]
    async fn budget_summary_hits_real_endpoint() {
        let summary_json = r#"{
            "tenant_config": {
                "max_tokens_per_day": 1000000,
                "max_cost_per_day": 500.0,
                "max_concurrent_agents": 50,
                "max_agent_lifetime_secs": null
            },
            "tenant_usage": {
                "entity_type": "tenant",
                "entity_id": "global",
                "tokens_used_today": 200000,
                "cost_today": 42.0,
                "active_agents": 7,
                "period_start": 1790000000
            },
            "workspaces": [
                {
                    "entity_type": "workspace",
                    "entity_id": "ws-1",
                    "config": {
                        "max_tokens_per_day": 500000,
                        "max_cost_per_day": 100.0,
                        "max_concurrent_agents": 10,
                        "max_agent_lifetime_secs": 3600
                    },
                    "usage": {
                        "entity_type": "workspace",
                        "entity_id": "ws-1",
                        "tokens_used_today": 123456,
                        "cost_today": 12.5,
                        "active_agents": 3,
                        "period_start": 1790000000
                    }
                }
            ]
        }"#;
        let (base, rx) = spawn_mock_http_server("200 OK", summary_json);
        let api = GyreClient::new(base, "tok".to_string());
        let summary = api.budget_summary().await.unwrap();
        let req = rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
        assert_eq!(req.request_line, "GET /api/v1/budget/summary HTTP/1.1");
        assert!(req
            .headers
            .to_ascii_lowercase()
            .contains("authorization: bearer tok"));
        assert_eq!(summary.tenant_config.max_tokens_per_day, Some(1000000));
        assert_eq!(summary.tenant_usage.active_agents, 7);
        assert_eq!(summary.workspaces.len(), 1);
        assert_eq!(summary.workspaces[0].entity_id, "ws-1");
        assert_eq!(
            summary.workspaces[0].config.max_agent_lifetime_secs,
            Some(3600)
        );
    }
}
