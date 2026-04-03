//! Explorer WebSocket handler.
//!
//! WS /api/v1/repos/:repo_id/explorer
//!
//! Receives user messages with canvas state, runs a multi-turn LLM agent loop
//! with tool calling (graph_summary, graph_query_dryrun, graph_nodes,
//! graph_edges, search), streams text responses and view queries back.
//! Also handles saved view CRUD over the same WebSocket.
//!
//! Auth: Bearer token in initial HTTP upgrade (via AuthenticatedAgent extractor).
//! The WebSocket itself does NOT handle auth messages.

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

/// Max agent refinement turns (spec says 3).
const MAX_AGENT_TURNS: usize = 3;

/// Max messages per session before requiring reconnect (prevents unbounded history).
const MAX_SESSION_MESSAGES: usize = 50;

/// Max conversation history entries before summarization window.
const MAX_CONVERSATION_HISTORY: usize = 20;

/// Minimum interval between user messages (rate limiting), in milliseconds.
const MIN_MESSAGE_INTERVAL_MS: u64 = 1000;

pub async fn explorer_ws(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
    Path(repo_id): Path<String>,
    auth: AuthenticatedAgent,
) -> impl IntoResponse {
    // Repo-scoped authorization: verify the user has access to this repo's tenant.
    // The AuthenticatedAgent extractor already validates the token.
    // Additional ABAC checks happen through the repo lookup below.
    ws.on_upgrade(move |socket| handle_explorer_session(socket, state, repo_id, auth))
}

