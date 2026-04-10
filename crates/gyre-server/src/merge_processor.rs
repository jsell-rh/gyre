use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use tracing::{error, info, warn};

use gyre_common::Id;
use gyre_domain::{
    AnalyticsEvent, GateStatus, MergeQueueEntry, MergeQueueEntryStatus, MergeRequest, MergeResult,
    MrStatus, TaskPriority,
};
use uuid::Uuid;

use crate::AppState;

pub fn spawn_merge_processor(state: Arc<AppState>) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(5));
        loop {
            interval.tick().await;
            if let Err(e) = process_next(&state).await {
                error!("merge processor error: {e:#}");
            }
        }
    });
}

/// Run one merge-processor cycle. Used by the job framework for manual triggering.
pub async fn run_once(state: &AppState) -> anyhow::Result<()> {
    process_next(state).await
}

/// Check if all `depends_on` for the given MR have status `Merged`.
/// Returns `Ok(true)` if all dependencies are satisfied, `Ok(false)` if any are pending.
async fn dependencies_satisfied(state: &AppState, mr_id: &Id) -> anyhow::Result<bool> {
    let mr = match state.merge_requests.find_by_id(mr_id).await? {
        Some(m) => m,
        None => return Ok(true), // MR not found — let the processor handle the error
    };
    for dep_id in &mr.depends_on {
        match state.merge_requests.find_by_id(dep_id).await? {
            Some(dep) if dep.status == MrStatus::Merged => continue,
            Some(_) => return Ok(false), // dependency not yet merged
            None => return Ok(false),    // missing dep — block until resolved
        }
    }
    Ok(true)
}

/// Check if all members of an atomic group are ready (all gates passed, deps satisfied).
/// Returns `Ok(true)` if no group or all members are ready.
async fn atomic_group_ready(state: &AppState, group: &str, mr_id: &Id) -> anyhow::Result<bool> {
    let all_mrs = state.merge_requests.list().await?;
    let members: Vec<_> = all_mrs
        .iter()
        .filter(|m| m.atomic_group.as_deref() == Some(group))
        .collect();

    for member in &members {
        if member.id == *mr_id {
            continue; // the current MR is handled by the caller
        }
        // Check deps satisfied for each group member
        if !dependencies_satisfied(state, &member.id).await? {
            return Ok(false);
        }
        // Check if the member is in the queue and gates are ready
        match crate::gate_executor::check_gates_for_mr(state, &member.id).await {
            Ok(true) => {}
            _ => return Ok(false),
        }
    }
    Ok(true)
}

/// Check dependencies for health issues and handle them (P5):
///
/// - Closed deps: creates a remediation task and fails the queue entry.
/// - Deps with 3+ gate failures: logs an escalation warning.
///
/// Returns `true` if a blocking issue was found (caller should skip further processing).
async fn handle_dep_health_issues(
    state: &AppState,
    entry: &MergeQueueEntry,
    mr: &MergeRequest,
) -> anyhow::Result<bool> {
    for dep_id in &mr.depends_on {
        let dep = match state.merge_requests.find_by_id(dep_id).await? {
            Some(d) => d,
            None => continue,
        };

        // Case 1: Dependency MR was closed before merging.
        if dep.status == MrStatus::Closed {
            warn!(
                mr_id = %mr.id,
                dep_id = %dep_id,
                "dependency MR was closed; failing queue entry and creating reassessment task"
            );

            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let task_id = Id::new(Uuid::new_v4().to_string());
            let mut task = gyre_domain::Task::new(
                task_id,
                format!("Dependency MR-{dep_id} was closed, reassess MR-{}", mr.id),
                now,
            );
            task.priority = TaskPriority::High;
            task.labels = vec!["dep-failure".to_string(), "auto-created".to_string()];
            task.description = Some(format!(
                "Dependency merge request {dep_id} was closed before merging. \
                 Review whether MR {} can still proceed or needs to be updated.",
                mr.id
            ));
            if let Err(e) = state.tasks.create(&task).await {
                warn!(mr_id = %mr.id, "failed to create dep-failure task: {e}");
            }

            state
                .merge_queue
                .update_status(
                    &entry.id,
                    MergeQueueEntryStatus::Failed,
                    Some(format!("dependency MR-{dep_id} was closed")),
                )
                .await?;

            return Ok(true);
        }

        // Case 2: Dependency has 3+ gate failures — log escalation.
        let gate_fail_count = state
            .gate_results
            .list_by_mr_id(dep_id.as_str())
            .await
            .unwrap_or_default()
            .iter()
            .filter(|r| matches!(r.status, GateStatus::Failed))
            .count();

        if gate_fail_count >= 3 {
            warn!(
                mr_id = %mr.id,
                dep_id = %dep_id,
                gate_fail_count,
                "dependency MR has {} gate failures; escalating — manual intervention may be needed",
                gate_fail_count,
            );
        }
    }

    Ok(false)
}

