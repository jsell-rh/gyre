//! Gate execution engine: runs quality gates for an MR in the background.

use std::sync::Arc;
use std::time::Duration;
use tracing::{info, warn};
use uuid::Uuid;

use gyre_common::Id;
use gyre_domain::{GateResult, GateStatus, GateType, Review, ReviewDecision};

use crate::{domain_events::DomainEvent, AppState};

/// Default timeout for agent-based gates (5 minutes).
const AGENT_GATE_TIMEOUT_SECS: u64 = 300;

/// Create pending GateResult records for all gates belonging to the MR's repo,
/// then spawn a background task that runs each gate and updates the result.
pub async fn trigger_gates_for_mr(state: Arc<AppState>, mr_id: Id, repo_id: Id) {
    // Collect gates for this repo.
    let gates: Vec<_> = {
        let lock = state.quality_gates.lock().await;
        lock.values()
            .filter(|g| g.repo_id.as_str() == repo_id.as_str())
            .cloned()
            .collect()
    };

    if gates.is_empty() {
        return;
    }

    // Create Pending GateResult for each gate.
    let mut result_ids: Vec<(Id, gyre_domain::QualityGate)> = Vec::new();
    {
        let mut lock = state.gate_results.lock().await;
        for gate in &gates {
            let result_id = Id::new(uuid::Uuid::new_v4().to_string());
            let _now = now_secs();
            let result = GateResult {
                id: result_id.clone(),
                gate_id: gate.id.clone(),
                mr_id: mr_id.clone(),
                status: GateStatus::Pending,
                output: None,
                started_at: None,
                finished_at: None,
            };
            lock.insert(result_id.to_string(), result);
            result_ids.push((result_id, gate.clone()));
        }
    }

    // Spawn background tasks for each gate.
    for (result_id, gate) in result_ids {
        let state = state.clone();
        let mr_id = mr_id.clone();
        tokio::spawn(async move {
            run_gate(state, result_id, gate, mr_id).await;
        });
    }
}

async fn run_gate(state: Arc<AppState>, result_id: Id, gate: gyre_domain::QualityGate, mr_id: Id) {
    let started_at = now_secs();

    // Mark as Running.
    {
        let mut lock = state.gate_results.lock().await;
        if let Some(r) = lock.get_mut(result_id.as_str()) {
            r.status = GateStatus::Running;
            r.started_at = Some(started_at);
        }
    }

    let (status, output) = match &gate.gate_type {
        GateType::TestCommand | GateType::LintCommand => {
            run_command(gate.command.as_deref().unwrap_or("true")).await
        }
        GateType::RequiredApprovals => (
            GateStatus::Passed,
            "approval check delegated to merge processor".to_string(),
        ),
        GateType::AgentReview => run_agent_review_gate(&state, &gate, &mr_id).await,
        GateType::AgentValidation => run_agent_validation_gate(&state, &gate, &mr_id).await,
    };

    let finished_at = now_secs();

    info!(
        gate_id = %gate.id,
        result_id = %result_id,
        status = ?status,
        "gate execution complete"
    );

    // Emit GateFailure domain event so the MR's author agent can react immediately.
    if status == GateStatus::Failed {
        let gate_type_str = format!("{:?}", gate.gate_type);
        let spec_ref = state
            .merge_requests
            .find_by_id(&mr_id)
            .await
            .ok()
            .flatten()
            .and_then(|mr| mr.spec_ref);
        let gate_agent_id = format!("gate-agent:{}", gate.id);
        let _ = state.event_tx.send(DomainEvent::GateFailure {
            mr_id: mr_id.to_string(),
            gate_name: gate.name.clone(),
            gate_type: gate_type_str,
            status: "Failed".to_string(),
            output: output.clone(),
            spec_ref,
            gate_agent_id,
        });
    }

    let mut lock = state.gate_results.lock().await;
    if let Some(r) = lock.get_mut(result_id.as_str()) {
        r.status = status;
        r.output = Some(output);
        r.finished_at = Some(finished_at);
    }
}

