//! Explorer WebSocket handler.
//!
//! WS /api/v1/repos/:repo_id/explorer
//!
//! Receives user messages with canvas state, runs a multi-turn LLM agent loop
//! with tool calling (graph_summary, graph_query_dryrun, graph_nodes,
//! graph_edges, search), streams text responses and view queries back.
//! Also handles saved view CRUD over the same WebSocket.

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Path, State,
    },
    response::IntoResponse,
};
use futures_util::{SinkExt, StreamExt};
use gyre_common::view_query::{ExplorerClientMessage, ExplorerServerMessage, SavedViewSummary};
use gyre_common::Id;
use gyre_ports::saved_view::SavedView;
use gyre_ports::{
    ContentBlock, ConversationContent, ConversationMessage, ToolCall, ToolDefinition,
};
use serde_json::json;
use std::sync::Arc;
use tracing::{info, warn};

use crate::{auth::AuthenticatedAgent, AppState};

/// Max agent turns (tool calls + refinements) before we force a response.
const MAX_AGENT_TURNS: usize = 6;

pub async fn explorer_ws(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
    Path(repo_id): Path<String>,
    auth: AuthenticatedAgent,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_explorer_session(socket, state, repo_id, auth))
}

async fn handle_explorer_session(
    socket: WebSocket,
    state: Arc<AppState>,
    repo_id: String,
    auth: AuthenticatedAgent,
) {
    let (mut sender, mut receiver) = socket.split();
    info!(repo_id = %repo_id, user = %auth.agent_id, "Explorer WebSocket session started");

    while let Some(msg) = receiver.next().await {
        let msg = match msg {
            Ok(Message::Text(text)) => text,
            Ok(Message::Close(_)) => break,
            Ok(_) => continue,
            Err(e) => {
                warn!("WebSocket error: {e}");
                break;
            }
        };

        let client_msg: ExplorerClientMessage = match serde_json::from_str(&msg) {
            Ok(m) => m,
            Err(e) => {
                let err = ExplorerServerMessage::Error {
                    message: format!("Invalid message: {e}"),
                };
                let _ = sender
                    .send(Message::Text(serde_json::to_string(&err).unwrap().into()))
                    .await;
                continue;
            }
        };

        match client_msg {
            ExplorerClientMessage::Message { text, canvas_state } => {
                // Send thinking status
                send_status(&mut sender, "thinking").await;

                // Run the agent loop
                match run_explorer_agent(
                    &state,
                    &repo_id,
                    &text,
                    &canvas_state,
                    &mut sender,
                )
                .await
                {
                    Ok(()) => {}
                    Err(e) => {
                        let err = ExplorerServerMessage::Error {
                            message: format!("Explorer query failed: {e}"),
                        };
                        let _ = sender
                            .send(Message::Text(serde_json::to_string(&err).unwrap().into()))
                            .await;
                    }
                }

                send_status(&mut sender, "ready").await;
            }

            ExplorerClientMessage::SaveView {
                name,
                description,
                query,
            } => {
                let now = crate::api::now_secs();
                let view = SavedView {
                    id: crate::api::new_id(),
                    repo_id: Id::new(&repo_id),
                    workspace_id: Id::new(""),
                    tenant_id: Id::new(&auth.tenant_id),
                    name,
                    description,
                    query_json: serde_json::to_string(&query).unwrap_or_default(),
                    created_by: auth.agent_id.clone(),
                    created_at: now,
                    updated_at: now,
                    is_system: false,
                };
                match state.saved_views.create(view).await {
                    Ok(v) => {
                        let msg = ExplorerServerMessage::Views {
                            views: vec![SavedViewSummary {
                                id: v.id.to_string(),
                                name: v.name,
                                description: v.description,
                                created_at: v.created_at,
                            }],
                        };
                        let _ = sender
                            .send(Message::Text(serde_json::to_string(&msg).unwrap().into()))
                            .await;
                    }
                    Err(e) => {
                        let err = ExplorerServerMessage::Error {
                            message: format!("Failed to save view: {e}"),
                        };
                        let _ = sender
                            .send(Message::Text(serde_json::to_string(&err).unwrap().into()))
                            .await;
                    }
                }
            }

            ExplorerClientMessage::LoadView { view_id } => {
                let vid = Id::new(&view_id);
                match state.saved_views.get(&vid).await {
                    Ok(Some(v)) => {
                        let query: serde_json::Value =
                            serde_json::from_str(&v.query_json).unwrap_or_default();
                        let msg = ExplorerServerMessage::ViewQuery { query };
                        let _ = sender
                            .send(Message::Text(serde_json::to_string(&msg).unwrap().into()))
                            .await;
                    }
                    Ok(None) => {
                        let err = ExplorerServerMessage::Error {
                            message: format!("View not found: {view_id}"),
                        };
                        let _ = sender
                            .send(Message::Text(serde_json::to_string(&err).unwrap().into()))
                            .await;
                    }
                    Err(e) => {
                        let err = ExplorerServerMessage::Error {
                            message: format!("Failed to load view: {e}"),
                        };
                        let _ = sender
                            .send(Message::Text(serde_json::to_string(&err).unwrap().into()))
                            .await;
                    }
                }
            }

            ExplorerClientMessage::ListViews => {
                let rid = Id::new(&repo_id);
                match state.saved_views.list_by_repo(&rid).await {
                    Ok(views) => {
                        let summaries: Vec<SavedViewSummary> = views
                            .into_iter()
                            .map(|v| SavedViewSummary {
                                id: v.id.to_string(),
                                name: v.name,
                                description: v.description,
                                created_at: v.created_at,
                            })
                            .collect();
                        let msg = ExplorerServerMessage::Views { views: summaries };
                        let _ = sender
                            .send(Message::Text(serde_json::to_string(&msg).unwrap().into()))
                            .await;
                    }
                    Err(e) => {
                        let err = ExplorerServerMessage::Error {
                            message: format!("Failed to list views: {e}"),
                        };
                        let _ = sender
                            .send(Message::Text(serde_json::to_string(&err).unwrap().into()))
                            .await;
                    }
                }
            }
        }
    }

    info!(repo_id = %repo_id, "Explorer WebSocket session ended");
}

