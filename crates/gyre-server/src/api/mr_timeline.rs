//! GET /api/v1/merge-requests/:id/timeline
//!
//! Assembles the SDLC timeline for an MR from multiple data sources:
//!  - Gate results           → GateResult events
//!  - Agent commits          → GitPush events
//!  - Architectural deltas   → GraphExtraction events
//!  - Workspace messages     → SpecLifecycleTrigger, AgentSpawned, MergeQueueEnqueued, Merged
//!  - MR status (Merged)     → Merged event
//!
//! No new storage is needed — all data is assembled from existing repos.
//!
//! **Deferred event types:**
//! - `ConversationTurn` — requires `TurnCommitLink` records (conversation provenance feature,
//!   PR #413). Add once that feature is available on this branch.
//! - `Notification` — no structured Notification query port exists yet; add when available.

use axum::{
    extract::{Path, State},
    Json,
};
use gyre_common::{message::MessageKind, Id};
use gyre_domain::MrStatus;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::HashSet, sync::Arc};
use tracing::instrument;

use crate::AppState;

use super::error::ApiError;

// ── Response types ──────────────────────────────────────────────────────────

/// A single SDLC timeline event for an MR.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineEvent {
    /// Unix epoch seconds.
    pub timestamp: u64,
    /// Event type discriminant. One of:
    /// SpecLifecycleTrigger, AgentSpawned, AgentCompleted, ConversationTurn,
    /// GitPush, GateResult, GraphExtraction, MergeQueueEnqueued, Merged.
    #[serde(rename = "type")]
    pub event_type: String,
    /// Structured payload specific to the event type.
    pub detail: Value,
}

#[derive(Debug, Serialize)]
pub struct MrTimelineResponse {
    pub mr_id: String,
    pub events: Vec<TimelineEvent>,
}

// ── Handler ─────────────────────────────────────────────────────────────────