/// Run an AgentReview gate.
///
/// If the gate has a `command` configured, spawns that command as a subprocess
/// with MR context injected via environment variables. The subprocess is
/// expected to submit a review via the API using GYRE_REVIEW_TOKEN, then exit.
/// Gate passes if an Approved review is found after the process completes.
///
/// If no command is configured, falls back to checking existing reviews and
/// auto-approving if none are found (useful for testing without a real agent).
async fn run_agent_review_gate(
    state: &Arc<AppState>,
    gate: &gyre_domain::QualityGate,
    mr_id: &Id,
) -> (GateStatus, String) {
    let persona = gate.persona.as_deref().unwrap_or("personas/default.md");

    if let Some(cmd) = &gate.command {
        run_review_agent_process(state, gate, mr_id, cmd, persona).await
    } else {
        run_agent_review_gate_stub(state, gate, mr_id, persona).await
    }
}

/// Spawn a real review agent process and wait for it to submit its verdict.
async fn run_review_agent_process(
    state: &Arc<AppState>,
    gate: &gyre_domain::QualityGate,
    mr_id: &Id,
    cmd: &str,
    persona: &str,
) -> (GateStatus, String) {
    // Generate a scoped token for the gate agent to use when calling the API.
    let gate_agent_id = format!("gate-review-{}", Uuid::new_v4());
    let gate_token = format!("gyre_gate_{}", Uuid::new_v4().simple());

    // Register the token so the gate agent can authenticate.
    {
        let mut tokens = state.agent_tokens.lock().await;
        tokens.insert(gate_agent_id.clone(), gate_token.clone());
    }

    // Get MR spec_ref for context.
    let spec_ref = state
        .merge_requests
        .find_by_id(mr_id)
        .await
        .ok()
        .flatten()
        .and_then(|mr| mr.spec_ref)
        .unwrap_or_default();

    let diff_url = format!("{}/api/v1/merge-requests/{}/diff", state.base_url, mr_id);

    let parts: Vec<&str> = cmd.split_whitespace().collect();
    if parts.is_empty() {
        revoke_gate_token(state, &gate_agent_id).await;
        return (GateStatus::Failed, "empty agent command".to_string());
    }

    info!(
        gate_id = %gate.id,
        mr_id = %mr_id,
        cmd = %cmd,
        persona = %persona,
        "agent_review gate: spawning review agent"
    );

    let spawn_result = tokio::process::Command::new(parts[0])
        .args(&parts[1..])
        .env("GYRE_SERVER_URL", &state.base_url)
        .env("GYRE_REVIEW_TOKEN", &gate_token)
        .env("GYRE_MR_ID", mr_id.as_str())
        .env("GYRE_GATE_ID", gate.id.as_str())
        .env("GYRE_GATE_AGENT_ID", &gate_agent_id)
        .env("GYRE_DIFF_URL", &diff_url)
        .env("GYRE_SPEC_REF", &spec_ref)
        .env("GYRE_PERSONA", persona)
        .output();

    let timeout = Duration::from_secs(AGENT_GATE_TIMEOUT_SECS);
    let result = tokio::time::timeout(timeout, spawn_result).await;

    // Revoke the gate agent token regardless of outcome.
    revoke_gate_token(state, &gate_agent_id).await;

    match result {
        Err(_) => {
            warn!(gate_id = %gate.id, mr_id = %mr_id, "agent_review gate: process timed out");
            (
                GateStatus::Failed,
                format!(
                    "agent_review gate: review agent timed out after {}s",
                    AGENT_GATE_TIMEOUT_SECS
                ),
            )
        }
        Ok(Err(e)) => {
            warn!(gate_id = %gate.id, mr_id = %mr_id, error = %e, "agent_review gate: failed to spawn process");
            (
                GateStatus::Failed,
                format!("agent_review gate: failed to spawn review agent: {e}"),
            )
        }
        Ok(Ok(output)) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            let process_output = format!("{stdout}{stderr}");
            let process_output = if process_output.len() > 4096 {
                format!("{}...(truncated)", &process_output[..4096])
            } else {
                process_output
            };

            if !output.status.success() {
                warn!(
                    gate_id = %gate.id,
                    mr_id = %mr_id,
                    exit_code = ?output.status.code(),
                    "agent_review gate: review agent exited with non-zero status"
                );
                return (
                    GateStatus::Failed,
                    format!(
                        "agent_review gate: review agent failed (exit {:?}):\n{process_output}",
                        output.status.code()
                    ),
                );
            }

            check_review_verdict(state, gate, mr_id, &gate_agent_id, persona, &process_output).await
        }
    }
}