async fn send_status(
    sender: &mut futures_util::stream::SplitSink<WebSocket, Message>,
    status: &str,
) {
    let msg = ExplorerServerMessage::Status {
        status: status.to_string(),
    };
    let _ = sender
        .send(Message::Text(serde_json::to_string(&msg).unwrap().into()))
        .await;
}

// ── Tool definitions for the explorer agent ──────────────────────────────────

fn explorer_tool_definitions() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "graph_summary".to_string(),
            description: "Get a condensed summary of the repo's knowledge graph: node/edge counts, top types by fields, top functions by calls, modules, test coverage stats.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        },
        ToolDefinition {
            name: "graph_query_dryrun".to_string(),
            description: "Dry-run a view query against the knowledge graph. Returns matched node count, names, resolved groups/callouts/narrative, and warnings. Use this to validate queries before sending to the frontend.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "object",
                        "description": "View query JSON (scope, emphasis, groups, callouts, narrative, annotation, edges, zoom)"
                    }
                },
                "required": ["query"]
            }),
        },
        ToolDefinition {
            name: "graph_nodes".to_string(),
            description: "Query graph nodes by name pattern or node type. Returns up to 50 nodes with details (id, name, qualified_name, node_type, file_path, etc.).".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "name_pattern": { "type": "string", "description": "Substring match on name or qualified_name (case-insensitive)" },
                    "node_type": { "type": "string", "description": "Filter by: package, module, type, interface, function, endpoint, component, table, constant, field" }
                }
            }),
        },
        ToolDefinition {
            name: "graph_edges".to_string(),
            description: "Query graph edges by source/target node ID or edge type. Returns up to 100 edges.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "node_id": { "type": "string", "description": "Find all edges connected to this node" },
                    "edge_type": { "type": "string", "description": "Filter by: contains, implements, depends_on, calls, field_of, returns, routes_to, governed_by" },
                    "source_id": { "type": "string", "description": "Filter by source node" },
                    "target_id": { "type": "string", "description": "Filter by target node" }
                }
            }),
        },
        ToolDefinition {
            name: "search".to_string(),
            description: "Full-text search across the knowledge graph. Searches node names, qualified names, doc comments, file paths, and spec paths.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Search term (case-insensitive substring match)" },
                    "limit": { "type": "integer", "description": "Max results to return (default 30)" }
                },
                "required": ["query"]
            }),
        },
    ]
}