/// GET /api/v1/merge-requests/:id/timeline
///
/// Returns the assembled SDLC timeline for the MR. Events are sorted ascending
/// by timestamp. Returns 200 with an empty events array if the MR exists but
/// no timeline data is available yet.
#[instrument(skip(state), fields(mr_id = %mr_id))]
pub async fn get_mr_timeline(
    State(state): State<Arc<AppState>>,
    Path(mr_id): Path<String>,
) -> Result<Json<MrTimelineResponse>, ApiError> {
    let id = Id::new(&mr_id);

    // 1. Fetch the MR — 404 if not found.
    let mr = state
        .merge_requests
        .find_by_id(&id)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("merge request {mr_id} not found")))?;

    let mut events: Vec<TimelineEvent> = Vec::new();
    let workspace_id = mr.workspace_id.clone();
    let repo_id = mr.repository_id.clone();

    // Pre-load quality gates for the repo to resolve gate names in GateResult events.
    let repo_gates: std::collections::HashMap<String, String> = state
        .quality_gates
        .list_by_repo_id(repo_id.as_str())
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|g| (g.id.to_string(), g.name))
        .collect();

    // Pre-load the author agent to resolve persona for AgentSpawned events.
    // Agent.name is used as a best-effort proxy for persona (the exact persona
    // field is not yet stored on Agent; this will be updated when persona tracking
    // is added to the agent record).
    let author_agent_name: Option<String> = if let Some(ref agent_id) = mr.author_agent_id {
        state
            .agents
            .find_by_id(agent_id)
            .await
            .ok()
            .flatten()
            .map(|a| a.name)
    } else {
        None
    };

    // 2. Gate results → GateResult events.
    {
        let gate_results = state.gate_results.list_by_mr_id(&mr_id).await?;
        for result in gate_results {
            let ts = result
                .finished_at
                .or(result.started_at)
                .unwrap_or(mr.created_at);
            let status_str = match result.status {
                gyre_domain::GateStatus::Passed => "pass",
                gyre_domain::GateStatus::Failed => "fail",
                gyre_domain::GateStatus::Running => "running",
                gyre_domain::GateStatus::Pending => "pending",
            };
            // Resolve gate name; fall back to gate_id string if the gate record is missing.
            let gate_name = repo_gates
                .get(&result.gate_id.to_string())
                .cloned()
                .unwrap_or_else(|| result.gate_id.to_string());
            events.push(TimelineEvent {
                timestamp: ts,
                event_type: "GateResult".to_string(),
                detail: serde_json::json!({
                    "gate": gate_name,
                    "status": status_str,
                }),
            });
        }
    }

    // 3. Agent commits → GitPush events + collect commit SHAs for delta cross-ref.
    let mut mr_commit_shas: HashSet<String> = HashSet::new();
    if let Some(ref agent_id) = mr.author_agent_id {
        let commits = state.agent_commits.find_by_agent(agent_id).await?;
        let source_branch = &mr.source_branch;
        // Normalize branch name: strip refs/heads/ prefix for comparison.
        let branch_name = source_branch
            .strip_prefix("refs/heads/")
            .unwrap_or(source_branch);
        for commit in commits {
            // Only commits in the MR's repo and on the source branch.
            if commit.repository_id != repo_id {
                continue;
            }
            let cb = commit.branch.as_str();
            let cb_norm = cb.strip_prefix("refs/heads/").unwrap_or(cb);
            if cb_norm != branch_name {
                continue;
            }
            mr_commit_shas.insert(commit.commit_sha.clone());
            // files_changed is not yet tracked in AgentCommit; will be populated
            // once the field is added to the agent commit record.
            events.push(TimelineEvent {
                timestamp: commit.timestamp,
                event_type: "GitPush".to_string(),
                detail: serde_json::json!({
                    "commit_sha": commit.commit_sha,
                    "agent_id": commit.agent_id.to_string(),
                    "files_changed": null,
                }),
            });
        }
    }

    // 4. Architectural deltas → GraphExtraction events.
    //    Query all deltas for the repo across the MR's lifetime and cross-reference
    //    by commit SHA to ensure they belong to this MR's agent's commits.
    {
        let deltas = state
            .graph_store
            .list_deltas(&repo_id, Some(mr.created_at.saturating_sub(1)), None)
            .await?;
        for delta in deltas {
            // Only include deltas whose commit was part of this MR.
            if !mr_commit_shas.contains(&delta.commit_sha) {
                continue;
            }
            // Parse the delta_json for nodes_added / nodes_modified counts.
            let (nodes_added, nodes_modified) = parse_delta_counts(delta.delta_json.as_str());
            events.push(TimelineEvent {
                timestamp: delta.timestamp,
                event_type: "GraphExtraction".to_string(),
                detail: serde_json::json!({
                    "commit_sha": delta.commit_sha,
                    "nodes_added": nodes_added,
                    "nodes_modified": nodes_modified,
                }),
            });
        }
    }

    // 5. Workspace messages — SpecLifecycleTrigger, AgentSpawned, AgentCompleted,
    //    MergeQueueEnqueued events.
    {
        // We query from the MR's creation time with a generous window.
        // Message.created_at is epoch milliseconds; MR.created_at is epoch seconds.
        let since_ts = mr.created_at.saturating_sub(60) * 1000; // 60s before creation, in ms
        let messages = match state
            .messages
            .list_by_workspace(&workspace_id, None, Some(since_ts), None, None, Some(500))
            .await
        {
            Ok(msgs) => msgs,
            Err(e) => {
                tracing::warn!(
                    mr_id = %mr_id,
                    workspace_id = %workspace_id,
                    error = %e,
                    "failed to load workspace messages for timeline; continuing without message events"
                );
                vec![]
            }
        };

        let author_agent_id_str = mr.author_agent_id.as_ref().map(|a| a.to_string());

        for msg in messages {
            let ts = msg.created_at / 1000; // ms → s
            let payload = msg.payload.as_ref().cloned().unwrap_or(Value::Null);

            match msg.kind {
                MessageKind::SpecChanged => {
                    // SpecLifecycleTrigger: spec changes in this repo during the MR's lifetime.
                    // Strictly filter by repo_id to avoid including unrelated spec changes.
                    let msg_repo_id = payload.get("repo_id").and_then(Value::as_str).unwrap_or("");
                    if msg_repo_id == repo_id.as_str() {
                        let spec_path = payload
                            .get("spec_path")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string();
                        let task_id = payload
                            .get("task_id")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string();
                        events.push(TimelineEvent {
                            timestamp: ts,
                            event_type: "SpecLifecycleTrigger".to_string(),
                            detail: serde_json::json!({
                                "spec_path": spec_path,
                                "task_id": task_id,
                            }),
                        });
                    }
                }

                MessageKind::AgentCreated => {
                    // AgentSpawned: only emit for this MR's author agent.
                    let msg_agent_id = payload
                        .get("agent_id")
                        .and_then(Value::as_str)
                        .map(str::to_string);
                    if author_agent_id_str.is_some() && msg_agent_id == author_agent_id_str {
                        // persona: prefer payload field if present (future), fall back to
                        // agent.name (best-effort proxy until persona is stored on Agent).
                        let persona: Value = payload.get("persona").cloned().unwrap_or_else(|| {
                            author_agent_name
                                .as_deref()
                                .map(|n| Value::String(n.to_string()))
                                .unwrap_or(Value::Null)
                        });
                        events.push(TimelineEvent {
                            timestamp: ts,
                            event_type: "AgentSpawned".to_string(),
                            detail: serde_json::json!({
                                "agent_id": msg_agent_id.unwrap_or_default(),
                                "persona": persona,
                            }),
                        });
                    }
                }

                MessageKind::AgentStatusChanged => {
                    // AgentCompleted: status transitions for this MR's author agent.
                    let msg_agent_id = payload
                        .get("agent_id")
                        .and_then(Value::as_str)
                        .map(str::to_string);
                    let status = payload.get("status").and_then(Value::as_str).unwrap_or("");
                    if author_agent_id_str.is_some()
                        && msg_agent_id == author_agent_id_str
                        && matches!(status, "completed" | "stopped" | "done")
                    {
                        events.push(TimelineEvent {
                            timestamp: ts,
                            event_type: "AgentCompleted".to_string(),
                            detail: serde_json::json!({
                                "agent_id": msg_agent_id.unwrap_or_default(),
                                "status": status,
                            }),
                        });
                    }
                }

                MessageKind::QueueUpdated => {
                    // MergeQueueEnqueued: filter by mr_id in payload.
                    let msg_mr_id = payload.get("mr_id").and_then(Value::as_str).unwrap_or("");
                    if msg_mr_id == mr_id {
                        let position = payload.get("position").and_then(Value::as_u64).unwrap_or(0);
                        events.push(TimelineEvent {
                            timestamp: ts,
                            event_type: "MergeQueueEnqueued".to_string(),
                            detail: serde_json::json!({"position": position}),
                        });
                    }
                }

                MessageKind::MrMerged => {
                    // Merged event from bus message.
                    let msg_mr_id = payload.get("mr_id").and_then(Value::as_str).unwrap_or("");
                    if msg_mr_id == mr_id {
                        events.push(TimelineEvent {
                            timestamp: ts,
                            event_type: "Merged".to_string(),
                            detail: serde_json::json!({}),
                        });
                    }
                }

                _ => {}
            }
        }
    }

    // 6. MR status → Merged event (fallback if no MrMerged bus message found).
    //    Avoid duplicates: only add if we haven't already added a Merged event.
    if matches!(mr.status, MrStatus::Merged) {
        let already_has_merged = events.iter().any(|e| e.event_type == "Merged");
        if !already_has_merged {
            events.push(TimelineEvent {
                timestamp: mr.updated_at,
                event_type: "Merged".to_string(),
                detail: serde_json::json!({}),
            });
        }
    }

    // Sort all events ascending by timestamp.
    events.sort_by_key(|e| e.timestamp);

    Ok(Json(MrTimelineResponse { mr_id, events }))
}

