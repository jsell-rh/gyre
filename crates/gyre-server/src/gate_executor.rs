//! Gate execution engine: runs quality gates for an MR in the background.

use std::sync::Arc;
use std::time::Duration;
use tracing::{info, warn};
use uuid::Uuid;

use gyre_common::attestation::GateAttestation;
use gyre_common::Id;
use gyre_domain::{GateResult, GateStatus, GateType, ReviewDecision};

use crate::otlp_receiver::TraceCaptureConfig;
use crate::AppState;

/// Default timeout for agent-based gates (5 minutes).
const AGENT_GATE_TIMEOUT_SECS: u64 = 300;

/// Maximum output length to include in gate attestation hash.
const GATE_ATTESTATION_OUTPUT_LIMIT: usize = 4096;

/// Create pending GateResult records for all gates belonging to the MR's repo,
/// then spawn a background task that runs each gate and updates the result.
pub async fn trigger_gates_for_mr(state: Arc<AppState>, mr_id: Id, repo_id: Id) {
    // Collect gates for this repo.
    let gates = state
        .quality_gates
        .list_by_repo_id(repo_id.as_str())
        .await
        .unwrap_or_default();

    if gates.is_empty() {
        return;
    }

    // Create Pending GateResult for each gate.
    let mut result_ids: Vec<(Id, gyre_domain::QualityGate)> = Vec::new();
    for gate in &gates {
        let result_id = Id::new(uuid::Uuid::new_v4().to_string());
        let result = GateResult {
            id: result_id.clone(),
            gate_id: gate.id.clone(),
            mr_id: mr_id.clone(),
            status: GateStatus::Pending,
            output: None,
            started_at: None,
            finished_at: None,
        };
        let _ = state.gate_results.save(&result).await;
        result_ids.push((result_id, gate.clone()));
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
    let _ = state
        .gate_results
        .update_status(
            result_id.as_str(),
            GateStatus::Running,
            Some(started_at),
            None,
            None,
        )
        .await;

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
        GateType::TraceCapture => run_trace_capture_gate(&state, &gate, &mr_id, &result_id).await,
    };

    let finished_at = now_secs();

    info!(
        gate_id = %gate.id,
        result_id = %result_id,
        status = ?status,
        "gate execution complete"
    );

    // Emit GateFailure event so the MR's author agent can react immediately.
    if status == GateStatus::Failed {
        let gate_type_str = format!("{:?}", gate.gate_type);
        let (spec_ref, workspace_id) = state
            .merge_requests
            .find_by_id(&mr_id)
            .await
            .ok()
            .flatten()
            .map(|mr| (mr.spec_ref, Some(mr.workspace_id)))
            .unwrap_or((None, None));
        let gate_agent_id = format!("gate-agent:{}", gate.id);
        let ws_id = workspace_id.unwrap_or_else(|| gyre_common::Id::new("default"));
        state
            .emit_event(
                Some(ws_id.clone()),
                gyre_common::message::Destination::Workspace(ws_id),
                gyre_common::message::MessageKind::GateFailure,
                Some(serde_json::json!({
                    "mr_id": mr_id.to_string(),
                    "gate_name": gate.name,
                    "gate_type": gate_type_str,
                    "status": "Failed",
                    "output": output,
                    "spec_ref": spec_ref,
                    "gate_agent_id": gate_agent_id,
                })),
            )
            .await;
    }

    // Notify MR author when gate fails (HSI §2).
    if status == GateStatus::Failed {
        if let Ok(Some(mr)) = state.merge_requests.find_by_id(&mr_id).await {
            if let Some(ref author_id) = mr.author_agent_id {
                crate::notifications::notify_gate_failure(
                    state.as_ref(),
                    author_id,
                    &mr.workspace_id,
                    &mr_id.to_string(),
                    &gate.name,
                    "default",
                )
                .await;
            }
        }
    }

    // Retry up to 3 times with backoff — concurrent gate writers can
    // contend on the SQLite write lock even with busy_timeout set.
    let mut last_err = None;
    for attempt in 0..3 {
        if attempt > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(100 * (1 << attempt))).await;
        }
        match state
            .gate_results
            .update_status(
                result_id.as_str(),
                status.clone(),
                None,
                Some(finished_at),
                Some(output.clone()),
            )
            .await
        {
            Ok(()) => {
                last_err = None;
                break;
            }
            Err(e) => {
                warn!(
                    gate_id = %gate.id,
                    result_id = %result_id,
                    attempt,
                    error = format!("{e:#}"),
                    "gate result update failed, retrying"
                );
                last_err = Some(e);
            }
        }
    }
    if let Some(e) = last_err {
        warn!(
            gate_id = %gate.id,
            result_id = %result_id,
            error = format!("{e:#}"),
            "failed to persist final gate result status after retries"
        );
    }

    // Produce a GateAttestation record (§3.2, §5.1) and store it
    // alongside the chain attestation for the MR's task.
    produce_gate_attestation(&state, &gate, &mr_id, &status, &output).await;
}

/// Produce a signed `GateAttestation` record for a completed gate (§3.2, §5.1).
///
/// The gate attestation is signed with the server's Ed25519 key and stored in
/// the chain attestation repository alongside the MR's task attestation chain.
/// Gate attestations enable merge-time verification to include gate constraints.
async fn produce_gate_attestation(
    state: &Arc<AppState>,
    gate: &gyre_domain::QualityGate,
    mr_id: &Id,
    status: &GateStatus,
    output: &str,
) {
    // Look up the MR to find the task and workspace.
    let mr = match state.merge_requests.find_by_id(mr_id).await.ok().flatten() {
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
        state
            .agents
            .find_by_id(&Id::new(&agent_id))
            .await
            .ok()
            .flatten()
            .and_then(|a| a.current_task_id.map(|id| id.to_string()))
    } else {
        None
    };

    let Some(task_id) = task_id else {
        return; // No task — cannot attach gate attestation to chain
    };

    // Find the existing attestation chain for this task.
    let attestations = match state.chain_attestations.find_by_task(&task_id).await {
        Ok(atts) if !atts.is_empty() => atts,
        _ => return, // No attestation chain — skip gate attestation
    };

    // Find the leaf attestation (highest chain_depth).
    let leaf = attestations.iter().max_by_key(|a| a.metadata.chain_depth);

    let Some(leaf) = leaf else {
        return;
    };

    // Build the output hash from the gate output text, truncated on a UTF-8
    // char boundary (external process output may be multibyte).
    let output_truncated = truncate_bytes(output, GATE_ATTESTATION_OUTPUT_LIMIT);
    let output_hash = {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(output_truncated.as_bytes());
        hasher.finalize().to_vec()
    };

    // Build the key binding for the gate signer (platform key).
    let key_binding = gyre_common::KeyBinding {
        public_key: state.agent_signing_key.public_key_bytes.clone(),
        user_identity: format!("gate-agent:{}", gate.id),
        issuer: state.base_url.clone(),
        trust_anchor_id: "gyre-platform".to_string(),
        issued_at: now_secs(),
        expires_at: now_secs() + 3600,
        user_signature: vec![], // Platform gates don't have user signatures
        platform_countersign: vec![],
    };

    // Build the GateAttestation record first, then sign using the shared
    // signable_bytes() helper to ensure sign/verify message parity (checklist §44).
    // GateType and GateStatus in gyre_domain are re-exported from gyre_common,
    // so they are the same type — no conversion needed.
    let mut gate_attestation = GateAttestation {
        gate_id: gate.id.to_string(),
        gate_name: gate.name.clone(),
        gate_type: gate.gate_type.clone(),
        status: status.clone(),
        output_hash,
        constraint: None, // Gate constraints are attached by review/validation agents
        signature: vec![], // Populated below after signing
        key_binding,
    };

    // Sign the gate attestation content using the shared signable_bytes() helper.
    let sign_bytes = gate_attestation.signable_bytes();
    gate_attestation.signature = state.agent_signing_key.sign_bytes(&sign_bytes);

    // Update the leaf attestation with this gate result appended.
    let mut updated_leaf = leaf.clone();
    updated_leaf.output.gate_results.push(gate_attestation);

    // Re-save the updated leaf attestation.
    if let Err(e) = state.chain_attestations.save(&updated_leaf).await {
        warn!(
            gate_id = %gate.id,
            mr_id = %mr_id,
            error = %e,
            "failed to persist gate attestation"
        );
    } else {
        // §5.3 location 2: write chain attestation as git note.
        crate::attestation::write_chain_note_if_committed(state, &updated_leaf).await;

        // §7.7: attestation.created audit event for gate attestation.
        info!(
            gate_id = %gate.id,
            mr_id = %mr_id,
            task_id = %task_id,
            category = "Provenance",
            event = "attestation.created",
            "attestation.created: gate attestation produced for gate {} (task {})",
            gate.name, task_id
        );
    }
}

