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
    graph::{ArchitecturalDelta, DeltaNodeEntry, EdgeType, FieldChange, GraphEdge, GraphNode},
    Id, Notification, NotificationType,
};
use gyre_domain::{
    GoExtractor, LanguageExtractor, PythonExtractor, RustExtractor, TypeScriptExtractor,
    WorkspaceRole,
};
use gyre_ports::{GraphPort, NotificationRepository, WorkspaceMembershipRepository};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::process::Command;
use tracing::{info, warn};
use uuid::Uuid;

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

/// Port references needed only for the post-extraction divergence check.
///
/// Bundled into a single struct to stay within clippy's argument-count limit.
pub struct DivergencePorts<'a> {
    pub notification_repo: &'a dyn NotificationRepository,
    pub membership_repo: &'a dyn WorkspaceMembershipRepository,
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
/// All errors are logged and swallowed — extraction must never fail a push.
pub async fn extract_and_store_graph(
    repo_path: &str,
    repo_id: &str,
    new_sha: &str,
    graph_store: Arc<dyn GraphPort>,
    git_bin: &str,
    agent_ctx: Option<AgentPushContext>,
    divergence_ports: Option<DivergencePorts<'_>>,
) {
    if let Err(e) = do_extract(
        repo_path,
        repo_id,
        new_sha,
        graph_store,
        git_bin,
        agent_ctx,
        divergence_ports,
    )
    .await
    {
        warn!(%repo_id, %new_sha, "knowledge graph extraction failed: {e}");
    }
}

async fn do_extract(
    repo_path: &str,
    repo_id: &str,
    new_sha: &str,
    graph_store: Arc<dyn GraphPort>,
    git_bin: &str,
    agent_ctx: Option<AgentPushContext>,
    divergence_ports: Option<DivergencePorts<'_>>,
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
        }
    }
    for (key, edge) in &old_edge_map {
        if !new_edge_map.contains_key(key) {
            graph_store.delete_edge(&edge.id).await?;
            edges_removed_count += 1;
        }
    }

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
            "edges_added": edges_added_count,
            "edges_removed": edges_removed_count,
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

    // --- Step 7: Non-blocking Pass 2 (LSP) -----------------------------------
    // Per lsp-call-graph.md, the graph is usable after Pass 1 with partial call data.
    // Pass 2 runs in the background and merges additional edges when done.
    // The temp directory is moved into the spawned task so it stays alive.
    {
        let pass2_nodes = final_nodes.clone();
        let pass2_edges: Vec<GraphEdge> = new_edge_map.into_values().collect();
        let pass2_repo_root = tmp.path().to_path_buf();
        let pass2_repo_id = repo_id_parsed.clone();
        let pass2_sha = new_sha.to_string();
        let pass2_graph_store = Arc::clone(&graph_store);

        // Fire-and-forget: spawn a background task so Pass 2 never blocks the
        // push response.  The `_tmp` binding keeps the temp directory alive
        // until the LSP analysis finishes.
        tokio::spawn(async move {
            let _tmp = tmp; // prevent TempDir drop until this task completes

            let lsp_edges = tokio::task::spawn_blocking(move || {
                extract_lsp_edges(
                    &pass2_repo_root,
                    &pass2_nodes,
                    &pass2_edges,
                    &pass2_repo_id,
                    &pass2_sha,
                )
            })
            .await
            .unwrap_or_default();

            // Persist any new LSP-discovered edges.
            for edge in lsp_edges {
                let _ = pass2_graph_store.create_edge(edge).await;
            }
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
        notif.entity_ref = Some(spec_ref.to_string());
        notif.repo_id = Some(repo_id.as_str().to_string());

        if let Err(e) = ports.notification_repo.create(&notif).await {
            warn!(
                user_id = %member.user_id,
                "failed to create divergence notification: {e}"
            );
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

/// Run Pass 2 (LSP) extraction and return additional edges.
/// This is designed to run AFTER Pass 1 results are already persisted,
/// so the graph is usable immediately and becomes complete when Pass 2 finishes.
pub fn extract_lsp_edges(
    repo_root: &Path,
    nodes: &[GraphNode],
    existing_edges: &[GraphEdge],
    repo_id: &Id,
    commit_sha: &str,
) -> Vec<GraphEdge> {
    if !repo_root.join("Cargo.toml").is_file() {
        return vec![];
    }

    let lsp_result = gyre_domain::lsp_call_graph::extract_call_graph(
        repo_root,
        nodes,
        existing_edges,
        repo_id,
        commit_sha,
    );

    if !lsp_result.errors.is_empty() {
        for err in &lsp_result.errors {
            tracing::info!("LSP call graph: {err}");
        }
    }

    if lsp_result.new_edges_found > 0 {
        tracing::info!(
            definitions = lsp_result.definitions_queried,
            new_edges = lsp_result.new_edges_found,
            "LSP call graph extraction complete"
        );
    }

    lsp_result.edges
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
}