// ── Explorer agent loop ──────────────────────────────────────────────────────

async fn run_explorer_agent(
    state: &AppState,
    repo_id: &str,
    user_question: &str,
    canvas_state: &gyre_common::view_query::CanvasState,
    sender: &mut futures_util::stream::SplitSink<WebSocket, Message>,
) -> anyhow::Result<()> {
    let llm = match &state.llm {
        Some(llm) => llm.clone(),
        None => {
            let text_msg = ExplorerServerMessage::Text {
                content: "LLM is not configured. Set GYRE_VERTEX_PROJECT and GYRE_VERTEX_LOCATION to enable conversational exploration.".to_string(),
                done: true,
            };
            sender
                .send(Message::Text(serde_json::to_string(&text_msg).unwrap().into()))
                .await?;
            return Ok(());
        }
    };

    let model = std::env::var("GYRE_EXPLORER_MODEL")
        .unwrap_or_else(|_| "claude-sonnet-4-6".to_string());
    let llm_port = llm.for_model(&model);

    // Load graph data once for tool execution
    let rid = Id::new(repo_id);
    let nodes = state
        .graph_store
        .list_nodes(&rid, None)
        .await
        .unwrap_or_default();
    let edges = state
        .graph_store
        .list_edges(&rid, None)
        .await
        .unwrap_or_default();

    let system_prompt = build_system_prompt();
    let tools = explorer_tool_definitions();

    // Build initial user message with canvas context
    let canvas_json = serde_json::to_string(canvas_state).unwrap_or_else(|_| "{}".to_string());
    let user_content = format!(
        "Canvas state:\n```json\n{canvas_json}\n```\n\nUser question: {user_question}"
    );

    let mut messages = vec![ConversationMessage {
        role: "user".to_string(),
        content: ConversationContent::Text(user_content),
    }];

    let selected_node_id = canvas_state.selected_node.as_ref().map(|n| n.id.as_str());

    // Multi-turn agent loop
    for turn in 0..MAX_AGENT_TURNS {
        let response = llm_port
            .complete_with_tools(&system_prompt, &messages, &tools, Some(4096))
            .await?;

        // If the LLM returned text, stream it to the client
        if !response.text.is_empty() {
            let text_msg = ExplorerServerMessage::Text {
                content: response.text.clone(),
                done: response.tool_calls.is_empty() && response.stop_reason != "tool_use",
            };
            sender
                .send(Message::Text(serde_json::to_string(&text_msg).unwrap().into()))
                .await?;
        }

        // Check for view_query blocks in the text
        let (_clean_text, view_query) = parse_view_query_from_text(&response.text);

        if let Some(query_json) = view_query {
            // Send the extracted view query to the frontend
            let view_msg = ExplorerServerMessage::ViewQuery {
                query: query_json,
            };
            sender
                .send(Message::Text(serde_json::to_string(&view_msg).unwrap().into()))
                .await?;
        }

        // If no tool calls, we're done
        if response.tool_calls.is_empty() || response.stop_reason == "end_turn" {
            break;
        }

        send_status(sender, "refining").await;

        // Add the assistant's response (with tool_use blocks) to the conversation
        let mut assistant_blocks = Vec::new();
        if !response.text.is_empty() {
            assistant_blocks.push(ContentBlock::Text {
                text: response.text.clone(),
            });
        }
        for tc in &response.tool_calls {
            assistant_blocks.push(ContentBlock::ToolUse {
                id: tc.id.clone(),
                name: tc.name.clone(),
                input: tc.input.clone(),
            });
        }
        messages.push(ConversationMessage {
            role: "assistant".to_string(),
            content: ConversationContent::Blocks(assistant_blocks),
        });

        // Execute each tool call and collect results
        let mut result_blocks = Vec::new();
        for tc in &response.tool_calls {
            let result = execute_tool(tc, repo_id, &nodes, &edges, selected_node_id, state).await;
            result_blocks.push(ContentBlock::ToolResult {
                tool_use_id: tc.id.clone(),
                content: result,
            });
        }

        // Add tool results as a user message
        messages.push(ConversationMessage {
            role: "user".to_string(),
            content: ConversationContent::Blocks(result_blocks),
        });

        // Safety: prevent runaway loops
        if turn == MAX_AGENT_TURNS - 1 {
            info!("Explorer agent hit max turns ({MAX_AGENT_TURNS}), forcing response");
        }
    }

    Ok(())
}