/// Run an AgentReview gate (agent-gates.md §AgentReview Gate).
///
/// The gate spawns a review agent process configured via `gate.command`
/// with the full MR context injected as environment variables:
/// the MR diff (full patch), the referenced spec at the SHA pinned in the
/// MR's `spec_ref`, the MR title, and the review persona's system prompt
/// (resolved nearest-wins: repo → workspace → tenant).
///
/// The agent authenticates with a scoped JWT carrying `review:submit` only —
/// git push is denied for that scope and the Review API binds the reviewer
/// identity to the token subject. The agent submits its verdict via
/// `POST /api/v1/merge-requests/:id/reviews`; Approved → Passed,
/// ChangesRequested → Failed. The token is revoked after the process exits
/// (single-minded agents: one review, then teardown).
///
/// A gate with no `command` configured cannot spawn a reviewer, so it fails
/// ("cannot determine state" is not "state is fine") — required gates block,
/// advisory gates record the failure without blocking.
async fn run_agent_review_gate(
    state: &Arc<AppState>,
    gate: &gyre_domain::QualityGate,
    mr_id: &Id,
) -> (GateStatus, String) {
    let persona = gate.persona.as_deref().unwrap_or("personas/default.md");

    match &gate.command {
        Some(cmd) => run_review_agent_process(state, gate, mr_id, cmd, persona).await,
        None => {
            warn!(
                gate_id = %gate.id,
                mr_id = %mr_id,
                "agent_review gate: no agent command configured; cannot spawn a reviewer"
            );
            (
                GateStatus::Failed,
                format!(
                    "agent_review gate failed: no agent command configured (persona={persona}); \
                     configure the gate's command to spawn a review agent"
                ),
            )
        }
    }
}

/// The MR context bundle handed to a spawned review agent (§AgentReview Gate
/// step 1): diff, spec at pinned SHA, MR description + acceptance criteria,
/// persona system prompt, and the scoped reviewer identity.
struct ReviewAgentContext {
    /// Ephemeral agent id (also the scoped JWT `sub`).
    gate_agent_id: String,
    /// Scoped JWT with `review:submit` only.
    token: String,
    /// Full unified diff text (all file patches concatenated).
    diff: String,
    /// Spec content at the SHA pinned in the MR's `spec_ref`, when resolvable.
    spec_content: Option<String>,
    /// The MR's spec reference ("path@sha") verbatim.
    spec_ref: String,
    /// MR title (the MR model has no separate title-level body field).
    mr_title: String,
    /// Description + acceptance criteria from the task the MR implements,
    /// resolved through the author agent (`MR → agent → task.description`).
    /// The task template carries the acceptance criteria the reviewer must
    /// check the change against; `None` when the MR has no linked task.
    task_description: Option<String>,
    /// Persona system prompt after nearest-wins resolution.
    persona_prompt: String,
    /// Slug the persona was resolved from (for attribution in the verdict).
    persona_slug: String,
}

/// Derive the persona lookup slug from a configured persona reference.
///
/// Gate configs store persona references as paths (`personas/security.md`),
/// matching the spec's example; the persona store keys on slug
/// (`security`). Falls back to the raw string when it is not a path.
fn persona_slug(persona: &str) -> &str {
    let stem = persona.rsplit('/').next().unwrap_or(persona);
    stem.strip_suffix(".md").unwrap_or(stem)
}

/// Resolve a review persona nearest-wins (repo → workspace → tenant) for the
/// MR's workspace, mirroring `personas::resolve_persona`.
async fn resolve_review_persona(
    state: &Arc<AppState>,
    repo: &gyre_domain::Repository,
    slug: &str,
) -> Option<gyre_domain::Persona> {
    let workspace = state
        .workspaces
        .find_by_id(&repo.workspace_id)
        .await
        .ok()
        .flatten();
    let tenant_id = workspace
        .map(|ws| ws.tenant_id)
        .unwrap_or_else(|| Id::new("default"));

    for scope in [
        gyre_domain::PersonaScope::Repo(repo.id.clone()),
        gyre_domain::PersonaScope::Workspace(repo.workspace_id.clone()),
        gyre_domain::PersonaScope::Tenant(tenant_id),
    ] {
        if let Ok(Some(persona)) = state.personas.find_by_slug_and_scope(slug, &scope).await {
            return Some(persona);
        }
    }
    None
}

/// Gather the MR context and mint the scoped reviewer identity for a gate
/// agent (§AgentReview Gate step 1).
///
/// Persona resolution failure fails the gate before any process is spawned:
/// a review against an unresolvable persona is not a review.
async fn build_review_agent_context(
    state: &Arc<AppState>,
    gate: &gyre_domain::QualityGate,
    mr_id: &Id,
    persona: &str,
) -> Result<ReviewAgentContext, String> {
    let mr = state
        .merge_requests
        .find_by_id(mr_id)
        .await
        .map_err(|e| format!("failed to load MR {mr_id}: {e:#}"))?
        .ok_or_else(|| format!("merge request {mr_id} not found"))?;
    let repo = state
        .repos
        .find_by_id(&mr.repository_id)
        .await
        .map_err(|e| format!("failed to load repo {}: {e:#}", mr.repository_id))?
        .ok_or_else(|| format!("repository {} not found", mr.repository_id))?;

    // Persona resolution (nearest-wins). Unresolvable persona = gate failure:
    // the reviewer would judge against criteria nobody defined.
    let slug = persona_slug(persona);
    let resolved = resolve_review_persona(state, &repo, slug).await.ok_or_else(|| {
        format!(
            "review persona '{persona}' (slug '{slug}') not found in scope chain \
             (repo {}, workspace {}, tenant)",
             repo.id, repo.workspace_id
        )
    })?;

    // MR diff: full patch text, mirroring the MR diff endpoint's
    // source-vs-target computation.
    let diff = state
        .git_ops
        .diff(&repo.path, &mr.target_branch, &mr.source_branch)
        .await
        .map_err(|e| format!("failed to compute MR diff: {e:#}"))?;
    let diff_text = diff
        .patches
        .iter()
        .filter_map(|p| p.patch.as_deref())
        .collect::<Vec<_>>()
        .join("\n");

    // Spec content at the SHA pinned in the MR's spec_ref. The reviewer must
    // judge against the spec the MR was authored under — the branch tip may
    // have moved since. An unresolvable pinned SHA is a context-gathering
    // failure (fail-closed), not an empty spec.
    let spec_ref = mr.spec_ref.clone().unwrap_or_default();
    let spec_content = if let Some((path, sha)) = spec_ref.rsplit_once('@') {
        match state
            .git_ops
            .read_file_at_commit(&repo.path, sha, path)
            .await
        {
            Ok(Some(bytes)) => Some(
                String::from_utf8(bytes).map_err(|_| {
                    format!("spec '{path}' at {sha} is not valid UTF-8")
                })?,
            ),
            Ok(None) => {
                return Err(format!(
                    "spec_ref '{spec_ref}' does not resolve: '{path}' absent at SHA {sha}"
                ))
            }
            Err(e) => return Err(format!("spec_ref '{spec_ref}' does not resolve: {e:#}")),
        }
    } else {
        None
    };

    // MR description + acceptance criteria (§AgentReview Gate step 1): the
    // MR model has no body field; the task the MR implements (resolved
    // through the author agent) carries the description with acceptance
    // criteria. An MR with no author agent or unlinked task simply has no
    // extra criteria — the spec + persona remain the review basis.
    let task_description = if let Some(agent_id) = mr.author_agent_id.as_ref() {
        match state.agents.find_by_id(agent_id).await {
            Ok(Some(agent)) => match agent.current_task_id {
                Some(task_id) => match state.tasks.find_by_id(&task_id).await {
                    Ok(Some(task)) => task.description,
                    Ok(None) => {
                        warn!(
                            mr_id = %mr.id,
                            task_id = %task_id,
                            "agent_review gate: author agent's task not found; no acceptance criteria"
                        );
                        None
                    }
                    Err(e) => {
                        warn!(
                            mr_id = %mr.id,
                            task_id = %task_id,
                            error = %e,
                            "agent_review gate: failed to load author agent's task; no acceptance criteria"
                        );
                        None
                    }
                },
                None => None,
            },
            Ok(None) => None,
            Err(e) => {
                warn!(
                    mr_id = %mr.id,
                    agent_id = %agent_id,
                    error = %e,
                    "agent_review gate: failed to load author agent; no acceptance criteria"
                );
                None
            }
        }
    } else {
        None
    };

    // Scoped reviewer identity: `review:submit` only. `task_id` carries the
    // gate id so the token is traceable to the gate run that minted it.
    let gate_agent_id = format!("gate-review-{}", Uuid::new_v4());
    let token = state
        .agent_signing_key
        .mint_scoped(
            &gate_agent_id,
            gate.id.as_str(),
            "forge",
            &state.base_url,
            AGENT_GATE_TIMEOUT_SECS + 60,
            "review:submit",
        )
        .map_err(|e| format!("failed to mint scoped review token: {e}"))?;
    // Register in agent_tokens so the auth extractor resolves the JWT and
    // teardown (kv_remove) revokes it.
    state
        .kv_store
        .kv_set("agent_tokens", &gate_agent_id, token.clone())
        .await
        .map_err(|e| format!("failed to register gate agent token: {e:#}"))?;

    Ok(ReviewAgentContext {
        gate_agent_id,
        token,
        diff: diff_text,
        spec_content,
        spec_ref,
        mr_title: mr.title,
        task_description,
        persona_prompt: resolved.system_prompt,
        persona_slug: resolved.slug,
    })
}