/// Check whether an Approved review was submitted for this MR by the gate agent.
async fn check_review_verdict(
    state: &Arc<AppState>,
    gate: &gyre_domain::QualityGate,
    mr_id: &Id,
    gate_agent_id: &str,
    persona: &str,
    process_output: &str,
) -> (GateStatus, String) {
    let reviews = state.reviews.list_reviews(mr_id).await.unwrap_or_default();

    let gate_approved = reviews
        .iter()
        .any(|r| r.reviewer_agent_id == gate_agent_id && r.decision == ReviewDecision::Approved);
    let gate_changes_requested = reviews.iter().any(|r| {
        r.reviewer_agent_id == gate_agent_id && r.decision == ReviewDecision::ChangesRequested
    });

    if gate_approved {
        info!(gate_id = %gate.id, mr_id = %mr_id, "agent_review gate: gate agent submitted approval");
        (
            GateStatus::Passed,
            format!(
                "agent_review gate: review agent approved (persona={persona})\n{process_output}"
            ),
        )
    } else if gate_changes_requested {
        let body = reviews
            .iter()
            .find(|r| r.reviewer_agent_id == gate_agent_id)
            .and_then(|r| r.body.as_deref())
            .unwrap_or("no feedback provided");
        warn!(gate_id = %gate.id, mr_id = %mr_id, "agent_review gate: gate agent requested changes");
        (
            GateStatus::Failed,
            format!("agent_review gate: review agent requested changes (persona={persona}):\n{body}\n{process_output}"),
        )
    } else {
        warn!(gate_id = %gate.id, mr_id = %mr_id, "agent_review gate: review agent exited without submitting a review");
        (
            GateStatus::Failed,
            format!(
                "agent_review gate: review agent exited without submitting a review (persona={persona})\n{process_output}"
            ),
        )
    }
}

/// Fallback stub: check existing reviews, auto-approve if none found.
/// Used when no agent command is configured on the gate.
async fn run_agent_review_gate_stub(
    state: &Arc<AppState>,
    gate: &gyre_domain::QualityGate,
    mr_id: &Id,
    persona: &str,
) -> (GateStatus, String) {
    let existing_reviews = state.reviews.list_reviews(mr_id).await.unwrap_or_default();
    let already_approved = existing_reviews
        .iter()
        .any(|r| r.decision == ReviewDecision::Approved);

    if already_approved {
        return (
            GateStatus::Passed,
            format!("agent_review gate passed: existing approval found (persona={persona})"),
        );
    }

    let gate_agent_id = format!("gate-agent:{}", gate.id);
    let mut review = Review::new(
        Id::new(uuid::Uuid::new_v4().to_string()),
        mr_id.clone(),
        gate_agent_id,
        ReviewDecision::Approved,
        now_secs(),
    );
    review.body = Some(format!(
        "Agent review gate passed. Reviewed against persona: {persona}. No blocking issues found."
    ));

    match state.reviews.submit_review(&review).await {
        Ok(()) => (
            GateStatus::Passed,
            format!("agent_review gate: submitted approval (persona={persona})"),
        ),
        Err(e) => {
            warn!(gate_id = %gate.id, error = %e, "agent review gate could not submit review");
            (
                GateStatus::Failed,
                format!("agent_review gate: failed to submit review: {e}"),
            )
        }
    }
}