/// Execute a tool call against the knowledge graph.
async fn execute_tool(
    tool_call: &ToolCall,
    repo_id: &str,
    nodes: &[gyre_common::graph::GraphNode],
    edges: &[gyre_common::graph::GraphEdge],
    selected_node_id: Option<&str>,
    _state: &AppState,
) -> String {
    match tool_call.name.as_str() {
        "graph_summary" => {
            let summary = gyre_domain::view_query_resolver::compute_graph_summary(
                repo_id, nodes, edges,
            );
            serde_json::to_string_pretty(&summary).unwrap_or_else(|e| format!("Error: {e}"))
        }
        "graph_query_dryrun" => {
            let query_value = tool_call.input.get("query").cloned().unwrap_or_default();
            match serde_json::from_value::<gyre_common::view_query::ViewQuery>(query_value) {
                Ok(query) => {
                    let result = gyre_domain::view_query_resolver::dry_run(
                        &query,
                        nodes,
                        edges,
                        selected_node_id,
                    );
                    serde_json::to_string_pretty(&result)
                        .unwrap_or_else(|e| format!("Error: {e}"))
                }
                Err(e) => format!("Invalid view query: {e}"),
            }
        }
        "graph_nodes" => {
            let name_pattern = tool_call
                .input
                .get("name_pattern")
                .and_then(|v| v.as_str())
                .map(|s| s.to_lowercase());
            let node_type_filter = tool_call
                .input
                .get("node_type")
                .and_then(|v| v.as_str())
                .map(|s| s.to_lowercase());

            let filtered: Vec<serde_json::Value> = nodes
                .iter()
                .filter(|n| n.deleted_at.is_none())
                .filter(|n| {
                    if let Some(ref pat) = name_pattern {
                        n.name.to_lowercase().contains(pat)
                            || n.qualified_name.to_lowercase().contains(pat)
                    } else {
                        true
                    }
                })
                .filter(|n| {
                    if let Some(ref nt) = node_type_filter {
                        format!("{:?}", n.node_type).to_lowercase() == *nt
                    } else {
                        true
                    }
                })
                .take(50)
                .map(|n| {
                    json!({
                        "id": n.id.to_string(),
                        "name": n.name,
                        "qualified_name": n.qualified_name,
                        "node_type": format!("{:?}", n.node_type).to_lowercase(),
                        "file_path": n.file_path,
                        "line_start": n.line_start,
                        "visibility": format!("{:?}", n.visibility).to_lowercase(),
                        "spec_path": n.spec_path,
                        "complexity": n.complexity,
                        "test_node": n.test_node,
                    })
                })
                .collect();

            format!(
                "{} nodes:\n{}",
                filtered.len(),
                serde_json::to_string_pretty(&filtered).unwrap_or_default()
            )
        }
        "graph_edges" => {
            let node_id = tool_call.input.get("node_id").and_then(|v| v.as_str());
            let edge_type = tool_call.input.get("edge_type").and_then(|v| v.as_str());
            let source_id = tool_call.input.get("source_id").and_then(|v| v.as_str());
            let target_id = tool_call.input.get("target_id").and_then(|v| v.as_str());

            let filtered: Vec<serde_json::Value> = edges
                .iter()
                .filter(|e| e.deleted_at.is_none())
                .filter(|e| {
                    if let Some(nid) = node_id {
                        e.source_id.to_string() == nid || e.target_id.to_string() == nid
                    } else {
                        true
                    }
                })
                .filter(|e| {
                    if let Some(et) = edge_type {
                        format!("{:?}", e.edge_type).to_lowercase() == et.to_lowercase()
                    } else {
                        true
                    }
                })
                .filter(|e| {
                    source_id.map_or(true, |s| e.source_id.to_string() == s)
                        && target_id.map_or(true, |t| e.target_id.to_string() == t)
                })
                .take(100)
                .map(|e| {
                    // Include source/target names for context
                    let source_name = nodes
                        .iter()
                        .find(|n| n.id == e.source_id)
                        .map(|n| n.name.as_str())
                        .unwrap_or("?");
                    let target_name = nodes
                        .iter()
                        .find(|n| n.id == e.target_id)
                        .map(|n| n.name.as_str())
                        .unwrap_or("?");
                    json!({
                        "id": e.id.to_string(),
                        "source_id": e.source_id.to_string(),
                        "source_name": source_name,
                        "target_id": e.target_id.to_string(),
                        "target_name": target_name,
                        "edge_type": format!("{:?}", e.edge_type).to_lowercase(),
                    })
                })
                .collect();

            format!(
                "{} edges:\n{}",
                filtered.len(),
                serde_json::to_string_pretty(&filtered).unwrap_or_default()
            )
        }
        "search" => {
            let query = tool_call
                .input
                .get("query")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_lowercase();
            let limit = tool_call
                .input
                .get("limit")
                .and_then(|v| v.as_u64())
                .unwrap_or(30) as usize;

            let results: Vec<serde_json::Value> = nodes
                .iter()
                .filter(|n| n.deleted_at.is_none())
                .filter(|n| {
                    n.name.to_lowercase().contains(&query)
                        || n.qualified_name.to_lowercase().contains(&query)
                        || n.file_path.to_lowercase().contains(&query)
                        || n.doc_comment
                            .as_ref()
                            .map_or(false, |d| d.to_lowercase().contains(&query))
                        || n.spec_path
                            .as_ref()
                            .map_or(false, |s| s.to_lowercase().contains(&query))
                })
                .take(limit)
                .map(|n| {
                    json!({
                        "id": n.id.to_string(),
                        "name": n.name,
                        "qualified_name": n.qualified_name,
                        "node_type": format!("{:?}", n.node_type).to_lowercase(),
                        "file_path": n.file_path,
                        "spec_path": n.spec_path,
                        "doc_comment": n.doc_comment.as_deref().map(|d| if d.len() > 100 { &d[..100] } else { d }),
                    })
                })
                .collect();

            format!(
                "{} results:\n{}",
                results.len(),
                serde_json::to_string_pretty(&results).unwrap_or_default()
            )
        }
        other => format!("Unknown tool: {other}"),
    }
}

