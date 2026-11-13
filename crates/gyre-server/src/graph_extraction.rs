//! Push-triggered knowledge graph extraction (M30b) and divergence detection (HSI §8 priority 5).
//!
//! After each successful git push, extracts architectural knowledge from the
//! repository source tree and persists nodes, edges, and an architectural
//! delta to the graph store.  Runs as a background task and never blocks the
//! push response.
//!
//! When the push was made by an agent working on a specific spec, a post-extraction
//! divergence check compares the recorded delta against recent deltas from other
//! agents targeting the same spec.  Conflicting interpretations generate priority-5
//! inbox notifications for all Admin and Developer workspace members.
use gyre_common::{
    call_graph::{CallGraphRunRecord, Language},
    graph::{
        ArchitecturalDelta, DeltaEdgeEntry, DeltaNodeEntry, EdgeType, FieldChange, GraphEdge,
        GraphNode,
    },
    Id, Notification, NotificationType,
};
use gyre_domain::{
    GoExtractor, LanguageExtractor, PythonExtractor, RustExtractor, TypeScriptExtractor,
    WorkspaceRole,
};
use gyre_ports::{GraphPort, KvJsonStore, NotificationRepository, WorkspaceMembershipRepository};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::process::Command;
use tracing::{info, warn};
use uuid::Uuid;

/// KV-store namespace for the durable Pass 2 completion marker
/// (`CallGraphRunRecord`, keyed by repo id). Read by the `graph_summary` and
/// `graph_query_dryrun` MCP tools to report call graph completeness
/// (specs/system/lsp-call-graph.md §8).
pub const CALL_GRAPH_STATUS_NAMESPACE: &str = "call_graph_status";

/// Server-side telemetry sinks for the Pass 2 call graph extraction
/// (task-076, lsp-call-graph.md §9): the Prometheus metrics registry and the
/// KV store that holds the durable per-repo completion marker.
pub struct CallGraphTelemetry {
    pub kv: Arc<dyn KvJsonStore>,
    pub metrics: Arc<crate::metrics::Metrics>,
}

/// Outcome of a Pass 2 run, consumed by the telemetry/marker path.
#[derive(Debug, Default)]
pub struct Pass2Outcome {
    /// Positive evidence that at least one language toolchain actually ran
    /// (non-empty raw output). Gates the completeness marker: a missing or
    /// failed toolchain degrades to empty output and must NOT mark the call
    /// graph complete.
    pub extraction_ran: bool,
    /// New `Calls` edges persisted by this run.
    pub edges_persisted: usize,
    /// Wall-clock duration of each language's extraction (§9 metric).
    pub per_language: Vec<(Language, std::time::Duration)>,
}

/// Optional agent context attached to a push-triggered extraction.
///
/// When present, the extraction records agent identity and spec reference in the
/// delta and runs a post-extraction divergence check.
pub struct AgentPushContext {
    /// ID of the agent that made this push.
    pub agent_id: String,
    /// Spec path this agent is implementing (from the agent's current task).
    pub spec_ref: String,
    /// Workspace ID — used to find Admin/Developer members for notifications.
    pub workspace_id: String,
    /// Tenant ID — stored on notifications.
    pub tenant_id: String,
}

/// Post-create delivery-channel fan-out callback (email/webhook/slack per
/// user-management.md §Delivery Channels). `None` in port-level tests.
pub type ChannelFanoutCallback =
    Box<dyn Fn(&Notification) -> futures_util::future::BoxFuture<'static, ()> + Send + Sync>;

/// Workspace/tenant identity for push-scoped notifications (system-explorer §9).
///
/// Derived at each extraction call site from the repo's own workspace, so
/// assertion-failure notifications fire for every push — including CLI/user
/// pushes and mirror syncs, which carry no agent context. Without a scope
/// (repo workspace unresolvable) notifications are skipped and the failure
/// is only persisted.
pub struct PushNotificationScope {
    pub workspace_id: String,
    pub tenant_id: String,
}

/// Port references needed for push-scoped notifications and the divergence
/// check (spec assertion failures use the notification + membership repos).
///
/// Bundled into a single struct to stay within clippy's argument-count limit.
///
/// `notification_repo` must be the broadcasting wrapper
/// (`AppState::broadcasting_notifications()`) in production: a bare
/// repository handle persists fine but bypasses the `/notifications/stream`
/// SSE fan-out, so connected clients never see the notification
/// (task-125 review finding).
pub struct DivergencePorts<'a> {
    pub notification_repo: &'a dyn NotificationRepository,
    pub membership_repo: &'a dyn WorkspaceMembershipRepository,
    /// Post-create channel fan-out (email/webhook/slack per
    /// user-management.md §Delivery Channels). `None` in port-level tests.
    pub on_created: Option<ChannelFanoutCallback>,
}

/// Identity/scope parameters for a divergence check.
///
/// Bundled to keep `check_divergence` within clippy's 7-argument limit.
pub struct DivergenceScope<'a> {
    pub spec_ref: &'a str,
    pub current_agent_id: &'a str,
    pub workspace_id: &'a str,
    pub tenant_id: &'a str,
}

/// Extract and persist the knowledge graph for a repo at a specific commit.
///
/// Uses `git archive` to snapshot the tree, runs the Rust extractor, clears
/// stale graph data for the repo, then persists the new nodes, edges, and an
/// [`ArchitecturalDelta`] record.
///
/// When `agent_ctx` is provided, the delta is enriched with agent identity and
/// spec reference, and a post-extraction divergence check is performed.
///
/// `notification_scope` carries the repo's workspace/tenant identity for
/// spec-assertion-failure notifications (§9) — independent of `agent_ctx`, so
/// notifications also fire for user and mirror pushes.
///
/// `pass2_telemetry` receives the Pass 2 (call graph) duration metric and
/// persists the durable per-repo completion marker (lsp-call-graph.md §8–9).
///
/// All errors are logged and swallowed — extraction must never fail a push.
#[allow(clippy::too_many_arguments)]
pub async fn extract_and_store_graph(
    repo_path: &str,
    repo_id: &str,
    new_sha: &str,
    graph_store: Arc<dyn GraphPort>,
    git_bin: &str,
    agent_ctx: Option<AgentPushContext>,
    divergence_ports: Option<DivergencePorts<'_>>,
    results_repo: Arc<dyn gyre_ports::SpecAssertionResultRepository>,
    notification_scope: Option<PushNotificationScope>,
    pass2_telemetry: Arc<CallGraphTelemetry>,
) {
    if let Err(e) = do_extract(
        repo_path,
        repo_id,
        new_sha,
        graph_store,
        git_bin,
        agent_ctx,
        divergence_ports,
        results_repo,
        notification_scope,
        pass2_telemetry,
    )
    .await
    {
        warn!(%repo_id, %new_sha, "knowledge graph extraction failed: {e}");
    }
}

#[allow(clippy::too_many_arguments)]
async fn do_extract(
    repo_path: &str,
    repo_id: &str,
    new_sha: &str,
    graph_store: Arc<dyn GraphPort>,
    git_bin: &str,
    agent_ctx: Option<AgentPushContext>,
    divergence_ports: Option<DivergencePorts<'_>>,
    results_repo: Arc<dyn gyre_ports::SpecAssertionResultRepository>,
    notification_scope: Option<PushNotificationScope>,
    pass2_telemetry: Arc<CallGraphTelemetry>,
) -> anyhow::Result<()> {
    // --- Step 1: snapshot the commit tree into a temp directory ---------------

    let tmp = tempfile::TempDir::new()?;

    // `git archive --format=tar <sha>` on a bare repo outputs a tar stream.
    let archive = Command::new(git_bin)
        .args(["-C", repo_path, "archive", "--format=tar", new_sha])
        .output()
        .await?;

    if !archive.status.success() {
        anyhow::bail!(
            "git archive failed: {}",
            String::from_utf8_lossy(&archive.stderr)
        );
    }

    // Write tar bytes to a file then extract with the system tar binary.
    let archive_path = tmp.path().join("archive.tar");
    tokio::fs::write(&archive_path, &archive.stdout).await?;

    let tar_status = Command::new("tar")
        .args([
            "-x",
            "-C",
            tmp.path().to_str().unwrap_or(""),
            "-f",
            archive_path.to_str().unwrap_or(""),
        ])
        .status()
        .await?;

    if !tar_status.success() {
        anyhow::bail!("tar extraction failed (exit code: {:?})", tar_status.code());
    }

    // --- Step 2: run the Rust extractor (blocking, CPU-intensive) -------------

    let tmp_path = tmp.path().to_path_buf();
    let sha_str = new_sha.to_string();
    let repo_id_str = repo_id.to_string();

    let (nodes, edges) =
        tokio::task::spawn_blocking(move || run_all_extractors(&tmp_path, &sha_str, &repo_id_str))
            .await?;

    if nodes.is_empty() && edges.is_empty() {
        info!(
            %repo_id,
            %new_sha,
            "graph extraction: no nodes found (non-Rust or empty repo)"
        );
        return Ok(());
    }

    let repo_id_parsed = Id::new(repo_id.to_string());

    // --- Step 3: load existing graph state for incremental diff ---------------

    let old_nodes = graph_store.list_nodes(&repo_id_parsed, None).await?;
    let old_edges = graph_store.list_edges(&repo_id_parsed, None).await?;

    // Build lookup map: qualified_name → existing GraphNode
    let old_node_map: HashMap<String, GraphNode> = old_nodes
        .into_iter()
        .map(|n| (n.qualified_name.clone(), n))
        .collect();

    // Build lookup map: edge key → existing GraphEdge
    let old_edge_map: HashMap<(String, String, String), gyre_common::graph::GraphEdge> = old_edges
        .into_iter()
        .map(|e| {
            let key = (
                e.source_id.as_str().to_string(),
                e.target_id.as_str().to_string(),
                edge_type_key(&e.edge_type).to_string(),
            );
            (key, e)
        })
        .collect();

    // Timestamp used for all time-travel fields in this extraction pass.
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    // --- Step 4: compute node diff + stabilise IDs ----------------------------

    // Maps newly-generated extractor UUID → existing stable UUID (for edge remapping).
    let mut id_remap: HashMap<String, String> = HashMap::new();

    // Delta tracking
    let mut delta_nodes_added: Vec<DeltaNodeEntry> = Vec::new();
    let mut delta_nodes_modified: Vec<serde_json::Value> = Vec::new();
    let mut new_qn_set: HashSet<String> = HashSet::new();

    let mut final_nodes: Vec<GraphNode> = nodes;

    for node in &mut final_nodes {
        new_qn_set.insert(node.qualified_name.clone());

        // Always stamp last_seen_at and clear any prior soft-delete.
        node.last_seen_at = now;
        node.deleted_at = None;

        if let Some(old) = old_node_map.get(&node.qualified_name) {
            // Existing node: remap ID, preserve immutable creation/first-seen metadata.
            id_remap.insert(node.id.as_str().to_string(), old.id.as_str().to_string());
            node.id = old.id.clone();
            node.created_sha = old.created_sha.clone();
            node.created_at = old.created_at;
            node.first_seen_at = old.first_seen_at;

            let changes = diff_nodes(old, node);
            if !changes.is_empty() {
                delta_nodes_modified.push(serde_json::json!({
                    "qualified_name": node.qualified_name,
                    "field_changes": changes.iter().map(|fc| serde_json::json!({
                        "field": fc.field,
                        "old_value": fc.old_value,
                        "new_value": fc.new_value,
                    })).collect::<Vec<_>>(),
                }));
            }
        } else {
            // New node: set first_seen_at.
            node.first_seen_at = now;
            delta_nodes_added.push(DeltaNodeEntry {
                name: node.name.clone(),
                node_type: format!("{:?}", node.node_type).to_lowercase(),
                qualified_name: node.qualified_name.clone(),
            });
        }
    }

    // Nodes in old but not in new → soft-delete.
    let mut delta_nodes_removed: Vec<String> = Vec::new();
    for (qn, old_node) in &old_node_map {
        if !new_qn_set.contains(qn) {
            delta_nodes_removed.push(qn.clone());
            graph_store.delete_node(&old_node.id).await?;
        }
    }

    // Upsert ALL nodes that appear in the new extraction (updates last_seen_at for unchanged ones).
    for node in &final_nodes {
        graph_store.create_node(node.clone()).await?;
    }

    // --- Step 5: compute and apply edge diff ----------------------------------

    // Remap edge source/target IDs to stable existing IDs where applicable.
    let mut new_edge_map: HashMap<(String, String, String), gyre_common::graph::GraphEdge> =
        HashMap::new();
    for mut edge in edges {
        if let Some(stable) = id_remap.get(edge.source_id.as_str()) {
            edge.source_id = Id::new(stable.clone());
        }
        if let Some(stable) = id_remap.get(edge.target_id.as_str()) {
            edge.target_id = Id::new(stable.clone());
        }
        edge.last_seen_at = now;
        edge.deleted_at = None;
        let key = (
            edge.source_id.as_str().to_string(),
            edge.target_id.as_str().to_string(),
            edge_type_key(&edge.edge_type).to_string(),
        );
        new_edge_map.insert(key, edge);
    }

    // Edge delta detail for narrative rendering (realized-model.md §6 edge
    // template): qualified names resolved from the merged node state — new
    // edges against the final node map, removed edges against the old one
    // (their endpoints are soft-deleted after this pass).
    let qn_by_id: HashMap<&str, &str> = final_nodes
        .iter()
        .map(|n| (n.id.as_str(), n.qualified_name.as_str()))
        .collect();
    let old_qn_by_id: HashMap<&str, &str> = old_node_map
        .values()
        .map(|n| (n.id.as_str(), n.qualified_name.as_str()))
        .collect();
    let mut delta_edges_added: Vec<DeltaEdgeEntry> = Vec::new();
    let mut delta_edges_removed: Vec<DeltaEdgeEntry> = Vec::new();

    let mut edges_added_count: usize = 0;
    let mut edges_removed_count: usize = 0;

    for (key, edge) in new_edge_map.iter_mut() {
        if let Some(old) = old_edge_map.get(key) {
            // Existing edge: preserve stable ID and first_seen_at, update last_seen_at.
            edge.id = old.id.clone();
            edge.first_seen_at = old.first_seen_at;
            graph_store.create_edge(edge.clone()).await?;
        } else {
            // New edge.
            edge.first_seen_at = now;
            graph_store.create_edge(edge.clone()).await?;
            edges_added_count += 1;
            if let Some(e) = edge_entry(edge, &qn_by_id) {
                delta_edges_added.push(e);
            }
        }
    }
    edges_removed_count += sweep_stale_edges(
        &old_edge_map,
        &new_edge_map,
        graph_store.as_ref(),
        &old_qn_by_id,
        &mut delta_edges_removed,
    )
    .await?;

    let node_count = final_nodes.len();
    let edge_count = new_edge_map.len();

    // --- Step 6: record an architectural delta --------------------------------

    let (delta_json, delta_agent_id, delta_spec_ref) = if let Some(ref ctx) = agent_ctx {
        let json = serde_json::json!({
            "nodes_extracted": node_count,
            "edges_extracted": edge_count,
            "nodes_added": delta_nodes_added,
            "nodes_removed": delta_nodes_removed,
            "nodes_modified": delta_nodes_modified,
            "edges_added": delta_edges_added,
            "edges_removed": delta_edges_removed,
        })
        .to_string();
        (
            json,
            Some(Id::new(ctx.agent_id.clone())),
            Some(ctx.spec_ref.clone()),
        )
    } else {
        let json = serde_json::json!({
            "nodes_extracted": node_count,
            "edges_extracted": edge_count,
            "nodes_added": delta_nodes_added,
            "nodes_removed": delta_nodes_removed,
            "nodes_modified": delta_nodes_modified.len(),
            "edges_added": edges_added_count,
            "edges_removed": edges_removed_count,
        })
        .to_string();
        (json, None, None)
    };

    let delta = ArchitecturalDelta {
        id: Id::new(Uuid::new_v4().to_string()),
        repo_id: repo_id_parsed.clone(),
        commit_sha: new_sha.to_string(),
        timestamp: now,
        agent_id: delta_agent_id,
        spec_ref: delta_spec_ref,
        delta_json,
    };
    let recorded_delta = graph_store.record_delta(delta).await?;

    info!(
        %repo_id,
        %new_sha,
        nodes = node_count,
        edges = edge_count,
        "knowledge graph extraction (pass 1) complete"
    );

    // --- Step 6b: spec assertion checking (post-extraction) -------------------
    // Scan spec/markdown files in the repo tree for `<!-- gyre:assert ... -->`
    // comments, evaluate them against the fresh graph, and create inbox
    // notifications for any failures.
    {
        let spec_edges: Vec<GraphEdge> = new_edge_map.values().cloned().collect();
        if let Err(e) = check_spec_assertions_on_push(
            tmp.path(),
            &final_nodes,
            &spec_edges,
            &repo_id_parsed,
            new_sha,
            notification_scope.as_ref(),
            divergence_ports.as_ref(),
            results_repo.as_ref(),
        )
        .await
        {
            warn!(%repo_id, "spec assertion check failed (non-fatal): {e}");
        }
    }

    // --- Step 7: Non-blocking Pass 2 (call graph) ----------------------------
    // Per lsp-call-graph.md §6, the graph is usable after Pass 1 with partial
    // call data. Pass 2 delegates to language type checkers (behind the
    // CallGraphExtractor port) in a background task and merges the complete
    // Calls edges when done. The temp dir is moved into the task to stay alive.
    {
        let pass2_nodes = final_nodes.clone();
        let pass2_edges: Vec<GraphEdge> = new_edge_map.into_values().collect();
        let pass2_repo_root = tmp.path().to_path_buf();
        let pass2_repo_id = repo_id_parsed.clone();
        let pass2_graph_store = Arc::clone(&graph_store);
        let pass2_telemetry = Arc::clone(&pass2_telemetry);

        // Fire-and-forget: Pass 2 never blocks the push response. The `_tmp`
        // binding keeps the extracted tree alive until the analysis finishes.
        tokio::spawn(async move {
            let _tmp = tmp;
            let extractor = gyre_adapters::call_graph::SubprocessCallGraphExtractor::new();
            // §9: measure Pass 2 duration — the spec's performance targets
            // (go ~2–5s, rust ~20s, python/ts ~30s) are only checkable
            // against an observed number.
            let started = std::time::Instant::now();
            let outcome = extract_and_persist_call_graph(
                &pass2_repo_root,
                &pass2_nodes,
                &pass2_edges,
                &pass2_repo_id,
                pass2_graph_store.as_ref(),
                &extractor,
            )
            .await;
            let duration = started.elapsed();
            record_pass2_telemetry(pass2_telemetry.as_ref(), &pass2_repo_id, &outcome, duration)
                .await;
        });
    }

    // --- Step 8: post-extraction divergence check (HSI §8 priority 5) ---------

    if let (Some(ctx), Some(ports)) = (agent_ctx, divergence_ports) {
        if !ctx.spec_ref.is_empty() {
            let scope = DivergenceScope {
                spec_ref: &ctx.spec_ref,
                current_agent_id: &ctx.agent_id,
                workspace_id: &ctx.workspace_id,
                tenant_id: &ctx.tenant_id,
            };
            if let Err(e) = check_divergence(
                &repo_id_parsed,
                &scope,
                &recorded_delta,
                graph_store.as_ref(),
                &ports,
            )
            .await
            {
                warn!(
                    %repo_id,
                    spec_ref = %ctx.spec_ref,
                    "divergence check failed (non-fatal): {e}"
                );
            }
        }
    }

    Ok(())
}

