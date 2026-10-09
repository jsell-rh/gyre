//! Push-time and merge-time constraint evaluation.
//!
//! Audit-only path (`evaluate_*`): Derives strategy-implied constraints from
//! `InputContent`, evaluates against the actual diff, logs results, and emits
//! `ConstraintViolation` events. Does NOT reject pushes or merges.
//!
//! Enforcement path (`enforce_*`): Same evaluation pipeline, but returns `Err`
//! to reject pushes/merges when the attestation chain is invalid or constraints
//! fail. Enforced by `builtin:require-signed-authorization` ABAC policy.
//!
//! Agent attestation levels are derived from workload attestation data
//! (supply-chain.md §2) and evaluated against repo stack policy requirements.

use gyre_common::attestation::OutputConstraint;
use gyre_common::message::{Destination, MessageKind};
use gyre_common::{AttestationInput, Id, NotificationType, VerificationResult};
use gyre_domain::constraint_evaluator::{
    self, Action, AgentContext, ConstraintInput, DiffStatsContext, OutputContext, TargetContext,
};
use tracing::{info, warn};

use crate::AppState;

/// Details about a single constraint violation for event emission.
#[derive(Debug, Clone)]
pub struct ConstraintViolationInfo {
    pub constraint_name: String,
    pub expression: String,
    pub message: String,
}

/// Run constraint evaluation at push time (§8 Phase 2).
///
/// Looks up the attestation chain for the task, derives strategy-implied
/// constraints from the `InputContent`, evaluates against the actual diff,
/// logs results, and emits `ConstraintViolation` events on failures.
///
/// This is audit-only: results are logged, events emitted, notifications
/// created, but pushes are NEVER rejected.
pub async fn evaluate_push_constraints(
    state: &AppState,
    task_id: &str,
    repo_id: &str,
    repo_path: &str,
    agent_id: &str,
    workspace_id: &Id,
    ref_updates: &[(String, String, String)], // (old_sha, new_sha, refname)
    default_branch: &str,
) {
    // Look up attestation chain for this task.
    let attestations = match state.chain_attestations.find_by_task(task_id).await {
        Ok(atts) if !atts.is_empty() => atts,
        Ok(_) => {
            tracing::debug!(
                task_id = %task_id,
                repo_id = %repo_id,
                "no attestation chain found for task (Phase 2, logging only)" // enforcement-mode:ok — Phase 2 is genuinely non-enforcing
            );
            return;
        }
        Err(e) => {
            warn!(
                task_id = %task_id,
                error = %e,
                "failed to query attestation chain (Phase 2, non-blocking)"
            );
            return;
        }
    };

    // Find the root SignedInput in the chain, along with its attestation_id.
    let signed_input_with_id = attestations.iter().find_map(|att| match &att.input {
        AttestationInput::Signed(si) => Some((si, &att.id)),
        _ => None,
    });

    let Some((signed_input, attestation_id)) = signed_input_with_id else {
        tracing::debug!(
            task_id = %task_id,
            "no SignedInput found in attestation chain (Phase 2, skipping)"
        );
        return;
    };
    let attestation_id = attestation_id.clone();

    // §2.4 Context binding (replay prevention), audit-only mirror: record
    // mismatches for Phase 2 observability without blocking the push.
    // Enforcement lives in `enforce_push_constraints`.
    {
        let (task_spec_sha, task_generation) = resolve_task_binding_context(state, task_id).await;
        let binding_result = crate::git_http::verify_context_binding(
            signed_input,
            repo_id,
            workspace_id.as_str(),
            task_spec_sha.as_deref(),
            task_generation,
        );
        if !binding_result.valid {
            for child in &binding_result.children {
                if !child.valid {
                    warn!(
                        task_id = %task_id,
                        repo_id = %repo_id,
                        agent_id = %agent_id,
                        category = "Provenance",
                        event = "attestation.context_binding_mismatch",
                        binding = %child.label,
                        message = %child.message,
                        "attestation.context_binding_mismatch (audit-only): {}",
                        child.message
                    );
                }
            }
        }
    }

    // Compute the diff for constraint evaluation.
    let diff_info = match compute_push_diff(repo_path, ref_updates).await {
        Some(d) => d,
        None => {
            warn!(
                task_id = %task_id,
                "failed to compute push diff for constraint evaluation"
            );
            return;
        }
    };

    // Build agent context from the agent record, workspace meta-spec set, and
    // workload attestation.
    let agent_ctx = build_agent_context(state, agent_id, task_id, workspace_id).await;

    // Look up workspace trust level.
    let workspace = state
        .workspaces
        .find_by_id(workspace_id)
        .await
        .ok()
        .flatten();
    let trust_level = workspace
        .as_ref()
        .map(|ws| format!("{:?}", ws.trust_level).to_lowercase());

    // Derive strategy-implied constraints, including repo attestation level policy.
    let required_attestation_level = get_repo_required_attestation_level(state, repo_id).await;
    let strategy_constraints = constraint_evaluator::derive_strategy_constraints(
        &signed_input.content,
        trust_level.as_deref(),
        required_attestation_level,
    );

    // Collect all constraints: explicit + strategy-implied + gate.
    let gate_constraints: Vec<gyre_common::attestation::GateConstraint> = attestations
        .iter()
        .flat_map(|att| {
            att.output
                .gate_results
                .iter()
                .filter_map(|gr| gr.constraint.clone())
        })
        .collect();

    let all_constraints = constraint_evaluator::collect_all_constraints(
        &signed_input.output_constraints,
        &strategy_constraints,
        &gate_constraints,
    );

    if all_constraints.is_empty() {
        tracing::debug!(
            task_id = %task_id,
            "no constraints to evaluate (Phase 2)"
        );
        return;
    }

    // Build CEL evaluation context.
    let branch = ref_updates
        .first()
        .map(|(_, _, refname)| {
            refname
                .strip_prefix("refs/heads/")
                .unwrap_or(refname)
                .to_string()
        })
        .unwrap_or_default();

    let target_ctx = TargetContext {
        repo_id: repo_id.to_string(),
        workspace_id: workspace_id.to_string(),
        branch: branch.clone(),
        default_branch: default_branch.to_string(),
    };

    let ci = ConstraintInput {
        input: &signed_input.content,
        output: &diff_info,
        agent: &agent_ctx,
        target: &target_ctx,
        action: Action::Push,
    };

    let ctx = match constraint_evaluator::build_cel_context(&ci) {
        Ok(c) => c,
        Err(e) => {
            warn!(
                task_id = %task_id,
                error = %e,
                "failed to build CEL context for constraint evaluation (Phase 2)"
            );
            return;
        }
    };

    // Evaluate all constraints.
    let result = constraint_evaluator::evaluate_all(&all_constraints, &ctx);

    // Log results.
    log_constraint_results(
        &result,
        task_id,
        repo_id,
        agent_id,
        "push",
        all_constraints.len(),
    );

    // If there are violations, emit ConstraintViolation events and create notifications.
    if !result.valid {
        let violations = extract_violations(&result, &all_constraints);
        // Build context_snapshot from the CEL context components (§7.5).
        let context_snapshot = serde_json::json!({
            "input": &signed_input.content,
            "output": &diff_info,
            "agent": &agent_ctx,
            "target": &target_ctx,
            "action": "push",
        });
        emit_constraint_violations(
            state,
            &violations,
            &attestation_id,
            repo_id,
            agent_id,
            workspace_id,
            "push",
            &context_snapshot,
        )
        .await;
        create_violation_notifications(state, &violations, task_id, repo_id, workspace_id, "push")
            .await;
    }
}

/// Phase 3: Enforce constraint evaluation at push time.
///
/// Same pipeline as `evaluate_push_constraints` but returns `Err(reason)` when
/// the attestation chain is invalid or any constraint fails — causing the push
/// handler to reject the push and undo ref updates.
///
/// Returns `Ok(())` when all constraints pass or when there is no attestation
/// chain (graceful degradation for repos that haven't adopted provenance yet).
pub async fn enforce_push_constraints(
    state: &AppState,
    task_id: &str,
    repo_id: &str,
    repo_path: &str,
    agent_id: &str,
    workspace_id: &Id,
    ref_updates: &[(String, String, String)],
    default_branch: &str,
) -> Result<(), String> {
    // Look up attestation chain for this task.
    let attestations = match state.chain_attestations.find_by_task(task_id).await {
        Ok(atts) if !atts.is_empty() => atts,
        Ok(_) => {
            // No attestation chain found. During Phase 3 migration, tasks that
            // predate the provenance system won't have chains. Log and allow.
            // The builtin:require-signed-authorization ABAC policy is the
            // enforcement gate — repos/workspaces that have adopted provenance
            // will have chains; those that haven't will not be blocked.
            tracing::debug!(
                task_id = %task_id,
                repo_id = %repo_id,
                "no attestation chain found for task (Phase 3 enforcement, allowing)"
            );
            return Ok(());
        }
        Err(e) => {
            // Query failures are non-blocking to avoid false rejections.
            warn!(
                task_id = %task_id,
                error = %e,
                "failed to query attestation chain (Phase 3, allowing)"
            );
            return Ok(());
        }
    };

    // Find the root SignedInput.
    let signed_input_with_id = attestations.iter().find_map(|att| match &att.input {
        AttestationInput::Signed(si) => Some((si, &att.id)),
        _ => None,
    });

    let Some((signed_input, attestation_id)) = signed_input_with_id else {
        // Chain exists but has no root SignedInput — reject.
        return Err(
            "push rejected: no SignedInput found in attestation chain — \
             a human-signed authorization root is required"
                .to_string(),
        );
    };
    let attestation_id = attestation_id.clone();

    // Verify the full attestation chain structure (§4.4, §6.2).
    let chain_result = crate::git_http::verify_chain(&attestations, 10);
    if !chain_result.valid {
        // §7.7: attestation.chain_invalid audit event.
        warn!(
            task_id = %task_id,
            repo_id = %repo_id,
            agent_id = %agent_id,
            category = "Provenance",
            event = "attestation.chain_invalid",
            message = %chain_result.message,
            "attestation.chain_invalid: push-time chain verification failed"
        );
        // Emit ConstraintViolation event for observability.
        state
            .emit_event(
                Some(workspace_id.clone()),
                Destination::Workspace(workspace_id.clone()),
                MessageKind::ConstraintViolation,
                Some(serde_json::json!({
                    "attestation_id": attestation_id,
                    "constraint_name": "attestation_chain_valid",
                    "expression": "verify_chain(attestation)",
                    "context_snapshot": {},
                    "action": "push",
                    "agent_id": agent_id,
                    "repo_id": repo_id,
                    "timestamp": crate::api::now_secs(),
                })),
            )
            .await;
        return Err(format!(
            "push rejected: attestation chain invalid — {}",
            chain_result.message
        ));
    }

    // §2.4 Context binding (replay prevention): compare the signed root
    // InputContent against the actual target of this push. A SignedInput
    // authorizing repo-A must not authorize a push to repo-B.
    let (task_spec_sha, task_generation) = resolve_task_binding_context(state, task_id).await;
    let binding_result = crate::git_http::verify_context_binding(
        signed_input,
        repo_id,
        workspace_id.as_str(),
        task_spec_sha.as_deref(),
        task_generation,
    );
    if !binding_result.valid {
        emit_context_binding_rejection(
            state,
            &binding_result,
            &attestation_id,
            task_id,
            repo_id,
            agent_id,
            workspace_id,
            "push",
        )
        .await;
        return Err(format!(
            "push rejected: attestation context binding failed — {}",
            context_binding_failure_summary(&binding_result)
        ));
    }

    // Compute the diff for constraint evaluation.
    let diff_info = match compute_push_diff(repo_path, ref_updates).await {
        Some(d) => d,
        None => {
            return Err(
                "push rejected: failed to compute diff for constraint evaluation".to_string(),
            );
        }
    };

    // Build agent context.
    let agent_ctx = build_agent_context(state, agent_id, task_id, workspace_id).await;

    // Workspace trust level.
    let workspace = state
        .workspaces
        .find_by_id(workspace_id)
        .await
        .ok()
        .flatten();
    let trust_level = workspace
        .as_ref()
        .map(|ws| format!("{:?}", ws.trust_level).to_lowercase());

    // Derive strategy-implied constraints, including repo attestation level policy.
    let required_attestation_level = get_repo_required_attestation_level(state, repo_id).await;
    let strategy_constraints = constraint_evaluator::derive_strategy_constraints(
        &signed_input.content,
        trust_level.as_deref(),
        required_attestation_level,
    );

    // Collect all constraints from the full chain (§4.3 additive accumulation).
    let (_root, explicit_from_chain, gate_from_chain) =
        crate::git_http::accumulate_chain_constraints(&attestations);

    let all_constraints = constraint_evaluator::collect_all_constraints(
        &explicit_from_chain,
        &strategy_constraints,
        &gate_from_chain,
    );

    if all_constraints.is_empty() {
        return Ok(());
    }

    // Build CEL evaluation context.
    let branch = ref_updates
        .first()
        .map(|(_, _, refname)| {
            refname
                .strip_prefix("refs/heads/")
                .unwrap_or(refname)
                .to_string()
        })
        .unwrap_or_default();

    let target_ctx = TargetContext {
        repo_id: repo_id.to_string(),
        workspace_id: workspace_id.to_string(),
        branch: branch.clone(),
        default_branch: default_branch.to_string(),
    };

    let ci = ConstraintInput {
        input: &signed_input.content,
        output: &diff_info,
        agent: &agent_ctx,
        target: &target_ctx,
        action: Action::Push,
    };

    let ctx = match constraint_evaluator::build_cel_context(&ci) {
        Ok(c) => c,
        Err(e) => {
            return Err(format!("push rejected: failed to build CEL context: {e}"));
        }
    };

    // Evaluate all constraints.
    let result = constraint_evaluator::evaluate_all(&all_constraints, &ctx);

    // Log results.
    log_constraint_results(
        &result,
        task_id,
        repo_id,
        agent_id,
        "push",
        all_constraints.len(),
    );

    if !result.valid {
        let violations = extract_violations(&result, &all_constraints);
        let context_snapshot = serde_json::json!({
            "input": &signed_input.content,
            "output": &diff_info,
            "agent": &agent_ctx,
            "target": &target_ctx,
            "action": "push",
        });
        emit_constraint_violations(
            state,
            &violations,
            &attestation_id,
            repo_id,
            agent_id,
            workspace_id,
            "push",
            &context_snapshot,
        )
        .await;
        create_violation_notifications(state, &violations, task_id, repo_id, workspace_id, "push")
            .await;

        // Build a human-readable rejection message with failing constraint details.
        let details: Vec<String> = violations
            .iter()
            .map(|v| format!("  - {}: {}", v.constraint_name, v.message))
            .collect();
        return Err(format!(
            "push rejected: {} constraint(s) failed:\n{}",
            violations.len(),
            details.join("\n")
        ));
    }

    Ok(())
}

