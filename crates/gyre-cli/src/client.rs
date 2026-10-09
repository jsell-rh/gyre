//! HTTP REST client for the Gyre server API.

use anyhow::{Context, Result};
use reqwest::Client;
use serde::Deserialize;

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
    pub current_task_id: Option<String>,
    pub last_heartbeat: Option<u64>,
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
    pub description: Option<String>,
    pub assigned_to: Option<String>,
    pub labels: Vec<String>,
}

#[derive(Deserialize, Debug)]
#[allow(dead_code)]
pub struct MrResponse {
    pub id: String,
    pub repository_id: String,
    pub title: String,
    pub source_branch: String,
    pub target_branch: String,
    pub status: String,
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
#[allow(dead_code)]
pub struct PersonaResponse {
    pub id: String,
    pub slug: String,
    pub approval_status: String,
}

#[derive(Deserialize, Debug, Clone)]
pub struct CreateUserResponse {
    pub user: CreateUserUser,
    pub api_key: CreatedApiKey,
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

// ── Helpers ───────────────────────────────────────────────────────────────────

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

    /// GET /api/v1/repos/:id/graph/concept/:name or
    /// GET /api/v1/workspaces/:id/graph/concept/:name
    ///
    /// Uses the dedicated concept search endpoints which filter by name pattern
    /// server-side (the generic /graph endpoints ignore the concept query param).
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

    /// GET /api/v1/personas - find an existing persona by slug (resume path:
    /// bootstrap re-runs skip re-registering built-in personas).
    pub async fn find_persona_by_slug(&self, slug: &str) -> Result<Option<PersonaResponse>> {
        let resp = self
            .client
            .get(format!("{}/api/v1/personas", self.base_url))
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
    pub async fn create_user(&self, username: &str, tenant_id: &str) -> Result<CreateUserResponse> {
        let body = serde_json::json!({ "username": username, "tenant_id": tenant_id });
        let resp = self
            .client
            .post(format!("{}/api/v1/users", self.base_url))
            .header("Authorization", self.auth_header())
            .json(&body)
            .send()
            .await
            .context("connecting to Gyre server")?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            anyhow::bail!("create user failed (HTTP {status}): {text}");
        }
        serde_json::from_str(&text).context("parsing create user response")
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
            return Ok(SpawnRepoOrchestratorOutcome::AlreadyLive);
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

/// Outcome of `spawn_repo_orchestrator`: fresh spawn, or a live orchestrator
/// already exists for the repo (HTTP 409 — bootstrap resume path).
pub enum SpawnRepoOrchestratorOutcome {
    Spawned(SpawnOrchestratorResponse),
    AlreadyLive,
}

#[cfg(test)]
mod tests {
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
        assert_eq!(parsed.agent.orchestrator_type.as_deref(), Some("repo_orchestrator"));
        assert_eq!(parsed.agent.repo_id.as_deref(), Some("repo"));
        assert_eq!(parsed.agent.restart_on_failure, Some(true));
    }
    #[test]
    fn auth_header_format() {
        let c = GyreClient::new("http://localhost:3333".to_string(), "mytoken".to_string());
        assert_eq!(c.auth_header(), "Bearer mytoken");
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
}