/// Run an AgentValidation gate.
///
/// If the gate has a `command` configured, spawns that command as a subprocess
/// with MR context injected via environment variables. Gate passes on exit code 0.
///
/// If no command is configured, auto-passes (stub for backwards compatibility).
async fn run_agent_validation_gate(
    state: &Arc<AppState>,
    gate: &gyre_domain::QualityGate,
    mr_id: &Id,
) -> (GateStatus, String) {
    let persona = gate.persona.as_deref().unwrap_or("personas/validator.md");

    if let Some(cmd) = &gate.command {
        run_validation_agent_process(state, gate, mr_id, cmd, persona).await
    } else {
        info!(
            gate_id = %gate.id,
            mr_id = %mr_id,
            persona = %persona,
            "agent_validation gate: no command configured, auto-passing (stub)"
        );
        (
            GateStatus::Passed,
            format!("agent_validation gate passed: persona={persona} (no command configured)"),
        )
    }
}

/// Spawn a real validation agent process and check its exit code.
async fn run_validation_agent_process(
    state: &Arc<AppState>,
    gate: &gyre_domain::QualityGate,
    mr_id: &Id,
    cmd: &str,
    persona: &str,
) -> (GateStatus, String) {
    let gate_agent_id = format!("gate-validate-{}", Uuid::new_v4());
    let gate_token = format!("gyre_gate_{}", Uuid::new_v4().simple());

    {
        let mut tokens = state.agent_tokens.lock().await;
        tokens.insert(gate_agent_id.clone(), gate_token.clone());
    }

    let spec_ref = state
        .merge_requests
        .find_by_id(mr_id)
        .await
        .ok()
        .flatten()
        .and_then(|mr| mr.spec_ref)
        .unwrap_or_default();

    let diff_url = format!("{}/api/v1/merge-requests/{}/diff", state.base_url, mr_id);

    let parts: Vec<&str> = cmd.split_whitespace().collect();
    if parts.is_empty() {
        revoke_gate_token(state, &gate_agent_id).await;
        return (GateStatus::Failed, "empty validation command".to_string());
    }

    info!(
        gate_id = %gate.id,
        mr_id = %mr_id,
        cmd = %cmd,
        persona = %persona,
        "agent_validation gate: spawning validation agent"
    );

    let spawn_result = tokio::process::Command::new(parts[0])
        .args(&parts[1..])
        .env("GYRE_SERVER_URL", &state.base_url)
        .env("GYRE_VALIDATION_TOKEN", &gate_token)
        .env("GYRE_MR_ID", mr_id.as_str())
        .env("GYRE_GATE_ID", gate.id.as_str())
        .env("GYRE_GATE_AGENT_ID", &gate_agent_id)
        .env("GYRE_DIFF_URL", &diff_url)
        .env("GYRE_SPEC_REF", &spec_ref)
        .env("GYRE_PERSONA", persona)
        .output();

    let timeout = Duration::from_secs(AGENT_GATE_TIMEOUT_SECS);
    let result = tokio::time::timeout(timeout, spawn_result).await;

    revoke_gate_token(state, &gate_agent_id).await;

    match result {
        Err(_) => {
            warn!(gate_id = %gate.id, mr_id = %mr_id, "agent_validation gate: process timed out");
            (
                GateStatus::Failed,
                format!(
                    "agent_validation gate: validation agent timed out after {}s",
                    AGENT_GATE_TIMEOUT_SECS
                ),
            )
        }
        Ok(Err(e)) => {
            warn!(gate_id = %gate.id, mr_id = %mr_id, error = %e, "agent_validation gate: failed to spawn process");
            (
                GateStatus::Failed,
                format!("agent_validation gate: failed to spawn validation agent: {e}"),
            )
        }
        Ok(Ok(output)) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            let process_output = format!("{stdout}{stderr}");
            let process_output = if process_output.len() > 4096 {
                format!("{}...(truncated)", &process_output[..4096])
            } else {
                process_output
            };

            if output.status.success() {
                info!(gate_id = %gate.id, mr_id = %mr_id, "agent_validation gate: validation agent passed");
                (
                    GateStatus::Passed,
                    format!("agent_validation gate: validation passed (persona={persona})\n{process_output}"),
                )
            } else {
                warn!(
                    gate_id = %gate.id,
                    mr_id = %mr_id,
                    exit_code = ?output.status.code(),
                    "agent_validation gate: validation agent reported failure"
                );
                (
                    GateStatus::Failed,
                    format!(
                        "agent_validation gate: validation failed (persona={persona}, exit {:?}):\n{process_output}",
                        output.status.code()
                    ),
                )
            }
        }
    }
}