/// Phase 3: Enforce constraint evaluation at merge time.
///
/// Same pipeline as `evaluate_merge_constraints` but returns `Err(reason)`
/// when the attestation chain is invalid or any constraint fails — causing
/// the merge processor to fail the queue entry.
///
/// Returns `Ok(())` when all constraints pass or when there is no attestation
/// chain (graceful degradation).
pub async fn enforce_merge_constraints(
    state: &AppState,
    mr_id: &str,
    repo_id: &str,
    repo_path: &str,
    merge_commit_sha: &str,
    workspace_id: &Id,
    _source_branch: &str,
    target_branch: &str,
    default_branch: &str,
) -> Result<(), String> {
    // Look up the MR to find the agent and task.
    let mr = match state
        .merge_requests
        .find_by_id(&Id::new(mr_id))
        .await
        .ok()
        .flatten()
    {
        Some(m) => m,
        None => return Ok(()), // MR not found — not our problem
    };

    let agent_id = mr
        .author_agent_id
        .as_ref()
        .map(|id| id.to_string())
        .unwrap_or_default();

    // Resolve the agent's task_id.
    let task_id = if !agent_id.is_empty() {
        let agent = state
            .agents
            .find_by_id(&Id::new(&agent_id))
            .await
            .ok()
            .flatten();
        agent.and_then(|a| a.current_task_id.map(|id| id.to_string()))
    } else {
        None
    };

    let Some(task_id) = task_id else {
        // No task_id → no attestation chain possible. Allow during migration.
        tracing::debug!(
            mr_id = %mr_id,
            "no task_id for merge — allowing (Phase 3 migration)"
        );
        return Ok(());
    };

    // Look up attestation chain for this task.
    let attestations = match state.chain_attestations.find_by_task(&task_id).await {
        Ok(atts) if !atts.is_empty() => atts,
        Ok(_) => {
            tracing::debug!(
                mr_id = %mr_id,
                task_id = %task_id,
                "no attestation chain for merge (Phase 3, allowing)"
            );
            return Ok(());
        }
        Err(e) => {
            warn!(
                mr_id = %mr_id,
                error = %e,
                "failed to query attestation chain for merge (Phase 3, allowing)"
            );
            return Ok(());
        }
    };

    // Find root SignedInput.
    let signed_input_with_id = attestations.iter().find_map(|att| match &att.input {
        AttestationInput::Signed(si) => Some((si, &att.id)),
        _ => None,
    });

    let Some((signed_input, attestation_id)) = signed_input_with_id else {
        return Err("merge blocked: no SignedInput found in attestation chain".to_string());
    };
    let attestation_id = attestation_id.clone();

    // Verify full attestation chain structure (§4.4, §6.2).
    let chain_result = crate::git_http::verify_chain(&attestations, 10);
    if !chain_result.valid {
        return Err(format!(
            "merge blocked: attestation chain invalid — {}",
            chain_result.message
        ));
    }

    // §2.4 Context binding (replay prevention): compare the signed root
    // InputContent against the actual target of this merge.
    let (task_spec_sha, task_generation) =
        resolve_task_binding_context(state, &task_id).await;
    let binding_result = crate::git_http::verify_context_binding(
        signed_input,
        repo_id,
        workspace_id.as_str(),
        task_spec_sha.as_deref(),
        task_generation,
    );
    if !binding_result.valid {
        emit_context_binding_rejection(
            state,
            &binding_result,
            &attestation_id,
            &task_id,
            repo_id,
            &agent_id,
            workspace_id,
            "merge",
        )
        .await;
        return Err(format!(
            "merge blocked: attestation context binding failed — {}",
            context_binding_failure_summary(&binding_result)
        ));
    }

    // Compute diff for the merge commit.
    let diff_info = match compute_commit_diff(repo_path, merge_commit_sha).await {
        Some(d) => d,
        None => {
            return Err(
                "merge blocked: failed to compute diff for constraint evaluation".to_string(),
            );
        }
    };

    let agent_ctx = build_agent_context(state, &agent_id, &task_id, workspace_id).await;

    let workspace = state
        .workspaces
        .find_by_id(workspace_id)
        .await
        .ok()
        .flatten();
    let trust_level = workspace
        .as_ref()
        .map(|ws| format!("{:?}", ws.trust_level).to_lowercase());

    let required_attestation_level = get_repo_required_attestation_level(state, repo_id).await;
    let strategy_constraints = constraint_evaluator::derive_strategy_constraints(
        &signed_input.content,
        trust_level.as_deref(),
        required_attestation_level,
    );

    // Collect all constraints from the full chain (§4.3 additive accumulation).
    let (_root, explicit_from_chain, gate_from_chain) =
        crate::git_http::accumulate_chain_constraints(&attestations);

    let all_constraints = constraint_evaluator::collect_all_constraints(
        &explicit_from_chain,
        &strategy_constraints,
        &gate_from_chain,
    );

    if all_constraints.is_empty() {
        return Ok(());
    }

    let target_ctx = TargetContext {
        repo_id: repo_id.to_string(),
        workspace_id: workspace_id.to_string(),
        branch: target_branch.to_string(),
        default_branch: default_branch.to_string(),
    };

    let ci = ConstraintInput {
        input: &signed_input.content,
        output: &diff_info,
        agent: &agent_ctx,
        target: &target_ctx,
        action: Action::Merge,
    };

    let ctx = match constraint_evaluator::build_cel_context(&ci) {
        Ok(c) => c,
        Err(e) => {
            return Err(format!("merge blocked: failed to build CEL context: {e}"));
        }
    };

    let result = constraint_evaluator::evaluate_all(&all_constraints, &ctx);

    log_constraint_results(
        &result,
        &task_id,
        repo_id,
        &agent_id,
        "merge",
        all_constraints.len(),
    );

    if !result.valid {
        let violations = extract_violations(&result, &all_constraints);
        let context_snapshot = serde_json::json!({
            "input": &signed_input.content,
            "output": &diff_info,
            "agent": &agent_ctx,
            "target": &target_ctx,
            "action": "merge",
        });
        emit_constraint_violations(
            state,
            &violations,
            &attestation_id,
            repo_id,
            &agent_id,
            workspace_id,
            "merge",
            &context_snapshot,
        )
        .await;
        // §7.5: priority 2 for all constraint violation notifications.
        create_violation_notifications(
            state,
            &violations,
            &task_id,
            repo_id,
            workspace_id,
            "merge",
        )
        .await;

        let details: Vec<String> = violations
            .iter()
            .map(|v| format!("  - {}: {}", v.constraint_name, v.message))
            .collect();
        return Err(format!(
            "merge blocked: {} constraint(s) failed:\n{}",
            violations.len(),
            details.join("\n")
        ));
    }

    Ok(())
}

/// Run constraint evaluation at merge time (§8 Phase 2).
///
/// Collects all constraints (explicit + strategy-implied + gate), evaluates
/// against the merged diff, logs results, and emits violations.
/// Audit-only: does NOT block the merge.
pub async fn evaluate_merge_constraints(
    state: &AppState,
    mr_id: &str,
    repo_id: &str,
    repo_path: &str,
    merge_commit_sha: &str,
    workspace_id: &Id,
    _source_branch: &str,
    target_branch: &str,
    default_branch: &str,
) {
    // Look up the MR to find the agent and task.
    let mr = match state
        .merge_requests
        .find_by_id(&Id::new(mr_id))
        .await
        .ok()
        .flatten()
    {
        Some(m) => m,
        None => return,
    };

    let agent_id = mr
        .author_agent_id
        .as_ref()
        .map(|id| id.to_string())
        .unwrap_or_default();

    // Resolve the agent's task_id.
    let task_id = if !agent_id.is_empty() {
        let agent = state
            .agents
            .find_by_id(&Id::new(&agent_id))
            .await
            .ok()
            .flatten();
        agent.and_then(|a| a.current_task_id.map(|id| id.to_string()))
    } else {
        None
    };

    let Some(task_id) = task_id else {
        tracing::debug!(
            mr_id = %mr_id,
            "no task_id found for merge constraint evaluation (Phase 2)"
        );
        return;
    };

    // Look up attestation chain for this task.
    let attestations = match state.chain_attestations.find_by_task(&task_id).await {
        Ok(atts) if !atts.is_empty() => atts,
        Ok(_) => {
            tracing::debug!(
                mr_id = %mr_id,
                task_id = %task_id,
                "no attestation chain found for merge (Phase 2)"
            );
            return;
        }
        Err(e) => {
            warn!(
                mr_id = %mr_id,
                task_id = %task_id,
                error = %e,
                "failed to query attestation chain for merge (Phase 2)"
            );
            return;
        }
    };

    // Find root SignedInput, along with its attestation_id.
    let signed_input_with_id = attestations.iter().find_map(|att| match &att.input {
        AttestationInput::Signed(si) => Some((si, &att.id)),
        _ => None,
    });

    let Some((signed_input, attestation_id)) = signed_input_with_id else {
        tracing::debug!(
            mr_id = %mr_id,
            "no SignedInput found for merge constraint evaluation (Phase 2)"
        );
        return;
    };
    let attestation_id = attestation_id.clone();

    // §2.4 Context binding (replay prevention), audit-only mirror: record
    // mismatches for Phase 2 observability without blocking the merge.
    // Enforcement lives in `enforce_merge_constraints`.
    {
        let (task_spec_sha, task_generation) =
            resolve_task_binding_context(state, &task_id).await;
        let binding_result = crate::git_http::verify_context_binding(
            signed_input,
            repo_id,
            workspace_id.as_str(),
            task_spec_sha.as_deref(),
            task_generation,
        );
        if !binding_result.valid {
            for child in &binding_result.children {
                if !child.valid {
                    warn!(
                        mr_id = %mr_id,
                        task_id = %task_id,
                        repo_id = %repo_id,
                        agent_id = %agent_id,
                        category = "Provenance",
                        event = "attestation.context_binding_mismatch",
                        binding = %child.label,
                        message = %child.message,
                        "attestation.context_binding_mismatch (audit-only): {}",
                        child.message
                    );
                }
            }
        }
    }

    // Compute diff for the merge commit.
    let diff_info = match compute_commit_diff(repo_path, merge_commit_sha).await {
        Some(d) => d,
        None => {
            warn!(
                mr_id = %mr_id,
                "failed to compute merge diff for constraint evaluation"
            );
            return;
        }
    };

    // Build agent context from the agent record, workspace meta-spec set, and
    // workload attestation.
    let agent_ctx = build_agent_context(state, &agent_id, &task_id, workspace_id).await;

    // Look up workspace trust level.
    let workspace = state
        .workspaces
        .find_by_id(workspace_id)
        .await
        .ok()
        .flatten();
    let trust_level = workspace
        .as_ref()
        .map(|ws| format!("{:?}", ws.trust_level).to_lowercase());

    // Derive strategy-implied constraints, including repo attestation level policy.
    let required_attestation_level = get_repo_required_attestation_level(state, repo_id).await;
    let strategy_constraints = constraint_evaluator::derive_strategy_constraints(
        &signed_input.content,
        trust_level.as_deref(),
        required_attestation_level,
    );

    // Collect gate constraints from attestation chain.
    let gate_constraints: Vec<gyre_common::attestation::GateConstraint> = attestations
        .iter()
        .flat_map(|att| {
            att.output
                .gate_results
                .iter()
                .filter_map(|gr| gr.constraint.clone())
        })
        .collect();

    let all_constraints = constraint_evaluator::collect_all_constraints(
        &signed_input.output_constraints,
        &strategy_constraints,
        &gate_constraints,
    );

    if all_constraints.is_empty() {
        return;
    }

    // Build CEL evaluation context.
    let target_ctx = TargetContext {
        repo_id: repo_id.to_string(),
        workspace_id: workspace_id.to_string(),
        branch: target_branch.to_string(),
        default_branch: default_branch.to_string(),
    };

    let ci = ConstraintInput {
        input: &signed_input.content,
        output: &diff_info,
        agent: &agent_ctx,
        target: &target_ctx,
        action: Action::Merge,
    };

    let ctx = match constraint_evaluator::build_cel_context(&ci) {
        Ok(c) => c,
        Err(e) => {
            warn!(
                mr_id = %mr_id,
                error = %e,
                "failed to build CEL context for merge constraint evaluation"
            );
            return;
        }
    };

    let result = constraint_evaluator::evaluate_all(&all_constraints, &ctx);

    log_constraint_results(
        &result,
        &task_id,
        repo_id,
        &agent_id,
        "merge",
        all_constraints.len(),
    );

    if !result.valid {
        let violations = extract_violations(&result, &all_constraints);
        // Build context_snapshot from the CEL context components (§7.5).
        let context_snapshot = serde_json::json!({
            "input": &signed_input.content,
            "output": &diff_info,
            "agent": &agent_ctx,
            "target": &target_ctx,
            "action": "merge",
        });
        emit_constraint_violations(
            state,
            &violations,
            &attestation_id,
            repo_id,
            &agent_id,
            workspace_id,
            "merge",
            &context_snapshot,
        )
        .await;
        create_violation_notifications(
            state,
            &violations,
            &task_id,
            repo_id,
            workspace_id,
            "merge",
        )
        .await;
    }
}


