//! Explorer WebSocket handler.
//!
//! WS /api/v1/repos/:repo_id/explorer
//!
//! Receives user messages with canvas state, spawns a Claude Agent SDK query
//! via a Node.js subprocess, streams text responses and view queries back.
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
use std::sync::Arc;
use tracing::{info, warn};

use crate::{auth::AuthenticatedAgent, AppState};

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
                let status = ExplorerServerMessage::Status {
                    status: "thinking".to_string(),
                };
                let _ = sender
                    .send(Message::Text(serde_json::to_string(&status).unwrap().into()))
                    .await;

                // Build graph context for the LLM
                let rid = Id::new(&repo_id);
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

                let summary = gyre_domain::view_query_resolver::compute_graph_summary(
                    &repo_id, &nodes, &edges,
                );

                // Build the prompt for the LLM explorer agent
                let canvas_json =
                    serde_json::to_string(&canvas_state).unwrap_or_else(|_| "{}".to_string());
                let summary_json =
                    serde_json::to_string(&summary).unwrap_or_else(|_| "{}".to_string());

                let system_prompt = build_system_prompt(&summary_json);
                let user_prompt = format!(
                    "Canvas state:\n```json\n{canvas_json}\n```\n\nUser question: {text}"
                );

                // Try to use the LLM port directly for the explorer query
                match run_explorer_query(
                    &state,
                    &system_prompt,
                    &user_prompt,
                    &repo_id,
                    &nodes,
                    &edges,
                    canvas_state.selected_node.as_ref().map(|n| n.id.as_str()),
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

fn build_system_prompt(graph_summary: &str) -> String {
    format!(
        r#"You are the Gyre Explorer agent. You help users understand their codebase by generating view queries that visualize the knowledge graph.

## Knowledge Graph Summary
{graph_summary}

## View Query Grammar

When the user asks a question, generate a view query JSON that visualizes the answer on the explorer canvas. Output the query in a <view_query>...</view_query> block.

### Scope Types
- `all`: Show everything
- `focus`: BFS from a node. Fields: node (name or "$clicked"), edges (array of edge types), direction ("outgoing"/"incoming"/"both"), depth (number)
- `filter`: Filter by node_types (array) or computed expression
- `test_gaps`: Functions not reachable from any test
- `concept`: Cross-cutting concept. Fields: seed_nodes (array), expand_edges (array), expand_depth (number)

### Edge Types
calls, contains, implements, depends_on, field_of, returns, routes_to, governed_by

### Emphasis
- `highlight.matched`: {{ color, label }} for matched nodes
- `dim_unmatched`: opacity 0.0-1.0 for non-matched
- `tiered_colors`: array of colors by BFS depth
- `heat`: {{ metric, palette }} — metric can be incoming_calls, complexity, churn, test_fragility

### Other Fields
- `edges.filter`: array of edge types to show
- `zoom`: "fit" or "current"
- `annotation`: {{ title, description }} — use $name for focused node name, {{{{count}}}} for result count
- `groups`: array of {{ name, nodes: [patterns], color }}
- `callouts`: array of {{ node, text, color }}
- `narrative`: array of {{ node, text, order }}

## Rules
1. Be specific with node names — use qualified names to avoid matching too many nodes
2. Always provide a text explanation alongside the view query
3. Use $selected to reference the node the user clicked
4. For interactive queries, use $clicked so each click re-runs the traversal
5. Keep groups focused (< 20 nodes each)
6. Prefer "fit" zoom so the result is visible

## Output Format
- Text explanation (conversational, concise)
- <view_query>{{ ... JSON ... }}</view_query> block"#
    )
}

/// Run the explorer query using the LLM port.
async fn run_explorer_query(
    state: &AppState,
    system_prompt: &str,
    user_prompt: &str,
    _repo_id: &str,
    nodes: &[gyre_common::graph::GraphNode],
    edges: &[gyre_common::graph::GraphEdge],
    selected_node_id: Option<&str>,
    sender: &mut futures_util::stream::SplitSink<WebSocket, Message>,
) -> anyhow::Result<()> {
    let llm = match &state.llm {
        Some(llm) => llm.clone(),
        None => {
            // No LLM configured — return a helpful message
            let text_msg = ExplorerServerMessage::Text {
                content: "LLM is not configured. Set GYRE_VERTEX_PROJECT and GYRE_VERTEX_LOCATION to enable conversational exploration.".to_string(),
                done: true,
            };
            sender
                .send(Message::Text(
                    serde_json::to_string(&text_msg).unwrap().into(),
                ))
                .await?;
            return Ok(());
        }
    };

    // Create the LLM port — use a fast model for explorer queries
    let model = std::env::var("GYRE_EXPLORER_MODEL")
        .unwrap_or_else(|_| "claude-sonnet-4-6".to_string());
    let llm_port = llm.for_model(&model);

    // Send refining status
    let status = ExplorerServerMessage::Status {
        status: "refining".to_string(),
    };
    sender
        .send(Message::Text(serde_json::to_string(&status).unwrap().into()))
        .await?;

    // Call LLM
    let response = llm_port
        .complete(system_prompt, user_prompt, Some(4096))
        .await?;

    // Parse the response for view queries and text
    let (text_content, view_query) = parse_llm_response(&response);

    // Send text response
    if !text_content.is_empty() {
        let text_msg = ExplorerServerMessage::Text {
            content: text_content,
            done: true,
        };
        sender
            .send(Message::Text(
                serde_json::to_string(&text_msg).unwrap().into(),
            ))
            .await?;
    }

    // If we got a view query, dry-run it and send
    if let Some(query_json) = view_query {
        // Try to parse and dry-run
        if let Ok(query) = serde_json::from_value::<gyre_common::view_query::ViewQuery>(
            query_json.clone(),
        ) {
            let dry_run = gyre_domain::view_query_resolver::dry_run(
                &query,
                nodes,
                edges,
                selected_node_id,
            );

            // If there are warnings and this is the first try, we could refine
            // For now, send the query as-is with any warnings
            if !dry_run.warnings.is_empty() {
                let warn_text = format!(
                    "Note: {}",
                    dry_run.warnings.join("; ")
                );
                let text_msg = ExplorerServerMessage::Text {
                    content: warn_text,
                    done: true,
                };
                sender
                    .send(Message::Text(
                        serde_json::to_string(&text_msg).unwrap().into(),
                    ))
                    .await?;
            }
        }

        let view_msg = ExplorerServerMessage::ViewQuery {
            query: query_json,
        };
        sender
            .send(Message::Text(
                serde_json::to_string(&view_msg).unwrap().into(),
            ))
            .await?;
    }

    // Send ready status
    let status = ExplorerServerMessage::Status {
        status: "ready".to_string(),
    };
    sender
        .send(Message::Text(serde_json::to_string(&status).unwrap().into()))
        .await?;

    Ok(())
}

/// Parse the LLM response to extract text and view query blocks.
fn parse_llm_response(response: &str) -> (String, Option<serde_json::Value>) {
    let mut text_parts = Vec::new();
    let mut view_query = None;

    // Extract <view_query>...</view_query> blocks
    let mut remaining = response;
    while let Some(start) = remaining.find("<view_query>") {
        // Add text before the block
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
            // Unclosed tag — treat rest as text
            text_parts.push(remaining.to_string());
            remaining = "";
            break;
        }
    }

    // Add any remaining text
    if !remaining.trim().is_empty() {
        text_parts.push(remaining.trim().to_string());
    }

    (text_parts.join("\n\n"), view_query)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_llm_response_with_view_query() {
        let response = r#"Here is the blast radius for TaskPort.

<view_query>{"scope": {"type": "focus", "node": "TaskPort", "edges": ["calls"], "direction": "incoming", "depth": 5}, "emphasis": {"dim_unmatched": 0.12}, "zoom": "fit", "annotation": {"title": "Blast radius: TaskPort"}}</view_query>

This shows all callers of TaskPort."#;

        let (text, query) = parse_llm_response(response);
        assert!(text.contains("blast radius"));
        assert!(text.contains("callers of TaskPort"));
        assert!(query.is_some());
        let q = query.unwrap();
        assert_eq!(q["scope"]["type"], "focus");
        assert_eq!(q["scope"]["node"], "TaskPort");
    }

    #[test]
    fn test_parse_llm_response_no_query() {
        let response = "I don't understand your question. Could you be more specific?";
        let (text, query) = parse_llm_response(response);
        assert_eq!(text, response);
        assert!(query.is_none());
    }

    #[test]
    fn test_parse_llm_response_only_query() {
        let response = r#"<view_query>{"scope": {"type": "all"}}</view_query>"#;
        let (text, query) = parse_llm_response(response);
        assert!(text.is_empty());
        assert!(query.is_some());
    }
}