/// Post-extraction divergence check (HSI §8 priority 5).
///
/// Compares the current agent's delta against recent deltas from other agents
/// targeting the same `spec_ref` within the last 7 days.  When the number of
/// conflicting node changes reaches or exceeds `GYRE_DIVERGENCE_THRESHOLD`
/// (default: 3), a `ConflictingInterpretations` notification is created for
/// every Admin, Developer, and Owner workspace member.
///
/// **Conflict definition** — both conditions must hold:
/// - Different `agent_id` values
/// - Same `name` but different `node_type` OR different `qualified_name` in `nodes_added`
///
/// **Exclusions:**
/// - Deltas where `spec_ref` is None (skip)
/// - Deltas where `agent_id` is None (human-pushed, skip)
/// - Agents whose ID contains "reconciliation" (intentional reconciliation, skip)
pub async fn check_divergence(
    repo_id: &Id,
    scope: &DivergenceScope<'_>,
    current_delta: &ArchitecturalDelta,
    graph_store: &dyn GraphPort,
    ports: &DivergencePorts<'_>,
) -> anyhow::Result<()> {
    let spec_ref = scope.spec_ref;
    let current_agent_id = scope.current_agent_id;
    let workspace_id = scope.workspace_id;
    let tenant_id = scope.tenant_id;
    let threshold: usize = std::env::var("GYRE_DIVERGENCE_THRESHOLD")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(3);

    let seven_days_secs: u64 = 7 * 24 * 60 * 60;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let since = now.saturating_sub(seven_days_secs);

    // Query recent deltas for this repo within the 7-day window.
    let recent_deltas = graph_store
        .list_deltas(repo_id, Some(since), Some(now))
        .await?;

    // Extract nodes_added from the current delta for comparison.
    let current_nodes = extract_nodes_added(&current_delta.delta_json);

    let mut total_conflict_count = 0usize;
    let mut conflicting_agent_id: Option<String> = None;
    let mut conflicting_commit_sha: Option<String> = None;
    let mut conflicting_node_names: Vec<String> = Vec::new();

    for other_delta in &recent_deltas {
        // Skip self.
        if other_delta.id == current_delta.id {
            continue;
        }

        // Skip deltas without spec_ref or agent_id (human-pushed or unrelated).
        let other_spec_ref = match &other_delta.spec_ref {
            Some(s) if !s.is_empty() => s.as_str(),
            _ => continue,
        };
        let other_agent_id = match &other_delta.agent_id {
            Some(a) if !a.as_str().is_empty() => a.as_str(),
            _ => continue,
        };

        // Must be the same spec_ref.
        if other_spec_ref != spec_ref {
            continue;
        }

        // Must be a different agent.
        if other_agent_id == current_agent_id {
            continue;
        }

        // Skip reconciliation agents — their divergence is intentional.
        if other_agent_id.contains("reconciliation") || current_agent_id.contains("reconciliation")
        {
            continue;
        }

        // Compare nodes_added between the two deltas.
        let other_nodes = extract_nodes_added(&other_delta.delta_json);
        let conflicts = find_node_conflicts(&current_nodes, &other_nodes);

        if !conflicts.is_empty() {
            total_conflict_count += conflicts.len();
            if conflicting_agent_id.is_none() {
                conflicting_agent_id = Some(other_agent_id.to_string());
                conflicting_commit_sha = Some(other_delta.commit_sha.clone());
            }
            for (name, _reason) in &conflicts {
                if !conflicting_node_names.contains(name) {
                    conflicting_node_names.push(name.clone());
                }
            }
        }
    }

    if total_conflict_count < threshold {
        return Ok(());
    }

    // Conflicts exceed threshold — create notifications for Admin/Developer/Owner members.
    info!(
        %repo_id,
        spec_ref,
        total_conflict_count,
        "divergence threshold exceeded — creating ConflictingInterpretations notifications"
    );

    let body = serde_json::json!({
        "spec_ref": spec_ref,
        "agent_a": current_agent_id,
        "commit_sha_a": &current_delta.commit_sha,
        "agent_b": conflicting_agent_id,
        "commit_sha_b": conflicting_commit_sha,
        "conflicting_nodes": conflicting_node_names,
        "conflict_count": total_conflict_count,
        "resolution_options": [
            {"label": "Pick A", "action": "revert_b"},
            {"label": "Pick B", "action": "revert_a"},
            {"label": "Reconcile", "action": "create_reconciliation_task"},
        ],
    })
    .to_string();

    let title = format!(
        "Conflicting spec interpretations: {} ({} conflicting nodes)",
        spec_ref,
        conflicting_node_names.len()
    );

    let ws_id = Id::new(workspace_id);
    let members = ports.membership_repo.list_by_workspace(&ws_id).await?;
    let now_i64 = now as i64;

    for member in members {
        // Notify Admin, Developer, and Owner members (per HSI §8 priority 5).
        let should_notify = matches!(
            member.role,
            WorkspaceRole::Admin | WorkspaceRole::Developer | WorkspaceRole::Owner
        );
        if !should_notify {
            continue;
        }

        let mut notif = Notification::new(
            Id::new(Uuid::new_v4().to_string()),
            ws_id.clone(),
            member.user_id.clone(),
            NotificationType::ConflictingInterpretations,
            &title,
            tenant_id,
            now_i64,
        );
        notif.body = Some(body.clone());
        notif.source.entity_type = "spec".to_string();
        notif.source.entity_id = spec_ref.to_string();
        notif.source.repo_id = Some(Id::new(repo_id.as_str().to_string()));

        if let Err(e) = ports.notification_repo.create(&notif).await {
            warn!(
                user_id = %member.user_id,
                "failed to create divergence notification: {e}"
            );
        } else if let Some(on_created) = ports.on_created.as_ref() {
            on_created(&notif).await;
        }
    }

    Ok(())
}

/// Return a stable string key for an edge type (used as HashMap key in edge diff).
fn edge_type_key(et: &EdgeType) -> &'static str {
    match et {
        EdgeType::Contains => "contains",
        EdgeType::Implements => "implements",
        EdgeType::DependsOn => "depends_on",
        EdgeType::Calls => "calls",
        EdgeType::FieldOf => "field_of",
        EdgeType::Returns => "returns",
        EdgeType::RoutesTo => "routes_to",
        EdgeType::Renders => "renders",
        EdgeType::PersistsTo => "persists_to",
        EdgeType::GovernedBy => "governed_by",
        EdgeType::ProducedBy => "produced_by",
    }
}

fn edge_entry<'a>(
    edge: &GraphEdge,
    qn_by_id: &HashMap<&'a str, &'a str>,
) -> Option<DeltaEdgeEntry> {
    let (source, target) = (
        qn_by_id.get(edge.source_id.as_str())?,
        qn_by_id.get(edge.target_id.as_str())?,
    );
    Some(DeltaEdgeEntry {
        edge_type: edge_type_key(&edge.edge_type).to_string(),
        source: (*source).to_string(),
        target: (*target).to_string(),
    })
}