/// Resolve the task context needed for §2.4 context binding: the task's
/// current spec SHA (approved SHA from the spec approval history if the task
/// has a spec_path, else the ledger's current SHA) and the task's current
/// deployment generation.
///
/// Returns `(None, None)` when the task itself cannot be resolved — callers
/// treating that as "no binding data" must still have the repo/workspace
/// bindings enforced against the real push target.
async fn resolve_task_binding_context(
    state: &AppState,
    task_id: &str,
) -> (Option<String>, Option<u32>) {
    let task = match state.tasks.find_by_id(&Id::new(task_id)).await {
        Ok(Some(t)) => t,
        _ => return (None, None),
    };

    let generation = Some(task.generation);

    // Resolve the currently approved spec SHA for the task's spec_path.
    // Preference order: newest active (non-revoked) approval event for the
    // path — that is the SHA a human actually signed — falling back to the
    // ledger's current SHA.
    let spec_sha = match task.spec_path.as_deref() {
        None => None,
        Some(path) => {
            let approved = state
                .spec_approval_history
                .list_by_path(path)
                .await
                .unwrap_or_default()
                .into_iter()
                .filter(|e| e.revoked_at.is_none())
                .max_by_key(|e| e.approved_at)
                .map(|e| e.spec_sha);
            match approved {
                Some(sha) => Some(sha),
                None => state
                    .spec_ledger
                    .find_by_path(path)
                    .await
                    .ok()
                    .flatten()
                    .map(|e| e.current_sha),
            }
        }
    };

    (spec_sha, generation)
}

/// Human-readable summary of a failed context binding naming every failing
/// binding (e.g. "context_binding.repo_id: signed repo_id repo-OTHER ...") so
/// rejection messages and audit trails identify which replay-prevention
/// binding actually failed.
fn context_binding_failure_summary(binding_result: &VerificationResult) -> String {
    binding_result
        .children
        .iter()
        .filter(|c| !c.valid)
        .map(|c| format!("{}: {}", c.label, c.message))
        .collect::<Vec<_>>()
        .join("; ")
}
/// Emit the audit trail for a §2.4 context-binding rejection: the
/// `attestation.chain_invalid` audit event (§7.7) plus one
/// `ConstraintViolation` Event-tier message per failing binding (§7.5),
/// reusing the existing violation emission path.
async fn emit_context_binding_rejection(
    state: &AppState,
    binding_result: &VerificationResult,
    attestation_id: &str,
    task_id: &str,
    repo_id: &str,
    agent_id: &str,
    workspace_id: &Id,
    action: &str,
) {
    // §7.7: attestation.chain_invalid audit event — the chain is valid
    // cryptographically but does not bind to this context.
    warn!(
        task_id = %task_id,
        repo_id = %repo_id,
        agent_id = %agent_id,
        category = "Provenance",
        event = "attestation.chain_invalid",
        message = %binding_result.message,
        "attestation.chain_invalid: {action}-time context binding failed"
    );

    let failed: Vec<&VerificationResult> =
        binding_result.children.iter().filter(|c| !c.valid).collect();

    let violations: Vec<ConstraintViolationInfo> = failed
        .iter()
        .map(|c| ConstraintViolationInfo {
            constraint_name: c.label.clone(),
            expression: "context_binding(signed_input, target)".to_string(),
            message: c.message.clone(),
        })
        .collect();

    let context_snapshot = serde_json::json!({
        "binding_result": binding_result,
        "action": action,
    });

    emit_constraint_violations(
        state,
        &violations,
        attestation_id,
        repo_id,
        agent_id,
        workspace_id,
        action,
        &context_snapshot,
    )
    .await;
    create_violation_notifications(state, &violations, task_id, repo_id, workspace_id, action)
        .await;
}

// ── Helpers ──────────────────────────────────────────────────────────────

/// Derive the agent's attestation level from workload attestation data.
///
/// Levels (supply-chain.md §2):
///   - 0: No workload attestation record exists
///   - 1: Workload attestation exists but no stack fingerprint (raw push)
///   - 2: Stack fingerprint present — self-reported via CLI
///   - 3: Stack fingerprint + container_id + image_hash — Gyre-managed runtime
fn derive_attestation_level(
    workload: Option<&crate::workload_attestation::WorkloadAttestation>,
) -> i64 {
    let Some(att) = workload else {
        return 0;
    };
    if att.stack_fingerprint.is_empty() {
        return 1;
    }
    let has_container = att.container_id.as_deref().is_some_and(|c| !c.is_empty());
    let has_image = att.image_hash.as_deref().is_some_and(|h| !h.is_empty());
    if has_container && has_image {
        3
    } else {
        2
    }
}

/// Look up the repo's required minimum attestation level from its stack policy.
///
/// Parses the `repo_stack_policies` KV entry as structured JSON containing
/// `{"fingerprint": "...", "required_level": N}`. Returns the `required_level`
/// from the policy, or `None` when no stack policy is configured.
/// Handles legacy plain-string entries by defaulting to Level 2
/// (supply-chain.md §2, Policy per Level).
pub(crate) async fn get_repo_required_attestation_level(
    state: &AppState,
    repo_id: &str,
) -> Option<i64> {
    state
        .kv_store
        .kv_get("repo_stack_policies", repo_id)
        .await
        .ok()
        .flatten()
        .map(|value| crate::api::stack_attest::parse_stack_policy(&value).required_level)
}

/// Build an `AgentContext` from the agent record, JWT claims, workspace
/// meta-spec set, and workload attestation data.
///
/// Populates `meta_spec_set_sha` from the workspace's current meta-spec set
/// (SHA256 of the serialized JSON). Derives `attestation_level` from the
/// agent's workload attestation record (supply-chain.md §2).
pub(crate) async fn build_agent_context(
    state: &AppState,
    agent_id: &str,
    task_id: &str,
    workspace_id: &Id,
) -> AgentContext {
    let agent = state
        .agents
        .find_by_id(&Id::new(agent_id))
        .await
        .ok()
        .flatten();

    // Persona comes from the agent's JWT claims at spawn time, not from the Agent
    // struct. For Phase 2, we derive it from the KV store where agent claims are
    // cached, or leave empty.
    let persona = state
        .kv_store
        .kv_get("agent_personas", agent_id)
        .await
        .ok()
        .flatten()
        .unwrap_or_default();

    // Meta-spec set SHA: compute from the workspace's current meta-spec set.
    // The agent runs in this workspace, so its meta-spec set is the workspace's
    // current set. If no set is configured, the SHA is empty (correctly fails
    // against a non-empty input SHA, indicating the set has changed).
    let meta_spec_set_sha =
        crate::compute_meta_spec_set_sha(state.meta_spec_sets.as_ref(), workspace_id).await;

    // Workload attestation: populate container_id, image_hash, and stack_hash
    // from the KV-cached workload attestation record if available.
    let workload = state
        .kv_store
        .kv_get("workload_attestations", agent_id)
        .await
        .ok()
        .flatten()
        .and_then(|json| {
            serde_json::from_str::<crate::workload_attestation::WorkloadAttestation>(&json).ok()
        });

    let attestation_level = derive_attestation_level(workload.as_ref());

    let (stack_hash, container_id, image_hash) = match &workload {
        Some(att) => (
            att.stack_fingerprint.clone(),
            att.container_id.clone().unwrap_or_default(),
            att.image_hash.clone().unwrap_or_default(),
        ),
        None => (String::new(), String::new(), String::new()),
    };

    AgentContext {
        id: agent_id.to_string(),
        persona,
        stack_hash,
        attestation_level,
        meta_spec_set_sha,
        spawned_by: agent
            .as_ref()
            .and_then(|a| a.spawned_by.clone())
            .unwrap_or_default(),
        task_id: task_id.to_string(),
        container_id,
        image_hash,
    }
}

/// Compute the diff for a push (across all ref updates).
async fn compute_push_diff(
    repo_path: &str,
    ref_updates: &[(String, String, String)],
) -> Option<OutputContext> {
    let git_bin = std::env::var("GYRE_GIT_PATH").unwrap_or_else(|_| "git".to_string());
    let mut all_changed = Vec::new();
    let mut all_added = Vec::new();
    let mut all_deleted = Vec::new();
    let mut total_insertions = 0u64;
    let mut total_deletions = 0u64;
    let mut commit_message = String::new();
    let mut commit_sha = String::new();

    for (old_sha, new_sha, _refname) in ref_updates {
        let range = if old_sha.starts_with("00000000") {
            new_sha.clone()
        } else {
            format!("{old_sha}..{new_sha}")
        };

        // Get changed files.
        let diff_out = tokio::process::Command::new(&git_bin)
            .arg("-C")
            .arg(repo_path)
            .arg("diff")
            .arg("--name-status")
            .arg(&range)
            .output()
            .await
            .ok()?;

        if diff_out.status.success() {
            let text = String::from_utf8_lossy(&diff_out.stdout);
            for line in text.lines() {
                let parts: Vec<&str> = line.splitn(2, '\t').collect();
                if parts.len() == 2 {
                    let status = parts[0].chars().next().unwrap_or(' ');
                    let file = parts[1].to_string();
                    match status {
                        'A' => all_added.push(file.clone()),
                        'D' => all_deleted.push(file.clone()),
                        _ => {}
                    }
                    all_changed.push(file);
                }
            }
        }

        // Get diff stats.
        let stat_out = tokio::process::Command::new(&git_bin)
            .arg("-C")
            .arg(repo_path)
            .arg("diff")
            .arg("--shortstat")
            .arg(&range)
            .output()
            .await
            .ok()?;

        if stat_out.status.success() {
            let stat_text = String::from_utf8_lossy(&stat_out.stdout);
            let (ins, del) = parse_shortstat(&stat_text);
            total_insertions += ins;
            total_deletions += del;
        }

        // Get commit message (latest).
        let msg_out = tokio::process::Command::new(&git_bin)
            .arg("-C")
            .arg(repo_path)
            .arg("log")
            .arg("-1")
            .arg("--format=%s")
            .arg(new_sha)
            .output()
            .await
            .ok()?;

        if msg_out.status.success() {
            commit_message = String::from_utf8_lossy(&msg_out.stdout).trim().to_string();
        }

        commit_sha = new_sha.clone();
    }

    Some(OutputContext {
        changed_files: all_changed,
        added_files: all_added,
        deleted_files: all_deleted,
        diff_stats: DiffStatsContext {
            insertions: total_insertions,
            deletions: total_deletions,
        },
        commit_message,
        commit_sha,
    })
}

/// Compute the diff for a merge commit (parent comparison).
pub(crate) async fn compute_commit_diff(
    repo_path: &str,
    commit_sha: &str,
) -> Option<OutputContext> {
    let git_bin = std::env::var("GYRE_GIT_PATH").unwrap_or_else(|_| "git".to_string());

    // Get changed files relative to first parent.
    let diff_out = tokio::process::Command::new(&git_bin)
        .arg("-C")
        .arg(repo_path)
        .arg("diff")
        .arg("--name-status")
        .arg(format!("{commit_sha}^..{commit_sha}"))
        .output()
        .await
        .ok()?;

    let mut changed = Vec::new();
    let mut added = Vec::new();
    let mut deleted = Vec::new();

    if diff_out.status.success() {
        let text = String::from_utf8_lossy(&diff_out.stdout);
        for line in text.lines() {
            let parts: Vec<&str> = line.splitn(2, '\t').collect();
            if parts.len() == 2 {
                let status = parts[0].chars().next().unwrap_or(' ');
                let file = parts[1].to_string();
                match status {
                    'A' => added.push(file.clone()),
                    'D' => deleted.push(file.clone()),
                    _ => {}
                }
                changed.push(file);
            }
        }
    }

    // Get diff stats.
    let stat_out = tokio::process::Command::new(&git_bin)
        .arg("-C")
        .arg(repo_path)
        .arg("diff")
        .arg("--shortstat")
        .arg(format!("{commit_sha}^..{commit_sha}"))
        .output()
        .await
        .ok()?;

    let (insertions, deletions) = if stat_out.status.success() {
        let stat_text = String::from_utf8_lossy(&stat_out.stdout);
        parse_shortstat(&stat_text)
    } else {
        (0, 0)
    };

    // Get commit message.
    let msg_out = tokio::process::Command::new(&git_bin)
        .arg("-C")
        .arg(repo_path)
        .arg("log")
        .arg("-1")
        .arg("--format=%s")
        .arg(commit_sha)
        .output()
        .await
        .ok()?;

    let commit_message = if msg_out.status.success() {
        String::from_utf8_lossy(&msg_out.stdout).trim().to_string()
    } else {
        String::new()
    };

    Some(OutputContext {
        changed_files: changed,
        added_files: added,
        deleted_files: deleted,
        diff_stats: DiffStatsContext {
            insertions,
            deletions,
        },
        commit_message,
        commit_sha: commit_sha.to_string(),
    })
}