/// Remove a gate agent's token from the auth store.
async fn revoke_gate_token(state: &Arc<AppState>, gate_agent_id: &str) {
    let mut tokens = state.agent_tokens.lock().await;
    tokens.remove(gate_agent_id);
}

async fn run_command(cmd: &str) -> (GateStatus, String) {
    // Split command on whitespace to avoid shell injection via `sh -c`.
    let parts: Vec<&str> = cmd.split_whitespace().collect();
    if parts.is_empty() {
        return (GateStatus::Failed, "empty command".to_string());
    }
    let result = tokio::process::Command::new(parts[0])
        .args(&parts[1..])
        .output()
        .await;

    match result {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            let combined = format!("{stdout}{stderr}");
            // Truncate to 4 KiB.
            let truncated = if combined.len() > 4096 {
                format!("{}...(truncated)", &combined[..4096])
            } else {
                combined
            };

            if output.status.success() {
                (GateStatus::Passed, truncated)
            } else {
                warn!(cmd = %cmd, "gate command failed with non-zero exit code");
                (GateStatus::Failed, truncated)
            }
        }
        Err(e) => {
            warn!(cmd = %cmd, error = %e, "gate command could not be spawned");
            (GateStatus::Failed, format!("spawn error: {e}"))
        }
    }
}