/// Soft-delete stale Pass 1 edges (step 5 of `do_extract`).
///
/// F8 (task-072 R2): `Calls` edges are exempt — they are owned by Pass 2, the
/// background call-graph pass. Pass 1's syntax-only extractors emit no `Calls`
/// edges for Go, so an unconditional sweep would soft-delete every Pass 2 edge
/// on each push and race the still-running Pass 2 writer of the previous push
/// (the graph would briefly lose all call data). Pass 2 reconciles its own edge
/// type in [`extract_and_persist_call_graph`]. Tradeoff: stale Pass 1 `Calls`
/// edges from the Rust/TS syntax extractors stay until Pass 2 for those
/// languages lands (Phase 1 scope is Go).
///
/// Returns the number of edges removed. Each actually-removed edge also
/// appends a `DeltaEdgeEntry` (endpoint qualified names resolved against
/// `old_qn_by_id`, the pre-extraction node state — removed-edge endpoints are
/// soft-deleted by this pass) to `delta_edges_removed` for narrative
/// rendering (realized-model.md §6); edges whose endpoints cannot be
/// resolved are omitted from the narrative rather than given a guessed name.
async fn sweep_stale_edges(
    old_edge_map: &HashMap<(String, String, String), GraphEdge>,
    new_edge_map: &HashMap<(String, String, String), GraphEdge>,
    graph_store: &dyn GraphPort,
    old_qn_by_id: &HashMap<&str, &str>,
    delta_edges_removed: &mut Vec<DeltaEdgeEntry>,
) -> anyhow::Result<usize> {
    let mut removed = 0usize;
    for (key, edge) in old_edge_map {
        if !new_edge_map.contains_key(key) {
            if edge.edge_type == EdgeType::Calls {
                continue;
            }
            graph_store.delete_edge(&edge.id).await?;
            removed += 1;
            if let Some(e) = edge_entry(edge, old_qn_by_id) {
                delta_edges_removed.push(e);
            }
        }
    }
    Ok(removed)
}

/// Compute field-level differences between an old and new version of the same node.
///
/// Compares the mutable fields that extraction can produce — skips ID, repo_id,
/// creation metadata (preserved from old), and metrics not extracted by the
/// RustExtractor (complexity, churn, coverage).
fn diff_nodes(old: &GraphNode, new: &GraphNode) -> Vec<FieldChange> {
    let mut changes: Vec<FieldChange> = Vec::new();

    let old_nt = format!("{:?}", old.node_type).to_lowercase();
    let new_nt = format!("{:?}", new.node_type).to_lowercase();
    if old_nt != new_nt {
        changes.push(FieldChange {
            field: "node_type".to_string(),
            old_value: Some(old_nt),
            new_value: Some(new_nt),
        });
    }

    if old.name != new.name {
        changes.push(FieldChange {
            field: "name".to_string(),
            old_value: Some(old.name.clone()),
            new_value: Some(new.name.clone()),
        });
    }

    if old.file_path != new.file_path {
        changes.push(FieldChange {
            field: "file_path".to_string(),
            old_value: Some(old.file_path.clone()),
            new_value: Some(new.file_path.clone()),
        });
    }

    if old.line_start != new.line_start {
        changes.push(FieldChange {
            field: "line_start".to_string(),
            old_value: Some(old.line_start.to_string()),
            new_value: Some(new.line_start.to_string()),
        });
    }

    if old.line_end != new.line_end {
        changes.push(FieldChange {
            field: "line_end".to_string(),
            old_value: Some(old.line_end.to_string()),
            new_value: Some(new.line_end.to_string()),
        });
    }

    let old_vis = format!("{:?}", old.visibility).to_lowercase();
    let new_vis = format!("{:?}", new.visibility).to_lowercase();
    if old_vis != new_vis {
        changes.push(FieldChange {
            field: "visibility".to_string(),
            old_value: Some(old_vis),
            new_value: Some(new_vis),
        });
    }

    if old.doc_comment != new.doc_comment {
        changes.push(FieldChange {
            field: "doc_comment".to_string(),
            old_value: old.doc_comment.clone(),
            new_value: new.doc_comment.clone(),
        });
    }

    if old.spec_path != new.spec_path {
        changes.push(FieldChange {
            field: "spec_path".to_string(),
            old_value: old.spec_path.clone(),
            new_value: new.spec_path.clone(),
        });
    }

    let old_conf = format!("{:?}", old.spec_confidence).to_lowercase();
    let new_conf = format!("{:?}", new.spec_confidence).to_lowercase();
    if old_conf != new_conf {
        changes.push(FieldChange {
            field: "spec_confidence".to_string(),
            old_value: Some(old_conf),
            new_value: Some(new_conf),
        });
    }

    changes
}

/// Run all registered language extractors on `repo_root` and merge results.
///
/// Extractors are tested with `detect()` first; only matching extractors run.
/// Results from all matching extractors are merged into a single node+edge list,
/// with every node and edge's `repo_id` fixed to the real repository ID.
///
/// To add a new language extractor (S2/S3/S4), import it and push it onto the
/// `extractors` vec — no other changes required.
fn run_all_extractors(
    repo_root: &Path,
    commit_sha: &str,
    repo_id_str: &str,
) -> (Vec<GraphNode>, Vec<GraphEdge>) {
    let extractors: Vec<Box<dyn LanguageExtractor>> = vec![
        Box::new(RustExtractor),
        Box::new(GoExtractor),
        Box::new(PythonExtractor),
        Box::new(TypeScriptExtractor),
    ];

    let repo_id = Id::new(repo_id_str.to_string());
    let mut all_nodes: Vec<GraphNode> = Vec::new();
    let mut all_edges: Vec<GraphEdge> = Vec::new();

    for extractor in &extractors {
        if !extractor.detect(repo_root) {
            continue;
        }

        let result = extractor.extract(repo_root, commit_sha);

        for err in &result.errors {
            tracing::warn!(
                extractor = extractor.name(),
                file = %err.file_path,
                "extraction warning: {}",
                err.message
            );
        }

        let mut nodes = result.nodes;
        let mut edges = result.edges;

        for n in &mut nodes {
            n.repo_id = repo_id.clone();
        }
        for e in &mut edges {
            e.repo_id = repo_id.clone();
        }

        all_nodes.extend(nodes);
        all_edges.extend(edges);
    }

    (all_nodes, all_edges)
}

/// Pass 2: extract the complete call graph via language type checkers (behind
/// the [`CallGraphExtractor`] port) and persist the resulting `Calls` edges.
///
/// Runs AFTER Pass 1 results are persisted, so the graph is usable immediately
/// and becomes complete when Pass 2 finishes. Best-effort — logs and continues
/// on any failure, never failing a push.
///
/// Returns the run's [`Pass2Outcome`] (persisted-edge count, per-language
/// durations, and whether a toolchain verifiably ran) for the §9 telemetry
/// and the §8 completeness marker.
pub async fn extract_and_persist_call_graph(
    repo_root: &Path,
    nodes: &[GraphNode],
    existing_edges: &[GraphEdge],
    repo_id: &Id,
    graph_store: &dyn GraphPort,
    extractor: &dyn gyre_ports::call_graph::CallGraphExtractor,
) -> Pass2Outcome {
    use gyre_domain::call_graph_resolve::{detect_all_languages, resolve_call_edges};
    let mut outcome = Pass2Outcome::default();
    let languages = detect_all_languages(repo_root);
    if languages.is_empty() {
        return outcome;
    }

    // Accumulate resolved edges so later languages dedup against earlier ones
    // (polyglot repos) as well as the Pass 1 edges.
    let mut known_edges = existing_edges.to_vec();

    for lang in languages {
        // §9: per-language duration — the spec's performance targets are
        // per-language (go ~2–5s, rust ~20s, python ~30s, ts ~30s).
        let lang_started = std::time::Instant::now();
        let raw = match extractor.extract_call_edges(repo_root, lang).await {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(
                    repo_id = %repo_id,
                    language = %lang,
                    error = %e,
                    "Pass 2 call graph extraction failed — graph will be missing some call edges"
                );
                outcome.per_language.push((lang, lang_started.elapsed()));
                continue;
            }
        };
        outcome.per_language.push((lang, lang_started.elapsed()));
        if raw.is_empty() {
            continue;
        }
        // Non-empty raw output is positive evidence the toolchain ran (the
        // adapter degrades a missing tool to `Ok(vec![])`, which is
        // indistinguishable from a genuinely call-free repo).
        outcome.extraction_ran = true;

        let edges = resolve_call_edges(lang, &raw, nodes, &known_edges, repo_id);
        for edge in edges {
            match graph_store.create_edge(edge.clone()).await {
                Ok(_) => {
                    outcome.edges_persisted += 1;
                    known_edges.push(edge);
                }
                Err(e) => {
                    tracing::warn!(repo_id = %repo_id, error = %e, "failed to persist Pass 2 edge");
                }
            }
        }
    }

    // Reconcile Pass 2's own edge type (F8, task-072 R2). Because step 5's
    // stale sweep skips `Calls`, removal of stale call edges is Pass 2's job:
    // a live Calls edge is soft-deleted when its (source, target) pair is
    // absent from this run's resolved set (freshly persisted + carried-over
    // Pass 1 edges), or when the pair is covered but this row's id is not —
    // a legacy row from before edge ids became content-derived (it would
    // otherwise duplicate the fresh row forever). Skipped when no language
    // produced raw output: a missing/failed toolchain degrades to empty
    // output, and deleting every Calls edge because the tool was unavailable
    // would wipe the graph's call data.
    if outcome.extraction_ran {
        let mut resolved: HashMap<(String, String), HashSet<String>> = HashMap::new();
        for e in known_edges
            .iter()
            .filter(|e| e.edge_type == EdgeType::Calls)
        {
            resolved
                .entry((
                    e.source_id.as_str().to_string(),
                    e.target_id.as_str().to_string(),
                ))
                .or_default()
                .insert(e.id.as_str().to_string());
        }
        let live_calls = match graph_store.list_edges(repo_id, Some(EdgeType::Calls)).await {
            Ok(edges) => edges,
            Err(e) => {
                tracing::warn!(repo_id = %repo_id, error = %e, "Pass 2 reconcile: list_edges failed");
                Vec::new()
            }
        };
        let mut reconciled = 0usize;
        for edge in live_calls {
            let pair = (
                edge.source_id.as_str().to_string(),
                edge.target_id.as_str().to_string(),
            );
            let stale = match resolved.get(&pair) {
                None => true,
                Some(ids) => !ids.contains(edge.id.as_str()),
            };
            if stale {
                if let Err(e) = graph_store.delete_edge(&edge.id).await {
                    tracing::warn!(repo_id = %repo_id, error = %e, "failed to delete stale Calls edge");
                } else {
                    reconciled += 1;
                }
            }
        }
        if reconciled > 0 {
            tracing::info!(
                repo_id = %repo_id,
                edges = reconciled,
                "Pass 2 call graph reconcile: stale Calls edges soft-deleted"
            );
        }
    }

    if outcome.edges_persisted > 0 {
        tracing::info!(
            repo_id = %repo_id,
            edges = outcome.edges_persisted,
            "LSP call graph extraction (Pass 2) complete"
        );
    }
    outcome
}

/// Record Pass 2 telemetry and the durable completeness marker
/// (task-076, lsp-call-graph.md §8–9).
///
/// - Observes the per-language durations on the
///   `gyre_graph_pass2_duration_seconds` Prometheus histogram.
/// - Logs the total duration against the spec's per-language targets
///   (§9: go ~2–5s, rust ~20s, python ~30s, typescript ~30s) so regressions
///   are visible in server logs.
/// - Persists a [`CallGraphRunRecord`] in the KV store (namespace
///   [`CALL_GRAPH_STATUS_NAMESPACE`], key = repo id) — but ONLY when the run
///   has positive evidence a toolchain ran (`extraction_ran`). A missing or
///   failed toolchain degrades to empty output; marking the call graph
///   complete then would be a fabrication.
pub async fn record_pass2_telemetry(
    telemetry: &CallGraphTelemetry,
    repo_id: &Id,
    outcome: &Pass2Outcome,
    total_duration: std::time::Duration,
) {
    for (lang, dur) in &outcome.per_language {
        telemetry
            .metrics
            .observe_pass2_duration(&lang.to_string(), dur.as_secs_f64());
    }

    if !outcome.extraction_ran {
        // No toolchain verifiably ran — do not fabricate a completeness marker.
        tracing::warn!(
            repo_id = %repo_id,
            duration_secs = %total_duration.as_secs_f64(),
            "Pass 2 call graph extraction finished without toolchain evidence — \
             call graph remains incomplete (no marker written)"
        );
        return;
    }

    let record = CallGraphRunRecord {
        completed_at: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        duration_secs: total_duration.as_secs_f64(),
        edges_persisted: outcome.edges_persisted,
        languages: outcome.per_language.iter().map(|(l, _)| *l).collect(),
    };
    let payload = match serde_json::to_string(&record) {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!(repo_id = %repo_id, error = %e, "Pass 2: failed to serialize run record");
            return;
        }
    };
    if let Err(e) = telemetry
        .kv
        .kv_set(CALL_GRAPH_STATUS_NAMESPACE, repo_id.as_str(), payload)
        .await
    {
        tracing::warn!(
            repo_id = %repo_id,
            error = %e,
            "Pass 2: failed to persist call graph completion marker"
        );
        return;
    }
    tracing::info!(
        repo_id = %repo_id,
        duration_secs = %record.duration_secs,
        edges = record.edges_persisted,
        languages = %record
            .languages
            .iter()
            .map(|l| l.to_string())
            .collect::<Vec<_>>()
            .join(","),
        "Pass 2 call graph telemetry recorded (spec §9 targets: go ~2-5s, rust ~20s, python ~30s, ts ~30s)"
    );
}