/// Parse `git diff --shortstat` output for insertions/deletions.
pub fn parse_shortstat(text: &str) -> (u64, u64) {
    let mut insertions = 0u64;
    let mut deletions = 0u64;

    for part in text.split(',') {
        let part = part.trim();
        if part.contains("insertion") {
            if let Some(n) = part.split_whitespace().next().and_then(|s| s.parse().ok()) {
                insertions = n;
            }
        } else if part.contains("deletion") {
            if let Some(n) = part.split_whitespace().next().and_then(|s| s.parse().ok()) {
                deletions = n;
            }
        }
    }

    (insertions, deletions)
}

/// Log constraint evaluation results (§7.7 audit events).
fn log_constraint_results(
    result: &VerificationResult,
    task_id: &str,
    repo_id: &str,
    agent_id: &str,
    action: &str,
    constraint_count: usize,
) {
    if result.valid {
        // §7.7: attestation.verified audit event.
        info!(
            task_id = %task_id,
            repo_id = %repo_id,
            agent_id = %agent_id,
            action = %action,
            category = "Provenance",
            event = "attestation.verified",
            constraints_evaluated = constraint_count,
            "attestation.verified: all {} constraint(s) passed",
            constraint_count
        );
    } else {
        // §7.7: attestation.constraint_failed audit event.
        warn!(
            task_id = %task_id,
            repo_id = %repo_id,
            agent_id = %agent_id,
            action = %action,
            category = "Provenance",
            event = "attestation.constraint_failed",
            constraints_evaluated = constraint_count,
            message = %result.message,
            "attestation.constraint_failed: constraint evaluation failed"
        );
    }
}

/// Extract violation details from a failed VerificationResult.
fn extract_violations(
    result: &VerificationResult,
    all_constraints: &[OutputConstraint],
) -> Vec<ConstraintViolationInfo> {
    let mut violations = Vec::new();
    for child in &result.children {
        if !child.valid {
            // Find the matching constraint to get the expression.
            let expression = all_constraints
                .iter()
                .find(|c| c.name == child.label)
                .map(|c| c.expression.clone())
                .unwrap_or_default();
            violations.push(ConstraintViolationInfo {
                constraint_name: child.label.clone(),
                expression,
                message: child.message.clone(),
            });
        }
    }
    violations
}

/// Emit `ConstraintViolation` Event-tier messages for each violation (§7.5).
///
/// Payload conforms to the spec-defined ConstraintViolation schema:
/// attestation_id, constraint_name, expression, context_snapshot, action,
/// agent_id, repo_id, timestamp.
async fn emit_constraint_violations(
    state: &AppState,
    violations: &[ConstraintViolationInfo],
    attestation_id: &str,
    repo_id: &str,
    agent_id: &str,
    workspace_id: &Id,
    action: &str,
    context_snapshot: &serde_json::Value,
) {
    // caller-scope:ok — violations are pre-computed for same workspace, broadcasts are workspace-scoped
    let now = crate::api::now_secs();

    for violation in violations {
        let payload = serde_json::json!({
            "attestation_id": attestation_id,
            "constraint_name": violation.constraint_name,
            "expression": violation.expression,
            "context_snapshot": context_snapshot,
            "action": action,
            "agent_id": agent_id,
            "repo_id": repo_id,
            "timestamp": now,
        });

        // Broadcast to workspace.
        state
            .emit_event(
                Some(workspace_id.clone()),
                Destination::Workspace(workspace_id.clone()),
                MessageKind::ConstraintViolation,
                Some(payload.clone()),
            )
            .await;

        // Directed to the author agent.
        if !agent_id.is_empty() {
            state
                .emit_event(
                    Some(workspace_id.clone()),
                    Destination::Agent(Id::new(agent_id)),
                    MessageKind::ConstraintViolation,
                    Some(payload),
                )
                .await;
        }
    }
}

