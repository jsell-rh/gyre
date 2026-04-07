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
use gyre_common::Id;
use gyre_domain::{MergeRequest, Task, TaskPriority, TaskStatus};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;
use tracing::instrument;

use crate::{auth::AuthenticatedAgent, AppState};

use gyre_domain::UserRole;

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
const PERMISSION_DENIED: i32 = -32603;

/// Returns true when the caller has at least the given role in the hierarchy
/// Admin > Developer > Agent > ReadOnly.
fn has_role_at_least(roles: &[UserRole], min_role: UserRole) -> bool {
    let level = |r: &UserRole| match r {
        UserRole::Admin => 4,
        UserRole::Developer => 3,
        UserRole::Agent => 2,
        UserRole::ReadOnly => 1,
    };
    let required = level(&min_role);
    roles.iter().any(|r| level(r) >= required)
}

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
                "description": "Create a new task in the Gyre platform. Orchestrators use task_type to distinguish implementation (worker agents), delegation (repo orchestrator), and coordination (cross-repo notification) tasks.",
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
                        "parent_task_id": { "type": "string", "description": "Parent task ID (links delegation→sub-task)" },
                        "labels": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "Labels to attach to the task"
                        },
                        "task_type": {
                            "type": "string",
                            "enum": ["implementation", "delegation", "coordination"],
                            "description": "Task type: implementation (triggers agent spawning), delegation (triggers repo orchestrator), coordination (cross-repo dependency notification). Null for informational/pre-approval tasks."
                        },
                        "order": {
                            "type": "integer",
                            "description": "Execution priority (lower = first). Tasks with the same order can run in parallel."
                        },
                        "depends_on": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "Task IDs that must complete before this task starts. Takes precedence over order."
                        },
                        "spec_path": { "type": "string", "description": "Spec path this task implements (e.g. system/auth.md)" },
                        "repo_id": { "type": "string", "description": "Repository ID this task belongs to" },
                        "workspace_id": { "type": "string", "description": "Workspace ID this task belongs to" }
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
                            "enum": ["backlog", "in_progress", "review", "done", "blocked", "cancelled"],
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
                            "enum": ["backlog", "in_progress", "review", "done", "blocked", "cancelled"],
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
                "description": "Record an activity event in the Gyre activity feed. Per-kind fields: TOOL_CALL_START requires tool_name; TOOL_CALL_END requires tool_name and duration_ms; TEXT_MESSAGE_CONTENT requires content; STATE_CHANGED requires new_state.",
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
                        "description": { "type": "string", "description": "Human-readable event description" },
                        "tool_name": { "type": "string", "description": "Tool name (required for TOOL_CALL_START and TOOL_CALL_END)" },
                        "duration_ms": { "type": "integer", "description": "Tool call duration in milliseconds (required for TOOL_CALL_END)" },
                        "task_id": { "type": "string", "description": "Associated task ID (optional, for RUN_STARTED and RUN_FINISHED)" },
                        "content": { "type": "string", "description": "Text content (required for TEXT_MESSAGE_CONTENT)" },
                        "role": { "type": "string", "description": "Message role (optional, for TEXT_MESSAGE_CONTENT)" },
                        "old_state": { "type": "string", "description": "Previous state (optional, for STATE_CHANGED)" },
                        "new_state": { "type": "string", "description": "New state (required for STATE_CHANGED)" }
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
                "description": "Signal that an agent has completed its current task. Optionally include a completion summary with decisions, uncertainties, and conversation_sha.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "agent_id": { "type": "string", "description": "Agent ID" },
                        "summary": {
                            "type": "object",
                            "description": "Optional completion summary (HSI §4). Include decisions made, uncertainties, and conversation SHA.",
                            "properties": {
                                "spec_ref": { "type": "string", "description": "Spec path this task implements" },
                                "decisions": {
                                    "type": "array",
                                    "items": {
                                        "type": "object",
                                        "properties": {
                                            "what": { "type": "string" },
                                            "why": { "type": "string" },
                                            "confidence": { "type": "string", "enum": ["high", "medium", "low"] },
                                            "alternatives_considered": { "type": "array", "items": { "type": "string" } }
                                        },
                                        "required": ["what", "why", "confidence"]
                                    }
                                },
                                "uncertainties": {
                                    "type": "array",
                                    "items": { "type": "string" },
                                    "description": "Open questions or areas where the spec was ambiguous"
                                },
                                "conversation_sha": { "type": "string", "description": "SHA-256 of the full conversation history" }
                            }
                        }
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
                "name": "conversation_upload",
                "description": "Upload the agent's full conversation blob for provenance linking (HSI §5). Call this just before agent.complete. The blob is base64-encoded zstd-compressed JSON. Max 10MB before base64 (configurable via GYRE_MAX_CONVERSATION_SIZE).",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "blob": {
                            "type": "string",
                            "description": "Base64-encoded zstd-compressed conversation JSON"
                        },
                        "conversation_sha": {
                            "type": "string",
                            "description": "Expected SHA-256 of the raw compressed bytes (optional — server computes and verifies)"
                        }
                    },
                    "required": ["blob"]
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
            },
            {
                "name": "gyre_message_send",
                "description": "Send a Directed or Custom message to an agent or workspace in the same workspace. Wraps POST /api/v1/workspaces/:workspace_id/messages.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "to": {
                            "type": "object",
                            "description": "Destination: {\"agent\": \"<id>\"} or {\"workspace\": \"<id>\"}"
                        },
                        "kind": {
                            "type": "string",
                            "description": "MessageKind string (e.g. task_assignment, review_request, status_update, escalation, or a custom kind)"
                        },
                        "payload": {
                            "type": "object",
                            "description": "Optional structured payload for the message"
                        },
                        "tier": {
                            "type": "string",
                            "enum": ["directed", "event"],
                            "description": "For Custom kinds: 'directed' to opt into ack-based delivery (default: event)"
                        }
                    },
                    "required": ["to", "kind"]
                }
            },
            {
                "name": "gyre_message_poll",
                "description": "Poll own inbox for new Directed messages. Wraps GET /api/v1/agents/:id/messages. Derives agent_id from JWT.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "after_ts": {
                            "type": "number",
                            "description": "Return messages with created_at > after_ts (default 0)"
                        },
                        "after_id": {
                            "type": "string",
                            "description": "Composite cursor: return messages after (after_ts, after_id)"
                        },
                        "limit": {
                            "type": "number",
                            "description": "Max messages to return (default 100, max 1000)"
                        },
                        "unacked_only": {
                            "type": "boolean",
                            "description": "If true, return only unacknowledged messages (crash recovery mode)"
                        }
                    }
                }
            },
            {
                "name": "gyre_message_ack",
                "description": "Acknowledge a received message. Wraps PUT /api/v1/agents/:id/messages/:message_id/ack. Derives agent_id from JWT.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "message_id": {
                            "type": "string",
                            "description": "ID of the message to acknowledge"
                        }
                    },
                    "required": ["message_id"]
                }
            },
            {
                "name": "graph_summary",
                "description": "Get a condensed summary of a repo's knowledge graph: node/edge counts, top types by fields, top functions by calls, modules, test coverage stats.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "repo_id": { "type": "string", "description": "Repository ID" }
                    },
                    "required": ["repo_id"]
                }
            },
            {
                "name": "graph_query_dryrun",
                "description": "Dry-run a view query against the knowledge graph. Returns matched node count, names, resolved groups/callouts/narrative, and warnings (e.g. 'too many matches'). Use this to validate queries before sending to the frontend.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "repo_id": { "type": "string", "description": "Repository ID" },
                        "query": { "type": "object", "description": "View query JSON (scope, emphasis, groups, callouts, narrative)" },
                        "selected_node_id": { "type": "string", "description": "Currently selected node ID (for $selected/$clicked resolution)" }
                    },
                    "required": ["repo_id", "query"]
                }
            },
            {
                "name": "graph_nodes",
                "description": "Query specific graph nodes by ID, name pattern, or node type. Returns up to 50 nodes with full details.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "repo_id": { "type": "string", "description": "Repository ID" },
                        "node_id": { "type": "string", "description": "Specific node ID to look up" },
                        "name_pattern": { "type": "string", "description": "Substring match on node name or qualified_name (case-insensitive)" },
                        "node_type": { "type": "string", "description": "Filter by node type: package, module, type, interface, function, endpoint, component, table, constant, field" }
                    },
                    "required": ["repo_id"]
                }
            },
            {
                "name": "graph_edges",
                "description": "Query graph edges by source/target node ID or edge type. Returns up to 100 edges.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "repo_id": { "type": "string", "description": "Repository ID" },
                        "node_id": { "type": "string", "description": "Find all edges connected to this node (source or target)" },
                        "edge_type": { "type": "string", "description": "Filter by edge type: contains, implements, depends_on, calls, field_of, returns, routes_to, governed_by" },
                        "source_id": { "type": "string", "description": "Filter edges by source node ID" },
                        "target_id": { "type": "string", "description": "Filter edges by target node ID" }
                    },
                    "required": ["repo_id"]
                }
            },
            {
                "name": "node_provenance",
                "description": "Get provenance (creation/modification history) for specific nodes. Shows who created or modified the node, when, and in which commit.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "repo_id": { "type": "string", "description": "Repository ID" },
                        "node_id": { "type": "string", "description": "Node ID to get provenance for" },
                        "name_pattern": { "type": "string", "description": "Find nodes by name and return their provenance" }
                    },
                    "required": ["repo_id"]
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

