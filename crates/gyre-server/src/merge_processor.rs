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
        // block the merge.
        for link in &relevant_links {
            if link.link_type == crate::spec_registry::SpecLinkType::ConflictsWith
                && link.source_path == spec_path
            {
                if let Ok(Some(conflicting)) =
                    state.spec_ledger.find_by_path(&link.target_path).await
                {
                    if conflicting.approval_status == crate::spec_registry::ApprovalStatus::Approved
                    {
                        let reason = format!(
                            "spec '{}' conflicts with approved spec '{}' — resolve the conflict first",
                            spec_path, link.target_path
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
