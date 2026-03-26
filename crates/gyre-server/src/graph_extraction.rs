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
    graph::{ArchitecturalDelta, DeltaNodeEntry},
    Id, Notification, NotificationType,
};
use gyre_domain::WorkspaceRole;
use gyre_ports::{GraphPort, NotificationRepository, WorkspaceMembershipRepository};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::process::Command;
use tracing::{info, warn};
use uuid::Uuid;

use gyre_domain::LanguageExtractor;
use gyre_domain::RustExtractor;

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
    graph_store: &dyn GraphPort,
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
    graph_store: &dyn GraphPort,
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

    let (nodes, edges) = tokio::task::spawn_blocking(move || {
        let extractor = RustExtractor;
        if !extractor.detect(&tmp_path) {
            // Not a Rust repository — nothing to extract.
            return (vec![], vec![]);
        }

        let result = extractor.extract(&tmp_path, &sha_str);
        for err in &result.errors {
            tracing::warn!(
                file = %err.file_path,
                "extraction warning: {}",
                err.message
            );
        }

        let repo_id = Id::new(repo_id_str);
        let mut nodes = result.nodes;
        let mut edges = result.edges;

        // Fix the placeholder repo_id on every emitted node and edge.
        for n in &mut nodes {
            n.repo_id = repo_id.clone();
        }
        for e in &mut edges {
            e.repo_id = repo_id.clone();
        }

        (nodes, edges)
    })
    .await?;

    if nodes.is_empty() && edges.is_empty() {
        info!(
            %repo_id,
            %new_sha,
            "graph extraction: no nodes found (non-Rust or empty repo)"
        );
        return Ok(());
    }

    let node_count = nodes.len();
    let edge_count = edges.len();
    let repo_id_parsed = Id::new(repo_id.to_string());

    // --- Step 3: clear stale data, then persist the new snapshot --------------

    graph_store.delete_nodes_by_repo(&repo_id_parsed).await?;
    graph_store.delete_edges_by_repo(&repo_id_parsed).await?;

    for node in &nodes {
        graph_store.create_node(node.clone()).await?;
    }
    for edge in edges {
        graph_store.create_edge(edge).await?;
    }

    // --- Step 4: record an architectural delta --------------------------------

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    // When agent context is present, build a richer delta_json with nodes_added
    // (filtered to spec_path-matching nodes when possible) for the divergence detector.
    let (delta_json, delta_agent_id, delta_spec_ref) = if let Some(ref ctx) = agent_ctx {
        let spec_ref_str = ctx.spec_ref.as_str();

        // Collect compact node entries for divergence comparison.
        // Prefer nodes whose spec_path matches the agent's spec_ref;
        // fall back to ALL nodes if none match (entire extraction may implement the spec).
        let spec_matched: Vec<DeltaNodeEntry> = nodes
            .iter()
            .filter(|n| n.spec_path.as_deref() == Some(spec_ref_str))
            .map(|n| DeltaNodeEntry {
                name: n.name.clone(),
                node_type: format!("{:?}", n.node_type).to_lowercase(),
                qualified_name: n.qualified_name.clone(),
            })
            .collect();

        let nodes_added = if spec_matched.is_empty() {
            nodes
                .iter()
                .map(|n| DeltaNodeEntry {
                    name: n.name.clone(),
                    node_type: format!("{:?}", n.node_type).to_lowercase(),
                    qualified_name: n.qualified_name.clone(),
                })
                .collect::<Vec<_>>()
        } else {
            spec_matched
        };

        // TODO(TASK-264-followup): nodes_modified field-level change detection is deferred.
        // Implementing it requires diffing the current extraction against the previous delta's
        // nodes_added, which means retaining the prior state between pushes.  The FieldChange
        // struct in gyre-common/src/graph.rs is wired for this when that work lands.
        // For now we store an empty array so the divergence checker can focus on nodes_added.
        let json = serde_json::json!({
            "nodes_extracted": node_count,
            "edges_extracted": edge_count,
            "nodes_added": nodes_added,
            "nodes_modified": serde_json::Value::Array(vec![]),
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
        "knowledge graph extraction complete"
    );

    // --- Step 5: post-extraction divergence check (HSI §8 priority 5) ---------

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
                graph_store,
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
}