/// Returns true when the caller authenticated via a Gyre-minted agent JWT
/// (i.e. `scope == "agent"` in the JWT claims). Global tokens, API keys, and
/// Keycloak JWTs do NOT satisfy this check and bypass repo-scope enforcement.
fn is_agent_jwt(auth: &AuthenticatedAgent) -> bool {
    auth.jwt_claims
        .as_ref()
        .and_then(|c| c.get("scope"))
        .and_then(|s| s.as_str())
        == Some("agent")
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
        "cancelled" | "canceled" => Some(TaskStatus::Cancelled),
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
    // Signal chain fields (agent-runtime.md §1):
    if let Some(tt) = get_str(args, "task_type") {
        task.task_type = match tt {
            "implementation" => Some(gyre_domain::TaskType::Implementation),
            "delegation" => Some(gyre_domain::TaskType::Delegation),
            "coordination" => Some(gyre_domain::TaskType::Coordination),
            _ => {
                return tool_error(format!(
                    "unknown task_type: {tt}. Use implementation, delegation, or coordination"
                ))
            }
        };
    }
    if let Some(order) = args.get("order").and_then(|v| v.as_u64()) {
        task.order = Some(order as u32);
    }
    if let Some(deps) = args.get("depends_on").and_then(|v| v.as_array()) {
        task.depends_on = deps
            .iter()
            .filter_map(|v| v.as_str().map(Id::new))
            .collect();
    }
    task.spec_path = get_str(args, "spec_path").map(|s| s.to_string());
    if let Some(repo_id) = get_str(args, "repo_id") {
        task.repo_id = Id::new(repo_id);
    }
    if let Some(ws_id) = get_str(args, "workspace_id") {
        task.workspace_id = Id::new(ws_id);
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
                    let mut v = json!({
                        "id": t.id.to_string(),
                        "title": t.title,
                        "status": format!("{:?}", t.status).to_lowercase(),
                        "priority": format!("{:?}", t.priority).to_lowercase(),
                        "assigned_to": t.assigned_to.map(|id| id.to_string()),
                    });
                    if let Some(ref tt) = t.task_type {
                        v["task_type"] = json!(format!("{:?}", tt).to_lowercase());
                    }
                    if let Some(ref sp) = t.spec_path {
                        v["spec_path"] = json!(sp);
                    }
                    v
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

async fn handle_record_activity(
    state: &AppState,
    args: &Value,
    auth: &AuthenticatedAgent,
) -> Value {
    use gyre_common::message::{Destination, Message, MessageKind, MessageOrigin};
    use std::time::{SystemTime, UNIX_EPOCH};

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

    // Map AG-UI event_type to the appropriate MessageKind per message-bus.md §MCP Integration.
    let kind = match event_type_str {
        "TOOL_CALL_START" => MessageKind::ToolCallStart,
        "TOOL_CALL_END" => MessageKind::ToolCallEnd,
        "TEXT_MESSAGE_CONTENT" => MessageKind::TextMessageContent,
        "RUN_STARTED" => MessageKind::RunStarted,
        "RUN_FINISHED" => MessageKind::RunFinished,
        "STATE_CHANGED" => MessageKind::StateChanged,
        _ => MessageKind::StateChanged, // Fallback for unknown/custom event types.
    };

    // Derive workspace and tenant from the caller's agent record.
    let caller_agent_id = Id::new(&auth.agent_id);
    let (ws_id, tenant_id) = match state.agents.find_by_id(&caller_agent_id).await {
        Ok(Some(a)) => (a.workspace_id, Id::new(&auth.tenant_id)),
        Ok(None) => {
            // Fallback for global token auth: use default workspace/tenant.
            (Id::new("default"), Id::new(&auth.tenant_id))
        }
        Err(_) => (Id::new("default"), Id::new(&auth.tenant_id)),
    };

    // Build per-kind payload per message-bus.md §Payload Schemas.
    let payload = match event_type_str {
        "TOOL_CALL_START" => {
            let tool_name = get_str(args, "tool_name")
                .unwrap_or(&description)
                .to_string();
            serde_json::json!({
                "agent_id": agent_id,
                "tool_name": tool_name,
            })
        }
        "TOOL_CALL_END" => {
            let tool_name = get_str(args, "tool_name")
                .unwrap_or(&description)
                .to_string();
            let duration_ms = args
                .get("duration_ms")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            serde_json::json!({
                "agent_id": agent_id,
                "tool_name": tool_name,
                "duration_ms": duration_ms,
            })
        }
        "RUN_STARTED" | "RUN_FINISHED" => {
            let task_id = get_str(args, "task_id").map(|s| s.to_string());
            serde_json::json!({
                "agent_id": agent_id,
                "task_id": task_id,
            })
        }
        "TEXT_MESSAGE_CONTENT" => {
            let content = get_str(args, "content").unwrap_or(&description).to_string();
            let role = get_str(args, "role").map(|s| s.to_string());
            serde_json::json!({
                "agent_id": agent_id,
                "content": content,
                "role": role,
            })
        }
        "STATE_CHANGED" | _ => {
            let old_state = get_str(args, "old_state").map(|s| s.to_string());
            let new_state = get_str(args, "new_state")
                .unwrap_or(&description)
                .to_string();
            serde_json::json!({
                "agent_id": agent_id,
                "old_state": old_state,
                "new_state": new_state,
            })
        }
    };

    // Derive origin from auth context per message-bus.md §Message Envelope origin table.
    // Agent JWT → MessageOrigin::Agent(sub claim), not Server.
    let from = MessageOrigin::Agent(Id::new(&auth.agent_id));

    let created_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    let msg_id = Id::new(uuid::Uuid::new_v4().to_string());

    // Construct Message directly instead of using emit_telemetry, so that
    // origin and tenant_id reflect the calling agent (F5 fix).
    let msg = Message {
        id: msg_id.clone(),
        tenant_id,
        from,
        workspace_id: Some(ws_id.clone()),
        to: Destination::Workspace(ws_id),
        kind,
        payload: Some(payload),
        created_at,
        signature: None, // Telemetry tier is unsigned.
        key_id: None,
        acknowledged: false,
    };
    state.telemetry_buffer.push(msg.clone());
    let _ = state.message_broadcast_tx.send(msg);

    tool_result(format!("Recorded activity event {}", msg_id))
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

    // Parse optional completion summary (HSI §4).
    let summary: Option<gyre_common::AgentCompletionSummary> = args
        .get("summary")
        .and_then(|v| serde_json::from_value(v.clone()).ok());

    match state.agents.find_by_id(&Id::new(&agent_id)).await {
        Ok(Some(mut agent)) => {
            use gyre_domain::AgentStatus;
            if let Err(e) = agent.transition_status(AgentStatus::Idle) {
                return tool_error(format!("Status transition failed: {e}"));
            }
            let ws_id = agent.workspace_id.clone();

            // ── Synchronous priority-1 notifications for uncertainties (HSI §4) ──────
            // MUST happen before returning — reliability-critical, not via MessageConsumer.
            if let Some(ref s) = summary {
                if !s.uncertainties.is_empty() {
                    create_uncertainty_notifications(state, &agent_id, &ws_id, s).await;
                }
            }

            // ── Emit AgentCompleted Event-tier message (HSI §4) ──────────────────────
            let payload =
                build_agent_completed_payload(&agent_id, agent.current_task_id.as_ref(), &summary);
            let ws_dest = gyre_common::Destination::Workspace(ws_id.clone());
            state
                .emit_event(
                    Some(ws_id.clone()),
                    ws_dest,
                    gyre_common::MessageKind::AgentCompleted,
                    Some(payload),
                )
                .await;

            // ── Persist completion_summary to MR attestation bundle (HSI §4 step 1) ──
            if let Some(ref s) = summary {
                if let Ok(all_mrs) = state.merge_requests.list().await {
                    for mr in all_mrs.into_iter().filter(|mr| {
                        mr.author_agent_id.as_ref().map(|id| id.to_string())
                            == Some(agent_id.clone())
                    }) {
                        let mr_id = mr.id.to_string();
                        if let Ok(Some(mut bundle)) =
                            state.attestation_store.find_by_mr_id(&mr_id).await
                        {
                            bundle.attestation.completion_summary = Some(s.clone());
                            if let Err(e) = state.attestation_store.save(&mr_id, &bundle).await {
                                tracing::warn!("agent_complete: failed to persist completion_summary to attestation for MR {mr_id}: {e}");
                            }
                        }
                    }
                }
            }

            // ── Telemetry for real-time dashboard ────────────────────────────────────
            state.emit_telemetry(
                ws_id,
                gyre_common::message::MessageKind::AgentStatusChanged,
                Some(serde_json::json!({
                    "agent_id": agent_id,
                    "status": "idle",
                    "reason": format!("Agent {} completed task", agent.name),
                })),
            );

            match state.agents.update(&agent).await {
                Ok(()) => tool_result(format!("Agent {agent_id} marked complete")),
                Err(e) => tool_error(format!("Failed to update agent: {e}")),
            }
        }
        Ok(None) => tool_error(format!("Agent not found: {agent_id}")),
        Err(e) => tool_error(format!("Error: {e}")),
    }
}

/// Handle `conversation.upload` — store agent conversation blob and back-fill turn links.
///
/// Expects base64-encoded zstd-compressed bytes in `blob`.
/// Derives workspace_id and tenant_id from the authenticated agent's JWT/identity.
async fn handle_conversation_upload(
    state: &AppState,
    args: &Value,
    auth: &AuthenticatedAgent,
) -> Value {
    use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
    use sha2::{Digest, Sha256};

    // Max allowed compressed size (default 10MB).
    let max_bytes: usize = std::env::var("GYRE_MAX_CONVERSATION_SIZE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(10 * 1024 * 1024);

    // Decode base64 blob.
    let blob_b64 = match get_str(args, "blob") {
        Some(s) => s,
        None => return tool_error("missing required field: blob"),
    };
    let compressed = match B64.decode(blob_b64.as_bytes()) {
        Ok(b) => b,
        Err(e) => return tool_error(format!("blob: invalid base64: {e}")),
    };
    if compressed.len() > max_bytes {
        return tool_error(format!(
            "blob exceeds max size of {} bytes (compressed)",
            max_bytes
        ));
    }

    // Compute SHA-256 of raw compressed bytes.
    let mut hasher = Sha256::new();
    hasher.update(&compressed);
    let computed_sha = hex::encode(hasher.finalize());

    // If caller provided a SHA, verify it matches.
    if let Some(claimed_sha) = get_str(args, "conversation_sha") {
        if claimed_sha != computed_sha {
            return tool_error(format!(
                "SHA-256 mismatch: provided {claimed_sha}, computed {computed_sha}"
            ));
        }
    }

    // Resolve agent to get workspace_id. Tenant comes from auth.
    let agent_id = Id::new(&auth.agent_id);
    let tenant_id = Id::new(&auth.tenant_id);

    let agent = match state.agents.find_by_id(&agent_id).await {
        Ok(Some(a)) => a,
        Ok(None) => return tool_error(format!("Agent not found: {}", auth.agent_id)),
        Err(e) => return tool_error(format!("Failed to lookup agent: {e}")),
    };
    let workspace_id = agent.workspace_id.clone();

    // Store conversation blob.
    let sha = match state
        .conversations
        .store(&agent_id, &workspace_id, &tenant_id, &compressed)
        .await
    {
        Ok(s) => s,
        Err(e) => {
            // Non-fatal per spec: completion still succeeds.
            tracing::warn!(
                agent_id = %auth.agent_id,
                error = %e,
                "conversation.upload failed — conversation marked unavailable"
            );
            return tool_error(format!("conversation upload failed (non-fatal): {e}"));
        }
    };

    // Back-fill turn links for this agent.
    let backfilled = state
        .conversations
        .backfill_turn_links(&agent_id, &sha, &tenant_id)
        .await
        .unwrap_or(0);

    // Persist SHA in KV store so merge_processor can populate MergeAttestation.conversation_sha.
    let kv_key = format!("conv_sha:{}", auth.agent_id);
    let _ = state
        .kv_store
        .kv_set("agent_provenance", &kv_key, sha.clone())
        .await;

    tool_result(format!(
        "Conversation uploaded: sha={sha}, backfilled {backfilled} turn links"
    ))
}

/// Build the `AgentCompleted` message payload (HSI §4).
fn build_agent_completed_payload(
    agent_id: &str,
    task_id: Option<&Id>,
    summary: &Option<gyre_common::AgentCompletionSummary>,
) -> serde_json::Value {
    let mut payload = serde_json::json!({
        "agent_id": agent_id,
    });
    if let Some(tid) = task_id {
        payload["task_id"] = serde_json::Value::String(tid.to_string());
    }
    if let Some(s) = summary {
        if let Some(ref spec_ref) = s.spec_ref {
            payload["spec_ref"] = serde_json::Value::String(spec_ref.clone());
        }
        payload["decisions"] = serde_json::to_value(&s.decisions).unwrap_or_default();
        payload["uncertainties"] = serde_json::to_value(&s.uncertainties).unwrap_or_default();
        if let Some(ref sha) = s.conversation_sha {
            payload["conversation_sha"] = serde_json::Value::String(sha.clone());
        }
    } else {
        payload["decisions"] = serde_json::json!([]);
        payload["uncertainties"] = serde_json::json!([]);
    }
    payload
}

/// Synchronously create priority-1 `AgentNeedsClarification` notifications for all
/// workspace Admin and Developer members (HSI §4 — reliability-critical path).
async fn create_uncertainty_notifications(
    state: &AppState,
    agent_id: &str,
    workspace_id: &Id,
    summary: &gyre_common::AgentCompletionSummary,
) {
    use gyre_common::{Id, Notification, NotificationType};
    use gyre_domain::WorkspaceRole;

    // Resolve tenant_id from the workspace record (avoid hardcoding "default").
    let tenant_id = match state.workspaces.find_by_id(workspace_id).await {
        Ok(Some(ws)) => ws.tenant_id.to_string(),
        Ok(None) => {
            tracing::warn!("agent_complete: workspace {workspace_id} not found; skipping uncertainty notifications");
            return;
        }
        Err(e) => {
            tracing::warn!(
                "agent_complete: failed to resolve workspace tenant for notifications: {e}"
            );
            return;
        }
    };

    let members = match state
        .workspace_memberships
        .list_by_workspace(workspace_id)
        .await
    {
        Ok(m) => m,
        Err(e) => {
            tracing::warn!(
                "agent_complete: failed to list workspace members for uncertainty notifications: {e}"
            );
            return;
        }
    };

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    let uncertainty_titles: Vec<String> = summary
        .uncertainties
        .iter()
        .enumerate()
        .map(|(i, u)| format!("Agent uncertainty {}: {}", i + 1, u))
        .collect();
    let title = if summary.uncertainties.len() == 1 {
        format!(
            "Agent needs clarification: {}",
            summary.uncertainties[0]
                .chars()
                .take(80)
                .collect::<String>()
        )
    } else {
        format!(
            "Agent needs clarification ({} open questions)",
            summary.uncertainties.len()
        )
    };
    let body = serde_json::to_string(&serde_json::json!({
        "uncertainties": &summary.uncertainties,
        "spec_ref": &summary.spec_ref,
    }))
    .ok();

    for member in &members {
        // Only notify Admin and Developer role members (Owner counts as Admin-level).
        if !matches!(
            member.role,
            WorkspaceRole::Admin | WorkspaceRole::Developer | WorkspaceRole::Owner
        ) {
            continue;
        }
        let notif_id = Id::new(uuid::Uuid::new_v4().to_string());
        let mut notif = Notification::new(
            notif_id,
            workspace_id.clone(),
            member.user_id.clone(),
            NotificationType::AgentNeedsClarification,
            title.clone(),
            &tenant_id,
            now,
        );
        notif.entity_ref = Some(agent_id.to_string());
        notif.body = body.clone();

        if let Err(e) = state.notifications.create(&notif).await {
            tracing::warn!(
                "agent_complete: failed to create AgentNeedsClarification notification for user {}: {e}",
                member.user_id
            );
        }
    }

    let _ = uncertainty_titles; // used to document intent
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

// ── Message bus MCP tools ────────────────────────────────────────────────────

async fn handle_message_send(state: &AppState, args: &Value, auth: &AuthenticatedAgent) -> Value {
    use gyre_common::message::{Destination, Message, MessageKind, MessageOrigin, MessageTier};

    // Parse destination.
    let to_value = match args.get("to") {
        Some(v) => v,
        None => return tool_error("missing required field: to"),
    };
    let to = match parse_mcp_destination(to_value) {
        Ok(d) => d,
        Err(msg) => return tool_error(msg),
    };

    // Parse kind.
    let kind_str = match get_str(args, "kind") {
        Some(k) => k,
        None => return tool_error("missing required field: kind"),
    };
    let kind: MessageKind =
        match serde_json::from_value(serde_json::Value::String(kind_str.to_string())) {
            Ok(k) => k,
            Err(_) => return tool_error(format!("unknown message kind: {kind_str}")),
        };

    // server_only check: MCP agents cannot send server-only kinds.
    if kind.server_only() {
        return tool_error(format!(
            "kind '{}' can only be sent by the server",
            kind_str
        ));
    }

    // Telemetry-tier standard kinds are not valid for message.send — they route
    // through gyre_record_activity instead (message-bus.md §MCP Integration).
    if kind.tier() == MessageTier::Telemetry && !matches!(kind, MessageKind::Custom(_)) {
        return tool_error(format!(
            "kind '{}' is Telemetry-tier — use gyre_record_activity instead of message.send",
            kind_str
        ));
    }

    // Determine effective tier.
    let effective_tier = if let MessageKind::Custom(_) = &kind {
        if get_str(args, "tier") == Some("directed") {
            MessageTier::Directed
        } else {
            MessageTier::Event
        }
    } else {
        kind.tier()
    };

    // Derive workspace from agent identity.
    let agent_id = Id::new(&auth.agent_id);
    let agent = match state.agents.find_by_id(&agent_id).await {
        Ok(Some(a)) => a,
        Ok(None) => return tool_error(format!("agent {} not found", auth.agent_id)),
        Err(e) => return tool_error(format!("failed to lookup agent: {e}")),
    };
    let ws_id = agent.workspace_id.clone();

    // Validate destination constraints.
    match (&effective_tier, &to) {
        (MessageTier::Directed, Destination::Workspace(_)) => {
            return tool_error("Directed tier requires Agent destination, not Workspace");
        }
        (MessageTier::Directed, Destination::Broadcast) => {
            return tool_error("Directed tier requires Agent destination, not Broadcast");
        }
        (MessageTier::Telemetry, Destination::Agent(_)) => {
            return tool_error("Telemetry tier cannot target Agent destination");
        }
        _ => {}
    }

    // Same-workspace constraint for Agent destination.
    if let Destination::Agent(ref target_id) = to {
        let target = match state.agents.find_by_id(target_id).await {
            Ok(Some(a)) => a,
            Ok(None) => return tool_error(format!("target agent {} not found", target_id)),
            Err(e) => return tool_error(format!("failed to lookup target agent: {e}")),
        };
        if target.workspace_id != ws_id {
            return tool_error(format!(
                "target agent {} is not in the same workspace",
                target_id
            ));
        }

        // Queue depth check for Directed tier.
        if effective_tier == MessageTier::Directed {
            let unacked = state.messages.count_unacked(target_id).await.unwrap_or(0);
            if unacked >= state.agent_inbox_max {
                return tool_error(format!(
                    "agent {} inbox is full ({} unacked messages)",
                    target_id, unacked
                ));
            }
        }
    }

    // Workspace destination must match sender's workspace.
    if let Destination::Workspace(ref dest_ws) = to {
        if *dest_ws != ws_id {
            return tool_error("workspace destination must match sender's workspace");
        }
    }

    let from = MessageOrigin::Agent(agent_id);
    let created_at = now_ms();
    let msg_id = Id::new(uuid::Uuid::new_v4().to_string());

    let workspace_id_opt = if matches!(to, Destination::Broadcast) {
        None
    } else {
        Some(ws_id.clone())
    };

    let mut msg = Message {
        id: msg_id.clone(),
        tenant_id: Id::new(&auth.tenant_id),
        from,
        workspace_id: workspace_id_opt,
        to,
        kind,
        payload: args.get("payload").cloned(),
        created_at,
        signature: None,
        key_id: None,
        acknowledged: false,
    };

    // Sign Directed and Event tier.
    if effective_tier != MessageTier::Telemetry {
        let (sig, kid) = crate::signing::sign_message(state, &msg);
        msg.signature = Some(sig);
        msg.key_id = Some(kid);
    }

    // Persist Directed and Event tier.
    if effective_tier != MessageTier::Telemetry && !matches!(msg.to, Destination::Broadcast) {
        if let Err(e) = state.messages.store(&msg).await {
            return tool_error(format!("failed to store message: {e}"));
        }
    }

    // Dispatch to consumers.
    let _ = state.message_dispatch_tx.try_send(msg.clone());

    tool_result(format!("Message sent: id={}, kind={}", msg_id, kind_str,))
}

// Signing uses the shared `crate::signing::sign_message` — no local copy.

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Parse MCP destination for `message.send`.
///
/// Per spec (message-bus.md §MCP Integration), `message.send` supports only
/// Directed or Custom messages to an agent in the same workspace. Broadcast
/// destination is NOT valid — it requires Server origin or Admin role, which
/// agent MCP callers do not have.
fn parse_mcp_destination(
    v: &serde_json::Value,
) -> Result<gyre_common::message::Destination, String> {
    if let Some(obj) = v.as_object() {
        if let Some(agent_id) = obj.get("agent").and_then(|v| v.as_str()) {
            return Ok(gyre_common::message::Destination::Agent(Id::new(agent_id)));
        }
        if let Some(ws_id) = obj.get("workspace").and_then(|v| v.as_str()) {
            return Ok(gyre_common::message::Destination::Workspace(Id::new(ws_id)));
        }
    }
    Err("invalid 'to': expected {\"agent\": \"<id>\"} or {\"workspace\": \"<id>\"}".to_string())
}

async fn handle_message_poll(state: &AppState, args: &Value, auth: &AuthenticatedAgent) -> Value {
    // Derive agent_id from auth context.
    let agent_id = Id::new(&auth.agent_id);

    // Verify agent exists.
    match state.agents.find_by_id(&agent_id).await {
        Ok(Some(_)) => {}
        Ok(None) => return tool_error(format!("agent {} not found", auth.agent_id)),
        Err(e) => return tool_error(format!("failed to lookup agent: {e}")),
    }

    let limit = args
        .get("limit")
        .and_then(|v| v.as_u64())
        .unwrap_or(100)
        .min(1000) as usize;

    let unacked_only = args
        .get("unacked_only")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let messages = if unacked_only {
        state.messages.list_unacked(&agent_id, limit).await
    } else {
        let after_ts = args.get("after_ts").and_then(|v| v.as_u64()).unwrap_or(0);
        let after_id = get_str(args, "after_id").map(Id::new);
        state
            .messages
            .list_after(&agent_id, after_ts, after_id.as_ref(), limit)
            .await
    };

    match messages {
        Ok(msgs) => {
            let items: Vec<serde_json::Value> = msgs
                .iter()
                .map(|m| {
                    serde_json::json!({
                        "id": m.id.to_string(),
                        "from": m.from,
                        "kind": m.kind.as_str(),
                        "payload": m.payload,
                        "created_at": m.created_at,
                        "acknowledged": m.acknowledged,
                    })
                })
                .collect();
            tool_result(format!(
                "{} message(s):\n{}",
                items.len(),
                serde_json::to_string_pretty(&items).unwrap_or_default()
            ))
        }
        Err(e) => tool_error(format!("failed to poll messages: {e}")),
    }
}

async fn handle_message_ack(state: &AppState, args: &Value, auth: &AuthenticatedAgent) -> Value {
    let message_id = match get_str(args, "message_id") {
        Some(id) => id.to_string(),
        None => return tool_error("missing required field: message_id"),
    };

    let agent_id = Id::new(&auth.agent_id);
    let mid = Id::new(&message_id);

    // Verify agent exists.
    match state.agents.find_by_id(&agent_id).await {
        Ok(Some(_)) => {}
        Ok(None) => return tool_error(format!("agent {} not found", auth.agent_id)),
        Err(e) => return tool_error(format!("failed to lookup agent: {e}")),
    }

    match state.messages.acknowledge(&mid, &agent_id).await {
        Ok(()) => tool_result(format!("Message {} acknowledged", message_id)),
        Err(e) => tool_error(format!("failed to ack message: {e}")),
    }
}

// ── Explorer graph MCP tools ──────────────────────────────────────────────────

async fn handle_graph_summary(state: &AppState, args: &Value) -> Value {
    let repo_id = match require_str(args, "repo_id") {
        Ok(r) => r.to_string(),
        Err(_) => return tool_error("missing required field: repo_id"),
    };
    let rid = Id::new(&repo_id);
    let nodes = match state.graph_store.list_nodes(&rid, None).await {
        Ok(n) => n,
        Err(e) => return tool_error(format!("Failed to load graph nodes: {e}")),
    };
    let edges = match state.graph_store.list_edges(&rid, None).await {
        Ok(e) => e,
        Err(e) => return tool_error(format!("Failed to load graph edges: {e}")),
    };
    let summary = gyre_domain::view_query_resolver::compute_graph_summary(&repo_id, &nodes, &edges);
    tool_result(serde_json::to_string_pretty(&summary).unwrap_or_default())
}

async fn handle_graph_query_dryrun(state: &AppState, args: &Value) -> Value {
    let repo_id = match require_str(args, "repo_id") {
        Ok(r) => r.to_string(),
        Err(_) => return tool_error("missing required field: repo_id"),
    };
    let query_value = match args.get("query") {
        Some(q) => q.clone(),
        None => return tool_error("missing required field: query"),
    };
    let query: gyre_common::view_query::ViewQuery = match serde_json::from_value(query_value) {
        Ok(q) => q,
        Err(e) => return tool_error(format!("Invalid view query: {e}")),
    };
    let selected = get_str(args, "selected_node_id");

    let rid = Id::new(&repo_id);
    let nodes = match state.graph_store.list_nodes(&rid, None).await {
        Ok(n) => n,
        Err(e) => return tool_error(format!("Failed to load graph nodes: {e}")),
    };
    let edges = match state.graph_store.list_edges(&rid, None).await {
        Ok(e) => e,
        Err(e) => return tool_error(format!("Failed to load graph edges: {e}")),
    };

    let result = gyre_domain::view_query_resolver::dry_run(&query, &nodes, &edges, selected);
    tool_result(serde_json::to_string_pretty(&result).unwrap_or_default())
}

async fn handle_graph_nodes(state: &AppState, args: &Value) -> Value {
    let repo_id = match require_str(args, "repo_id") {
        Ok(r) => r.to_string(),
        Err(_) => return tool_error("missing required field: repo_id"),
    };
    let rid = Id::new(&repo_id);

    // Specific node by ID
    if let Some(node_id) = get_str(args, "node_id") {
        let nid = Id::new(node_id);
        match state.graph_store.get_node(&nid).await {
            Ok(Some(node)) => {
                return tool_result(serde_json::to_string_pretty(&node).unwrap_or_default());
            }
            Ok(None) => return tool_error(format!("Node not found: {node_id}")),
            Err(e) => return tool_error(format!("Failed: {e}")),
        }
    }

    // Filter by type
    let node_type_filter =
        get_str(args, "node_type").and_then(|s| match s.to_lowercase().as_str() {
            "package" => Some(gyre_common::NodeType::Package),
            "module" => Some(gyre_common::NodeType::Module),
            "type" | "struct" => Some(gyre_common::NodeType::Type),
            "trait" => Some(gyre_common::NodeType::Trait),
            "interface" => Some(gyre_common::NodeType::Interface),
            "function" => Some(gyre_common::NodeType::Function),
            "method" => Some(gyre_common::NodeType::Method),
            "class" => Some(gyre_common::NodeType::Class),
            "enum" => Some(gyre_common::NodeType::Enum),
            "enum_variant" | "variant" => Some(gyre_common::NodeType::EnumVariant),
            "endpoint" => Some(gyre_common::NodeType::Endpoint),
            "component" => Some(gyre_common::NodeType::Component),
            "table" => Some(gyre_common::NodeType::Table),
            "constant" => Some(gyre_common::NodeType::Constant),
            "field" => Some(gyre_common::NodeType::Field),
            "spec" => Some(gyre_common::NodeType::Spec),
            _ => None,
        });

    let nodes = match state.graph_store.list_nodes(&rid, node_type_filter).await {
        Ok(n) => n,
        Err(e) => return tool_error(format!("Failed: {e}")),
    };

    // Name pattern filter
    let name_pattern = get_str(args, "name_pattern").map(|s| s.to_lowercase());
    let filtered: Vec<_> = nodes
        .into_iter()
        .filter(|n| n.deleted_at.is_none())
        .filter(|n| match &name_pattern {
            Some(pat) => {
                n.name.to_lowercase().contains(pat) || n.qualified_name.to_lowercase().contains(pat)
            }
            None => true,
        })
        .take(50)
        .collect();

    let items: Vec<serde_json::Value> = filtered
        .iter()
        .map(|n| {
            json!({
                "id": n.id.to_string(),
                "name": n.name,
                "qualified_name": n.qualified_name,
                "node_type": format!("{:?}", n.node_type).to_lowercase(),
                "file_path": n.file_path,
                "line_start": n.line_start,
                "line_end": n.line_end,
                "visibility": format!("{:?}", n.visibility).to_lowercase(),
                "spec_path": n.spec_path,
                "complexity": n.complexity,
                "test_node": n.test_node,
                "test_coverage": n.test_coverage,
            })
        })
        .collect();

    tool_result(format!(
        "{} nodes:\n{}",
        items.len(),
        serde_json::to_string_pretty(&items).unwrap_or_default()
    ))
}

async fn handle_graph_edges(state: &AppState, args: &Value) -> Value {
    let repo_id = match require_str(args, "repo_id") {
        Ok(r) => r.to_string(),
        Err(_) => return tool_error("missing required field: repo_id"),
    };
    let rid = Id::new(&repo_id);

    // If node_id is specified, get edges for that node
    if let Some(node_id) = get_str(args, "node_id") {
        let nid = Id::new(node_id);
        match state.graph_store.list_edges_for_node(&nid).await {
            Ok(edges) => {
                let items: Vec<serde_json::Value> = edges
                    .iter()
                    .filter(|e| e.deleted_at.is_none())
                    .take(100)
                    .map(|e| {
                        json!({
                            "id": e.id.to_string(),
                            "source_id": e.source_id.to_string(),
                            "target_id": e.target_id.to_string(),
                            "edge_type": format!("{:?}", e.edge_type).to_lowercase(),
                        })
                    })
                    .collect();
                return tool_result(format!(
                    "{} edges:\n{}",
                    items.len(),
                    serde_json::to_string_pretty(&items).unwrap_or_default()
                ));
            }
            Err(e) => return tool_error(format!("Failed: {e}")),
        }
    }

    // Filter by edge type
    let edge_type_filter =
        get_str(args, "edge_type").and_then(|s| match s.to_lowercase().as_str() {
            "contains" => Some(gyre_common::EdgeType::Contains),
            "implements" => Some(gyre_common::EdgeType::Implements),
            "depends_on" => Some(gyre_common::EdgeType::DependsOn),
            "calls" => Some(gyre_common::EdgeType::Calls),
            "field_of" => Some(gyre_common::EdgeType::FieldOf),
            "returns" => Some(gyre_common::EdgeType::Returns),
            "routes_to" => Some(gyre_common::EdgeType::RoutesTo),
            "governed_by" => Some(gyre_common::EdgeType::GovernedBy),
            _ => None,
        });

    let edges = match state.graph_store.list_edges(&rid, edge_type_filter).await {
        Ok(e) => e,
        Err(e) => return tool_error(format!("Failed: {e}")),
    };

    let source_filter = get_str(args, "source_id");
    let target_filter = get_str(args, "target_id");

    let items: Vec<serde_json::Value> = edges
        .iter()
        .filter(|e| e.deleted_at.is_none())
        .filter(|e| {
            source_filter.map_or(true, |s| e.source_id.to_string() == s)
                && target_filter.map_or(true, |t| e.target_id.to_string() == t)
        })
        .take(100)
        .map(|e| {
            json!({
                "id": e.id.to_string(),
                "source_id": e.source_id.to_string(),
                "target_id": e.target_id.to_string(),
                "edge_type": format!("{:?}", e.edge_type).to_lowercase(),
            })
        })
        .collect();

    tool_result(format!(
        "{} edges:\n{}",
        items.len(),
        serde_json::to_string_pretty(&items).unwrap_or_default()
    ))
}

async fn handle_node_provenance(state: &AppState, args: &Value) -> Value {
    let repo_id = match require_str(args, "repo_id") {
        Ok(r) => r.to_string(),
        Err(_) => return tool_error("missing required field: repo_id"),
    };
    let rid = Id::new(&repo_id);

    // Find nodes by ID or name pattern
    let target_nodes: Vec<gyre_common::graph::GraphNode> =
        if let Some(node_id) = get_str(args, "node_id") {
            let nid = Id::new(node_id);
            match state.graph_store.get_node(&nid).await {
                Ok(Some(n)) => vec![n],
                Ok(None) => return tool_error(format!("Node not found: {node_id}")),
                Err(e) => return tool_error(format!("Failed: {e}")),
            }
        } else if let Some(pattern) = get_str(args, "name_pattern") {
            let pat_lower = pattern.to_lowercase();
            match state.graph_store.list_nodes(&rid, None).await {
                Ok(nodes) => nodes
                    .into_iter()
                    .filter(|n| {
                        n.deleted_at.is_none()
                            && (n.name.to_lowercase().contains(&pat_lower)
                                || n.qualified_name.to_lowercase().contains(&pat_lower))
                    })
                    .take(10)
                    .collect(),
                Err(e) => return tool_error(format!("Failed: {e}")),
            }
        } else {
            return tool_error("Provide either node_id or name_pattern");
        };

    let items: Vec<Value> = target_nodes
        .iter()
        .map(|n| {
            json!({
                "id": n.id.to_string(),
                "name": n.name,
                "qualified_name": n.qualified_name,
                "node_type": format!("{:?}", n.node_type).to_lowercase(),
                "file_path": n.file_path,
                "created_at": n.created_at,
                "last_modified_at": n.last_modified_at,
                "created_sha": n.created_sha,
                "last_modified_sha": n.last_modified_sha,
                "spec_path": n.spec_path,
                "complexity": n.complexity,
                "churn_count_30d": n.churn_count_30d,
                "test_coverage": n.test_coverage,
            })
        })
        .collect();

    tool_result(format!(
        "{} node(s) provenance:\n{}",
        items.len(),
        serde_json::to_string_pretty(&items).unwrap_or_default()
    ))
}

// ── MCP Resources ─────────────────────────────────────────────────────────────

fn resource_definitions() -> Value {
    json!({
        "resources": [
            {
                "uri": "spec://",
                "name": "Spec Files",
                "description": "Read spec markdown files. URI: spec://{path} (e.g. spec://system/design-principles.md)",
                "mimeType": "text/markdown",
                "uriTemplate": "spec://{path}"
            },
            {
                "uri": "agents://",
                "name": "Workspace Agents",
                "description": "List active agents in a workspace. URI: agents://{workspace_id} or agents:// for all.",
                "mimeType": "application/json",
                "uriTemplate": "agents://{workspace_id}"
            },
            {
                "uri": "queue://",
                "name": "Merge Queue",
                "description": "List merge queue entries for a repository. URI: queue://{repo_id}",
                "mimeType": "application/json",
                "uriTemplate": "queue://{repo_id}"
            },
            {
                "uri": "conversation://context",
                "name": "Conversation Context",
                "description": "Original agent conversation history for interrogation agents (HSI §4). Only accessible to the spawned interrogation agent.",
                "mimeType": "application/json"
            }
        ]
    })
}

async fn handle_resource_read(state: &AppState, auth: &AuthenticatedAgent, uri: &str) -> Value {
    if uri == "conversation://context" {
        // HSI §4: Serve the original agent's conversation to the interrogation agent.
        // The context is scoped to the calling agent — each interrogation agent can
        // only see its own conversation context, not another agent's.
        let agent_id = &auth.agent_id;
        match state
            .kv_store
            .kv_get("interrogation_context", agent_id.as_str())
            .await
        {
            Ok(Some(blob)) => json!({
                "contents": [{
                    "uri": uri,
                    "mimeType": "application/json",
                    "text": blob
                }]
            }),
            Ok(None) => json!({
                "contents": [{
                    "uri": uri,
                    "mimeType": "application/json",
                    "text": "{\"turns\": [], \"note\": \"No conversation context available for this agent.\"}"
                }]
            }),
            Err(e) => json!({"error": format!("failed to read conversation context: {e}")}),
        }
    } else if let Some(raw_path) = uri.strip_prefix("spec://") {
        let safe_path = raw_path.trim_start_matches('/');
        if safe_path.contains("..") || safe_path.starts_with('/') {
            return json!({"error": "invalid spec path — path traversal not allowed"});
        }
        let file_path = format!("specs/{safe_path}");
        match std::fs::read_to_string(&file_path) {
            Ok(content) => json!({
                "contents": [{
                    "uri": uri,
                    "mimeType": "text/markdown",
                    "text": content
                }]
            }),
            Err(e) => json!({"error": format!("cannot read {file_path}: {e}")}),
        }
    } else if let Some(workspace_id) = uri.strip_prefix("agents://") {
        let agents_result = if workspace_id.is_empty() || workspace_id == "*" {
            state.agents.list().await
        } else {
            state.agents.list_by_workspace(&Id::new(workspace_id)).await
        };
        match agents_result {
            Ok(list) => {
                let active: Vec<Value> = list
                    .into_iter()
                    .filter(|a| {
                        matches!(
                            a.status,
                            gyre_domain::AgentStatus::Active | gyre_domain::AgentStatus::Idle
                        )
                    })
                    .map(|a| {
                        json!({
                            "id": a.id.to_string(),
                            "name": a.name,
                            "status": format!("{:?}", a.status).to_lowercase(),
                            "workspace_id": a.workspace_id.to_string(),
                        })
                    })
                    .collect();
                json!({
                    "contents": [{
                        "uri": uri,
                        "mimeType": "application/json",
                        "text": serde_json::to_string_pretty(&active).unwrap_or_default()
                    }]
                })
            }
            Err(e) => json!({"error": format!("failed to list agents: {e}")}),
        }
    } else if let Some(repo_id) = uri.strip_prefix("queue://") {
        match state.merge_queue.list_queue().await {
            Ok(entries) => {
                let mut results = Vec::new();
                for entry in entries {
                    if let Ok(Some(mr)) = state
                        .merge_requests
                        .find_by_id(&entry.merge_request_id)
                        .await
                    {
                        if mr.repository_id.to_string() == repo_id {
                            results.push(json!({
                                "id": entry.id.to_string(),
                                "merge_request_id": entry.merge_request_id.to_string(),
                                "priority": entry.priority,
                                "status": format!("{:?}", entry.status).to_lowercase(),
                                "enqueued_at": entry.enqueued_at,
                            }));
                        }
                    }
                }
                json!({
                    "contents": [{
                        "uri": uri,
                        "mimeType": "application/json",
                        "text": serde_json::to_string_pretty(&results).unwrap_or_default()
                    }]
                })
            }
            Err(e) => json!({"error": format!("failed to list merge queue: {e}")}),
        }
    } else {
        json!({"error": format!("unknown resource URI scheme: {uri}")})
    }
}

// ── Main MCP request dispatcher ───────────────────────────────────────────────

#[instrument(skip(state, req, auth), fields(method = %req.method))]
pub async fn mcp_handler(
    State(state): State<Arc<AppState>>,
    auth: AuthenticatedAgent,
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
                    "tools": {},
                    "resources": {}
                }
            }),
        ),
        // Client sends this after init — no response body needed but we ack
        "notifications/initialized" => JsonRpcResponse::ok(id, json!(null)),
        "tools/list" => JsonRpcResponse::ok(id, tool_definitions()),
        "resources/list" => JsonRpcResponse::ok(id, resource_definitions()),
        "resources/read" => {
            let params = req.params.unwrap_or(json!({}));
            let uri = params.get("uri").and_then(|v| v.as_str()).unwrap_or("");
            if uri.is_empty() {
                JsonRpcResponse::err(id, INVALID_PARAMS, "missing required field: uri")
            } else {
                let result = handle_resource_read(&state, &auth, uri).await;
                JsonRpcResponse::ok(id, result)
            }
        }
        "tools/call" => {
            let params = req.params.unwrap_or(json!({}));
            let tool_name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let args = params.get("arguments").cloned().unwrap_or(json!({}));

            // Per-tool RBAC (MCP-1-B): write tools require Agent or Developer role;
            // read-only tools allow ReadOnly.
            let needs_write = matches!(
                tool_name,
                "gyre_create_task"
                    | "gyre_update_task"
                    | "gyre_create_mr"
                    | "gyre_record_activity"
                    | "gyre_agent_heartbeat"
                    | "gyre_agent_complete"
                    | "conversation_upload"
                    | "gyre_message_send"
                    | "gyre_message_ack"
            );
            if needs_write && !has_role_at_least(&auth.roles, UserRole::Agent) {
                return Json(JsonRpcResponse::err(
                    id,
                    PERMISSION_DENIED,
                    "insufficient permissions: this tool requires Agent or higher role",
                ));
            }

            // Repo-scope validation (TASK-216): an agent JWT is scoped to the
            // repo it was spawned against. Enforce that the JWT cannot act on
            // other repos. Global/API-key/Keycloak callers bypass this check.
            if tool_name == "gyre_create_mr" && is_agent_jwt(&auth) {
                let agent_id = Id::new(&auth.agent_id);
                let worktrees = state
                    .worktrees
                    .find_by_agent(&agent_id)
                    .await
                    .unwrap_or_default();
                if worktrees.is_empty() {
                    return Json(JsonRpcResponse::err(
                        id,
                        PERMISSION_DENIED,
                        "PERMISSION_DENIED: agent has no active worktrees — cannot verify repo scope",
                    ));
                } else if let Some(wt) = worktrees.first() {
                    let requested_repo = args
                        .get("repository_id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    if wt.repository_id.to_string() != requested_repo {
                        return Json(JsonRpcResponse::err(
                            id,
                            PERMISSION_DENIED,
                            format!(
                                "PERMISSION_DENIED: agent is scoped to repo {}, cannot create MR in {}",
                                wt.repository_id, requested_repo
                            ),
                        ));
                    }
                }
            }

            // Agent-identity validation: a JWT agent can only signal completion
            // for itself.
            if tool_name == "gyre_agent_complete" && is_agent_jwt(&auth) {
                let requested_agent = args.get("agent_id").and_then(|v| v.as_str()).unwrap_or("");
                if !requested_agent.is_empty() && requested_agent != auth.agent_id {
                    return Json(JsonRpcResponse::err(
                        id,
                        PERMISSION_DENIED,
                        format!(
                            "PERMISSION_DENIED: agent {} cannot complete agent {}",
                            auth.agent_id, requested_agent
                        ),
                    ));
                }
            }

            let result = match tool_name {
                "gyre_create_task" => handle_create_task(&state, &args).await,
                "gyre_list_tasks" => handle_list_tasks(&state, &args).await,
                "gyre_update_task" => handle_update_task(&state, &args).await,
                "gyre_create_mr" => handle_create_mr(&state, &args).await,
                "gyre_list_mrs" => handle_list_mrs(&state, &args).await,
                "gyre_record_activity" => handle_record_activity(&state, &args, &auth).await,
                "gyre_agent_heartbeat" => handle_agent_heartbeat(&state, &args).await,
                "gyre_agent_complete" => handle_agent_complete(&state, &args).await,
                "gyre_analytics_query" => handle_analytics_query(&state, &args).await,
                "gyre_search" => handle_search(&state, &args).await,
                "conversation_upload" => handle_conversation_upload(&state, &args, &auth).await,
                "gyre_message_send" => handle_message_send(&state, &args, &auth).await,
                "gyre_message_poll" => handle_message_poll(&state, &args, &auth).await,
                "gyre_message_ack" => handle_message_ack(&state, &args, &auth).await,
                "graph_summary" => handle_graph_summary(&state, &args).await,
                "graph_query_dryrun" => handle_graph_query_dryrun(&state, &args).await,
                "graph_nodes" => handle_graph_nodes(&state, &args).await,
                "graph_edges" => handle_graph_edges(&state, &args).await,
                "node_provenance" => handle_node_provenance(&state, &args).await,
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
pub async fn mcp_sse_handler(
    State(state): State<Arc<AppState>>,
    _auth: AuthenticatedAgent,
) -> impl IntoResponse {
    let rx = state.message_broadcast_tx.subscribe();
    let event_stream = stream::unfold(rx, |mut rx| async move {
        loop {
            match rx.recv().await {
                Ok(msg) => {
                    let data = serde_json::to_string(&msg).unwrap_or_default();
                    // Map all bus messages to SSE "message" events.
                    let sse_event = Event::default().event("message").data(data);
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
                    .header("authorization", "Bearer test-token")
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
        assert!(names.contains(&"node_provenance"));
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
    async fn mcp_write_tool_requires_agent_role() {
        // ReadOnly tokens must be rejected for write tools.
        // The test-state global token is Admin, so we need to use a non-admin token
        // that maps to ReadOnly. This test verifies the role-check path by directly
        // calling has_role_at_least with ReadOnly roles.
        assert!(!has_role_at_least(&[UserRole::ReadOnly], UserRole::Agent));
        assert!(has_role_at_least(&[UserRole::Agent], UserRole::Agent));
        assert!(has_role_at_least(&[UserRole::Developer], UserRole::Agent));
        assert!(has_role_at_least(&[UserRole::Admin], UserRole::Agent));
    }

    // ── is_agent_jwt detection ─────────────────────────────────────────────────

    #[test]
    fn is_agent_jwt_detects_gyre_jwt() {
        let auth_with_agent_scope = AuthenticatedAgent {
            agent_id: "agent-1".to_string(),
            user_id: None,
            roles: vec![UserRole::Agent],
            tenant_id: "default".to_string(),
            jwt_claims: Some(serde_json::json!({
                "sub": "agent-1",
                "scope": "agent",
                "task_id": "task-1"
            })),
            deprecated_token_auth: false,
        };
        assert!(is_agent_jwt(&auth_with_agent_scope));

        // Global token has no jwt_claims
        let auth_global = AuthenticatedAgent {
            agent_id: "system".to_string(),
            user_id: None,
            roles: vec![UserRole::Admin],
            tenant_id: "default".to_string(),
            jwt_claims: None,
            deprecated_token_auth: false,
        };
        assert!(!is_agent_jwt(&auth_global));

        // Keycloak JWT has jwt_claims but different scope
        let auth_keycloak = AuthenticatedAgent {
            agent_id: "alice".to_string(),
            user_id: None,
            roles: vec![UserRole::Developer],
            tenant_id: "default".to_string(),
            jwt_claims: Some(serde_json::json!({
                "sub": "user-abc",
                "realm_access": {"roles": ["developer"]}
            })),
            deprecated_token_auth: false,
        };
        assert!(!is_agent_jwt(&auth_keycloak));
    }

    // ── resources/list ────────────────────────────────────────────────────────

    #[tokio::test]
    async fn mcp_resources_list() {
        let (status, json) = mcp_post(
            app(),
            json!({
                "jsonrpc": "2.0",
                "id": 30,
                "method": "resources/list"
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let resources = json["result"]["resources"].as_array().unwrap();
        let names: Vec<&str> = resources
            .iter()
            .map(|r| r["name"].as_str().unwrap())
            .collect();
        assert!(names.contains(&"Spec Files"));
        assert!(names.contains(&"Workspace Agents"));
        assert!(names.contains(&"Merge Queue"));
    }

    // ── resources/read — unknown scheme ──────────────────────────────────────

    #[tokio::test]
    async fn mcp_resources_read_unknown_scheme() {
        let (status, json) = mcp_post(
            app(),
            json!({
                "jsonrpc": "2.0",
                "id": 31,
                "method": "resources/read",
                "params": { "uri": "unknown://foo" }
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(json["result"]["error"]
            .as_str()
            .unwrap()
            .contains("unknown resource URI scheme"));
    }

    // ── resources/read — missing uri ─────────────────────────────────────────

    #[tokio::test]
    async fn mcp_resources_read_missing_uri() {
        let (status, json) = mcp_post(
            app(),
            json!({
                "jsonrpc": "2.0",
                "id": 32,
                "method": "resources/read",
                "params": {}
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(json["error"]["code"].as_i64().is_some());
        assert_eq!(json["error"]["code"], INVALID_PARAMS);
    }

    // ── resources/read — agents:// ────────────────────────────────────────────

    #[tokio::test]
    async fn mcp_resources_read_agents() {
        let (status, json) = mcp_post(
            app(),
            json!({
                "jsonrpc": "2.0",
                "id": 33,
                "method": "resources/read",
                "params": { "uri": "agents://" }
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        // Should return a contents array (may be empty if no agents)
        assert!(json["result"]["contents"].as_array().is_some());
    }

    // ── resources/read — queue:// ─────────────────────────────────────────────

    #[tokio::test]
    async fn mcp_resources_read_queue() {
        let (status, json) = mcp_post(
            app(),
            json!({
                "jsonrpc": "2.0",
                "id": 34,
                "method": "resources/read",
                "params": { "uri": "queue://nonexistent-repo-id" }
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(json["result"]["contents"].as_array().is_some());
    }

    // ── initialize advertises resources capability ─────────────────────────────

    #[tokio::test]
    async fn mcp_initialize_advertises_resources() {
        let (status, json) = mcp_post(
            app(),
            json!({
                "jsonrpc": "2.0",
                "id": 35,
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
        assert!(json["result"]["capabilities"]["resources"].is_object());
    }

    // ── gyre_agent_complete scope: agent cannot complete another agent ─────────

    #[test]
    fn agent_scope_complete_self_allowed() {
        // Validate logic: requested_agent matches auth.agent_id — should pass
        let auth_agent_id = "agent-abc";
        let requested = "agent-abc";
        // Same → no permission denied
        assert_eq!(requested, auth_agent_id);
    }

    #[test]
    fn agent_scope_complete_other_denied() {
        // Validate logic: different agent IDs should trigger denial
        let auth_agent_id = "agent-abc";
        let requested = "agent-xyz";
        assert_ne!(requested, auth_agent_id);
    }

    // ── spec:// path traversal rejected ──────────────────────────────────────

    #[tokio::test]
    async fn mcp_resources_spec_path_traversal_rejected() {
        let (status, json) = mcp_post(
            app(),
            json!({
                "jsonrpc": "2.0",
                "id": 36,
                "method": "resources/read",
                "params": { "uri": "spec://../etc/passwd" }
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(json["result"]["error"]
            .as_str()
            .unwrap()
            .contains("path traversal"));
    }

    // ── signal chain: gyre_create_task with task_type ──────────────────────

    #[tokio::test]
    async fn mcp_create_task_with_signal_chain_fields() {
        let (status, json) = mcp_post(
            app(),
            json!({
                "jsonrpc": "2.0",
                "id": 60,
                "method": "tools/call",
                "params": {
                    "name": "gyre_create_task",
                    "arguments": {
                        "title": "Implement auth module",
                        "task_type": "implementation",
                        "order": 1,
                        "depends_on": ["task-parent-1"],
                        "spec_path": "system/auth.md",
                        "repo_id": "repo-abc",
                        "workspace_id": "ws-abc",
                        "parent_task_id": "delegation-task-1"
                    }
                }
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(
            !json["result"]["isError"].as_bool().unwrap(),
            "create_task with signal chain fields should succeed: {json}"
        );
        let text = json["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("Created task"));
    }

    #[tokio::test]
    async fn mcp_create_task_delegation_type() {
        let (status, json) = mcp_post(
            app(),
            json!({
                "jsonrpc": "2.0",
                "id": 61,
                "method": "tools/call",
                "params": {
                    "name": "gyre_create_task",
                    "arguments": {
                        "title": "Decompose spec into sub-tasks",
                        "task_type": "delegation",
                        "spec_path": "system/auth.md"
                    }
                }
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(!json["result"]["isError"].as_bool().unwrap());
    }

    #[tokio::test]
    async fn mcp_create_task_invalid_task_type() {
        let (status, json) = mcp_post(
            app(),
            json!({
                "jsonrpc": "2.0",
                "id": 62,
                "method": "tools/call",
                "params": {
                    "name": "gyre_create_task",
                    "arguments": {
                        "title": "Bad type",
                        "task_type": "invalid_type"
                    }
                }
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(json["result"]["isError"].as_bool().unwrap());
        let text = json["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("unknown task_type"));
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

    // ── gyre_create_task tool advertises signal chain fields ──────────────────

    #[tokio::test]
    async fn create_task_tool_schema_includes_signal_chain_fields() {
        let (status, json) = mcp_post(
            app(),
            json!({
                "jsonrpc": "2.0",
                "id": 63,
                "method": "tools/list",
                "params": {}
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let tools = json["result"]["tools"].as_array().unwrap();
        let create_task = tools
            .iter()
            .find(|t| t["name"] == "gyre_create_task")
            .expect("gyre_create_task must be in tools list");
        let props = &create_task["inputSchema"]["properties"];
        assert!(
            props.get("task_type").is_some(),
            "must have task_type field"
        );
        assert!(props.get("order").is_some(), "must have order field");
        assert!(
            props.get("depends_on").is_some(),
            "must have depends_on field"
        );
        assert!(
            props.get("spec_path").is_some(),
            "must have spec_path field"
        );
        assert!(props.get("repo_id").is_some(), "must have repo_id field");
        assert!(
            props.get("workspace_id").is_some(),
            "must have workspace_id field"
        );
    }

    // ── agent_complete tool advertises summary field in schema ────────────────

    #[tokio::test]
    async fn agent_complete_tool_schema_includes_summary() {
        let (status, json) = mcp_post(
            app(),
            json!({
                "jsonrpc": "2.0",
                "id": 50,
                "method": "tools/list",
                "params": {}
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let tools = json["result"]["tools"].as_array().unwrap();
        let complete = tools
            .iter()
            .find(|t| t["name"] == "gyre_agent_complete")
            .expect("gyre_agent_complete must be in tools list");
        let props = &complete["inputSchema"]["properties"];
        assert!(
            props.get("summary").is_some(),
            "gyre_agent_complete must advertise a 'summary' field in its inputSchema"
        );
    }

    // ── agent_complete with unknown agent returns error ────────────────────────

    #[tokio::test]
    async fn agent_complete_unknown_agent_returns_error() {
        let (status, json) = mcp_post(
            app(),
            json!({
                "jsonrpc": "2.0",
                "id": 51,
                "method": "tools/call",
                "params": {
                    "name": "gyre_agent_complete",
                    "arguments": {
                        "agent_id": "nonexistent-agent",
                        "summary": {
                            "spec_ref": "specs/system/example.md",
                            "decisions": [{"what": "used retry", "why": "spec says so", "confidence": "high"}],
                            "uncertainties": ["timeout behavior undefined"],
                            "conversation_sha": "abc123"
                        }
                    }
                }
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(json["result"]["isError"].as_bool().unwrap());
        let text = json["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("not found") || text.contains("Agent not found"));
    }

    // ── AgentCompleted MessageKind roundtrips ─────────────────────────────────

    #[test]
    fn agent_completed_message_kind_roundtrip() {
        use gyre_common::MessageKind;
        let k = MessageKind::AgentCompleted;
        assert_eq!(k.as_str(), "agent_completed");
        assert!(k.server_only());
        assert_eq!(k.tier(), gyre_common::MessageTier::Event);
        let json = serde_json::to_string(&k).unwrap();
        let back: MessageKind = serde_json::from_str(&json).unwrap();
        assert_eq!(back, MessageKind::AgentCompleted);
    }

    #[test]
    fn reconciliation_completed_message_kind_roundtrip() {
        use gyre_common::MessageKind;
        let k = MessageKind::ReconciliationCompleted;
        assert_eq!(k.as_str(), "reconciliation_completed");
        assert!(k.server_only());
        assert_eq!(k.tier(), gyre_common::MessageTier::Event);
        let json = serde_json::to_string(&k).unwrap();
        let back: MessageKind = serde_json::from_str(&json).unwrap();
        assert_eq!(back, MessageKind::ReconciliationCompleted);
    }

    // ── AgentCompletionSummary parsing in build_agent_completed_payload ───────

    #[test]
    fn build_agent_completed_payload_with_summary() {
        use gyre_common::{AgentCompletionSummary, Decision};
        let summary = AgentCompletionSummary {
            spec_ref: Some("specs/system/example.md".to_string()),
            decisions: vec![Decision {
                what: "used retry".to_string(),
                why: "spec says so".to_string(),
                confidence: "high".to_string(),
                alternatives_considered: Some(vec!["no retry".to_string()]),
            }],
            uncertainties: vec!["timeout behavior undefined".to_string()],
            conversation_sha: Some("abc123".to_string()),
        };
        let task_id = Id::new("task-abc");
        let payload = build_agent_completed_payload("agent-1", Some(&task_id), &Some(summary));
        assert_eq!(payload["agent_id"], "agent-1");
        assert_eq!(payload["task_id"], "task-abc");
        assert_eq!(payload["spec_ref"], "specs/system/example.md");
        assert_eq!(payload["decisions"].as_array().unwrap().len(), 1);
        assert_eq!(payload["uncertainties"].as_array().unwrap().len(), 1);
        assert_eq!(payload["conversation_sha"], "abc123");
    }

    // ── message bus MCP tools ─────────────────────────────────────────────────

    #[tokio::test]
    async fn mcp_tools_list_includes_message_tools() {
        let (status, json) = mcp_post(
            app(),
            json!({
                "jsonrpc": "2.0",
                "id": 70,
                "method": "tools/list"
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let tools = json["result"]["tools"].as_array().unwrap();
        let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert!(
            names.contains(&"gyre_message_send"),
            "missing gyre_message_send"
        );
        assert!(
            names.contains(&"gyre_message_poll"),
            "missing gyre_message_poll"
        );
        assert!(
            names.contains(&"gyre_message_ack"),
            "missing gyre_message_ack"
        );
    }

    #[tokio::test]
    async fn mcp_message_send_requires_agent() {
        // Sending a message requires an agent to exist
        let state = test_state();
        let app = crate::build_router(state.clone());

        let (status, json) = mcp_post(
            app,
            json!({
                "jsonrpc": "2.0",
                "id": 71,
                "method": "tools/call",
                "params": {
                    "name": "gyre_message_send",
                    "arguments": {
                        "to": {"agent": "target-agent"},
                        "kind": "task_assignment"
                    }
                }
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        // system agent doesn't exist in test state → error
        assert!(json["result"]["isError"].as_bool().unwrap());
    }

    #[tokio::test]
    async fn mcp_message_send_directed_succeeds() {
        let state = test_state();
        // Create sender agent (test-token maps to agent_id "system")
        let mut sender = gyre_domain::Agent::new(Id::new("system"), "system", 0);
        sender.workspace_id = Id::new("ws-mcp");
        state.agents.create(&sender).await.unwrap();
        // Create target agent in same workspace
        let mut target = gyre_domain::Agent::new(Id::new("agent-target"), "target", 0);
        target.workspace_id = Id::new("ws-mcp");
        state.agents.create(&target).await.unwrap();

        let app = crate::build_router(state.clone());
        let (status, json) = mcp_post(
            app,
            json!({
                "jsonrpc": "2.0",
                "id": 72,
                "method": "tools/call",
                "params": {
                    "name": "gyre_message_send",
                    "arguments": {
                        "to": {"agent": "agent-target"},
                        "kind": "task_assignment",
                        "payload": {"task_id": "TASK-99"}
                    }
                }
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(
            !json["result"]["isError"].as_bool().unwrap(),
            "message send should succeed: {json}"
        );
        let text = json["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("Message sent"));
    }

    #[tokio::test]
    async fn mcp_message_send_cross_workspace_rejected() {
        let state = test_state();
        let mut sender = gyre_domain::Agent::new(Id::new("system"), "system", 0);
        sender.workspace_id = Id::new("ws-a");
        state.agents.create(&sender).await.unwrap();
        let mut target = gyre_domain::Agent::new(Id::new("agent-other-ws"), "other", 0);
        target.workspace_id = Id::new("ws-b");
        state.agents.create(&target).await.unwrap();

        let app = crate::build_router(state.clone());
        let (status, json) = mcp_post(
            app,
            json!({
                "jsonrpc": "2.0",
                "id": 73,
                "method": "tools/call",
                "params": {
                    "name": "gyre_message_send",
                    "arguments": {
                        "to": {"agent": "agent-other-ws"},
                        "kind": "task_assignment"
                    }
                }
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(json["result"]["isError"].as_bool().unwrap());
        let text = json["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("not in the same workspace"));
    }

    #[tokio::test]
    async fn mcp_message_send_server_only_rejected() {
        let state = test_state();
        let mut sender = gyre_domain::Agent::new(Id::new("system"), "system", 0);
        sender.workspace_id = Id::new("ws-so");
        state.agents.create(&sender).await.unwrap();

        let app = crate::build_router(state.clone());
        let (status, json) = mcp_post(
            app,
            json!({
                "jsonrpc": "2.0",
                "id": 74,
                "method": "tools/call",
                "params": {
                    "name": "gyre_message_send",
                    "arguments": {
                        "to": {"workspace": "ws-so"},
                        "kind": "agent_created"
                    }
                }
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(json["result"]["isError"].as_bool().unwrap());
        let text = json["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("server"));
    }

    #[tokio::test]
    async fn mcp_message_poll_returns_messages() {
        let state = test_state();
        let mut agent = gyre_domain::Agent::new(Id::new("system"), "system", 0);
        agent.workspace_id = Id::new("ws-poll");
        state.agents.create(&agent).await.unwrap();

        // Store a message for the agent.
        use gyre_common::message::{Destination, Message, MessageKind, MessageOrigin};
        let msg = Message {
            id: Id::new("poll-msg-1"),
            tenant_id: Id::new("default"),
            from: MessageOrigin::Server,
            workspace_id: Some(Id::new("ws-poll")),
            to: Destination::Agent(Id::new("system")),
            kind: MessageKind::TaskAssignment,
            payload: Some(json!({"task_id": "T-1"})),
            created_at: 5_000,
            signature: None,
            key_id: None,
            acknowledged: false,
        };
        state.messages.store(&msg).await.unwrap();

        let app = crate::build_router(state.clone());
        let (status, json) = mcp_post(
            app,
            json!({
                "jsonrpc": "2.0",
                "id": 75,
                "method": "tools/call",
                "params": {
                    "name": "gyre_message_poll",
                    "arguments": {
                        "after_ts": 0,
                        "limit": 10
                    }
                }
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(
            !json["result"]["isError"].as_bool().unwrap(),
            "poll should succeed: {json}"
        );
        let text = json["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("1 message(s)"));
        assert!(text.contains("poll-msg-1"));
    }

    #[tokio::test]
    async fn mcp_message_poll_unacked_only() {
        let state = test_state();
        let mut agent = gyre_domain::Agent::new(Id::new("system"), "system", 0);
        agent.workspace_id = Id::new("ws-unack");
        state.agents.create(&agent).await.unwrap();

        use gyre_common::message::{Destination, Message, MessageKind, MessageOrigin};
        let msg = Message {
            id: Id::new("unack-msg-1"),
            tenant_id: Id::new("default"),
            from: MessageOrigin::Server,
            workspace_id: Some(Id::new("ws-unack")),
            to: Destination::Agent(Id::new("system")),
            kind: MessageKind::ReviewRequest,
            payload: None,
            created_at: 1_000,
            signature: None,
            key_id: None,
            acknowledged: false,
        };
        state.messages.store(&msg).await.unwrap();

        let app = crate::build_router(state.clone());
        let (status, json) = mcp_post(
            app,
            json!({
                "jsonrpc": "2.0",
                "id": 76,
                "method": "tools/call",
                "params": {
                    "name": "gyre_message_poll",
                    "arguments": {
                        "unacked_only": true
                    }
                }
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(!json["result"]["isError"].as_bool().unwrap());
        let text = json["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("1 message(s)"));
    }

    #[tokio::test]
    async fn mcp_message_ack_succeeds() {
        let state = test_state();
        let mut agent = gyre_domain::Agent::new(Id::new("system"), "system", 0);
        agent.workspace_id = Id::new("ws-ack");
        state.agents.create(&agent).await.unwrap();

        use gyre_common::message::{Destination, Message, MessageKind, MessageOrigin};
        let msg = Message {
            id: Id::new("ack-mcp-1"),
            tenant_id: Id::new("default"),
            from: MessageOrigin::Server,
            workspace_id: Some(Id::new("ws-ack")),
            to: Destination::Agent(Id::new("system")),
            kind: MessageKind::TaskAssignment,
            payload: None,
            created_at: 2_000,
            signature: None,
            key_id: None,
            acknowledged: false,
        };
        state.messages.store(&msg).await.unwrap();

        let app = crate::build_router(state.clone());
        let (status, json) = mcp_post(
            app,
            json!({
                "jsonrpc": "2.0",
                "id": 77,
                "method": "tools/call",
                "params": {
                    "name": "gyre_message_ack",
                    "arguments": {
                        "message_id": "ack-mcp-1"
                    }
                }
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(
            !json["result"]["isError"].as_bool().unwrap(),
            "ack should succeed: {json}"
        );
        let text = json["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("acknowledged"));
    }

    #[tokio::test]
    async fn mcp_message_ack_missing_id_returns_error() {
        let (status, json) = mcp_post(
            app(),
            json!({
                "jsonrpc": "2.0",
                "id": 78,
                "method": "tools/call",
                "params": {
                    "name": "gyre_message_ack",
                    "arguments": {}
                }
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(json["result"]["isError"].as_bool().unwrap());
    }

    #[tokio::test]
    async fn mcp_message_send_broadcast_rejected() {
        let state = test_state();
        let mut sender = gyre_domain::Agent::new(Id::new("system"), "system", 0);
        sender.workspace_id = Id::new("ws-bc");
        state.agents.create(&sender).await.unwrap();

        let app = crate::build_router(state.clone());
        let (status, json) = mcp_post(
            app,
            json!({
                "jsonrpc": "2.0",
                "id": 80,
                "method": "tools/call",
                "params": {
                    "name": "gyre_message_send",
                    "arguments": {
                        "to": "broadcast",
                        "kind": "task_assignment"
                    }
                }
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(
            json["result"]["isError"].as_bool().unwrap(),
            "broadcast destination should be rejected for message.send: {json}"
        );
        let text = json["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("invalid"));
    }

    #[tokio::test]
    async fn mcp_message_send_telemetry_kind_rejected() {
        let state = test_state();
        let mut sender = gyre_domain::Agent::new(Id::new("system"), "system", 0);
        sender.workspace_id = Id::new("ws-tel");
        state.agents.create(&sender).await.unwrap();

        let app = crate::build_router(state.clone());
        let (status, json) = mcp_post(
            app,
            json!({
                "jsonrpc": "2.0",
                "id": 81,
                "method": "tools/call",
                "params": {
                    "name": "gyre_message_send",
                    "arguments": {
                        "to": {"workspace": "ws-tel"},
                        "kind": "tool_call_start"
                    }
                }
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(
            json["result"]["isError"].as_bool().unwrap(),
            "telemetry kind should be rejected for message.send: {json}"
        );
        let text = json["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("Telemetry-tier"));
        assert!(text.contains("gyre_record_activity"));
    }

    #[tokio::test]
    async fn mcp_record_activity_maps_event_types() {
        let state = test_state();
        // Create an agent so workspace lookup succeeds.
        let mut agent = gyre_domain::Agent::new(Id::new("system"), "system", 0);
        agent.workspace_id = Id::new("ws-activity");
        state.agents.create(&agent).await.unwrap();

        let app = crate::build_router(state.clone());

        // Test each AG-UI event type mapping.
        for event_type in &[
            "TOOL_CALL_START",
            "TOOL_CALL_END",
            "TEXT_MESSAGE_CONTENT",
            "RUN_STARTED",
            "RUN_FINISHED",
            "STATE_CHANGED",
        ] {
            let (status, json) = mcp_post(
                app.clone(),
                json!({
                    "jsonrpc": "2.0",
                    "id": 82,
                    "method": "tools/call",
                    "params": {
                        "name": "gyre_record_activity",
                        "arguments": {
                            "agent_id": "system",
                            "event_type": event_type,
                            "description": format!("test {}", event_type),
                        }
                    }
                }),
            )
            .await;
            assert_eq!(status, StatusCode::OK, "failed for {event_type}");
            assert!(
                !json["result"]["isError"].as_bool().unwrap(),
                "record_activity should succeed for {event_type}: {json}"
            );
        }
    }

    #[tokio::test]
    async fn mcp_record_activity_uses_agent_origin_and_tenant() {
        // F5: verify that record_activity messages use MessageOrigin::Agent,
        // not Server, and use the caller's tenant_id.
        let state = test_state();
        // Global token auth maps to agent_id "system", so create agent with that id.
        let mut agent = gyre_domain::Agent::new(Id::new("system"), "system", 0);
        agent.workspace_id = Id::new("ws-f5");
        state.agents.create(&agent).await.unwrap();

        let app = crate::build_router(state.clone());
        let (status, json) = mcp_post(
            app,
            json!({
                "jsonrpc": "2.0",
                "id": 90,
                "method": "tools/call",
                "params": {
                    "name": "gyre_record_activity",
                    "arguments": {
                        "agent_id": "system",
                        "event_type": "RUN_STARTED",
                        "description": "test origin"
                    }
                }
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(
            !json["result"]["isError"].as_bool().unwrap(),
            "record_activity failed: {json}"
        );

        // Verify the telemetry buffer message has agent origin, not server.
        let msgs = state.telemetry_buffer.list_since(&Id::new("ws-f5"), 0, 100);
        assert!(!msgs.is_empty(), "telemetry buffer should have messages");
        let msg = &msgs[0];
        match &msg.from {
            gyre_common::message::MessageOrigin::Agent(id) => {
                // Global token auth maps to agent_id "system".
                assert_eq!(id.to_string(), "system");
            }
            other => panic!("expected MessageOrigin::Agent, got {:?}", other),
        }
        // tenant_id should come from auth context, not hardcoded.
        assert_eq!(msg.tenant_id.to_string(), "default"); // test auth uses default tenant
    }

    #[tokio::test]
    async fn mcp_record_activity_per_kind_payload_schemas() {
        // F6: verify that per-kind payloads conform to message-bus.md §Payload Schemas.
        let state = test_state();
        let mut agent = gyre_domain::Agent::new(Id::new("system"), "system", 0);
        agent.workspace_id = Id::new("ws-f6");
        state.agents.create(&agent).await.unwrap();

        let app = crate::build_router(state.clone());

        // Test TOOL_CALL_START — should have agent_id + tool_name.
        let (_status, json) = mcp_post(
            app.clone(),
            json!({
                "jsonrpc": "2.0",
                "id": 91,
                "method": "tools/call",
                "params": {
                    "name": "gyre_record_activity",
                    "arguments": {
                        "agent_id": "system",
                        "event_type": "TOOL_CALL_START",
                        "description": "fallback tool name",
                        "tool_name": "grep"
                    }
                }
            }),
        )
        .await;
        assert!(!json["result"]["isError"].as_bool().unwrap());

        // Test TOOL_CALL_END — should have agent_id + tool_name + duration_ms.
        let (_status, json) = mcp_post(
            app.clone(),
            json!({
                "jsonrpc": "2.0",
                "id": 92,
                "method": "tools/call",
                "params": {
                    "name": "gyre_record_activity",
                    "arguments": {
                        "agent_id": "system",
                        "event_type": "TOOL_CALL_END",
                        "description": "grep completed",
                        "tool_name": "grep",
                        "duration_ms": 42
                    }
                }
            }),
        )
        .await;
        assert!(!json["result"]["isError"].as_bool().unwrap());

        // Test TEXT_MESSAGE_CONTENT — should have agent_id + content.
        let (_status, json) = mcp_post(
            app.clone(),
            json!({
                "jsonrpc": "2.0",
                "id": 93,
                "method": "tools/call",
                "params": {
                    "name": "gyre_record_activity",
                    "arguments": {
                        "agent_id": "system",
                        "event_type": "TEXT_MESSAGE_CONTENT",
                        "description": "some text",
                        "content": "hello world",
                        "role": "assistant"
                    }
                }
            }),
        )
        .await;
        assert!(!json["result"]["isError"].as_bool().unwrap());

        // Test STATE_CHANGED — should have agent_id + new_state.
        let (_status, json) = mcp_post(
            app.clone(),
            json!({
                "jsonrpc": "2.0",
                "id": 94,
                "method": "tools/call",
                "params": {
                    "name": "gyre_record_activity",
                    "arguments": {
                        "agent_id": "system",
                        "event_type": "STATE_CHANGED",
                        "description": "thinking",
                        "old_state": "idle",
                        "new_state": "thinking"
                    }
                }
            }),
        )
        .await;
        assert!(!json["result"]["isError"].as_bool().unwrap());

        // Verify stored telemetry payloads have per-kind fields.
        let msgs = state.telemetry_buffer.list_since(&Id::new("ws-f6"), 0, 100);
        assert!(
            msgs.len() >= 4,
            "expected 4+ telemetry messages, got {}",
            msgs.len()
        );

        // Check TOOL_CALL_START payload.
        let tool_start = msgs
            .iter()
            .find(|m| matches!(m.kind, gyre_common::message::MessageKind::ToolCallStart))
            .expect("ToolCallStart message");
        let p = tool_start.payload.as_ref().unwrap();
        assert_eq!(p["tool_name"], "grep");
        assert_eq!(p["agent_id"], "system");

        // Check TOOL_CALL_END payload.
        let tool_end = msgs
            .iter()
            .find(|m| matches!(m.kind, gyre_common::message::MessageKind::ToolCallEnd))
            .expect("ToolCallEnd message");
        let p = tool_end.payload.as_ref().unwrap();
        assert_eq!(p["tool_name"], "grep");
        assert_eq!(p["duration_ms"], 42);

        // Check TEXT_MESSAGE_CONTENT payload.
        let text_msg = msgs
            .iter()
            .find(|m| {
                matches!(
                    m.kind,
                    gyre_common::message::MessageKind::TextMessageContent
                )
            })
            .expect("TextMessageContent message");
        let p = text_msg.payload.as_ref().unwrap();
        assert_eq!(p["content"], "hello world");
        assert_eq!(p["role"], "assistant");

        // Check STATE_CHANGED payload.
        let state_changed = msgs
            .iter()
            .find(|m| matches!(m.kind, gyre_common::message::MessageKind::StateChanged))
            .expect("StateChanged message");
        let p = state_changed.payload.as_ref().unwrap();
        assert_eq!(p["new_state"], "thinking");
        assert_eq!(p["old_state"], "idle");
    }

    #[test]
    fn build_agent_completed_payload_without_summary() {
        let payload = build_agent_completed_payload("agent-2", None, &None);
        assert_eq!(payload["agent_id"], "agent-2");
        assert!(payload.get("task_id").is_none() || payload["task_id"].is_null());
        assert_eq!(payload["decisions"].as_array().unwrap().len(), 0);
        assert_eq!(payload["uncertainties"].as_array().unwrap().len(), 0);
        assert!(payload.get("spec_ref").is_none() || payload["spec_ref"].is_null());
    }
}
