//! REST API integration tests — every endpoint exercised.
//!
//! Starts a real gyre-server on a random port and drives it via reqwest.
//! Each test function is independent: it spins up its own server so tests
//! can run fully in parallel without shared state.
//!
//! Coverage:
//!   - /health, /healthz, /readyz, /metrics, /api/v1/version (public)
//!   - Projects CRUD
//!   - Repos CRUD
//!   - Agents (create, get, list, status, heartbeat, messages, logs)
//!   - Tasks CRUD + status transitions
//!   - Merge Requests (create, get, list, status, comments, reviews, diff)
//!   - Merge Queue (enqueue, list, cancel)
//!   - Quality Gates + Push Gates
//!   - Analytics + Costs
//!   - Audit events
//!   - Admin endpoints (health, jobs, seed, kill, reassign, export, retention)
//!   - SIEM targets
//!   - Compute Targets
//!   - Network peers
//!   - A2A discover + agent card
//!   - Agent Compose
//!   - Worktrees
//!   - Agent commit tracking
//!   - Stack attestation (M14.1/M14.2)
//!   - Speculative merge (M13.5)
//!   - Code awareness (blame, hot-files, review-routing)
//!   - Release automation (release/prepare)
//!   - Spec approvals (approve, list, revoke)
//!   - Auth: missing/invalid token → 401

use gyre_server::{build_router, build_state};
use serde_json::json;
use std::sync::Arc;

// ── Test server helper ────────────────────────────────────────────────────────

const TOKEN: &str = "api-integration-token";

struct Ctx {
    client: reqwest::Client,
    base: String,
}

impl Ctx {
    async fn new() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let base_url = format!("http://127.0.0.1:{port}");

        let state = build_state(TOKEN, &base_url, None);
        let app = build_router(Arc::clone(&state));
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        Self {
            client: reqwest::Client::new(),
            base: base_url,
        }
    }

    // ── Authenticated helpers ─────────────────────────────────────────────────

    async fn get(&self, path: &str) -> reqwest::Response {
        self.client
            .get(format!("{}{path}", self.base))
            .header("Authorization", format!("Bearer {TOKEN}"))
            .send()
            .await
            .unwrap()
    }

    async fn post(&self, path: &str, body: serde_json::Value) -> reqwest::Response {
        self.client
            .post(format!("{}{path}", self.base))
            .header("Authorization", format!("Bearer {TOKEN}"))
            .json(&body)
            .send()
            .await
            .unwrap()
    }

    async fn put(&self, path: &str, body: serde_json::Value) -> reqwest::Response {
        self.client
            .put(format!("{}{path}", self.base))
            .header("Authorization", format!("Bearer {TOKEN}"))
            .json(&body)
            .send()
            .await
            .unwrap()
    }

    async fn delete(&self, path: &str) -> reqwest::Response {
        self.client
            .delete(format!("{}{path}", self.base))
            .header("Authorization", format!("Bearer {TOKEN}"))
            .send()
            .await
            .unwrap()
    }

    // ── No-auth helpers (for 401 tests) ──────────────────────────────────────

    async fn get_no_auth(&self, path: &str) -> reqwest::Response {
        self.client
            .get(format!("{}{path}", self.base))
            .send()
            .await
            .unwrap()
    }

    async fn post_no_auth(&self, path: &str, body: serde_json::Value) -> reqwest::Response {
        self.client
            .post(format!("{}{path}", self.base))
            .json(&body)
            .send()
            .await
            .unwrap()
    }

    // ── JSON body helpers ─────────────────────────────────────────────────────

    async fn get_json(&self, path: &str) -> serde_json::Value {
        self.get(path).await.json().await.unwrap()
    }

    async fn post_json(&self, path: &str, body: serde_json::Value) -> serde_json::Value {
        self.post(path, body).await.json().await.unwrap()
    }

    async fn put_json(&self, path: &str, body: serde_json::Value) -> serde_json::Value {
        self.put(path, body).await.json().await.unwrap()
    }
}

// ── 1. Public / Infrastructure endpoints ─────────────────────────────────────

#[tokio::test]
async fn health_returns_ok() {
    let ctx = Ctx::new().await;
    let resp = ctx.get("/health").await;
    assert_eq!(resp.status(), 200);
    let j: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(j["status"], "ok");
}

#[tokio::test]
async fn healthz_returns_ok() {
    let ctx = Ctx::new().await;
    let resp = ctx.get("/healthz").await;
    assert_eq!(resp.status(), 200);
    let j: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(j["status"], "ok");
}

#[tokio::test]
async fn readyz_returns_ok() {
    let ctx = Ctx::new().await;
    let resp = ctx.get("/readyz").await;
    assert_eq!(resp.status(), 200);
    let j: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(j["status"], "ok");
    assert!(j["checks"].is_object());
}

#[tokio::test]
async fn metrics_endpoint_returns_text() {
    let ctx = Ctx::new().await;
    let resp = ctx.get("/metrics").await;
    assert_eq!(resp.status(), 200);
}

#[tokio::test]
async fn version_is_public() {
    let ctx = Ctx::new().await;
    // No auth header required for /api/v1/version
    let resp = ctx.get_no_auth("/api/v1/version").await;
    assert_eq!(resp.status(), 200);
    let j: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(j["name"], "gyre");
}

// ── 2. Auth enforcement ───────────────────────────────────────────────────────

#[tokio::test]
async fn missing_token_returns_401() {
    let ctx = Ctx::new().await;
    let resp = ctx.get_no_auth("/api/v1/projects").await;
    assert_eq!(resp.status(), 401);
}

#[tokio::test]
async fn invalid_token_returns_401() {
    let ctx = Ctx::new().await;
    let resp = ctx
        .client
        .get(format!("{}/api/v1/projects", ctx.base))
        .header("Authorization", "Bearer wrong-token")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);
}

#[tokio::test]
async fn post_without_token_returns_401() {
    let ctx = Ctx::new().await;
    let resp = ctx
        .post_no_auth("/api/v1/projects", json!({"name": "x"}))
        .await;
    assert_eq!(resp.status(), 401);
}

// ── 3. Projects CRUD ──────────────────────────────────────────────────────────