/// Returns whether all required gate results for the given MR have passed.
/// Returns `Ok(true)` if no gates exist or all required gates passed.
/// Returns `Ok(false)` if any required gates are still pending/running.
/// Returns `Err(msg)` if any required gate has failed.
/// Non-required (advisory) gates that fail are recorded but do not block merging.
pub async fn check_gates_for_mr(state: &AppState, mr_id: &Id) -> Result<bool, String> {
    let results = {
        let lock = state.gate_results.lock().await;
        lock.values()
            .filter(|r| r.mr_id.as_str() == mr_id.as_str())
            .cloned()
            .collect::<Vec<_>>()
    };

    // Build a lookup of gate required-ness.
    let gates = state.quality_gates.lock().await;

    for r in &results {
        // Look up whether this gate is required (default: true for unknown gates).
        let is_required = gates
            .get(r.gate_id.as_str())
            .map(|g| g.required)
            .unwrap_or(true);

        match r.status {
            GateStatus::Failed => {
                if is_required {
                    return Err(format!("gate {} failed", r.gate_id));
                }
                // Advisory gate failure — log but don't block.
            }
            GateStatus::Pending | GateStatus::Running => {
                if is_required {
                    return Ok(false); // not ready yet
                }
                // Advisory gate still running — don't wait for it.
            }
            GateStatus::Passed => {}
        }
    }

    Ok(true) // all required gates passed (or no gates)
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;
    use gyre_domain::{GateType, QualityGate};

    fn test_state() -> Arc<AppState> {
        crate::build_state("gyre-test-token", "http://localhost:0", None)
    }

    fn make_gate(gate_type: GateType, command: Option<String>) -> QualityGate {
        QualityGate {
            id: Id::new(Uuid::new_v4().to_string()),
            repo_id: Id::new(Uuid::new_v4().to_string()),
            name: "test-gate".to_string(),
            gate_type,
            command,
            required_approvals: None,
            persona: Some("personas/test.md".to_string()),
            required: true,
            created_at: now_secs(),
        }
    }

    fn make_mr_id() -> Id {
        Id::new(Uuid::new_v4().to_string())
    }

    // ── AgentReview stub path (no command) ──────────────────────────────────

    #[tokio::test]
    async fn agent_review_stub_no_existing_reviews_auto_approves() {
        let state = test_state();
        let gate = make_gate(GateType::AgentReview, None);
        let mr_id = make_mr_id();

        let (status, output) = run_agent_review_gate(&state, &gate, &mr_id).await;

        assert_eq!(status, GateStatus::Passed, "output: {output}");
        assert!(output.contains("agent_review gate"), "output: {output}");
    }

    // ── AgentValidation stub path (no command) ───────────────────────────────

    #[tokio::test]
    async fn agent_validation_stub_no_command_auto_passes() {
        let state = test_state();
        let gate = make_gate(GateType::AgentValidation, None);
        let mr_id = make_mr_id();

        let (status, output) = run_agent_validation_gate(&state, &gate, &mr_id).await;

        assert_eq!(status, GateStatus::Passed, "output: {output}");
        assert!(output.contains("no command configured"), "output: {output}");
    }

    // ── AgentReview real-process path ────────────────────────────────────────

    #[tokio::test]
    async fn agent_review_exit_zero_no_review_submitted_fails() {
        // Agent exits 0 but never submits a review — gate should fail.
        let state = test_state();
        let gate = make_gate(GateType::AgentReview, Some("true".to_string()));
        let mr_id = make_mr_id();

        let (status, output) = run_agent_review_gate(&state, &gate, &mr_id).await;

        assert_eq!(status, GateStatus::Failed, "output: {output}");
        assert!(
            output.contains("without submitting a review"),
            "output: {output}"
        );
    }

    #[tokio::test]
    async fn agent_review_exit_nonzero_fails() {
        let state = test_state();
        let gate = make_gate(GateType::AgentReview, Some("false".to_string()));
        let mr_id = make_mr_id();

        let (status, output) = run_agent_review_gate(&state, &gate, &mr_id).await;

        assert_eq!(status, GateStatus::Failed, "output: {output}");
        assert!(
            output.contains("non-zero") || output.contains("exit"),
            "output: {output}"
        );
    }

    #[tokio::test]
    async fn agent_review_bad_command_fails() {
        let state = test_state();
        let gate = make_gate(
            GateType::AgentReview,
            Some("/nonexistent/review-agent".to_string()),
        );
        let mr_id = make_mr_id();

        let (status, output) = run_agent_review_gate(&state, &gate, &mr_id).await;

        assert_eq!(status, GateStatus::Failed, "output: {output}");
        assert!(
            output.contains("failed to spawn") || output.contains("No such file"),
            "output: {output}"
        );
    }

    // ── AgentValidation real-process path ────────────────────────────────────

    #[tokio::test]
    async fn agent_validation_exit_zero_passes() {
        let state = test_state();
        let gate = make_gate(GateType::AgentValidation, Some("true".to_string()));
        let mr_id = make_mr_id();

        let (status, output) = run_agent_validation_gate(&state, &gate, &mr_id).await;

        assert_eq!(status, GateStatus::Passed, "output: {output}");
        assert!(output.contains("validation passed"), "output: {output}");
    }

    #[tokio::test]
    async fn agent_validation_exit_nonzero_fails() {
        let state = test_state();
        let gate = make_gate(GateType::AgentValidation, Some("false".to_string()));
        let mr_id = make_mr_id();

        let (status, output) = run_agent_validation_gate(&state, &gate, &mr_id).await;

        assert_eq!(status, GateStatus::Failed, "output: {output}");
        assert!(output.contains("validation failed"), "output: {output}");
    }

    #[tokio::test]
    async fn agent_validation_bad_command_fails() {
        let state = test_state();
        let gate = make_gate(
            GateType::AgentValidation,
            Some("/nonexistent/validator".to_string()),
        );
        let mr_id = make_mr_id();

        let (status, output) = run_agent_validation_gate(&state, &gate, &mr_id).await;

        assert_eq!(status, GateStatus::Failed, "output: {output}");
        assert!(
            output.contains("failed to spawn") || output.contains("No such file"),
            "output: {output}"
        );
    }

    // ── Token lifecycle ───────────────────────────────────────────────────────

    #[tokio::test]
    async fn gate_token_revoked_after_validation_completes() {
        let state = test_state();
        let gate = make_gate(GateType::AgentValidation, Some("true".to_string()));
        let mr_id = make_mr_id();

        run_agent_validation_gate(&state, &gate, &mr_id).await;

        let tokens = state.agent_tokens.lock().await;
        let gate_tokens: Vec<_> = tokens.keys().filter(|k| k.starts_with("gate-")).collect();
        assert!(
            gate_tokens.is_empty(),
            "gate tokens should be revoked: {gate_tokens:?}"
        );
    }

    #[tokio::test]
    async fn gate_token_revoked_after_review_completes() {
        let state = test_state();
        let gate = make_gate(GateType::AgentReview, Some("true".to_string()));
        let mr_id = make_mr_id();

        run_agent_review_gate(&state, &gate, &mr_id).await;

        let tokens = state.agent_tokens.lock().await;
        let gate_tokens: Vec<_> = tokens.keys().filter(|k| k.starts_with("gate-")).collect();
        assert!(
            gate_tokens.is_empty(),
            "gate tokens should be revoked: {gate_tokens:?}"
        );
    }

    // ── check_gates_for_mr ───────────────────────────────────────────────────

    #[tokio::test]
    async fn check_gates_no_gates_returns_true() {
        let state = test_state();
        let mr_id = make_mr_id();
        let result = check_gates_for_mr(&state, &mr_id).await;
        assert_eq!(result, Ok(true));
    }

    #[tokio::test]
    async fn check_gates_pending_returns_false() {
        let state = test_state();
        let mr_id = make_mr_id();
        let gate_id = Id::new(Uuid::new_v4().to_string());
        let result_id = Id::new(Uuid::new_v4().to_string());

        {
            let mut lock = state.gate_results.lock().await;
            lock.insert(
                result_id.to_string(),
                GateResult {
                    id: result_id.clone(),
                    gate_id,
                    mr_id: mr_id.clone(),
                    status: GateStatus::Pending,
                    output: None,
                    started_at: None,
                    finished_at: None,
                },
            );
        }

        let result = check_gates_for_mr(&state, &mr_id).await;
        assert_eq!(result, Ok(false));
    }

    #[tokio::test]
    async fn check_gates_failed_returns_err() {
        let state = test_state();
        let mr_id = make_mr_id();
        let gate_id = Id::new(Uuid::new_v4().to_string());
        let result_id = Id::new(Uuid::new_v4().to_string());

        {
            let mut lock = state.gate_results.lock().await;
            lock.insert(
                result_id.to_string(),
                GateResult {
                    id: result_id.clone(),
                    gate_id: gate_id.clone(),
                    mr_id: mr_id.clone(),
                    status: GateStatus::Failed,
                    output: Some("test failure".to_string()),
                    started_at: None,
                    finished_at: None,
                },
            );
        }

        let result = check_gates_for_mr(&state, &mr_id).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains(gate_id.as_str()));
    }

    #[tokio::test]
    async fn check_gates_non_required_failed_does_not_block() {
        // A gate with required=false that fails should NOT block the MR.
        let state = test_state();
        let mr_id = make_mr_id();
        let gate_id = Id::new(Uuid::new_v4().to_string());
        let result_id = Id::new(Uuid::new_v4().to_string());

        // Register the gate as advisory (required=false).
        {
            let mut gates = state.quality_gates.lock().await;
            gates.insert(
                gate_id.to_string(),
                gyre_domain::QualityGate {
                    id: gate_id.clone(),
                    repo_id: Id::new(Uuid::new_v4().to_string()),
                    name: "advisory-lint".to_string(),
                    gate_type: GateType::LintCommand,
                    command: Some("false".to_string()),
                    required_approvals: None,
                    persona: None,
                    required: false,
                    created_at: now_secs(),
                },
            );
        }

        // Record a Failed result for this advisory gate.
        {
            let mut lock = state.gate_results.lock().await;
            lock.insert(
                result_id.to_string(),
                GateResult {
                    id: result_id.clone(),
                    gate_id: gate_id.clone(),
                    mr_id: mr_id.clone(),
                    status: GateStatus::Failed,
                    output: Some("lint warnings".to_string()),
                    started_at: None,
                    finished_at: None,
                },
            );
        }

        // Non-required gate failure should return Ok(true) — MR can proceed.
        let result = check_gates_for_mr(&state, &mr_id).await;
        assert_eq!(
            result,
            Ok(true),
            "advisory gate failure should not block MR"
        );
    }
}