/// Read the durable Pass 2 completion marker for a repo and decide whether
/// the call graph is complete for the current graph state
/// (lsp-call-graph.md §8). Used by the `graph_summary` and
/// `graph_query_dryrun` tools (MCP + explorer agent).
///
/// A missing marker (Pass 2 never completed), an unreadable store, or a
/// marker older than the latest Pass 1 extraction all resolve to `false` —
/// blast-radius answers must under-claim rather than over-claim.
pub async fn call_graph_complete_for_repo(
    kv: &dyn KvJsonStore,
    repo_id: &Id,
    nodes: &[GraphNode],
) -> bool {
    let completed_at = kv
        .kv_get(CALL_GRAPH_STATUS_NAMESPACE, repo_id.as_str())
        .await
        .ok()
        .flatten()
        .and_then(|payload| serde_json::from_str::<CallGraphRunRecord>(&payload).ok())
        .map(|r| r.completed_at);
    gyre_domain::view_query_resolver::is_call_graph_complete(completed_at, nodes)
}

/// Post-extraction spec assertion check.
///
/// Scans the extracted repo tree for markdown files containing
/// `<!-- gyre:assert ... -->` comments, evaluates each assertion against the
/// fresh graph data, persists the results (§9 inline view), and creates
/// priority-9 inbox notifications for failures.
///
/// Notifications are scoped by `notification_scope` (the repo's workspace),
/// NOT by agent context — every push type (agent, user CLI, mirror sync)
/// notifies. Failures already recorded for the same commit are not
/// re-notified (mirror-sync cycles re-extract unchanged SHAs).
#[allow(clippy::too_many_arguments)]
async fn check_spec_assertions_on_push(
    repo_root: &Path,
    nodes: &[GraphNode],
    edges: &[GraphEdge],
    repo_id: &Id,
    commit_sha: &str,
    notification_scope: Option<&PushNotificationScope>,
    divergence_ports: Option<&DivergencePorts<'_>>,
    results_repo: &dyn gyre_ports::SpecAssertionResultRepository,
) -> anyhow::Result<()> {
    use gyre_domain::spec_assertions;

    // Collect markdown files from specs/ directory (if it exists).
    let specs_dir = repo_root.join("specs");
    if !specs_dir.is_dir() {
        return Ok(());
    }

    let mut failed_assertions: Vec<(String, usize, String)> = Vec::new();
    // Specs whose failures were already recorded for this exact commit
    // (duplicate-suppressed — see below).
    let mut specs_already_notified: std::collections::HashSet<String> =
        std::collections::HashSet::new();

    // Walk the specs directory for markdown files.
    fn walk_md_files(dir: &Path, results: &mut Vec<std::path::PathBuf>) {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk_md_files(&path, results);
                } else if path
                    .extension()
                    .map_or(false, |ext| ext == "md" || ext == "markdown")
                {
                    results.push(path);
                }
            }
        }
    }

    let mut md_files = Vec::new();
    walk_md_files(&specs_dir, &mut md_files);

    // Spec paths present in the pushed tree (whether or not they still carry
    // assertions) — the complement of stored rows is swept below so deleted
    // or renamed specs do not keep serving stale results.
    let mut present_spec_paths: std::collections::HashSet<String> =
        std::collections::HashSet::new();

    for md_path in &md_files {
        // Canonical spec identity: path relative to `specs/` (no `specs/`
        // prefix) — the same form the spec ledger, `GET /api/v1/specs/:path`,
        // and the assertion-results endpoint use. `md_path` always starts at
        // `repo_root/specs/`, so strip both. Registered before the read so
        // an unreadable-but-present spec is never swept as deleted.
        let relative_path = md_path
            .strip_prefix(repo_root)
            .unwrap_or(md_path)
            .to_string_lossy()
            .to_string();
        let spec_path = relative_path
            .strip_prefix("specs/")
            .unwrap_or(&relative_path)
            .to_string();
        present_spec_paths.insert(spec_path.clone());

        let content = match std::fs::read_to_string(md_path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let parsed = spec_assertions::parse_assertions(&content);
        if parsed.is_empty() {
            // The spec no longer carries assertions (they were removed since
            // the last push, or the marker string only appears inside fenced
            // documentation examples). Replace semantics require the stored
            // set to reflect this: drop any prior rows for the spec.
            if let Err(e) = results_repo
                .delete_by_spec(repo_id.as_str(), &spec_path)
                .await
            {
                warn!(
                    %repo_id,
                    spec_path = %spec_path,
                    "failed to clear stale spec assertion results: {e}"
                );
            }
            continue;
        }

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        // Previously stored rows for this spec — the last push's check state.
        // Used both for duplicate suppression (same commit re-extracted by a
        // mirror-sync cycle) and to detect newly failing assertions.
        let prior_results = results_repo
            .list_by_spec(repo_id.as_str(), &spec_path)
            .await
            .unwrap_or_default();

        let records: Vec<gyre_domain::SpecAssertionResult> = parsed
            .iter()
            .zip(spec_assertions::evaluate_assertions(&parsed, nodes, edges))
            .map(|(assertion, result)| gyre_domain::SpecAssertionResult {
                id: Uuid::new_v4().to_string(),
                repo_id: repo_id.as_str().to_string(),
                spec_path: spec_path.clone(),
                line: result.line,
                assertion_type: assertion.type_name().to_string(),
                assertion_text: result.assertion_text,
                params_json: assertion.params_json().to_string(),
                passed: result.passed,
                explanation: result.explanation.clone(),
                commit_sha: commit_sha.to_string(),
                checked_at: now,
            })
            .collect();

        for record in &records {
            if !record.passed {
                failed_assertions.push((
                    spec_path.clone(),
                    record.line,
                    record.explanation.clone(),
                ));
            }
        }

        // Persist the full result set (pass and fail) so the spec's inline
        // view can show the last push's check state (system-explorer.md §9).
        if let Err(e) = results_repo.save_results(&records).await {
            warn!(
                %repo_id,
                spec_path = %spec_path,
                "failed to persist spec assertion results: {e}"
            );
        }

        // Duplicate suppression: if the stored set for this spec was already
        // produced by this exact commit (mirror-sync cycles re-extract
        // unchanged SHAs every interval), do not notify again.
        let already_recorded_for_commit =
            !prior_results.is_empty() && prior_results.iter().all(|r| r.commit_sha == commit_sha);
        if already_recorded_for_commit {
            specs_already_notified.insert(spec_path);
        }
    }

    // Replace semantics for specs that no longer exist in the pushed tree:
    // a deleted or renamed spec file leaves stored rows that the GET
    // endpoint would keep serving as the spec's "latest check". Sweep them.
    if let Ok(stored_paths) = results_repo.list_spec_paths(repo_id.as_str()).await {
        for stale in stored_paths {
            if present_spec_paths.contains(&stale) {
                continue;
            }
            if let Err(e) = results_repo.delete_by_spec(repo_id.as_str(), &stale).await {
                warn!(
                    %repo_id,
                    spec_path = %stale,
                    "failed to sweep stale spec assertion results: {e}"
                );
            }
        }
    }

    // Only failures from specs whose failing state is new for this commit
    // warrant a notification; drop suppressed ones.
    failed_assertions.retain(|(path, _, _)| !specs_already_notified.contains(path));

    if failed_assertions.is_empty() {
        return Ok(());
    }

    info!(
        %repo_id,
        failures = failed_assertions.len(),
        "spec assertion failures detected on push"
    );

    // Create priority-9 notifications for the failures (§9 Inbox items).
    // Scoped by the repo's workspace — fires for every push type, not just
    // agent pushes. Skipped when the scope or notification ports are
    // unavailable (repo workspace unresolvable, or a caller without ports).
    if let (Some(scope), Some(ports)) = (notification_scope, divergence_ports) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        // The first failing spec — linked as the notification's entity_ref so
        // the Inbox "Update Spec" action can open it.
        let first_spec_path = failed_assertions[0].0.clone();

        let body = serde_json::json!({
            "repo_id": repo_id.as_str(),
            "spec_path": first_spec_path,
            "commit_sha": commit_sha,
            "failures": failed_assertions.iter().map(|(path, line, explanation)| {
                serde_json::json!({
                    "spec_path": path,
                    "line": line,
                    "explanation": explanation,
                })
            }).collect::<Vec<_>>(),
        })
        .to_string();

        let title = format!(
            "Spec assertion failures: {} assertion(s) failed",
            failed_assertions.len()
        );

        let ws_id = Id::new(&scope.workspace_id);
        let members = ports.membership_repo.list_by_workspace(&ws_id).await?;

        for member in members {
            let should_notify = matches!(
                member.role,
                WorkspaceRole::Admin | WorkspaceRole::Developer | WorkspaceRole::Owner
            );
            if !should_notify {
                continue;
            }

            let mut notif = Notification::new(
                Id::new(Uuid::new_v4().to_string()),
                ws_id.clone(),
                member.user_id.clone(),
                NotificationType::SpecAssertionFailure,
                &title,
                &scope.tenant_id,
                now,
            );
            notif.body = Some(body.clone());
            notif.source.entity_type = "spec".to_string();
            notif.source.repo_id = Some(Id::new(repo_id.as_str().to_string()));
            // Link to the failing spec so Inbox actions can open it.
            notif.source.entity_id = first_spec_path.clone();

            if let Err(e) = ports.notification_repo.create(&notif).await {
                warn!(
                    user_id = %member.user_id,
                    "failed to create spec assertion notification: {e}"
                );
            } else if let Some(on_created) = ports.on_created.as_ref() {
                on_created(&notif).await;
            }
        }
    }
    Ok(())
}

/// Parse `nodes_added` from a delta_json string.
///
/// Returns an empty vec if the field is absent or malformed (backward-compatible
/// with the compact format that only stores counts).
fn extract_nodes_added(delta_json: &str) -> Vec<DeltaNodeEntry> {
    let Ok(val) = serde_json::from_str::<serde_json::Value>(delta_json) else {
        return vec![];
    };
    let Some(arr) = val.get("nodes_added").and_then(|v| v.as_array()) else {
        return vec![];
    };
    arr.iter()
        .filter_map(|v| serde_json::from_value::<DeltaNodeEntry>(v.clone()).ok())
        .collect()
}

/// Find conflicting nodes between two `nodes_added` lists.
///
/// A conflict exists when both lists contain a node with the same `name` but
/// different `node_type` OR different `qualified_name`.
///
/// Returns pairs of `(node_name, conflict_reason)`.
fn find_node_conflicts(a: &[DeltaNodeEntry], b: &[DeltaNodeEntry]) -> Vec<(String, &'static str)> {
    let mut conflicts = Vec::new();
    for node_a in a {
        for node_b in b {
            if node_a.name != node_b.name {
                continue;
            }
            if node_a.node_type != node_b.node_type {
                conflicts.push((node_a.name.clone(), "node_type_mismatch"));
            } else if node_a.qualified_name != node_b.qualified_name {
                conflicts.push((node_a.name.clone(), "qualified_name_mismatch"));
            }
        }
    }
    conflicts
}

#[cfg(test)]
mod tests {
    use super::*;
    use gyre_common::graph::DeltaNodeEntry;

    fn make_node(name: &str, node_type: &str, qname: &str) -> DeltaNodeEntry {
        DeltaNodeEntry {
            name: name.to_string(),
            node_type: node_type.to_string(),
            qualified_name: qname.to_string(),
        }
    }

    #[test]
    fn edge_entry_resolves_qualified_names() {
        let qn_by_id: HashMap<&str, &str> = [("n-1", "crate::Foo"), ("n-2", "crate::Bar")]
            .into_iter()
            .collect();
        let e = GraphEdge {
            id: Id::new("e-1"),
            repo_id: Id::new("repo-1"),
            source_id: Id::new("n-1"),
            target_id: Id::new("n-2"),
            edge_type: EdgeType::Implements,
            metadata: None,
            first_seen_at: 100,
            last_seen_at: 100,
            deleted_at: None,
        };
        let entry = edge_entry(&e, &qn_by_id).expect("both endpoints known");
        assert_eq!(entry.edge_type, "implements");
        assert_eq!(entry.source, "crate::Foo");
        assert_eq!(entry.target, "crate::Bar");
    }

    #[test]
    fn edge_entry_skips_unknown_endpoints() {
        // Grounding contract: unknown endpoints are omitted, never guessed.
        let qn_by_id: HashMap<&str, &str> = [("n-1", "crate::Foo")].into_iter().collect();
        let e = GraphEdge {
            id: Id::new("e-1"),
            repo_id: Id::new("repo-1"),
            source_id: Id::new("n-1"),
            target_id: Id::new("n-ghost"),
            edge_type: EdgeType::Calls,
            metadata: None,
            first_seen_at: 100,
            last_seen_at: 100,
            deleted_at: None,
        };
        assert!(edge_entry(&e, &qn_by_id).is_none());
    }