/// Create priority-2 Inbox notifications for constraint violations (§7.5).
async fn create_violation_notifications(
    state: &AppState,
    violations: &[ConstraintViolationInfo],
    task_id: &str,
    repo_id: &str,
    workspace_id: &Id,
    action: &str,
) {
    // Get workspace members (Admin + Developer roles).
    let members = state
        .workspace_memberships
        .list_by_workspace(workspace_id)
        .await
        .unwrap_or_default();

    // Resolve tenant_id from workspace.
    let tenant_id = state
        .workspaces
        .find_by_id(workspace_id)
        .await
        .ok()
        .flatten()
        .map(|ws| ws.tenant_id.to_string())
        .unwrap_or_else(|| "default".to_string());

    let now = crate::api::now_secs() as i64;

    for violation in violations {
        let title = format!(
            "Constraint violation at {action}: {}",
            violation.constraint_name
        );

        for member in &members {
            if !matches!(
                member.role,
                gyre_domain::WorkspaceRole::Admin
                    | gyre_domain::WorkspaceRole::Developer
                    | gyre_domain::WorkspaceRole::Owner
            ) {
                continue;
            }

            let notif_id = Id::new(uuid::Uuid::new_v4().to_string());
            let mut notif = gyre_common::Notification::new(
                notif_id,
                workspace_id.clone(),
                member.user_id.clone(),
                NotificationType::ConstraintViolation,
                title.clone(),
                &tenant_id,
                now,
            );
            notif.body = Some(
                serde_json::json!({
                    "expression": violation.expression,
                    "message": violation.message,
                    "task_id": task_id,
                    "action": action,
                })
                .to_string(),
            );
            notif.repo_id = Some(repo_id.to_string());
            notif.entity_ref = Some(format!("task:{task_id}"));

            if let Err(e) = state.notifications.create(&notif).await {
                warn!(
                    constraint = %violation.constraint_name,
                    error = %e,
                    "failed to create constraint violation notification"
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gyre_common::attestation::{InputContent, PersonaRef, ScopeConstraint, SignedInput};
    use gyre_common::message::MessageKind;

    #[test]
    fn parse_shortstat_insertions_and_deletions() {
        let stat = " 3 files changed, 42 insertions(+), 7 deletions(-)\n";
        let (ins, del) = parse_shortstat(stat);
        assert_eq!(ins, 42);
        assert_eq!(del, 7);
    }

    #[test]
    fn parse_shortstat_insertions_only() {
        let stat = " 1 file changed, 10 insertions(+)\n";
        let (ins, del) = parse_shortstat(stat);
        assert_eq!(ins, 10);
        assert_eq!(del, 0);
    }

    #[test]
    fn parse_shortstat_deletions_only() {
        let stat = " 2 files changed, 5 deletions(-)\n";
        let (ins, del) = parse_shortstat(stat);
        assert_eq!(ins, 0);
        assert_eq!(del, 5);
    }

    #[test]
    fn parse_shortstat_empty() {
        let (ins, del) = parse_shortstat("");
        assert_eq!(ins, 0);
        assert_eq!(del, 0);
    }

    #[test]
    fn extract_violations_from_result() {
        let result = VerificationResult {
            label: "constraint evaluation".to_string(),
            valid: false,
            message: "constraint failed: scope check".to_string(),
            children: vec![
                VerificationResult {
                    label: "persona check".to_string(),
                    valid: true,
                    message: "passed".to_string(),
                    children: vec![],
                },
                VerificationResult {
                    label: "scope check".to_string(),
                    valid: false,
                    message: "constraint failed: scope check".to_string(),
                    children: vec![],
                },
            ],
        };

        let constraints = vec![
            OutputConstraint {
                name: "persona check".to_string(),
                expression: r#"agent.persona == "security""#.to_string(),
            },
            OutputConstraint {
                name: "scope check".to_string(),
                expression: r#"output.changed_files.all(f, f.startsWith("src/"))"#.to_string(),
            },
        ];

        let violations = extract_violations(&result, &constraints);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].constraint_name, "scope check");
        assert!(violations[0]
            .expression
            .contains("output.changed_files.all"));
    }

    // ── Integration: push-time constraint evaluation ────────────────────

    fn make_signed_input(
        allowed_paths: Vec<&str>,
        forbidden_paths: Vec<&str>,
        persona_constraints: Vec<&str>,
        output_constraints: Vec<OutputConstraint>,
    ) -> SignedInput {
        let kb = gyre_common::KeyBinding {
            public_key: vec![1; 32],
            user_identity: "user:jsell".to_string(),
            issuer: "https://keycloak.example.com".to_string(),
            trust_anchor_id: "tenant-keycloak".to_string(),
            issued_at: 1_700_000_000,
            expires_at: u64::MAX,
            user_signature: vec![10],
            platform_countersign: vec![20],
        };

        SignedInput {
            content: InputContent {
                spec_path: "specs/system/payments.md".to_string(),
                spec_sha: "abc12345".to_string(),
                workspace_id: "ws-1".to_string(),
                repo_id: "repo-1".to_string(),
                persona_constraints: persona_constraints
                    .into_iter()
                    .map(|n| PersonaRef {
                        name: n.to_string(),
                    })
                    .collect(),
                meta_spec_set_sha: "def456".to_string(),
                scope: ScopeConstraint {
                    allowed_paths: allowed_paths.into_iter().map(String::from).collect(),
                    forbidden_paths: forbidden_paths.into_iter().map(String::from).collect(),
                },
            },
            output_constraints,
            valid_until: u64::MAX,
            expected_generation: None,
            signature: vec![30],
            key_binding: kb,
        }
    }

    fn make_attestation(signed_input: SignedInput, task_id: &str) -> gyre_common::Attestation {
        gyre_common::Attestation {
            id: "att-test".to_string(),
            input: AttestationInput::Signed(signed_input),
            output: gyre_common::AttestationOutput {
                content_hash: vec![40],
                commit_sha: "sha-abc".to_string(),
                agent_signature: None,
                gate_results: vec![],
            },
            metadata: gyre_common::AttestationMetadata {
                created_at: 1_700_000_000,
                workspace_id: "ws-1".to_string(),
                repo_id: "repo-1".to_string(),
                task_id: task_id.to_string(),
                agent_id: "agent-1".to_string(),
                chain_depth: 0,
            },
        }
    }

    #[tokio::test]
    async fn evaluate_push_no_attestation_chain_is_noop() {
        let state = crate::mem::test_state();
        let tmp = tempfile::tempdir().unwrap();
        let repo_path = tmp.path().to_str().unwrap();

        // Subscribe to broadcast before call.
        let mut rx = state.message_broadcast_tx.subscribe();

        // No attestation stored → should return early without emitting events.
        evaluate_push_constraints(
            &state,
            "TASK-99",
            "repo-1",
            repo_path,
            "agent-1",
            &Id::new("ws-1"),
            &[],
            "main",
        )
        .await;

        // Verify: no events emitted (graceful degradation, no attestation chain).
        assert!(
            rx.try_recv().is_err(),
            "no events should be emitted when there is no attestation chain"
        );
    }

    /// Create a temporary git repository with two commits.
    ///
    /// Returns (tempdir, initial_commit_sha, second_commit_sha). The second
    /// commit adds `src/payments/handler.rs` so tests can exercise scope
    /// constraints against known file paths.
    fn init_test_git_repo() -> (tempfile::TempDir, String, String) {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();

        // git init
        let status = std::process::Command::new("git")
            .args(["init"])
            .current_dir(repo)
            .output()
            .unwrap();
        assert!(status.status.success(), "git init failed");

        // Configure user for commits.
        std::process::Command::new("git")
            .args(["config", "user.email", "test@gyre.dev"])
            .current_dir(repo)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(repo)
            .output()
            .unwrap();

        // Initial commit with a README.
        std::fs::write(repo.join("README.md"), "# test repo\n").unwrap();
        std::process::Command::new("git")
            .args(["add", "README.md"])
            .current_dir(repo)
            .output()
            .unwrap();
        let out = std::process::Command::new("git")
            .args(["commit", "-m", "initial commit"])
            .current_dir(repo)
            .output()
            .unwrap();
        assert!(out.status.success(), "initial commit failed");
        let initial_sha = get_head_sha(repo);

        // Second commit: add src/payments/handler.rs.
        std::fs::create_dir_all(repo.join("src/payments")).unwrap();
        std::fs::write(repo.join("src/payments/handler.rs"), "pub fn handle() {}\n").unwrap();
        std::process::Command::new("git")
            .args(["add", "src/payments/handler.rs"])
            .current_dir(repo)
            .output()
            .unwrap();
        let out = std::process::Command::new("git")
            .args(["commit", "-m", "add payment handler"])
            .current_dir(repo)
            .output()
            .unwrap();
        assert!(out.status.success(), "second commit failed");
        let second_sha = get_head_sha(repo);

        (tmp, initial_sha, second_sha)
    }

    fn get_head_sha(repo: &std::path::Path) -> String {
        let out = std::process::Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(repo)
            .output()
            .unwrap();
        String::from_utf8(out.stdout).unwrap().trim().to_string()
    }

    /// Compute the SHA256 of a meta-spec set JSON string (same algorithm as
    /// `compute_meta_spec_set_sha`).
    fn meta_spec_sha(json: &str) -> String {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(json.as_bytes());
        hex::encode(hasher.finalize())
    }

    /// Set up the state with a workspace meta-spec set and agent persona so
    /// that `build_agent_context` populates these fields correctly during
    /// constraint evaluation.
    async fn seed_agent_context(
        state: &crate::AppState,
        workspace_id: &str,
        agent_id: &str,
    ) -> String {
        let ws_id = Id::new(workspace_id);

        // Store a meta-spec set for the workspace.
        let meta_set_json = r#"{"workspace_id":"ws-1","personas":{}}"#;
        state
            .meta_spec_sets
            .upsert(&ws_id, meta_set_json)
            .await
            .unwrap();
        let sha = meta_spec_sha(meta_set_json);

        // Store agent persona in KV (same key as build_agent_context reads).
        state
            .kv_store
            .kv_set("agent_personas", agent_id, "security".to_string())
            .await
            .unwrap();

        sha
    }

    #[tokio::test]
    async fn evaluate_push_with_attestation_chain_runs_constraints() {
        let (tmp, initial_sha, second_sha) = init_test_git_repo();
        let repo_path = tmp.path().to_str().unwrap();

        let state = crate::mem::test_state();

        // Seed workspace meta-spec set and agent persona so build_agent_context
        // produces valid values matching the InputContent.
        let meta_sha = seed_agent_context(&state, "ws-1", "agent-1").await;

        // Store a SignedInput attestation with a constraint that FAILS when
        // the commit changes any file (the test commit adds a file, so
        // output.changed_files.size() > 0 → the constraint output.changed_files.size() == 0 fails).
        let si = make_signed_input(
            vec!["src/payments/**"],
            vec![],
            vec!["security"],
            vec![OutputConstraint {
                name: "no-changes check".to_string(),
                expression: "output.changed_files.size() == 0".to_string(),
            }],
        );
        // Update the InputContent's meta_spec_set_sha to match what the workspace produces.
        let mut si = si;
        si.content.meta_spec_set_sha = meta_sha;

        let att = make_attestation(si, "TASK-100");
        state.chain_attestations.save(&att).await.unwrap();

        // Subscribe to the broadcast channel before evaluation.
        let mut rx = state.message_broadcast_tx.subscribe();

        // Run push evaluation with real repo path and real commit SHAs.
        evaluate_push_constraints(
            &state,
            "TASK-100",
            "repo-1",
            repo_path,
            "agent-1",
            &Id::new("ws-1"),
            &[(initial_sha, second_sha, "refs/heads/main".to_string())],
            "main",
        )
        .await;

        // Assert: ConstraintViolation events emitted to broadcast channel.
        // The "no-changes check" should fail because the push adds a file.
        let msg = rx
            .try_recv()
            .expect("should have received ConstraintViolation workspace broadcast");
        assert_eq!(msg.kind, MessageKind::ConstraintViolation);
        let payload = msg.payload.as_ref().unwrap();
        assert_eq!(payload["attestation_id"], "att-test");
        assert_eq!(payload["action"], "push");
        assert!(
            payload["constraint_name"]
                .as_str()
                .unwrap()
                .contains("no-changes check")
                || payload["constraint_name"]
                    .as_str()
                    .unwrap()
                    .contains("scope")
                || payload["constraint_name"]
                    .as_str()
                    .unwrap()
                    .contains("persona")
                || payload["constraint_name"]
                    .as_str()
                    .unwrap()
                    .contains("meta-spec"),
            "violation event should reference a known constraint name, got: {}",
            payload["constraint_name"]
        );

        // Verify the agent-directed message was also sent.
        let msg2 = rx
            .try_recv()
            .expect("should have received agent-directed ConstraintViolation");
        assert_eq!(msg2.kind, MessageKind::ConstraintViolation);
    }

    #[tokio::test]
    async fn emit_constraint_violations_creates_events() {
        let state = crate::mem::test_state();
        let ws_id = Id::new("ws-1");

        // Subscribe to broadcast before emitting.
        let mut rx = state.message_broadcast_tx.subscribe();

        let violations = vec![ConstraintViolationInfo {
            constraint_name: "scope check".to_string(),
            expression: r#"output.changed_files.all(f, f.startsWith("src/"))"#.to_string(),
            message: "constraint failed: scope check".to_string(),
        }];

        let context_snapshot = serde_json::json!({"action": "push"});
        emit_constraint_violations(
            &state,
            &violations,
            "att-test-1",
            "repo-1",
            "agent-1",
            &ws_id,
            "push",
            &context_snapshot,
        )
        .await;

        // Verify events were emitted via broadcast channel.
        // Each violation emits 2 messages: workspace broadcast + agent-directed.
        let msg1 = rx
            .try_recv()
            .expect("should have received workspace broadcast");
        assert_eq!(msg1.kind, MessageKind::ConstraintViolation);
        let payload = msg1.payload.as_ref().unwrap();
        assert_eq!(payload["attestation_id"], "att-test-1");
        assert_eq!(payload["constraint_name"], "scope check");
        assert_eq!(payload["context_snapshot"]["action"], "push");
        assert_eq!(payload["action"], "push");
        assert_eq!(payload["agent_id"], "agent-1");

        let msg2 = rx.try_recv().expect("should have received agent-directed");
        assert_eq!(msg2.kind, MessageKind::ConstraintViolation);
    }

    #[tokio::test]
    async fn create_violation_notifications_for_workspace_members() {
        let state = crate::mem::test_state();
        let ws_id = Id::new("ws-test");

        // Create a workspace.
        let ws = gyre_domain::Workspace {
            id: ws_id.clone(),
            tenant_id: Id::new("default"),
            name: "Test WS".to_string(),
            slug: "test-ws".to_string(),
            description: None,
            budget: None,
            max_repos: None,
            max_agents_per_repo: None,
            trust_level: gyre_domain::TrustLevel::Guided,
            llm_model: None,
            created_at: 0,
            compute_target_id: None,
        };
        state.workspaces.create(&ws).await.unwrap();

        // Add a developer member.
        let membership = gyre_domain::WorkspaceMembership::new(
            Id::new("mem-1"),
            Id::new("user-1"),
            ws_id.clone(),
            gyre_domain::WorkspaceRole::Developer,
            Id::new("system"),
            0,
        );
        state
            .workspace_memberships
            .create(&membership)
            .await
            .unwrap();

        let violations = vec![ConstraintViolationInfo {
            constraint_name: "path scope".to_string(),
            expression: r#"output.changed_files.all(f, f.startsWith("src/"))"#.to_string(),
            message: "constraint failed".to_string(),
        }];

        create_violation_notifications(&state, &violations, "TASK-101", "repo-1", &ws_id, "push")
            .await;

        // Verify notification was created.
        let notifications = state
            .notifications
            .list_for_user(
                &Id::new("user-1"),
                Some(&ws_id),
                None,
                None,
                Some("ConstraintViolation"),
                100,
                0,
            )
            .await
            .unwrap();
        assert_eq!(notifications.len(), 1);
        assert_eq!(
            notifications[0].notification_type,
            NotificationType::ConstraintViolation
        );
        assert_eq!(notifications[0].priority, 2);
        assert!(notifications[0].title.contains("Constraint violation"));
        assert!(notifications[0].title.contains("path scope"));
        assert_eq!(notifications[0].repo_id.as_deref(), Some("repo-1"));
    }

    // ── Integration: merge-time constraint evaluation ─────────────────

    #[tokio::test]
    async fn evaluate_merge_no_attestation_chain_is_noop() {
        let state = crate::mem::test_state();
        let tmp = tempfile::tempdir().unwrap();
        let repo_path = tmp.path().to_str().unwrap();

        // Create a MR with an agent author but no attestation chain.
        let mr = gyre_domain::MergeRequest {
            id: Id::new("mr-no-chain"),
            repository_id: Id::new("repo-1"),
            title: "Test MR".to_string(),
            source_branch: "feat/test".to_string(),
            target_branch: "main".to_string(),
            status: gyre_domain::MrStatus::Approved,
            author_agent_id: Some(Id::new("agent-merge-1")),
            reviewers: vec![],
            diff_stats: None,
            has_conflicts: None,
            spec_ref: None,
            depends_on: vec![],
            atomic_group: None,
            created_at: 0,
            updated_at: 0,
            workspace_id: Id::new("ws-1"),
            reverted_at: None,
            revert_commit_sha: None,
        };
        state.merge_requests.create(&mr).await.unwrap();

        // Create the agent with a current_task_id.
        let mut agent = gyre_domain::Agent::new(Id::new("agent-merge-1"), "merge-agent", 0);
        agent.current_task_id = Some(Id::new("TASK-MERGE-1"));
        state.agents.create(&agent).await.unwrap();

        // Subscribe to broadcast before call.
        let mut rx = state.message_broadcast_tx.subscribe();

        // No attestation stored for TASK-MERGE-1 → should return early without events.
        evaluate_merge_constraints(
            &state,
            "mr-no-chain",
            "repo-1",
            repo_path,
            "abc123",
            &Id::new("ws-1"),
            "feat/test",
            "main",
            "main",
        )
        .await;

        // Verify: no events emitted (graceful degradation, no attestation chain).
        assert!(
            rx.try_recv().is_err(),
            "no events should be emitted when there is no attestation chain"
        );
    }

    #[tokio::test]
    async fn evaluate_merge_with_attestation_chain_runs_constraints() {
        let (tmp, _initial_sha, second_sha) = init_test_git_repo();
        let repo_path = tmp.path().to_str().unwrap();

        let state = crate::mem::test_state();

        // Seed workspace meta-spec set and agent persona.
        let meta_sha = seed_agent_context(&state, "ws-1", "agent-merge-2").await;

        // Create a MR with an agent author.
        let mr = gyre_domain::MergeRequest {
            id: Id::new("mr-with-chain"),
            repository_id: Id::new("repo-1"),
            title: "Test MR with chain".to_string(),
            source_branch: "feat/provenance".to_string(),
            target_branch: "main".to_string(),
            status: gyre_domain::MrStatus::Approved,
            author_agent_id: Some(Id::new("agent-merge-2")),
            reviewers: vec![],
            diff_stats: None,
            has_conflicts: None,
            spec_ref: None,
            depends_on: vec![],
            atomic_group: None,
            created_at: 0,
            updated_at: 0,
            workspace_id: Id::new("ws-1"),
            reverted_at: None,
            revert_commit_sha: None,
        };
        state.merge_requests.create(&mr).await.unwrap();

        // Create the agent with a current_task_id.
        let mut agent = gyre_domain::Agent::new(Id::new("agent-merge-2"), "merge-agent-2", 0);
        agent.current_task_id = Some(Id::new("TASK-MERGE-2"));
        state.agents.create(&agent).await.unwrap();

        // Store a SignedInput attestation with a constraint that FAILS.
        let mut si = make_signed_input(
            vec!["src/**"],
            vec![],
            vec!["security"],
            vec![OutputConstraint {
                name: "no-changes merge check".to_string(),
                expression: "output.changed_files.size() == 0".to_string(),
            }],
        );
        si.content.meta_spec_set_sha = meta_sha;
        let att = make_attestation(si, "TASK-MERGE-2");
        state.chain_attestations.save(&att).await.unwrap();

        // Subscribe to broadcast before evaluation.
        let mut rx = state.message_broadcast_tx.subscribe();

        // Run merge evaluation with the real repo and second commit SHA.
        // compute_commit_diff will diff second_sha^..second_sha (the second
        // commit adds src/payments/handler.rs).
        evaluate_merge_constraints(
            &state,
            "mr-with-chain",
            "repo-1",
            repo_path,
            &second_sha,
            &Id::new("ws-1"),
            "feat/provenance",
            "main",
            "main",
        )
        .await;

        // Assert: ConstraintViolation events emitted — the "no-changes merge
        // check" should fail because the commit has changed files.
        let msg = rx
            .try_recv()
            .expect("should have received ConstraintViolation workspace broadcast for merge");
        assert_eq!(msg.kind, MessageKind::ConstraintViolation);
        let payload = msg.payload.as_ref().unwrap();
        assert_eq!(payload["action"], "merge");

        // Agent-directed message.
        let msg2 = rx
            .try_recv()
            .expect("should have received agent-directed ConstraintViolation for merge");
        assert_eq!(msg2.kind, MessageKind::ConstraintViolation);
    }

    // ── build_agent_context ──────────────────────────────────────────

    #[tokio::test]
    async fn build_agent_context_populates_meta_spec_set_sha_from_workspace() {
        let state = crate::mem::test_state();
        let ws_id = Id::new("ws-ctx-1");

        // Store a meta-spec set for the workspace.
        let meta_set_json = r#"{"workspace_id":"ws-ctx-1","personas":{},"principles":[],"standards":[],"process":[]}"#;
        state
            .meta_spec_sets
            .upsert(&ws_id, meta_set_json)
            .await
            .unwrap();

        // Compute expected SHA256.
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(meta_set_json.as_bytes());
        let expected_sha = hex::encode(hasher.finalize());

        let ctx = build_agent_context(&state, "agent-ctx-1", "TASK-CTX-1", &ws_id).await;

        assert_eq!(ctx.meta_spec_set_sha, expected_sha);
        assert!(!ctx.meta_spec_set_sha.is_empty());
    }

    #[tokio::test]
    async fn build_agent_context_empty_sha_when_no_meta_spec_set() {
        let state = crate::mem::test_state();
        let ws_id = Id::new("ws-no-meta");

        // No meta-spec set stored → SHA should be empty.
        let ctx = build_agent_context(&state, "agent-ctx-2", "TASK-CTX-2", &ws_id).await;

        assert!(ctx.meta_spec_set_sha.is_empty());
    }

    #[tokio::test]
    async fn build_agent_context_populates_workload_fields() {
        let state = crate::mem::test_state();
        let ws_id = Id::new("ws-wl-1");

        // Store a workload attestation for the agent.
        let att = crate::workload_attestation::WorkloadAttestation {
            agent_id: "agent-wl-1".to_string(),
            pid: Some(1234),
            hostname: "test-host".to_string(),
            compute_target: "local".to_string(),
            stack_fingerprint: "sha256:stack-abc".to_string(),
            attested_at: 1_700_000_000,
            alive: true,
            last_verified_at: 1_700_000_000,
            container_id: Some("container-xyz".to_string()),
            image_hash: Some("sha256:img-def".to_string()),
        };
        let json = serde_json::to_string(&att).unwrap();
        state
            .kv_store
            .kv_set("workload_attestations", "agent-wl-1", json)
            .await
            .unwrap();

        let ctx = build_agent_context(&state, "agent-wl-1", "TASK-WL-1", &ws_id).await;

        assert_eq!(ctx.stack_hash, "sha256:stack-abc");
        assert_eq!(ctx.container_id, "container-xyz");
        assert_eq!(ctx.image_hash, "sha256:img-def");
        // Level 3: stack + container + image.
        assert_eq!(ctx.attestation_level, 3);
    }

    #[test]
    fn derive_attestation_level_no_workload() {
        // No workload attestation record → Level 0.
        assert_eq!(derive_attestation_level(None), 0);
    }

    #[test]
    fn derive_attestation_level_empty_stack() {
        // Workload attestation exists but no stack fingerprint → Level 1 (raw push).
        let att = crate::workload_attestation::WorkloadAttestation {
            agent_id: "agent-1".to_string(),
            pid: Some(100),
            hostname: "host".to_string(),
            compute_target: "local".to_string(),
            stack_fingerprint: String::new(),
            attested_at: 1_700_000_000,
            alive: true,
            last_verified_at: 1_700_000_000,
            container_id: None,
            image_hash: None,
        };
        assert_eq!(derive_attestation_level(Some(&att)), 1);
    }

    #[test]
    fn derive_attestation_level_stack_only() {
        // Stack fingerprint present, no container → Level 2 (CLI self-reported).
        let att = crate::workload_attestation::WorkloadAttestation {
            agent_id: "agent-2".to_string(),
            pid: Some(200),
            hostname: "host".to_string(),
            compute_target: "local".to_string(),
            stack_fingerprint: "sha256:stack-abc".to_string(),
            attested_at: 1_700_000_000,
            alive: true,
            last_verified_at: 1_700_000_000,
            container_id: None,
            image_hash: None,
        };
        assert_eq!(derive_attestation_level(Some(&att)), 2);
    }

    #[test]
    fn derive_attestation_level_full_container() {
        // Stack + container_id + image_hash → Level 3 (Gyre-managed runtime).
        let att = crate::workload_attestation::WorkloadAttestation {
            agent_id: "agent-3".to_string(),
            pid: Some(300),
            hostname: "host".to_string(),
            compute_target: "container".to_string(),
            stack_fingerprint: "sha256:stack-xyz".to_string(),
            attested_at: 1_700_000_000,
            alive: true,
            last_verified_at: 1_700_000_000,
            container_id: Some("ctr-123".to_string()),
            image_hash: Some("sha256:img-456".to_string()),
        };
        assert_eq!(derive_attestation_level(Some(&att)), 3);
    }

    #[test]
    fn derive_attestation_level_stack_with_empty_container() {
        // Stack present, container_id is Some("") and image_hash is Some("") →
        // Level 2 (empty strings don't count as container attestation).
        let att = crate::workload_attestation::WorkloadAttestation {
            agent_id: "agent-4".to_string(),
            pid: Some(400),
            hostname: "host".to_string(),
            compute_target: "local".to_string(),
            stack_fingerprint: "sha256:stack-def".to_string(),
            attested_at: 1_700_000_000,
            alive: true,
            last_verified_at: 1_700_000_000,
            container_id: Some(String::new()),
            image_hash: Some(String::new()),
        };
        assert_eq!(derive_attestation_level(Some(&att)), 2);
    }

    #[tokio::test]
    async fn get_repo_required_attestation_level_no_policy() {
        // Repo with no stack policy → None.
        let state = crate::mem::test_state();
        let level = get_repo_required_attestation_level(&state, "repo-no-policy").await;
        assert_eq!(level, None);
    }

    #[tokio::test]
    async fn get_repo_required_attestation_level_with_policy_level2() {
        // Repo with stack policy at Level 2 → Some(2).
        let state = crate::mem::test_state();
        let policy = serde_json::json!({
            "fingerprint": "sha256:required-fingerprint",
            "required_level": 2
        });
        state
            .kv_store
            .kv_set(
                "repo_stack_policies",
                "repo-with-policy",
                serde_json::to_string(&policy).unwrap(),
            )
            .await
            .unwrap();
        let level = get_repo_required_attestation_level(&state, "repo-with-policy").await;
        assert_eq!(level, Some(2));
    }

    #[tokio::test]
    async fn get_repo_required_attestation_level_with_policy_level3() {
        // Repo with stack policy at Level 3 → Some(3).
        let state = crate::mem::test_state();
        let policy = serde_json::json!({
            "fingerprint": "sha256:required-fingerprint",
            "required_level": 3
        });
        state
            .kv_store
            .kv_set(
                "repo_stack_policies",
                "repo-level3",
                serde_json::to_string(&policy).unwrap(),
            )
            .await
            .unwrap();
        let level = get_repo_required_attestation_level(&state, "repo-level3").await;
        assert_eq!(level, Some(3));
    }

    #[tokio::test]
    async fn get_repo_required_attestation_level_legacy_plain_string() {
        // Legacy plain-string format (backward compat) → defaults to Some(2).
        let state = crate::mem::test_state();
        state
            .kv_store
            .kv_set(
                "repo_stack_policies",
                "repo-legacy",
                "sha256:legacy-fingerprint".to_string(),
            )
            .await
            .unwrap();
        let level = get_repo_required_attestation_level(&state, "repo-legacy").await;
        assert_eq!(level, Some(2));
    }

    #[tokio::test]
    async fn build_agent_context_derives_attestation_level_from_workload() {
        // Verify build_agent_context derives attestation_level from workload data.
        let state = crate::mem::test_state();
        let ws_id = Id::new("ws-att-level");

        // No workload → Level 0.
        let ctx = build_agent_context(&state, "agent-no-wl", "TASK-1", &ws_id).await;
        assert_eq!(ctx.attestation_level, 0);

        // Level 2: stack hash, no container.
        let att2 = crate::workload_attestation::WorkloadAttestation {
            agent_id: "agent-level2".to_string(),
            pid: Some(100),
            hostname: "host".to_string(),
            compute_target: "local".to_string(),
            stack_fingerprint: "sha256:stack-for-level2".to_string(),
            attested_at: 1_700_000_000,
            alive: true,
            last_verified_at: 1_700_000_000,
            container_id: None,
            image_hash: None,
        };
        state
            .kv_store
            .kv_set(
                "workload_attestations",
                "agent-level2",
                serde_json::to_string(&att2).unwrap(),
            )
            .await
            .unwrap();
        let ctx2 = build_agent_context(&state, "agent-level2", "TASK-2", &ws_id).await;
        assert_eq!(ctx2.attestation_level, 2);

        // Level 3: stack + container + image.
        let att3 = crate::workload_attestation::WorkloadAttestation {
            agent_id: "agent-level3".to_string(),
            pid: Some(200),
            hostname: "host".to_string(),
            compute_target: "container".to_string(),
            stack_fingerprint: "sha256:stack-for-level3".to_string(),
            attested_at: 1_700_000_000,
            alive: true,
            last_verified_at: 1_700_000_000,
            container_id: Some("ctr-abc".to_string()),
            image_hash: Some("sha256:img-xyz".to_string()),
        };
        state
            .kv_store
            .kv_set(
                "workload_attestations",
                "agent-level3",
                serde_json::to_string(&att3).unwrap(),
            )
            .await
            .unwrap();
        let ctx3 = build_agent_context(&state, "agent-level3", "TASK-3", &ws_id).await;
        assert_eq!(ctx3.attestation_level, 3);
    }

    #[test]
    fn attestation_level_constraints_generated_correctly() {
        // Verifies derive_strategy_constraints produces correct attestation-level
        // constraint text for various (workspace_strategy, repo_policy) combinations.
        // Actual CEL evaluation is tested in gyre-domain constraint_evaluator tests.
        use gyre_domain::constraint_evaluator;

        let content = InputContent {
            spec_path: "specs/system/payments.md".to_string(),
            spec_sha: "abc123".to_string(),
            workspace_id: "ws-1".to_string(),
            repo_id: "repo-1".to_string(),
            persona_constraints: vec![],
            meta_spec_set_sha: "def456".to_string(),
            scope: ScopeConstraint {
                allowed_paths: vec![],
                forbidden_paths: vec![],
            },
        };

        // Case 1: Repo requires Level 3, supervised workspace.
        let strategy_with_policy = constraint_evaluator::derive_strategy_constraints(
            &content,
            Some("supervised"),
            Some(3),
        );
        assert!(
            strategy_with_policy
                .iter()
                .any(|c| c.expression.contains("agent.attestation_level >= 3")),
            "should produce attestation level >= 3 constraint"
        );
        // Also has supervised constraint (level >= 2).
        assert!(
            strategy_with_policy
                .iter()
                .any(|c| c.expression.contains("agent.attestation_level >= 2")),
            "supervised workspace should produce level >= 2 constraint"
        );

        // Case 2: No repo policy, autonomous workspace.
        let strategy_no_policy =
            constraint_evaluator::derive_strategy_constraints(&content, Some("autonomous"), None);
        assert!(
            !strategy_no_policy
                .iter()
                .any(|c| c.expression.contains("agent.attestation_level")),
            "should not produce attestation constraint when no policy and autonomous"
        );

        // Case 3: No repo policy, supervised workspace → still produces supervised level >= 2.
        let strategy_supervised_only =
            constraint_evaluator::derive_strategy_constraints(&content, Some("supervised"), None);
        assert!(
            strategy_supervised_only
                .iter()
                .any(|c| c.expression.contains("agent.attestation_level >= 2")),
            "supervised workspace should produce level >= 2 even without repo policy"
        );
        assert!(
            !strategy_supervised_only
                .iter()
                .any(|c| c.expression.contains("agent.attestation_level >= 3")),
            "should not produce level >= 3 when repo has no policy"
        );
    }

    // ── Phase 3: Enforcement tests ──────────────────────────────────

    #[tokio::test]
    async fn enforce_push_no_attestation_allows() {
        // When no attestation chain exists, enforcement allows the push
        // (graceful degradation during migration).
        let state = crate::mem::test_state();
        let (tmp, initial_sha, second_sha) = init_test_git_repo();
        let repo_path = tmp.path().to_str().unwrap();

        let result = enforce_push_constraints(
            &state,
            "TASK-ENF-1",
            "repo-1",
            repo_path,
            "agent-1",
            &Id::new("ws-1"),
            &[(initial_sha, second_sha, "refs/heads/main".to_string())],
            "main",
        )
        .await;

        assert!(
            result.is_ok(),
            "should allow push when no attestation chain exists: {:?}",
            result
        );
    }

    #[tokio::test]
    async fn enforce_push_invalid_chain_rejects() {
        // When an attestation chain exists but is invalid (forged signature),
        // enforcement rejects the push.
        let (tmp, initial_sha, second_sha) = init_test_git_repo();
        let repo_path = tmp.path().to_str().unwrap();

        let state = crate::mem::test_state();
        let meta_sha = seed_agent_context(&state, "ws-1", "agent-enf-2").await;

        // Store an attestation with a forged signature.
        use ring::signature::{Ed25519KeyPair, KeyPair};
        let rng = ring::rand::SystemRandom::new();
        let pkcs8 = Ed25519KeyPair::generate_pkcs8(&rng).unwrap();
        let key_pair = Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap();

        let mut si = make_signed_input(vec!["src/**"], vec![], vec!["security"], vec![]);
        si.content.meta_spec_set_sha = meta_sha;
        // Forge the signature (wrong bytes).
        si.signature = vec![0xDE; 64];
        si.key_binding.public_key = key_pair.public_key().as_ref().to_vec();

        let att = make_attestation(si, "TASK-ENF-2");
        state.chain_attestations.save(&att).await.unwrap();

        let result = enforce_push_constraints(
            &state,
            "TASK-ENF-2",
            "repo-1",
            repo_path,
            "agent-enf-2",
            &Id::new("ws-1"),
            &[(initial_sha, second_sha, "refs/heads/main".to_string())],
            "main",
        )
        .await;

        assert!(result.is_err(), "should reject push with forged signature");
        let err = result.unwrap_err();
        assert!(
            err.contains("attestation chain invalid"),
            "error should mention invalid chain: {err}"
        );
    }

    #[tokio::test]
    async fn enforce_push_constraint_failure_rejects() {
        // When constraints exist and fail, enforcement rejects.
        let (tmp, initial_sha, second_sha) = init_test_git_repo();
        let repo_path = tmp.path().to_str().unwrap();

        let state = crate::mem::test_state();
        let meta_sha = seed_agent_context(&state, "ws-1", "agent-enf-3").await;

        // Create a properly signed attestation with a constraint that fails.
        use ring::signature::{Ed25519KeyPair, KeyPair};
        let rng = ring::rand::SystemRandom::new();
        let pkcs8 = Ed25519KeyPair::generate_pkcs8(&rng).unwrap();
        let key_pair = Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap();

        let mut si = make_signed_input(
            vec!["src/payments/**"],
            vec![],
            vec!["security"],
            vec![OutputConstraint {
                name: "no changes allowed".to_string(),
                expression: "output.changed_files.size() == 0".to_string(),
            }],
        );
        si.content.meta_spec_set_sha = meta_sha;

        // Sign properly.
        let content_bytes = serde_json::to_vec(&si.content).unwrap();
        let content_hash = ring::digest::digest(&ring::digest::SHA256, &content_bytes);
        si.signature = key_pair.sign(content_hash.as_ref()).as_ref().to_vec();
        si.key_binding.public_key = key_pair.public_key().as_ref().to_vec();

        let att = make_attestation(si, "TASK-ENF-3");
        state.chain_attestations.save(&att).await.unwrap();

        let result = enforce_push_constraints(
            &state,
            "TASK-ENF-3",
            "repo-1",
            repo_path,
            "agent-enf-3",
            &Id::new("ws-1"),
            &[(initial_sha, second_sha, "refs/heads/main".to_string())],
            "main",
        )
        .await;

        assert!(
            result.is_err(),
            "should reject push with failing constraint"
        );
        let err = result.unwrap_err();
        assert!(
            err.contains("constraint(s) failed"),
            "error should mention constraint failure: {err}"
        );
    }

    #[tokio::test]
    async fn enforce_push_valid_chain_allows() {
        // When constraints pass, enforcement allows the push.
        let (tmp, initial_sha, second_sha) = init_test_git_repo();
        let repo_path = tmp.path().to_str().unwrap();

        let state = crate::mem::test_state();
        let meta_sha = seed_agent_context(&state, "ws-1", "agent-enf-4").await;

        use ring::signature::{Ed25519KeyPair, KeyPair};
        let rng = ring::rand::SystemRandom::new();
        let pkcs8 = Ed25519KeyPair::generate_pkcs8(&rng).unwrap();
        let key_pair = Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap();

        // Constraint that passes: changed files must be non-empty (our test commit adds a file).
        let mut si = make_signed_input(
            vec!["src/**"],
            vec![],
            vec!["security"],
            vec![OutputConstraint {
                name: "has changes".to_string(),
                expression: "output.changed_files.size() > 0".to_string(),
            }],
        );
        si.content.meta_spec_set_sha = meta_sha;

        let content_bytes = serde_json::to_vec(&si.content).unwrap();
        let content_hash = ring::digest::digest(&ring::digest::SHA256, &content_bytes);
        si.signature = key_pair.sign(content_hash.as_ref()).as_ref().to_vec();
        si.key_binding.public_key = key_pair.public_key().as_ref().to_vec();

        let att = make_attestation(si, "TASK-ENF-4");
        state.chain_attestations.save(&att).await.unwrap();

        let result = enforce_push_constraints(
            &state,
            "TASK-ENF-4",
            "repo-1",
            repo_path,
            "agent-enf-4",
            &Id::new("ws-1"),
            &[(initial_sha, second_sha, "refs/heads/main".to_string())],
            "main",
        )
        .await;

        assert!(
            result.is_ok(),
            "should allow push when constraints pass: {:?}",
            result
        );
    }

    #[tokio::test]
    async fn enforce_merge_no_attestation_allows() {
        // When no attestation chain exists, enforcement allows the merge.
        let state = crate::mem::test_state();
        let (tmp, _initial_sha, second_sha) = init_test_git_repo();
        let repo_path = tmp.path().to_str().unwrap();

        // Create a MR with an agent author.
        let mr = gyre_domain::MergeRequest {
            id: Id::new("mr-enf-noop"),
            repository_id: Id::new("repo-1"),
            title: "Enforcement no-op MR".to_string(),
            source_branch: "feat/test".to_string(),
            target_branch: "main".to_string(),
            status: gyre_domain::MrStatus::Approved,
            author_agent_id: Some(Id::new("agent-merge-enf-1")),
            reviewers: vec![],
            diff_stats: None,
            has_conflicts: None,
            spec_ref: None,
            depends_on: vec![],
            atomic_group: None,
            created_at: 0,
            updated_at: 0,
            workspace_id: Id::new("ws-1"),
            reverted_at: None,
            revert_commit_sha: None,
        };
        state.merge_requests.create(&mr).await.unwrap();

        let mut agent =
            gyre_domain::Agent::new(Id::new("agent-merge-enf-1"), "merge-enf-agent-1", 0);
        agent.current_task_id = Some(Id::new("TASK-MERGE-ENF-1"));
        state.agents.create(&agent).await.unwrap();

        let result = enforce_merge_constraints(
            &state,
            "mr-enf-noop",
            "repo-1",
            repo_path,
            &second_sha,
            &Id::new("ws-1"),
            "feat/test",
            "main",
            "main",
        )
        .await;

        assert!(
            result.is_ok(),
            "should allow merge when no attestation chain exists: {:?}",
            result
        );
    }

    #[tokio::test]
    async fn enforce_merge_constraint_failure_blocks() {
        // When constraints fail at merge time, enforcement blocks.
        let (tmp, _initial_sha, second_sha) = init_test_git_repo();
        let repo_path = tmp.path().to_str().unwrap();

        let state = crate::mem::test_state();
        let meta_sha = seed_agent_context(&state, "ws-1", "agent-merge-enf-2").await;

        // Create MR + agent.
        let mr = gyre_domain::MergeRequest {
            id: Id::new("mr-enf-fail"),
            repository_id: Id::new("repo-1"),
            title: "Enforcement fail MR".to_string(),
            source_branch: "feat/test".to_string(),
            target_branch: "main".to_string(),
            status: gyre_domain::MrStatus::Approved,
            author_agent_id: Some(Id::new("agent-merge-enf-2")),
            reviewers: vec![],
            diff_stats: None,
            has_conflicts: None,
            spec_ref: None,
            depends_on: vec![],
            atomic_group: None,
            created_at: 0,
            updated_at: 0,
            workspace_id: Id::new("ws-1"),
            reverted_at: None,
            revert_commit_sha: None,
        };
        state.merge_requests.create(&mr).await.unwrap();

        let mut agent =
            gyre_domain::Agent::new(Id::new("agent-merge-enf-2"), "merge-enf-agent-2", 0);
        agent.current_task_id = Some(Id::new("TASK-MERGE-ENF-2"));
        state.agents.create(&agent).await.unwrap();

        // Store attestation with a constraint that fails.
        use ring::signature::{Ed25519KeyPair, KeyPair};
        let rng = ring::rand::SystemRandom::new();
        let pkcs8 = Ed25519KeyPair::generate_pkcs8(&rng).unwrap();
        let key_pair = Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap();

        let mut si = make_signed_input(
            vec!["src/**"],
            vec![],
            vec!["security"],
            vec![OutputConstraint {
                name: "no changes merge".to_string(),
                expression: "output.changed_files.size() == 0".to_string(),
            }],
        );
        si.content.meta_spec_set_sha = meta_sha;

        let content_bytes = serde_json::to_vec(&si.content).unwrap();
        let content_hash = ring::digest::digest(&ring::digest::SHA256, &content_bytes);
        si.signature = key_pair.sign(content_hash.as_ref()).as_ref().to_vec();
        si.key_binding.public_key = key_pair.public_key().as_ref().to_vec();

        let att = make_attestation(si, "TASK-MERGE-ENF-2");
        state.chain_attestations.save(&att).await.unwrap();

        let result = enforce_merge_constraints(
            &state,
            "mr-enf-fail",
            "repo-1",
            repo_path,
            &second_sha,
            &Id::new("ws-1"),
            "feat/test",
            "main",
            "main",
        )
        .await;

        assert!(result.is_err(), "should block merge when constraint fails");
        let err = result.unwrap_err();
        assert!(
            err.contains("constraint(s) failed"),
            "error should mention constraint failure: {err}"
        );
    }

    // ── TASK-188: §2.4 Context binding (replay prevention) ──────────────

    /// Build a REAL Ed25519-signed SignedInput for binding tests: the content
    /// (including repo_id/workspace_id/spec_sha/expected_generation) is fully
    /// signed, so only the binding comparison — not signature validity —
    /// decides the outcome.
    fn make_binding_signed_input(
        repo_id: &str,
        workspace_id: &str,
        spec_sha: &str,
        expected_generation: Option<u32>,
        key_pair: &ring::signature::Ed25519KeyPair,
        meta_spec_set_sha: &str,
    ) -> SignedInput {
        use ring::signature::KeyPair;
        let content = InputContent {
            spec_path: "specs/system/payments.md".to_string(),
            spec_sha: spec_sha.to_string(),
            workspace_id: workspace_id.to_string(),
            repo_id: repo_id.to_string(),
            persona_constraints: vec![],
            meta_spec_set_sha: meta_spec_set_sha.to_string(),
            scope: ScopeConstraint {
                allowed_paths: vec![],
                forbidden_paths: vec![],
            },
        };
        let content_bytes = serde_json::to_vec(&content).unwrap();
        let content_hash = ring::digest::digest(&ring::digest::SHA256, &content_bytes);
        let signature = key_pair.sign(content_hash.as_ref()).as_ref().to_vec();

        SignedInput {
            content,
            output_constraints: vec![],
            valid_until: u64::MAX,
            expected_generation,
            signature,
            key_binding: gyre_common::KeyBinding {
                public_key: key_pair.public_key().as_ref().to_vec(),
                user_identity: "user:jsell".to_string(),
                issuer: "https://keycloak.example.com".to_string(),
                trust_anchor_id: "tenant-keycloak".to_string(),
                issued_at: 1_700_000_000,
                expires_at: u64::MAX,
                user_signature: vec![10],
                platform_countersign: vec![20],
            },
        }
    }

    fn make_binding_keypair() -> ring::signature::Ed25519KeyPair {
        use ring::signature::KeyPair;
        let rng = ring::rand::SystemRandom::new();
        let pkcs8 = ring::signature::Ed25519KeyPair::generate_pkcs8(&rng).unwrap();
        ring::signature::Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap()
    }

    /// Seed a task with repo/workspace/generation so the binding check has a
    /// real persisted target to compare against.
    async fn seed_binding_task(state: &crate::AppState, task_id: &str, generation: u32) {
        let mut task = gyre_domain::Task::new(Id::new(task_id), "binding test task", 1000);
        task.repo_id = Id::new("repo-1");
        task.workspace_id = Id::new("ws-1");
        task.generation = generation;
        state.tasks.create(&task).await.unwrap();
    }

    #[tokio::test]
    async fn enforce_push_rejects_repo_id_mismatch() {
        // A SignedInput authorizing repo-OTHER cannot authorize a push to
        // repo-1 — even though the signature is perfectly valid.
        let (tmp, initial_sha, second_sha) = init_test_git_repo();
        let repo_path = tmp.path().to_str().unwrap();

        let state = crate::mem::test_state();
        let meta_sha = seed_agent_context(&state, "ws-1", "agent-bind-1").await;
        seed_binding_task(&state, "TASK-BIND-1", 1).await;

        let key_pair = make_binding_keypair();
        let si = make_binding_signed_input(
            "repo-OTHER",
            "ws-1",
            "abc12345",
            None,
            &key_pair,
            &meta_sha,
        );
        let att = make_attestation(si, "TASK-BIND-1");
        state.chain_attestations.save(&att).await.unwrap();

        let result = enforce_push_constraints(
            &state,
            "TASK-BIND-1",
            "repo-1", // push target
            repo_path,
            "agent-bind-1",
            &Id::new("ws-1"),
            &[(initial_sha, second_sha, "refs/heads/main".to_string())],
            "main",
        )
        .await;

        assert!(
            result.is_err(),
            "push with SignedInput bound to another repo must be rejected"
        );
        let err = result.unwrap_err();
        assert!(
            err.contains("context binding") && err.contains("repo_id"),
            "error should name the failing repo_id binding: {err}"
        );
    }

    #[tokio::test]
    async fn enforce_merge_rejects_workspace_id_mismatch() {
        // A SignedInput authorizing ws-OTHER cannot authorize a merge in ws-1.
        let (tmp, _initial_sha, second_sha) = init_test_git_repo();
        let repo_path = tmp.path().to_str().unwrap();

        let state = crate::mem::test_state();
        let meta_sha = seed_agent_context(&state, "ws-1", "agent-bind-2").await;
        seed_binding_task(&state, "TASK-BIND-2", 1).await;

        let mr = gyre_domain::MergeRequest {
            id: Id::new("mr-bind-ws"),
            repository_id: Id::new("repo-1"),
            title: "Binding test MR".to_string(),
            source_branch: "feat/test".to_string(),
            target_branch: "main".to_string(),
            status: gyre_domain::MrStatus::Approved,
            author_agent_id: Some(Id::new("agent-bind-2")),
            reviewers: vec![],
            diff_stats: None,
            has_conflicts: None,
            spec_ref: None,
            depends_on: vec![],
            atomic_group: None,
            created_at: 0,
            updated_at: 0,
            workspace_id: Id::new("ws-1"),
            reverted_at: None,
            revert_commit_sha: None,
        };
        state.merge_requests.create(&mr).await.unwrap();

        let mut agent = gyre_domain::Agent::new(Id::new("agent-bind-2"), "bind-agent-2", 0);
        agent.current_task_id = Some(Id::new("TASK-BIND-2"));
        state.agents.create(&agent).await.unwrap();

        let key_pair = make_binding_keypair();
        let si = make_binding_signed_input(
            "repo-1",
            "ws-OTHER",
            "abc12345",
            None,
            &key_pair,
            &meta_sha,
        );
        let att = make_attestation(si, "TASK-BIND-2");
        state.chain_attestations.save(&att).await.unwrap();

        let result = enforce_merge_constraints(
            &state,
            "mr-bind-ws",
            "repo-1",
            repo_path,
            &second_sha,
            &Id::new("ws-1"), // merge target workspace
            "feat/test",
            "main",
            "main",
        )
        .await;

        assert!(
            result.is_err(),
            "merge with SignedInput bound to another workspace must be rejected"
        );
        let err = result.unwrap_err();
        assert!(
            err.contains("context binding") && err.contains("workspace_id"),
            "error should name the failing workspace_id binding: {err}"
        );
    }

    #[tokio::test]
    async fn enforce_push_rejects_stale_generation() {
        // expected_generation = Some(2) but the task is at generation 1
        // (never reassigned to that generation) — the old authorization
        // cannot be replayed.
        let (tmp, initial_sha, second_sha) = init_test_git_repo();
        let repo_path = tmp.path().to_str().unwrap();

        let state = crate::mem::test_state();
        let meta_sha = seed_agent_context(&state, "ws-1", "agent-bind-3").await;
        seed_binding_task(&state, "TASK-BIND-3", 1).await;

        let key_pair = make_binding_keypair();
        let si = make_binding_signed_input(
            "repo-1",
            "ws-1",
            "abc12345",
            Some(2), // pinned to generation 2; task is at generation 1
            &key_pair,
            &meta_sha,
        );
        let att = make_attestation(si, "TASK-BIND-3");
        state.chain_attestations.save(&att).await.unwrap();

        let result = enforce_push_constraints(
            &state,
            "TASK-BIND-3",
            "repo-1",
            repo_path,
            "agent-bind-3",
            &Id::new("ws-1"),
            &[(initial_sha, second_sha, "refs/heads/main".to_string())],
            "main",
        )
        .await;

        assert!(
            result.is_err(),
            "push with expected_generation=2 against generation-1 task must be rejected"
        );
        let err = result.unwrap_err();
        assert!(
            err.contains("context binding") && err.contains("generation"),
            "error should name the failing generation binding: {err}"
        );
    }

    #[tokio::test]
    async fn enforce_push_matching_generation_passes() {
        // expected_generation = Some(1) and the task is at generation 1 —
        // the pin matches, the push passes.
        let (tmp, initial_sha, second_sha) = init_test_git_repo();
        let repo_path = tmp.path().to_str().unwrap();

        let state = crate::mem::test_state();
        let meta_sha = seed_agent_context(&state, "ws-1", "agent-bind-4").await;
        seed_binding_task(&state, "TASK-BIND-4", 1).await;

        let key_pair = make_binding_keypair();
        let si = make_binding_signed_input(
            "repo-1",
            "ws-1",
            "abc12345",
            Some(1),
            &key_pair,
            &meta_sha,
        );
        let att = make_attestation(si, "TASK-BIND-4");
        state.chain_attestations.save(&att).await.unwrap();

        let result = enforce_push_constraints(
            &state,
            "TASK-BIND-4",
            "repo-1",
            repo_path,
            "agent-bind-4",
            &Id::new("ws-1"),
            &[(initial_sha, second_sha, "refs/heads/main".to_string())],
            "main",
        )
        .await;

        assert!(
            result.is_ok(),
            "push with all bindings matching must be allowed: {:?}",
            result
        );
    }

    #[tokio::test]
    async fn enforce_push_matching_context_passes() {
        // All bindings match (repo, workspace, no generation pin, no approved
        // spec SHA recorded for the task's spec_path) — legitimate work is
        // not over-rejected.
        let (tmp, initial_sha, second_sha) = init_test_git_repo();
        let repo_path = tmp.path().to_str().unwrap();

        let state = crate::mem::test_state();
        let meta_sha = seed_agent_context(&state, "ws-1", "agent-bind-5").await;
        seed_binding_task(&state, "TASK-BIND-5", 1).await;

        let key_pair = make_binding_keypair();
        let si = make_binding_signed_input(
            "repo-1",
            "ws-1",
            "abc12345",
            None,
            &key_pair,
            &meta_sha,
        );
        let att = make_attestation(si, "TASK-BIND-5");
        state.chain_attestations.save(&att).await.unwrap();

        let result = enforce_push_constraints(
            &state,
            "TASK-BIND-5",
            "repo-1",
            repo_path,
            "agent-bind-5",
            &Id::new("ws-1"),
            &[(initial_sha, second_sha, "refs/heads/main".to_string())],
            "main",
        )
        .await;

        assert!(
            result.is_ok(),
            "push with fully matching context must be allowed: {:?}",
            result
        );
    }

    #[tokio::test]
    async fn enforce_push_rejects_spec_sha_mismatch() {
        // The task's spec was approved at a different SHA than the one the
        // SignedInput pins — a modified spec requires a new approval.
        let (tmp, initial_sha, second_sha) = init_test_git_repo();
        let repo_path = tmp.path().to_str().unwrap();

        let state = crate::mem::test_state();
        let meta_sha = seed_agent_context(&state, "ws-1", "agent-bind-6").await;

        // Task with a spec_path whose current approved SHA is "f".repeat(40).
        let mut task = gyre_domain::Task::new(Id::new("TASK-BIND-6"), "spec sha test", 1000);
        task.repo_id = Id::new("repo-1");
        task.workspace_id = Id::new("ws-1");
        task.spec_path = Some("specs/system/payments.md".to_string());
        task.generation = 1;
        state.tasks.create(&task).await.unwrap();

        // Record an approval event at the CURRENT spec SHA.
        let event = gyre_domain::SpecApprovalEvent {
            id: "approval-bind-6".to_string(),
            spec_path: "specs/system/payments.md".to_string(),
            spec_sha: "f".repeat(40),
            approver_type: "human".to_string(),
            approver_id: "user:approver".to_string(),
            persona: None,
            approved_at: 1_700_000_100,
            revoked_at: None,
            revoked_by: None,
            revocation_reason: None,
        };
        state.spec_approval_history.record(&event).await.unwrap();

        let key_pair = make_binding_keypair();
        // SignedInput pins an OLD SHA ("abc12345") — spec changed since.
        let si = make_binding_signed_input(
            "repo-1",
            "ws-1",
            "abc12345",
            None,
            &key_pair,
            &meta_sha,
        );
        let att = make_attestation(si, "TASK-BIND-6");
        state.chain_attestations.save(&att).await.unwrap();

        let result = enforce_push_constraints(
            &state,
            "TASK-BIND-6",
            "repo-1",
            repo_path,
            "agent-bind-6",
            &Id::new("ws-1"),
            &[(initial_sha, second_sha, "refs/heads/main".to_string())],
            "main",
        )
        .await;

        assert!(
            result.is_err(),
            "push with SignedInput pinned to a stale spec SHA must be rejected"
        );
        let err = result.unwrap_err();
        assert!(
            err.contains("context binding") && err.contains("spec_sha"),
            "error should name the failing spec_sha binding: {err}"
        );
    }

    #[test]
    fn verify_context_binding_unit_matrix() {
        // Direct unit coverage of the comparison logic (real values, no
        // storage): each binding independently rejects, all-match passes.
        let key_pair = make_binding_keypair();
        let si = |repo: &str, ws: &str, gen: Option<u32>| {
            make_binding_signed_input(repo, ws, "abc12345", gen, &key_pair, "meta-sha")
        };

        // All match (with generation pin satisfied).
        let r = crate::git_http::verify_context_binding(&si("repo-1", "ws-1", Some(3)), "repo-1", "ws-1", None, Some(3));
        assert!(r.valid, "all bindings match must pass");
        assert_eq!(r.children.len(), 3);

        // repo mismatch.
        let r = crate::git_http::verify_context_binding(&si("repo-A", "ws-1", None), "repo-B", "ws-1", None, None);
        assert!(!r.valid);
        assert!(r.children.iter().any(|c| c.label == "context_binding.repo_id" && !c.valid));

        // workspace mismatch.
        let r = crate::git_http::verify_context_binding(&si("repo-1", "ws-A", None), "repo-1", "ws-B", None, None);
        assert!(!r.valid);
        assert!(r.children.iter().any(|c| c.label == "context_binding.workspace_id" && !c.valid));

        // spec_sha mismatch.
        let r = crate::git_http::verify_context_binding(&si("repo-1", "ws-1", None), "repo-1", "ws-1", Some("different-sha"), None);
        assert!(!r.valid);
        assert!(r.children.iter().any(|c| c.label == "context_binding.spec_sha" && !c.valid));

        // generation pin vs missing task generation → fail closed.
        let r = crate::git_http::verify_context_binding(&si("repo-1", "ws-1", Some(2)), "repo-1", "ws-1", None, None);
        assert!(!r.valid);
        assert!(r.children.iter().any(|c| c.label == "context_binding.expected_generation" && !c.valid));

        // generation pin vs different generation.
        let r = crate::git_http::verify_context_binding(&si("repo-1", "ws-1", Some(2)), "repo-1", "ws-1", None, Some(1));
        assert!(!r.valid);
        assert!(r.children.iter().any(|c| c.label == "context_binding.expected_generation" && !c.valid));

        // No pin → no generation check performed.
        let r = crate::git_http::verify_context_binding(&si("repo-1", "ws-1", None), "repo-1", "ws-1", None, Some(9));
        assert!(r.valid);
        assert!(!r.children.iter().any(|c| c.label == "context_binding.expected_generation"));

        // spec_sha binding skipped when current SHA unresolvable (None).
        let r = crate::git_http::verify_context_binding(&si("repo-1", "ws-1", None), "repo-1", "ws-1", None, None);
        assert!(r.valid);
        assert!(!r.children.iter().any(|c| c.label == "context_binding.spec_sha"));
    }

    #[tokio::test]
    async fn task_generation_increments_on_reassignment() {
        // The API reassignment path bumps Task.generation — the value the
        // expected_generation binding compares against is real persisted
        // state, not a constant.
        let state = crate::mem::test_state();
        let task_id = "task-gen-bump";
        let mut task = gyre_domain::Task::new(Id::new(task_id), "gen bump", 1000);
        task.assigned_to = Some(Id::new("agent-a"));
        state.tasks.create(&task).await.unwrap();

        // Round-trip through update (no reassignment) — generation unchanged.
        task.title = "renamed".to_string();
        state.tasks.update(&task).await.unwrap();
        let t = state.tasks.find_by_id(&Id::new(task_id)).await.unwrap().unwrap();
        assert_eq!(t.generation, 1, "non-reassigning update must not bump generation");

        // Reassign via the domain value the API sets — generation bumps.
        let mut t = t;
        t.assigned_to = Some(Id::new("agent-b"));
        t.generation = t.generation.saturating_add(1);
        state.tasks.update(&t).await.unwrap();
        let t2 = state.tasks.find_by_id(&Id::new(task_id)).await.unwrap().unwrap();
        assert_eq!(t2.generation, 2, "reassignment must bump generation");
        assert_eq!(t2.assigned_to.as_ref().map(|i| i.to_string()), Some("agent-b".to_string()));
    }

    #[tokio::test]
    async fn update_task_api_reassignment_bumps_generation() {
        // Full API-level proof: PUT /tasks/:id with a different assignee
        // increments generation; the SignedInput pinned to the old
        // generation then fails the binding check.
        use axum::body::Body;
        use tower::ServiceExt;

        let state = crate::mem::test_state();
        let mut task = gyre_domain::Task::new(Id::new("task-api-gen"), "api gen", 1000);
        task.assigned_to = Some(Id::new("agent-old"));
        state.tasks.create(&task).await.unwrap();

        let app = crate::api::api_router().with_state(state.clone());
        let body = serde_json::json!({ "assigned_to": "agent-new" });
        let resp = app
            .oneshot(
                http::Request::builder()
                    .method("PUT")
                    .uri("/api/v1/tasks/task-api-gen")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), axum::http::StatusCode::OK);

        let mut updated = state
            .tasks
            .find_by_id(&Id::new("task-api-gen"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(updated.generation, 2, "API reassignment must bump generation");

        // An authorization pinned to generation 1 is now stale and must be
        // rejected at the enforcement boundary.
        let (tmp, initial_sha, second_sha) = init_test_git_repo();
        let repo_path = tmp.path().to_str().unwrap();
        let meta_sha = seed_agent_context(&state, "ws-1", "agent-bind-7").await;
        updated.repo_id = Id::new("repo-1");
        updated.workspace_id = Id::new("ws-1");
        state.tasks.update(&updated).await.unwrap();

        let key_pair = make_binding_keypair();
        let si = make_binding_signed_input(
            "repo-1",
            "ws-1",
            "abc12345",
            Some(1), // pinned to the pre-reassignment generation
            &key_pair,
            &meta_sha,
        );
        let att = make_attestation(si, "task-api-gen");
        state.chain_attestations.save(&att).await.unwrap();

        let result = enforce_push_constraints(
            &state,
            "task-api-gen",
            "repo-1",
            repo_path,
            "agent-bind-7",
            &Id::new("ws-1"),
            &[(initial_sha, second_sha, "refs/heads/main".to_string())],
            "main",
        )
        .await;

        assert!(
            result.is_err(),
            "authorization pinned to generation 1 must be rejected after reassignment to generation 2"
        );
        let err = result.unwrap_err();
        assert!(
            err.contains("generation"),
            "error should name the generation binding: {err}"
        );
    }
}
