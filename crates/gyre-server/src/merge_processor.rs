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

async fn process_next(state: &AppState) -> anyhow::Result<()> {
    // Get all queued entries and find the first one whose dependencies are all merged.
    let all_queued = state.merge_queue.list_queue().await?;
    let entry = {
        let mut found = None;
        for candidate in all_queued {
            if candidate.status != MergeQueueEntryStatus::Queued {
                continue;
            }
            if dependencies_satisfied(state, &candidate.merge_request_id).await? {
                found = Some(candidate);
                break;
            }
        }
        match found {
            Some(e) => e,
            None => return Ok(()),
        }
    };

    info!(entry_id = %entry.id, mr_id = %entry.merge_request_id, "processing merge queue entry");

    // Mark as Processing
    state
        .merge_queue
        .update_status(&entry.id, MergeQueueEntryStatus::Processing, None)
        .await?;

    // Look up the merge request
    let mr = match state
        .merge_requests
        .find_by_id(&entry.merge_request_id)
        .await?
    {
        Some(mr) => mr,
        None => {
            warn!(mr_id = %entry.merge_request_id, "MR not found for queue entry");
            state
                .merge_queue
                .update_status(
                    &entry.id,
                    MergeQueueEntryStatus::Failed,
                    Some("merge request not found".to_string()),
                )
                .await?;
            return Ok(());
        }
    };

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

    // If this MR is part of an atomic group, ensure all group members are ready.
    if let Some(ref group) = mr.atomic_group {
        match atomic_group_ready(state, group, &mr.id).await {
            Ok(true) => {}
            Ok(false) => {
                info!(entry_id = %entry.id, group = %group, "atomic group not ready, requeueing");
                state
                    .merge_queue
                    .update_status(&entry.id, MergeQueueEntryStatus::Queued, None)
                    .await?;
                return Ok(());
            }
            Err(e) => {
                warn!(entry_id = %entry.id, error = %e, "error checking atomic group");
            }
        }
    }

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

    // Check quality gates before merging.
    match crate::gate_executor::check_gates_for_mr(state, &mr.id).await {
        Ok(true) => {} // all passed or no gates
        Ok(false) => {
            // Gates still running — put the entry back to Queued to retry later.
            info!(entry_id = %entry.id, "quality gates still running, requeueing");
            state
                .merge_queue
                .update_status(&entry.id, MergeQueueEntryStatus::Queued, None)
                .await?;
            return Ok(());
        }
        Err(reason) => {
            warn!(entry_id = %entry.id, reason = %reason, "quality gate failed, blocking merge");
            state
                .merge_queue
                .update_status(
                    &entry.id,
                    MergeQueueEntryStatus::Failed,
                    Some(format!("quality gate failed: {reason}")),
                )
                .await?;
            return Ok(());
        }
    }

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
}
