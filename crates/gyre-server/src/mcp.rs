//! MCP (Model Context Protocol) server implementation.
//!
//! Exposes Gyre capabilities as MCP tools over HTTP transport.
//!
//! Endpoints:
//! - `POST /mcp`     — JSON-RPC 2.0 request/response (tool calls, init, list)
//! - `GET  /mcp/sse` — Server-Sent Events stream for server→client notifications

use axum::{
    extract::State,
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse,
    },
    Json,
};
use futures_util::stream;
use gyre_common::{ActivityEventData, AgEventType, Id};
use gyre_domain::{MergeRequest, Task, TaskPriority, TaskStatus};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;
use tracing::instrument;

use crate::AppState;

// ── JSON-RPC 2.0 types ────────────────────────────────────────────────────────

const PROTOCOL_VERSION: &str = "2024-11-05";

#[derive(Debug, Deserialize)]
pub struct JsonRpcRequest {
    #[allow(dead_code)]
    pub jsonrpc: String,
    #[serde(default)]
    pub id: Option<Value>,
    pub method: String,
    #[serde(default)]
    pub params: Option<Value>,
}

#[derive(Debug, Serialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
}

#[derive(Debug, Serialize)]
pub struct RpcError {
    pub code: i32,
    pub message: String,
}

// Standard JSON-RPC error codes
const METHOD_NOT_FOUND: i32 = -32601;
const INVALID_PARAMS: i32 = -32602;

impl JsonRpcResponse {
    fn ok(id: Option<Value>, result: Value) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: Some(result),
            error: None,
        }
    }

    fn err(id: Option<Value>, code: i32, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(RpcError {
                code,
                message: message.into(),
            }),
        }
    }
}

// ── Tool definitions ──────────────────────────────────────────────────────────