#[tokio::test]
async fn projects_crud() {
    let ctx = Ctx::new().await;

    // Create
    let resp = ctx
        .post(
            "/api/v1/projects",
            json!({"name": "proj-a", "description": "test"}),
        )
        .await;
    assert_eq!(resp.status(), 201);
    let project: serde_json::Value = resp.json().await.unwrap();
    let project_id = project["id"].as_str().unwrap().to_string();
    assert_eq!(project["name"], "proj-a");

    // Get
    let got = ctx
        .get_json(&format!("/api/v1/projects/{project_id}"))
        .await;
    assert_eq!(got["id"], project_id);
    assert_eq!(got["name"], "proj-a");

    // List
    let list = ctx.get_json("/api/v1/projects").await;
    let arr = list.as_array().unwrap();
    assert!(arr.iter().any(|p| p["id"] == project_id));

    // Update
    let updated = ctx
        .put_json(
            &format!("/api/v1/projects/{project_id}"),
            json!({"name": "proj-a-renamed", "description": "updated"}),
        )
        .await;
    assert_eq!(updated["name"], "proj-a-renamed");

    // Delete
    let del_resp = ctx.delete(&format!("/api/v1/projects/{project_id}")).await;
    assert!(del_resp.status().is_success());

    // Get after delete → 404
    let resp404 = ctx.get(&format!("/api/v1/projects/{project_id}")).await;
    assert_eq!(resp404.status(), 404);
}

#[tokio::test]
async fn get_nonexistent_project_returns_404() {
    let ctx = Ctx::new().await;
    let resp = ctx.get("/api/v1/projects/no-such-id").await;
    assert_eq!(resp.status(), 404);
}

// ── 4. Repos CRUD ─────────────────────────────────────────────────────────────

async fn create_project(ctx: &Ctx) -> String {
    let j = ctx
        .post_json(
            "/api/v1/projects",
            json!({"name": "test-project", "description": ""}),
        )
        .await;
    j["id"].as_str().unwrap().to_string()
}

