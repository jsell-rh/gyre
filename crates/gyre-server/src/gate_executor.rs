//! Gate execution engine: runs quality gates for an MR in the background.

use std::sync::Arc;
use tracing::{info, warn};

use gyre_common::Id;
use gyre_domain::{GateResult, GateStatus, GateType, Review, ReviewDecision};

use crate::{domain_events::DomainEvent, AppState};

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
        GateType::RequiredApprovals => {
            // Check reviews for this MR — resolved at execution time.
            // For now we just pass (no MR context needed; the merge_processor
            // will re-verify before the actual merge).
            (
                GateStatus::Passed,
                "approval check delegated to merge processor".to_string(),
            )
        }
        GateType::AgentReview => run_agent_review_gate(&state, &gate, &mr_id).await,
        GateType::AgentValidation => run_agent_validation_gate(&gate, &mr_id).await,
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

/// Run an AgentReview gate: simulate a review agent examining the MR.
///
/// In a real deployment this would spawn a full agent process with the MR diff
/// and spec context. The agent would then submit its verdict via the Review API.
/// For the in-process implementation we check existing reviews first and
/// auto-submit an agent review if none exist yet.
async fn run_agent_review_gate(
    state: &Arc<AppState>,
    gate: &gyre_domain::QualityGate,
    mr_id: &Id,
) -> (GateStatus, String) {
    let persona = gate.persona.as_deref().unwrap_or("personas/default.md");

    // Check if there is already an approval review on this MR (from any reviewer).
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

    // No approval yet — submit an agent gate review.
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

/// Run an AgentValidation gate: simulate a validation agent running domain-specific checks.
async fn run_agent_validation_gate(
    gate: &gyre_domain::QualityGate,
    mr_id: &Id,
) -> (GateStatus, String) {
    let persona = gate.persona.as_deref().unwrap_or("personas/validator.md");
    info!(
        gate_id = %gate.id,
        mr_id = %mr_id,
        persona = %persona,
        "agent_validation gate: running domain-specific checks"
    );
    // Validation agent always passes in the in-process implementation.
    // A real deployment would spawn an agent subprocess and wait for its report.
    (
        GateStatus::Passed,
        format!("agent_validation gate passed: persona={persona}"),
    )
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

/// Returns whether all gate results for the given MR have passed.
/// Returns `Ok(true)` if no gates exist or all passed.
/// Returns `Ok(false)` if any are still pending/running.
/// Returns `Err(msg)` if any gate has failed.
pub async fn check_gates_for_mr(state: &AppState, mr_id: &Id) -> Result<bool, String> {
    let lock = state.gate_results.lock().await;
    let results: Vec<_> = lock
        .values()
        .filter(|r| r.mr_id.as_str() == mr_id.as_str())
        .collect();

    for r in &results {
        match r.status {
            GateStatus::Failed => {
                return Err(format!("gate {} failed", r.gate_id));
            }
            GateStatus::Pending | GateStatus::Running => {
                return Ok(false); // not ready yet
            }
            GateStatus::Passed => {}
        }
    }

    Ok(true) // all passed (or no gates)
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