fn tool_definitions() -> Value {
    json!({
        "tools": [
            {
                "name": "gyre_create_task",
                "description": "Create a new task in the Gyre platform.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "title": { "type": "string", "description": "Task title" },
                        "description": { "type": "string", "description": "Task description" },
                        "priority": {
                            "type": "string",
                            "enum": ["low", "medium", "high", "critical"],
                            "description": "Task priority (default: medium)"
                        },
                        "parent_task_id": { "type": "string", "description": "Parent task ID for subtasks" },
                        "labels": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "Labels to attach to the task"
                        }
                    },
                    "required": ["title"]
                }
            },
            {
                "name": "gyre_list_tasks",
                "description": "List tasks with optional filters.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "status": {
                            "type": "string",
                            "enum": ["backlog", "in_progress", "review", "done", "blocked"],
                            "description": "Filter by task status"
                        },
                        "assigned_to": { "type": "string", "description": "Filter by assigned agent ID" }
                    }
                }
            },
            {
                "name": "gyre_update_task",
                "description": "Update an existing task's fields or status.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "id": { "type": "string", "description": "Task ID to update" },
                        "title": { "type": "string" },
                        "description": { "type": "string" },
                        "priority": { "type": "string", "enum": ["low", "medium", "high", "critical"] },
                        "assigned_to": { "type": "string", "description": "Agent ID to assign" },
                        "status": {
                            "type": "string",
                            "enum": ["backlog", "in_progress", "review", "done", "blocked"],
                            "description": "Transition to new status"
                        },
                        "branch": { "type": "string", "description": "Git branch for this task" },
                        "pr_link": { "type": "string", "description": "Pull request URL" }
                    },
                    "required": ["id"]
                }
            },
            {
                "name": "gyre_create_mr",
                "description": "Create a merge request.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "title": { "type": "string", "description": "MR title" },
                        "repository_id": { "type": "string", "description": "Repository ID" },
                        "source_branch": { "type": "string", "description": "Source branch name" },
                        "target_branch": { "type": "string", "description": "Target branch name" },
                        "author_agent_id": { "type": "string", "description": "Agent ID creating the MR" }
                    },
                    "required": ["title", "repository_id", "source_branch", "target_branch"]
                }
            },
            {
                "name": "gyre_list_mrs",
                "description": "List merge requests with optional filters.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "status": { "type": "string", "description": "Filter by MR status" },
                        "repository_id": { "type": "string", "description": "Filter by repository" }
                    }
                }
            },
            {
                "name": "gyre_record_activity",
                "description": "Record an activity event in the Gyre activity feed.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "agent_id": { "type": "string", "description": "Agent producing the event" },
                        "event_type": {
                            "type": "string",
                            "enum": [
                                "TOOL_CALL_START", "TOOL_CALL_END",
                                "TEXT_MESSAGE_CONTENT",
                                "RUN_STARTED", "RUN_FINISHED",
                                "STATE_CHANGED", "ERROR"
                            ],
                            "description": "AG-UI typed event type"
                        },
                        "description": { "type": "string", "description": "Human-readable event description" }
                    },
                    "required": ["agent_id", "event_type", "description"]
                }
            },
            {
                "name": "gyre_agent_heartbeat",
                "description": "Send a heartbeat for an agent to keep it alive.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "agent_id": { "type": "string", "description": "Agent ID" }
                    },
                    "required": ["agent_id"]
                }
            },
            {
                "name": "gyre_agent_complete",
                "description": "Signal that an agent has completed its current task.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "agent_id": { "type": "string", "description": "Agent ID" }
                    },
                    "required": ["agent_id"]
                }
            },
            {
                "name": "gyre_analytics_query",
                "description": "Query analytics data. Supports usage (count + trend), compare (before/after pivot), and top N events.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "query_type": {
                            "type": "string",
                            "enum": ["usage", "compare", "top"],
                            "description": "Type of analytics query"
                        },
                        "params": {
                            "type": "object",
                            "description": "Query parameters. For 'usage': {event_name, since?, until?}. For 'compare': {event_name, before, pivot, after?}. For 'top': {limit?, since?}."
                        }
                    },
                    "required": ["query_type", "params"]
                }
            },
            {
                "name": "gyre_search",
                "description": "Full-text search across tasks, agents, MRs, and specs. Returns matching entities ranked by relevance.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "q": { "type": "string", "description": "Search query" },
                        "entity_type": {
                            "type": "string",
                            "enum": ["task", "agent", "mr", "spec"],
                            "description": "Filter by entity type (optional)"
                        },
                        "workspace_id": { "type": "string", "description": "Limit to a specific workspace (optional)" },
                        "limit": { "type": "number", "description": "Max results to return (default 10)" }
                    },
                    "required": ["q"]
                }
            }
        ]
    })
}

// ── Tool call handlers ────────────────────────────────────────────────────────

fn get_str<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key)?.as_str()
}

fn require_str<'a>(args: &'a Value, key: &str) -> Result<&'a str, JsonRpcResponse> {
    get_str(args, key).ok_or_else(|| {
        JsonRpcResponse::err(
            None,
            INVALID_PARAMS,
            format!("missing required field: {key}"),
        )
    })
}

fn tool_result(text: impl Into<String>) -> Value {
    json!({
        "content": [{ "type": "text", "text": text.into() }],
        "isError": false
    })
}