/// Build a dependency graph from queued entries and their MRs.
///
/// Returns an adjacency list (mr_id → set of mr_ids it depends on) restricted to
/// entries currently in the queue. Dependencies on MRs outside the queue (already
/// merged or not yet queued) are not included — they are handled by
/// `dependencies_satisfied()` at merge time.
async fn build_queue_dependency_graph(
    state: &AppState,
    queued_entries: &[MergeQueueEntry],
) -> anyhow::Result<HashMap<String, HashSet<String>>> {
    let queued_mr_ids: HashSet<String> = queued_entries
        .iter()
        .map(|e| e.merge_request_id.to_string())
        .collect();

    let mut graph: HashMap<String, HashSet<String>> = HashMap::new();

    // Initialize every queued MR with an empty dependency set.
    for mr_id in &queued_mr_ids {
        graph.entry(mr_id.clone()).or_default();
    }

    // Populate edges from depends_on.
    for entry in queued_entries {
        let mr_id = entry.merge_request_id.to_string();
        if let Ok(Some(mr)) = state
            .merge_requests
            .find_by_id(&entry.merge_request_id)
            .await
        {
            for dep in &mr.depends_on {
                let dep_str = dep.to_string();
                if queued_mr_ids.contains(&dep_str) {
                    graph.entry(mr_id.clone()).or_default().insert(dep_str);
                }
            }

            // Atomic group members imply ordering: within the same group, members
            // form a dependency chain in their enqueue order. This ensures the
            // topological sort respects atomic group sequencing.
            if let Some(ref group) = mr.atomic_group {
                let group_members: Vec<_> = queued_entries
                    .iter()
                    .filter(|e| {
                        e.merge_request_id != entry.merge_request_id
                            && queued_mr_ids.contains(&e.merge_request_id.to_string())
                    })
                    .collect();

                for other in &group_members {
                    if let Ok(Some(other_mr)) = state
                        .merge_requests
                        .find_by_id(&other.merge_request_id)
                        .await
                    {
                        if other_mr.atomic_group.as_deref() == Some(group) {
                            // Earlier-enqueued member is a dependency of later-enqueued member.
                            if other.enqueued_at < entry.enqueued_at {
                                graph
                                    .entry(mr_id.clone())
                                    .or_default()
                                    .insert(other.merge_request_id.to_string());
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(graph)
}

/// Topological sort using Kahn's algorithm with priority-aware tier ordering.
///
/// Within each tier of independent MRs (nodes whose in-queue dependencies are all
/// resolved), higher-priority entries are processed first. This implements the spec's
/// "Priority Within Dependency Tiers" requirement: dependencies define ordering
/// constraints, but within independent MRs, priority still applies.
///
/// Returns entries in processing order.
fn topological_sort_with_priority(
    entries: &[MergeQueueEntry],
    graph: &HashMap<String, HashSet<String>>,
) -> Vec<MergeQueueEntry> {
    // Build a priority lookup: mr_id → (priority, enqueued_at) for tie-breaking.
    let entry_map: HashMap<String, &MergeQueueEntry> = entries
        .iter()
        .map(|e| (e.merge_request_id.to_string(), e))
        .collect();

    // Compute in-degree for each node.
    // in_deg[node] = number of in-graph dependencies node has (edges INTO node).
    let mut in_deg: HashMap<String, usize> = graph.keys().map(|k| (k.clone(), 0)).collect();
    for (node, deps) in graph {
        for dep in deps {
            if graph.contains_key(dep) {
                // `node` depends on `dep`, so `node` has an incoming edge from `dep`.
                // But in-degree counts edges INTO a node. `dep → node` means in_deg[node] += 1.
                // Wait — the graph is mr_id → {its dependencies}. So graph[A] = {B} means
                // A depends on B, i.e., edge B → A (B must come before A).
                // in_degree[A] should count how many dependencies A has in the graph.
                *in_deg.entry(node.clone()).or_insert(0) += 1;
            }
        }
    }

    let mut result = Vec::new();
    let mut available: Vec<String> = in_deg
        .iter()
        .filter(|(_, &deg)| deg == 0)
        .map(|(id, _)| id.clone())
        .collect();

    while !available.is_empty() {
        // Sort available nodes by priority descending, then enqueued_at ascending for stability.
        available.sort_by(|a, b| {
            let ea = entry_map.get(a);
            let eb = entry_map.get(b);
            let pa = ea.map(|e| e.priority).unwrap_or(0);
            let pb = eb.map(|e| e.priority).unwrap_or(0);
            let ta = ea.map(|e| e.enqueued_at).unwrap_or(u64::MAX);
            let tb = eb.map(|e| e.enqueued_at).unwrap_or(u64::MAX);
            pb.cmp(&pa).then(ta.cmp(&tb))
        });

        // Take the highest-priority available node.
        let node = available.remove(0);

        if let Some(entry) = entry_map.get(&node) {
            result.push((*entry).clone());
        }

        // Remove this node's outgoing edges (other nodes that depend on it).
        for (other, deps) in graph.iter() {
            if deps.contains(&node) {
                if let Some(deg) = in_deg.get_mut(other) {
                    *deg = deg.saturating_sub(1);
                    if *deg == 0
                        && !result
                            .iter()
                            .any(|e| e.merge_request_id.to_string() == *other)
                    {
                        available.push(other.clone());
                    }
                }
            }
        }
    }

    result
}

/// Compute the maximum dependency chain depth in the graph using topological sort + DP.
///
/// Uses topological ordering to compute the longest path (chain depth) from any root
/// to any leaf. BFS finds shortest paths and would underestimate depth in DAGs with
/// diamond patterns; this algorithm computes the correct longest path in a DAG.
///
/// Returns the maximum chain depth (0 = no dependencies, 1 = one level, etc.).
fn compute_max_chain_depth(graph: &HashMap<String, HashSet<String>>) -> usize {
    if graph.is_empty() {
        return 0;
    }

    // Compute in-degree for topological ordering.
    let mut in_deg: HashMap<String, usize> = graph.keys().map(|k| (k.clone(), 0)).collect();
    for (node, deps) in graph {
        for dep in deps {
            if graph.contains_key(dep) {
                *in_deg.entry(node.clone()).or_insert(0) += 1;
            }
        }
    }

    // Kahn's algorithm to get topological order.
    let mut queue: VecDeque<String> = in_deg
        .iter()
        .filter(|(_, &deg)| deg == 0)
        .map(|(id, _)| id.clone())
        .collect();

    // depth[node] = longest path ending at node.
    let mut depth: HashMap<String, usize> = graph.keys().map(|k| (k.clone(), 0)).collect();

    let mut topo_order = Vec::new();
    while let Some(node) = queue.pop_front() {
        topo_order.push(node.clone());
        // For every other node that depends on `node`, reduce in-degree.
        for (other, deps) in graph.iter() {
            if deps.contains(&node) {
                let new_depth = depth.get(&node).copied().unwrap_or(0) + 1;
                let entry = depth.entry(other.clone()).or_insert(0);
                if new_depth > *entry {
                    *entry = new_depth;
                }
                if let Some(deg) = in_deg.get_mut(other) {
                    *deg = deg.saturating_sub(1);
                    if *deg == 0 {
                        queue.push_back(other.clone());
                    }
                }
            }
        }
    }

    depth.values().copied().max().unwrap_or(0)
}

/// Emit a warning notification when the dependency chain depth exceeds 10 levels.
///
/// Per merge-dependencies.md §Failure Handling: "Dependency chain too deep (>10 levels):
/// Warning to repo orchestrator. Not rejected, but flagged as a decomposition smell."
async fn warn_deep_chain(state: &AppState, depth: usize, workspace_id: &Id) {
    // caller-scope:ok — single workspace notification, not iterating cross-workspace entities
    warn!(
        chain_depth = depth,
        workspace_id = workspace_id.as_str(),
        "MR dependency chain is {} levels deep (>10) — decomposition smell",
        depth
    );

    let members = state
        .workspace_memberships
        .list_by_workspace(workspace_id)
        .await
        .unwrap_or_default();

    let body_json = serde_json::json!({
        "chain_depth": depth,
        "threshold": 10,
        "workspace_id": workspace_id.as_str(),
    })
    .to_string();

    for member in &members {
        crate::notifications::notify_rich(
            state,
            workspace_id.clone(),
            member.user_id.clone(),
            gyre_common::NotificationType::DependencyChainTooDeep,
            format!("MR dependency chain is {depth} levels deep — consider decomposing further"),
            "default",
            Some(body_json.clone()),
            None,
            None,
        )
        .await;
    }
}

/// Merge all members of an atomic group as a single transactional unit.
///
/// Per merge-dependencies.md §Atomic Group Rules:
/// 1. All members must have all gates passed before any member merges.
/// 2. The merge processor locks the queue and merges all members in dependency
///    order with no interleaving.
/// 3. If any member fails to merge (conflict), the entire group is rolled back
///    and requeued.
/// 4. Atomic groups imply dependency ordering among members.
///
/// Returns `Ok(())` on success or failure — errors are handled internally
/// (rollback, requeue, notify). Only propagates infrastructure errors.
async fn merge_atomic_group(
    state: &AppState,
    group_name: &str,
    first_mr: &MergeRequest,
    sorted: &[MergeQueueEntry],
    queued_entries: &[MergeQueueEntry],
) -> anyhow::Result<()> {
    // Collect all group members from the queued entries, sorted by topological order.
    let group_member_ids: HashSet<String> = {
        let all_mrs = state.merge_requests.list().await?;
        all_mrs
            .iter()
            .filter(|m| m.atomic_group.as_deref() == Some(group_name))
            .map(|m| m.id.to_string())
            .collect()
    };

    // Filter sorted entries to only those in this group, preserving topological order.
    let group_entries: Vec<MergeQueueEntry> = sorted
        .iter()
        .filter(|e| group_member_ids.contains(&e.merge_request_id.to_string()))
        .filter(|e| {
            queued_entries
                .iter()
                .any(|q| q.id == e.id && q.status == MergeQueueEntryStatus::Queued)
        })
        .cloned()
        .collect();

    if group_entries.is_empty() {
        return Ok(());
    }

    info!(
        group = %group_name,
        member_count = group_entries.len(),
        "starting atomic group transactional merge"
    );

    // Look up the repository (all group members share the same repo).
    let repo = match state.repos.find_by_id(&first_mr.repository_id).await? {
        Some(r) => r,
        None => {
            warn!(repo_id = %first_mr.repository_id, "repository not found for atomic group");
            for ge in &group_entries {
                state
                    .merge_queue
                    .update_status(
                        &ge.id,
                        MergeQueueEntryStatus::Failed,
                        Some("repository not found".to_string()),
                    )
                    .await?;
            }
            return Ok(());
        }
    };

    // Record the target branch HEAD before starting, for rollback.
    let target_branch = &first_mr.target_branch;
    let pre_group_sha =
        crate::git_refs::resolve_ref(&repo.path, &format!("refs/heads/{target_branch}")).await;

    // Mark all group members as Processing.
    for ge in &group_entries {
        state
            .merge_queue
            .update_status(&ge.id, MergeQueueEntryStatus::Processing, None)
            .await?;
    }

    // Track successfully merged entries for potential rollback.
    let mut merged_entries: Vec<(MergeQueueEntry, MergeRequest)> = Vec::new();

    // Sequentially merge each member in dependency order.
    for ge in &group_entries {
        let mr = match state
            .merge_requests
            .find_by_id(&ge.merge_request_id)
            .await?
        {
            Some(m) => m,
            None => {
                warn!(mr_id = %ge.merge_request_id, "MR not found during atomic group merge");
                // Rollback and requeue.
                rollback_atomic_group(
                    state,
                    group_name,
                    &repo,
                    target_branch,
                    pre_group_sha.as_deref(),
                    &merged_entries,
                    &group_entries,
                    &format!("MR {} not found", ge.merge_request_id),
                    &ge.merge_request_id,
                )
                .await?;
                return Ok(());
            }
        };

        // P5: Check dependency health for this member.
        if handle_dep_health_issues(state, ge, &mr).await? {
            // Dep health issue found — rollback the group.
            rollback_atomic_group(
                state,
                group_name,
                &repo,
                target_branch,
                pre_group_sha.as_deref(),
                &merged_entries,
                &group_entries,
                &format!("dependency health issue for MR {}", mr.id),
                &mr.id,
            )
            .await?;
            return Ok(());
        }

        // Attempt the merge for this member.
        let result = state
            .git_ops
            .merge_branches(&repo.path, &mr.source_branch, &mr.target_branch)
            .await;

        match result {
            Ok(MergeResult::Success { merge_commit_sha }) => {
                info!(
                    group = %group_name,
                    mr_id = %mr.id,
                    sha = %merge_commit_sha,
                    "atomic group member merged successfully"
                );

                // Update MR status to Merged.
                let mut updated_mr = mr.clone();
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                updated_mr.updated_at = now;
                if updated_mr.status == MrStatus::Open {
                    let _ = updated_mr.transition_status(MrStatus::Approved);
                }
                if let Err(e) = updated_mr.transition_status(MrStatus::Merged) {
                    warn!("could not transition MR to Merged: {e}");
                }
                let _ = state.merge_requests.update(&updated_mr).await;

                state
                    .merge_queue
                    .update_status(&ge.id, MergeQueueEntryStatus::Merged, None)
                    .await?;

                // Track analytics.
                let ev = AnalyticsEvent::new(
                    Id::new(Uuid::new_v4().to_string()),
                    "merge_queue.processed",
                    updated_mr.author_agent_id.as_ref().map(|id| id.to_string()),
                    serde_json::json!({
                        "entry_id": ge.id.to_string(),
                        "mr_id": updated_mr.id.to_string(),
                        "result": "merged",
                        "atomic_group": group_name,
                    }),
                    now,
                );
                let _ = state.analytics.record(&ev).await;

                // Notify the MR author.
                if let Some(ref author_id) = updated_mr.author_agent_id {
                    crate::notifications::notify_mr_merged(
                        state,
                        author_id,
                        &updated_mr.workspace_id,
                        &updated_mr.id.to_string(),
                        "default",
                    )
                    .await;
                }

                merged_entries.push((ge.clone(), updated_mr));
            }
            Ok(MergeResult::Conflict { message }) => {
                warn!(
                    group = %group_name,
                    mr_id = %mr.id,
                    reason = %message,
                    "atomic group member merge conflict — rolling back entire group"
                );
                rollback_atomic_group(
                    state,
                    group_name,
                    &repo,
                    target_branch,
                    pre_group_sha.as_deref(),
                    &merged_entries,
                    &group_entries,
                    &format!("conflict in MR {}: {}", mr.id, message),
                    &mr.id,
                )
                .await?;
                return Ok(());
            }
            Err(e) => {
                error!(
                    group = %group_name,
                    mr_id = %mr.id,
                    error = %e,
                    "atomic group member git error — rolling back entire group"
                );
                rollback_atomic_group(
                    state,
                    group_name,
                    &repo,
                    target_branch,
                    pre_group_sha.as_deref(),
                    &merged_entries,
                    &group_entries,
                    &format!("git error in MR {}: {}", mr.id, e),
                    &mr.id,
                )
                .await?;
                return Ok(());
            }
        }
    }

    info!(
        group = %group_name,
        merged_count = merged_entries.len(),
        "atomic group merged successfully"
    );

    Ok(())
}

/// Roll back an atomic group merge: reset the target branch, requeue all members,
/// and notify all distinct authors.
async fn rollback_atomic_group(
    state: &AppState,
    group_name: &str,
    repo: &gyre_domain::Repository,
    target_branch: &str,
    pre_group_sha: Option<&str>,
    merged_entries: &[(MergeQueueEntry, MergeRequest)],
    all_group_entries: &[MergeQueueEntry],
    failure_reason: &str,
    failing_mr_id: &Id,
) -> anyhow::Result<()> {
    // Step 1: Reset the target branch to the pre-group SHA (undo already-merged members).
    if !merged_entries.is_empty() {
        if let Some(sha) = pre_group_sha {
            info!(
                group = %group_name,
                target_branch = %target_branch,
                reset_to = %sha,
                merged_count = merged_entries.len(),
                "rolling back atomic group: resetting target branch"
            );
            if let Err(e) = state
                .git_ops
                .reset_branch(&repo.path, target_branch, sha)
                .await
            {
                error!(
                    group = %group_name,
                    error = %e,
                    "failed to reset branch during atomic group rollback"
                );
            }
        } else {
            warn!(
                group = %group_name,
                "no pre-group SHA recorded; cannot reset branch"
            );
        }

        // Revert MR statuses for already-merged members.
        for (_, mr) in merged_entries {
            let mut reverted_mr = mr.clone();
            reverted_mr.status = MrStatus::Open;
            reverted_mr.updated_at = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let _ = state.merge_requests.update(&reverted_mr).await;
        }
    }

    // Step 2: Requeue all group members (set status back to Queued with failure reason).
    for ge in all_group_entries {
        state
            .merge_queue
            .update_status(
                &ge.id,
                MergeQueueEntryStatus::Queued,
                Some(format!("atomic group rollback: {failure_reason}")),
            )
            .await?;
    }

    // Step 3: Collect distinct author agent IDs from all group members.
    let mut author_ids: HashSet<String> = HashSet::new();
    let mut workspace_id: Option<Id> = None;
    for ge in all_group_entries {
        if let Ok(Some(mr)) = state.merge_requests.find_by_id(&ge.merge_request_id).await {
            if let Some(ref author) = mr.author_agent_id {
                author_ids.insert(author.to_string());
            }
            if workspace_id.is_none() {
                workspace_id = Some(mr.workspace_id.clone());
            }
        }
    }

    let ws_id = workspace_id.unwrap_or_else(|| Id::new("default"));

    // Step 4: Notify all distinct authors.
    let members = state
        .workspace_memberships
        .list_by_workspace(&ws_id)
        .await
        .unwrap_or_default();

    // Collect user IDs for notification: all distinct MR author agent IDs
    // plus workspace members.
    let mut notify_user_ids: HashSet<String> = author_ids.clone();
    for member in &members {
        notify_user_ids.insert(member.user_id.to_string());
    }

    let body_json = serde_json::json!({
        "group": group_name,
        "failing_mr_id": failing_mr_id.to_string(),
        "failure_reason": failure_reason,
        "member_count": all_group_entries.len(),
        "rolled_back_count": merged_entries.len(),
    })
    .to_string();

    for user_id in &notify_user_ids {
        crate::notifications::notify_rich(
            state,
            ws_id.clone(),
            Id::new(user_id),
            gyre_common::NotificationType::AtomicGroupFailure,
            format!(
                "Atomic group '{}' failed: {} — all members rolled back and requeued",
                group_name, failure_reason
            ),
            "default",
            Some(body_json.clone()),
            Some(failing_mr_id.to_string()),
            Some(repo.id.to_string()),
        )
        .await;
    }

    // Step 5: Emit AtomicGroupFailed event.
    state
        .emit_event(
            Some(ws_id.clone()),
            gyre_common::message::Destination::Workspace(ws_id),
            gyre_common::MessageKind::AtomicGroupFailed,
            Some(serde_json::json!({
                "group": group_name,
                "failing_mr_id": failing_mr_id.to_string(),
                "failure_reason": failure_reason,
                "member_count": all_group_entries.len(),
                "rolled_back_count": merged_entries.len(),
                "member_mr_ids": all_group_entries.iter().map(|e| e.merge_request_id.to_string()).collect::<Vec<_>>(),
            })),
        )
        .await;

    warn!(
        group = %group_name,
        failing_mr = %failing_mr_id,
        "atomic group rollback complete: {} members requeued, {} authors notified",
        all_group_entries.len(),
        notify_user_ids.len(),
    );

    Ok(())
}

async fn process_next(state: &AppState) -> anyhow::Result<()> {
    // Step 1: Get all queued entries.
    let all_queued = state.merge_queue.list_queue().await?;
    let queued_entries: Vec<_> = all_queued
        .into_iter()
        .filter(|e| e.status == MergeQueueEntryStatus::Queued)
        .collect();

    if queued_entries.is_empty() {
        return Ok(());
    }

    // Step 2: Build dependency graph from depends_on + atomic_group.
    let graph = build_queue_dependency_graph(state, &queued_entries).await?;

    // Step 2b: Check chain depth and warn if >10.
    let max_depth = compute_max_chain_depth(&graph);
    if max_depth > 10 {
        // Find a workspace_id from any queued entry for the notification target.
        if let Some(first) = queued_entries.first() {
            if let Ok(Some(mr)) = state
                .merge_requests
                .find_by_id(&first.merge_request_id)
                .await
            {
                warn_deep_chain(state, max_depth, &mr.workspace_id).await;
            }
        }
    }

    // Step 3: Topological sort (respecting priority within each tier).
    let sorted = topological_sort_with_priority(&queued_entries, &graph);

    // Step 4: For each entry in topological order, check deps (4a), gates (4b),
    // and atomic group readiness (4c). Skip entries that aren't ready; select
    // the first entry that passes all checks.
    let (entry, mr) = {
        let mut found = None;
        for candidate in &sorted {
            // Step 4a: Are all dependencies Merged?
            if !dependencies_satisfied(state, &candidate.merge_request_id).await? {
                continue;
            }

            // Step 4b: Are all gates passed?
            match crate::gate_executor::check_gates_for_mr(state, &candidate.merge_request_id).await
            {
                Ok(true) => {}         // all gates passed (or no gates)
                Ok(false) => continue, // gates still running — skip, try next candidate
                Err(reason) => {
                    // Gate failed — fail this entry and try next candidate.
                    warn!(
                        entry_id = %candidate.id,
                        reason = %reason,
                        "quality gate failed during candidate selection, failing entry"
                    );
                    state
                        .merge_queue
                        .update_status(
                            &candidate.id,
                            MergeQueueEntryStatus::Failed,
                            Some(format!("quality gate failed: {reason}")),
                        )
                        .await?;
                    continue;
                }
            }

            // Look up the MR (needed for atomic group check and later processing).
            let candidate_mr = match state
                .merge_requests
                .find_by_id(&candidate.merge_request_id)
                .await?
            {
                Some(mr) => mr,
                None => {
                    warn!(mr_id = %candidate.merge_request_id, "MR not found for queue entry, skipping");
                    state
                        .merge_queue
                        .update_status(
                            &candidate.id,
                            MergeQueueEntryStatus::Failed,
                            Some("merge request not found".to_string()),
                        )
                        .await?;
                    continue;
                }
            };

            // Step 4c: Is this part of an atomic group? If so, are all members ready?
            if let Some(ref group) = candidate_mr.atomic_group {
                match atomic_group_ready(state, group, &candidate_mr.id).await {
                    Ok(true) => {}
                    Ok(false) => {
                        info!(
                            entry_id = %candidate.id,
                            group = %group,
                            "atomic group not ready, skipping to next candidate"
                        );
                        continue;
                    }
                    Err(e) => {
                        warn!(entry_id = %candidate.id, error = %e, "error checking atomic group, skipping");
                        continue;
                    }
                }
            }

            found = Some((candidate.clone(), candidate_mr));
            break;
        }
        match found {
            Some(pair) => pair,
            None => return Ok(()),
        }
    };

    info!(entry_id = %entry.id, mr_id = %entry.merge_request_id, "processing merge queue entry");

    // If this entry is part of an atomic group, merge all group members
    // as a transactional unit — no interleaving allowed.
    if let Some(ref group_name) = mr.atomic_group {
        return merge_atomic_group(state, group_name, &mr, &sorted, &queued_entries).await;
    }

    // ── Single-entry merge path (non-atomic-group) ─────────────────────

    // Mark as Processing
    state
        .merge_queue
        .update_status(&entry.id, MergeQueueEntryStatus::Processing, None)
        .await?;

    // P5: Check dependency health (closed deps, gate failure escalation).
    if handle_dep_health_issues(state, &entry, &mr).await? {
        return Ok(());
    }

    // Look up the repository
    let repo = match state.repos.find_by_id(&mr.repository_id).await? {
        Some(r) => r,
        None => {
            warn!(repo_id = %mr.repository_id, "repository not found for MR");
            state
                .merge_queue
                .update_status(
                    &entry.id,
                    MergeQueueEntryStatus::Failed,
                    Some("repository not found".to_string()),
                )
                .await?;
            return Ok(());
        }
    };

    // Note: Atomic group readiness (step 4c) is already verified in the
    // selection loop above — no need to re-check here.

    // Check spec enforcement policy before merging.
    {
        let policy = state
            .spec_policies
            .get_for_repo(&repo.id.to_string())
            .await
            .unwrap_or_default();

        if policy.require_spec_ref || policy.require_approved_spec {
            if mr.spec_ref.is_none() {
                let reason = "spec policy requires a spec_ref but MR has none".to_string();
                warn!(entry_id = %entry.id, %reason, "spec policy blocked merge");
                state
                    .merge_queue
                    .update_status(&entry.id, MergeQueueEntryStatus::Failed, Some(reason))
                    .await?;
                return Ok(());
            }

            if policy.require_approved_spec {
                let spec_ref = mr.spec_ref.as_deref().unwrap();
                if let Err(reason) = crate::api::gates::verify_spec_ref(state, spec_ref).await {
                    warn!(entry_id = %entry.id, %reason, "spec approval check blocked merge");
                    state
                        .merge_queue
                        .update_status(
                            &entry.id,
                            MergeQueueEntryStatus::Failed,
                            Some(format!("spec approval required: {reason}")),
                        )
                        .await?;
                    return Ok(());
                }
            }
        }

        // Check if the spec_ref SHA is current (warn_stale_spec / require_current_spec).
        if policy.warn_stale_spec || policy.require_current_spec {
            if let Some(spec_ref) = mr.spec_ref.as_deref() {
                // Parse "path@sha" — same format used by verify_spec_ref.
                if let Some((path, sha)) = spec_ref.rsplit_once('@') {
                    let current = crate::git_refs::resolve_blob_sha(&repo.path, path).await;
                    let is_stale = match &current {
                        // If the file can't be resolved (new/empty repo), treat as non-stale.
                        Some(cur) => cur != sha,
                        None => false,
                    };
                    if is_stale {
                        let current_sha = current.unwrap_or_default();
                        if policy.require_current_spec {
                            let reason = format!(
                                "spec policy requires current spec: '{path}' HEAD is {current_sha} but MR references {sha}"
                            );
                            warn!(entry_id = %entry.id, %reason, "stale spec blocked merge");
                            state
                                .merge_queue
                                .update_status(
                                    &entry.id,
                                    MergeQueueEntryStatus::Failed,
                                    Some(reason),
                                )
                                .await?;
                            return Ok(());
                        } else {
                            // warn_stale_spec only — emit domain event, don't block.
                            warn!(
                                entry_id = %entry.id,
                                mr_id = %mr.id,
                                spec_path = %path,
                                spec_sha = %sha,
                                %current_sha,
                                "stale spec_ref detected (warn only)"
                            );
                            state
                                .emit_event(
                                    Some(mr.workspace_id.clone()),
                                    gyre_common::message::Destination::Workspace(
                                        mr.workspace_id.clone(),
                                    ),
                                    gyre_common::message::MessageKind::StaleSpecWarning,
                                    Some(serde_json::json!({
                                        "mr_id": mr.id.to_string(),
                                        "repo_id": repo.id.to_string(),
                                        "spec_path": path,
                                        "spec_sha": sha,
                                        "current_sha": current_sha,
                                    })),
                                )
                                .await;
                        }
                    }
                }
            }
        }
    }

    // TASK-020: Check breaking change enforcement policy.
    // If the MR's repo has unacknowledged breaking changes and the workspace policy
    // is `block`, reject the merge until all dependent repos acknowledge.
    {
        let breaking_changes = state
            .breaking_changes
            .list_by_source_repo(&mr.repository_id)
            .await
            .unwrap_or_default();

        let unacknowledged: Vec<_> = breaking_changes
            .iter()
            .filter(|bc| !bc.acknowledged)
            .collect();

        if !unacknowledged.is_empty() {
            let dep_policy = state
                .dependency_policies
                .get_for_workspace(&mr.workspace_id)
                .await
                .unwrap_or_default();

            match dep_policy.breaking_change_behavior {
                gyre_domain::BreakingChangeBehavior::Block => {
                    let count = unacknowledged.len();
                    let reason = format!(
                        "dependency policy requires acknowledgment: {count} unacknowledged \
                         breaking change(s) from this repo"
                    );
                    warn!(entry_id = %entry.id, %reason, "breaking change policy blocked merge");
                    state
                        .merge_queue
                        .update_status(&entry.id, MergeQueueEntryStatus::Failed, Some(reason))
                        .await?;
                    return Ok(());
                }
                gyre_domain::BreakingChangeBehavior::Warn => {
                    let count = unacknowledged.len();
                    warn!(
                        entry_id = %entry.id,
                        mr_id = %mr.id,
                        unacknowledged_count = count,
                        "breaking change detected (warn only, proceeding with merge)"
                    );
                }
                gyre_domain::BreakingChangeBehavior::Notify => {
                    // Notify only — no merge blocking or warnings.
                }
            }
        }
    }

    // TASK-019: Check spec link merge gates.
    // spec-links.md §Merge Gates: when an MR references a spec, the forge checks
    // that spec's links for supersession, conflicts, and unimplemented dependencies.
    if let Some(spec_ref) = mr.spec_ref.as_deref() {
        // Parse "path@sha" to get the spec path.
        let spec_path = spec_ref
            .rsplit_once('@')
            .map(|(p, _)| p)
            .unwrap_or(spec_ref);

        // Collect links for the referenced spec without holding lock across awaits.
        let relevant_links: Vec<crate::spec_registry::SpecLinkEntry> = {
            let links = state.spec_links_store.lock().await;
            links
                .iter()
                .filter(|l| l.source_path == spec_path || l.target_path == spec_path)
                .cloned()
                .collect()
        };

        // Check 1: If the spec is superseded (has an inbound `supersedes` link
        // from another spec), reject the merge.
        for link in &relevant_links {
            if link.link_type == crate::spec_registry::SpecLinkType::Supersedes
                && link.target_path == spec_path
            {
                let reason = format!(
                    "spec '{}' has been superseded by '{}'. Update your spec_ref.",
                    spec_path, link.source_path
                );
                warn!(entry_id = %entry.id, %reason, "spec link merge gate blocked merge");
                state
                    .merge_queue
                    .update_status(&entry.id, MergeQueueEntryStatus::Failed, Some(reason))
                    .await?;
                return Ok(());
            }
        }

        // Check 2: If the spec has a `conflicts_with` link to an approved spec,
        // block the merge. conflicts_with is bidirectional (spec-links.md §Cycle Detection),
        // so check both source_path and target_path matching the referenced spec.
        for link in &relevant_links {
            if link.link_type == crate::spec_registry::SpecLinkType::ConflictsWith
                && (link.source_path == spec_path || link.target_path == spec_path)
            {
                let other_spec_path = if link.source_path == spec_path {
                    &link.target_path
                } else {
                    &link.source_path
                };
                if let Ok(Some(conflicting)) = state.spec_ledger.find_by_path(other_spec_path).await
                {
                    if conflicting.approval_status == crate::spec_registry::ApprovalStatus::Approved
                    {
                        let reason = format!(
                            "spec '{}' conflicts with approved spec '{}' — resolve the conflict first",
                            spec_path, other_spec_path
                        );
                        warn!(entry_id = %entry.id, %reason, "spec link merge gate blocked merge");
                        state
                            .merge_queue
                            .update_status(&entry.id, MergeQueueEntryStatus::Failed, Some(reason))
                            .await?;
                        return Ok(());
                    }
                }
            }
        }

        // Check 3: If the spec has `depends_on` links where the target spec's
        // implementation is incomplete, add a warning (not a block).
        for link in &relevant_links {
            if link.link_type == crate::spec_registry::SpecLinkType::DependsOn
                && link.source_path == spec_path
            {
                if let Ok(Some(target)) = state.spec_ledger.find_by_path(&link.target_path).await {
                    if target.approval_status != crate::spec_registry::ApprovalStatus::Approved {
                        warn!(
                            entry_id = %entry.id,
                            mr_id = %mr.id,
                            spec_path = %spec_path,
                            dependency = %link.target_path,
                            dep_status = %target.approval_status,
                            "spec dependency not yet implemented (warning only, not blocking merge)"
                        );
                        state
                            .emit_event(
                                Some(mr.workspace_id.clone()),
                                gyre_common::message::Destination::Workspace(
                                    mr.workspace_id.clone(),
                                ),
                                gyre_common::message::MessageKind::StaleSpecWarning,
                                Some(serde_json::json!({
                                    "mr_id": mr.id.to_string(),
                                    "spec_path": spec_path,
                                    "dependency": link.target_path,
                                    "dependency_status": target.approval_status.to_string(),
                                    "warning": "dependency spec implementation not yet complete",
                                })),
                            )
                            .await;
                    }
                }
            }
        }
    }

    // Note: Quality gates (step 4b) are already verified in the selection
    // loop above — no need to re-check here.

    // Phase 3 (TASK-008): Constraint enforcement — block merge if attestation
    // chain is invalid or any constraint fails. Evaluated before the merge so we
    // can fail the queue entry without leaving a dangling merge commit.
    //
    // We use the source branch HEAD as the "merge commit" for diff computation
    // since the actual merge commit doesn't exist yet.
    if let Some(source_sha) =
        crate::git_refs::resolve_ref(&repo.path, &format!("refs/heads/{}", mr.source_branch)).await
    {
        if let Err(reason) = crate::constraint_check::enforce_merge_constraints(
            state,
            &mr.id.to_string(),
            &repo.id.to_string(),
            &repo.path,
            &source_sha,
            &mr.workspace_id,
            &mr.source_branch,
            &mr.target_branch,
            &repo.default_branch,
        )
        .await
        {
            warn!(
                entry_id = %entry.id,
                mr_id = %mr.id,
                %reason,
                "constraint enforcement blocked merge"
            );
            state
                .merge_queue
                .update_status(&entry.id, MergeQueueEntryStatus::Failed, Some(reason))
                .await?;
            return Ok(());
        }
    }

    // Attempt the merge
    let result = state
        .git_ops
        .merge_branches(&repo.path, &mr.source_branch, &mr.target_branch)
        .await;

    match result {
        Ok(MergeResult::Success { merge_commit_sha }) => {
            info!(entry_id = %entry.id, sha = %merge_commit_sha, "merge succeeded");

            // Update MR status to Merged
            let mut updated_mr = mr;
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            updated_mr.updated_at = now;
            // Transition: Approved -> Merged. If already Open, transition to Approved first.
            if updated_mr.status == MrStatus::Open {
                let _ = updated_mr.transition_status(MrStatus::Approved);
            }
            if let Err(e) = updated_mr.transition_status(MrStatus::Merged) {
                warn!("could not transition MR to Merged: {e}");
            }
            let _ = state.merge_requests.update(&updated_mr).await;

            state
                .merge_queue
                .update_status(&entry.id, MergeQueueEntryStatus::Merged, None)
                .await?;

            // Auto-track merge_queue.processed analytics event
            let ev = AnalyticsEvent::new(
                Id::new(Uuid::new_v4().to_string()),
                "merge_queue.processed",
                updated_mr.author_agent_id.as_ref().map(|id| id.to_string()),
                serde_json::json!({
                    "entry_id": entry.id.to_string(),
                    "mr_id": updated_mr.id.to_string(),
                    "result": "merged",
                }),
                now,
            );
            let _ = state.analytics.record(&ev).await;

            // Build and store a signed merge attestation bundle (G5).
            let gate_results_snapshot = state
                .gate_results
                .list_by_mr_id(updated_mr.id.as_str())
                .await
                .unwrap_or_default()
                .into_iter()
                .map(|r| crate::attestation::AttestationGateResult {
                    gate_id: r.gate_id.to_string(),
                    gate_type: String::new(), // gate type looked up below
                    status: format!("{:?}", r.status),
                    output: r.output.clone(),
                })
                .collect::<Vec<_>>();

            // Enrich gate_type from quality_gates.
            let mut gate_results_enriched = Vec::with_capacity(gate_results_snapshot.len());
            for mut gr in gate_results_snapshot {
                if let Ok(Some(gate)) = state.quality_gates.find_by_id(&gr.gate_id).await {
                    gr.gate_type = format!("{:?}", gate.gate_type);
                }
                gate_results_enriched.push(gr);
            }

            // Check spec approval status.
            let spec_fully_approved = if let Some(spec_ref) = updated_mr.spec_ref.as_deref() {
                crate::api::gates::verify_spec_ref(state, spec_ref)
                    .await
                    .is_ok()
            } else {
                true // no spec bound — treat as approved
            };

            // Look up conversation SHA uploaded by the authoring agent (HSI §5).
            let conversation_sha = if let Some(ref agent_id) = updated_mr.author_agent_id {
                let kv_key = format!("conv_sha:{}", agent_id.as_str());
                state
                    .kv_store
                    .kv_get("agent_provenance", &kv_key)
                    .await
                    .unwrap_or(None)
            } else {
                None
            };

            let attestation = crate::attestation::MergeAttestation {
                attestation_version: 1,
                mr_id: updated_mr.id.to_string(),
                merge_commit_sha: merge_commit_sha.clone(),
                merged_at: now,
                gate_results: gate_results_enriched,
                spec_ref: updated_mr.spec_ref.clone(),
                spec_fully_approved,
                author_agent_id: updated_mr.author_agent_id.as_ref().map(|id| id.to_string()),
                conversation_sha,
                // completion_summary is populated from the AgentCompleted message payload
                // when the agent calls agent.complete with a summary. At merge time, the
                // processor does not re-fetch the summary — it is stored at agent.complete time.
                completion_summary: None,
                meta_specs_used: vec![],
            };

            let bundle =
                crate::attestation::sign_attestation(attestation, &state.agent_signing_key);

            // Persist attestation bundle in the attestation store (always).
            let _ = state
                .attestation_store
                .save(&updated_mr.id.to_string(), &bundle)
                .await;
            info!(mr_id = %updated_mr.id, sha = %merge_commit_sha, "attestation bundle created and stored");

            // §5.3 location 2 — Git note on refs/notes/attestations.
            // During the dual-write period, we write EITHER the chain attestation
            // OR the legacy AttestationBundle — never both — to avoid a race
            // condition between two fire-and-forget tokio::spawn writes to the
            // same ref. The chain attestation takes priority when available.
            let chain_note_written = {
                let mut written = false;
                if let Ok(Some(chain_att)) = state
                    .chain_attestations
                    .find_by_commit(&merge_commit_sha)
                    .await
                {
                    // Load the full chain (root to leaf) from the database.
                    let chain = match state.chain_attestations.load_chain(&chain_att.id).await {
                        Ok(c) if !c.is_empty() => c,
                        _ => vec![chain_att],
                    };
                    crate::attestation::attach_chain_attestation_note(
                        &repo.path,
                        &merge_commit_sha,
                        &chain,
                    )
                    .await;
                    written = true;
                } else if let Some(ref agent_id) = updated_mr.author_agent_id {
                    // Fallback: look up by agent's task if no commit-indexed attestation.
                    if let Ok(Some(agent)) = state.agents.find_by_id(agent_id).await {
                        if let Some(ref task_id) = agent.current_task_id {
                            if let Ok(atts) = state
                                .chain_attestations
                                .find_by_task(task_id.as_str())
                                .await
                            {
                                if let Some(leaf) =
                                    atts.iter().max_by_key(|a| a.metadata.chain_depth)
                                {
                                    // Load the full chain from the leaf.
                                    let chain =
                                        match state.chain_attestations.load_chain(&leaf.id).await {
                                            Ok(c) if !c.is_empty() => c,
                                            _ => vec![leaf.clone()],
                                        };
                                    crate::attestation::attach_chain_attestation_note(
                                        &repo.path,
                                        &merge_commit_sha,
                                        &chain,
                                    )
                                    .await;
                                    written = true;
                                }
                            }
                        }
                    }
                }
                written
            };

            // Legacy fallback: write the AttestationBundle as a git note only
            // when no chain attestation was written, avoiding a race on the
            // same refs/notes/attestations ref.
            if !chain_note_written {
                let bundle_json = serde_json::to_string(&bundle).unwrap_or_default();
                let repo_path = repo.path.clone();
                let sha_for_note = merge_commit_sha.clone();
                tokio::spawn(async move {
                    let out = tokio::process::Command::new("git")
                        .args([
                            "-C",
                            &repo_path,
                            "notes",
                            "--ref=refs/notes/attestations",
                            "add",
                            "-f",
                            "-m",
                            &bundle_json,
                            &sha_for_note,
                        ])
                        .output()
                        .await;
                    match out {
                        Ok(o) if o.status.success() => {
                            tracing::info!(sha = %sha_for_note, "legacy attestation note attached");
                        }
                        Ok(o) => {
                            tracing::warn!(
                                sha = %sha_for_note,
                                stderr = %String::from_utf8_lossy(&o.stderr),
                                "git notes failed — attestation stored in memory only"
                            );
                        }
                        Err(e) => {
                            tracing::warn!(sha = %sha_for_note, error = %e, "git not found — attestation stored in memory only");
                        }
                    }
                });
            }

            // TASK-007 (Phase 2): Merge-time constraint evaluation (audit-only).
            // Collect all constraints (explicit + strategy-implied + gate),
            // evaluate against the merged diff, log results, emit violations.
            // Does NOT block the merge.
            crate::constraint_check::evaluate_merge_constraints(
                state,
                &updated_mr.id.to_string(),
                &repo.id.to_string(),
                &repo.path,
                &merge_commit_sha,
                &updated_mr.workspace_id,
                &updated_mr.source_branch,
                &updated_mr.target_branch,
                &repo.default_branch,
            )
            .await;

            // Notify the MR author that their MR was merged (HSI §2).
            if let Some(ref author_id) = updated_mr.author_agent_id {
                crate::notifications::notify_mr_merged(
                    state,
                    author_id,
                    &updated_mr.workspace_id,
                    &updated_mr.id.to_string(),
                    "default",
                )
                .await;
            }

            // TASK-022: Cascade testing — trigger test tasks in dependent repos.
            trigger_cascade_tests(
                state,
                &repo.id.to_string(),
                &repo.name,
                &merge_commit_sha,
                &updated_mr.workspace_id,
            )
            .await;
        }
        Ok(MergeResult::Conflict { message }) => {
            warn!(entry_id = %entry.id, reason = %message, "merge conflict");
            state
                .merge_queue
                .update_status(
                    &entry.id,
                    MergeQueueEntryStatus::Failed,
                    Some(format!("conflict: {}", message)),
                )
                .await?;
        }
        Err(e) => {
            error!(entry_id = %entry.id, error = %e, "git merge error");
            state
                .merge_queue
                .update_status(
                    &entry.id,
                    MergeQueueEntryStatus::Failed,
                    Some(format!("git error: {e}")),
                )
                .await?;
        }
    }

    Ok(())
}

/// Trigger cascade tests for all repos that depend on the merged repo.
///
/// When `require_cascade_tests` is enabled in the workspace dependency policy
/// and a merge completes in `merged_repo_id`, this function:
/// 1. Queries the dependency graph for all repos depending on the merged repo
/// 2. Creates a cascade test task in each dependent repo
/// 3. Emits a `cascade_test_triggered` event to the dependent repo's workspace
/// 4. Notifies workspace members of the cascade test
pub(crate) async fn trigger_cascade_tests(
    state: &AppState,
    merged_repo_id: &str,
    merged_repo_name: &str,
    merge_commit_sha: &str,
    workspace_id: &Id,
) {
    // Check workspace dependency policy.
    let policy = state
        .dependency_policies
        .get_for_workspace(workspace_id)
        .await
        .unwrap_or_default();

    if !policy.require_cascade_tests {
        return;
    }

    // Query dependency graph for all repos depending on the merged repo.
    let dependents = match state
        .dependencies
        .list_dependents(&Id::new(merged_repo_id))
        .await
    {
        Ok(deps) => deps,
        Err(e) => {
            warn!("cascade-test: failed to list dependents for {merged_repo_id}: {e}");
            return;
        }
    };

    if dependents.is_empty() {
        return;
    }

    let now = crate::api::now_secs();
    let sha_short = &merge_commit_sha[..8.min(merge_commit_sha.len())];

    for dep_edge in &dependents {
        // Resolve the dependent repo's workspace — the task belongs to
        // the dependent repo's workspace, not the merged repo's.
        let dep_repo = state
            .repos
            .find_by_id(&dep_edge.source_repo_id)
            .await
            .ok()
            .flatten();

        let dep_workspace_id = dep_repo
            .as_ref()
            .map(|r| r.workspace_id.clone())
            .unwrap_or_else(|| workspace_id.clone());

        let dep_repo_name = dep_repo
            .as_ref()
            .map(|r| r.name.clone())
            .unwrap_or_else(|| dep_edge.source_repo_id.to_string());

        // Check the dependent workspace's policy — respect opt-out (checklist #76).
        let dep_policy = state
            .dependency_policies
            .get_for_workspace(&dep_workspace_id)
            .await
            .unwrap_or_default();

        if !dep_policy.require_cascade_tests {
            info!(
                dependent_repo = dep_edge.source_repo_id.as_str(),
                dependent_workspace = dep_workspace_id.as_str(),
                "skipping cascade test — dependent workspace has cascade testing disabled"
            );
            continue;
        }

        // Create a cascade test task in the dependent repo.
        let task_id = Id::new(Uuid::new_v4().to_string());
        let mut task = gyre_domain::Task::new(
            task_id,
            format!("Cascade test: {dep_repo_name} against {merged_repo_name}@{sha_short}"),
            now,
        );
        task.priority = TaskPriority::High;
        task.labels = vec!["cascade-test".to_string(), "auto-created".to_string()];
        task.description = Some(format!(
            "Repo '{merged_repo_name}' merged commit {merge_commit_sha}. \
             Run tests in '{dep_repo_name}' against the new version to verify compatibility."
        ));
        task.workspace_id = dep_workspace_id.clone();
        task.repo_id = dep_edge.source_repo_id.clone();

        if let Err(e) = state.tasks.create(&task).await {
            warn!("cascade-test: failed to create task for {dep_repo_name}: {e}");
        }

        // Emit a cascade_test_triggered event to the dependent repo's workspace.
        let payload = serde_json::json!({
            "event": "cascade_test_triggered",
            "merged_repo_id": merged_repo_id,
            "merged_repo_name": merged_repo_name,
            "merge_commit_sha": merge_commit_sha,
            "dependent_repo_id": dep_edge.source_repo_id.as_str(),
            "dependent_repo_name": dep_repo_name,
            "task_id": task.id.as_str(),
        });

        state
            .emit_event(
                Some(dep_workspace_id.clone()),
                gyre_common::message::Destination::Workspace(dep_workspace_id.clone()),
                gyre_common::MessageKind::Custom("cascade_test_triggered".to_string()),
                Some(payload),
            )
            .await;

        // Notify workspace members of the dependent repo.
        let members = state
            .workspace_memberships
            .list_by_workspace(&dep_workspace_id)
            .await
            .unwrap_or_default();

        for member in &members {
            let body_json = serde_json::json!({
                "merged_repo": merged_repo_name,
                "merge_commit_sha": merge_commit_sha,
                "dependent_repo": dep_repo_name,
                "task_id": task.id.as_str(),
            })
            .to_string();

            crate::notifications::notify_rich(
                state,
                dep_workspace_id.clone(),
                member.user_id.clone(),
                gyre_common::NotificationType::CascadeTestTriggered,
                format!(
                    "Cascade test triggered: {dep_repo_name} against {merged_repo_name}@{sha_short}"
                ),
                "default",
                Some(body_json),
                Some(dep_edge.source_repo_id.to_string()),
                Some(merged_repo_id.to_string()),
            )
            .await;
        }

        info!(
            merged_repo = merged_repo_id,
            dependent_repo = dep_edge.source_repo_id.as_str(),
            "cascade test task created for dependent repo"
        );
    }

    info!(
        merged_repo = merged_repo_id,
        dependent_count = dependents.len(),
        "cascade testing triggered for {} dependent repo(s)",
        dependents.len(),
    );
}

/// Report the result of a cascade test and handle failures.
///
/// Called when a cascade test task completes. On failure:
/// 1. Creates a follow-up task in the dependent repo with failure details
/// 2. Emits a `cascade_test_failed` event to the workspace orchestrator
/// 3. Sends CascadeTestFailed notifications to workspace members
///
/// On success, emits a `cascade_test_passed` activity event.
pub(crate) async fn report_cascade_test_result(
    state: &AppState,
    task: &gyre_domain::Task,
    passed: bool,
    failure_details: Option<&str>,
) {
    let now = crate::api::now_secs();

    if passed {
        // Emit cascade_test_passed activity event.
        state
            .emit_event(
                Some(task.workspace_id.clone()),
                gyre_common::message::Destination::Workspace(task.workspace_id.clone()),
                gyre_common::MessageKind::Custom("cascade_test_passed".to_string()),
                Some(serde_json::json!({
                    "event": "cascade_test_passed",
                    "task_id": task.id.as_str(),
                    "repo_id": task.repo_id.as_str(),
                    "title": &task.title,
                })),
            )
            .await;

        info!(
            task_id = task.id.as_str(),
            repo_id = task.repo_id.as_str(),
            "cascade test passed"
        );
    } else {
        let details = failure_details.unwrap_or("Cascade test failed — see task for details");

        // Create a follow-up task in the dependent repo with failure details.
        let follow_up_id = Id::new(Uuid::new_v4().to_string());
        let mut follow_up = gyre_domain::Task::new(
            follow_up_id,
            format!("Cascade test failure: {}", task.title),
            now,
        );
        follow_up.priority = TaskPriority::High;
        follow_up.labels = vec![
            "cascade-test-failure".to_string(),
            "auto-created".to_string(),
        ];
        follow_up.description = Some(format!(
            "Cascade test failed. Original task: {}. Details: {}",
            task.title, details
        ));
        follow_up.workspace_id = task.workspace_id.clone();
        follow_up.repo_id = task.repo_id.clone();

        if let Err(e) = state.tasks.create(&follow_up).await {
            warn!("cascade-test: failed to create follow-up task: {e}");
        }

        // Emit cascade_test_failed event to workspace orchestrator.
        let payload = serde_json::json!({
            "event": "cascade_test_failed",
            "task_id": task.id.as_str(),
            "follow_up_task_id": follow_up.id.as_str(),
            "repo_id": task.repo_id.as_str(),
            "title": &task.title,
            "failure_details": details,
        });

        state
            .emit_event(
                Some(task.workspace_id.clone()),
                gyre_common::message::Destination::Workspace(task.workspace_id.clone()),
                gyre_common::MessageKind::Custom("cascade_test_failed".to_string()),
                Some(payload),
            )
            .await;

        // Notify workspace members of cascade test failure.
        let members = state
            .workspace_memberships
            .list_by_workspace(&task.workspace_id)
            .await
            .unwrap_or_default();

        for member in &members {
            let body_json = serde_json::json!({
                "task_id": task.id.as_str(),
                "follow_up_task_id": follow_up.id.as_str(),
                "repo_id": task.repo_id.as_str(),
                "failure_details": details,
            })
            .to_string();

            crate::notifications::notify_rich(
                state,
                task.workspace_id.clone(),
                member.user_id.clone(),
                gyre_common::NotificationType::CascadeTestFailed,
                format!("Cascade test failed: {}", task.title),
                "default",
                Some(body_json),
                Some(task.repo_id.to_string()),
                None,
            )
            .await;
        }

        info!(
            task_id = task.id.as_str(),
            repo_id = task.repo_id.as_str(),
            "cascade test failed, follow-up task created"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mem::test_state;
    use gyre_common::Id;
    use gyre_domain::{
        DependencyEdge, DependencyPolicy, DependencyType, DetectionMethod, Repository,
    };

    async fn create_repo_in_workspace(
        state: &AppState,
        name: &str,
        workspace_id: &str,
    ) -> Repository {
        let repo = Repository::new(
            Id::new(uuid::Uuid::new_v4().to_string()),
            Id::new(workspace_id),
            name,
            format!("/tmp/{name}.git"),
            0,
        );
        state.repos.create(&repo).await.unwrap();
        repo
    }

    async fn create_dependency(state: &AppState, source_repo_id: &Id, target_repo_id: &Id) {
        let edge = DependencyEdge::new(
            Id::new(uuid::Uuid::new_v4().to_string()),
            source_repo_id.clone(),
            target_repo_id.clone(),
            DependencyType::Code,
            "Cargo.toml",
            "target-crate",
            DetectionMethod::CargoToml,
            1000,
        );
        state.dependencies.save(&edge).await.unwrap();
    }

    #[tokio::test]
    async fn trigger_cascade_tests_creates_tasks_for_dependents() {
        let state = test_state();

        let repo_b = create_repo_in_workspace(&state, "repo-b", "ws-1").await;
        let repo_a = create_repo_in_workspace(&state, "repo-a", "ws-1").await;

        // repo-a depends on repo-b
        create_dependency(&state, &repo_a.id, &repo_b.id).await;

        // Enable cascade testing
        let policy = DependencyPolicy {
            require_cascade_tests: true,
            ..Default::default()
        };
        state
            .dependency_policies
            .set_for_workspace(&Id::new("ws-1"), &policy)
            .await
            .unwrap();

        // Subscribe to broadcast channel before calling (F1).
        let mut rx = state.message_broadcast_tx.subscribe();

        trigger_cascade_tests(
            &state,
            repo_b.id.as_str(),
            "repo-b",
            "abc123def456",
            &Id::new("ws-1"),
        )
        .await;

        // Verify a cascade test task was created in repo-a
        let tasks = state.tasks.list_by_repo(&repo_a.id).await.unwrap();
        assert_eq!(tasks.len(), 1);
        assert!(tasks[0].title.contains("repo-a"));
        assert!(tasks[0].title.contains("repo-b"));
        assert!(tasks[0].title.contains("abc123de")); // sha_short
        assert_eq!(tasks[0].priority, TaskPriority::High);
        assert!(tasks[0].labels.contains(&"cascade-test".to_string()));
        assert!(tasks[0].labels.contains(&"auto-created".to_string()));
        assert_eq!(tasks[0].workspace_id.as_str(), "ws-1");
        assert_eq!(tasks[0].repo_id.as_str(), repo_a.id.as_str());

        // Verify cascade_test_triggered event was emitted (F1).
        let msg = rx
            .try_recv()
            .expect("cascade_test_triggered event should be emitted");
        assert_eq!(
            msg.kind,
            gyre_common::MessageKind::Custom("cascade_test_triggered".to_string())
        );
        let payload = msg.payload.as_ref().expect("event should have payload");
        assert_eq!(payload["event"], "cascade_test_triggered");
        assert_eq!(payload["merged_repo_id"], repo_b.id.as_str());
        assert_eq!(payload["dependent_repo_id"], repo_a.id.as_str());
    }

    #[tokio::test]
    async fn trigger_cascade_tests_skips_when_policy_disabled() {
        let state = test_state();

        let repo_b = create_repo_in_workspace(&state, "repo-b", "ws-1").await;
        let repo_a = create_repo_in_workspace(&state, "repo-a", "ws-1").await;

        create_dependency(&state, &repo_a.id, &repo_b.id).await;

        // Disable cascade testing
        let policy = DependencyPolicy {
            require_cascade_tests: false,
            ..Default::default()
        };
        state
            .dependency_policies
            .set_for_workspace(&Id::new("ws-1"), &policy)
            .await
            .unwrap();

        let mut rx = state.message_broadcast_tx.subscribe();

        trigger_cascade_tests(
            &state,
            repo_b.id.as_str(),
            "repo-b",
            "abc123def456",
            &Id::new("ws-1"),
        )
        .await;

        // No tasks should be created
        let tasks = state.tasks.list_by_repo(&repo_a.id).await.unwrap();
        assert!(tasks.is_empty());

        // No events should be emitted when policy is disabled.
        assert!(
            rx.try_recv().is_err(),
            "no events should be emitted when cascade testing is disabled"
        );
    }

    #[tokio::test]
    async fn trigger_cascade_tests_creates_task_per_dependent() {
        let state = test_state();

        let repo_b = create_repo_in_workspace(&state, "repo-b", "ws-1").await;
        let repo_a = create_repo_in_workspace(&state, "repo-a", "ws-1").await;
        let repo_c = create_repo_in_workspace(&state, "repo-c", "ws-1").await;
        let repo_d = create_repo_in_workspace(&state, "repo-d", "ws-1").await;

        // All depend on repo-b
        create_dependency(&state, &repo_a.id, &repo_b.id).await;
        create_dependency(&state, &repo_c.id, &repo_b.id).await;
        create_dependency(&state, &repo_d.id, &repo_b.id).await;

        let policy = DependencyPolicy {
            require_cascade_tests: true,
            ..Default::default()
        };
        state
            .dependency_policies
            .set_for_workspace(&Id::new("ws-1"), &policy)
            .await
            .unwrap();

        let mut rx = state.message_broadcast_tx.subscribe();

        trigger_cascade_tests(
            &state,
            repo_b.id.as_str(),
            "repo-b",
            "sha12345",
            &Id::new("ws-1"),
        )
        .await;

        // Each dependent repo gets its own task
        let tasks_a = state.tasks.list_by_repo(&repo_a.id).await.unwrap();
        let tasks_c = state.tasks.list_by_repo(&repo_c.id).await.unwrap();
        let tasks_d = state.tasks.list_by_repo(&repo_d.id).await.unwrap();

        assert_eq!(tasks_a.len(), 1);
        assert_eq!(tasks_c.len(), 1);
        assert_eq!(tasks_d.len(), 1);

        assert!(tasks_a[0].title.contains("repo-a"));
        assert!(tasks_c[0].title.contains("repo-c"));
        assert!(tasks_d[0].title.contains("repo-d"));

        // Verify one cascade_test_triggered event per dependent.
        let mut event_count = 0;
        while rx.try_recv().is_ok() {
            event_count += 1;
        }
        assert_eq!(event_count, 3, "should emit one event per dependent repo");
    }

    #[tokio::test]
    async fn trigger_cascade_tests_no_dependents_is_noop() {
        let state = test_state();

        let repo_b = create_repo_in_workspace(&state, "repo-b", "ws-1").await;

        let policy = DependencyPolicy {
            require_cascade_tests: true,
            ..Default::default()
        };
        state
            .dependency_policies
            .set_for_workspace(&Id::new("ws-1"), &policy)
            .await
            .unwrap();

        let mut rx = state.message_broadcast_tx.subscribe();

        trigger_cascade_tests(
            &state,
            repo_b.id.as_str(),
            "repo-b",
            "sha12345",
            &Id::new("ws-1"),
        )
        .await;

        // No tasks since no dependents exist
        let all_tasks = state.tasks.list().await.unwrap();
        assert!(all_tasks.is_empty());

        // No events since no dependents exist.
        assert!(
            rx.try_recv().is_err(),
            "no events should be emitted when there are no dependents"
        );
    }

    #[tokio::test]
    async fn trigger_cascade_tests_resolves_dependent_workspace() {
        let state = test_state();

        let repo_b = create_repo_in_workspace(&state, "repo-b", "ws-MERGED").await;
        let repo_a = create_repo_in_workspace(&state, "repo-a", "ws-DEPENDENT").await;

        create_dependency(&state, &repo_a.id, &repo_b.id).await;

        let policy = DependencyPolicy {
            require_cascade_tests: true,
            ..Default::default()
        };
        // Set policy for both workspaces so cascade testing is enabled on both sides.
        state
            .dependency_policies
            .set_for_workspace(&Id::new("ws-MERGED"), &policy)
            .await
            .unwrap();
        state
            .dependency_policies
            .set_for_workspace(&Id::new("ws-DEPENDENT"), &policy)
            .await
            .unwrap();

        // Subscribe to broadcast channel before calling.
        let mut rx = state.message_broadcast_tx.subscribe();

        trigger_cascade_tests(
            &state,
            repo_b.id.as_str(),
            "repo-b",
            "sha12345",
            &Id::new("ws-MERGED"),
        )
        .await;

        // Task should be in the dependent repo's workspace, not the merged repo's
        let tasks = state.tasks.list_by_repo(&repo_a.id).await.unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(
            tasks[0].workspace_id.as_str(),
            "ws-DEPENDENT",
            "task should be in the dependent repo's workspace"
        );

        // Event should target the dependent workspace, not the merged workspace.
        let msg = rx
            .try_recv()
            .expect("cascade_test_triggered event should be emitted");
        assert_eq!(
            msg.kind,
            gyre_common::MessageKind::Custom("cascade_test_triggered".to_string())
        );
        // Event destination should be the dependent workspace.
        assert_eq!(
            msg.to,
            gyre_common::message::Destination::Workspace(Id::new("ws-DEPENDENT")),
            "event should target the dependent workspace"
        );
    }

    #[tokio::test]
    async fn report_cascade_test_result_passed_no_follow_up_task() {
        let state = test_state();

        let repo_a = create_repo_in_workspace(&state, "repo-a", "ws-1").await;

        let mut task = gyre_domain::Task::new(
            Id::new("cascade-task-1"),
            "Cascade test: repo-a against repo-b@abc12345",
            1000,
        );
        task.labels = vec!["cascade-test".to_string(), "auto-created".to_string()];
        task.workspace_id = Id::new("ws-1");
        task.repo_id = repo_a.id.clone();
        state.tasks.create(&task).await.unwrap();

        // Subscribe to broadcast channel before calling (F5).
        let mut rx = state.message_broadcast_tx.subscribe();

        report_cascade_test_result(&state, &task, true, None).await;

        // No follow-up task should be created for a passing test
        let tasks = state.tasks.list_by_repo(&repo_a.id).await.unwrap();
        assert_eq!(
            tasks.len(),
            1,
            "only the original cascade test task should exist"
        );

        // Verify cascade_test_passed event was emitted (F5).
        let msg = rx
            .try_recv()
            .expect("cascade_test_passed event should be emitted");
        assert_eq!(
            msg.kind,
            gyre_common::MessageKind::Custom("cascade_test_passed".to_string())
        );
        let payload = msg.payload.as_ref().expect("event should have payload");
        assert_eq!(payload["event"], "cascade_test_passed");
        assert_eq!(payload["task_id"], "cascade-task-1");
        assert_eq!(payload["repo_id"], repo_a.id.as_str());
    }

    #[tokio::test]
    async fn report_cascade_test_result_failed_creates_follow_up_task() {
        let state = test_state();

        let repo_a = create_repo_in_workspace(&state, "repo-a", "ws-1").await;

        let mut task = gyre_domain::Task::new(
            Id::new("cascade-task-2"),
            "Cascade test: repo-a against repo-b@abc12345",
            1000,
        );
        task.labels = vec!["cascade-test".to_string(), "auto-created".to_string()];
        task.workspace_id = Id::new("ws-1");
        task.repo_id = repo_a.id.clone();
        state.tasks.create(&task).await.unwrap();

        // Subscribe to broadcast channel before calling (F1).
        let mut rx = state.message_broadcast_tx.subscribe();

        report_cascade_test_result(
            &state,
            &task,
            false,
            Some("Tests failed: 3 failures in integration tests"),
        )
        .await;

        // A follow-up task should be created
        let tasks = state.tasks.list_by_repo(&repo_a.id).await.unwrap();
        assert_eq!(tasks.len(), 2, "original + follow-up task");

        let follow_up = tasks
            .iter()
            .find(|t| t.labels.contains(&"cascade-test-failure".to_string()))
            .expect("follow-up task should have cascade-test-failure label");

        assert!(follow_up.title.contains("Cascade test failure"));
        assert_eq!(follow_up.priority, TaskPriority::High);
        assert!(follow_up.labels.contains(&"auto-created".to_string()));
        assert_eq!(follow_up.workspace_id.as_str(), "ws-1");
        assert_eq!(follow_up.repo_id.as_str(), repo_a.id.as_str());
        assert!(follow_up
            .description
            .as_ref()
            .unwrap()
            .contains("Tests failed: 3 failures"));

        // Verify cascade_test_failed event was emitted (F1).
        let msg = rx
            .try_recv()
            .expect("cascade_test_failed event should be emitted");
        assert_eq!(
            msg.kind,
            gyre_common::MessageKind::Custom("cascade_test_failed".to_string())
        );
        let payload = msg.payload.as_ref().expect("event should have payload");
        assert_eq!(payload["event"], "cascade_test_failed");
        assert_eq!(payload["task_id"], "cascade-task-2");
        assert_eq!(payload["repo_id"], repo_a.id.as_str());
        assert!(payload["failure_details"]
            .as_str()
            .unwrap()
            .contains("Tests failed: 3 failures"));
        // follow_up_task_id should be present
        assert!(
            payload["follow_up_task_id"].as_str().is_some(),
            "event should include follow_up_task_id"
        );
    }

    #[tokio::test]
    async fn report_cascade_test_failure_notifies_workspace_members() {
        let state = test_state();

        let repo_a = create_repo_in_workspace(&state, "repo-a", "ws-1").await;

        // Add a workspace member
        let membership = gyre_domain::WorkspaceMembership::new(
            Id::new("member-1"),
            Id::new("user-1"),
            Id::new("ws-1"),
            gyre_domain::WorkspaceRole::Developer,
            Id::new("admin"),
            1000,
        );
        state
            .workspace_memberships
            .create(&membership)
            .await
            .unwrap();

        let mut task = gyre_domain::Task::new(
            Id::new("cascade-task-3"),
            "Cascade test: repo-a against repo-b@abc12345",
            1000,
        );
        task.labels = vec!["cascade-test".to_string(), "auto-created".to_string()];
        task.workspace_id = Id::new("ws-1");
        task.repo_id = repo_a.id.clone();
        state.tasks.create(&task).await.unwrap();

        // Subscribe to broadcast channel before calling (F1).
        let mut rx = state.message_broadcast_tx.subscribe();

        report_cascade_test_result(&state, &task, false, Some("test failure")).await;

        // Verify notification was created for the workspace member
        let notifications = state
            .notifications
            .list_for_user(
                &Id::new("user-1"),
                Some(&Id::new("ws-1")),
                None,
                None,
                None,
                100,
                0,
            )
            .await
            .unwrap();

        assert!(
            !notifications.is_empty(),
            "workspace member should be notified"
        );
        let notif = &notifications[0];
        assert_eq!(
            notif.notification_type,
            gyre_common::NotificationType::CascadeTestFailed
        );
        assert!(notif.title.contains("Cascade test failed"));

        // Verify notification body JSON contains expected fields (F3).
        let body: serde_json::Value =
            serde_json::from_str(notif.body.as_ref().expect("notification should have body"))
                .expect("notification body should be valid JSON");
        assert_eq!(body["task_id"], "cascade-task-3");
        assert!(
            body["follow_up_task_id"].as_str().is_some(),
            "body should contain follow_up_task_id"
        );
        assert_eq!(body["repo_id"], repo_a.id.as_str());
        assert_eq!(body["failure_details"], "test failure");

        // Verify entity_ref and priority (F3).
        assert_eq!(
            notif.entity_ref.as_deref(),
            Some(repo_a.id.as_str()),
            "entity_ref should be the repo_id"
        );
        assert_eq!(
            notif.priority,
            gyre_common::NotificationType::CascadeTestFailed.default_priority(),
            "priority should match CascadeTestFailed priority"
        );

        // Verify cascade_test_failed event was also emitted (F1).
        let msg = rx
            .try_recv()
            .expect("cascade_test_failed event should be emitted");
        assert_eq!(
            msg.kind,
            gyre_common::MessageKind::Custom("cascade_test_failed".to_string())
        );
    }

    #[tokio::test]
    async fn trigger_cascade_tests_default_policy_enables_cascade() {
        let state = test_state();

        let repo_b = create_repo_in_workspace(&state, "repo-b", "ws-1").await;
        let repo_a = create_repo_in_workspace(&state, "repo-a", "ws-1").await;

        create_dependency(&state, &repo_a.id, &repo_b.id).await;

        // Default policy has require_cascade_tests: true
        // (no explicit policy set — uses default)

        let mut rx = state.message_broadcast_tx.subscribe();

        trigger_cascade_tests(
            &state,
            repo_b.id.as_str(),
            "repo-b",
            "sha12345",
            &Id::new("ws-1"),
        )
        .await;

        // Default policy enables cascade testing, so task should be created
        let tasks = state.tasks.list_by_repo(&repo_a.id).await.unwrap();
        assert_eq!(tasks.len(), 1);

        // Event should be emitted with default policy.
        let msg = rx
            .try_recv()
            .expect("event should be emitted with default policy");
        assert_eq!(
            msg.kind,
            gyre_common::MessageKind::Custom("cascade_test_triggered".to_string())
        );
    }

    /// F2: Verify that CascadeTestTriggered notifications are sent to workspace members
    /// when cascade tests are triggered.
    #[tokio::test]
    async fn trigger_cascade_tests_notifies_workspace_members() {
        let state = test_state();

        let repo_b = create_repo_in_workspace(&state, "repo-b", "ws-1").await;
        let repo_a = create_repo_in_workspace(&state, "repo-a", "ws-1").await;

        create_dependency(&state, &repo_a.id, &repo_b.id).await;

        // Add a workspace member
        let membership = gyre_domain::WorkspaceMembership::new(
            Id::new("member-1"),
            Id::new("user-1"),
            Id::new("ws-1"),
            gyre_domain::WorkspaceRole::Developer,
            Id::new("admin"),
            1000,
        );
        state
            .workspace_memberships
            .create(&membership)
            .await
            .unwrap();

        let policy = DependencyPolicy {
            require_cascade_tests: true,
            ..Default::default()
        };
        state
            .dependency_policies
            .set_for_workspace(&Id::new("ws-1"), &policy)
            .await
            .unwrap();

        let mut rx = state.message_broadcast_tx.subscribe();

        trigger_cascade_tests(
            &state,
            repo_b.id.as_str(),
            "repo-b",
            "abc123def456",
            &Id::new("ws-1"),
        )
        .await;

        // Verify cascade_test_triggered event was emitted.
        let msg = rx
            .try_recv()
            .expect("cascade_test_triggered event should be emitted");
        assert_eq!(
            msg.kind,
            gyre_common::MessageKind::Custom("cascade_test_triggered".to_string())
        );

        // Verify CascadeTestTriggered notification was sent
        let notifications = state
            .notifications
            .list_for_user(
                &Id::new("user-1"),
                Some(&Id::new("ws-1")),
                None,
                None,
                None,
                100,
                0,
            )
            .await
            .unwrap();

        assert!(
            !notifications.is_empty(),
            "workspace member should receive CascadeTestTriggered notification"
        );
        let notif = &notifications[0];
        assert_eq!(
            notif.notification_type,
            gyre_common::NotificationType::CascadeTestTriggered
        );
        assert!(notif.title.contains("Cascade test triggered"));
        assert!(notif.title.contains("repo-a"));
        assert!(notif.title.contains("repo-b"));

        // Verify notification body JSON fields
        let body: serde_json::Value =
            serde_json::from_str(notif.body.as_ref().expect("notification should have body"))
                .expect("body should be valid JSON");
        assert_eq!(body["merged_repo"], "repo-b");
        assert_eq!(body["merge_commit_sha"], "abc123def456");
        assert_eq!(body["dependent_repo"], "repo-a");
        assert!(
            body["task_id"].as_str().is_some(),
            "body should contain task_id"
        );

        // Verify entity_ref and priority
        assert_eq!(
            notif.entity_ref.as_deref(),
            Some(repo_a.id.as_str()),
            "entity_ref should be the dependent repo_id"
        );
        assert_eq!(
            notif.priority,
            gyre_common::NotificationType::CascadeTestTriggered.default_priority(),
            "priority should match CascadeTestTriggered priority"
        );
    }

    /// F4: Verify that cascade tests are skipped for dependent repos whose workspace
    /// has cascade testing disabled, even if the merged repo's workspace has it enabled.
    #[tokio::test]
    async fn trigger_cascade_tests_respects_dependent_workspace_opt_out() {
        let state = test_state();

        let repo_b = create_repo_in_workspace(&state, "repo-b", "ws-MERGED").await;
        let repo_a = create_repo_in_workspace(&state, "repo-a", "ws-DEPENDENT").await;

        create_dependency(&state, &repo_a.id, &repo_b.id).await;

        // Enable cascade testing in the merged repo's workspace
        let merged_policy = DependencyPolicy {
            require_cascade_tests: true,
            ..Default::default()
        };
        state
            .dependency_policies
            .set_for_workspace(&Id::new("ws-MERGED"), &merged_policy)
            .await
            .unwrap();

        // Disable cascade testing in the dependent repo's workspace
        let dep_policy = DependencyPolicy {
            require_cascade_tests: false,
            ..Default::default()
        };
        state
            .dependency_policies
            .set_for_workspace(&Id::new("ws-DEPENDENT"), &dep_policy)
            .await
            .unwrap();

        let mut rx = state.message_broadcast_tx.subscribe();

        trigger_cascade_tests(
            &state,
            repo_b.id.as_str(),
            "repo-b",
            "sha12345",
            &Id::new("ws-MERGED"),
        )
        .await;

        // No task should be created — dependent workspace opted out
        let tasks = state.tasks.list_by_repo(&repo_a.id).await.unwrap();
        assert!(
            tasks.is_empty(),
            "no cascade test task should be created when dependent workspace has cascade testing disabled"
        );

        // No events should be emitted when dependent workspace opts out.
        assert!(
            rx.try_recv().is_err(),
            "no events should be emitted when dependent workspace has cascade testing disabled"
        );
    }

    // ── Topological sort & priority tests (TASK-026) ─────────────────────────

    /// Create an MR with the given id, workspace, and depends_on list.
    async fn create_mr_with_deps(
        state: &AppState,
        mr_id: &str,
        workspace_id: &str,
        depends_on: Vec<&str>,
    ) -> gyre_domain::MergeRequest {
        let mut mr = gyre_domain::MergeRequest::new(
            Id::new(mr_id),
            Id::new("repo-1"),
            format!("MR {mr_id}"),
            format!("feat/{mr_id}"),
            "main",
            1000,
        );
        mr.workspace_id = Id::new(workspace_id);
        mr.depends_on = depends_on.into_iter().map(Id::new).collect();
        state.merge_requests.create(&mr).await.unwrap();
        mr
    }

    /// Enqueue an MR with a given priority and enqueued_at timestamp.
    async fn enqueue_mr(
        state: &AppState,
        mr_id: &str,
        priority: u32,
        enqueued_at: u64,
    ) -> MergeQueueEntry {
        let entry = MergeQueueEntry::new(
            Id::new(format!("entry-{mr_id}")),
            Id::new(mr_id),
            priority,
            enqueued_at,
        );
        state.merge_queue.enqueue(&entry).await.unwrap();
        entry
    }

    #[tokio::test]
    async fn topological_sort_prioritizes_independent_mrs() {
        // Two independent MRs (no deps between them): higher priority processes first.
        let state = test_state();

        create_mr_with_deps(&state, "mr-low", "ws-1", vec![]).await;
        create_mr_with_deps(&state, "mr-high", "ws-1", vec![]).await;

        let e_low = enqueue_mr(&state, "mr-low", 25, 1000).await; // Low priority
        let e_high = enqueue_mr(&state, "mr-high", 100, 1001).await; // Critical priority

        let entries = vec![e_low.clone(), e_high.clone()];
        let graph = build_queue_dependency_graph(&state, &entries)
            .await
            .unwrap();
        let sorted = topological_sort_with_priority(&entries, &graph);

        assert_eq!(sorted.len(), 2);
        assert_eq!(
            sorted[0].merge_request_id.as_str(),
            "mr-high",
            "higher priority MR should be first"
        );
        assert_eq!(
            sorted[1].merge_request_id.as_str(),
            "mr-low",
            "lower priority MR should be second"
        );
    }

    #[tokio::test]
    async fn topological_sort_respects_dependencies_over_priority() {
        // mr-high (priority 100) depends on mr-low (priority 25).
        // mr-low must come first despite lower priority.
        let state = test_state();

        create_mr_with_deps(&state, "mr-low", "ws-1", vec![]).await;
        create_mr_with_deps(&state, "mr-high", "ws-1", vec!["mr-low"]).await;

        let e_low = enqueue_mr(&state, "mr-low", 25, 1000).await;
        let e_high = enqueue_mr(&state, "mr-high", 100, 1001).await;

        let entries = vec![e_high.clone(), e_low.clone()]; // intentional reverse order
        let graph = build_queue_dependency_graph(&state, &entries)
            .await
            .unwrap();
        let sorted = topological_sort_with_priority(&entries, &graph);

        assert_eq!(sorted.len(), 2);
        assert_eq!(
            sorted[0].merge_request_id.as_str(),
            "mr-low",
            "dependency must come first even with lower priority"
        );
        assert_eq!(
            sorted[1].merge_request_id.as_str(),
            "mr-high",
            "dependent MR comes after its dependency"
        );
    }

    #[tokio::test]
    async fn topological_sort_priority_within_same_tier() {
        // Diamond: mr-a depends on mr-root, mr-b depends on mr-root.
        // mr-a has higher priority than mr-b.
        // Expected: mr-root first, then mr-a, then mr-b.
        let state = test_state();

        create_mr_with_deps(&state, "mr-root", "ws-1", vec![]).await;
        create_mr_with_deps(&state, "mr-a", "ws-1", vec!["mr-root"]).await;
        create_mr_with_deps(&state, "mr-b", "ws-1", vec!["mr-root"]).await;

        let e_root = enqueue_mr(&state, "mr-root", 50, 1000).await;
        let e_a = enqueue_mr(&state, "mr-a", 100, 1001).await;
        let e_b = enqueue_mr(&state, "mr-b", 25, 1002).await;

        let entries = vec![e_b.clone(), e_root.clone(), e_a.clone()];
        let graph = build_queue_dependency_graph(&state, &entries)
            .await
            .unwrap();
        let sorted = topological_sort_with_priority(&entries, &graph);

        assert_eq!(sorted.len(), 3);
        assert_eq!(sorted[0].merge_request_id.as_str(), "mr-root");
        assert_eq!(
            sorted[1].merge_request_id.as_str(),
            "mr-a",
            "higher priority independent MR in tier 2 should be first"
        );
        assert_eq!(
            sorted[2].merge_request_id.as_str(),
            "mr-b",
            "lower priority independent MR in tier 2 should be second"
        );
    }

    #[tokio::test]
    async fn topological_sort_chain_ordering() {
        // Linear chain: mr-c depends on mr-b, mr-b depends on mr-a.
        // Even if mr-c has highest priority, order must be: a, b, c.
        let state = test_state();

        create_mr_with_deps(&state, "mr-a", "ws-1", vec![]).await;
        create_mr_with_deps(&state, "mr-b", "ws-1", vec!["mr-a"]).await;
        create_mr_with_deps(&state, "mr-c", "ws-1", vec!["mr-b"]).await;

        let e_a = enqueue_mr(&state, "mr-a", 25, 1000).await;
        let e_b = enqueue_mr(&state, "mr-b", 50, 1001).await;
        let e_c = enqueue_mr(&state, "mr-c", 100, 1002).await;

        let entries = vec![e_c.clone(), e_a.clone(), e_b.clone()];
        let graph = build_queue_dependency_graph(&state, &entries)
            .await
            .unwrap();
        let sorted = topological_sort_with_priority(&entries, &graph);

        assert_eq!(sorted.len(), 3);
        assert_eq!(sorted[0].merge_request_id.as_str(), "mr-a");
        assert_eq!(sorted[1].merge_request_id.as_str(), "mr-b");
        assert_eq!(sorted[2].merge_request_id.as_str(), "mr-c");
    }

    #[tokio::test]
    async fn topological_sort_atomic_group_ordering() {
        // mr-first and mr-second are in atomic group "bundle".
        // mr-first enqueued before mr-second.
        // Atomic group implies: mr-second depends on mr-first.
        let state = test_state();

        let mut mr_first = gyre_domain::MergeRequest::new(
            Id::new("mr-first"),
            Id::new("repo-1"),
            "First in group",
            "feat/first",
            "main",
            1000,
        );
        mr_first.workspace_id = Id::new("ws-1");
        mr_first.atomic_group = Some("bundle".to_string());
        state.merge_requests.create(&mr_first).await.unwrap();

        let mut mr_second = gyre_domain::MergeRequest::new(
            Id::new("mr-second"),
            Id::new("repo-1"),
            "Second in group",
            "feat/second",
            "main",
            1000,
        );
        mr_second.workspace_id = Id::new("ws-1");
        mr_second.atomic_group = Some("bundle".to_string());
        state.merge_requests.create(&mr_second).await.unwrap();

        // mr-second has higher priority but must come after mr-first (enqueue order)
        let e_first = enqueue_mr(&state, "mr-first", 25, 1000).await;
        let e_second = enqueue_mr(&state, "mr-second", 100, 1001).await;

        let entries = vec![e_second.clone(), e_first.clone()];
        let graph = build_queue_dependency_graph(&state, &entries)
            .await
            .unwrap();
        let sorted = topological_sort_with_priority(&entries, &graph);

        assert_eq!(sorted.len(), 2);
        assert_eq!(
            sorted[0].merge_request_id.as_str(),
            "mr-first",
            "earlier-enqueued atomic group member must come first"
        );
        assert_eq!(
            sorted[1].merge_request_id.as_str(),
            "mr-second",
            "later-enqueued atomic group member must come second"
        );
    }

    #[tokio::test]
    async fn chain_depth_warning_emitted_at_depth_exceeding_10() {
        // Build a chain of 12 MRs: mr-0 <- mr-1 <- ... <- mr-11.
        // Chain depth = 11 levels, which exceeds the >10 threshold.
        let state = test_state();

        // Add a workspace member for notification delivery.
        let membership = gyre_domain::WorkspaceMembership::new(
            Id::new("member-1"),
            Id::new("user-1"),
            Id::new("ws-1"),
            gyre_domain::WorkspaceRole::Developer,
            Id::new("admin"),
            1000,
        );
        state
            .workspace_memberships
            .create(&membership)
            .await
            .unwrap();

        let mut entries = Vec::new();
        for i in 0..12 {
            let mr_id = format!("mr-{i}");
            let deps = if i == 0 {
                vec![]
            } else {
                vec![format!("mr-{}", i - 1)]
            };
            let mut mr = gyre_domain::MergeRequest::new(
                Id::new(&mr_id),
                Id::new("repo-1"),
                format!("MR {mr_id}"),
                format!("feat/{mr_id}"),
                "main",
                1000,
            );
            mr.workspace_id = Id::new("ws-1");
            mr.depends_on = deps.into_iter().map(Id::new).collect();
            state.merge_requests.create(&mr).await.unwrap();

            let entry = enqueue_mr(&state, &mr_id, 50, 1000 + i as u64).await;
            entries.push(entry);
        }

        let graph = build_queue_dependency_graph(&state, &entries)
            .await
            .unwrap();
        let max_depth = compute_max_chain_depth(&graph);
        assert_eq!(max_depth, 11, "chain of 12 MRs has depth 11");
        assert!(max_depth > 10, "should exceed the >10 threshold");

        // Actually trigger the warning via process_next — which will call warn_deep_chain.
        // Instead of calling process_next (which would try to merge),
        // call warn_deep_chain directly.
        warn_deep_chain(&state, max_depth, &Id::new("ws-1")).await;

        // Verify notification was created.
        let notifications = state
            .notifications
            .list_for_user(
                &Id::new("user-1"),
                Some(&Id::new("ws-1")),
                None,
                None,
                None,
                100,
                0,
            )
            .await
            .unwrap();

        assert_eq!(notifications.len(), 1);
        let notif = &notifications[0];
        assert_eq!(
            notif.notification_type,
            gyre_common::NotificationType::DependencyChainTooDeep
        );
        assert_eq!(
            notif.priority,
            gyre_common::NotificationType::DependencyChainTooDeep.default_priority(),
        );
        assert_eq!(
            notif.priority, 7,
            "DependencyChainTooDeep should be priority 7"
        );
        assert!(notif.title.contains("11 levels deep"));

        // Verify body JSON.
        let body: serde_json::Value =
            serde_json::from_str(notif.body.as_ref().expect("should have body"))
                .expect("should be valid JSON");
        assert_eq!(body["chain_depth"], 11);
        assert_eq!(body["threshold"], 10);
        assert_eq!(body["workspace_id"], "ws-1");
    }

    #[tokio::test]
    async fn chain_depth_not_warned_at_10_or_below() {
        // Chain of 11 MRs = depth 10, should NOT trigger warning.
        let state = test_state();

        let mut entries = Vec::new();
        for i in 0..11 {
            let mr_id = format!("mr-{i}");
            let deps = if i == 0 {
                vec![]
            } else {
                vec![format!("mr-{}", i - 1)]
            };
            let mut mr = gyre_domain::MergeRequest::new(
                Id::new(&mr_id),
                Id::new("repo-1"),
                format!("MR {mr_id}"),
                format!("feat/{mr_id}"),
                "main",
                1000,
            );
            mr.workspace_id = Id::new("ws-1");
            mr.depends_on = deps.into_iter().map(Id::new).collect();
            state.merge_requests.create(&mr).await.unwrap();

            let entry = enqueue_mr(&state, &mr_id, 50, 1000 + i as u64).await;
            entries.push(entry);
        }

        let graph = build_queue_dependency_graph(&state, &entries)
            .await
            .unwrap();
        let max_depth = compute_max_chain_depth(&graph);
        assert_eq!(max_depth, 10, "chain of 11 MRs has depth 10");
        assert!(max_depth <= 10, "should NOT exceed the >10 threshold");
    }

    #[tokio::test]
    async fn chain_depth_diamond_asymmetric_uses_longest_path() {
        // Asymmetric diamond: mr-a -> mr-g (direct, depth 1)
        //                     mr-a -> mr-b -> mr-c -> mr-g (depth 3)
        // Longest path through g should be 3, not 1.
        // This tests that the algorithm computes longest paths, not shortest (BFS).
        let state = test_state();

        create_mr_with_deps(&state, "mr-a", "ws-1", vec![]).await;
        create_mr_with_deps(&state, "mr-b", "ws-1", vec!["mr-a"]).await;
        create_mr_with_deps(&state, "mr-c", "ws-1", vec!["mr-b"]).await;
        create_mr_with_deps(&state, "mr-g", "ws-1", vec!["mr-a", "mr-c"]).await;

        let e_a = enqueue_mr(&state, "mr-a", 50, 1000).await;
        let e_b = enqueue_mr(&state, "mr-b", 50, 1001).await;
        let e_c = enqueue_mr(&state, "mr-c", 50, 1002).await;
        let e_g = enqueue_mr(&state, "mr-g", 50, 1003).await;

        let entries = vec![e_a, e_b, e_c, e_g];
        let graph = build_queue_dependency_graph(&state, &entries)
            .await
            .unwrap();
        let max_depth = compute_max_chain_depth(&graph);
        assert_eq!(
            max_depth, 3,
            "longest path a->b->c->g has depth 3, not shortest a->g depth 1"
        );
    }

    #[tokio::test]
    async fn topological_sort_same_priority_uses_enqueue_order() {
        // Two independent MRs with same priority: FIFO by enqueued_at.
        let state = test_state();

        create_mr_with_deps(&state, "mr-earlier", "ws-1", vec![]).await;
        create_mr_with_deps(&state, "mr-later", "ws-1", vec![]).await;

        let e_earlier = enqueue_mr(&state, "mr-earlier", 50, 1000).await;
        let e_later = enqueue_mr(&state, "mr-later", 50, 1001).await;

        let entries = vec![e_later.clone(), e_earlier.clone()];
        let graph = build_queue_dependency_graph(&state, &entries)
            .await
            .unwrap();
        let sorted = topological_sort_with_priority(&entries, &graph);

        assert_eq!(sorted.len(), 2);
        assert_eq!(
            sorted[0].merge_request_id.as_str(),
            "mr-earlier",
            "earlier-enqueued MR should be first when priorities are equal"
        );
        assert_eq!(sorted[1].merge_request_id.as_str(), "mr-later");
    }

    #[tokio::test]
    async fn topological_sort_dep_outside_queue_ignored() {
        // mr-b depends on mr-a, but mr-a is not in the queue (already merged).
        // mr-b should still be processable.
        let state = test_state();

        // Create mr-a but DON'T enqueue it.
        create_mr_with_deps(&state, "mr-a", "ws-1", vec![]).await;
        create_mr_with_deps(&state, "mr-b", "ws-1", vec!["mr-a"]).await;

        let e_b = enqueue_mr(&state, "mr-b", 50, 1000).await;

        let entries = vec![e_b.clone()];
        let graph = build_queue_dependency_graph(&state, &entries)
            .await
            .unwrap();
        let sorted = topological_sort_with_priority(&entries, &graph);

        assert_eq!(sorted.len(), 1);
        assert_eq!(
            sorted[0].merge_request_id.as_str(),
            "mr-b",
            "MR with out-of-queue dependency should still appear in sorted order"
        );
        // The dep on mr-a is not in the graph (only in-queue deps are edges).
        // dependencies_satisfied() at merge time will check the actual MR status.
        assert!(
            graph.get("mr-b").unwrap().is_empty(),
            "out-of-queue dependency should not appear in graph edges"
        );
    }

    #[tokio::test]
    async fn empty_queue_returns_empty_sort() {
        let state = test_state();
        let entries: Vec<MergeQueueEntry> = vec![];
        let graph = build_queue_dependency_graph(&state, &entries)
            .await
            .unwrap();
        let sorted = topological_sort_with_priority(&entries, &graph);
        assert!(sorted.is_empty());
        assert_eq!(compute_max_chain_depth(&graph), 0);
    }

    /// F1 scenario: When the highest-priority candidate has pending gates, the
    /// selection loop should skip it and process the next candidate whose gates
    /// have passed. Before the fix, the loop selected the first deps-satisfied
    /// candidate and only checked gates after the loop — if gates were pending,
    /// the entry was requeued and subsequent candidates were never tried.
    #[tokio::test]
    async fn selection_skips_gates_pending_tries_next_candidate() {
        use gyre_domain::{GateResult, GateType, QualityGate};

        let state = test_state();

        // Create a repo for the MRs.
        let repo = create_repo_in_workspace(&state, "test-repo", "ws-1").await;

        // Create two independent MRs (no dependencies between them).
        let mut mr_a = gyre_domain::MergeRequest::new(
            Id::new("mr-a"),
            repo.id.clone(),
            "MR A (gates pending)",
            "feat/a",
            "main",
            1000,
        );
        mr_a.workspace_id = Id::new("ws-1");
        state.merge_requests.create(&mr_a).await.unwrap();

        let mut mr_b = gyre_domain::MergeRequest::new(
            Id::new("mr-b"),
            repo.id.clone(),
            "MR B (gates passed)",
            "feat/b",
            "main",
            1000,
        );
        mr_b.workspace_id = Id::new("ws-1");
        state.merge_requests.create(&mr_b).await.unwrap();

        // Enqueue: mr-a with higher priority, mr-b with lower priority.
        enqueue_mr(&state, "mr-a", 100, 1000).await;
        enqueue_mr(&state, "mr-b", 50, 1001).await;

        // Create a required quality gate.
        let gate = QualityGate {
            id: Id::new("gate-1"),
            repo_id: repo.id.clone(),
            name: "unit-tests".to_string(),
            gate_type: GateType::TestCommand,
            command: Some("cargo test".to_string()),
            required_approvals: None,
            persona: None,
            required: true,
            created_at: 1000,
        };
        state.quality_gates.save(&gate).await.unwrap();

        // mr-a has a pending gate result → check_gates_for_mr returns Ok(false).
        let gate_result_a = GateResult {
            id: Id::new("gr-a"),
            gate_id: Id::new("gate-1"),
            mr_id: Id::new("mr-a"),
            status: GateStatus::Pending,
            output: None,
            started_at: None,
            finished_at: None,
        };
        state.gate_results.save(&gate_result_a).await.unwrap();

        // mr-b has NO gate results → check_gates_for_mr returns Ok(true) (all passed).
        // (No gate result means no required gates are pending/failed.)

        // Run a merge-processor cycle.
        // event-emission:ok — this test verifies candidate selection behavior (gates pending → skip),
        // not merge-time event emission which is covered by other tests.
        process_next(&state).await.unwrap();

        // Verify: mr-b was selected and merged (not mr-a).
        let updated_b = state
            .merge_requests
            .find_by_id(&Id::new("mr-b"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            updated_b.status,
            MrStatus::Merged,
            "mr-b should be merged because it was the next candidate after mr-a was skipped"
        );

        // Verify: mr-a was NOT processed — still Open status.
        let updated_a = state
            .merge_requests
            .find_by_id(&Id::new("mr-a"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            updated_a.status,
            MrStatus::Open,
            "mr-a should remain Open because its gates are pending"
        );

        // Verify queue states: mr-b's entry should be Merged, mr-a's should still be Queued.
        let entry_a = state
            .merge_queue
            .find_by_id(&Id::new("entry-mr-a"))
            .await
            .unwrap()
            .expect("mr-a entry should exist");
        assert_eq!(
            entry_a.status,
            MergeQueueEntryStatus::Queued,
            "mr-a queue entry should remain Queued"
        );

        let entry_b = state
            .merge_queue
            .find_by_id(&Id::new("entry-mr-b"))
            .await
            .unwrap()
            .expect("mr-b entry should exist");
        assert_eq!(
            entry_b.status,
            MergeQueueEntryStatus::Merged,
            "mr-b queue entry should be Merged"
        );
    }

    /// Verify that a candidate with failed gates is marked Failed during selection
    /// and the next candidate is tried.
    #[tokio::test]
    async fn selection_fails_gates_failed_entry_tries_next() {
        use gyre_domain::{GateResult, GateType, QualityGate};

        let state = test_state();

        let repo = create_repo_in_workspace(&state, "test-repo", "ws-1").await;

        // Create two independent MRs.
        let mut mr_a = gyre_domain::MergeRequest::new(
            Id::new("mr-a"),
            repo.id.clone(),
            "MR A (gate failed)",
            "feat/a",
            "main",
            1000,
        );
        mr_a.workspace_id = Id::new("ws-1");
        state.merge_requests.create(&mr_a).await.unwrap();

        let mut mr_b = gyre_domain::MergeRequest::new(
            Id::new("mr-b"),
            repo.id.clone(),
            "MR B (no gates)",
            "feat/b",
            "main",
            1000,
        );
        mr_b.workspace_id = Id::new("ws-1");
        state.merge_requests.create(&mr_b).await.unwrap();

        enqueue_mr(&state, "mr-a", 100, 1000).await;
        enqueue_mr(&state, "mr-b", 50, 1001).await;

        // Create a required gate and a failed result for mr-a.
        let gate = QualityGate {
            id: Id::new("gate-1"),
            repo_id: repo.id.clone(),
            name: "unit-tests".to_string(),
            gate_type: GateType::TestCommand,
            command: Some("cargo test".to_string()),
            required_approvals: None,
            persona: None,
            required: true,
            created_at: 1000,
        };
        state.quality_gates.save(&gate).await.unwrap();

        let gate_result_a = GateResult {
            id: Id::new("gr-a"),
            gate_id: Id::new("gate-1"),
            mr_id: Id::new("mr-a"),
            status: GateStatus::Failed,
            output: Some("test failure".to_string()),
            started_at: Some(1000),
            finished_at: Some(1001),
        };
        state.gate_results.save(&gate_result_a).await.unwrap();

        // event-emission:ok — this test verifies candidate selection behavior (gate failed → fail entry,
        // try next), not merge-time event emission which is covered by other tests.
        process_next(&state).await.unwrap();

        // mr-a should be Failed (gate failure during selection).
        let entry_a = state
            .merge_queue
            .find_by_id(&Id::new("entry-mr-a"))
            .await
            .unwrap()
            .expect("mr-a entry should exist");
        assert_eq!(
            entry_a.status,
            MergeQueueEntryStatus::Failed,
            "mr-a queue entry should be Failed due to gate failure"
        );

        // mr-b should be merged (selected after mr-a was failed).
        let updated_b = state
            .merge_requests
            .find_by_id(&Id::new("mr-b"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            updated_b.status,
            MrStatus::Merged,
            "mr-b should be merged after mr-a was failed due to gate failure"
        );
    }

    /// Verify that a candidate whose atomic group is not ready is skipped,
    /// and the next topologically-ordered candidate is tried (spec step 4c).
    #[tokio::test]
    async fn selection_skips_atomic_group_not_ready_tries_next_candidate() {
        use gyre_domain::{GateResult, GateType, QualityGate};

        let state = test_state();

        let repo = create_repo_in_workspace(&state, "test-repo", "ws-1").await;

        // MR A: in atomic group "bundle", high priority.
        let mut mr_a = gyre_domain::MergeRequest::new(
            Id::new("mr-a"),
            repo.id.clone(),
            "MR A (in atomic group)",
            "feat/a",
            "main",
            1000,
        );
        mr_a.workspace_id = Id::new("ws-1");
        mr_a.atomic_group = Some("bundle".to_string());
        state.merge_requests.create(&mr_a).await.unwrap();

        // MR C: also in atomic group "bundle" — has a pending gate,
        // making the group not ready for MR A.
        let mut mr_c = gyre_domain::MergeRequest::new(
            Id::new("mr-c"),
            repo.id.clone(),
            "MR C (group member, gates pending)",
            "feat/c",
            "main",
            1000,
        );
        mr_c.workspace_id = Id::new("ws-1");
        mr_c.atomic_group = Some("bundle".to_string());
        state.merge_requests.create(&mr_c).await.unwrap();

        // MR B: no atomic group, low priority, all gates passed.
        let mut mr_b = gyre_domain::MergeRequest::new(
            Id::new("mr-b"),
            repo.id.clone(),
            "MR B (no group)",
            "feat/b",
            "main",
            1000,
        );
        mr_b.workspace_id = Id::new("ws-1");
        state.merge_requests.create(&mr_b).await.unwrap();

        // Enqueue: mr-a (high priority), mr-b (low priority).
        // mr-c is NOT enqueued — it's only a group member affecting readiness.
        enqueue_mr(&state, "mr-a", 100, 1000).await;
        enqueue_mr(&state, "mr-b", 50, 1001).await;

        // Create a required gate and a pending result for mr-c.
        // This makes atomic_group_ready("bundle", "mr-a") return Ok(false)
        // because group member mr-c has a pending required gate.
        let gate = QualityGate {
            id: Id::new("gate-1"),
            repo_id: repo.id.clone(),
            name: "unit-tests".to_string(),
            gate_type: GateType::TestCommand,
            command: Some("cargo test".to_string()),
            required_approvals: None,
            persona: None,
            required: true,
            created_at: 1000,
        };
        state.quality_gates.save(&gate).await.unwrap();

        let gate_result_c = GateResult {
            id: Id::new("gr-c"),
            gate_id: Id::new("gate-1"),
            mr_id: Id::new("mr-c"),
            status: GateStatus::Pending,
            output: None,
            started_at: None,
            finished_at: None,
        };
        state.gate_results.save(&gate_result_c).await.unwrap();

        // Run a merge-processor cycle.
        // event-emission:ok — this test verifies candidate selection behavior (atomic group
        // not ready → skip), not merge-time event emission which is covered by other tests.
        process_next(&state).await.unwrap();

        // Verify: mr-b was selected and merged (not mr-a).
        let updated_b = state
            .merge_requests
            .find_by_id(&Id::new("mr-b"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            updated_b.status,
            MrStatus::Merged,
            "mr-b should be merged because it was the next candidate after mr-a was skipped"
        );

        // Verify: mr-a was NOT processed — still Open status.
        let updated_a = state
            .merge_requests
            .find_by_id(&Id::new("mr-a"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            updated_a.status,
            MrStatus::Open,
            "mr-a should remain Open because its atomic group is not ready"
        );

        // Verify queue states: mr-b's entry should be Merged, mr-a's should still be Queued.
        let entry_a = state
            .merge_queue
            .find_by_id(&Id::new("entry-mr-a"))
            .await
            .unwrap()
            .expect("mr-a entry should exist");
        assert_eq!(
            entry_a.status,
            MergeQueueEntryStatus::Queued,
            "mr-a queue entry should remain Queued"
        );

        let entry_b = state
            .merge_queue
            .find_by_id(&Id::new("entry-mr-b"))
            .await
            .unwrap()
            .expect("mr-b entry should exist");
        assert_eq!(
            entry_b.status,
            MergeQueueEntryStatus::Merged,
            "mr-b queue entry should be Merged"
        );
    }

    // ── Atomic group transactional merge tests (TASK-027) ─────────────────

    /// Helper to create an MR in an atomic group.
    async fn create_mr_in_group(
        state: &AppState,
        mr_id: &str,
        repo_id: &Id,
        workspace_id: &str,
        group: &str,
        source_branch: &str,
        author_agent_id: Option<&str>,
    ) -> gyre_domain::MergeRequest {
        let mut mr = gyre_domain::MergeRequest::new(
            Id::new(mr_id),
            repo_id.clone(),
            format!("MR {mr_id}"),
            source_branch,
            "main",
            1000,
        );
        mr.workspace_id = Id::new(workspace_id);
        mr.atomic_group = Some(group.to_string());
        mr.author_agent_id = author_agent_id.map(Id::new);
        state.merge_requests.create(&mr).await.unwrap();
        mr
    }

    /// Test: All members of an atomic group merge in a single processor cycle
    /// in dependency order with no interleaving.
    #[tokio::test]
    async fn atomic_group_all_members_merge_in_one_cycle() {
        let state = test_state();

        let repo = create_repo_in_workspace(&state, "test-repo", "ws-1").await;

        // Create two MRs in atomic group "bundle".
        create_mr_in_group(
            &state,
            "mr-first",
            &repo.id,
            "ws-1",
            "bundle",
            "feat/first",
            Some("agent-1"),
        )
        .await;
        create_mr_in_group(
            &state,
            "mr-second",
            &repo.id,
            "ws-1",
            "bundle",
            "feat/second",
            Some("agent-2"),
        )
        .await;

        // Enqueue in order: mr-first first, mr-second second.
        enqueue_mr(&state, "mr-first", 50, 1000).await;
        enqueue_mr(&state, "mr-second", 50, 1001).await;

        // Subscribe to broadcast channel before running.
        let mut rx = state.message_broadcast_tx.subscribe();

        // Run a single merge-processor cycle.
        process_next(&state).await.unwrap();

        // Both members should be merged.
        let updated_first = state
            .merge_requests
            .find_by_id(&Id::new("mr-first"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            updated_first.status,
            MrStatus::Merged,
            "mr-first should be merged"
        );

        let updated_second = state
            .merge_requests
            .find_by_id(&Id::new("mr-second"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            updated_second.status,
            MrStatus::Merged,
            "mr-second should be merged"
        );

        // Both queue entries should be Merged.
        let entry_first = state
            .merge_queue
            .find_by_id(&Id::new("entry-mr-first"))
            .await
            .unwrap()
            .expect("mr-first entry should exist");
        assert_eq!(entry_first.status, MergeQueueEntryStatus::Merged);

        let entry_second = state
            .merge_queue
            .find_by_id(&Id::new("entry-mr-second"))
            .await
            .unwrap()
            .expect("mr-second entry should exist");
        assert_eq!(entry_second.status, MergeQueueEntryStatus::Merged);

        // No AtomicGroupFailed event should be emitted (success case).
        let mut found_failed = false;
        while let Ok(msg) = rx.try_recv() {
            if msg.kind == gyre_common::MessageKind::AtomicGroupFailed {
                found_failed = true;
            }
        }
        assert!(
            !found_failed,
            "no AtomicGroupFailed event should be emitted on success"
        );
    }

    /// Test: No interleaving — a non-group MR does not merge between group members.
    #[tokio::test]
    async fn atomic_group_no_interleaving_with_non_group_mr() {
        let state = test_state();

        let repo = create_repo_in_workspace(&state, "test-repo", "ws-1").await;

        // Create two MRs in atomic group "bundle".
        create_mr_in_group(&state, "mr-g1", &repo.id, "ws-1", "bundle", "feat/g1", None).await;
        create_mr_in_group(&state, "mr-g2", &repo.id, "ws-1", "bundle", "feat/g2", None).await;

        // Create a non-group MR with HIGHER priority than the group members.
        let mut mr_solo = gyre_domain::MergeRequest::new(
            Id::new("mr-solo"),
            repo.id.clone(),
            "Solo MR",
            "feat/solo",
            "main",
            1000,
        );
        mr_solo.workspace_id = Id::new("ws-1");
        state.merge_requests.create(&mr_solo).await.unwrap();

        // Enqueue: group members first, solo with higher priority.
        enqueue_mr(&state, "mr-g1", 50, 1000).await;
        enqueue_mr(&state, "mr-g2", 50, 1001).await;
        enqueue_mr(&state, "mr-solo", 100, 1002).await;

        // Run one cycle — the solo MR has higher priority but the topological sort
        // picks mr-solo first (it's independent). But since mr-g1 is also independent,
        // if mr-solo is selected first (higher priority), it should merge solo.
        // Then on the next cycle, the group should merge together.

        // First cycle: mr-solo merges (highest priority independent).
        // event-emission:ok — this test verifies atomic group no-interleaving behavior,
        // not merge-time event emission which is covered by other tests.
        process_next(&state).await.unwrap();

        let solo_status = state
            .merge_requests
            .find_by_id(&Id::new("mr-solo"))
            .await
            .unwrap()
            .unwrap()
            .status;
        assert_eq!(
            solo_status,
            MrStatus::Merged,
            "solo MR should merge in the first cycle"
        );

        // Second cycle: both group members should merge together.
        process_next(&state).await.unwrap();

        let g1_status = state
            .merge_requests
            .find_by_id(&Id::new("mr-g1"))
            .await
            .unwrap()
            .unwrap()
            .status;
        let g2_status = state
            .merge_requests
            .find_by_id(&Id::new("mr-g2"))
            .await
            .unwrap()
            .unwrap()
            .status;

        assert_eq!(g1_status, MrStatus::Merged, "mr-g1 should be merged");
        assert_eq!(g2_status, MrStatus::Merged, "mr-g2 should be merged");
    }

    /// Test: Rollback on failure — all already-merged members are rolled back
    /// and the entire group is requeued.
    #[tokio::test]
    async fn atomic_group_rollback_requeues_all_members() {
        let state = test_state();

        let repo = create_repo_in_workspace(&state, "test-repo", "ws-1").await;

        // Create two MRs in atomic group "bundle".
        let mr_first = create_mr_in_group(
            &state,
            "mr-first",
            &repo.id,
            "ws-1",
            "bundle",
            "feat/first",
            Some("agent-1"),
        )
        .await;
        let mr_second = create_mr_in_group(
            &state,
            "mr-second",
            &repo.id,
            "ws-1",
            "bundle",
            "feat/second",
            Some("agent-2"),
        )
        .await;

        // Enqueue both.
        let e_first = enqueue_mr(&state, "mr-first", 50, 1000).await;
        let e_second = enqueue_mr(&state, "mr-second", 50, 1001).await;

        // Subscribe to broadcast channel.
        let mut rx = state.message_broadcast_tx.subscribe();

        // Simulate: mr-first was already merged, mr-second failed.
        let merged_entries = vec![(e_first.clone(), mr_first.clone())];
        let all_group_entries = vec![e_first.clone(), e_second.clone()];

        rollback_atomic_group(
            &state,
            "bundle",
            &repo,
            "main",
            Some("pre_group_sha_abc123"),
            &merged_entries,
            &all_group_entries,
            "conflict in MR mr-second: file.rs",
            &mr_second.id,
        )
        .await
        .unwrap();

        // Both entries should be requeued (status back to Queued).
        let entry_first = state
            .merge_queue
            .find_by_id(&e_first.id)
            .await
            .unwrap()
            .expect("mr-first entry should exist");
        assert_eq!(
            entry_first.status,
            MergeQueueEntryStatus::Queued,
            "mr-first should be requeued after rollback"
        );

        let entry_second = state
            .merge_queue
            .find_by_id(&e_second.id)
            .await
            .unwrap()
            .expect("mr-second entry should exist");
        assert_eq!(
            entry_second.status,
            MergeQueueEntryStatus::Queued,
            "mr-second should be requeued after rollback"
        );

        // AtomicGroupFailed event should be emitted.
        let msg = rx
            .try_recv()
            .expect("AtomicGroupFailed event should be emitted");
        assert_eq!(msg.kind, gyre_common::MessageKind::AtomicGroupFailed);
        let payload = msg.payload.as_ref().expect("event should have payload");
        assert_eq!(payload["group"], "bundle");
        assert_eq!(payload["failing_mr_id"], mr_second.id.to_string());
        assert_eq!(payload["member_count"], 2);
        assert_eq!(payload["rolled_back_count"], 1);
    }

    /// Test: All distinct authors receive AtomicGroupFailure notifications
    /// when a group merge fails.
    #[tokio::test]
    async fn atomic_group_failure_notifies_all_distinct_authors() {
        let state = test_state();

        let repo = create_repo_in_workspace(&state, "test-repo", "ws-1").await;

        // Create two MRs with DIFFERENT author_agent_ids.
        let mr_first = create_mr_in_group(
            &state,
            "mr-first",
            &repo.id,
            "ws-1",
            "bundle",
            "feat/first",
            Some("author-A"),
        )
        .await;
        let mr_second = create_mr_in_group(
            &state,
            "mr-second",
            &repo.id,
            "ws-1",
            "bundle",
            "feat/second",
            Some("author-B"),
        )
        .await;

        // Add a workspace member (human user who should also be notified).
        let membership = gyre_domain::WorkspaceMembership::new(
            Id::new("member-1"),
            Id::new("user-1"),
            Id::new("ws-1"),
            gyre_domain::WorkspaceRole::Developer,
            Id::new("admin"),
            1000,
        );
        state
            .workspace_memberships
            .create(&membership)
            .await
            .unwrap();

        let e_first = enqueue_mr(&state, "mr-first", 50, 1000).await;
        let e_second = enqueue_mr(&state, "mr-second", 50, 1001).await;

        // Subscribe to broadcast.
        let mut rx = state.message_broadcast_tx.subscribe();

        // Rollback with failure.
        let merged_entries = vec![(e_first.clone(), mr_first.clone())];
        let all_group_entries = vec![e_first.clone(), e_second.clone()];

        rollback_atomic_group(
            &state,
            "bundle",
            &repo,
            "main",
            Some("pre_group_sha_abc123"),
            &merged_entries,
            &all_group_entries,
            "conflict in MR mr-second",
            &mr_second.id,
        )
        .await
        .unwrap();

        // Check notifications for author-A.
        let notifs_a = state
            .notifications
            .list_for_user(
                &Id::new("author-A"),
                Some(&Id::new("ws-1")),
                None,
                None,
                None,
                100,
                0,
            )
            .await
            .unwrap();
        assert!(
            !notifs_a.is_empty(),
            "author-A should receive a notification"
        );
        assert_eq!(
            notifs_a[0].notification_type,
            gyre_common::NotificationType::AtomicGroupFailure
        );
        assert_eq!(
            notifs_a[0].priority,
            gyre_common::NotificationType::AtomicGroupFailure.default_priority(),
            "priority should match AtomicGroupFailure default"
        );
        assert!(notifs_a[0].title.contains("bundle"));
        assert!(notifs_a[0].title.contains("rolled back"));

        // Verify notification body.
        let body: serde_json::Value = serde_json::from_str(
            notifs_a[0]
                .body
                .as_ref()
                .expect("notification should have body"),
        )
        .expect("body should be valid JSON");
        assert_eq!(body["group"], "bundle");
        assert_eq!(body["failing_mr_id"], mr_second.id.to_string());
        assert_eq!(body["member_count"], 2);
        assert_eq!(body["rolled_back_count"], 1);

        // Verify entity_ref.
        assert_eq!(
            notifs_a[0].entity_ref.as_deref(),
            Some(mr_second.id.as_str()),
            "entity_ref should be the failing MR id"
        );

        // Check notifications for author-B.
        let notifs_b = state
            .notifications
            .list_for_user(
                &Id::new("author-B"),
                Some(&Id::new("ws-1")),
                None,
                None,
                None,
                100,
                0,
            )
            .await
            .unwrap();
        assert!(
            !notifs_b.is_empty(),
            "author-B should receive a notification"
        );
        assert_eq!(
            notifs_b[0].notification_type,
            gyre_common::NotificationType::AtomicGroupFailure
        );

        // Check notifications for workspace member user-1.
        let notifs_user = state
            .notifications
            .list_for_user(
                &Id::new("user-1"),
                Some(&Id::new("ws-1")),
                None,
                None,
                None,
                100,
                0,
            )
            .await
            .unwrap();
        assert!(
            !notifs_user.is_empty(),
            "workspace member should receive a notification"
        );

        // Verify event.
        let msg = rx
            .try_recv()
            .expect("AtomicGroupFailed event should be emitted");
        assert_eq!(msg.kind, gyre_common::MessageKind::AtomicGroupFailed);
    }

    /// Test: Atomic group implies dependency ordering among members.
    /// Members enqueued in order automatically have dependency edges.
    #[tokio::test]
    async fn atomic_group_implies_dependency_ordering() {
        let state = test_state();

        let repo = create_repo_in_workspace(&state, "test-repo", "ws-1").await;

        // Create three MRs in the same atomic group.
        create_mr_in_group(&state, "mr-a", &repo.id, "ws-1", "deploy", "feat/a", None).await;
        create_mr_in_group(&state, "mr-b", &repo.id, "ws-1", "deploy", "feat/b", None).await;
        create_mr_in_group(&state, "mr-c", &repo.id, "ws-1", "deploy", "feat/c", None).await;

        // Enqueue in order: a, b, c. Atomic group implies a <- b <- c.
        let e_a = enqueue_mr(&state, "mr-a", 50, 1000).await;
        let e_b = enqueue_mr(&state, "mr-b", 50, 1001).await;
        let e_c = enqueue_mr(&state, "mr-c", 50, 1002).await;

        let entries = vec![e_c.clone(), e_b.clone(), e_a.clone()]; // shuffled
        let graph = build_queue_dependency_graph(&state, &entries)
            .await
            .unwrap();
        let sorted = topological_sort_with_priority(&entries, &graph);

        // Dependency ordering should force a, b, c order.
        assert_eq!(sorted.len(), 3);
        assert_eq!(
            sorted[0].merge_request_id.as_str(),
            "mr-a",
            "first-enqueued group member should be first"
        );
        assert_eq!(
            sorted[1].merge_request_id.as_str(),
            "mr-b",
            "second-enqueued group member should be second"
        );
        assert_eq!(
            sorted[2].merge_request_id.as_str(),
            "mr-c",
            "third-enqueued group member should be third"
        );

        // Run a single merge cycle — all three should merge together.
        // event-emission:ok — this test verifies atomic group ordering behavior,
        // not merge-time event emission which is covered by other tests.
        process_next(&state).await.unwrap();

        let status_a = state
            .merge_requests
            .find_by_id(&Id::new("mr-a"))
            .await
            .unwrap()
            .unwrap()
            .status;
        let status_b = state
            .merge_requests
            .find_by_id(&Id::new("mr-b"))
            .await
            .unwrap()
            .unwrap()
            .status;
        let status_c = state
            .merge_requests
            .find_by_id(&Id::new("mr-c"))
            .await
            .unwrap()
            .unwrap()
            .status;

        assert_eq!(status_a, MrStatus::Merged, "mr-a should be merged");
        assert_eq!(status_b, MrStatus::Merged, "mr-b should be merged");
        assert_eq!(status_c, MrStatus::Merged, "mr-c should be merged");
    }

    /// Test: Rollback with no pre-group SHA (edge case — e.g., target branch
    /// doesn't exist yet). Should still requeue and notify.
    #[tokio::test]
    async fn atomic_group_rollback_without_pre_group_sha() {
        let state = test_state();

        let repo = create_repo_in_workspace(&state, "test-repo", "ws-1").await;

        let mr_first = create_mr_in_group(
            &state,
            "mr-first",
            &repo.id,
            "ws-1",
            "bundle",
            "feat/first",
            Some("agent-1"),
        )
        .await;

        let e_first = enqueue_mr(&state, "mr-first", 50, 1000).await;

        let mut rx = state.message_broadcast_tx.subscribe();

        // Rollback with no pre-group SHA.
        let merged_entries: Vec<(MergeQueueEntry, MergeRequest)> = vec![];
        let all_group_entries = vec![e_first.clone()];

        rollback_atomic_group(
            &state,
            "bundle",
            &repo,
            "main",
            None, // no pre-group SHA
            &merged_entries,
            &all_group_entries,
            "MR not found",
            &mr_first.id,
        )
        .await
        .unwrap();

        // Entry should still be requeued.
        let entry = state
            .merge_queue
            .find_by_id(&e_first.id)
            .await
            .unwrap()
            .expect("entry should exist");
        assert_eq!(
            entry.status,
            MergeQueueEntryStatus::Queued,
            "entry should be requeued even without pre-group SHA"
        );

        // Event should still be emitted.
        let msg = rx
            .try_recv()
            .expect("AtomicGroupFailed event should be emitted");
        assert_eq!(msg.kind, gyre_common::MessageKind::AtomicGroupFailed);
    }
}