/// Spawn a real review agent process and wait for it to submit its verdict.
async fn run_review_agent_process(
    state: &Arc<AppState>,
    gate: &gyre_domain::QualityGate,
    mr_id: &Id,
    cmd: &str,
    persona: &str,
) -> (GateStatus, String) {
    // Gather MR context and mint the scoped reviewer identity. A failure
    // here (missing MR/repo/persona, unresolvable spec_ref, diff failure)
    // fails the gate before spawning anything.
    let ctx = match build_review_agent_context(state, gate, mr_id, persona).await {
        Ok(ctx) => ctx,
        Err(e) => {
            warn!(gate_id = %gate.id, mr_id = %mr_id, error = %e, "agent_review gate: context gathering failed");
            return (GateStatus::Failed, format!("agent_review gate failed: {e}"));
        }
    };

    let gate_agent_id = ctx.gate_agent_id.clone();
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
        gate_agent_id = %gate_agent_id,
        "agent_review gate: spawning review agent"
    );

    // The full spec text and MR diff are handed to the agent via temp files
    // rather than env vars: env vars are size-limited and these are the
    // review's primary artifacts. The MR description (task description with
    // acceptance criteria) is small enough for an env var.
    let spec_file = ctx.spec_content.as_ref().map(|content| {
        let path = std::env::temp_dir().join(format!("gyre-gate-spec-{}.md", Uuid::new_v4()));
        std::fs::write(&path, content).ok().map(|_| path)
    });
    let diff_file = {
        let path = std::env::temp_dir().join(format!("gyre-gate-diff-{}.patch", Uuid::new_v4()));
        std::fs::write(&path, &ctx.diff).ok().map(|_| path)
    };

    let mut command = tokio::process::Command::new(parts[0]);
    command.args(&parts[1..]);
    command
        .env("GYRE_SERVER_URL", &state.base_url)
        .env("GYRE_REVIEW_TOKEN", &ctx.token)
        .env("GYRE_MR_ID", mr_id.as_str())
        .env("GYRE_MR_TITLE", &ctx.mr_title)
        .env("GYRE_GATE_ID", gate.id.as_str())
        .env("GYRE_GATE_AGENT_ID", &gate_agent_id)
        .env("GYRE_DIFF_URL", &diff_url)
        .env("GYRE_SPEC_REF", &ctx.spec_ref)
        .env("GYRE_PERSONA", persona)
        .env("GYRE_PERSONA_SLUG", &ctx.persona_slug)
        .env("GYRE_PERSONA_PROMPT", &ctx.persona_prompt)
        .env("GYRE_SPEC_CONTENT", ctx.spec_content.as_deref().unwrap_or(""))
        .env(
            "GYRE_TASK_DESCRIPTION",
            ctx.task_description.as_deref().unwrap_or(""),
        );
    if let Some(Some(path)) = &spec_file {
        command.env("GYRE_SPEC_FILE", path.display().to_string());
    }
    if let Some(path) = &diff_file {
        command.env("GYRE_DIFF_FILE", path.display().to_string());
    }

    let spawn_result = command.output();

    let timeout = Duration::from_secs(gate.timeout_secs.unwrap_or(AGENT_GATE_TIMEOUT_SECS));
    let result = tokio::time::timeout(timeout, spawn_result).await;

    // Teardown (§AgentReview Gate step 5): revoke the scoped token
    // regardless of outcome — single-minded agents get one verdict.
    revoke_gate_token(state, &gate_agent_id).await;
    if let Some(Some(path)) = &spec_file {
        let _ = std::fs::remove_file(path);
    }
    if let Some(path) = &diff_file {
        let _ = std::fs::remove_file(path);
    }

    match result {
        Err(_) => {
            warn!(gate_id = %gate.id, mr_id = %mr_id, "agent_review gate: process timed out");
            (
                GateStatus::Failed,
                format!(
                    "agent_review gate: review agent timed out after {}s",
                    gate.timeout_secs.unwrap_or(AGENT_GATE_TIMEOUT_SECS)
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
                format!("{}...(truncated)", truncate_bytes(&process_output, 4096))
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

/// Check whether the gate agent submitted a verdict for this MR and map it
/// to gate status (§AgentReview Gate steps 3-4): Approved → Passed,
/// ChangesRequested → Failed, no verdict → Failed (an agent that exited
/// without submitting did not complete its review).
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

/// Run an AgentValidation gate.
///
/// Spawns the validation agent configured via `gate.command` with the MR
/// context injected via environment variables; the agent reports pass/fail
/// through its exit code (§Gate Types: "Agent reports pass/fail").
///
/// A gate with no `command` configured cannot spawn a validator, so it
/// fails — same fail-closed semantics as AgentReview.
async fn run_agent_validation_gate(
    state: &Arc<AppState>,
    gate: &gyre_domain::QualityGate,
    mr_id: &Id,
) -> (GateStatus, String) {
    let persona = gate.persona.as_deref().unwrap_or("personas/validator.md");

    match &gate.command {
        Some(cmd) => run_validation_agent_process(state, gate, mr_id, cmd, persona).await,
        None => {
            warn!(
                gate_id = %gate.id,
                mr_id = %mr_id,
                "agent_validation gate: no agent command configured; cannot spawn a validator"
            );
            (
                GateStatus::Failed,
                format!(
                    "agent_validation gate failed: no agent command configured (persona={persona}); \
                     configure the gate's command to spawn a validation agent"
                ),
            )
        }
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
    // Scoped validator identity: `review:submit` lets the validator read MR
    // context and report its result, and nothing else.
    let gate_agent_id = format!("gate-validate-{}", Uuid::new_v4());
    let gate_token = state
        .agent_signing_key
        .mint_scoped(
            &gate_agent_id,
            gate.id.as_str(),
            "forge",
            &state.base_url,
            AGENT_GATE_TIMEOUT_SECS + 60,
            "review:submit",
        )
        .unwrap_or_else(|e| {
            tracing::error!("scoped token mint failed, falling back to UUID token: {e}");
            format!("gyre_gate_{}", Uuid::new_v4().simple())
        });

    let _ = state
        .kv_store
        .kv_set("agent_tokens", &gate_agent_id, gate_token.clone())
        .await;

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

    let timeout = Duration::from_secs(gate.timeout_secs.unwrap_or(AGENT_GATE_TIMEOUT_SECS));
    let result = tokio::time::timeout(timeout, spawn_result).await;

    revoke_gate_token(state, &gate_agent_id).await;
    match result {
        Err(_) => {
            warn!(gate_id = %gate.id, mr_id = %mr_id, "agent_validation gate: process timed out");
            (
                GateStatus::Failed,
                format!(
                    "agent_validation gate: validation agent timed out after {}s",
                    gate.timeout_secs.unwrap_or(AGENT_GATE_TIMEOUT_SECS)
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
                format!("{}...(truncated)", truncate_bytes(&process_output, 4096))
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

/// Run a TraceCapture gate (observational — always passes).
///
/// Lifecycle:
/// 1. Parse gate config (otlp_port, test_command, max_spans, capture_external).
/// 2. Start an OTLP HTTP receiver on otlp_port.
/// 3. Run test_command with OTel env vars injected.
/// 4. Stop the receiver; collect captured spans.
/// 5. Resolve span-to-graph-node linkage (heuristic).
/// 6. Store the GateTrace via TraceRepository::store.
/// 7. Return Passed (trace capture is observational, not a quality gate).
async fn run_trace_capture_gate(
    state: &Arc<AppState>,
    gate: &gyre_domain::QualityGate,
    mr_id: &Id,
    gate_run_id: &Id,
) -> (GateStatus, String) {
    // Parse config from gate.command field (JSON).
    // Server-level OTLP config (env vars) provides defaults and the max_spans ceiling.
    let mut config = gate
        .command
        .as_deref()
        .and_then(|c| serde_json::from_str::<TraceCaptureConfig>(c).ok())
        .unwrap_or_default();
    // Enforce the server-level max_spans cap so operators can bound memory usage.
    config.max_spans = config.max_spans.min(state.otlp_config.max_spans_per_trace);

    // Honor the server-level GYRE_OTLP_ENABLED switch (HSI §3a). When the OTLP
    // receiver is disabled, the gate still passes (observational) but captures nothing.
    if !state.otlp_config.enabled {
        info!(gate_id = %gate.id, mr_id = %mr_id, "trace_capture gate: OTLP receiver disabled (GYRE_OTLP_ENABLED=false), skipping capture");
        return (
            GateStatus::Passed,
            "trace_capture gate: OTLP receiver disabled — capture skipped".to_string(),
        );
    }

    // Resolve the receiver port: gate-level override, else server-level default.
    let port = config.otlp_port.unwrap_or(state.otlp_config.grpc_port);

    info!(
        gate_id = %gate.id,
        mr_id = %mr_id,
        otlp_port = port,
        test_command = %config.test_command,
        "trace_capture gate: starting OTLP gRPC receiver"
    );

    // Look up the MR's source branch head commit SHA (best effort).
    // Spec §3a: GateTrace is linked to MR and commit SHA — resolve the branch
    // head via git, falling back to "unknown" when the repo/branch is missing.
    let commit_sha = resolve_source_commit_sha(state, mr_id).await;

    // Template `{{repo_name}}` in the gate's env map (HSI §3a) — e.g.
    // `OTEL_SERVICE_NAME: "{{repo_name}}"` becomes the repository name.
    let repo_name = match state.merge_requests.find_by_id(mr_id).await {
        Ok(Some(mr)) => state
            .repos
            .find_by_id(&mr.repository_id)
            .await
            .ok()
            .flatten()
            .map(|repo| repo.name)
            .unwrap_or_default(),
        _ => String::new(),
    };
    for value in config.env.values_mut() {
        *value = value.replace("{{repo_name}}", &repo_name);
    }

    // Run the OTLP receiver + test command.
    let capture_result = crate::otlp_receiver::run_trace_capture(
        config,
        port,
        mr_id.clone(),
        gate_run_id.clone(),
        commit_sha,
    )
    .await;

    match capture_result {
        Ok(trace) => {
            let span_count = trace.spans.len();

            // Resolve span-to-graph-node linkage (heuristic).
            let trace = crate::otlp_receiver::resolve_graph_linkage(state, trace).await;

            // Store via TraceRepository.
            match state.traces.store(&trace).await {
                Ok(()) => {
                    info!(
                        gate_id = %gate.id,
                        mr_id = %mr_id,
                        span_count,
                        "trace_capture gate: stored {} spans",
                        span_count
                    );
                    (
                        GateStatus::Passed,
                        format!("trace_capture gate: captured {span_count} spans for MR {mr_id}"),
                    )
                }
                Err(e) => {
                    warn!(gate_id = %gate.id, mr_id = %mr_id, error = %e, "trace_capture gate: failed to store trace");
                    // Still pass — storage failure is not a quality gate failure.
                    (
                        GateStatus::Passed,
                        format!("trace_capture gate: {span_count} spans captured but storage failed: {e}"),
                    )
                }
            }
        }
        Err(e) => {
            warn!(gate_id = %gate.id, mr_id = %mr_id, error = %e, "trace_capture gate: capture failed");
            // TraceCapture always passes — capture failure is observational.
            (
                GateStatus::Passed,
                format!("trace_capture gate: capture failed (observational only): {e}"),
            )
        }
    }
}

/// Remove a gate agent's token from the auth store.
async fn revoke_gate_token(state: &Arc<AppState>, gate_agent_id: &str) {
    let _ = state
        .kv_store
        .kv_remove("agent_tokens", gate_agent_id)
        .await;
}

/// Resolve the MR's source-branch head commit SHA for GateTrace linkage
/// (HSI §3a). MR → repository → `git rev-parse refs/heads/<branch>`; falls
/// back to "unknown" when the MR, repo, or branch cannot be resolved.
async fn resolve_source_commit_sha(state: &Arc<AppState>, mr_id: &Id) -> String {
    let mr = state.merge_requests.find_by_id(mr_id).await.ok().flatten();
    let Some(mr) = mr else {
        return "unknown".to_string();
    };
    let repo = state
        .repos
        .find_by_id(&mr.repository_id)
        .await
        .ok()
        .flatten();
    let Some(repo) = repo else {
        return "unknown".to_string();
    };
    let refname = format!("refs/heads/{}", mr.source_branch);
    crate::git_refs::resolve_ref(&repo.path, &refname)
        .await
        .unwrap_or_else(|| "unknown".to_string())
}

/// Truncate a string to at most `limit` bytes on a UTF-8 char boundary.
/// External process output may be multibyte — a fixed byte index would
/// panic ("byte index N is not a char boundary").
fn truncate_bytes(s: &str, limit: usize) -> &str {
    if s.len() <= limit {
        return s;
    }
    let end = s
        .char_indices()
        .take_while(|(i, _)| *i <= limit)
        .map(|(i, _)| i)
        .last()
        .unwrap_or(0);
    &s[..end]
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
                format!("{}...(truncated)", truncate_bytes(&combined, 4096))
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

/// Run a command with an optional working directory and timeout.
/// Same semantics as `run_command`, plus:
/// - `cwd`: working directory for the child process (None = server cwd).
/// - `timeout_secs`: kill the process and fail the gate when exceeded.
async fn run_command_in_dir(
    cmd: &str,
    cwd: Option<&std::path::Path>,
    timeout_secs: u64,
) -> (GateStatus, String) {
    // Split command on whitespace to avoid shell injection via `sh -c`.
    let parts: Vec<&str> = cmd.split_whitespace().collect();
    if parts.is_empty() {
        return (GateStatus::Failed, "empty command".to_string());
    }
    let mut command = tokio::process::Command::new(parts[0]);
    command.args(&parts[1..]);
    if let Some(dir) = cwd {
        command.current_dir(dir);
    }

    let result = match tokio::time::timeout(
        Duration::from_secs(timeout_secs),
        command.output(),
    )
    .await
    {
        Ok(r) => r,
        Err(_) => {
            warn!(cmd = %cmd, timeout_secs, "gate command timed out");
            return (
                GateStatus::Failed,
                format!("timed out after {timeout_secs}s"),
            );
        }
    };

    match result {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            let combined = format!("{stdout}{stderr}");
            // Truncate to 4 KiB, on a UTF-8 char boundary (external process
            // output may be multibyte — a fixed byte index would panic).
            let truncated = if combined.len() > 4096 {
                format!("{}...(truncated)", truncate_bytes(&combined, 4096))
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

/// Run all post-merge-phase gates for a repository against `head_sha`
/// (the new HEAD of the default branch after a merge landed).
///
/// Returns `Ok(())` when all required post-merge gates pass (or none exist).
/// Returns `Err(reason)` naming the first required gate that failed — the
/// reason is used in the revert notification and remediation task
/// (platform-model.md §6). Non-required gates are advisory: failures are
/// logged but do not fail validation.
///
/// Gates run inside a detached git worktree checked out at `head_sha`, so
/// they validate the exact merged tree (platform-model.md §6, task-095
/// R2-1). If worktree preparation fails (e.g. mem mode without a repo on
/// disk), gates fall back to the server's working directory.
pub async fn run_post_merge_gates(
    state: &AppState,
    repo: &gyre_domain::Repository,
    head_sha: &str,
) -> Result<(), String> {
    let gates = state
        .quality_gates
        .list_by_repo_id_and_phase(repo.id.as_str(), gyre_domain::GatePhase::PostMerge)
        .await
        .map_err(|e| format!("failed to list post-merge gates: {e}"))?;

    if gates.is_empty() {
        return Ok(());
    }

    // Post-merge validation must run against the exact merged tree, not
    // whatever a long-lived agent worktree happens to have checked out
    // (platform-model.md §6, task-095 R2-1). Create a detached worktree at
    // the merge head SHA, run the gates there, and remove it afterwards.
    // Worktree prep is best-effort: without it (e.g. mem mode), gates fall
    // back to running in the server working directory.
    let gate_dir: Option<(std::path::PathBuf, tempfile::TempDir)> = {
        let tmp = tempfile::tempdir()
            .map_err(|e| format!("failed to create temp dir for post-merge gate worktree: {e}"))?;
        let worktree_path = tmp.path().join("gate");
        match state
            .git_ops
            .create_detached_worktree(
                repo.path.as_str(),
                worktree_path.to_str().unwrap_or_default(),
                head_sha,
            )
            .await
        {
            // The TempDir stays alive in `gate_dir` until the gates finish.
            Ok(()) => Some((worktree_path, tmp)),
            Err(e) => {
                warn!(
                    repo_id = %repo.id, head = %head_sha, error = %e,
                    "could not create detached worktree at merge head; post-merge gates fall back to server cwd"
                );
                None
            }
        }
    };
    let cwd: Option<std::path::PathBuf> = gate_dir.as_ref().map(|(p, _)| p.clone());
    let result = run_post_merge_gate_commands(&gates, cwd.as_deref()).await;

    // Remove the detached gate worktree (if created) after gates finish:
    // deregister it from the repo's worktree list; the tempdir is removed
    // when `gate_dir` drops at the end of this function. This runs on both
    // the pass and fail paths — a failed gate must not leak the worktree.
    if let Some((path, _tmp)) = &gate_dir {
        if let Err(e) = state
            .git_ops
            .remove_worktree(repo.path.as_str(), path.to_str().unwrap_or_default())
            .await
        {
            warn!(
                repo_id = %repo.id, worktree = %path.display(), error = %e,
                "failed to remove post-merge gate worktree"
            );
        }
    }

    result?;
    info!(repo_id = %repo.id, head = %head_sha, "post-merge gates passed");
    Ok(())
}

async fn run_post_merge_gate_commands(
    gates: &[gyre_domain::QualityGate],
    cwd: Option<&std::path::Path>,
) -> Result<(), String> {
    for gate in gates {
        match &gate.gate_type {
            GateType::TestCommand | GateType::LintCommand => {
                let timeout = gate.timeout_secs.unwrap_or(AGENT_GATE_TIMEOUT_SECS);
                let (status, output) = run_command_in_dir(
                    gate.command.as_deref().unwrap_or("true"),
                    cwd,
                    timeout,
                )
                .await;
                if status == GateStatus::Failed {
                    if gate.required {
                        return Err(format!(
                            "post-merge gate '{}' failed: {}",
                            gate.name, output
                        ));
                    }
                    warn!(gate = %gate.name, "advisory post-merge gate failed: {output}");
                }
            }
            other => {
                // Post-merge validation is command-based; other gate types
                // are pre-merge concepts and are skipped in this phase.
                info!(gate = %gate.name, gate_type = ?other, "skipping non-command post-merge gate");
            }
        }
    }
    Ok(())
}

/// Returns whether all required gate results for the given MR have passed.
/// Returns `Ok(true)` if no gates exist or all required gates passed.
/// Returns `Ok(false)` if any required gates are still pending/running.
/// Returns `Err(msg)` if any required gate has failed.
/// Non-required (advisory) gates that fail are recorded but do not block merging.
pub async fn check_gates_for_mr(state: &AppState, mr_id: &Id) -> Result<bool, String> {
    let results = state
        .gate_results
        .list_by_mr_id(mr_id.as_str())
        .await
        .unwrap_or_default();

    for r in &results {
        // Look up whether this gate is required (default: true for unknown gates).
        let is_required = state
            .quality_gates
            .find_by_id(r.gate_id.as_str())
            .await
            .ok()
            .flatten()
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
    use gyre_domain::{GateType, QualityGate, Review};
    use gyre_ports::GitOpsPort;

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
            gate_phase: Default::default(),
            timeout_secs: None,
            created_at: now_secs(),
        }
    }

    fn make_mr_id() -> Id {
        Id::new(Uuid::new_v4().to_string())
    }

    /// Seed a repository row (no persona). Returns (mr_id, repo_id).
    async fn seed_mr_with_repo(state: &Arc<AppState>) -> (Id, Id) {
        let repo_id = Id::new(Uuid::new_v4().to_string());
        let repo = gyre_domain::Repository::new(
            repo_id.clone(),
            Id::new("ws-test"),
            "review-repo",
            format!("/tmp/gyre-test-repo-{}", Uuid::new_v4()),
            now_secs(),
        );
        state.repos.create(&repo).await.unwrap();

        let mr_id = make_mr_id();
        let mr = gyre_domain::MergeRequest::new(
            mr_id.clone(),
            repo_id.clone(),
            "Add feature",
            "feature-branch",
            "main",
            now_secs(),
        );
        state.merge_requests.create(&mr).await.unwrap();
        (mr_id, repo_id)
    }

    /// Seed a repository row plus a persona resolvable in the repo's scope
    /// chain. Returns (mr_id, repo_id).
    async fn seed_mr_with_repo_and_persona(state: &Arc<AppState>, slug: &str) -> (Id, Id) {
        let (mr_id, repo_id) = seed_mr_with_repo(state).await;

        let persona = gyre_domain::Persona::new(
            Id::new(Uuid::new_v4().to_string()),
            format!("Test persona {slug}"),
            slug.to_string(),
            gyre_domain::PersonaScope::Workspace(Id::new("ws-test")),
            format!("You are the {slug} reviewer. Check the diff against the spec."),
            now_secs(),
        );
        state.personas.create(&persona).await.unwrap();
        (mr_id, repo_id)
    }

    /// Absolute path to the review-agent driver fixture (canonicalized at
    /// the call site — never a relative default resolved against cwd).
    fn review_driver_path() -> String {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/review_agent_driver.sh")
            .display()
            .to_string()
    }

    /// Seed the full §AgentReview Gate fixture into `state`: a real bare git
    /// repository on disk (spec at a pinned SHA, feature branch with a diff),
    /// repo + workspace-scoped persona rows, an MR pinned to the spec SHA
    /// with an author agent whose task carries the description/acceptance
    /// criteria. Returns (mr_id, repo_id, spec_sha).
    async fn seed_full_review_fixture(state: &Arc<AppState>) -> (Id, Id, String) {
        let dir = std::env::temp_dir().join(format!("gyre-gate-e2e-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let repo_path = dir.join("bare.git");
        let repo_path_str = repo_path.to_str().unwrap().to_string();

        let git_ops = gyre_adapters::Git2OpsAdapter::new();
        git_ops.init_bare(&repo_path_str).await.unwrap();
        git_ops
            .create_initial_commit(&repo_path_str, "main")
            .await
            .unwrap();
        // Pin: spec v1 lands on main, then the branch advances past it — the
        // reviewer must read the pinned v1, not the tip.
        let spec_sha = git_ops
            .write_file(
                &repo_path_str,
                "main",
                "specs/system/widget.md",
                b"# Spec v1",
                "Add widget spec v1",
            )
            .await
            .unwrap();
        git_ops
            .write_file(
                &repo_path_str,
                "main",
                "specs/system/widget.md",
                b"# Spec v2",
                "Advance widget spec to v2",
            )
            .await
            .unwrap();

        let repo_id = Id::new(Uuid::new_v4().to_string());
        let repo = gyre_domain::Repository::new(
            repo_id.clone(),
            Id::new("ws-test"),
            "review-repo",
            &repo_path_str,
            now_secs(),
        );
        state.repos.create(&repo).await.unwrap();

        let persona = gyre_domain::Persona::new(
            Id::new(Uuid::new_v4().to_string()),
            "Test persona test-reviewer".to_string(),
            "test-reviewer".to_string(),
            gyre_domain::PersonaScope::Workspace(Id::new("ws-test")),
            "You are the test-reviewer reviewer. Check the diff against the spec.".to_string(),
            now_secs(),
        );
        state.personas.create(&persona).await.unwrap();

        // Author agent + task with description (the acceptance-criteria
        // carrier for the MR).
        let task_id = Id::new(Uuid::new_v4().to_string());
        let mut task = gyre_domain::Task::new(task_id.clone(), "Implement the widget", now_secs());
        task.description = Some("Implement the widget per specs/system/widget.md".to_string());
        task.workspace_id = Id::new("ws-test");
        task.repo_id = repo_id.clone();
        state.tasks.create(&task).await.unwrap();

        let agent_id = Id::new(Uuid::new_v4().to_string());
        let mut agent = gyre_domain::Agent::new(agent_id.clone(), "author-agent", now_secs());
        agent.current_task_id = Some(task_id);
        agent.workspace_id = Id::new("ws-test");
        state.agents.create(&agent).await.unwrap();

        // MR: feature branch diverges from main so the diff is non-empty.
        git_ops
            .create_branch(&repo_path_str, "feature-branch", "refs/heads/main")
            .await
            .unwrap();
        git_ops
            .write_file(
                &repo_path_str,
                "feature-branch",
                "src/widget.rs",
                b"pub fn widget() {}",
                "Add widget",
            )
            .await
            .unwrap();

        let mr_id = make_mr_id();
        let mut mr = gyre_domain::MergeRequest::new(
            mr_id.clone(),
            repo_id.clone(),
            "Add feature",
            "feature-branch",
            "main",
            now_secs(),
        );
        mr.spec_ref = Some(format!("specs/system/widget.md@{spec_sha}"));
        mr.author_agent_id = Some(agent_id);
        mr.workspace_id = Id::new("ws-test");
        state.merge_requests.create(&mr).await.unwrap();

        (mr_id, repo_id, spec_sha)
    }

    // ── truncate_bytes: UTF-8 char-boundary truncation (task-095 F4) ────────

    #[test]
    fn truncate_bytes_ascii_under_limit_untouched() {
        assert_eq!(truncate_bytes("hello", 4096), "hello");
        assert_eq!(truncate_bytes("", 4096), "");
    }

    #[test]
    fn truncate_bytes_multibyte_boundary_does_not_panic() {
        // Each 'é' is 2 bytes; 2048 of them = 4096 bytes exactly.
        let s: String = "é".repeat(2048);
        assert_eq!(truncate_bytes(&s, 4096).len(), 4096);
        // 2049 chars = 4098 bytes: byte 4096 falls mid-character, so the
        // cut backs off to the previous boundary (4096) — no panic.
        let s: String = "é".repeat(2049);
        assert_eq!(truncate_bytes(&s, 4096).len(), 4096);
        // 3-byte chars: 1366 × 3 = 4098 bytes; cut backs to 4095.
        let s: String = "字".repeat(1366);
        let t = truncate_bytes(&s, 4096);
        assert!(t.len() <= 4096 && (4096 - t.len()) < 3);
        assert!(t.chars().all(|c| c == '字'));
    }

    #[test]
    fn truncate_bytes_leading_multibyte_backs_to_zero() {
        // A 4-byte char at offset 0 with limit 2: no boundary fits, cut = "".
        let s = "\u{1F600}".repeat(3);
        let t = truncate_bytes(&s, 2);
        assert_eq!(t, "");
    }

    // ── AgentReview / AgentValidation no-command path (fail-closed) ────────

    #[tokio::test]
    async fn agent_review_no_command_fails_closed() {
        // A gate with no agent command cannot spawn a reviewer. Fabricating
        // an approval (the old stub) is prohibited: "cannot determine state"
        // is not "state is fine". The gate must fail.
        let state = test_state();
        let gate = make_gate(GateType::AgentReview, None);
        let mr_id = make_mr_id();

        let (status, output) = run_agent_review_gate(&state, &gate, &mr_id).await;

        assert_eq!(status, GateStatus::Failed, "output: {output}");
        assert!(
            output.contains("no agent command configured"),
            "output: {output}"
        );
    }

    #[tokio::test]
    async fn agent_validation_no_command_fails_closed() {
        let state = test_state();
        let gate = make_gate(GateType::AgentValidation, None);
        let mr_id = make_mr_id();

        let (status, output) = run_agent_validation_gate(&state, &gate, &mr_id).await;

        assert_eq!(status, GateStatus::Failed, "output: {output}");
        assert!(
            output.contains("no agent command configured"),
            "output: {output}"
        );
    }

    // ── AgentReview context gathering (fail-closed on missing fixtures) ────

    #[tokio::test]
    async fn agent_review_missing_mr_fails_at_context_gathering() {
        // A command is configured, but the MR does not exist: the gate fails
        // before spawning any process (no scoped token leaks).
        let state = test_state();
        let gate = make_gate(GateType::AgentReview, Some("true".to_string()));
        let mr_id = make_mr_id();

        let (status, output) = run_agent_review_gate(&state, &gate, &mr_id).await;

        assert_eq!(status, GateStatus::Failed, "output: {output}");
        assert!(
            output.contains("context gathering failed"),
            "output: {output}"
        );
        // No gate token was minted (teardown invariant: fail before mint).
        let gate_tokens: Vec<_> = state
            .kv_store
            .kv_list("agent_tokens")
            .await
            .unwrap_or_default()
            .into_iter()
            .filter(|(k, _)| k.starts_with("gate-"))
            .collect();
        assert!(gate_tokens.is_empty(), "tokens: {gate_tokens:?}");
    }

    #[tokio::test]
    async fn agent_review_unresolvable_persona_fails_at_context_gathering() {
        // MR + repo exist, but the persona slug resolves nowhere in the
        // scope chain: a review against undefined criteria is not a review.
        let state = test_state();
        let (mr_id, _repo_id) = seed_mr_with_repo(&state).await;
        let gate = make_gate(GateType::AgentReview, Some("true".to_string()));

        let (status, output) = run_agent_review_gate(&state, &gate, &mr_id).await;

        assert_eq!(status, GateStatus::Failed, "output: {output}");
        assert!(
            output.contains("not found in scope chain"),
            "output: {output}"
        );
    }

    #[tokio::test]
    async fn agent_review_exit_nonzero_fails() {
        // Full fixture: MR, repo, persona. The agent process itself fails
        // (non-zero exit) — the gate fails with the agent's output.
        let state = test_state();
        let (mr_id, _repo_id) = seed_mr_with_repo_and_persona(&state, "test-reviewer").await;
        let gate = make_gate(GateType::AgentReview, Some("false".to_string()));
        // The gate's persona must match the seeded slug.
        let mut gate = gate;
        gate.persona = Some("personas/test-reviewer.md".to_string());

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
        let (mr_id, _repo_id) = seed_mr_with_repo_and_persona(&state, "test-reviewer").await;
        let mut gate = make_gate(
            GateType::AgentReview,
            Some("/nonexistent/review-agent".to_string()),
        );
        gate.persona = Some("personas/test-reviewer.md".to_string());

        let (status, output) = run_agent_review_gate(&state, &gate, &mr_id).await;

        assert_eq!(status, GateStatus::Failed, "output: {output}");
        assert!(
            output.contains("failed to spawn") || output.contains("No such file"),
            "output: {output}"
        );
    }

    #[tokio::test]
    async fn agent_review_exit_zero_no_review_submitted_fails() {
        // Agent exits 0 but never submits a review — no verdict means the
        // review did not happen; the gate fails.
        let state = test_state();
        let (mr_id, _repo_id) = seed_mr_with_repo_and_persona(&state, "test-reviewer").await;
        let mut gate = make_gate(GateType::AgentReview, Some("true".to_string()));
        gate.persona = Some("personas/test-reviewer.md".to_string());

        let (status, output) = run_agent_review_gate(&state, &gate, &mr_id).await;

        assert_eq!(status, GateStatus::Failed, "output: {output}");
        assert!(
            output.contains("without submitting a review"),
            "output: {output}"
        );
    }

    // ── AgentReview verdict mapping (check_review_verdict) ──────────────────

    #[tokio::test]
    async fn review_verdict_approved_maps_to_passed() {
        let state = test_state();
        let gate = make_gate(GateType::AgentReview, None);
        let mr_id = make_mr_id();
        let gate_agent_id = "gate-review-verdict-1".to_string();

        let review = Review::new(
            Id::new(Uuid::new_v4().to_string()),
            mr_id.clone(),
            gate_agent_id.clone(),
            ReviewDecision::Approved,
            now_secs(),
        );
        state.reviews.submit_review(&review).await.unwrap();

        let (status, output) =
            check_review_verdict(&state, &gate, &mr_id, &gate_agent_id, "p", "").await;

        assert_eq!(status, GateStatus::Passed, "output: {output}");
        assert!(output.contains("approved"), "output: {output}");
    }

    #[tokio::test]
    async fn review_verdict_changes_requested_maps_to_failed_with_body() {
        let state = test_state();
        let gate = make_gate(GateType::AgentReview, None);
        let mr_id = make_mr_id();
        let gate_agent_id = "gate-review-verdict-2".to_string();

        let mut review = Review::new(
            Id::new(Uuid::new_v4().to_string()),
            mr_id.clone(),
            gate_agent_id.clone(),
            ReviewDecision::ChangesRequested,
            now_secs(),
        );
        review.body = Some("criteria X unmet".to_string());
        state.reviews.submit_review(&review).await.unwrap();

        let (status, output) =
            check_review_verdict(&state, &gate, &mr_id, &gate_agent_id, "p", "").await;

        assert_eq!(status, GateStatus::Failed, "output: {output}");
        assert!(
            output.contains("requested changes"),
            "output: {output}"
        );
        assert!(output.contains("criteria X unmet"), "output: {output}");
    }

    #[tokio::test]
    async fn review_verdict_ignores_reviews_from_other_agents() {
        // Only the gate's own agent's verdict counts — another agent's
        // approval must not satisfy this gate's review requirement.
        let state = test_state();
        let gate = make_gate(GateType::AgentReview, None);
        let mr_id = make_mr_id();

        let other = Review::new(
            Id::new(Uuid::new_v4().to_string()),
            mr_id.clone(),
            "some-other-agent",
            ReviewDecision::Approved,
            now_secs(),
        );
        state.reviews.submit_review(&other).await.unwrap();

        let (status, output) = check_review_verdict(
            &state,
            &gate,
            &mr_id,
            "gate-review-verdict-3",
            "p",
            "",
        )
        .await;

        assert_eq!(status, GateStatus::Failed, "output: {output}");
        assert!(
            output.contains("without submitting a review"),
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

        let gate_tokens: Vec<_> = state
            .kv_store
            .kv_list("agent_tokens")
            .await
            .unwrap_or_default()
            .into_iter()
            .filter(|(k, _)| k.starts_with("gate-"))
            .collect();
        assert!(
            gate_tokens.is_empty(),
            "gate tokens should be revoked: {gate_tokens:?}"
        );
    }

    #[tokio::test]
    async fn gate_token_revoked_after_review_completes() {
        let state = test_state();
        let (mr_id, _repo_id) = seed_mr_with_repo_and_persona(&state, "test-reviewer").await;
        let mut gate = make_gate(GateType::AgentReview, Some("true".to_string()));
        gate.persona = Some("personas/test-reviewer.md".to_string());

        run_agent_review_gate(&state, &gate, &mr_id).await;

        let gate_tokens: Vec<_> = state
            .kv_store
            .kv_list("agent_tokens")
            .await
            .unwrap_or_default()
            .into_iter()
            .filter(|(k, _)| k.starts_with("gate-"))
            .collect();
        assert!(
            gate_tokens.is_empty(),
            "gate tokens should be revoked: {gate_tokens:?}"
        );
    }

    // ── AgentReview end-to-end protocol (real server, real HTTP, real JWT) ──
    //
    // Mirrors git_http tests: bind a real axum server with the full
    // require_auth + ABAC middleware stack, back it with a real bare git
    // repository, and drive a real review-agent process (shell script) that
    // authenticates with the scoped JWT and submits its verdict through the
    // Review API. Proves the whole §AgentReview Gate chain:
    //   persona resolution → MR context (diff/spec/description) → scoped
    //   token → ABAC allow-list → reviewer identity binding → verdict →
    //   gate status mapping → token teardown.

    #[tokio::test(flavor = "multi_thread")]
    async fn agent_review_end_to_end_approved_verdict_passes_gate() {
        // Bind first so the state's base_url (JWT issuer + agent's server
        // URL) points at the live server from the start.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let base_url = format!("http://127.0.0.1:{port}");

        let state = crate::build_state("gyre-test-token", &base_url, None);
        let (mr_id, _repo_id, spec_sha) = seed_full_review_fixture(&state).await;

        let app = crate::build_router(state.clone());
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        let driver = review_driver_path();
        let mut gate = make_gate(
            GateType::AgentReview,
            Some(format!("{driver} approved")),
        );
        gate.persona = Some("personas/test-reviewer.md".to_string());

        let (status, output) = run_agent_review_gate(&state, &gate, &mr_id).await;

        assert_eq!(status, GateStatus::Passed, "output: {output}");
        assert!(output.contains("approved"), "output: {output}");
        // Context delivery assertions (spec step 1).
        assert!(
            output.contains("persona_prompt=You are the test-reviewer reviewer"),
            "persona prompt not delivered: {output}"
        );
        assert!(
            output.contains("mr_title=Add feature"),
            "MR title not delivered: {output}"
        );
        assert!(
            output.contains("task_description=Implement the widget"),
            "task description (acceptance criteria) not delivered: {output}"
        );
        assert!(
            output.contains("spec_content=# Spec v1"),
            "spec content not delivered: {output}"
        );
        assert!(
            output.contains(&format!("spec_ref=specs/system/widget.md@{spec_sha}")),
            "spec_ref not delivered: {output}"
        );
        assert!(
            output.contains("diff_first_line=diff --git"),
            "full diff not delivered via GYRE_DIFF_FILE: {output}"
        );
        assert!(
            output.contains("http_code=201"),
            "review submission through the live API failed: {output}"
        );

        // The submitted review is bound to the gate agent's token subject,
        // not the forged reviewer id in the request body.
        let reviews = state.reviews.list_reviews(&mr_id).await.unwrap();
        assert_eq!(reviews.len(), 1, "reviews: {reviews:?}");
        assert!(
            reviews[0].reviewer_agent_id.starts_with("gate-review-"),
            "reviewer identity not bound to token subject: {:?}",
            reviews[0].reviewer_agent_id
        );
        assert_eq!(reviews[0].decision, ReviewDecision::Approved);

        // Teardown: the scoped token is revoked after the verdict.
        let gate_tokens: Vec<_> = state
            .kv_store
            .kv_list("agent_tokens")
            .await
            .unwrap_or_default()
            .into_iter()
            .filter(|(k, _)| k.starts_with("gate-"))
            .collect();
        assert!(gate_tokens.is_empty(), "tokens: {gate_tokens:?}");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn agent_review_end_to_end_changes_requested_fails_gate() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let base_url = format!("http://127.0.0.1:{port}");

        let state = crate::build_state("gyre-test-token", &base_url, None);
        let (mr_id, _repo_id, _spec_sha) = seed_full_review_fixture(&state).await;

        let app = crate::build_router(state.clone());
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        let driver = review_driver_path();
        let mut gate = make_gate(
            GateType::AgentReview,
            Some(format!("{driver} changes_requested")),
        );
        gate.persona = Some("personas/test-reviewer.md".to_string());

        let (status, output) = run_agent_review_gate(&state, &gate, &mr_id).await;

        assert_eq!(status, GateStatus::Failed, "output: {output}");
        assert!(
            output.contains("requested changes"),
            "ChangesRequested not mapped to Failed: {output}"
        );
        assert!(
            output.contains("The diff violates the persona criteria"),
            "review body not surfaced in gate output: {output}"
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

        state
            .gate_results
            .save(&GateResult {
                id: result_id.clone(),
                gate_id,
                mr_id: mr_id.clone(),
                status: GateStatus::Pending,
                output: None,
                started_at: None,
                finished_at: None,
            })
            .await
            .unwrap();

        let result = check_gates_for_mr(&state, &mr_id).await;
        assert_eq!(result, Ok(false));
    }

    #[tokio::test]
    async fn check_gates_failed_returns_err() {
        let state = test_state();
        let mr_id = make_mr_id();
        let gate_id = Id::new(Uuid::new_v4().to_string());
        let result_id = Id::new(Uuid::new_v4().to_string());

        state
            .gate_results
            .save(&GateResult {
                id: result_id.clone(),
                gate_id: gate_id.clone(),
                mr_id: mr_id.clone(),
                status: GateStatus::Failed,
                output: Some("test failure".to_string()),
                started_at: None,
                finished_at: None,
            })
            .await
            .unwrap();

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
        state
            .quality_gates
            .save(&gyre_domain::QualityGate {
                id: gate_id.clone(),
                repo_id: Id::new(Uuid::new_v4().to_string()),
                name: "advisory-lint".to_string(),
                gate_type: GateType::LintCommand,
                command: Some("false".to_string()),
                required_approvals: None,
                persona: None,
                required: false,
                gate_phase: Default::default(),
                timeout_secs: None,
                created_at: now_secs(),
            })
            .await
            .unwrap();

        // Record a Failed result for this advisory gate.
        state
            .gate_results
            .save(&GateResult {
                id: result_id.clone(),
                gate_id: gate_id.clone(),
                mr_id: mr_id.clone(),
                status: GateStatus::Failed,
                output: Some("lint warnings".to_string()),
                started_at: None,
                finished_at: None,
            })
            .await
            .unwrap();

        // Non-required gate failure should return Ok(true) — MR can proceed.
        let result = check_gates_for_mr(&state, &mr_id).await;
        assert_eq!(
            result,
            Ok(true),
            "advisory gate failure should not block MR"
        );
    }

    // ── TraceCapture gate ───────────────────────────────────────────────────

    #[tokio::test]
    async fn trace_capture_gate_always_passes_even_on_bad_command() {
        // TraceCapture is observational — it always passes, even when the test
        // command fails or the OTLP receiver can't start.
        let state = test_state();
        let gate = make_gate(
            GateType::TraceCapture,
            // Invalid JSON config means defaults apply; "false" as test_command exits non-zero.
            Some(r#"{"test_command": "false"}"#.to_string()),
        );
        let mr_id = make_mr_id();
        let result_id = Id::new(Uuid::new_v4().to_string());

        let (status, output) = run_trace_capture_gate(&state, &gate, &mr_id, &result_id).await;

        assert_eq!(
            status,
            GateStatus::Passed,
            "TraceCapture must always pass (observational): {output}"
        );
    }

    #[tokio::test]
    async fn trace_capture_gate_always_passes_with_default_config() {
        // With no config (empty/invalid JSON), defaults apply.
        // The test command "cargo test --features integration" likely won't succeed
        // in CI, but the gate should still pass.
        let state = test_state();
        let gate = make_gate(GateType::TraceCapture, None);
        let mr_id = make_mr_id();
        let result_id = Id::new(Uuid::new_v4().to_string());

        let (status, output) = run_trace_capture_gate(&state, &gate, &mr_id, &result_id).await;

        assert_eq!(
            status,
            GateStatus::Passed,
            "TraceCapture must always pass (observational): {output}"
        );
    }

    #[tokio::test]
    async fn trace_capture_gate_successful_run_stores_trace() {
        // When the test command succeeds, spans should be stored.
        let state = test_state();
        // Use "true" as a test command that exits successfully (no spans emitted).
        let gate = make_gate(
            GateType::TraceCapture,
            Some(r#"{"test_command": "true", "otlp_port": 0}"#.to_string()),
        );
        let mr_id = make_mr_id();
        let result_id = Id::new(Uuid::new_v4().to_string());

        let (status, output) = run_trace_capture_gate(&state, &gate, &mr_id, &result_id).await;

        assert_eq!(status, GateStatus::Passed, "output: {output}");
        assert!(
            output.contains("captured") || output.contains("trace_capture gate"),
            "output should mention trace capture: {output}"
        );

        // Verify a GateTrace was stored (even if 0 spans, the trace header is stored).
        let stored = state.traces.get_by_mr(&mr_id).await.unwrap();
        assert!(
            stored.is_some(),
            "trace should be stored for the MR after successful capture"
        );
        let trace = stored.unwrap();
        assert_eq!(trace.mr_id, mr_id);
    }

    #[tokio::test]
    async fn trace_capture_gate_skips_when_otlp_disabled() {
        // mem::test_state constructs otlp_config with enabled=false. The gate
        // must still pass (observational) but skip capture and store nothing.
        let state = crate::mem::test_state();
        assert!(
            !state.otlp_config.enabled,
            "precondition: OTLP receiver disabled in mem::test_state"
        );
        let gate = make_gate(
            GateType::TraceCapture,
            Some(r#"{"test_command": "true", "otlp_port": 0}"#.to_string()),
        );
        let mr_id = make_mr_id();
        let result_id = Id::new(Uuid::new_v4().to_string());

        let (status, output) = run_trace_capture_gate(&state, &gate, &mr_id, &result_id).await;

        assert_eq!(
            status,
            GateStatus::Passed,
            "disabled path still passes (observational): {output}"
        );
        assert!(
            output.contains("disabled"),
            "output should indicate capture was skipped: {output}"
        );
        // No trace is stored when the receiver is disabled.
        let stored = state.traces.get_by_mr(&mr_id).await.unwrap();
        assert!(
            stored.is_none(),
            "no trace should be stored when OTLP is disabled"
        );
    }

    /// HSI §3a: the stored GateTrace must be linked to the MR's source-branch
    /// head commit SHA. Real temp git repo; branch head resolved via
    /// `git rev-parse refs/heads/<source_branch>`.
    #[tokio::test]
    async fn trace_capture_gate_stores_source_branch_commit_sha() {
        let state = test_state();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().to_str().unwrap().to_string();

        for args in [
            vec!["init", &path],
            vec!["-C", &path, "config", "user.email", "t@t.com"],
            vec!["-C", &path, "config", "user.name", "T"],
        ] {
            std::process::Command::new("git")
                .args(&args)
                .output()
                .unwrap();
        }
        std::fs::write(dir.path().join("f.txt"), "x").unwrap();
        for args in [
            vec!["-C", &path, "add", "."],
            vec!["-C", &path, "commit", "-m", "c1"],
        ] {
            std::process::Command::new("git")
                .args(&args)
                .output()
                .unwrap();
        }
        // Create the source branch and advance it beyond main.
        std::process::Command::new("git")
            .args(["-C", &path, "checkout", "-b", "feat/x"])
            .output()
            .unwrap();
        std::fs::write(dir.path().join("g.txt"), "y").unwrap();
        for args in [
            vec!["-C", &path, "add", "."],
            vec!["-C", &path, "commit", "-m", "c2"],
        ] {
            std::process::Command::new("git")
                .args(&args)
                .output()
                .unwrap();
        }
        let expected_sha = String::from_utf8_lossy(
            &std::process::Command::new("git")
                .args(["-C", &path, "rev-parse", "HEAD"])
                .output()
                .unwrap()
                .stdout,
        )
        .trim()
        .to_string();

        let repo = gyre_domain::Repository::new(
            Id::new("repo-sha-test"),
            Id::new("default"),
            "sha-test-repo",
            path,
            now_secs(),
        );
        state.repos.create(&repo).await.unwrap();
        let mr = gyre_domain::MergeRequest::new(
            Id::new("mr-sha-test"),
            repo.id,
            "feat",
            "feat/x",
            "main",
            now_secs(),
        );
        state.merge_requests.create(&mr).await.unwrap();

        let gate = make_gate(
            GateType::TraceCapture,
            Some(r#"{"test_command": "true", "otlp_port": 0}"#.to_string()),
        );
        let mr_id = Id::new("mr-sha-test");
        let result_id = Id::new(Uuid::new_v4().to_string());
        let (status, output) = run_trace_capture_gate(&state, &gate, &mr_id, &result_id).await;
        assert_eq!(status, GateStatus::Passed, "output: {output}");

        let trace = state
            .traces
            .get_by_mr(&mr_id)
            .await
            .unwrap()
            .expect("trace stored");
        assert_eq!(
            trace.commit_sha, expected_sha,
            "trace must link the source-branch head SHA, not 'unknown'"
        );
        assert_eq!(trace.commit_sha.len(), 40, "40-char hex SHA");
    }

    /// Without a repo/branch to resolve, commit_sha falls back to "unknown"
    /// (observational gate still passes).
    #[tokio::test]
    async fn trace_capture_gate_commit_sha_falls_back_to_unknown() {
        let state = test_state();
        let gate = make_gate(
            GateType::TraceCapture,
            Some(r#"{"test_command": "true", "otlp_port": 0}"#.to_string()),
        );
        let mr_id = make_mr_id(); // no MR stored
        let result_id = Id::new(Uuid::new_v4().to_string());

        let (status, output) = run_trace_capture_gate(&state, &gate, &mr_id, &result_id).await;
        assert_eq!(status, GateStatus::Passed, "output: {output}");

        let trace = state
            .traces
            .get_by_mr(&mr_id)
            .await
            .unwrap()
            .expect("trace stored");
        assert_eq!(trace.commit_sha, "unknown");
    }

    /// HSI §3a: `{{repo_name}}` in the gate config's env values is templated
    /// to the MR's repository name before the test command runs. Observable
    /// via a script file that dumps the env value.
    #[tokio::test]
    async fn trace_capture_gate_templates_repo_name_in_env() {
        let state = test_state();
        let dir = tempfile::tempdir().unwrap();
        let out_path = dir.path().join("svc.txt");
        let script_path = dir.path().join("dump.sh");
        std::fs::write(
            &script_path,
            format!(
                "#!/bin/sh\nprintf '%s' \"$OTEL_SERVICE_NAME\" > {}\n",
                out_path.display()
            ),
        )
        .unwrap();

        let repo = gyre_domain::Repository::new(
            Id::new("repo-tpl-test"),
            Id::new("default"),
            "payments-api",
            dir.path().join("repo.git").display().to_string(),
            now_secs(),
        );
        state.repos.create(&repo).await.unwrap();
        let mr = gyre_domain::MergeRequest::new(
            Id::new("mr-tpl-test"),
            repo.id,
            "feat",
            "feat/y",
            "main",
            now_secs(),
        );
        state.merge_requests.create(&mr).await.unwrap();

        let gate = make_gate(
            GateType::TraceCapture,
            Some(format!(
                r#"{{"test_command": "sh {}", "otlp_port": 0, "env": {{"OTEL_SERVICE_NAME": "{{{{repo_name}}}}"}}}}"#,
                script_path.display()
            )),
        );
        let mr_id = Id::new("mr-tpl-test");
        let result_id = Id::new(Uuid::new_v4().to_string());

        let (status, output) = run_trace_capture_gate(&state, &gate, &mr_id, &result_id).await;
        assert_eq!(status, GateStatus::Passed, "output: {output}");

        let observed = std::fs::read_to_string(&out_path)
            .expect("test command should have written the templated env value");
        assert_eq!(
            observed, "payments-api",
            "{{repo_name}} must be templated to the repository name"
        );
    }
}
