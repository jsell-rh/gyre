//! Push-time and merge-time constraint evaluation (Phase 2 — audit-only).
//!
//! Derives strategy-implied constraints from `InputContent`, evaluates them
//! against the actual diff, logs results, and emits `ConstraintViolation`
//! events on failures. Does NOT reject pushes or merges (audit-only).

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
                "no attestation chain found for task (Phase 2, audit-only)"
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

    // Build agent context from the agent record.
    let agent_ctx = build_agent_context(state, agent_id, task_id).await;

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

    // Derive strategy-implied constraints.
    let strategy_constraints = constraint_evaluator::derive_strategy_constraints(
        &signed_input.content,
        trust_level.as_deref(),
        None, // attestation level policy not yet available at push time
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

    // Build agent context.
    let agent_ctx = build_agent_context(state, &agent_id, &task_id).await;

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

    // Derive strategy-implied constraints.
    let strategy_constraints = constraint_evaluator::derive_strategy_constraints(
        &signed_input.content,
        trust_level.as_deref(),
        None,
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

// ── Helpers ──────────────────────────────────────────────────────────────

/// Build an `AgentContext` from the agent record and JWT claims in the database.
async fn build_agent_context(state: &AppState, agent_id: &str, task_id: &str) -> AgentContext {
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

    AgentContext {
        id: agent_id.to_string(),
        persona,
        stack_hash: String::new(),
        attestation_level: 0,
        meta_spec_set_sha: String::new(),
        spawned_by: agent
            .as_ref()
            .and_then(|a| a.spawned_by.clone())
            .unwrap_or_default(),
        task_id: task_id.to_string(),
        container_id: String::new(),
        image_hash: String::new(),
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
async fn compute_commit_diff(repo_path: &str, commit_sha: &str) -> Option<OutputContext> {
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
fn parse_shortstat(text: &str) -> (u64, u64) {
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

/// Log constraint evaluation results.
fn log_constraint_results(
    result: &VerificationResult,
    task_id: &str,
    repo_id: &str,
    agent_id: &str,
    action: &str,
    constraint_count: usize,
) {
    if result.valid {
        info!(
            task_id = %task_id,
            repo_id = %repo_id,
            agent_id = %agent_id,
            action = %action,
            constraints_evaluated = constraint_count,
            "attestation.verified: all {} constraint(s) passed (audit-only)",
            constraint_count
        );
    } else {
        warn!(
            task_id = %task_id,
            repo_id = %repo_id,
            agent_id = %agent_id,
            action = %action,
            constraints_evaluated = constraint_count,
            message = %result.message,
            "attestation.constraint_failed: constraint evaluation failed (audit-only, not rejecting)"
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
        // No attestation stored → should just log and return.
        evaluate_push_constraints(
            &state,
            "TASK-99",
            "repo-1",
            "/nonexistent",
            "agent-1",
            &Id::new("ws-1"),
            &[],
            "main",
        )
        .await;
        // No panic = success. The function gracefully degrades.
    }

    #[tokio::test]
    async fn evaluate_push_with_attestation_chain_runs_constraints() {
        let state = crate::mem::test_state();

        // Store a SignedInput attestation for the task.
        let si = make_signed_input(
            vec!["src/payments/**"],
            vec!["src/auth/**"],
            vec!["security"],
            vec![OutputConstraint {
                name: "test constraint".to_string(),
                expression: "true".to_string(), // always passes
            }],
        );
        let att = make_attestation(si, "TASK-100");
        state.chain_attestations.save(&att).await.unwrap();

        // Run evaluation — this will try to compute the diff from a nonexistent
        // repo path, so it will log a warning and return. But the attestation
        // lookup should succeed.
        evaluate_push_constraints(
            &state,
            "TASK-100",
            "repo-1",
            "/nonexistent/path",
            "agent-1",
            &Id::new("ws-1"),
            &[(
                "0".repeat(40),
                "a".repeat(40),
                "refs/heads/main".to_string(),
            )],
            "main",
        )
        .await;
        // No panic = success. The function found the attestation but failed
        // to compute the diff (expected for this test without a real git repo).
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
            revert_mr_id: None,
        };
        state.merge_requests.create(&mr).await.unwrap();

        // Create the agent with a current_task_id.
        let mut agent = gyre_domain::Agent::new(Id::new("agent-merge-1"), "merge-agent", 0);
        agent.current_task_id = Some(Id::new("TASK-MERGE-1"));
        state.agents.create(&agent).await.unwrap();

        // No attestation stored for TASK-MERGE-1 → should gracefully degrade.
        evaluate_merge_constraints(
            &state,
            "mr-no-chain",
            "repo-1",
            "/nonexistent",
            "abc123",
            &Id::new("ws-1"),
            "feat/test",
            "main",
            "main",
        )
        .await;
        // No panic = success. The function gracefully degrades when no
        // attestation chain exists.
    }

    #[tokio::test]
    async fn evaluate_merge_with_attestation_chain_runs_constraints() {
        let state = crate::mem::test_state();

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
            revert_mr_id: None,
        };
        state.merge_requests.create(&mr).await.unwrap();

        // Create the agent with a current_task_id.
        let mut agent = gyre_domain::Agent::new(Id::new("agent-merge-2"), "merge-agent-2", 0);
        agent.current_task_id = Some(Id::new("TASK-MERGE-2"));
        state.agents.create(&agent).await.unwrap();

        // Store a SignedInput attestation for the task.
        let si = make_signed_input(
            vec!["src/**"],
            vec![],
            vec!["security"],
            vec![OutputConstraint {
                name: "merge test constraint".to_string(),
                expression: "true".to_string(),
            }],
        );
        let att = make_attestation(si, "TASK-MERGE-2");
        state.chain_attestations.save(&att).await.unwrap();

        // Run merge evaluation — this will successfully look up the MR,
        // resolve the agent and task, find the attestation chain, but fail
        // to compute the diff from a nonexistent repo path (expected).
        evaluate_merge_constraints(
            &state,
            "mr-with-chain",
            "repo-1",
            "/nonexistent/path",
            "abc123def",
            &Id::new("ws-1"),
            "feat/provenance",
            "main",
            "main",
        )
        .await;
        // No panic = success. The function found the MR, resolved the agent
        // and task, found the attestation chain, but could not compute the
        // diff (expected without a real git repo).
    }
}