fn build_system_prompt() -> String {
    r##"You are the Gyre Explorer agent. You help users understand their codebase by generating view queries that visualize the knowledge graph.

You have access to tools that query the knowledge graph. When the user asks a question:

1. Call graph_summary to understand the codebase structure
2. If you need specific nodes, call graph_nodes with a name_pattern
3. If you need relationships, call graph_edges
4. Generate a view query JSON and validate it with graph_query_dryrun
5. If the dry-run has warnings (too many matches, unresolved nodes), refine and dry-run again
6. When satisfied, output the view query in a <view_query>{ ... JSON ... }</view_query> block
7. Also provide a text explanation of what the visualization shows

## View Query Grammar

### Scope Types
- `all`: Show everything
- `focus`: BFS from a node. Fields: node (name or "$clicked"), edges (array of edge types), direction ("outgoing"/"incoming"/"both"), depth (number)
- `filter`: Filter by node_types (array), computed expression, or name_pattern
- `test_gaps`: Functions not reachable from any test
- `concept`: Cross-cutting concept. Fields: seed_nodes (array), expand_edges (array), expand_depth (number)

### Edge Types
calls, contains, implements, depends_on, field_of, returns, routes_to, governed_by

### Emphasis
- `highlight.matched`: { color, label } for matched nodes
- `dim_unmatched`: opacity 0.0-1.0 for non-matched
- `tiered_colors`: array of colors by BFS depth (e.g. ["#ef4444", "#f97316", "#eab308", "#94a3b8"])
- `heat`: { metric, palette } — metric can be incoming_calls, complexity, churn, test_fragility
- `badges`: { template } — e.g. "{{count}} calls"

### Other Fields
- `edges.filter`: array of edge types to show
- `zoom`: "fit" or "current"
- `annotation`: { title, description } — use $name for focused node name, {{count}} for result count
- `groups`: array of { name, nodes: [node names], color }
- `callouts`: array of { node_name, text, color }
- `narrative`: array of { node_name, text, order }

## Rules
1. Be specific with node names — use qualified names when ambiguous
2. Always provide a text explanation alongside the view query
3. Use $selected to reference the node the user clicked
4. For interactive queries, use $clicked so each click re-runs the traversal
5. Keep groups focused (< 20 nodes each)
6. Prefer "fit" zoom so the result is visible
7. ALWAYS call graph_summary first to understand what's in the graph
8. ALWAYS dry-run your view query before finalizing it
9. If a dry-run returns warnings, refine the query and dry-run again

## Output Format
- Text explanation (conversational, concise)
- <view_query>{ ... JSON ... }</view_query> block"##.to_string()
}