/// Parse `nodes_added` and `nodes_modified` counts from a delta JSON string.
///
/// The delta_json is opaque, but graph_extraction.rs serialises a known shape.
/// We do a best-effort parse and fall back to 0 rather than failing the request.
fn parse_delta_counts(delta_json: &str) -> (u64, u64) {
    if let Ok(v) = serde_json::from_str::<Value>(delta_json) {
        let added = v
            .get("nodes_added")
            .and_then(|v| v.as_u64())
            .or_else(|| {
                v.get("added")
                    .and_then(Value::as_array)
                    .map(|a| a.len() as u64)
            })
            .unwrap_or(0);
        let modified = v
            .get("nodes_modified")
            .and_then(|v| v.as_u64())
            .or_else(|| {
                v.get("modified")
                    .and_then(Value::as_array)
                    .map(|a| a.len() as u64)
            })
            .unwrap_or(0);
        (added, modified)
    } else {
        (0, 0)
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mem::test_state;
    use axum::{body::Body, Router};
    use gyre_common::Id;
    use gyre_domain::{GateResult, GateStatus, GateType, QualityGate};
    use http::{Request, StatusCode};
    use serde::Serialize;
    use tower::ServiceExt;

    fn app() -> Router {
        crate::api::api_router().with_state(test_state())
    }

    async fn body_json(resp: axum::response::Response) -> Value {
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    fn authed_post(uri: &str, body: impl Serialize) -> Request<Body> {
        Request::builder()
            .method("POST")
            .uri(uri)
            .header("content-type", "application/json")
            .header("authorization", "Bearer test-token")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap()
    }

    fn authed_get(uri: &str) -> Request<Body> {
        Request::builder()
            .method("GET")
            .uri(uri)
            .header("authorization", "Bearer test-token")
            .body(Body::empty())
            .unwrap()
    }

    fn authed_put(uri: &str, body: impl Serialize) -> Request<Body> {
        Request::builder()
            .method("PUT")
            .uri(uri)
            .header("content-type", "application/json")
            .header("authorization", "Bearer test-token")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap()
    }

    /// Create a repo with workspace_id "ws-test". Returns (app, repo_id).
    async fn create_repo(app: Router) -> (Router, String) {
        let body = serde_json::json!({"workspace_id": "ws-test", "name": "test-repo"});
        let resp = app
            .clone()
            .oneshot(authed_post("/api/v1/repos", body))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED, "repo creation failed");
        let json = body_json(resp).await;
        let repo_id = json["id"].as_str().unwrap().to_string();
        (app, repo_id)
    }

    async fn create_mr(app: Router, repo_id: &str) -> (Router, String) {
        let body = serde_json::json!({
            "repository_id": repo_id,
            "title": "Test MR",
            "source_branch": "feat/test",
            "target_branch": "main",
        });
        let resp = app
            .clone()
            .oneshot(authed_post("/api/v1/merge-requests", body))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED, "MR creation failed");
        let json = body_json(resp).await;
        let mr_id = json["id"].as_str().unwrap().to_string();
        (app, mr_id)
    }

    #[tokio::test]
    async fn timeline_not_found() {
        let resp = app()
            .oneshot(authed_get("/api/v1/merge-requests/no-such-mr/timeline"))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn timeline_empty_for_new_mr() {
        let app = app();
        let (app, repo_id) = create_repo(app).await;
        let (app, mr_id) = create_mr(app, &repo_id).await;

        let resp = app
            .oneshot(authed_get(&format!(
                "/api/v1/merge-requests/{mr_id}/timeline"
            )))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json["mr_id"], mr_id);
        assert!(json["events"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn timeline_includes_gate_results() {
        let state = test_state();
        let app = crate::api::api_router().with_state(state.clone());
        let (app, repo_id) = create_repo(app).await;
        let (app, mr_id) = create_mr(app, &repo_id).await;

        // Insert a gate and a result.
        let gate = QualityGate {
            id: Id::new("gate-1"),
            repo_id: Id::new(&repo_id),
            name: "cargo-test".to_string(),
            gate_type: GateType::TestCommand,
            command: Some("cargo test".to_string()),
            required_approvals: None,
            persona: None,
            required: true,
            created_at: 1000,
        };
        state.quality_gates.save(&gate).await.unwrap();

        let result = GateResult {
            id: Id::new("result-1"),
            gate_id: Id::new("gate-1"),
            mr_id: Id::new(&mr_id),
            status: GateStatus::Passed,
            output: Some("142 tests passed".to_string()),
            started_at: Some(2000),
            finished_at: Some(2100),
        };
        state.gate_results.save(&result).await.unwrap();

        let resp = app
            .oneshot(authed_get(&format!(
                "/api/v1/merge-requests/{mr_id}/timeline"
            )))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        let events = json["events"].as_array().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["type"], "GateResult");
        assert_eq!(events[0]["detail"]["gate"], "cargo-test");
        assert_eq!(events[0]["detail"]["status"], "pass");
        assert_eq!(events[0]["timestamp"], 2100);
    }

    #[tokio::test]
    async fn timeline_merged_mr_has_merged_event() {
        let state = test_state();
        let app = crate::api::api_router().with_state(state.clone());
        let (app, repo_id) = create_repo(app).await;
        let (app, mr_id) = create_mr(app, &repo_id).await;

        // MR must be approved before it can be merged.
        let approve_resp = app
            .clone()
            .oneshot(authed_put(
                &format!("/api/v1/merge-requests/{mr_id}/status"),
                serde_json::json!({"status": "approved"}),
            ))
            .await
            .unwrap();
        assert_eq!(approve_resp.status(), StatusCode::OK, "approve failed");

        let transition_resp = app
            .clone()
            .oneshot(authed_put(
                &format!("/api/v1/merge-requests/{mr_id}/status"),
                serde_json::json!({"status": "merged"}),
            ))
            .await
            .unwrap();
        assert_eq!(
            transition_resp.status(),
            StatusCode::OK,
            "merge transition failed"
        );

        let resp = app
            .oneshot(authed_get(&format!(
                "/api/v1/merge-requests/{mr_id}/timeline"
            )))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        let events = json["events"].as_array().unwrap();
        // Should have at least one Merged event.
        let merged = events.iter().find(|e| e["type"] == "Merged");
        assert!(merged.is_some(), "expected a Merged event");
    }

    #[tokio::test]
    async fn timeline_git_push_events_from_agent_commits() {
        use gyre_domain::AgentCommit;
        let state = test_state();
        let app = crate::api::api_router().with_state(state.clone());
        let (app, repo_id) = create_repo(app).await;

        // Create MR with an author_agent_id via the REST API so the branch is "feat/test".
        let body = serde_json::json!({
            "repository_id": repo_id,
            "title": "Test MR",
            "source_branch": "feat/test",
            "target_branch": "main",
            "author_agent_id": "agent-42",
        });
        let resp = app
            .clone()
            .oneshot(authed_post("/api/v1/merge-requests", body))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let json = body_json(resp).await;
        let mr_id = json["id"].as_str().unwrap().to_string();

        // Record a commit by the author agent on the source branch.
        let commit = AgentCommit::new(
            Id::new("c-1"),
            Id::new("agent-42"),
            Id::new(&repo_id),
            "deadbeef",
            "feat/test",
            5000,
        );
        state.agent_commits.record(&commit).await.unwrap();

        let resp = app
            .oneshot(authed_get(&format!(
                "/api/v1/merge-requests/{mr_id}/timeline"
            )))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        let events = json["events"].as_array().unwrap();
        let push_events: Vec<_> = events.iter().filter(|e| e["type"] == "GitPush").collect();
        assert_eq!(push_events.len(), 1);
        assert_eq!(push_events[0]["detail"]["commit_sha"], "deadbeef");
        assert_eq!(push_events[0]["timestamp"], 5000);
    }

    #[tokio::test]
    async fn timeline_events_sorted_ascending() {
        use gyre_domain::AgentCommit;
        let state = test_state();
        let app = crate::api::api_router().with_state(state.clone());
        let (app, repo_id) = create_repo(app).await;

        // Create MR with author agent.
        let body = serde_json::json!({
            "repository_id": repo_id,
            "title": "Sort Test MR",
            "source_branch": "feat/sort",
            "target_branch": "main",
            "author_agent_id": "agent-sort",
        });
        let resp = app
            .clone()
            .oneshot(authed_post("/api/v1/merge-requests", body))
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::CREATED,
            "sort-test MR creation failed"
        );
        let json = body_json(resp).await;
        let mr_id = json["id"].as_str().unwrap().to_string();

        // Two commits at different timestamps (later first in storage order).
        for (sha, ts) in [("sha-b", 9000u64), ("sha-a", 3000u64)] {
            let commit = AgentCommit::new(
                Id::new(format!("c-{sha}")),
                Id::new("agent-sort"),
                Id::new(&repo_id),
                sha,
                "feat/sort",
                ts,
            );
            state.agent_commits.record(&commit).await.unwrap();
        }

        // Gate result with timestamp between the two commits.
        let gate = QualityGate {
            id: Id::new("gate-sort"),
            repo_id: Id::new(&repo_id),
            name: "sort-gate".to_string(),
            gate_type: GateType::TestCommand,
            command: None,
            required_approvals: None,
            persona: None,
            required: true,
            created_at: 1000,
        };
        state.quality_gates.save(&gate).await.unwrap();
        let gate_result = GateResult {
            id: Id::new("r-sort"),
            gate_id: Id::new("gate-sort"),
            mr_id: Id::new(&mr_id),
            status: GateStatus::Passed,
            output: None,
            started_at: Some(5000),
            finished_at: Some(6000),
        };
        state.gate_results.save(&gate_result).await.unwrap();

        let resp = app
            .oneshot(authed_get(&format!(
                "/api/v1/merge-requests/{mr_id}/timeline"
            )))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        let events = json["events"].as_array().unwrap();
        assert!(!events.is_empty());

        // Verify strict ascending order.
        let timestamps: Vec<u64> = events
            .iter()
            .map(|e| e["timestamp"].as_u64().unwrap())
            .collect();
        let mut sorted = timestamps.clone();
        sorted.sort();
        assert_eq!(
            timestamps, sorted,
            "events must be sorted ascending by timestamp"
        );
    }

    #[tokio::test]
    async fn timeline_graph_extraction_events() {
        use gyre_common::graph::ArchitecturalDelta;
        use gyre_domain::AgentCommit;
        let state = test_state();
        let app = crate::api::api_router().with_state(state.clone());
        let (app, repo_id) = create_repo(app).await;

        let body = serde_json::json!({
            "repository_id": repo_id,
            "title": "Delta MR",
            "source_branch": "feat/delta",
            "target_branch": "main",
            "author_agent_id": "agent-delta",
        });
        let resp = app
            .clone()
            .oneshot(authed_post("/api/v1/merge-requests", body))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let json = body_json(resp).await;
        let mr_id = json["id"].as_str().unwrap().to_string();
        // Capture the MR's created_at so we can set timestamps after it.
        let mr_created_at = json["created_at"].as_u64().unwrap_or(0);
        let ts_commit = mr_created_at + 10;
        let ts_delta = mr_created_at + 20;

        // Record a commit that will be picked up by the handler.
        let commit = AgentCommit::new(
            Id::new("c-delta"),
            Id::new("agent-delta"),
            Id::new(&repo_id),
            "delta-sha",
            "feat/delta",
            ts_commit,
        );
        state.agent_commits.record(&commit).await.unwrap();

        // Record a delta for the same commit SHA in the same repo.
        let delta = ArchitecturalDelta {
            id: Id::new("d-1"),
            repo_id: Id::new(&repo_id),
            commit_sha: "delta-sha".to_string(),
            timestamp: ts_delta,
            agent_id: Some(Id::new("agent-delta")),
            spec_ref: None,
            delta_json: r#"{"nodes_added": 2, "nodes_modified": 1}"#.to_string(),
        };
        state.graph_store.record_delta(delta).await.unwrap();

        let resp = app
            .oneshot(authed_get(&format!(
                "/api/v1/merge-requests/{mr_id}/timeline"
            )))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        let events = json["events"].as_array().unwrap();

        let graph_events: Vec<_> = events
            .iter()
            .filter(|e| e["type"] == "GraphExtraction")
            .collect();
        assert_eq!(
            graph_events.len(),
            1,
            "expected exactly 1 GraphExtraction event"
        );
        assert_eq!(graph_events[0]["detail"]["commit_sha"], "delta-sha");
        assert_eq!(graph_events[0]["detail"]["nodes_added"], 2);
        assert_eq!(graph_events[0]["detail"]["nodes_modified"], 1);
        assert_eq!(graph_events[0]["timestamp"], ts_delta);
    }

    #[tokio::test]
    async fn timeline_git_push_filters_wrong_branch() {
        use gyre_domain::AgentCommit;
        let state = test_state();
        let app = crate::api::api_router().with_state(state.clone());
        let (app, repo_id) = create_repo(app).await;

        let body = serde_json::json!({
            "repository_id": repo_id,
            "title": "Filter MR",
            "source_branch": "feat/correct",
            "target_branch": "main",
            "author_agent_id": "agent-filter",
        });
        let resp = app
            .clone()
            .oneshot(authed_post("/api/v1/merge-requests", body))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let json = body_json(resp).await;
        let mr_id = json["id"].as_str().unwrap().to_string();

        // Commit on the CORRECT branch — should appear.
        let good_commit = AgentCommit::new(
            Id::new("c-good"),
            Id::new("agent-filter"),
            Id::new(&repo_id),
            "sha-good",
            "feat/correct",
            5000,
        );
        state.agent_commits.record(&good_commit).await.unwrap();

        // Commit on a WRONG branch — must NOT appear.
        let bad_commit = AgentCommit::new(
            Id::new("c-bad"),
            Id::new("agent-filter"),
            Id::new(&repo_id),
            "sha-bad",
            "feat/wrong-branch",
            5001,
        );
        state.agent_commits.record(&bad_commit).await.unwrap();

        let resp = app
            .oneshot(authed_get(&format!(
                "/api/v1/merge-requests/{mr_id}/timeline"
            )))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        let events = json["events"].as_array().unwrap();
        let push_events: Vec<_> = events.iter().filter(|e| e["type"] == "GitPush").collect();
        assert_eq!(
            push_events.len(),
            1,
            "only correct-branch commit should appear"
        );
        assert_eq!(push_events[0]["detail"]["commit_sha"], "sha-good");
    }

    #[tokio::test]
    async fn timeline_git_push_filters_wrong_repo() {
        use gyre_domain::AgentCommit;
        let state = test_state();
        let app = crate::api::api_router().with_state(state.clone());
        let (app, repo_id) = create_repo(app).await;
        let (app, repo_id2) = create_repo(app).await; // second repo

        let body = serde_json::json!({
            "repository_id": repo_id,
            "title": "Repo Filter MR",
            "source_branch": "feat/repo-filter",
            "target_branch": "main",
            "author_agent_id": "agent-repofilt",
        });
        let resp = app
            .clone()
            .oneshot(authed_post("/api/v1/merge-requests", body))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let json = body_json(resp).await;
        let mr_id = json["id"].as_str().unwrap().to_string();

        // Commit in the MR's repo — should appear.
        let good = AgentCommit::new(
            Id::new("c-repo-good"),
            Id::new("agent-repofilt"),
            Id::new(&repo_id),
            "sha-repo-good",
            "feat/repo-filter",
            6000,
        );
        state.agent_commits.record(&good).await.unwrap();

        // Same agent, same branch name, but DIFFERENT repo — must NOT appear.
        let bad = AgentCommit::new(
            Id::new("c-repo-bad"),
            Id::new("agent-repofilt"),
            Id::new(&repo_id2),
            "sha-repo-bad",
            "feat/repo-filter",
            6001,
        );
        state.agent_commits.record(&bad).await.unwrap();

        let resp = app
            .oneshot(authed_get(&format!(
                "/api/v1/merge-requests/{mr_id}/timeline"
            )))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        let events = json["events"].as_array().unwrap();
        let push_events: Vec<_> = events.iter().filter(|e| e["type"] == "GitPush").collect();
        assert_eq!(push_events.len(), 1, "only same-repo commit should appear");
        assert_eq!(push_events[0]["detail"]["commit_sha"], "sha-repo-good");
    }

    #[tokio::test]
    async fn timeline_merged_dedup_exactly_one_merged_event() {
        // The status transition to Merged emits one MrMerged bus message automatically.
        // The handler also has a status-based fallback. Together they could produce two
        // Merged events without dedup logic. Verify exactly one appears.
        let state = test_state();
        let app = crate::api::api_router().with_state(state.clone());
        let (app, repo_id) = create_repo(app).await;
        let (app, mr_id) = create_mr(app, &repo_id).await;

        // Transition MR to Merged (Open → Approved → Merged).
        // The server emits MrMerged to the workspace message bus on this transition.
        let approve = app
            .clone()
            .oneshot(authed_put(
                &format!("/api/v1/merge-requests/{mr_id}/status"),
                serde_json::json!({"status": "approved"}),
            ))
            .await
            .unwrap();
        assert_eq!(approve.status(), StatusCode::OK);
        let merge = app
            .clone()
            .oneshot(authed_put(
                &format!("/api/v1/merge-requests/{mr_id}/status"),
                serde_json::json!({"status": "merged"}),
            ))
            .await
            .unwrap();
        assert_eq!(merge.status(), StatusCode::OK);

        // Now the timeline has: 1 Merged from bus message (step 5) and the fallback
        // (step 6) would add another if dedup is broken. Verify exactly one.
        let resp = app
            .oneshot(authed_get(&format!(
                "/api/v1/merge-requests/{mr_id}/timeline"
            )))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        let events = json["events"].as_array().unwrap();
        let merged_events: Vec<_> = events.iter().filter(|e| e["type"] == "Merged").collect();
        assert_eq!(
            merged_events.len(),
            1,
            "dedup: status fallback must not duplicate bus-message Merged event"
        );
    }

    #[tokio::test]
    async fn parse_delta_counts_handles_malformed_json() {
        let (a, m) = parse_delta_counts("not json at all");
        assert_eq!(a, 0);
        assert_eq!(m, 0);
    }

    #[tokio::test]
    async fn parse_delta_counts_nodes_added_field() {
        let json = r#"{"nodes_added": 3, "nodes_modified": 1}"#;
        let (a, m) = parse_delta_counts(json);
        assert_eq!(a, 3);
        assert_eq!(m, 1);
    }

    #[tokio::test]
    async fn parse_delta_counts_fallback_to_array_length() {
        let json = r#"{"added": ["n1", "n2"], "modified": ["n3"]}"#;
        let (a, m) = parse_delta_counts(json);
        assert_eq!(a, 2);
        assert_eq!(m, 1);
    }
}