async fn handle_explorer_session(
    socket: WebSocket,
    state: Arc<AppState>,
    repo_id: String,
    auth: AuthenticatedAgent,
) {
    let (mut sender, mut receiver) = socket.split();

    // Verify the user has access to this repo via workspace → tenant chain.
    let rid = Id::new(&repo_id);
    let repo_workspace_id = match state.repos.find_by_id(&rid).await {
        Ok(Some(repo)) => repo.workspace_id.clone(),
        Ok(None) => {
            let err = ExplorerServerMessage::Error {
                message: format!("Repository not found: {repo_id}"),
            };
            let _ = sender
                .send(Message::Text(serde_json::to_string(&err).unwrap().into()))
                .await;
            return;
        }
        Err(e) => {
            let err = ExplorerServerMessage::Error {
                message: format!("Failed to look up repository: {e}"),
            };
            let _ = sender
                .send(Message::Text(serde_json::to_string(&err).unwrap().into()))
                .await;
            return;
        }
    };
    // Check the workspace belongs to the user's tenant.
    match state.workspaces.find_by_id(&repo_workspace_id).await {
        Ok(Some(ws)) => {
            if ws.tenant_id.as_str() != auth.tenant_id {
                let err = ExplorerServerMessage::Error {
                    message: "Access denied: repo not in your tenant".to_string(),
                };
                let _ = sender
                    .send(Message::Text(serde_json::to_string(&err).unwrap().into()))
                    .await;
                return;
            }
        }
        _ => {
            // If workspace lookup fails, deny access.
            let err = ExplorerServerMessage::Error {
                message: "Access denied: workspace not found".to_string(),
            };
            let _ = sender
                .send(Message::Text(serde_json::to_string(&err).unwrap().into()))
                .await;
            return;
        }
    }

    info!(repo_id = %repo_id, user = %auth.agent_id, "Explorer WebSocket session started");

    // Maintain conversation history across messages within a session.
    let mut conversation_history: Vec<ConversationMessage> = Vec::new();
    let mut message_count: usize = 0;
    let mut last_message_time = std::time::Instant::now()
        .checked_sub(std::time::Duration::from_secs(10))
        .unwrap_or_else(std::time::Instant::now);
    // Cache graph data for the session (avoid re-fetching per message).
    let mut cached_nodes: Option<Vec<gyre_common::graph::GraphNode>> = None;
    let mut cached_edges: Option<Vec<gyre_common::graph::GraphEdge>> = None;

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
                // Rate limiting: enforce minimum interval between messages.
                let now = std::time::Instant::now();
                let elapsed = now.duration_since(last_message_time).as_millis() as u64;
                if elapsed < MIN_MESSAGE_INTERVAL_MS {
                    let err = ExplorerServerMessage::Error {
                        message: "Please wait before sending another message.".to_string(),
                    };
                    let _ = sender
                        .send(Message::Text(serde_json::to_string(&err).unwrap().into()))
                        .await;
                    continue;
                }
                last_message_time = now;

                // Session message limit: prevent unbounded history growth.
                message_count += 1;
                if message_count > MAX_SESSION_MESSAGES {
                    let err = ExplorerServerMessage::Error {
                        message: "Session message limit reached. Please reconnect for a fresh session.".to_string(),
                    };
                    let _ = sender
                        .send(Message::Text(serde_json::to_string(&err).unwrap().into()))
                        .await;
                    continue;
                }

                // Send thinking status
                send_status(&mut sender, "thinking").await;

                // Run the agent loop with conversation history
                match run_explorer_agent(
                    &state,
                    &repo_id,
                    &text,
                    &canvas_state,
                    &mut sender,
                    &mut conversation_history,
                    &mut cached_nodes,
                    &mut cached_edges,
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
                // Use workspace_id from the repo we already validated.
                let workspace_id = match state.repos.find_by_id(&rid).await {
                    Ok(Some(r)) => r.workspace_id.to_string(),
                    _ => String::new(),
                };
                let now = crate::api::now_secs();
                let view = SavedView {
                    id: crate::api::new_id(),
                    repo_id: Id::new(&repo_id),
                    workspace_id: Id::new(&workspace_id),
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
                        // Verify the view belongs to this repo.
                        if v.repo_id.as_str() != repo_id {
                            let err = ExplorerServerMessage::Error {
                                message: "View does not belong to this repository".to_string(),
                            };
                            let _ = sender
                                .send(Message::Text(serde_json::to_string(&err).unwrap().into()))
                                .await;
                            continue;
                        }
                        // Verify tenant access.
                        if v.tenant_id.as_str() != auth.tenant_id {
                            let err = ExplorerServerMessage::Error {
                                message: "Access denied".to_string(),
                            };
                            let _ = sender
                                .send(Message::Text(serde_json::to_string(&err).unwrap().into()))
                                .await;
                            continue;
                        }
                        let query: serde_json::Value = match serde_json::from_str(&v.query_json) {
                            Ok(q) => q,
                            Err(e) => {
                                let err = ExplorerServerMessage::Error {
                                    message: format!("Malformed view query: {e}"),
                                };
                                let _ = sender
                                    .send(Message::Text(
                                        serde_json::to_string(&err).unwrap().into(),
                                    ))
                                    .await;
                                continue;
                            }
                        };
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
                match state.saved_views.list_by_repo(&rid).await {
                    Ok(views) => {
                        // Filter to views in the user's tenant.
                        let summaries: Vec<SavedViewSummary> = views
                            .into_iter()
                            .filter(|v| v.tenant_id.as_str() == auth.tenant_id)
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
) -> bool {
    let msg = ExplorerServerMessage::Status {
        status: status.to_string(),
    };
    sender
        .send(Message::Text(serde_json::to_string(&msg).unwrap().into()))
        .await
        .is_ok()
}

/// Stream text to the client in word-boundary chunks for natural display.
/// Returns false if the client disconnected.
async fn stream_text(
    sender: &mut futures_util::stream::SplitSink<WebSocket, Message>,
    text: &str,
    done: bool,
) -> bool {
    // Stream in word-boundary chunks for more natural progressive display.
    // Aim for ~40-60 char chunks, breaking at word boundaries.
    const TARGET_CHUNK: usize = 50;

    if text.len() <= TARGET_CHUNK || done {
        let msg = ExplorerServerMessage::Text {
            content: text.to_string(),
            done,
        };
        return sender
            .send(Message::Text(serde_json::to_string(&msg).unwrap().into()))
            .await
            .is_ok();
    }

    let mut start = 0;
    let bytes = text.as_bytes();
    while start < bytes.len() {
        let end = (start + TARGET_CHUNK).min(bytes.len());
        // Find word boundary
        let chunk_end = if end >= bytes.len() {
            bytes.len()
        } else {
            // Look backwards for a space
            let mut pos = end;
            while pos > start && bytes[pos] != b' ' && bytes[pos] != b'\n' {
                pos -= 1;
            }
            if pos == start {
                end // No space found, use the target
            } else {
                pos + 1 // Include the space in this chunk
            }
        };

        let chunk = &text[start..chunk_end];
        let is_last = chunk_end >= bytes.len();
        let msg = ExplorerServerMessage::Text {
            content: chunk.to_string(),
            done: is_last && done,
        };
        if sender
            .send(Message::Text(serde_json::to_string(&msg).unwrap().into()))
            .await
            .is_err()
        {
            return false; // Client disconnected
        }
        start = chunk_end;
    }
    true
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
    conversation_history: &mut Vec<ConversationMessage>,
    cached_nodes: &mut Option<Vec<gyre_common::graph::GraphNode>>,
    cached_edges: &mut Option<Vec<gyre_common::graph::GraphEdge>>,
) -> anyhow::Result<()> {
    let llm = match &state.llm {
        Some(llm) => llm.clone(),
        None => {
            stream_text(
                sender,
                "LLM is not configured. Set GYRE_VERTEX_PROJECT and GYRE_VERTEX_LOCATION to enable conversational exploration.",
                true,
            )
            .await;
            return Ok(());
        }
    };

    let model = std::env::var("GYRE_LLM_MODEL").unwrap_or_else(|_| "claude-sonnet-4-6".to_string());
    let llm_port = llm.for_model(&model);

    // Load graph data (cached across messages in the session).
    let rid = Id::new(repo_id);
    if cached_nodes.is_none() {
        *cached_nodes = Some(
            state
                .graph_store
                .list_nodes(&rid, None)
                .await
                .unwrap_or_default(),
        );
        *cached_edges = Some(
            state
                .graph_store
                .list_edges(&rid, None)
                .await
                .unwrap_or_default(),
        );
    }
    let nodes = cached_nodes.as_ref().unwrap();
    let edges = cached_edges.as_ref().unwrap();

    let system_prompt = build_system_prompt();
    let tools = explorer_tool_definitions();

    // Build user message with structured canvas context
    let mut canvas_context_parts = Vec::new();
    if let Some(ref sel) = canvas_state.selected_node {
        let qname = sel.qualified_name.as_deref().unwrap_or(&sel.name);
        canvas_context_parts.push(format!(
            "Selected node: {qname} (type: {}, id: {})",
            sel.node_type, sel.id
        ));
    }
    if !canvas_state.visible_tree_groups.is_empty() {
        canvas_context_parts.push(format!(
            "Visible groups: {}",
            canvas_state.visible_tree_groups.join(", ")
        ));
    }
    if let Some(ref lens) = canvas_state.active_lens {
        if !lens.is_empty() {
            canvas_context_parts.push(format!("Active lens: {lens}"));
        }
    }
    let canvas_summary = if canvas_context_parts.is_empty() {
        String::new()
    } else {
        format!("[Canvas: {}]\n\n", canvas_context_parts.join(" | "))
    };
    let user_content = format!("{canvas_summary}{user_question}");

    // Truncate conversation history to prevent unbounded growth.
    // Keep the first message (for context) and the most recent messages.
    if conversation_history.len() > MAX_CONVERSATION_HISTORY {
        let keep_recent = MAX_CONVERSATION_HISTORY - 2;
        let summary_msg = ConversationMessage {
            role: "user".to_string(),
            content: ConversationContent::Text(
                "[Earlier conversation messages were summarized to save context. The conversation continues below.]".to_string(),
            ),
        };
        let recent: Vec<ConversationMessage> =
            conversation_history.split_off(conversation_history.len() - keep_recent);
        conversation_history.clear();
        conversation_history.push(summary_msg);
        conversation_history.extend(recent);
    }

    // Add to persistent conversation history
    conversation_history.push(ConversationMessage {
        role: "user".to_string(),
        content: ConversationContent::Text(user_content),
    });

    let selected_node_id = canvas_state.selected_node.as_ref().map(|n| n.id.as_str());

    // Multi-turn agent loop with self-check.
    // Separate budgets: tool_turns (max 6) for tool use, refinements (max 3) for view query fixes.
    let mut refinement_count = 0;
    let max_total_turns = MAX_AGENT_TURNS * 2 + 3; // 9 total iterations max
    for turn in 0..max_total_turns {
        let response = llm_port
            .complete_with_tools(&system_prompt, conversation_history, &tools, Some(4096))
            .await?;

        // If the LLM returned text, stream it to the client
        if !response.text.is_empty() {
            let has_tool_calls =
                !response.tool_calls.is_empty() && response.stop_reason == "tool_use";
            let is_final = !has_tool_calls;

            // Check for view_query blocks in the text
            let (clean_text, view_query) = parse_view_query_from_text(&response.text);

            // Stream the clean text (without view_query block)
            if !clean_text.is_empty() {
                if !stream_text(sender, &clean_text, is_final && view_query.is_none()).await {
                    break; // Client disconnected
                }
            }

            // If a view query was found, perform server-enforced self-check
            if let Some(query_json) = view_query {
                // Dry-run the query server-side
                let dry_run_result = if let Ok(query) =
                    serde_json::from_value::<gyre_common::view_query::ViewQuery>(query_json.clone())
                {
                    Some(gyre_domain::view_query_resolver::dry_run(
                        &query,
                        &nodes,
                        &edges,
                        selected_node_id,
                    ))
                } else {
                    None
                };

                if let Some(ref dr) = dry_run_result {
                    if !dr.warnings.is_empty() && refinement_count < MAX_AGENT_TURNS {
                        // Self-check failed: inject dry-run results back to agent for refinement
                        refinement_count += 1;
                        send_status(sender, "refining").await;

                        conversation_history.push(ConversationMessage {
                            role: "assistant".to_string(),
                            content: ConversationContent::Text(response.text.clone()),
                        });

                        let feedback = format!(
                            "The view query had issues during dry-run. Please refine it.\n\nDry-run result:\n- matched_nodes: {}\n- warnings: {:?}\n- matched names (sample): {:?}\n\nPlease fix the warnings and generate an improved <view_query>.",
                            dr.matched_nodes,
                            dr.warnings,
                            &dr.matched_node_names[..dr.matched_node_names.len().min(10)]
                        );
                        conversation_history.push(ConversationMessage {
                            role: "user".to_string(),
                            content: ConversationContent::Text(feedback),
                        });
                        continue;
                    }
                }

                // Send finalized text done before view_query to avoid race
                if !clean_text.is_empty() {
                    let done_msg = ExplorerServerMessage::Text {
                        content: String::new(),
                        done: true,
                    };
                    if sender
                        .send(Message::Text(
                            serde_json::to_string(&done_msg).unwrap().into(),
                        ))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }

                // Send the view query to the frontend
                let view_msg = ExplorerServerMessage::ViewQuery {
                    query: query_json.clone(),
                };
                if sender
                    .send(Message::Text(
                        serde_json::to_string(&view_msg).unwrap().into(),
                    ))
                    .await
                    .is_err()
                {
                    break;
                }
            }
        }

        // If no tool calls or stop_reason indicates completion, we're done
        if response.tool_calls.is_empty() || response.stop_reason != "tool_use" {
            // Add final assistant response to conversation history
            conversation_history.push(ConversationMessage {
                role: "assistant".to_string(),
                content: ConversationContent::Text(response.text.clone()),
            });

            // If max_tokens, send a truncation warning
            if response.stop_reason == "max_tokens" && !response.text.is_empty() {
                let done_msg = ExplorerServerMessage::Text {
                    content: "\n\n*(Response truncated due to length)*".to_string(),
                    done: true,
                };
                let _ = sender
                    .send(Message::Text(
                        serde_json::to_string(&done_msg).unwrap().into(),
                    ))
                    .await;
            }
            break;
        }

        if !send_status(sender, "refining").await {
            break; // Client disconnected
        }

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
        conversation_history.push(ConversationMessage {
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
        conversation_history.push(ConversationMessage {
            role: "user".to_string(),
            content: ConversationContent::Blocks(result_blocks),
        });

        // Safety: prevent runaway loops
        if turn >= max_total_turns - 1 {
            info!("Explorer agent hit max turns, forcing final response");
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
            let summary =
                gyre_domain::view_query_resolver::compute_graph_summary(repo_id, nodes, edges);
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
                    serde_json::to_string_pretty(&result).unwrap_or_else(|e| format!("Error: {e}"))
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
    r##"You are the Gyre Explorer agent. You help humans understand codebases they didn't write by generating interactive visualizations of the knowledge graph.

## Context
User messages may include a [Canvas: ...] prefix showing what's currently selected/visible.
- "Selected node: X" means the user clicked on X — $selected resolves to it
- "Visible groups: A, B" means those tree groups are expanded on screen
- "Active lens: structural|evaluative|trace" shows the current analysis mode

## Workflow
1. Call graph_summary to understand the codebase structure
2. If you need specific nodes, call graph_nodes with a name_pattern
3. If you need relationships, call graph_edges
4. Generate a view query JSON and validate it with graph_query_dryrun
5. If the dry-run has warnings, refine and dry-run again
6. Output the view query in a <view_query>{ ... JSON ... }</view_query> block
7. Provide a text explanation of what the visualization reveals

## Grounding Rules
- EVERY claim must be traceable to actual nodes/edges from tool results
- When naming nodes, use the exact qualified_name from graph_nodes results
- Never invent node names — always verify via graph_nodes or search first
- When explaining structure, cite the specific edge types connecting nodes
- If you're unsure whether a node exists, search for it before referencing it

## View Query Grammar

### Scope Types
- `all`: Show everything
- `focus`: BFS from a node. Fields: node (name or "$clicked"), edges (array), direction ("outgoing"/"incoming"/"both"), depth (number)
- `filter`: Filter by node_types (array), computed (expression), or name_pattern
- `test_gaps`: Functions not reachable from any test
- `diff`: Changed nodes between commits. Fields: from_commit, to_commit
- `concept`: Cross-cutting concept. Fields: seed_nodes (array), expand_edges (array), expand_depth (number), expand_direction ("outgoing"/"incoming"/"both")

### Edge Types
calls, contains, implements, depends_on, field_of, returns, routes_to, governed_by

### Computed Expressions (for filter scope)
- `$where(property, 'op', value)` — property: complexity, churn, test_coverage, incoming_calls, outgoing_calls, field_count, test_fragility. op: >, >=, <, <=, ==
- `$callers(node, depth?)`, `$callees(node, depth?)` — call graph traversal
- `$implementors(trait)` — types implementing a trait
- `$fields(type)` — fields of a type
- `$descendants(module)`, `$ancestors(node)` — containment hierarchy
- `$governed_by(spec_path)` — nodes governed by a spec
- `$test_unreachable`, `$test_reachable` — test coverage
- `$intersect(A, B)`, `$union(A, B)`, `$diff(A, B)` — set operations on expressions
- `$reachable(node, [edge_types], direction, depth)` — general BFS

### Emphasis
- `highlight`: { matched: { color, label } } for matched nodes
- `dim_unmatched`: opacity 0.0-1.0 for non-matched
- `tiered_colors`: array of colors by BFS depth (e.g. ["#ef4444", "#f97316", "#eab308", "#94a3b8"])
- `heat`: { metric, palette } — metric: incoming_calls, complexity, churn, test_fragility, test_coverage
- `badges`: { template } — e.g. "{{count}} calls"

### Other Fields
- `edges`: { filter: [edge types] }
- `zoom`: "fit" or "current"
- `annotation`: { title, description } — $name for focused node name, {{count}} for result count, {{group_count}} for group count
- `groups`: array of { name, nodes: [qualified node names], color }
- `callouts`: array of { node: "qualified_name", text, color }
- `narrative`: array of { node: "qualified_name", text, order }

## Rules
1. Use exact qualified names from tool results — never guess
2. Always explain what the visualization reveals and why it matters
3. Use $selected when referring to the user's clicked node
4. For interactive (click-to-explore) queries, use $clicked in the focus node
5. Keep groups focused (< 20 nodes each)
6. Prefer "fit" zoom
7. ALWAYS call graph_summary first
8. ALWAYS dry-run before finalizing
9. Refine if dry-run returns warnings

## Output Format
- Conversational explanation (what it shows, what's interesting, what to look at)
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
        assert!(
            prompt.contains("graph_query_dryrun"),
            "missing graph_query_dryrun"
        );
        assert!(prompt.contains("view_query"), "missing view_query");
        assert!(prompt.contains("Scope"), "missing Scope");
        assert!(prompt.contains("Emphasis"), "missing Emphasis");
        assert!(prompt.contains("ALWAYS dry-run"), "missing ALWAYS dry-run");
    }

    #[test]
    fn test_max_agent_turns_is_three() {
        assert_eq!(MAX_AGENT_TURNS, 3, "Spec requires max 3 refinement turns");
    }

    #[test]
    fn test_client_message_deserialization() {
        // Verify the server correctly deserializes the expected message format.
        let msg_json =
            r#"{"type":"message","text":"What is TaskPort?","canvas_state":{"zoom_level":1.5}}"#;
        let msg: ExplorerClientMessage = serde_json::from_str(msg_json).unwrap();
        match msg {
            ExplorerClientMessage::Message { text, canvas_state } => {
                assert_eq!(text, "What is TaskPort?");
                assert!((canvas_state.zoom_level - 1.5).abs() < f64::EPSILON);
            }
            _ => panic!("Expected Message variant"),
        }
    }

    #[test]
    fn test_save_view_message_with_all_fields() {
        let msg_json = r#"{"type":"save_view","name":"My View","description":"A test view","query":{"scope":{"type":"all"}}}"#;
        let msg: ExplorerClientMessage = serde_json::from_str(msg_json).unwrap();
        match msg {
            ExplorerClientMessage::SaveView {
                name,
                description,
                query,
            } => {
                assert_eq!(name, "My View");
                assert_eq!(description.as_deref(), Some("A test view"));
                assert_eq!(query["scope"]["type"], "all");
            }
            _ => panic!("Expected SaveView variant"),
        }
    }

    #[test]
    fn test_server_message_serialization() {
        // Verify streamed text format
        let msg = ExplorerServerMessage::Text {
            content: "Hello".to_string(),
            done: false,
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains(r#""type":"text""#));
        assert!(json.contains(r#""done":false"#));

        // Verify view query format
        let msg = ExplorerServerMessage::ViewQuery {
            query: json!({"scope": {"type": "all"}}),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains(r#""type":"view_query""#));
    }

    #[test]
    fn test_grounding_and_canvas_instructions() {
        let prompt = build_system_prompt();
        assert!(
            prompt.contains("Grounding Rules"),
            "System prompt should contain grounding instructions"
        );
        assert!(
            prompt.contains("Canvas"),
            "System prompt should explain canvas state format"
        );
        assert!(
            prompt.contains("EVERY claim must be traceable"),
            "System prompt should require grounded claims"
        );
    }
}