/// Parse the LLM response to extract view query blocks from text.
fn parse_view_query_from_text(text: &str) -> (String, Option<serde_json::Value>) {
    let mut text_parts = Vec::new();
    let mut view_query = None;

    let mut remaining = text;
    while let Some(start) = remaining.find("<view_query>") {
        let before = &remaining[..start];
        if !before.trim().is_empty() {
            text_parts.push(before.trim().to_string());
        }

        let after_tag = &remaining[start + 12..];
        if let Some(end) = after_tag.find("</view_query>") {
            let json_str = after_tag[..end].trim();
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(json_str) {
                view_query = Some(parsed);
            }
            remaining = &after_tag[end + 13..];
        } else {
            text_parts.push(remaining.to_string());
            remaining = "";
            break;
        }
    }

    if !remaining.trim().is_empty() {
        text_parts.push(remaining.trim().to_string());
    }

    (text_parts.join("\n\n"), view_query)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_view_query_from_text_with_query() {
        let text = r#"Here is the blast radius for TaskPort.

<view_query>{"scope": {"type": "focus", "node": "TaskPort", "edges": ["calls"], "direction": "incoming", "depth": 5}, "emphasis": {"dim_unmatched": 0.12}, "zoom": "fit", "annotation": {"title": "Blast radius: TaskPort"}}</view_query>

This shows all callers of TaskPort."#;

        let (clean, query) = parse_view_query_from_text(text);
        assert!(clean.contains("blast radius"));
        assert!(clean.contains("callers of TaskPort"));
        assert!(query.is_some());
        let q = query.unwrap();
        assert_eq!(q["scope"]["type"], "focus");
        assert_eq!(q["scope"]["node"], "TaskPort");
    }

    #[test]
    fn test_parse_view_query_no_query() {
        let text = "I don't understand your question. Could you be more specific?";
        let (clean, query) = parse_view_query_from_text(text);
        assert_eq!(clean, text);
        assert!(query.is_none());
    }

    #[test]
    fn test_parse_view_query_only_query() {
        let text = r#"<view_query>{"scope": {"type": "all"}}</view_query>"#;
        let (clean, query) = parse_view_query_from_text(text);
        assert!(clean.is_empty());
        assert!(query.is_some());
    }

    #[test]
    fn test_explorer_tools_are_defined() {
        let tools = explorer_tool_definitions();
        assert_eq!(tools.len(), 5);
        let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"graph_summary"));
        assert!(names.contains(&"graph_query_dryrun"));
        assert!(names.contains(&"graph_nodes"));
        assert!(names.contains(&"graph_edges"));
        assert!(names.contains(&"search"));
    }

    #[test]
    fn test_system_prompt_contains_key_instructions() {
        let prompt = build_system_prompt();
        assert!(prompt.contains("graph_summary"), "missing graph_summary");
        assert!(prompt.contains("graph_query_dryrun"), "missing graph_query_dryrun");
        assert!(prompt.contains("view_query"), "missing view_query");
        assert!(prompt.contains("Scope"), "missing Scope");
        assert!(prompt.contains("Emphasis"), "missing Emphasis");
        assert!(prompt.contains("ALWAYS dry-run"), "missing ALWAYS dry-run");
    }
}
