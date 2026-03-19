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
    fn auth_header_format() {
        let c = GyreClient::new("http://localhost:3333".to_string(), "mytoken".to_string());
        assert_eq!(c.auth_header(), "Bearer mytoken");
    }
}