    #[test]
    fn no_conflict_when_names_differ() {
        let a = vec![make_node("Foo", "type", "crate::Foo")];
        let b = vec![make_node("Bar", "type", "crate::Bar")];
        assert!(find_node_conflicts(&a, &b).is_empty());
    }

    #[test]
    fn no_conflict_when_identical() {
        let a = vec![make_node("Foo", "type", "crate::Foo")];
        let b = vec![make_node("Foo", "type", "crate::Foo")];
        assert!(find_node_conflicts(&a, &b).is_empty());
    }

    #[test]
    fn conflict_on_node_type_mismatch() {
        let a = vec![make_node("Handler", "type", "crate::Handler")];
        let b = vec![make_node("Handler", "interface", "crate::Handler")];
        let conflicts = find_node_conflicts(&a, &b);
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].0, "Handler");
        assert_eq!(conflicts[0].1, "node_type_mismatch");
    }

    #[test]
    fn conflict_on_qualified_name_mismatch() {
        let a = vec![make_node("Handler", "type", "crate::auth::Handler")];
        let b = vec![make_node("Handler", "type", "crate::core::Handler")];
        let conflicts = find_node_conflicts(&a, &b);
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].1, "qualified_name_mismatch");
    }

    #[test]
    fn multiple_conflicts_detected() {
        let a = vec![
            make_node("Alpha", "type", "crate::Alpha"),
            make_node("Beta", "function", "crate::beta"),
        ];
        let b = vec![
            make_node("Alpha", "interface", "crate::Alpha"),
            make_node("Beta", "function", "crate::other::beta"),
        ];
        let conflicts = find_node_conflicts(&a, &b);
        assert_eq!(conflicts.len(), 2);
    }

    #[test]
    fn extract_nodes_added_from_rich_json() {
        let json = serde_json::json!({
            "nodes_extracted": 2,
            "edges_extracted": 0,
            "nodes_added": [
                {"name": "Foo", "node_type": "type", "qualified_name": "crate::Foo"},
                {"name": "bar", "node_type": "function", "qualified_name": "crate::bar"},
            ],
            "nodes_modified": [],
        })
        .to_string();

        let nodes = extract_nodes_added(&json);
        assert_eq!(nodes.len(), 2);
        assert_eq!(nodes[0].name, "Foo");
        assert_eq!(nodes[1].node_type, "function");
    }

    #[test]
    fn extract_nodes_added_from_compact_json_returns_empty() {
        // Compact format (no agent context) — backward-compatible.
        let json = r#"{"nodes_extracted": 5, "edges_extracted": 3}"#;
        assert!(extract_nodes_added(json).is_empty());
    }

    #[test]
    fn extract_nodes_added_from_malformed_json_returns_empty() {
        assert!(extract_nodes_added("not json at all").is_empty());
    }

    // ── Incremental diff helpers ───────────────────────────────────────────────

    use gyre_common::{
        graph::{GraphNode, NodeType, SpecConfidence, Visibility},
        Id,
    };

    fn make_graph_node(id: &str, qname: &str) -> GraphNode {
        GraphNode {
            id: Id::new(id),
            repo_id: Id::new("repo1"),
            node_type: NodeType::Function,
            name: id.to_string(),
            qualified_name: qname.to_string(),
            file_path: "src/lib.rs".to_string(),
            line_start: 10,
            line_end: 20,
            visibility: Visibility::Public,
            doc_comment: None,
            spec_path: None,
            spec_paths: vec![],
            spec_confidence: SpecConfidence::None,
            last_modified_sha: "sha1".to_string(),
            last_modified_by: None,
            last_modified_at: 1000,
            created_sha: "sha1".to_string(),
            created_at: 1000,
            complexity: None,
            churn_count_30d: 0,
            test_coverage: None,
            first_seen_at: 1000,
            last_seen_at: 1000,
            deleted_at: None,
            test_node: false,
            spec_approved_at: None,
            milestone_completed_at: None,
        }
    }

    #[test]
    fn diff_nodes_no_changes_returns_empty() {
        let node = make_graph_node("foo", "crate::foo");
        let changes = diff_nodes(&node, &node);
        assert!(
            changes.is_empty(),
            "identical nodes should produce no diffs"
        );
    }

    #[test]
    fn diff_nodes_file_path_change_detected() {
        let old = make_graph_node("foo", "crate::foo");
        let mut new = make_graph_node("foo", "crate::foo");
        new.file_path = "src/other.rs".to_string();

        let changes = diff_nodes(&old, &new);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].field, "file_path");
        assert_eq!(changes[0].old_value.as_deref(), Some("src/lib.rs"));
        assert_eq!(changes[0].new_value.as_deref(), Some("src/other.rs"));
    }

    #[test]
    fn diff_nodes_line_range_change_detected() {
        let old = make_graph_node("bar", "crate::bar");
        let mut new = make_graph_node("bar", "crate::bar");
        new.line_start = 50;
        new.line_end = 80;

        let changes = diff_nodes(&old, &new);
        let fields: Vec<&str> = changes.iter().map(|c| c.field.as_str()).collect();
        assert!(fields.contains(&"line_start"), "expected line_start diff");
        assert!(fields.contains(&"line_end"), "expected line_end diff");
    }

    #[test]
    fn diff_nodes_node_type_change_detected() {
        let old = make_graph_node("MyTrait", "crate::MyTrait");
        let mut new = make_graph_node("MyTrait", "crate::MyTrait");
        new.node_type = NodeType::Interface;

        let changes = diff_nodes(&old, &new);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].field, "node_type");
        assert_eq!(changes[0].old_value.as_deref(), Some("function"));
        assert_eq!(changes[0].new_value.as_deref(), Some("interface"));
    }

    #[test]
    fn diff_nodes_doc_comment_added() {
        let old = make_graph_node("baz", "crate::baz");
        let mut new = make_graph_node("baz", "crate::baz");
        new.doc_comment = Some("Now documented.".to_string());

        let changes = diff_nodes(&old, &new);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].field, "doc_comment");
        assert!(changes[0].old_value.is_none());
        assert_eq!(changes[0].new_value.as_deref(), Some("Now documented."));
    }

    #[test]
    fn diff_nodes_multiple_fields_changed() {
        let old = make_graph_node("qux", "crate::qux");
        let mut new = make_graph_node("qux", "crate::qux");
        new.file_path = "src/new.rs".to_string();
        new.line_start = 1;
        new.doc_comment = Some("doc".to_string());

        let changes = diff_nodes(&old, &new);
        assert_eq!(changes.len(), 3);
    }

    // ── Pass 2 sync → graph storage integration (lsp-call-graph.md §6) ──────────

    use gyre_adapters::mem_graph::MemGraphStore;
    use gyre_common::call_graph::{CallEdge, Language};
    use gyre_common::graph::{EdgeType, GraphEdge};

    /// Fake [`CallGraphExtractor`] standing in for the subprocess adapter, so the
    /// resolve→persist pipeline can be exercised hermetically (no Go toolchain).
    struct FakeCallGraphExtractor {
        edges: Vec<CallEdge>,
    }

    #[async_trait::async_trait]
    impl gyre_ports::call_graph::CallGraphExtractor for FakeCallGraphExtractor {
        async fn extract_call_edges(
            &self,
            _repo_path: &Path,
            _language: Language,
        ) -> anyhow::Result<Vec<CallEdge>> {
            Ok(self.edges.clone())
        }
    }

    fn make_go_fn(qname: &str, file: &str, repo_id: &Id) -> GraphNode {
        let mut n = make_graph_node(qname, qname);
        n.file_path = file.to_string();
        n.repo_id = repo_id.clone();
        n
    }

    #[tokio::test]
    async fn sync_go_repo_persists_calls_edges_in_graph_store() {
        // A repo with a go.mod → detect_all_languages reports Go.
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(
            dir.path().join("go.mod"),
            "module example.com/app\n\ngo 1.21\n",
        )
        .unwrap();

        let repo_id = Id::new("repo-go");
        let caller = make_go_fn("example.com/app/api.Handler", "api/handler.go", &repo_id);
        let callee = make_go_fn("example.com/app/svc.DoWork", "svc/work.go", &repo_id);
        let nodes = vec![caller.clone(), callee.clone()];

        // Graph is usable after Pass 1: nodes persisted before Pass 2 runs.
        let store = MemGraphStore::new();
        for n in &nodes {
            store.create_node(n.clone()).await.unwrap();
        }

        // Pass 2 reports a cross-package call that Pass 1 syntax analysis missed.
        let fake = FakeCallGraphExtractor {
            edges: vec![CallEdge {
                from: "example.com/app/api.Handler".to_string(),
                to: "example.com/app/svc.DoWork".to_string(),
            }],
        };

        let outcome =
            extract_and_persist_call_graph(dir.path(), &nodes, &[], &repo_id, &store, &fake).await;
        assert_eq!(
            outcome.edges_persisted, 1,
            "one cross-package Calls edge should be persisted"
        );

        // The Calls edge must actually appear in the graph store, resolved to the
        // Pass 1 node IDs.
        let calls = store
            .list_edges(&repo_id, Some(EdgeType::Calls))
            .await
            .unwrap();
        assert_eq!(calls.len(), 1, "Calls edge must appear in the graph store");
        assert_eq!(calls[0].source_id, caller.id);
        assert_eq!(calls[0].target_id, callee.id);
        assert_eq!(calls[0].edge_type, EdgeType::Calls);
        // §7 (task-076): Pass 2 emits the SAME EdgeType::Calls variant as
        // Pass 1 — provenance is carried in the metadata tag so
        // `graph_summary` can report per-pass counts.
        assert_eq!(
            calls[0].metadata.as_deref(),
            Some(gyre_common::call_graph::PASS2_EDGE_METADATA),
            "Pass 2 Calls edge must carry the provenance metadata tag"
        );
        // §8 (task-076): a run with toolchain evidence must leave the call
        // graph readable as complete.
        assert!(outcome.extraction_ran);
        assert_eq!(outcome.per_language.len(), 1);
        assert_eq!(outcome.per_language[0].0, Language::Go);
    }

    /// task-076 (lsp-call-graph.md §8–9): the durable Pass 2 completion marker
    /// and the §9 duration metric are written only on positive toolchain
    /// evidence, and the marker drives `call_graph_complete_for_repo`.
    #[tokio::test]
    async fn pass2_telemetry_marker_gated_on_toolchain_evidence() {
        use crate::mem::MemKvStore;
        use crate::metrics::Metrics;

        let kv = Arc::new(MemKvStore::default());
        let metrics = Arc::new(Metrics::new().unwrap());
        let telemetry = CallGraphTelemetry {
            kv: Arc::clone(&kv) as Arc<dyn KvJsonStore>,
            metrics: Arc::clone(&metrics),
        };
        let repo_id = Id::new("repo-t76");

        // No toolchain evidence (adapter degraded to empty output) → no
        // marker, no histogram observation. Marking the graph complete here
        // would fabricate completeness.
        let no_evidence = Pass2Outcome {
            extraction_ran: false,
            edges_persisted: 0,
            per_language: vec![],
        };
        record_pass2_telemetry(
            &telemetry,
            &repo_id,
            &no_evidence,
            std::time::Duration::from_secs(1),
        )
        .await;
        assert!(
            kv.kv_get(CALL_GRAPH_STATUS_NAMESPACE, repo_id.as_str())
                .await
                .unwrap()
                .is_none(),
            "no toolchain evidence → no completion marker"
        );
        assert!(!metrics
            .render()
            .contains("gyre_graph_pass2_duration_seconds"));

        // Toolchain ran → marker + duration metric recorded.
        let with_evidence = Pass2Outcome {
            extraction_ran: true,
            edges_persisted: 3,
            per_language: vec![(Language::Go, std::time::Duration::from_millis(2500))],
        };
        record_pass2_telemetry(
            &telemetry,
            &repo_id,
            &with_evidence,
            std::time::Duration::from_millis(2600),
        )
        .await;
        let marker = kv
            .kv_get(CALL_GRAPH_STATUS_NAMESPACE, repo_id.as_str())
            .await
            .unwrap()
            .expect("toolchain evidence → completion marker");
        let record: CallGraphRunRecord = serde_json::from_str(&marker).unwrap();
        assert_eq!(record.edges_persisted, 3);
        assert_eq!(record.languages, vec![Language::Go]);
        assert!(record.duration_secs > 0.0);
        let rendered = metrics.render();
        assert!(
            rendered.contains("gyre_graph_pass2_duration_seconds"),
            "Pass 2 duration must be exposed as a metric, got:\n{rendered}"
        );
        assert!(rendered.contains("language=\"go\""));

        // Fresh marker + nodes persisted at/before completion → complete.
        let mut node = make_graph_node("fn_a", "crate::fn_a");
        node.repo_id = repo_id.clone();
        node.last_seen_at = record.completed_at;
        assert!(call_graph_complete_for_repo(kv.as_ref(), &repo_id, &[node.clone()]).await);

        // A newer Pass 1 extraction (last_seen_at after the marker) → the
        // graph is call-incomplete until Pass 2 re-runs.
        node.last_seen_at = record.completed_at + 1;
        assert!(!call_graph_complete_for_repo(kv.as_ref(), &repo_id, &[node]).await);

        // Stale marker (completed before the latest extraction) → incomplete.
        // (Covered above; also check a repo with no marker at all.)
        assert!(!call_graph_complete_for_repo(kv.as_ref(), &Id::new("repo-none"), &[]).await);
    }

    #[tokio::test]
    async fn sync_rust_repo_persists_calls_edges_in_graph_store() {
        // A repo with a Cargo.toml -> detect_all_languages reports Rust, and
        // the Rust Pass 2 edge format (module-qualified names, see
        // gyre-adapters::call_graph_rust) resolves against Pass 1 nodes and
        // persists in the graph store. The qualified names use the naming the
        // real rust-analyzer adapter emits for src/<module>.rs files.
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"fx\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();

        let repo_id = Id::new("repo-rust");
        let caller = make_go_fn("beta::beta_caller", "src/beta.rs", &repo_id);
        let callee = make_go_fn("alpha::alpha_target", "src/alpha.rs", &repo_id);
        let nodes = vec![caller.clone(), callee.clone()];

        let store = MemGraphStore::new();
        for n in &nodes {
            store.create_node(n.clone()).await.unwrap();
        }

        let fake = FakeCallGraphExtractor {
            edges: vec![
                // Resolvable pub-fn-to-pub-fn edge (the kind the real
                // rust-analyzer adapter emits for a cross-module call).
                CallEdge {
                    from: "beta::beta_caller".to_string(),
                    to: "alpha::alpha_target".to_string(),
                },
                // Caller with no Pass 1 node (private fn / impl method):
                // must be dropped, not mis-linked.
                CallEdge {
                    from: "beta::spin".to_string(),
                    to: "alpha::alpha_target".to_string(),
                },
            ],
        };

        let outcome =
            extract_and_persist_call_graph(dir.path(), &nodes, &[], &repo_id, &store, &fake).await;
        assert_eq!(
            outcome.edges_persisted, 1,
            "only the pub-to-pub edge should persist"
        );

        let calls = store
            .list_edges(&repo_id, Some(EdgeType::Calls))
            .await
            .unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].source_id, caller.id);
        assert_eq!(calls[0].target_id, callee.id);
        assert_eq!(calls[0].edge_type, EdgeType::Calls);
        // §7 (task-076): the Rust leg emits the same EdgeType::Calls variant
        // with the same provenance metadata tag as the Go/TS legs — Pass 2
        // edge type consistency holds per language.
        assert_eq!(
            calls[0].metadata.as_deref(),
            Some(gyre_common::call_graph::PASS2_EDGE_METADATA),
            "Pass 2 Calls edge must carry the provenance metadata tag"
        );
    }
    #[tokio::test]
    async fn sync_ts_repo_persists_calls_edges_in_graph_store() {
        // Same pipeline as the Go test above, for the TypeScript leg
        // (task-075): a repo with TS markers → detect_all_languages reports
        // TypeScript → Pass 2 edges resolve against Pass 1 nodes. The raw
        // edge qnames here are exactly the format the LSP driver
        // (scripts/ts-callgraph.mjs) emits and typescript_extractor.rs
        // indexes: "<repo-relative path without extension>.<Name>" — the
        // driver test in gyre-adapters asserts that same format against a
        // recorded real-server run, so this pins the two halves together.
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join("package.json"), "{\"name\":\"web\"}").unwrap();
        std::fs::write(dir.path().join("tsconfig.json"), "{}").unwrap();

        let repo_id = Id::new("repo-ts");
        let caller = make_go_fn("src/app.compute", "src/app.ts", &repo_id);
        let callee = make_go_fn("src/math.add", "src/math.ts", &repo_id);
        let nodes = vec![caller.clone(), callee.clone()];

        let store = MemGraphStore::new();
        for n in &nodes {
            store.create_node(n.clone()).await.unwrap();
        }

        // The cross-module call the LSP references pass reports for
        // `compute` calling `add` through the "./math" import.
        let fake = FakeCallGraphExtractor {
            edges: vec![CallEdge {
                from: "src/app.compute".to_string(),
                to: "src/math.add".to_string(),
            }],
        };

        let outcome =
            extract_and_persist_call_graph(dir.path(), &nodes, &[], &repo_id, &store, &fake).await;
        assert_eq!(
            outcome.edges_persisted, 1,
            "one cross-module Calls edge should be persisted"
        );

        let calls = store
            .list_edges(&repo_id, Some(EdgeType::Calls))
            .await
            .unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].source_id, caller.id);
        assert_eq!(calls[0].target_id, callee.id);
        assert_eq!(calls[0].edge_type, EdgeType::Calls);
        // §7 (task-076): the TS leg emits the same EdgeType::Calls variant
        // with the same provenance metadata tag as the Go leg — Pass 2 edge
        // type consistency holds per language.
        assert_eq!(
            calls[0].metadata.as_deref(),
            Some(gyre_common::call_graph::PASS2_EDGE_METADATA),
            "Pass 2 Calls edge must carry the provenance metadata tag"
        );
    }

    #[tokio::test]
    async fn push_check_persists_assertion_results_via_repo() {
        use crate::mem::MemSpecAssertionResultRepository;
        use gyre_common::graph::{SpecConfidence, Visibility};
        use gyre_ports::SpecAssertionResultRepository as _;

        // A repo snapshot with a spec carrying two assertions: one that passes
        // against the graph below, one that fails.
        let dir = tempfile::TempDir::new().unwrap();
        let specs_dir = dir.path().join("specs").join("system");
        std::fs::create_dir_all(&specs_dir).unwrap();
        std::fs::write(
            specs_dir.join("architecture.md"),
            "# Architecture\n\n<!-- gyre:assert type=\"no_dependency\" from=\"gyre-domain\" to=\"gyre-adapters\" -->\n\n<!-- gyre:assert type=\"implements\" subject=\"SearchService\" trait=\"FullTextPort\" -->\n",
        )
        .unwrap();

        let repo_id = Id::new("repo-assert");
        let mk_node = |id: &str, name: &str| GraphNode {
            id: Id::new(id),
            repo_id: repo_id.clone(),
            node_type: NodeType::Module,
            name: name.to_string(),
            qualified_name: name.to_string(),
            file_path: format!("crates/{name}/src/lib.rs"),
            line_start: 1,
            line_end: 10,
            visibility: Visibility::Public,
            doc_comment: None,
            spec_path: None,
            spec_paths: vec![],
            spec_confidence: SpecConfidence::None,
            last_modified_sha: "abc".to_string(),
            last_modified_by: None,
            last_modified_at: 0,
            created_sha: "abc".to_string(),
            created_at: 0,
            complexity: None,
            churn_count_30d: 0,
            test_coverage: None,
            first_seen_at: 0,
            last_seen_at: 0,
            deleted_at: None,
            test_node: false,
            spec_approved_at: None,
            milestone_completed_at: None,
        };
        let nodes = vec![mk_node("nd", "gyre-domain"), mk_node("na", "gyre-adapters")];

        let results_repo = MemSpecAssertionResultRepository::default();
        check_spec_assertions_on_push(
            dir.path(),
            &nodes,
            &[],
            &repo_id,
            "deadbeef",
            None,
            None,
            &results_repo,
        )
        .await
        .unwrap();

        // Both assertions (pass and fail) are persisted under the canonical
        // spec path (no `specs/` prefix), ordered by line.
        let stored = results_repo
            .list_by_spec(repo_id.as_str(), "system/architecture.md")
            .await
            .unwrap();
        assert_eq!(stored.len(), 2, "both assertions must be persisted");
        assert_eq!(stored[0].line, 3);
        assert_eq!(stored[0].assertion_type, "no_dependency");
        assert!(
            stored[0].passed,
            "no edge from gyre-domain to gyre-adapters exists"
        );
        assert_eq!(stored[0].commit_sha, "deadbeef");
        assert_eq!(stored[1].line, 5);
        assert_eq!(stored[1].assertion_type, "implements");
        assert!(
            !stored[1].passed,
            "SearchService node is absent from the graph"
        );

        // A second push replaces the stored set for the same spec (no stale
        // rows from the first check linger).
        std::fs::write(
            specs_dir.join("architecture.md"),
            "# Architecture\n\n<!-- gyre:assert type=\"all_have\" node_type=\"Endpoint\" property=\"auth_middleware\" -->\n",
        )
        .unwrap();
        check_spec_assertions_on_push(
            dir.path(),
            &nodes,
            &[],
            &repo_id,
            "deadbeef2",
            None,
            None,
            &results_repo,
        )
        .await
        .unwrap();

        let replaced = results_repo
            .list_by_spec(repo_id.as_str(), "system/architecture.md")
            .await
            .unwrap();
        assert_eq!(
            replaced.len(),
            1,
            "re-push must replace prior rows for the spec"
        );
        assert_eq!(replaced[0].assertion_type, "all_have");
        assert_eq!(replaced[0].commit_sha, "deadbeef2");
    }

    #[tokio::test]
    async fn push_check_sweeps_stale_results_when_assertions_or_specs_vanish() {
        use crate::mem::MemSpecAssertionResultRepository;
        use gyre_common::graph::{SpecConfidence, Visibility};
        use gyre_ports::SpecAssertionResultRepository as _;

        let dir = tempfile::TempDir::new().unwrap();
        let specs_dir = dir.path().join("specs").join("system");
        std::fs::create_dir_all(&specs_dir).unwrap();
        std::fs::write(
            specs_dir.join("kept.md"),
            "<!-- gyre:assert type=\"no_dependency\" from=\"gyre-domain\" to=\"gyre-adapters\" -->\n",
        )
        .unwrap();
        std::fs::write(
            specs_dir.join("emptied.md"),
            "<!-- gyre:assert type=\"no_dependency\" from=\"gyre-domain\" to=\"gyre-adapters\" -->\n",
        )
        .unwrap();
        std::fs::write(
            specs_dir.join("doomed.md"),
            "<!-- gyre:assert type=\"no_dependency\" from=\"gyre-domain\" to=\"gyre-adapters\" -->\n",
        )
        .unwrap();

        let repo_id = Id::new("repo-sweep");
        let mk_node = |id: &str, name: &str| GraphNode {
            id: Id::new(id),
            repo_id: repo_id.clone(),
            node_type: NodeType::Module,
            name: name.to_string(),
            qualified_name: name.to_string(),
            file_path: format!("crates/{name}/src/lib.rs"),
            line_start: 1,
            line_end: 10,
            visibility: Visibility::Public,
            doc_comment: None,
            spec_path: None,
            spec_paths: vec![],
            spec_confidence: SpecConfidence::None,
            last_modified_sha: "abc".to_string(),
            last_modified_by: None,
            last_modified_at: 0,
            created_sha: "abc".to_string(),
            created_at: 0,
            complexity: None,
            churn_count_30d: 0,
            test_coverage: None,
            first_seen_at: 0,
            last_seen_at: 0,
            deleted_at: None,
            test_node: false,
            spec_approved_at: None,
            milestone_completed_at: None,
        };
        let nodes = vec![mk_node("nd", "gyre-domain"), mk_node("na", "gyre-adapters")];

        let results_repo = MemSpecAssertionResultRepository::default();
        check_spec_assertions_on_push(
            dir.path(),
            &nodes,
            &[],
            &repo_id,
            "sha1",
            None,
            None,
            &results_repo,
        )
        .await
        .unwrap();

        for path in ["system/kept.md", "system/emptied.md", "system/doomed.md"] {
            let stored = results_repo
                .list_by_spec(repo_id.as_str(), path)
                .await
                .unwrap();
            assert_eq!(stored.len(), 1, "{path} seeded with one result");
        }

        // Next push: `emptied.md` keeps its file but loses its assertions;
        // `doomed.md` is deleted entirely; `kept.md` is unchanged.
        std::fs::write(
            specs_dir.join("emptied.md"),
            "# Emptied\n\nNo assertions remain.\n",
        )
        .unwrap();
        std::fs::remove_file(specs_dir.join("doomed.md")).unwrap();

        check_spec_assertions_on_push(
            dir.path(),
            &nodes,
            &[],
            &repo_id,
            "sha2",
            None,
            None,
            &results_repo,
        )
        .await
        .unwrap();

        let kept = results_repo
            .list_by_spec(repo_id.as_str(), "system/kept.md")
            .await
            .unwrap();
        assert_eq!(kept.len(), 1, "unchanged spec keeps its results");
        assert_eq!(kept[0].commit_sha, "sha2", "unchanged spec is re-checked");

        let emptied = results_repo
            .list_by_spec(repo_id.as_str(), "system/emptied.md")
            .await
            .unwrap();
        assert!(
            emptied.is_empty(),
            "spec whose assertions were all removed must not keep stale rows"
        );

        let doomed = results_repo
            .list_by_spec(repo_id.as_str(), "system/doomed.md")
            .await
            .unwrap();
        assert!(
            doomed.is_empty(),
            "deleted spec file must not keep stale rows"
        );
    }

    #[tokio::test]
    async fn push_check_creates_priority9_notifications_for_failed_assertions() {
        use crate::mem::{
            MemNotificationRepository, MemSpecAssertionResultRepository,
            MemWorkspaceMembershipRepository,
        };
        use gyre_common::NotificationType;
        use gyre_domain::{WorkspaceMembership, WorkspaceRole};

        // Spec with one failing assertion (subject node absent from graph).
        let dir = tempfile::TempDir::new().unwrap();
        let specs_dir = dir.path().join("specs").join("system");
        std::fs::create_dir_all(&specs_dir).unwrap();
        std::fs::write(
            specs_dir.join("architecture.md"),
            "# Architecture\n\n<!-- gyre:assert type=\"implements\" subject=\"SearchService\" trait=\"FullTextPort\" -->\n",
        )
        .unwrap();

        let repo_id = Id::new("repo-assert-notif");
        let nodes: Vec<GraphNode> = vec![];
        let results_repo = MemSpecAssertionResultRepository::default();
        let notif_repo = MemNotificationRepository::default();
        let membership_repo = MemWorkspaceMembershipRepository::default();

        // One Developer and one Viewer in the workspace; only the Developer
        // is within the notify set (Admin/Developer/Owner).
        membership_repo
            .create(&WorkspaceMembership::new(
                Id::new("m-dev"),
                Id::new("user-dev"),
                Id::new("ws-1"),
                WorkspaceRole::Developer,
                Id::new("inviter"),
                1_000,
            ))
            .await
            .unwrap();
        membership_repo
            .create(&WorkspaceMembership::new(
                Id::new("m-view"),
                Id::new("user-view"),
                Id::new("ws-1"),
                WorkspaceRole::Viewer,
                Id::new("inviter"),
                1_000,
            ))
            .await
            .unwrap();

        let scope = PushNotificationScope {
            workspace_id: "ws-1".to_string(),
            tenant_id: "tenant-1".to_string(),
        };
        let ports = DivergencePorts {
            notification_repo: &notif_repo,
            membership_repo: &membership_repo,
            on_created: None,
        };

        check_spec_assertions_on_push(
            dir.path(),
            &nodes,
            &[],
            &repo_id,
            "deadbeef",
            Some(&scope),
            Some(&ports),
            &results_repo,
        )
        .await
        .unwrap();

        // The Developer gets exactly one priority-9 SpecAssertionFailure
        // notification carrying the failing spec and assertion.
        let dev_notifs = notif_repo
            .list_for_user(
                &Id::new("user-dev"),
                Some(&Id::new("ws-1")),
                None,
                None,
                None,
                None,
                50,
                0,
            )
            .await
            .unwrap();
        assert_eq!(dev_notifs.len(), 1, "developer must be notified");
        let n = &dev_notifs[0];
        assert_eq!(n.notification_type, NotificationType::SpecAssertionFailure);
        assert_eq!(n.priority_rank, 9);
        assert_eq!(
            n.source.repo_id.as_ref().map(|r| r.as_str()),
            Some("repo-assert-notif")
        );
        let body: serde_json::Value = serde_json::from_str(n.body.as_deref().unwrap()).unwrap();
        assert_eq!(body["failures"][0]["spec_path"], "system/architecture.md");
        assert_eq!(body["repo_id"], "repo-assert-notif");
        // §9: the notification links to the failing spec so the Inbox
        // "Update Spec" action can open it.
        assert_eq!(n.source.entity_id.as_str(), "system/architecture.md");
        assert_eq!(body["spec_path"], "system/architecture.md");

        // The Viewer is outside the notify set.
        let view_notifs = notif_repo
            .list_for_user(
                &Id::new("user-view"),
                Some(&Id::new("ws-1")),
                None,
                None,
                None,
                None,
                50,
                0,
            )
            .await
            .unwrap();
        assert!(
            view_notifs.is_empty(),
            "viewer must not receive spec assertion failure notifications"
        );

        // All-failing assertions must not spam one notification per failure:
        // still a single notification per user for the push.
        assert_eq!(body["failures"].as_array().unwrap().len(), 1);

        // Duplicate suppression: re-checking the same commit (mirror-sync
        // cycle re-extracting an unchanged SHA) must not notify again.
        check_spec_assertions_on_push(
            dir.path(),
            &nodes,
            &[],
            &repo_id,
            "deadbeef",
            Some(&scope),
            Some(&ports),
            &results_repo,
        )
        .await
        .unwrap();
        let dev_notifs_after = notif_repo
            .list_for_user(
                &Id::new("user-dev"),
                Some(&Id::new("ws-1")),
                None,
                None,
                None,
                None,
                50,
                0,
            )
            .await
            .unwrap();
        assert_eq!(
            dev_notifs_after.len(),
            1,
            "same-commit re-check must not duplicate the notification"
        );

        // A new commit with the same failure does notify again (the failure
        // is still live and the state is new for this push).
        check_spec_assertions_on_push(
            dir.path(),
            &nodes,
            &[],
            &repo_id,
            "deadbeef3",
            Some(&scope),
            Some(&ports),
            &results_repo,
        )
        .await
        .unwrap();
        let dev_notifs_new_commit = notif_repo
            .list_for_user(
                &Id::new("user-dev"),
                Some(&Id::new("ws-1")),
                None,
                None,
                None,
                None,
                50,
                0,
            )
            .await
            .unwrap();
        assert_eq!(
            dev_notifs_new_commit.len(),
            2,
            "a new commit with a failing assertion must notify again"
        );
    }

    /// Regression test for the task-125 review finding: push-scoped
    /// persistence must fan out to the SSE broadcast channel, not only
    /// store. When `DivergencePorts` is built with the broadcasting wrapper
    /// (as every production feeder does since the repair), a spec-assertion
    /// failure on push is both persisted AND delivered to a live
    /// `/notifications/stream` subscriber. With the bare repository this
    /// test fails at the first `try_recv` (persisted but never broadcast).
    #[tokio::test]
    async fn push_check_notifications_broadcast_to_stream_subscribers() {
        use crate::mem::{
            MemNotificationRepository, MemSpecAssertionResultRepository,
            MemWorkspaceMembershipRepository,
        };
        use crate::notifications::BroadcastingNotificationRepo;
        use gyre_common::NotificationType;
        use gyre_domain::{WorkspaceMembership, WorkspaceRole};

        // Same fixture shape as the test above: one failing assertion
        // (subject node absent from the graph), one Developer in the notify
        // set.
        let dir = tempfile::TempDir::new().unwrap();
        let specs_dir = dir.path().join("specs").join("system");
        std::fs::create_dir_all(&specs_dir).unwrap();
        std::fs::write(
            specs_dir.join("architecture.md"),
            "# Architecture\n\n<!-- gyre:assert type=\"implements\" subject=\"SearchService\" trait=\"FullTextPort\" -->\n",
        )
        .unwrap();

        let repo_id = Id::new("repo-assert-broadcast");
        let nodes: Vec<GraphNode> = vec![];
        let results_repo = MemSpecAssertionResultRepository::default();
        let notif_repo: Arc<dyn NotificationRepository> =
            Arc::new(MemNotificationRepository::default());
        let membership_repo = MemWorkspaceMembershipRepository::default();
        membership_repo
            .create(&WorkspaceMembership::new(
                Id::new("m-dev-broadcast"),
                Id::new("user-dev"),
                Id::new("ws-1"),
                WorkspaceRole::Developer,
                Id::new("inviter"),
                1_000,
            ))
            .await
            .unwrap();

        // The SSE subscriber, wired exactly like the /notifications/stream
        // handler: a receiver on the broadcast channel.
        let (tx, mut rx) = tokio::sync::broadcast::channel::<String>(64);
        let broadcasting = BroadcastingNotificationRepo::new(Arc::clone(&notif_repo), tx);
        let ports = DivergencePorts {
            notification_repo: &broadcasting,
            membership_repo: &membership_repo,
            on_created: None,
        };
        let scope = PushNotificationScope {
            workspace_id: "ws-1".to_string(),
            tenant_id: "tenant-1".to_string(),
        };

        check_spec_assertions_on_push(
            dir.path(),
            &nodes,
            &[],
            &repo_id,
            "deadbeef",
            Some(&scope),
            Some(&ports),
            &results_repo,
        )
        .await
        .unwrap();

        // Persisted …
        let dev_notifs = notif_repo
            .list_for_user(
                &Id::new("user-dev"),
                Some(&Id::new("ws-1")),
                None,
                None,
                None,
                None,
                50,
                0,
            )
            .await
            .unwrap();
        assert_eq!(dev_notifs.len(), 1, "developer must be notified");

        // … AND broadcast: exactly one event on the channel, carrying the
        // same notification the stream handler would parse and deliver.
        let msg = rx.try_recv().expect(
            "push-check notification must be broadcast to the SSE channel, not only persisted",
        );
        let notif: gyre_common::Notification = serde_json::from_str(&msg)
            .expect("broadcast payload must be serialized Notification JSON");
        assert_eq!(
            notif.notification_type,
            NotificationType::SpecAssertionFailure
        );
        assert_eq!(notif.user_id.as_str(), "user-dev");
        assert_eq!(notif.workspace_id.as_str(), "ws-1");
        assert_eq!(
            notif.source.repo_id.as_ref().map(|r| r.as_str()),
            Some("repo-assert-broadcast")
        );
        assert!(
            rx.try_recv().is_err(),
            "exactly one broadcast event is expected (no viewer notification)"
        );
    }

    // ── Compact-delta edge counts (realized-model.md §6, task-152) ─────────────

    /// Drive the REAL end-to-end extraction pipeline over a real bare git repo:
    /// commit 1 extracts nodes + a Contains edge; commit 2 removes the child
    /// file so the edge disappears. With no agent context the delta takes the
    /// compact count-only format, and `"edges_removed"` must record the TRUE
    /// removed-edge count. Regression test for the c14e5769 refactor that
    /// dropped `edges_removed_count += 1` from the removal loop (persisted
    /// `"edges_removed": 0` while edges were actually removed).
    #[tokio::test]
    async fn compact_delta_records_true_removed_edge_count() {
        use crate::mem::MemSpecAssertionResultRepository;

        let tmp = tempfile::TempDir::new().unwrap();
        let bare = tmp.path().join("repo.git");
        std::fs::create_dir_all(&bare).unwrap();
        assert!(
            std::process::Command::new("git")
                .arg("init")
                .arg("--bare")
                .arg(&bare)
                .status()
                .unwrap()
                .success(),
            "git init --bare failed"
        );

        // Working clone for authoring commits.
        let work = tmp.path().join("work");
        std::process::Command::new("git")
            .args(["clone", bare.to_str().unwrap(), work.to_str().unwrap()])
            .output()
            .unwrap();
        let git = |args: &[&str]| {
            let out = std::process::Command::new("git")
                .args(["-C", work.to_str().unwrap()])
                .args(args)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "git {:?} failed: {}",
                args,
                String::from_utf8_lossy(&out.stderr)
            );
            out
        };

        // RustExtractor.detect requires a Cargo.toml at the repo root.
        std::fs::write(
            work.join("Cargo.toml"),
            "[package]\nname = \"edge-count-fixture\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();

        // Commit 1: two public functions → two Contains edges (module → fn).
        std::fs::write(work.join("lib.rs"), "pub fn find() {}\npub fn index() {}\n").unwrap();
        git(&["add", "."]);
        git(&[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "commit",
            "-m",
            "one",
        ]);
        let sha1 = String::from_utf8_lossy(&git(&["rev-parse", "HEAD"]).stdout)
            .trim()
            .to_string();

        let store = Arc::new(MemGraphStore::new());
        let repo_id = Id::new("repo-edge-count");
        let results_repo = Arc::new(MemSpecAssertionResultRepository::default());
        // Pass 2 telemetry sinks (task-076): real in-memory sinks — the marker
        // path runs on the no-toolchain-evidence branch here, but it is the
        // production wiring, not a stub.
        let telemetry = Arc::new(CallGraphTelemetry {
            kv: Arc::new(crate::mem::MemKvStore::default()) as Arc<dyn KvJsonStore>,
            metrics: Arc::new(crate::metrics::Metrics::new().unwrap()),
        });
        extract_and_store_graph(
            work.to_str().unwrap(),
            repo_id.as_str(),
            &sha1,
            Arc::clone(&store) as Arc<dyn GraphPort>,
            "git",
            None,
            None,
            Arc::clone(&results_repo) as Arc<dyn gyre_ports::SpecAssertionResultRepository>,
            None,
            Arc::clone(&telemetry),
        )
        .await;

        let edges_after_first = store.list_edges(&repo_id, None).await.unwrap().len();
        assert!(
            edges_after_first >= 2,
            "commit 1 must extract two Contains edges, got {edges_after_first}"
        );

        // Commit 2: drop one function → its Contains edge disappears.
        std::fs::write(work.join("lib.rs"), "pub fn find() {}\n").unwrap();
        git(&["add", "."]);
        git(&[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "commit",
            "-m",
            "two",
        ]);
        let sha2 = String::from_utf8_lossy(&git(&["rev-parse", "HEAD"]).stdout)
            .trim()
            .to_string();

        extract_and_store_graph(
            work.to_str().unwrap(),
            repo_id.as_str(),
            &sha2,
            Arc::clone(&store) as Arc<dyn GraphPort>,
            "git",
            None,
            None,
            results_repo as Arc<dyn gyre_ports::SpecAssertionResultRepository>,
            None,
            telemetry,
        )
        .await;

        // The second delta must record the removed edge: compact format, no
        // agent ctx. Read it back through the port, not the in-memory write.
        let deltas = store.list_deltas(&repo_id, None, None).await.unwrap();
        assert_eq!(deltas.len(), 2, "one delta per extraction");
        let second = &deltas[1];
        let facts: serde_json::Value = serde_json::from_str(&second.delta_json).unwrap();
        let removed = facts["edges_removed"].as_u64().unwrap_or(999);
        assert_eq!(
            removed, 1,
            "compact delta must record the true removed-edge count (delta_json: {})",
            second.delta_json
        );
    }

    #[tokio::test]
    async fn pass2_dedups_calls_edge_already_present_from_pass1() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(
            dir.path().join("go.mod"),
            "module example.com/app\n\ngo 1.21\n",
        )
        .unwrap();

        let repo_id = Id::new("repo-go");
        let caller = make_go_fn("example.com/app/api.Handler", "api/handler.go", &repo_id);
        let callee = make_go_fn("example.com/app/svc.DoWork", "svc/work.go", &repo_id);
        let nodes = vec![caller.clone(), callee.clone()];

        let store = MemGraphStore::new();
        for n in &nodes {
            store.create_node(n.clone()).await.unwrap();
        }

        // Pass 1 already produced this exact Calls edge.
        let existing = vec![GraphEdge {
            id: Id::new("pass1-edge"),
            repo_id: repo_id.clone(),
            source_id: caller.id.clone(),
            target_id: callee.id.clone(),
            edge_type: EdgeType::Calls,
            metadata: None,
            first_seen_at: 1,
            last_seen_at: 1,
            deleted_at: None,
        }];

        let fake = FakeCallGraphExtractor {
            edges: vec![CallEdge {
                from: "example.com/app/api.Handler".to_string(),
                to: "example.com/app/svc.DoWork".to_string(),
            }],
        };

        let outcome =
            extract_and_persist_call_graph(dir.path(), &nodes, &existing, &repo_id, &store, &fake)
                .await;
        assert_eq!(
            outcome.edges_persisted, 0,
            "duplicate of a Pass 1 Calls edge must be skipped"
        );

        let calls = store
            .list_edges(&repo_id, Some(EdgeType::Calls))
            .await
            .unwrap();
        assert!(
            calls.is_empty(),
            "no new Calls edge should be persisted for a duplicate"
        );
    }

    #[tokio::test]
    async fn pass2_edge_ids_stable_across_runs() {
        // F8 idempotence: re-running Pass 2 over the same call structure must
        // reuse the content-derived edge id (upsert in place), not mint a new
        // UUID — consumers tracking edges by id or first_seen_at would
        // otherwise see churn on every push.
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(
            dir.path().join("go.mod"),
            "module example.com/app\n\ngo 1.21\n",
        )
        .unwrap();

        let repo_id = Id::new("repo-go");
        let caller = make_go_fn("example.com/app/api.Handler", "api/handler.go", &repo_id);
        let callee = make_go_fn("example.com/app/svc.DoWork", "svc/work.go", &repo_id);
        let nodes = vec![caller.clone(), callee.clone()];

        let store = MemGraphStore::new();
        for n in &nodes {
            store.create_node(n.clone()).await.unwrap();
        }

        let fake = FakeCallGraphExtractor {
            edges: vec![CallEdge {
                from: "example.com/app/api.Handler".to_string(),
                to: "example.com/app/svc.DoWork".to_string(),
            }],
        };

        extract_and_persist_call_graph(dir.path(), &nodes, &[], &repo_id, &store, &fake).await;
        let first = store
            .list_edges(&repo_id, Some(EdgeType::Calls))
            .await
            .unwrap();
        assert_eq!(first.len(), 1);
        let first = &first[0];

        // Ensure the second run's `now` differs, so a row replacement (rather
        // than an in-place upsert) would be observable in first_seen_at.
        tokio::time::sleep(std::time::Duration::from_millis(1100)).await;

        extract_and_persist_call_graph(dir.path(), &nodes, &[], &repo_id, &store, &fake).await;
        let second = store
            .list_edges(&repo_id, Some(EdgeType::Calls))
            .await
            .unwrap();
        assert_eq!(
            second.len(),
            1,
            "second run must not create a duplicate row"
        );
        assert_eq!(
            second[0].id, first.id,
            "edge id must be content-derived and stable across runs"
        );
        assert_eq!(
            second[0].first_seen_at, first.first_seen_at,
            "first_seen_at must be preserved across runs"
        );
    }

    #[tokio::test]
    async fn sweep_preserves_calls_edges_owned_by_pass2() {
        // F8 mid-flight race: push N+1's Pass 1 stale sweep runs while push
        // N's Pass 2 writer may still be persisting Calls edges. The sweep
        // must never touch the Calls type — a delete there races the async
        // writer and briefly empties the graph's call data.
        let repo_id = Id::new("repo-race");
        let store = MemGraphStore::new();
        let node = make_graph_node("fn-a", "crate::fn_a");

        let calls_edge = GraphEdge {
            id: Id::new("pass2-edge"),
            repo_id: repo_id.clone(),
            source_id: node.id.clone(),
            target_id: Id::new("fn-b"),
            edge_type: EdgeType::Calls,
            metadata: None,
            first_seen_at: 1,
            last_seen_at: 1,
            deleted_at: None,
        };
        let contains_edge = GraphEdge {
            id: Id::new("pass1-contains"),
            repo_id: repo_id.clone(),
            source_id: node.id.clone(),
            target_id: Id::new("mod-x"),
            edge_type: EdgeType::Contains,
            metadata: None,
            first_seen_at: 1,
            last_seen_at: 1,
            deleted_at: None,
        };
        for e in [&calls_edge, &contains_edge] {
            store.create_edge(e.clone()).await.unwrap();
        }

        // Pass 1 of the next push re-emits neither edge (e.g. both endpoints
        // moved to a file the syntax extractors no longer parse).
        let old_edge_map: HashMap<(String, String, String), GraphEdge> =
            [calls_edge.clone(), contains_edge.clone()]
                .into_iter()
                .map(|e| {
                    (
                        (
                            e.source_id.as_str().to_string(),
                            e.target_id.as_str().to_string(),
                            edge_type_key(&e.edge_type).to_string(),
                        ),
                        e,
                    )
                })
                .collect();

        let old_qn_by_id: HashMap<&str, &str> =
            [("fn-a", "crate::fn_a"), ("mod-x", "crate::mod_x")]
                .into_iter()
                .collect();
        let mut delta_edges_removed: Vec<DeltaEdgeEntry> = Vec::new();
        let removed = sweep_stale_edges(
            &old_edge_map,
            &HashMap::new(),
            &store,
            &old_qn_by_id,
            &mut delta_edges_removed,
        )
        .await
        .unwrap();
        assert_eq!(removed, 1, "only the non-Calls stale edge is removed");
        assert_eq!(
            delta_edges_removed.len(),
            1,
            "only the actually-removed edge yields a narrative entry"
        );
        assert_eq!(delta_edges_removed[0].edge_type, "contains");
        assert_eq!(delta_edges_removed[0].source, "crate::fn_a");
        assert_eq!(delta_edges_removed[0].target, "crate::mod_x");

        let live = store.list_edges(&repo_id, None).await.unwrap();
        assert_eq!(
            live.len(),
            1,
            "the Calls edge must survive the Pass 1 sweep"
        );
        assert_eq!(live[0].id, Id::new("pass2-edge"));
        assert!(live[0].deleted_at.is_none());
    }

    #[tokio::test]
    async fn pass2_reconciles_stale_calls_edges() {
        // F8 lifecycle: with the Pass 1 sweep no longer deleting Calls edges,
        // Pass 2 must reconcile its own type — a call edge that disappears
        // from the type checker's output is soft-deleted, live ones keep
        // their identity.
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(
            dir.path().join("go.mod"),
            "module example.com/app\n\ngo 1.21\n",
        )
        .unwrap();

        let repo_id = Id::new("repo-go");
        let caller = make_go_fn("example.com/app/api.Handler", "api/handler.go", &repo_id);
        let callee = make_go_fn("example.com/app/svc.DoWork", "svc/work.go", &repo_id);
        let other = make_go_fn("example.com/app/svc.Gone", "svc/gone.go", &repo_id);
        let nodes = vec![caller.clone(), callee.clone(), other.clone()];

        let store = MemGraphStore::new();
        for n in &nodes {
            store.create_node(n.clone()).await.unwrap();
        }

        // Run 1: two calls.
        let fake_two = FakeCallGraphExtractor {
            edges: vec![
                CallEdge {
                    from: "example.com/app/api.Handler".to_string(),
                    to: "example.com/app/svc.DoWork".to_string(),
                },
                CallEdge {
                    from: "example.com/app/api.Handler".to_string(),
                    to: "example.com/app/svc.Gone".to_string(),
                },
            ],
        };
        extract_and_persist_call_graph(dir.path(), &nodes, &[], &repo_id, &store, &fake_two).await;
        let after_first = store
            .list_edges(&repo_id, Some(EdgeType::Calls))
            .await
            .unwrap();
        assert_eq!(after_first.len(), 2);

        // Run 2: the call to svc.Gone no longer exists in the source.
        let fake_one = FakeCallGraphExtractor {
            edges: vec![CallEdge {
                from: "example.com/app/api.Handler".to_string(),
                to: "example.com/app/svc.DoWork".to_string(),
            }],
        };
        extract_and_persist_call_graph(dir.path(), &nodes, &[], &repo_id, &store, &fake_one).await;

        let after_second = store
            .list_edges(&repo_id, Some(EdgeType::Calls))
            .await
            .unwrap();
        assert_eq!(
            after_second.len(),
            1,
            "stale Calls edge must be reconciled away"
        );
        assert_eq!(after_second[0].target_id, callee.id);
        assert_eq!(
            after_second[0].id, after_first[0].id,
            "surviving edge keeps its content-derived id"
        );
    }

    #[tokio::test]
    async fn pass2_reconcile_skipped_when_toolchain_unavailable() {
        // A missing toolchain degrades to Ok(vec![]) — indistinguishable from
        // a call-free repo. Reconcile must not interpret that as "no calls
        // exist" and wipe the graph's Calls edges.
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(
            dir.path().join("go.mod"),
            "module example.com/app\n\ngo 1.21\n",
        )
        .unwrap();

        let repo_id = Id::new("repo-go");
        let caller = make_go_fn("example.com/app/api.Handler", "api/handler.go", &repo_id);
        let callee = make_go_fn("example.com/app/svc.DoWork", "svc/work.go", &repo_id);
        let nodes = vec![caller.clone(), callee.clone()];

        let store = MemGraphStore::new();
        for n in &nodes {
            store.create_node(n.clone()).await.unwrap();
        }
        store
            .create_edge(GraphEdge {
                id: Id::new("prior-pass2-edge"),
                repo_id: repo_id.clone(),
                source_id: caller.id.clone(),
                target_id: callee.id.clone(),
                edge_type: EdgeType::Calls,
                metadata: None,
                first_seen_at: 1,
                last_seen_at: 1,
                deleted_at: None,
            })
            .await
            .unwrap();

        let empty = FakeCallGraphExtractor { edges: vec![] };
        extract_and_persist_call_graph(dir.path(), &nodes, &[], &repo_id, &store, &empty).await;

        let calls = store
            .list_edges(&repo_id, Some(EdgeType::Calls))
            .await
            .unwrap();
        assert_eq!(
            calls.len(),
            1,
            "empty toolchain output must not wipe existing Calls edges"
        );
    }

    #[tokio::test]
    async fn pass2_reconcile_removes_legacy_duplicate_id_rows() {
        // Rows written before edge ids became content-derived carry random
        // UUIDs for the same (source, target) pair. Reconcile must collapse
        // them onto the fresh content-derived row instead of keeping both.
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(
            dir.path().join("go.mod"),
            "module example.com/app\n\ngo 1.21\n",
        )
        .unwrap();

        let repo_id = Id::new("repo-go");
        let caller = make_go_fn("example.com/app/api.Handler", "api/handler.go", &repo_id);
        let callee = make_go_fn("example.com/app/svc.DoWork", "svc/work.go", &repo_id);
        let nodes = vec![caller.clone(), callee.clone()];

        let store = MemGraphStore::new();
        for n in &nodes {
            store.create_node(n.clone()).await.unwrap();
        }
        // The legacy row: same pair, pre-content-id UUID.
        store
            .create_edge(GraphEdge {
                id: Id::new("legacy-random-uuid"),
                repo_id: repo_id.clone(),
                source_id: caller.id.clone(),
                target_id: callee.id.clone(),
                edge_type: EdgeType::Calls,
                metadata: None,
                first_seen_at: 1,
                last_seen_at: 1,
                deleted_at: None,
            })
            .await
            .unwrap();

        let fake = FakeCallGraphExtractor {
            edges: vec![CallEdge {
                from: "example.com/app/api.Handler".to_string(),
                to: "example.com/app/svc.DoWork".to_string(),
            }],
        };
        extract_and_persist_call_graph(dir.path(), &nodes, &[], &repo_id, &store, &fake).await;

        let calls = store
            .list_edges(&repo_id, Some(EdgeType::Calls))
            .await
            .unwrap();
        assert_eq!(calls.len(), 1, "legacy duplicate row must be collapsed");
        assert_ne!(calls[0].id, Id::new("legacy-random-uuid"));
        assert_eq!(calls[0].source_id, caller.id);
        assert_eq!(calls[0].target_id, callee.id);
    }
}