fn tool_error(text: impl Into<String>) -> Value {
    json!({
        "content": [{ "type": "text", "text": text.into() }],
        "isError": true
    })
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn new_id() -> Id {
    Id::new(uuid::Uuid::new_v4().to_string())
}

fn parse_priority(s: &str) -> TaskPriority {
    match s {
        "low" => TaskPriority::Low,
        "high" => TaskPriority::High,
        "critical" => TaskPriority::Critical,
        _ => TaskPriority::Medium,
    }
}

fn parse_status(s: &str) -> Option<TaskStatus> {
    match s {
        "backlog" => Some(TaskStatus::Backlog),
        "in_progress" => Some(TaskStatus::InProgress),
        "review" => Some(TaskStatus::Review),
        "done" => Some(TaskStatus::Done),
        "blocked" => Some(TaskStatus::Blocked),
        _ => None,
    }
}

async fn handle_create_task(state: &AppState, args: &Value) -> Value {
    let title = match require_str(args, "title") {
        Ok(t) => t.to_string(),
        Err(_) => return tool_error("missing required field: title"),
    };
    let now = now_secs();
    let mut task = Task::new(new_id(), title, now);
    task.description = get_str(args, "description").map(|s| s.to_string());
    if let Some(p) = get_str(args, "priority") {
        task.priority = parse_priority(p);
    }
    task.parent_task_id = get_str(args, "parent_task_id").map(Id::new);
    if let Some(labels) = args.get("labels").and_then(|v| v.as_array()) {
        task.labels = labels
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect();
    }
    match state.tasks.create(&task).await {
        Ok(()) => tool_result(format!("Created task {} (id: {})", task.title, task.id)),
        Err(e) => tool_error(format!("Failed to create task: {e}")),
    }
}

async fn handle_list_tasks(state: &AppState, args: &Value) -> Value {
    let tasks = if let Some(status_str) = get_str(args, "status") {
        if let Some(status) = parse_status(status_str) {
            state.tasks.list_by_status(&status).await
        } else {
            return tool_error(format!("Unknown status: {status_str}"));
        }
    } else if let Some(agent_id) = get_str(args, "assigned_to") {
        state.tasks.list_by_assignee(&Id::new(agent_id)).await
    } else {
        state.tasks.list().await
    };
    match tasks {
        Ok(list) => {
            let items: Vec<Value> = list
                .into_iter()
                .map(|t| {
                    json!({
                        "id": t.id.to_string(),
                        "title": t.title,
                        "status": format!("{:?}", t.status).to_lowercase(),
                        "priority": format!("{:?}", t.priority).to_lowercase(),
                        "assigned_to": t.assigned_to.map(|id| id.to_string()),
                    })
                })
                .collect();
            tool_result(serde_json::to_string_pretty(&items).unwrap_or_default())
        }
        Err(e) => tool_error(format!("Failed to list tasks: {e}")),
    }
}

async fn handle_update_task(state: &AppState, args: &Value) -> Value {
    let id_str = match require_str(args, "id") {
        Ok(s) => s.to_string(),
        Err(_) => return tool_error("missing required field: id"),
    };
    let mut task = match state.tasks.find_by_id(&Id::new(&id_str)).await {
        Ok(Some(t)) => t,
        Ok(None) => return tool_error(format!("Task not found: {id_str}")),
        Err(e) => return tool_error(format!("Error: {e}")),
    };
    if let Some(title) = get_str(args, "title") {
        task.title = title.to_string();
    }
    if let Some(desc) = get_str(args, "description") {
        task.description = Some(desc.to_string());
    }
    if let Some(p) = get_str(args, "priority") {
        task.priority = parse_priority(p);
    }
    if let Some(agent_id) = get_str(args, "assigned_to") {
        task.assigned_to = Some(Id::new(agent_id));
    }
    if let Some(branch) = get_str(args, "branch") {
        task.branch = Some(branch.to_string());
    }
    if let Some(pr) = get_str(args, "pr_link") {
        task.pr_link = Some(pr.to_string());
    }
    if let Some(status_str) = get_str(args, "status") {
        if let Some(new_status) = parse_status(status_str) {
            if let Err(e) = task.transition_status(new_status) {
                return tool_error(format!("Invalid status transition: {e}"));
            }
        } else {
            return tool_error(format!("Unknown status: {status_str}"));
        }
    }
    task.updated_at = now_secs();
    match state.tasks.update(&task).await {
        Ok(()) => tool_result(format!("Updated task {id_str}")),
        Err(e) => tool_error(format!("Failed to update task: {e}")),
    }
}

async fn handle_create_mr(state: &AppState, args: &Value) -> Value {
    let title = match get_str(args, "title") {
        Some(t) => t.to_string(),
        None => return tool_error("missing required field: title"),
    };
    let repo_id = match get_str(args, "repository_id") {
        Some(r) => r.to_string(),
        None => return tool_error("missing required field: repository_id"),
    };
    let source = match get_str(args, "source_branch") {
        Some(s) => s.to_string(),
        None => return tool_error("missing required field: source_branch"),
    };
    let target = match get_str(args, "target_branch") {
        Some(t) => t.to_string(),
        None => return tool_error("missing required field: target_branch"),
    };
    let now = now_secs();
    let mut mr = MergeRequest::new(
        new_id(),
        Id::new(&repo_id),
        title.clone(),
        source,
        target,
        now,
    );
    mr.author_agent_id = get_str(args, "author_agent_id").map(Id::new);
    match state.merge_requests.create(&mr).await {
        Ok(()) => tool_result(format!("Created MR '{}' (id: {})", title, mr.id)),
        Err(e) => tool_error(format!("Failed to create MR: {e}")),
    }
}

async fn handle_list_mrs(state: &AppState, args: &Value) -> Value {
    let mrs = if let Some(repo_id) = get_str(args, "repository_id") {
        state.merge_requests.list_by_repo(&Id::new(repo_id)).await
    } else {
        state.merge_requests.list().await
    };
    match mrs {
        Ok(mut list) => {
            if let Some(status_str) = get_str(args, "status") {
                list.retain(|mr| format!("{:?}", mr.status).to_lowercase() == status_str);
            }
            let items: Vec<Value> = list
                .into_iter()
                .map(|mr| {
                    json!({
                        "id": mr.id.to_string(),
                        "title": mr.title,
                        "status": format!("{:?}", mr.status).to_lowercase(),
                        "source_branch": mr.source_branch,
                        "target_branch": mr.target_branch,
                    })
                })
                .collect();
            tool_result(serde_json::to_string_pretty(&items).unwrap_or_default())
        }
        Err(e) => tool_error(format!("Failed to list MRs: {e}")),
    }
}

async fn handle_record_activity(state: &AppState, args: &Value) -> Value {
    let agent_id = match get_str(args, "agent_id") {
        Some(a) => a.to_string(),
        None => return tool_error("missing required field: agent_id"),
    };
    let event_type_str = match get_str(args, "event_type") {
        Some(e) => e,
        None => return tool_error("missing required field: event_type"),
    };
    let description = match get_str(args, "description") {
        Some(d) => d.to_string(),
        None => return tool_error("missing required field: description"),
    };
    let event = ActivityEventData {
        event_id: uuid::Uuid::new_v4().to_string(),
        agent_id,
        event_type: AgEventType::from(event_type_str),
        description,
        timestamp: now_secs(),
    };
    let event_id = event.event_id.clone();
    state.activity_store.record(event.clone());
    let _ = state.broadcast_tx.send(event);
    tool_result(format!("Recorded activity event {event_id}"))
}

async fn handle_agent_heartbeat(state: &AppState, args: &Value) -> Value {
    let agent_id = match get_str(args, "agent_id") {
        Some(a) => a.to_string(),
        None => return tool_error("missing required field: agent_id"),
    };
    match state.agents.find_by_id(&Id::new(&agent_id)).await {
        Ok(Some(mut agent)) => {
            agent.heartbeat(now_secs());
            match state.agents.update(&agent).await {
                Ok(()) => tool_result(format!("Heartbeat recorded for agent {agent_id}")),
                Err(e) => tool_error(format!("Failed to update agent: {e}")),
            }
        }
        Ok(None) => tool_error(format!("Agent not found: {agent_id}")),
        Err(e) => tool_error(format!("Error: {e}")),
    }
}

async fn handle_agent_complete(state: &AppState, args: &Value) -> Value {
    let agent_id = match get_str(args, "agent_id") {
        Some(a) => a.to_string(),
        None => return tool_error("missing required field: agent_id"),
    };
    match state.agents.find_by_id(&Id::new(&agent_id)).await {
        Ok(Some(mut agent)) => {
            use gyre_domain::AgentStatus;
            if let Err(e) = agent.transition_status(AgentStatus::Idle) {
                return tool_error(format!("Status transition failed: {e}"));
            }
            // Record completion event
            let event = ActivityEventData {
                event_id: uuid::Uuid::new_v4().to_string(),
                agent_id: agent_id.clone(),
                event_type: AgEventType::RunFinished,
                description: format!("Agent {} completed task", agent.name),
                timestamp: now_secs(),
            };
            state.activity_store.record(event.clone());
            let _ = state.broadcast_tx.send(event);
            match state.agents.update(&agent).await {
                Ok(()) => tool_result(format!("Agent {agent_id} marked complete")),
                Err(e) => tool_error(format!("Failed to update agent: {e}")),
            }
        }
        Ok(None) => tool_error(format!("Agent not found: {agent_id}")),
        Err(e) => tool_error(format!("Error: {e}")),
    }
}

async fn handle_analytics_query(state: &AppState, args: &Value) -> Value {
    let query_type = match get_str(args, "query_type") {
        Some(q) => q,
        None => return tool_error("missing required field: query_type"),
    };
    let params = args.get("params").cloned().unwrap_or(json!({}));
    let now = now_secs();

    match query_type {
        "usage" => {
            let event_name = match get_str(&params, "event_name") {
                Some(n) => n.to_string(),
                None => return tool_error("params.event_name is required for 'usage' query"),
            };
            let until = params.get("until").and_then(|v| v.as_u64()).unwrap_or(now);
            let since = params
                .get("since")
                .and_then(|v| v.as_u64())
                .unwrap_or_else(|| until.saturating_sub(86400));
            let period_len = until.saturating_sub(since);

            let count = match state.analytics.count(&event_name, since, until).await {
                Ok(c) => c,
                Err(e) => return tool_error(format!("analytics query failed: {e}")),
            };
            let events = state
                .analytics
                .query(Some(&event_name), Some(since), 10_000)
                .await
                .unwrap_or_default();
            let unique_agents = events
                .iter()
                .filter(|e| e.timestamp <= until)
                .filter_map(|e| e.agent_id.as_deref())
                .collect::<std::collections::HashSet<_>>()
                .len() as u64;

            let prev_count = state
                .analytics
                .count(&event_name, since.saturating_sub(period_len), since)
                .await
                .unwrap_or(0);
            let trend = if prev_count == 0 {
                if count > 0 {
                    "up"
                } else {
                    "flat"
                }
            } else {
                let change = (count as f64 - prev_count as f64) / prev_count as f64;
                if change > 0.10 {
                    "up"
                } else if change < -0.10 {
                    "down"
                } else {
                    "flat"
                }
            };

            tool_result(
                serde_json::to_string_pretty(&json!({
                    "event_name": event_name,
                    "count": count,
                    "unique_agents": unique_agents,
                    "trend": trend,
                }))
                .unwrap_or_default(),
            )
        }
        "compare" => {
            let event_name = match get_str(&params, "event_name") {
                Some(n) => n.to_string(),
                None => return tool_error("params.event_name is required for 'compare' query"),
            };
            let before = match params.get("before").and_then(|v| v.as_u64()) {
                Some(b) => b,
                None => return tool_error("params.before is required for 'compare' query"),
            };
            let pivot = match params.get("pivot").and_then(|v| v.as_u64()) {
                Some(p) => p,
                None => return tool_error("params.pivot is required for 'compare' query"),
            };
            let after_end = params.get("after").and_then(|v| v.as_u64()).unwrap_or(now);

            let before_count = state
                .analytics
                .count(&event_name, before, pivot)
                .await
                .unwrap_or(0);
            let after_count = state
                .analytics
                .count(&event_name, pivot, after_end)
                .await
                .unwrap_or(0);
            let change_pct = if before_count == 0 {
                None
            } else {
                Some((after_count as f64 - before_count as f64) / before_count as f64 * 100.0)
            };

            tool_result(
                serde_json::to_string_pretty(&json!({
                    "event_name": event_name,
                    "before_count": before_count,
                    "after_count": after_count,
                    "change_pct": change_pct,
                    "improved": after_count > before_count,
                }))
                .unwrap_or_default(),
            )
        }
        "top" => {
            let limit = params
                .get("limit")
                .and_then(|v| v.as_u64())
                .unwrap_or(10)
                .min(100) as usize;
            let since = params
                .get("since")
                .and_then(|v| v.as_u64())
                .unwrap_or_else(|| now.saturating_sub(86400));

            let events = state
                .analytics
                .query(None, Some(since), 100_000)
                .await
                .unwrap_or_default();
            let mut counts: std::collections::HashMap<String, u64> =
                std::collections::HashMap::new();
            for e in events {
                *counts.entry(e.event_name).or_default() += 1;
            }
            let mut entries: Vec<_> = counts
                .into_iter()
                .map(|(n, c)| json!({"event_name": n, "count": c}))
                .collect();
            entries.sort_by(|a, b| b["count"].as_u64().cmp(&a["count"].as_u64()));
            entries.truncate(limit);

            tool_result(serde_json::to_string_pretty(&json!(entries)).unwrap_or_default())
        }
        other => tool_error(format!(
            "unknown query_type: {other}. Use 'usage', 'compare', or 'top'"
        )),
    }
}

async fn handle_search(state: &AppState, args: &Value) -> Value {
    let q = match get_str(args, "q") {
        Some(q) => q.to_string(),
        None => return tool_error("missing required field: q"),
    };
    let entity_type = get_str(args, "entity_type").map(|s| s.to_string());
    let workspace_id = get_str(args, "workspace_id").map(|s| s.to_string());
    let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(10) as usize;

    match state
        .search
        .search(gyre_ports::search::SearchQuery {
            query: q.clone(),
            entity_type,
            workspace_id,
            limit,
        })
        .await
    {
        Ok(results) => {
            let text = if results.is_empty() {
                format!("No results for '{q}'")
            } else {
                let lines: Vec<String> = results
                    .iter()
                    .map(|r| {
                        format!(
                            "[{}] {} (id: {}) — {}",
                            r.entity_type, r.title, r.entity_id, r.snippet
                        )
                    })
                    .collect();
                format!("Found {} result(s):\n{}", results.len(), lines.join("\n"))
            };
            tool_result(text)
        }
        Err(e) => tool_error(format!("Search failed: {e}")),
    }
}

// ── Main MCP request dispatcher ───────────────────────────────────────────────

#[instrument(skip(state, req), fields(method = %req.method))]
pub async fn mcp_handler(
    State(state): State<Arc<AppState>>,
    Json(req): Json<JsonRpcRequest>,
) -> Json<JsonRpcResponse> {
    let id = req.id.clone();
    let response = match req.method.as_str() {
        "initialize" => JsonRpcResponse::ok(
            id,
            json!({
                "protocolVersion": PROTOCOL_VERSION,
                "serverInfo": {
                    "name": "gyre",
                    "version": "0.1.0"
                },
                "capabilities": {
                    "tools": {}
                }
            }),
        ),
        // Client sends this after init — no response body needed but we ack
        "notifications/initialized" => JsonRpcResponse::ok(id, json!(null)),
        "tools/list" => JsonRpcResponse::ok(id, tool_definitions()),
        "tools/call" => {
            let params = req.params.unwrap_or(json!({}));
            let tool_name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let args = params.get("arguments").cloned().unwrap_or(json!({}));
            let result = match tool_name {
                "gyre_create_task" => handle_create_task(&state, &args).await,
                "gyre_list_tasks" => handle_list_tasks(&state, &args).await,
                "gyre_update_task" => handle_update_task(&state, &args).await,
                "gyre_create_mr" => handle_create_mr(&state, &args).await,
                "gyre_list_mrs" => handle_list_mrs(&state, &args).await,
                "gyre_record_activity" => handle_record_activity(&state, &args).await,
                "gyre_agent_heartbeat" => handle_agent_heartbeat(&state, &args).await,
                "gyre_agent_complete" => handle_agent_complete(&state, &args).await,
                "gyre_analytics_query" => handle_analytics_query(&state, &args).await,
                "gyre_search" => handle_search(&state, &args).await,
                other => tool_error(format!("Unknown tool: {other}")),
            };
            JsonRpcResponse::ok(id, result)
        }
        other => JsonRpcResponse::err(id, METHOD_NOT_FOUND, format!("Method not found: {other}")),
    };
    Json(response)
}

/// GET /mcp/sse — SSE stream for server→client notifications.
/// Returns an open SSE connection; events are emitted as activity broadcasts arrive.
pub async fn mcp_sse_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let rx = state.broadcast_tx.subscribe();
    let event_stream = stream::unfold(rx, |mut rx| async move {
        loop {
            match rx.recv().await {
                Ok(event) => {
                    let data = serde_json::to_string(&json!({
                        "event_id": event.event_id,
                        "agent_id": event.agent_id,
                        "event_type": event.event_type,
                        "description": event.description,
                        "timestamp": event.timestamp,
                    }))
                    .unwrap_or_default();
                    let sse_event = Event::default().event("activity").data(data);
                    return Some((Ok::<Event, std::convert::Infallible>(sse_event), rx));
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return None,
            }
        }
    });
    Sse::new(event_stream).keep_alive(KeepAlive::default())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mem::test_state;
    use axum::{body::Body, Router};
    use http::{Request, StatusCode};
    use tower::ServiceExt;

    fn app() -> Router {
        crate::build_router(test_state())
    }

    async fn mcp_post(app: Router, body: Value) -> (StatusCode, Value) {
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/mcp")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = resp.status();
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: Value = serde_json::from_slice(&bytes).unwrap();
        (status, json)
    }

    #[tokio::test]
    async fn mcp_initialize() {
        let (status, json) = mcp_post(
            app(),
            json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {
                    "protocolVersion": "2024-11-05",
                    "clientInfo": { "name": "test", "version": "0.1" },
                    "capabilities": {}
                }
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["result"]["protocolVersion"], "2024-11-05");
        assert_eq!(json["result"]["serverInfo"]["name"], "gyre");
    }

    #[tokio::test]
    async fn mcp_tools_list() {
        let (status, json) = mcp_post(
            app(),
            json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "tools/list"
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let tools = json["result"]["tools"].as_array().unwrap();
        let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert!(names.contains(&"gyre_create_task"));
        assert!(names.contains(&"gyre_list_tasks"));
        assert!(names.contains(&"gyre_update_task"));
        assert!(names.contains(&"gyre_create_mr"));
        assert!(names.contains(&"gyre_list_mrs"));
        assert!(names.contains(&"gyre_record_activity"));
        assert!(names.contains(&"gyre_agent_heartbeat"));
        assert!(names.contains(&"gyre_agent_complete"));
        assert!(names.contains(&"gyre_search"));
    }

    #[tokio::test]
    async fn mcp_create_task() {
        let (status, json) = mcp_post(
            app(),
            json!({
                "jsonrpc": "2.0",
                "id": 3,
                "method": "tools/call",
                "params": {
                    "name": "gyre_create_task",
                    "arguments": {
                        "title": "Test MCP Task",
                        "priority": "high"
                    }
                }
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(json["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("Created task"));
        assert!(!json["result"]["isError"].as_bool().unwrap());
    }

    #[tokio::test]
    async fn mcp_list_tasks_empty() {
        let (status, json) = mcp_post(
            app(),
            json!({
                "jsonrpc": "2.0",
                "id": 4,
                "method": "tools/call",
                "params": {
                    "name": "gyre_list_tasks",
                    "arguments": {}
                }
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(!json["result"]["isError"].as_bool().unwrap());
    }

    #[tokio::test]
    async fn mcp_record_activity() {
        let (status, json) = mcp_post(
            app(),
            json!({
                "jsonrpc": "2.0",
                "id": 5,
                "method": "tools/call",
                "params": {
                    "name": "gyre_record_activity",
                    "arguments": {
                        "agent_id": "agent-1",
                        "event_type": "RUN_STARTED",
                        "description": "Agent started a run"
                    }
                }
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(!json["result"]["isError"].as_bool().unwrap());
    }

    #[tokio::test]
    async fn mcp_unknown_method() {
        let (status, json) = mcp_post(
            app(),
            json!({
                "jsonrpc": "2.0",
                "id": 99,
                "method": "nonexistent/method"
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(json["error"]["code"].as_i64().is_some());
        assert_eq!(json["error"]["code"], METHOD_NOT_FOUND);
    }

    #[tokio::test]
    async fn mcp_unknown_tool() {
        let (status, json) = mcp_post(
            app(),
            json!({
                "jsonrpc": "2.0",
                "id": 10,
                "method": "tools/call",
                "params": {
                    "name": "nonexistent_tool",
                    "arguments": {}
                }
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        // Unknown tool returns a tool_error result (isError: true), not an RPC error
        assert!(json["result"]["isError"].as_bool().unwrap());
    }

    #[tokio::test]
    async fn mcp_analytics_query_usage() {
        let (status, json) = mcp_post(
            app(),
            json!({
                "jsonrpc": "2.0",
                "id": 20,
                "method": "tools/call",
                "params": {
                    "name": "gyre_analytics_query",
                    "arguments": {
                        "query_type": "usage",
                        "params": { "event_name": "agent.spawned", "since": 0, "until": 9999999999u64 }
                    }
                }
            }),
        ).await;
        assert_eq!(status, StatusCode::OK);
        assert!(!json["result"]["isError"].as_bool().unwrap());
        let text = json["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("count"));
        assert!(text.contains("trend"));
    }

    #[tokio::test]
    async fn mcp_analytics_query_top() {
        let (status, json) = mcp_post(
            app(),
            json!({
                "jsonrpc": "2.0",
                "id": 21,
                "method": "tools/call",
                "params": {
                    "name": "gyre_analytics_query",
                    "arguments": {
                        "query_type": "top",
                        "params": { "limit": 5, "since": 0 }
                    }
                }
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(!json["result"]["isError"].as_bool().unwrap());
    }

    #[tokio::test]
    async fn mcp_analytics_query_compare() {
        let (status, json) = mcp_post(
            app(),
            json!({
                "jsonrpc": "2.0",
                "id": 22,
                "method": "tools/call",
                "params": {
                    "name": "gyre_analytics_query",
                    "arguments": {
                        "query_type": "compare",
                        "params": { "event_name": "mr.merged", "before": 0, "pivot": 1000, "after": 9999999999u64 }
                    }
                }
            }),
        ).await;
        assert_eq!(status, StatusCode::OK);
        assert!(!json["result"]["isError"].as_bool().unwrap());
        let text = json["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("before_count"));
        assert!(text.contains("improved"));
    }

    #[tokio::test]
    async fn mcp_tools_list_includes_analytics_query() {
        let (status, json) = mcp_post(
            app(),
            json!({
                "jsonrpc": "2.0",
                "id": 23,
                "method": "tools/list"
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let tools = json["result"]["tools"].as_array().unwrap();
        let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert!(names.contains(&"gyre_analytics_query"));
    }

    #[tokio::test]
    async fn mcp_create_task_missing_title() {
        let (status, json) = mcp_post(
            app(),
            json!({
                "jsonrpc": "2.0",
                "id": 11,
                "method": "tools/call",
                "params": {
                    "name": "gyre_create_task",
                    "arguments": {}
                }
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(json["result"]["isError"].as_bool().unwrap());
    }
}