async fn create_repo(ctx: &Ctx, project_id: &str) -> String {
    let j = ctx
        .post_json(
            "/api/v1/repos",
            json!({"project_id": project_id, "name": "test-repo"}),
        )
        .await;
    j["id"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn repos_create_and_get() {
    let ctx = Ctx::new().await;
    let proj_id = create_project(&ctx).await;

    let resp = ctx
        .post(
            "/api/v1/repos",
            json!({"project_id": proj_id, "name": "myrepo"}),
        )
        .await;
    assert_eq!(resp.status(), 201);
    let repo: serde_json::Value = resp.json().await.unwrap();
    let repo_id = repo["id"].as_str().unwrap().to_string();
    assert_eq!(repo["name"], "myrepo");

    let got = ctx.get_json(&format!("/api/v1/repos/{repo_id}")).await;
    assert_eq!(got["id"], repo_id);
}

#[tokio::test]
async fn repos_list_by_project() {
    let ctx = Ctx::new().await;
    let proj_id = create_project(&ctx).await;
    create_repo(&ctx, &proj_id).await;

    let list = ctx
        .get_json(&format!("/api/v1/repos?project_id={proj_id}"))
        .await;
    let arr = list.as_array().unwrap();
    assert!(!arr.is_empty());
}

#[tokio::test]
async fn repos_branches_empty_on_new_repo() {
    let ctx = Ctx::new().await;
    let proj_id = create_project(&ctx).await;
    let repo_id = create_repo(&ctx, &proj_id).await;

    // Branches list: empty (no real git repo on disk)
    let resp = ctx.get(&format!("/api/v1/repos/{repo_id}/branches")).await;
    // Either 200 with empty array or 404/500 — we just check it doesn't 401
    assert_ne!(resp.status(), 401_u16);
}

#[tokio::test]
async fn repos_get_nonexistent_returns_404() {
    let ctx = Ctx::new().await;
    let resp = ctx.get("/api/v1/repos/nonexistent-id").await;
    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn repos_push_gates_get_and_set() {
    let ctx = Ctx::new().await;
    let proj_id = create_project(&ctx).await;
    let repo_id = create_repo(&ctx, &proj_id).await;

    // Get (should return empty or defaults)
    let resp = ctx
        .get(&format!("/api/v1/repos/{repo_id}/push-gates"))
        .await;
    assert!(resp.status().is_success());

    // Set a push gate (names are hyphenated: "conventional-commit", "task-ref", "no-em-dash")
    let put_resp = ctx
        .put(
            &format!("/api/v1/repos/{repo_id}/push-gates"),
            json!({"gates": ["conventional-commit"]}),
        )
        .await;
    assert!(put_resp.status().is_success());
}

// ── 5. Agents ─────────────────────────────────────────────────────────────────

async fn create_agent(ctx: &Ctx) -> String {
    let j = ctx
        .post_json(
            "/api/v1/agents",
            json!({"name": "test-agent", "capabilities": ["rust"]}),
        )
        .await;
    j["id"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn agents_create_and_get() {
    let ctx = Ctx::new().await;

    let resp = ctx
        .post(
            "/api/v1/agents",
            json!({"name": "worker-1", "capabilities": ["rust"]}),
        )
        .await;
    assert_eq!(resp.status(), 201);
    let agent: serde_json::Value = resp.json().await.unwrap();
    let agent_id = agent["id"].as_str().unwrap().to_string();
    assert_eq!(agent["name"], "worker-1");
    assert!(agent["auth_token"].is_string());

    let got = ctx.get_json(&format!("/api/v1/agents/{agent_id}")).await;
    assert_eq!(got["id"], agent_id);
    assert_eq!(got["name"], "worker-1");
}

#[tokio::test]
async fn agents_list() {
    let ctx = Ctx::new().await;
    create_agent(&ctx).await;

    let list = ctx.get_json("/api/v1/agents").await;
    let arr = list.as_array().unwrap();
    assert!(!arr.is_empty());
}

#[tokio::test]
async fn agent_status_update() {
    let ctx = Ctx::new().await;
    let agent_id = create_agent(&ctx).await;

    let resp = ctx
        .put(
            &format!("/api/v1/agents/{agent_id}/status"),
            json!({"status": "active"}),
        )
        .await;
    assert!(resp.status().is_success());
}

#[tokio::test]
async fn agent_heartbeat() {
    let ctx = Ctx::new().await;
    let agent_id = create_agent(&ctx).await;

    let resp = ctx
        .put(&format!("/api/v1/agents/{agent_id}/heartbeat"), json!({}))
        .await;
    assert!(resp.status().is_success());
}

#[tokio::test]
async fn agent_messages_send_and_receive() {
    let ctx = Ctx::new().await;
    let agent_id = create_agent(&ctx).await;

    // Send a message to the agent (fields: from, content, message_type)
    let resp = ctx
        .post(
            &format!("/api/v1/agents/{agent_id}/messages"),
            json!({"from": "test-sender", "content": {"text": "hello"}}),
        )
        .await;
    assert!(resp.status().is_success());

    // Poll messages
    let msgs = ctx
        .get_json(&format!("/api/v1/agents/{agent_id}/messages"))
        .await;
    assert!(msgs.is_array() || msgs.is_object());
}

#[tokio::test]
async fn agent_logs_append_and_get() {
    let ctx = Ctx::new().await;
    let agent_id = create_agent(&ctx).await;

    // Append a log line
    let resp = ctx
        .post(
            &format!("/api/v1/agents/{agent_id}/logs"),
            json!({"message": "starting work"}),
        )
        .await;
    assert!(resp.status().is_success());

    // Get logs — returns Vec<String> directly (no wrapper object)
    let logs = ctx
        .get_json(&format!("/api/v1/agents/{agent_id}/logs"))
        .await;
    let lines = logs.as_array().unwrap();
    assert!(!lines.is_empty());
    assert!(lines[0].as_str().unwrap().contains("starting work"));
}

#[tokio::test]
async fn agent_card_put_and_discover() {
    let ctx = Ctx::new().await;
    let agent_id = create_agent(&ctx).await;

    // Update agent status to active so it appears in discover
    ctx.put(
        &format!("/api/v1/agents/{agent_id}/status"),
        json!({"status": "active"}),
    )
    .await;

    // Publish agent card
    let resp = ctx
        .put(
            &format!("/api/v1/agents/{agent_id}/card"),
            json!({
                "agent_id": agent_id,
                "name": "test-agent",
                "description": "integration test agent",
                "capabilities": ["rust", "api-design"],
                "protocols": ["mcp"],
                "endpoint": "http://localhost:9000"
            }),
        )
        .await;
    assert!(resp.status().is_success());

    // Discover agents with capability
    let discovered = ctx
        .get_json("/api/v1/agents/discover?capability=rust")
        .await;
    let arr = discovered.as_array().unwrap();
    assert!(arr.iter().any(|c| c["name"] == "test-agent"));
}

#[tokio::test]
async fn agent_stack_register_and_get() {
    let ctx = Ctx::new().await;
    let agent_id = create_agent(&ctx).await;

    // Register stack fingerprint (AgentStack struct: agent_id, agents_md_hash, hooks,
    // mcp_servers, model, cli_version, settings_hash, persona_hash)
    let resp = ctx
        .post(
            &format!("/api/v1/agents/{agent_id}/stack"),
            json!({
                "agent_id": agent_id,
                "agents_md_hash": "sha256:abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890",
                "hooks": [],
                "mcp_servers": [],
                "model": "claude-opus-4-6",
                "cli_version": "1.0.0",
                "settings_hash": "sha256:0000000000000000000000000000000000000000000000000000000000000000",
                "persona_hash": null
            }),
        )
        .await;
    assert!(resp.status().is_success());

    // Get stack — response is StackResponse { agent_id, stack: AgentStack, fingerprint }
    let stack_resp = ctx
        .get_json(&format!("/api/v1/agents/{agent_id}/stack"))
        .await;
    assert_eq!(stack_resp["stack"]["model"], "claude-opus-4-6");
}

// ── 6. Tasks CRUD + transitions ───────────────────────────────────────────────

async fn create_task(ctx: &Ctx) -> String {
    let j = ctx
        .post_json(
            "/api/v1/tasks",
            json!({"title": "Test task", "description": "a task for testing"}),
        )
        .await;
    j["id"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn tasks_create_get_list() {
    let ctx = Ctx::new().await;

    let resp = ctx
        .post(
            "/api/v1/tasks",
            json!({"title": "My Task", "description": "details", "priority": "high"}),
        )
        .await;
    assert_eq!(resp.status(), 201);
    let task: serde_json::Value = resp.json().await.unwrap();
    let task_id = task["id"].as_str().unwrap().to_string();
    assert_eq!(task["title"], "My Task");
    assert_eq!(task["status"], "backlog");

    // Get
    let got = ctx.get_json(&format!("/api/v1/tasks/{task_id}")).await;
    assert_eq!(got["id"], task_id);

    // List
    let list = ctx.get_json("/api/v1/tasks").await;
    let arr = list.as_array().unwrap();
    assert!(arr.iter().any(|t| t["id"] == task_id));
}

#[tokio::test]
async fn tasks_update_and_transition() {
    let ctx = Ctx::new().await;
    let task_id = create_task(&ctx).await;

    // Update
    let updated = ctx
        .put_json(
            &format!("/api/v1/tasks/{task_id}"),
            json!({"title": "Updated Task", "priority": "medium"}),
        )
        .await;
    assert_eq!(updated["title"], "Updated Task");

    // Transition status: backlog → in_progress
    let resp = ctx
        .put(
            &format!("/api/v1/tasks/{task_id}/status"),
            json!({"status": "in_progress"}),
        )
        .await;
    assert!(resp.status().is_success());

    // Verify transition
    let got = ctx.get_json(&format!("/api/v1/tasks/{task_id}")).await;
    assert_eq!(got["status"], "in_progress");
}

#[tokio::test]
async fn tasks_list_by_status() {
    let ctx = Ctx::new().await;
    create_task(&ctx).await;

    let list = ctx.get_json("/api/v1/tasks?status=backlog").await;
    let arr = list.as_array().unwrap();
    assert!(!arr.is_empty());
    assert!(arr.iter().all(|t| t["status"] == "backlog"));
}

#[tokio::test]
async fn get_nonexistent_task_returns_404() {
    let ctx = Ctx::new().await;
    let resp = ctx.get("/api/v1/tasks/nonexistent").await;
    assert_eq!(resp.status(), 404);
}

// ── 7. Merge Requests ─────────────────────────────────────────────────────────

async fn create_mr(ctx: &Ctx, repo_id: &str) -> String {
    let j = ctx
        .post_json(
            "/api/v1/merge-requests",
            json!({
                "title": "Test MR",
                "repository_id": repo_id,
                "source_branch": "feat/test",
                "target_branch": "main",
                "author_agent_id": "test-agent"
            }),
        )
        .await;
    j["id"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn merge_requests_create_and_get() {
    let ctx = Ctx::new().await;
    let proj_id = create_project(&ctx).await;
    let repo_id = create_repo(&ctx, &proj_id).await;

    let resp = ctx
        .post(
            "/api/v1/merge-requests",
            json!({
                "title": "Add feature",
                "repository_id": repo_id,
                "source_branch": "feat/add-feature",
                "target_branch": "main",
                "author_agent_id": "agent-123"
            }),
        )
        .await;
    assert_eq!(resp.status(), 201);
    let mr: serde_json::Value = resp.json().await.unwrap();
    let mr_id = mr["id"].as_str().unwrap().to_string();
    assert_eq!(mr["status"], "open");
    assert_eq!(mr["source_branch"], "feat/add-feature");

    let got = ctx
        .get_json(&format!("/api/v1/merge-requests/{mr_id}"))
        .await;
    assert_eq!(got["id"], mr_id);
}

#[tokio::test]
async fn merge_requests_list_and_filter() {
    let ctx = Ctx::new().await;
    let proj_id = create_project(&ctx).await;
    let repo_id = create_repo(&ctx, &proj_id).await;
    create_mr(&ctx, &repo_id).await;

    let list = ctx.get_json("/api/v1/merge-requests").await;
    let arr = list.as_array().unwrap();
    assert!(!arr.is_empty());

    let filtered = ctx
        .get_json(&format!("/api/v1/merge-requests?repository_id={repo_id}"))
        .await;
    let arr2 = filtered.as_array().unwrap();
    assert!(!arr2.is_empty());
}

#[tokio::test]
async fn merge_requests_comments_and_reviews() {
    let ctx = Ctx::new().await;
    let proj_id = create_project(&ctx).await;
    let repo_id = create_repo(&ctx, &proj_id).await;
    let mr_id = create_mr(&ctx, &repo_id).await;

    // Add comment
    let cmt_resp = ctx
        .post(
            &format!("/api/v1/merge-requests/{mr_id}/comments"),
            json!({"body": "Looks good to me", "author_agent_id": "reviewer-1"}),
        )
        .await;
    assert!(cmt_resp.status().is_success());

    // List comments
    let comments = ctx
        .get_json(&format!("/api/v1/merge-requests/{mr_id}/comments"))
        .await;
    let arr = comments.as_array().unwrap();
    assert!(!arr.is_empty());
    assert!(arr[0]["body"].as_str().unwrap().contains("Looks good"));

    // Submit review (approve) — fields: reviewer_agent_id, decision, body (optional)
    let rev_resp = ctx
        .post(
            &format!("/api/v1/merge-requests/{mr_id}/reviews"),
            json!({"decision": "approved", "reviewer_agent_id": "reviewer-1", "body": "LGTM"}),
        )
        .await;
    assert!(rev_resp.status().is_success());

    // List reviews
    let reviews = ctx
        .get_json(&format!("/api/v1/merge-requests/{mr_id}/reviews"))
        .await;
    let arr2 = reviews.as_array().unwrap();
    assert!(!arr2.is_empty());
}

#[tokio::test]
async fn merge_request_status_transition() {
    let ctx = Ctx::new().await;
    let proj_id = create_project(&ctx).await;
    let repo_id = create_repo(&ctx, &proj_id).await;
    let mr_id = create_mr(&ctx, &repo_id).await;

    // Transition to approved
    let resp = ctx
        .put(
            &format!("/api/v1/merge-requests/{mr_id}/status"),
            json!({"status": "approved"}),
        )
        .await;
    assert!(resp.status().is_success());

    let got = ctx
        .get_json(&format!("/api/v1/merge-requests/{mr_id}"))
        .await;
    assert_eq!(got["status"], "approved");
}

#[tokio::test]
async fn merge_request_diff_endpoint() {
    let ctx = Ctx::new().await;
    let proj_id = create_project(&ctx).await;
    let repo_id = create_repo(&ctx, &proj_id).await;
    let mr_id = create_mr(&ctx, &repo_id).await;

    // Diff: no real git repo, returns empty or error but should not 401
    let resp = ctx
        .get(&format!("/api/v1/merge-requests/{mr_id}/diff"))
        .await;
    assert_ne!(resp.status(), 401_u16);
}

// ── 8. Merge Queue ────────────────────────────────────────────────────────────

#[tokio::test]
async fn merge_queue_enqueue_list_cancel() {
    let ctx = Ctx::new().await;
    let proj_id = create_project(&ctx).await;
    let repo_id = create_repo(&ctx, &proj_id).await;
    let mr_id = create_mr(&ctx, &repo_id).await;

    // Enqueue
    let enq_resp = ctx
        .post(
            "/api/v1/merge-queue/enqueue",
            json!({"merge_request_id": mr_id}),
        )
        .await;
    assert!(enq_resp.status().is_success());
    let entry: serde_json::Value = enq_resp.json().await.unwrap();
    let entry_id = entry["id"].as_str().unwrap().to_string();
    assert_eq!(entry["status"], "queued");

    // List
    let list = ctx.get_json("/api/v1/merge-queue").await;
    let arr = list.as_array().unwrap();
    assert!(arr.iter().any(|e| e["id"] == entry_id));

    // Cancel
    let del_resp = ctx.delete(&format!("/api/v1/merge-queue/{entry_id}")).await;
    assert!(del_resp.status().is_success());

    // List again: entry should be gone or cancelled
    let list2 = ctx.get_json("/api/v1/merge-queue").await;
    let arr2 = list2.as_array().unwrap();
    assert!(arr2.iter().all(|e| e["id"] != entry_id));
}

// ── 9. Quality Gates ──────────────────────────────────────────────────────────

#[tokio::test]
async fn quality_gates_create_list_delete() {
    let ctx = Ctx::new().await;
    let proj_id = create_project(&ctx).await;
    let repo_id = create_repo(&ctx, &proj_id).await;

    // Create gate (gate_type uses snake_case; name field is required)
    let gate_resp = ctx
        .post(
            &format!("/api/v1/repos/{repo_id}/gates"),
            json!({
                "name": "run-tests",
                "gate_type": "test_command",
                "command": "cargo test --all"
            }),
        )
        .await;
    assert!(gate_resp.status().is_success());
    let gate: serde_json::Value = gate_resp.json().await.unwrap();
    let gate_id = gate["id"].as_str().unwrap().to_string();

    // List gates
    let gates = ctx
        .get_json(&format!("/api/v1/repos/{repo_id}/gates"))
        .await;
    let arr = gates.as_array().unwrap();
    assert!(arr.iter().any(|g| g["id"] == gate_id));

    // Delete gate
    let del_resp = ctx
        .delete(&format!("/api/v1/repos/{repo_id}/gates/{gate_id}"))
        .await;
    assert!(del_resp.status().is_success());

    // List: gate gone
    let gates2 = ctx
        .get_json(&format!("/api/v1/repos/{repo_id}/gates"))
        .await;
    let arr2 = gates2.as_array().unwrap();
    assert!(arr2.iter().all(|g| g["id"] != gate_id));
}

// ── 10. Analytics + Costs ─────────────────────────────────────────────────────

#[tokio::test]
async fn analytics_record_and_query() {
    let ctx = Ctx::new().await;

    // Record event
    let resp = ctx
        .post(
            "/api/v1/analytics/events",
            json!({
                "event_name": "task.status_changed",
                "agent_id": "agent-1",
                "properties": {"from": "backlog", "to": "in_progress"}
            }),
        )
        .await;
    assert!(resp.status().is_success());

    // Query events
    let events = ctx.get_json("/api/v1/analytics/events").await;
    let arr = events.as_array().unwrap();
    assert!(!arr.is_empty());

    // Count events (requires event_name, since, until query params)
    let count_resp = ctx
        .get("/api/v1/analytics/count?event_name=task.status_changed&since=0&until=9999999999")
        .await;
    assert!(count_resp.status().is_success());

    // Daily counts (requires event_name, since, until query params)
    let daily_resp = ctx
        .get("/api/v1/analytics/daily?event_name=task.status_changed&since=0&until=9999999999")
        .await;
    assert!(daily_resp.status().is_success());
}

#[tokio::test]
async fn costs_record_and_query() {
    let ctx = Ctx::new().await;

    // Record a cost entry (currency is required)
    let resp = ctx
        .post(
            "/api/v1/costs",
            json!({
                "agent_id": "agent-1",
                "task_id": "task-1",
                "cost_type": "llm_tokens",
                "amount": 0.05,
                "currency": "USD"
            }),
        )
        .await;
    assert!(resp.status().is_success());

    // Query costs (requires agent_id or task_id filter)
    let costs = ctx.get_json("/api/v1/costs?agent_id=agent-1").await;
    let arr = costs.as_array().unwrap();
    assert!(!arr.is_empty());

    // Cost summary (requires since + until query params)
    let summary = ctx
        .get_json("/api/v1/costs/summary?since=0&until=9999999999")
        .await;
    assert!(summary.is_object());
}

// ── 11. Audit events ──────────────────────────────────────────────────────────

#[tokio::test]
async fn audit_record_and_query() {
    let ctx = Ctx::new().await;

    // Record audit event
    let resp = ctx
        .post(
            "/api/v1/audit/events",
            json!({
                "event_type": "process_exec",
                "agent_id": "agent-1",
                "pid": 12345,
                "detail": "git commit"
            }),
        )
        .await;
    assert!(resp.status().is_success());

    // Query audit events
    let events = ctx.get_json("/api/v1/audit/events").await;
    assert!(events.is_array() || events.is_object());

    // Stats
    let stats = ctx.get_json("/api/v1/audit/stats").await;
    assert!(stats.is_object());
}

// ── 12. Admin endpoints ───────────────────────────────────────────────────────

#[tokio::test]
async fn admin_health() {
    let ctx = Ctx::new().await;
    let resp = ctx.get("/api/v1/admin/health").await;
    assert!(resp.status().is_success());
    let j: serde_json::Value = resp.json().await.unwrap();
    assert!(j["uptime_secs"].is_number());
}

#[tokio::test]
async fn admin_jobs() {
    let ctx = Ctx::new().await;
    let resp = ctx.get("/api/v1/admin/jobs").await;
    assert!(resp.status().is_success());
    let j: serde_json::Value = resp.json().await.unwrap();
    assert!(j.is_array() || j.is_object());
}

#[tokio::test]
async fn admin_seed_idempotent() {
    let ctx = Ctx::new().await;

    // First seed
    let resp1 = ctx.post("/api/v1/admin/seed", json!({})).await;
    assert!(resp1.status().is_success());

    // Second seed (idempotent)
    let resp2 = ctx.post("/api/v1/admin/seed", json!({})).await;
    assert!(resp2.status().is_success());
    let j: serde_json::Value = resp2.json().await.unwrap();
    assert_eq!(j["already_seeded"], true);
}

#[tokio::test]
async fn admin_export() {
    let ctx = Ctx::new().await;
    let resp = ctx.get("/api/v1/admin/export").await;
    assert!(resp.status().is_success());
    let j: serde_json::Value = resp.json().await.unwrap();
    assert!(j.is_object());
}

#[tokio::test]
async fn admin_retention_list_and_update() {
    let ctx = Ctx::new().await;

    // List retention policies
    let resp = ctx.get("/api/v1/admin/retention").await;
    assert!(resp.status().is_success());

    // Update retention policies (takes Vec<RetentionPolicy> with data_type and max_age_days)
    let put_resp = ctx
        .put(
            "/api/v1/admin/retention",
            json!([{"data_type": "activity", "max_age_days": 30}]),
        )
        .await;
    assert!(put_resp.status().is_success());
}

#[tokio::test]
async fn admin_kill_agent() {
    let ctx = Ctx::new().await;
    let agent_id = create_agent(&ctx).await;

    // Status → active
    ctx.put(
        &format!("/api/v1/agents/{agent_id}/status"),
        json!({"status": "active"}),
    )
    .await;

    // Kill
    let resp = ctx
        .post(&format!("/api/v1/admin/agents/{agent_id}/kill"), json!({}))
        .await;
    assert!(resp.status().is_success());

    // Agent should be dead/stopped
    let got = ctx.get_json(&format!("/api/v1/agents/{agent_id}")).await;
    assert!(
        got["status"] == "dead" || got["status"] == "idle" || got["status"] == "stopped",
        "agent status after kill: {}",
        got["status"]
    );
}

#[tokio::test]
async fn admin_reassign_agent() {
    let ctx = Ctx::new().await;
    let agent_id = create_agent(&ctx).await;
    let task_id = create_task(&ctx).await;

    // Reassign
    let resp = ctx
        .post(
            &format!("/api/v1/admin/agents/{agent_id}/reassign"),
            json!({"task_id": task_id}),
        )
        .await;
    // Might succeed or return 400/422 if agent not in right state
    assert_ne!(resp.status(), 401_u16);
}

// ── 13. SIEM targets ──────────────────────────────────────────────────────────

#[tokio::test]
async fn siem_targets_crud() {
    let ctx = Ctx::new().await;

    // Create SIEM target (fields: name, target_type, config, enabled)
    let resp = ctx
        .post(
            "/api/v1/admin/siem",
            json!({
                "name": "splunk-prod",
                "target_type": "webhook",
                "config": {"url": "https://siem.example.com/events", "format": "json"},
                "enabled": true
            }),
        )
        .await;
    assert!(resp.status().is_success());
    let target: serde_json::Value = resp.json().await.unwrap();
    let target_id = target["id"].as_str().unwrap().to_string();

    // List
    let list = ctx.get_json("/api/v1/admin/siem").await;
    let arr = list.as_array().unwrap();
    assert!(arr.iter().any(|t| t["id"] == target_id));

    // Update
    let upd = ctx
        .put(
            &format!("/api/v1/admin/siem/{target_id}"),
            json!({"enabled": false}),
        )
        .await;
    assert!(upd.status().is_success());

    // Delete
    let del = ctx.delete(&format!("/api/v1/admin/siem/{target_id}")).await;
    assert!(del.status().is_success());
}

// ── 14. Compute Targets ───────────────────────────────────────────────────────

#[tokio::test]
async fn compute_targets_crud() {
    let ctx = Ctx::new().await;

    // Create
    let resp = ctx
        .post(
            "/api/v1/admin/compute-targets",
            json!({
                "name": "local-docker",
                "target_type": "docker",
                "host": "unix:///var/run/docker.sock"
            }),
        )
        .await;
    assert!(resp.status().is_success());
    let ct: serde_json::Value = resp.json().await.unwrap();
    let ct_id = ct["id"].as_str().unwrap().to_string();

    // Get
    let got = ctx
        .get_json(&format!("/api/v1/admin/compute-targets/{ct_id}"))
        .await;
    assert_eq!(got["id"], ct_id);

    // List
    let list = ctx.get_json("/api/v1/admin/compute-targets").await;
    let arr = list.as_array().unwrap();
    assert!(arr.iter().any(|t| t["id"] == ct_id));

    // Delete
    let del = ctx
        .delete(&format!("/api/v1/admin/compute-targets/{ct_id}"))
        .await;
    assert!(del.status().is_success());
}

// ── 15. Network peers ─────────────────────────────────────────────────────────

#[tokio::test]
async fn network_peers_register_list_delete() {
    let ctx = Ctx::new().await;
    let agent_id = create_agent(&ctx).await;

    // Register peer (field is wireguard_pubkey, not public_key)
    let resp = ctx
        .post(
            "/api/v1/network/peers",
            json!({
                "agent_id": agent_id,
                "wireguard_pubkey": "abc123publickey==",
                "endpoint": "10.0.0.1:51820",
                "allowed_ips": ["10.1.0.1/32"]
            }),
        )
        .await;
    assert!(resp.status().is_success());
    let peer: serde_json::Value = resp.json().await.unwrap();
    let peer_id = peer["id"].as_str().unwrap().to_string();

    // List peers
    let list = ctx.get_json("/api/v1/network/peers").await;
    let arr = list.as_array().unwrap();
    assert!(arr.iter().any(|p| p["id"] == peer_id));

    // Get by agent
    let by_agent = ctx
        .get_json(&format!("/api/v1/network/peers/agent/{agent_id}"))
        .await;
    assert!(by_agent["id"].is_string() || by_agent.is_null());

    // DERP map
    let derp = ctx.get_json("/api/v1/network/derp-map").await;
    assert!(derp.is_object() || derp.is_array());

    // Delete
    let del = ctx
        .delete(&format!("/api/v1/network/peers/{peer_id}"))
        .await;
    assert!(del.status().is_success());
}

// ── 16. Agent Compose ─────────────────────────────────────────────────────────

#[tokio::test]
async fn compose_apply_status_teardown() {
    let ctx = Ctx::new().await;
    let proj_id = create_project(&ctx).await;
    let repo_id = create_repo(&ctx, &proj_id).await;

    // Apply a compose spec (requires version, project_id, repo_id, agents)
    let resp = ctx
        .post(
            "/api/v1/compose/apply",
            json!({
                "version": "1",
                "project_id": proj_id,
                "repo_id": repo_id,
                "agents": [
                    {
                        "name": "orchestrator",
                        "role": "Orchestrator",
                        "capabilities": ["planning"],
                        "task": {
                            "title": "Integration test compose",
                            "priority": "Low"
                        }
                    }
                ]
            }),
        )
        .await;
    assert!(resp.status().is_success());
    let compose_resp: serde_json::Value = resp.json().await.unwrap();
    let compose_id = compose_resp["compose_id"].as_str().unwrap().to_string();

    // Status (requires compose_id query param)
    let status = ctx
        .get_json(&format!("/api/v1/compose/status?compose_id={compose_id}"))
        .await;
    assert_eq!(status["compose_id"], compose_id);

    // Teardown (requires compose_id in body)
    let teardown = ctx
        .post(
            "/api/v1/compose/teardown",
            json!({"compose_id": compose_id}),
        )
        .await;
    assert!(teardown.status().is_success());
}

// ── 17. Worktrees ─────────────────────────────────────────────────────────────

#[tokio::test]
async fn worktrees_create_list_delete() {
    let ctx = Ctx::new().await;
    let proj_id = create_project(&ctx).await;
    let repo_id = create_repo(&ctx, &proj_id).await;
    let agent_id = create_agent(&ctx).await;

    // Create worktree
    let resp = ctx
        .post(
            &format!("/api/v1/repos/{repo_id}/worktrees"),
            json!({
                "agent_id": agent_id,
                "branch": "feat/wt-test",
                "path": "/tmp/test-worktree"
            }),
        )
        .await;
    assert!(resp.status().is_success());
    let wt: serde_json::Value = resp.json().await.unwrap();
    let wt_id = wt["id"].as_str().unwrap().to_string();

    // List worktrees
    let list = ctx
        .get_json(&format!("/api/v1/repos/{repo_id}/worktrees"))
        .await;
    let arr = list.as_array().unwrap();
    assert!(arr.iter().any(|w| w["id"] == wt_id));

    // Delete worktree
    let del = ctx
        .delete(&format!("/api/v1/repos/{repo_id}/worktrees/{wt_id}"))
        .await;
    assert!(del.status().is_success());
}

// ── 18. Agent-commit tracking ─────────────────────────────────────────────────

#[tokio::test]
async fn agent_commits_record_and_list() {
    let ctx = Ctx::new().await;
    let proj_id = create_project(&ctx).await;
    let repo_id = create_repo(&ctx, &proj_id).await;
    let agent_id = create_agent(&ctx).await;

    // Record commit
    let resp = ctx
        .post(
            &format!("/api/v1/repos/{repo_id}/commits/record"),
            json!({
                "agent_id": agent_id,
                "commit_sha": "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2",
                "branch": "feat/test",
                "message": "feat: add test commit"
            }),
        )
        .await;
    assert!(resp.status().is_success());

    // List agent commits
    let list = ctx
        .get_json(&format!(
            "/api/v1/repos/{repo_id}/agent-commits?agent_id={agent_id}"
        ))
        .await;
    assert!(list.is_array());
}

// ── 19. Code awareness ────────────────────────────────────────────────────────

#[tokio::test]
async fn code_awareness_endpoints() {
    let ctx = Ctx::new().await;
    let proj_id = create_project(&ctx).await;
    let repo_id = create_repo(&ctx, &proj_id).await;

    // Blame (no real git, returns error or empty)
    let blame = ctx
        .get(&format!("/api/v1/repos/{repo_id}/blame?path=src/main.rs"))
        .await;
    assert_ne!(blame.status(), 401_u16);

    // Hot files
    let hot = ctx.get(&format!("/api/v1/repos/{repo_id}/hot-files")).await;
    assert_ne!(hot.status(), 401_u16);

    // Review routing (no real git)
    let routing = ctx
        .get(&format!(
            "/api/v1/repos/{repo_id}/review-routing?path=src/main.rs"
        ))
        .await;
    assert_ne!(routing.status(), 401_u16);
}

#[tokio::test]
async fn agent_touched_paths() {
    let ctx = Ctx::new().await;
    let agent_id = create_agent(&ctx).await;

    let resp = ctx
        .get(&format!("/api/v1/agents/{agent_id}/touched-paths"))
        .await;
    assert_ne!(resp.status(), 401_u16);
}

// ── 20. Speculative merge ─────────────────────────────────────────────────────

#[tokio::test]
async fn speculative_merge_endpoints() {
    let ctx = Ctx::new().await;
    let proj_id = create_project(&ctx).await;
    let repo_id = create_repo(&ctx, &proj_id).await;

    // List speculative results
    let list = ctx
        .get(&format!("/api/v1/repos/{repo_id}/speculative"))
        .await;
    assert_ne!(list.status(), 401_u16);

    // Get for specific branch
    let branch_result = ctx
        .get(&format!("/api/v1/repos/{repo_id}/speculative/feat~test"))
        .await;
    assert_ne!(branch_result.status(), 401_u16);
}

// ── 21. Stack attestation policy (M14.2) ──────────────────────────────────────

#[tokio::test]
async fn stack_policy_get_and_set() {
    let ctx = Ctx::new().await;
    let proj_id = create_project(&ctx).await;
    let repo_id = create_repo(&ctx, &proj_id).await;

    // Get (no policy set)
    let resp = ctx
        .get(&format!("/api/v1/repos/{repo_id}/stack-policy"))
        .await;
    assert!(resp.status().is_success());

    // Set policy
    let put_resp = ctx
        .put(
            &format!("/api/v1/repos/{repo_id}/stack-policy"),
            json!({"required_fingerprint": "sha256:abc123"}),
        )
        .await;
    assert!(put_resp.status().is_success());
}

// ── 22. Activity log ──────────────────────────────────────────────────────────

#[tokio::test]
async fn activity_log_query() {
    let ctx = Ctx::new().await;
    let resp = ctx.get("/api/v1/activity?limit=10").await;
    assert!(resp.status().is_success());
    let j: serde_json::Value = resp.json().await.unwrap();
    assert!(j.is_array());
}

// ── 23. Spec approvals ────────────────────────────────────────────────────────

#[tokio::test]
async fn spec_approvals_lifecycle() {
    let ctx = Ctx::new().await;
    let agent_id = create_agent(&ctx).await;

    // Approve a spec (fields: path, sha [40-char hex], approver_id, signature)
    let resp = ctx
        .post(
            "/api/v1/specs/approve",
            json!({
                "path": "specs/system/my-feature.md",
                "sha": "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2",
                "approver_id": format!("agent:{agent_id}")
            }),
        )
        .await;
    assert!(resp.status().is_success());
    let approval: serde_json::Value = resp.json().await.unwrap();
    let approval_id = approval["id"].as_str().unwrap().to_string();

    // List approvals
    let list = ctx.get_json("/api/v1/specs/approvals").await;
    let arr = list.as_array().unwrap();
    assert!(arr.iter().any(|a| a["id"] == approval_id));

    // Revoke (fields: approval_id, revoked_by, reason)
    let revoke_resp = ctx
        .post(
            "/api/v1/specs/revoke",
            json!({
                "approval_id": approval_id,
                "revoked_by": format!("agent:{agent_id}"),
                "reason": "superseded by new spec version"
            }),
        )
        .await;
    assert!(revoke_resp.status().is_success());
}

// ── 24. Release automation ────────────────────────────────────────────────────

#[tokio::test]
async fn release_prepare_repo_not_found() {
    let ctx = Ctx::new().await;

    let resp = ctx
        .post(
            "/api/v1/release/prepare",
            json!({
                "repo_id": "nonexistent-repo",
                "create_mr": false
            }),
        )
        .await;
    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn release_prepare_empty_repo() {
    let ctx = Ctx::new().await;
    let proj_id = create_project(&ctx).await;
    let repo_id = create_repo(&ctx, &proj_id).await;

    // No real git repo on disk; commits_since returns empty → v0.1.0, no MR
    let resp = ctx
        .post(
            "/api/v1/release/prepare",
            json!({
                "repo_id": repo_id,
                "branch": "main",
                "create_mr": false
            }),
        )
        .await;
    assert!(resp.status().is_success());
    let j: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(j["next_version"], "v0.1.0");
    assert_eq!(j["bump_type"], "none");
}

// ── 25. Admin audit search ────────────────────────────────────────────────────

#[tokio::test]
async fn admin_audit_search() {
    let ctx = Ctx::new().await;
    let resp = ctx.get("/api/v1/admin/audit").await;
    assert!(resp.status().is_success());
    let j: serde_json::Value = resp.json().await.unwrap();
    assert!(j.is_array() || j.is_object());
}

// ── 26. Admin snapshot ────────────────────────────────────────────────────────

#[tokio::test]
async fn admin_snapshot_create_list_delete() {
    let ctx = Ctx::new().await;

    // Create snapshot
    let resp = ctx.post("/api/v1/admin/snapshot", json!({})).await;
    // May fail if no db URL configured but should not 401
    assert_ne!(resp.status(), 401_u16);

    // List snapshots
    let list_resp = ctx.get("/api/v1/admin/snapshots").await;
    assert_ne!(list_resp.status(), 401_u16);
}

// ── 27. MR gate results ───────────────────────────────────────────────────────

#[tokio::test]
async fn mr_gate_results() {
    let ctx = Ctx::new().await;
    let proj_id = create_project(&ctx).await;
    let repo_id = create_repo(&ctx, &proj_id).await;
    let mr_id = create_mr(&ctx, &repo_id).await;

    let resp = ctx
        .get(&format!("/api/v1/merge-requests/{mr_id}/gates"))
        .await;
    assert!(resp.status().is_success());
    let j: serde_json::Value = resp.json().await.unwrap();
    assert!(j.is_array());
}

// ── 28. MCP endpoints ─────────────────────────────────────────────────────────

#[tokio::test]
async fn mcp_initialize() {
    let ctx = Ctx::new().await;

    let resp = ctx
        .client
        .post(format!("{}/mcp", ctx.base))
        .header("Authorization", format!("Bearer {TOKEN}"))
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "clientInfo": {"name": "integration-test", "version": "1.0"}
            }
        }))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());
    let j: serde_json::Value = resp.json().await.unwrap();
    assert!(j["result"].is_object() || j["id"] == 1);
}

#[tokio::test]
async fn mcp_tools_list() {
    let ctx = Ctx::new().await;

    let resp = ctx
        .client
        .post(format!("{}/mcp", ctx.base))
        .header("Authorization", format!("Bearer {TOKEN}"))
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/list",
            "params": {}
        }))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());
    let j: serde_json::Value = resp.json().await.unwrap();
    let tools = &j["result"]["tools"];
    assert!(tools.is_array());
    let arr = tools.as_array().unwrap();
    assert!(!arr.is_empty());
    // Verify gyre_create_task is present
    assert!(arr.iter().any(|t| t["name"] == "gyre_create_task"));
}

// ── 29. Admin run job ─────────────────────────────────────────────────────────

#[tokio::test]
async fn admin_run_job() {
    let ctx = Ctx::new().await;

    let resp = ctx
        .post("/api/v1/admin/jobs/merge_processor/run", json!({}))
        .await;
    // May fail with 404 if job name not registered but should not 401
    assert_ne!(resp.status(), 401_u16);
}

// ── 30. Provenance endpoint ───────────────────────────────────────────────────

#[tokio::test]
async fn provenance_query() {
    let ctx = Ctx::new().await;
    let proj_id = create_project(&ctx).await;
    let repo_id = create_repo(&ctx, &proj_id).await;

    let resp = ctx
        .get(&format!("/api/v1/repos/{repo_id}/provenance"))
        .await;
    assert_ne!(resp.status(), 401_u16);
}

// ── 31. AIBOM endpoint ───────────────────────────────────────────────────────

#[tokio::test]
async fn aibom_query() {
    let ctx = Ctx::new().await;
    let proj_id = create_project(&ctx).await;
    let repo_id = create_repo(&ctx, &proj_id).await;

    let resp = ctx.get(&format!("/api/v1/repos/{repo_id}/aibom")).await;
    assert_ne!(resp.status(), 401_u16);
}

// ── 32. API key creation (Admin) ──────────────────────────────────────────────

#[tokio::test]
async fn api_key_creation() {
    let ctx = Ctx::new().await;

    // API key creation requires Admin role AND a user account (from JWT).
    // The global dev token is Admin but has no user_id, so this returns 403.
    // We just verify the endpoint exists (not 404/401).
    let resp = ctx
        .post(
            "/api/v1/auth/api-keys",
            json!({"name": "integration-test-key"}),
        )
        .await;
    assert_ne!(resp.status(), 404_u16, "endpoint should exist");
    assert_ne!(resp.status(), 401_u16, "should be authenticated");
}

// ── CORS tests ────────────────────────────────────────────────────────────────

/// Verify that the CORS preflight response includes the server's own port in
/// Access-Control-Allow-Origin when GYRE_PORT matches the listening port.
///
/// Regression test for: boss 401 bug on port 2223 — CORS default only listed
/// 2222/3000/5173, so preflight for GET+Authorization from port 2223 failed.
#[tokio::test]
async fn cors_includes_server_port() {
    // Spin up a server on a random port, set GYRE_PORT so the CORS layer picks it up.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();

    // SAFETY: tests are single-threaded per tokio runtime; we set the env var
    // before building state so the CORS layer reads the correct port.
    // This env var is only consumed at startup, so parallel tests are not affected.
    unsafe { std::env::set_var("GYRE_PORT", port.to_string()) };

    let base_url = format!("http://127.0.0.1:{port}");
    let state = build_state(TOKEN, &base_url, None);
    let app = build_router(Arc::clone(&state));
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    // Clean up env var immediately after server startup so parallel tests are not affected.
    unsafe { std::env::remove_var("GYRE_PORT") };

    let client = reqwest::Client::new();
    let origin = format!("http://localhost:{port}");

    // Send a CORS preflight OPTIONS request simulating a browser requesting
    // a credentialed GET (Authorization header triggers preflight).
    let resp = client
        .request(
            reqwest::Method::OPTIONS,
            format!("{base_url}/api/v1/version"),
        )
        .header("Origin", &origin)
        .header("Access-Control-Request-Method", "GET")
        .header("Access-Control-Request-Headers", "authorization")
        .send()
        .await
        .unwrap();

    let allow_origin = resp
        .headers()
        .get("access-control-allow-origin")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    assert_eq!(
        allow_origin, origin,
        "CORS allow-origin should include the server's own port (http://localhost:{port})"
    );
}
