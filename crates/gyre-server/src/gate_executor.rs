//! Gate execution engine: runs quality gates for an MR in the background.

use std::sync::Arc;
use std::time::Duration;
use tracing::{info, warn};
use uuid::Uuid;

use gyre_common::attestation::GateAttestation;
use gyre_common::Id;
use gyre_domain::{AgentStatus, GateResult, GateStatus, GateType, Review, ReviewDecision};

use crate::otlp_receiver::TraceCaptureConfig;
use crate::AppState;

/// Default timeout for agent-based gates (5 minutes).
const AGENT_GATE_TIMEOUT_SECS: u64 = 300;

/// How often a persona-constrained manifest gate re-checks for a qualifying
/// review while waiting for its timeout window to elapse.
const PERSONA_GATE_POLL_INTERVAL: Duration = Duration::from_secs(2);

/// TTL for a gate agent's scoped JWT. The token must outlive the agent
/// process's whole run window — the agent submits its verdict at the END of
/// the window, and the auth extractor rejects expired JWTs, so a token
/// shorter than the configured `gate.timeout_secs` would discard a verdict
/// submitted within the allowed window and falsely fail the gate. The +60s
/// grace keeps the token valid for the submission round-trip after a
/// last-moment verdict.
fn gate_token_ttl_secs(gate: &gyre_domain::QualityGate) -> u64 {
    gate.timeout_secs.unwrap_or(AGENT_GATE_TIMEOUT_SECS) + 60
}

/// Maximum output length to include in gate attestation hash.
const GATE_ATTESTATION_OUTPUT_LIMIT: usize = 4096;

/// A per-spec gate declared by the spec manifest (spec-registry.md §16,
/// Agent Approver / Gate Schema).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ManifestGateSpec {
    /// Persona name, e.g. "security".
    pub persona: String,
    /// Minimum attestation level for the reviewing agent (default 1).
    pub min_attestation_level: u32,
    /// When set, the reviewing agent's stack fingerprint must match exactly.
    pub stack_hash: Option<String>,
    /// Manifest path of the spec that declared this gate (e.g.
    /// "system/identity-security.md").
    pub spec_path: String,
}

/// Attestation constraints carried by a manifest-declared gate into
/// execution (spec-registry.md §16 Agent Approver / Gate Schema).
#[derive(Debug, Clone)]
struct GateConstraints {
    min_attestation_level: u32,
    stack_hash: Option<String>,
    /// The forge-spawned gate agent running this gate (spec-registry.md §16:
    /// "Forge spawns security gate agent and accountability gate agent for
    /// this MR"). `None` for repo-level gates and direct unit-test calls;
    /// torn down when the gate resolves (agent-gates.md §Gate Agent
    /// Lifecycle).
    gate_agent_id: Option<Id>,
}

/// Split a `spec_ref` of the form `"path@sha"` into `(path, sha)`.
/// Returns `None` when the `@` separator or either side is missing.
fn split_spec_ref(spec_ref: &str) -> Option<(&str, &str)> {
    let (path, sha) = spec_ref.rsplit_once('@')?;
    if path.is_empty() || sha.is_empty() {
        return None;
    }
    Some((path, sha))
}

/// Normalize a spec path for manifest matching. Manifest entry paths are
/// relative to `specs/` (e.g. "system/identity-security.md"); `spec_ref`
/// paths may carry a leading `specs/` prefix (docs/agent-protocol.md uses
/// `"specs/system/agent-gates.md@<sha>"`, task `spec_path`s do not).
/// Strips any leading `./`, `/`, and `specs/` prefix.
fn normalize_spec_path(path: &str) -> &str {
    let p = path.trim_start_matches("./").trim_start_matches('/');
    p.strip_prefix("specs/").unwrap_or(p)
}

/// Extract the persona name from a persona path like `"personas/security.md"`
/// (manifest personas are bare names; `QualityGate.persona` is a file path).
fn persona_name_from_path(path: &str) -> &str {
    let base = path.rsplit('/').next().unwrap_or(path);
    base.strip_suffix(".md").unwrap_or(base)
}

/// Resolve the manifest-declared gates that apply to an MR from its
/// `spec_ref` (spec-registry.md §16 Agent Gates Integration): the manifest
/// entry whose path matches the `spec_ref` path contributes its `gates:`
/// entries as gate agents for the MR.
///
/// Pure selection over `(manifest, spec_ref)` — empty result when the
/// manifest is absent, the MR has no `spec_ref`, no entry matches the path,
/// or the entry declares no gates. Gates repeating a persona already seen in
/// the same entry are deduplicated (first declaration wins).
pub(crate) fn manifest_gates_for_mr(
    manifest: Option<&crate::spec_registry::SpecManifest>,
    spec_ref: Option<&str>,
) -> Vec<ManifestGateSpec> {
    let Some(manifest) = manifest else {
        return Vec::new();
    };
    let Some(spec_ref) = spec_ref else {
        return Vec::new();
    };
    // The path portion of the spec_ref: before the last '@' when a SHA is
    // pinned, otherwise the whole string (spawn.rs records bare paths for
    // specs not yet in the ledger — those still name a spec).
    let path = match split_spec_ref(spec_ref) {
        Some((path, _sha)) => path,
        None => spec_ref,
    };
    let normalized = normalize_spec_path(path);
    let Some(entry) = manifest
        .specs
        .iter()
        .find(|e| normalize_spec_path(&e.path) == normalized)
    else {
        return Vec::new();
    };

    let mut out = Vec::new();
    let mut seen_personas = std::collections::HashSet::new();
    for gate in &entry.gates {
        if !seen_personas.insert(gate.persona.clone()) {
            continue; // same persona declared twice — first declaration wins
        }
        out.push(ManifestGateSpec {
            persona: gate.persona.clone(),
            min_attestation_level: gate.min_attestation_level.unwrap_or(1),
            stack_hash: gate.stack_hash.clone(),
            spec_path: entry.path.clone(),
        });
    }
    out
}

/// Resolve the manifest-declared gates for an MR (spec-registry.md §16).
///
/// Reads `specs/manifest.yaml` for the MR's repository and selects the entry
/// matching the MR's `spec_ref` path. The manifest is read at repo HEAD —
/// the same policy source task-193's approval resolver uses
/// (`resolve_new_approval_status`) — because the SHA embedded in `spec_ref`
/// is the spec file's *blob* SHA (the ledger's `current_sha`, set by
/// `sync_spec_ledger` via `git rev-parse <commit>:<path>`), which is not a
/// treeish and cannot address `specs/manifest.yaml` via
/// `git show <sha>:specs/manifest.yaml`.
async fn resolve_manifest_gates_for_mr(state: &AppState, mr_id: &Id) -> Vec<ManifestGateSpec> {
    let Ok(Some(mr)) = state.merge_requests.find_by_id(mr_id).await else {
        return Vec::new();
    };
    let Ok(Some(repo)) = state.repos.find_by_id(&mr.repository_id).await else {
        return Vec::new();
    };
    let manifest = crate::spec_registry::read_manifest(&repo.path, "HEAD").await;
    manifest_gates_for_mr(manifest.as_ref(), mr.spec_ref.as_deref())
}

/// Materialize a manifest-declared gate as a `QualityGate` for execution.
///
/// The gate is deliberately NOT persisted to the quality-gates store:
/// manifest gates are a per-MR selection (driven by the MR's `spec_ref`),
/// not repo configuration — persisting them would make every other MR in
/// the repo run them too. The deterministic id lets `check_gates_for_mr`
/// attribute results (unknown gate ids default to `required = true`, so a
/// failed manifest gate blocks the merge) and keeps re-enqueues idempotent.
fn materialize_manifest_gate(repo_id: &Id, mg: &ManifestGateSpec) -> gyre_domain::QualityGate {
    gyre_domain::QualityGate {
        id: Id::new(format!(
            "manifest-gate:{}:{}:{}",
            repo_id, mg.spec_path, mg.persona
        )),
        repo_id: repo_id.clone(),
        name: format!("manifest gate: {} ({})", mg.persona, mg.spec_path),
        gate_type: GateType::AgentReview,
        command: None,
        required_approvals: None,
        persona: Some(format!("personas/{}.md", mg.persona)),
        validation_type: None,
        required: true,
        gate_phase: Default::default(),
        // Manifest gates are a per-MR selection, not part of the repo's
        // stored gate chain — no chain position to occupy. `0` matches the
        // convention for gates outside the ordered repo chain (ties broken
        // by created_at).
        position: 0,
        timeout_secs: None,
        created_at: now_secs(),
    }
}

/// TTL for a gate agent's JWT: the default gate timeout window plus slack
/// for the agent runtime to poll the ReviewRequest, review the diff, and
/// submit its verdict (plus raise its workload attestation).
const GATE_AGENT_TOKEN_TTL_SECS: u64 = AGENT_GATE_TIMEOUT_SECS + 600;

/// Launch the forge-spawned gate agent's runtime process (spec-registry.md
/// §16: "Forge spawns security gate agent and accountability gate agent for
/// this MR").
///
/// Delegates to the platform's single agent-process execution path
/// (`api::spawn::launch_agent_workload` — the same machinery
/// `spawn_agent_core` uses): the resolved compute target (workspace
/// assignment → tenant default → local) launches the agent runtime command
/// with the gate agent's scoped token in `GYRE_AUTH_TOKEN` — the credential
/// delivery channel every spawned agent's runtime uses — plus the review
/// context (MR, gate, persona, attestation constraints). The launch also
/// writes the gate agent's workload attestation record (the
/// `workload_attestations` namespace `check_reviewer_attestation` derives
/// levels from): without it the agent's own reviews would sit at attestation
/// level 0 and could never satisfy even the manifest-schema default
/// `min_attestation_level` of 1.
///
/// Best-effort: when no runtime command is available (or the launch
/// fails), the gate still runs — it polls for a qualifying review and fails
/// closed at its timeout; it never auto-passes.
async fn launch_gate_agent_runtime(
    state: &Arc<AppState>,
    agent: &gyre_domain::Agent,
    mr: &gyre_domain::MergeRequest,
    mg: &ManifestGateSpec,
    gate: &gyre_domain::QualityGate,
    token: &str,
) {
    use gyre_domain::{ComputeTargetEntity, ComputeTargetType};

    // Resolve the compute target with the same priority spawn_agent_core
    // uses: workspace assignment → tenant default → local.
    let workspace = state
        .workspaces
        .find_by_id(&mr.workspace_id)
        .await
        .ok()
        .flatten();
    let resolved_ct_entity: Option<ComputeTargetEntity> = if let Some(ws) = &workspace {
        if let Some(ct_id) = ws.compute_target_id.clone() {
            state.compute_targets.get_by_id(&ct_id).await.ok().flatten()
        } else {
            state
                .compute_targets
                .get_default_for_tenant(&ws.tenant_id)
                .await
                .ok()
                .flatten()
        }
    } else {
        None
    };
    let compute_target_label = resolved_ct_entity
        .as_ref()
        .map(|e| e.id.to_string())
        .unwrap_or_else(|| "local".to_string());
    let resolved_target_config =
        resolved_ct_entity
            .as_ref()
            .map(|e| crate::api::compute::ComputeTargetConfig {
                id: e.id.to_string(),
                name: e.name.clone(),
                target_type: match e.target_type {
                    ComputeTargetType::Container => "container",
                    ComputeTargetType::Ssh => "ssh",
                    ComputeTargetType::Kubernetes => "kubernetes",
                }
                .to_string(),
                config: e.config.clone(),
            });

    // Command is server-controlled only — never from user input (C-1 RCE
    // fix): compute target's configured command, GYRE_AGENT_COMMAND env
    // var, or the platform entrypoint.
    let command = resolved_target_config
        .as_ref()
        .and_then(|cfg| cfg.config.get("command"))
        .and_then(|v| v.as_str())
        .map(String::from)
        .or_else(|| std::env::var("GYRE_AGENT_COMMAND").ok())
        .unwrap_or_else(|| "/gyre/entrypoint.sh".to_string());
    let args: Vec<String> = resolved_target_config
        .as_ref()
        .and_then(|cfg| cfg.config.get("args"))
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    // The gate agent reviews the MR, so the repo checkout is its working
    // directory (canonicalized to an absolute path — containers require
    // one; falls back to the platform's absolute /workspace default when
    // the repo cannot be resolved).
    let work_dir = state
        .repos
        .find_by_id(&mr.repository_id)
        .await
        .ok()
        .flatten()
        .map(|r| {
            std::fs::canonicalize(&r.path)
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or(r.path)
        })
        .unwrap_or_else(|| "/workspace".to_string());

    // Review context for the launched runtime: everything it needs to
    // authenticate (GYRE_AUTH_TOKEN), identify its assignment (MR, gate,
    // persona), and know the constraints its review must satisfy.
    let mut env = std::collections::HashMap::new();
    env.insert("GYRE_SERVER_URL".to_string(), state.base_url.clone());
    env.insert("GYRE_AUTH_TOKEN".to_string(), token.to_string());
    env.insert("GYRE_AGENT_ID".to_string(), agent.id.to_string());
    env.insert("GYRE_PERSONA".to_string(), mg.persona.clone());
    env.insert("GYRE_MR_ID".to_string(), mr.id.to_string());
    env.insert("GYRE_REPO_ID".to_string(), mr.repository_id.to_string());
    env.insert("GYRE_GATE_ID".to_string(), gate.id.to_string());
    env.insert(
        "GYRE_DIFF_URL".to_string(),
        format!("{}/api/v1/merge-requests/{}/diff", state.base_url, mr.id),
    );
    if let Some(spec_ref) = &mr.spec_ref {
        env.insert("GYRE_SPEC_REF".to_string(), spec_ref.clone());
    }
    env.insert(
        "GYRE_MIN_ATTESTATION_LEVEL".to_string(),
        mg.min_attestation_level.to_string(),
    );
    if let Some(stack_hash) = &mg.stack_hash {
        env.insert("GYRE_STACK_HASH".to_string(), stack_hash.clone());
    }

    match crate::api::spawn::launch_agent_workload(
        state,
        agent,
        resolved_target_config,
        command,
        args,
        env,
        work_dir,
        compute_target_label.clone(),
    )
    .await
    {
        Ok(_) => info!(
            agent_id = %agent.id,
            mr_id = %mr.id,
            persona = %mg.persona,
            compute_target = %compute_target_label,
            "manifest gate: launched gate agent runtime"
        ),
        Err(e) => warn!(
            mr_id = %mr.id,
            persona = %mg.persona,
            error = e.to_string(),
            "manifest gate: gate agent runtime launch rejected; gate will poll reviews"
        ),
    }
}

/// Spawn the ephemeral gate agent for a manifest-declared gate
/// (spec-registry.md §16: "Forge spawns security gate agent and
/// accountability gate agent for this MR"; agent-gates.md §Gate Agent
/// Lifecycle: ephemeral, single-purpose, attributed).
///
/// This is the production spawn path for manifest gates. It creates a real
/// `Agent` record, records the gate persona in the `agent_personas` KV
/// namespace (the persona source `build_agent_context` and
/// `evaluate_persona_reviews` read — spawn time is its production writer),
/// mints a scoped auth token the gate agent runtime authenticates with,
/// launches the agent's runtime process through the platform's compute-target
/// machinery (see `launch_gate_agent_runtime` — the token is delivered to the
/// launched runtime via `GYRE_AUTH_TOKEN`, and its workload attestation is
/// recorded), and dispatches a `ReviewRequest` message telling it to review
/// the MR. The agent submits its verdict through the standard review API;
/// the gate observes the review and tears the agent down once it resolves
/// (`teardown_gate_agent`).
///
/// Idempotent per (MR, persona) while the gate runs: an already-Active
/// agent with the deterministic name is reused, so re-enqueueing an MR does
/// not double-spawn. A torn-down (terminal) agent is replaced under a
/// fresh suffixed name.
async fn spawn_manifest_gate_agent(
    state: &Arc<AppState>,
    mr: &gyre_domain::MergeRequest,
    mg: &ManifestGateSpec,
    gate: &gyre_domain::QualityGate,
) -> Option<Id> {
    let base_name = format!("gate-{}-{}", mg.persona, mr.id.as_str());
    let mut name = base_name.clone();
    let mut retry = 1u32;
    loop {
        match state.agents.find_by_name(&name).await {
            Ok(Some(existing)) => {
                if existing.status == AgentStatus::Active {
                    // Gate still running from a previous enqueue — reuse its
                    // agent rather than double-spawning the persona.
                    info!(
                        agent_id = %existing.id,
                        mr_id = %mr.id,
                        persona = %mg.persona,
                        "manifest gate: reusing running gate agent"
                    );
                    return Some(existing.id);
                }
                // Terminal (torn down) agent retires its name; take the next
                // one so find_by_name reuse stays unambiguous.
                retry += 1;
                name = format!("{base_name}-r{retry}");
            }
            Ok(None) => break,
            Err(e) => {
                warn!(
                    mr_id = %mr.id,
                    persona = %mg.persona,
                    error = format!("{e:#}"),
                    "manifest gate: gate agent name lookup failed; gate will poll reviews"
                );
                return None;
            }
        }
    }

    let mut agent = gyre_domain::Agent::new(Id::new(Uuid::new_v4().to_string()), name, now_secs());
    agent.workspace_id = mr.workspace_id.clone();
    agent.spawned_by = Some("server".to_string());
    if let Err(e) = agent.transition_status(AgentStatus::Active) {
        warn!(mr_id = %mr.id, error = e.to_string(), "manifest gate: agent activation failed");
        return None;
    }
    if let Err(e) = state.agents.create(&agent).await {
        warn!(
            mr_id = %mr.id,
            error = format!("{e:#}"),
            "manifest gate: gate agent record creation failed; gate will poll reviews"
        );
        return None;
    }

    // Production persona record: the same namespace build_agent_context and
    // the persona gate read. Without a writer here (and at agent spawn), no
    // reviewer could ever match a gate persona in a real deployment.
    if let Err(e) = state
        .kv_store
        .kv_set("agent_personas", agent.id.as_str(), mg.persona.clone())
        .await
    {
        warn!(
            agent_id = %agent.id,
            error = format!("{e:#}"),
            "manifest gate: failed to record gate agent persona"
        );
    }

    // Scoped reviewer identity (review repair): `review:submit` only, minted
    // with `mint_scoped` exactly like the command-based gate path
    // (`build_review_agent_context`). `submit_review` binds the reviewer to
    // the verified token subject ONLY for review:submit-scoped JWTs — a
    // general-purpose "agent"-scoped token here would leave the gate
    // agent's reviews recorded under a request-body reviewer id, so the
    // persona / min_attestation_level / stack_hash constraints would be
    // evaluated against an identity the token never verified. The token
    // still lets the runtime read the MR diff, fetch the pinned spec, and
    // submit its verdict; its ReviewRequest arrives via the launch env
    // (GYRE_MR_ID / GYRE_PERSONA / …) and the /mcp surface
    // (`gyre_message_poll` derives the inbox from the verified token
    // subject, so it is inherently own-inbox). The REST inbox route
    // (GET /api/v1/agents/{id}/messages) is kept off the reviewer
    // allow-list as least privilege / defense in depth: the routed
    // handler (messages::poll_messages) does enforce own-inbox for
    // non-admin callers, but a single-purpose review token has no need
    // for it. A mint or registration
    // failure fails the spawn closed: an unscoped fallback token is exactly
    // the hole this closes, and with no gate agent running the gate polls
    // reviews and fails at its timeout (never auto-passes).
    let token = match state.agent_signing_key.mint_scoped(
        agent.id.as_str(),
        gate.id.as_str(),
        "forge",
        &state.base_url,
        GATE_AGENT_TOKEN_TTL_SECS,
        crate::auth::REVIEW_SUBMIT_SCOPE,
    ) {
        Ok(token) => token,
        Err(e) => {
            warn!(
                agent_id = %agent.id,
                mr_id = %mr.id,
                persona = %mg.persona,
                error = %e,
                "manifest gate: scoped reviewer token mint failed; failing the gate agent spawn closed"
            );
            let _ = agent.transition_status(AgentStatus::Stopped);
            let _ = state.agents.update(&agent).await;
            return None;
        }
    };
    if let Err(e) = state
        .kv_store
        .kv_set("agent_tokens", agent.id.as_str(), token.clone())
        .await
    {
        warn!(
            agent_id = %agent.id,
            mr_id = %mr.id,
            error = format!("{e:#}"),
            "manifest gate: failed to register gate agent token; failing the gate agent spawn closed"
        );
        let _ = agent.transition_status(AgentStatus::Stopped);
        let _ = state.agents.update(&agent).await;
        return None;
    }

    // Dispatch the review request the gate agent runtime polls for (MCP
    // gyre_message_poll / GET messages). Carries everything the runtime
    // needs: the MR, the persona to review as, and the attestation
    // constraints its review must satisfy.
    state
        .emit_event(
            Some(mr.workspace_id.clone()),
            gyre_common::message::Destination::Agent(agent.id.clone()),
            gyre_common::message::MessageKind::ReviewRequest,
            Some(serde_json::json!({
                "mr_id": mr.id.to_string(),
                "repo_id": mr.repository_id.to_string(),
                "spec_path": mg.spec_path,
                "spec_ref": mr.spec_ref,
                "persona": mg.persona,
                "min_attestation_level": mg.min_attestation_level,
                "stack_hash": mg.stack_hash,
            })),
        )
        .await;

    info!(
        agent_id = %agent.id,
        mr_id = %mr.id,
        persona = %mg.persona,
        "manifest gate: spawned gate agent"
    );

    // Launch the agent's runtime process (compute-target machinery, token in
    // GYRE_AUTH_TOKEN, workload attestation recorded). Must happen after the
    // token registration above so the launched runtime can authenticate
    // immediately.
    launch_gate_agent_runtime(state, &agent, mr, mg, gate, &token).await;

    Some(agent.id)
}

/// Tear down a forge-spawned gate agent after its gate resolved
/// (agent-gates.md §Gate Agent Lifecycle: "Agent is torn down after
/// submitting its verdict"). Kills its runtime process/container/Pod via
/// the backend that owns the handle (`spawned_backends` — the same
/// invariant the admin kill path implements and spawn.rs enforces with a
/// dedicated test: a container/SshDocker/Kubernetes handle must go to
/// `docker rm --force` / `kubectl delete pod` / the remote docker daemon,
/// and an SshBare handle's pid is a REMOTE-host pid that LocalTarget must
/// never be handed), removes the spawned_backends entry the launch
/// registered (no map leak), revokes its token (audited via
/// AgentTokenRevoked) and transitions it to Stopped — the record is kept
/// so its reviews stay attributed.
async fn teardown_gate_agent(state: &Arc<AppState>, agent_id: &Id) {
    let Ok(Some(mut agent)) = state.agents.find_by_id(agent_id).await else {
        return;
    };
    // Kill the runtime process/container/Pod if it is still running
    // (best-effort — the handle may already be gone if the process exited
    // on its own). The backend recorded at spawn time decides how;
    // LocalTarget (pid kill) is only the fallback for handles with no
    // recorded backend. The spawned_backends entry is removed
    // unconditionally so it cannot leak when the handle was already reaped.
    let handle = state
        .process_registry
        .lock()
        .await
        .remove(agent_id.as_str());
    let backend = state
        .spawned_backends
        .lock()
        .await
        .remove(agent_id.as_str());
    if let Some(handle) = handle {
        let kill_result = match backend.as_deref() {
            Some(b) => gyre_ports::ComputeTarget::kill_process(b, &handle).await,
            None => {
                gyre_ports::ComputeTarget::kill_process(
                    &gyre_adapters::compute::LocalTarget,
                    &handle,
                )
                .await
            }
        };
        if let Err(e) = kill_result {
            warn!(
                agent_id = %agent_id,
                error = format!("{e:#}"),
                "manifest gate: gate agent process kill failed (may have already exited)"
            );
        }
    }
    crate::agent_audit::revoke_agent_token(state, &agent, "manifest gate resolved").await;
    if let Err(e) = agent.transition_status(AgentStatus::Stopped) {
        warn!(
            agent_id = %agent_id,
            error = e.to_string(),
            "manifest gate: gate agent teardown status transition failed"
        );
        return;
    }
    if let Err(e) = state.agents.update(&agent).await {
        warn!(
            agent_id = %agent_id,
            error = format!("{e:#}"),
            "manifest gate: gate agent teardown update failed"
        );
    }
}

/// Create pending GateResult records for all gates belonging to the MR's
/// repo, plus the per-spec gates declared by the spec manifest for the MR's
/// `spec_ref` (spec-registry.md §16), then spawn a background task that
/// runs each gate and updates the result.
pub async fn trigger_gates_for_mr(state: Arc<AppState>, mr_id: Id, repo_id: Id) {
    // Collect gates for this repo.
    let gates = state
        .quality_gates
        .list_by_repo_id(repo_id.as_str())
        .await
        .unwrap_or_default();

    // Manifest-declared per-spec gates: an MR whose spec_ref names a spec
    // also runs the gate agents that spec's manifest entry declares.
    let manifest_gates = resolve_manifest_gates_for_mr(&state, &mr_id).await;

    if gates.is_empty() && manifest_gates.is_empty() {
        return;
    }

    // Union, don't replace — but never double-spawn a persona. When a
    // repo-level gate and a manifest gate target the same persona, the
    // MANIFEST gate wins: the manifest is the reviewed per-spec policy
    // artifact (spec-registry.md §16) and carries the
    // min_attestation_level / stack_hash constraints that gate's reviewer
    // must satisfy — constraints a repo-level gate cannot enforce. The
    // alternative (repo-gate-wins) is strictly weaker, not just redundant:
    // a no-command repo-level AgentReview gate fails closed (it cannot
    // spawn a reviewer, so a required gate BLOCKS the merge), and a
    // command-based repo gate runs a reviewer with no attestation
    // constraints at all. Either way nothing is lost by skipping the
    // repo-level duplicate: the manifest gate spawns a real gate agent
    // runtime (launched through the same compute-target machinery
    // `spawn_agent_core` uses, with a workload attestation — see
    // `launch_gate_agent_runtime`), so the persona actually reviews, under
    // the manifest's constraints. The repo-level duplicate for that
    // persona is skipped, so the persona still runs exactly once.
    let manifest_personas: std::collections::HashSet<&str> = manifest_gates
        .iter()
        .map(|mg| mg.persona.as_str())
        .collect();
    let gates: Vec<gyre_domain::QualityGate> = gates
        .into_iter()
        .filter(|g| {
            g.persona
                .as_deref()
                .map(persona_name_from_path)
                .is_none_or(|p| !manifest_personas.contains(p))
        })
        .collect();

    // Forge-spawn the gate agent for each manifest gate (spec-registry.md
    // §16: "Forge spawns security gate agent and accountability gate agent
    // for this MR") — a real Agent record carrying the persona, a scoped
    // auth token, and a dispatched ReviewRequest the agent runtime picks
    // up. The MR is guaranteed to exist here: manifest gates resolve only
    // for an MR whose spec_ref was read.
    let mr = state.merge_requests.find_by_id(&mr_id).await.ok().flatten();

    // Create Pending GateResult for each gate.
    let mut result_ids: Vec<(Id, gyre_domain::QualityGate, Option<GateConstraints>)> = Vec::new();
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
        result_ids.push((result_id, gate.clone(), None));
    }
    for mg in &manifest_gates {
        let gate = materialize_manifest_gate(&repo_id, mg);
        let gate_agent_id = match mr.as_ref() {
            Some(mr) => spawn_manifest_gate_agent(&state, mr, mg, &gate).await,
            None => None,
        };
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
        result_ids.push((
            result_id,
            gate,
            Some(GateConstraints {
                min_attestation_level: mg.min_attestation_level,
                stack_hash: mg.stack_hash.clone(),
                gate_agent_id,
            }),
        ));
    }

    // Spawn background tasks for each gate.
    for (result_id, gate, constraints) in result_ids {
        let state = state.clone();
        let mr_id = mr_id.clone();
        tokio::spawn(async move {
            run_gate(state, result_id, gate, mr_id, constraints).await;
        });
    }
}

async fn run_gate(
    state: Arc<AppState>,
    result_id: Id,
    gate: gyre_domain::QualityGate,
    mr_id: Id,
    constraints: Option<GateConstraints>,
) {
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
        GateType::AgentReview => {
            run_agent_review_gate(&state, &gate, &mr_id, constraints.as_ref()).await
        }
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

    // Auto-track gate outcome (analytics.md §Auto-Emitted Events).
    let gate_type_str = format!("{:?}", gate.gate_type);
    let duration_secs = finished_at.saturating_sub(started_at);
    let (event_name, props) = if status == GateStatus::Failed {
        (
            "gate.failed",
            serde_json::json!({
                "gate_id": gate.id.to_string(),
                "gate_type": gate_type_str,
                "mr_id": mr_id.to_string(),
                "output_snippet": truncate_bytes(&output, 512),
            }),
        )
    } else {
        (
            "gate.passed",
            serde_json::json!({
                "gate_id": gate.id.to_string(),
                "gate_type": gate_type_str,
                "mr_id": mr_id.to_string(),
                "duration_secs": duration_secs,
            }),
        )
    };
    let scope = state
        .merge_requests
        .find_by_id(&mr_id)
        .await
        .ok()
        .flatten()
        .map(|mr| (mr.workspace_id, mr.repository_id));
    let ev = gyre_domain::AnalyticsEvent::new(
        gyre_common::Id::new(uuid::Uuid::new_v4().to_string()),
        event_name,
        None,
        props,
        finished_at,
    )
    .with_scope(
        None,
        None,
        scope.as_ref().map(|(ws, _)| ws),
        scope.as_ref().map(|(_, repo)| repo),
    );
    let _ = state.analytics.record(&ev).await;

    // Emit GateFailure event so the MR's author agent can react immediately.
    if status == GateStatus::Failed {
        let gate_type_str = format!("{:?}", gate.gate_type);
        match state
            .merge_requests
            .find_by_id(&mr_id)
            .await
            .ok()
            .flatten()
            .map(|mr| (mr.spec_ref, mr.workspace_id))
        {
            Some((spec_ref, ws_id)) => {
                let gate_agent_id = format!("gate-agent:{}", gate.id);
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
            None => {
                // The MR's workspace scope cannot be determined — skip the
                // scope-scoped event emission rather than fabricating a
                // "default" workspace identity (task-097 F3).
                tracing::warn!(
                    "gate_executor: merge request {mr_id} not found; skipping GateFailure event emission"
                );
            }
        }
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

    // Notify the MR author when this pass makes ALL gates green
    // (user-management.md §Notification Entity: GatePassed — Low,
    // informational). De-duplicated per (user, MR) inside the helper so the
    // last-finishing gates do not repeat the announcement.
    if status == GateStatus::Passed {
        if let Ok(true) = check_gates_for_mr(state.as_ref(), &mr_id).await {
            if let Ok(Some(mr)) = state.merge_requests.find_by_id(&mr_id).await {
                if let Some(ref author_id) = mr.author_agent_id {
                    // The tenant scope comes from the workspace lookup — when
                    // it cannot be resolved there is no scoperable tenant, so
                    // skip rather than fabricate one
                    // (check-fabricated-scope-defaults.sh).
                    if let Ok(Some(ws)) = state.workspaces.find_by_id(&mr.workspace_id).await {
                        crate::notifications::notify_gate_passed(
                            state.as_ref(),
                            author_id,
                            &mr.workspace_id,
                            &mr_id.to_string(),
                            &ws.tenant_id.to_string(),
                        )
                        .await;
                    }
                }
            }
        }
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
/// If the gate carries manifest-declared attestation constraints
/// (spec-registry.md §16), runs the persona-constrained review path, which
/// never auto-passes: it requires an Approved review from an agent running
/// the named persona whose workload attestation satisfies the constraints.
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
    constraints: Option<&GateConstraints>,
) -> (GateStatus, String) {
    let persona = gate.persona.as_deref().unwrap_or("personas/default.md");

    match &gate.command {
        Some(cmd) => run_review_agent_process(state, gate, mr_id, cmd, persona).await,
        None if constraints.is_some() => {
            // Manifest-declared gate (spec-registry.md §16): enforce the
            // persona + attestation constraints — never the auto-pass stub.
            run_persona_review_gate(
                state,
                gate,
                mr_id,
                persona,
                constraints.expect("checked by match guard"),
            )
            .await
        }
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
///
/// A workspace that cannot be loaded has no resolvable tenant: the tenant
/// scope is skipped (logged) rather than fabricated as `"default"` — a
/// fabricated scope identity would silently re-target persona lookup and
/// leak a real tenant named "default" (frozen-baseline check on scope
/// fabrication).
async fn resolve_review_persona(
    state: &Arc<AppState>,
    repo: &gyre_domain::Repository,
    slug: &str,
) -> Option<gyre_domain::Persona> {
    let workspace = match state.workspaces.find_by_id(&repo.workspace_id).await {
        Ok(ws) => ws,
        Err(e) => {
            warn!(
                workspace_id = %repo.workspace_id,
                error = %e,
                "agent_review gate: workspace lookup failed; skipping tenant persona scope"
            );
            None
        }
    };

    let mut scopes = vec![
        gyre_domain::PersonaScope::Repo(repo.id.clone()),
        gyre_domain::PersonaScope::Workspace(repo.workspace_id.clone()),
    ];
    if let Some(ws) = workspace {
        scopes.push(gyre_domain::PersonaScope::Tenant(ws.tenant_id));
    } else {
        warn!(
            workspace_id = %repo.workspace_id,
            "agent_review gate: workspace not found; skipping tenant persona scope"
        );
    }

    for scope in scopes {
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
    let resolved = resolve_review_persona(state, &repo, slug)
        .await
        .ok_or_else(|| {
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
    // have moved since. Both pin shapes resolve through the port: a commit
    // SHA + tree path, or the production blob SHA from the spec ledger
    // (whose ledger path is not a repo tree path — the blob IS the content).
    // An unresolvable pinned SHA is a context-gathering failure
    // (fail-closed), not an empty spec.
    let spec_ref = mr.spec_ref.clone().unwrap_or_default();
    let spec_content = if let Some((path, sha)) = spec_ref.rsplit_once('@') {
        match state
            .git_ops
            .read_file_at_commit(&repo.path, sha, path)
            .await
        {
            Ok(Some(bytes)) => Some(
                String::from_utf8(bytes)
                    .map_err(|_| format!("spec '{path}' at {sha} is not valid UTF-8"))?,
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
            gate_token_ttl_secs(gate),
            crate::auth::REVIEW_SUBMIT_SCOPE,
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

    // agent-gates.md §Integration with Agent Gates step 1: the gate agent
    // receives the MR diff AND the spec at the pinned SHA (not HEAD). The
    // endpoint serves the blob content at the MR's spec_ref SHA, so a spec
    // edited after the MR was opened cannot silently change what the gate
    // reviews against.
    let spec_url = if ctx.spec_ref.is_empty() {
        String::new()
    } else {
        format!("{}/api/v1/merge-requests/{}/spec", state.base_url, mr_id)
    };

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
        .env("GYRE_SPEC_URL", &spec_url)
        .env("GYRE_PERSONA", persona)
        .env("GYRE_PERSONA_SLUG", &ctx.persona_slug)
        .env("GYRE_PERSONA_PROMPT", &ctx.persona_prompt)
        .env(
            "GYRE_SPEC_CONTENT",
            ctx.spec_content.as_deref().unwrap_or(""),
        )
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

/// Outcome of evaluating an MR's reviews against a persona-constrained
/// manifest gate.
enum PersonaReview {
    /// An Approved review from the named persona satisfying all constraints.
    Qualified(String),
    /// A persona agent explicitly requested changes — fail fast.
    Rejected(String),
    /// No qualifying review yet; carries why a near-miss approval fell short.
    Waiting(Option<String>),
}

/// Persona-driven review gate for manifest-declared gates (spec-registry.md
/// §16 Agent Gates Integration: "Forge spawns security gate agent and
/// accountability gate agent for this MR").
///
/// Passes only when the MR has an `Approved` review from an agent running
/// the named persona whose workload attestation satisfies the configured
/// constraints — the same validity rules the spec-approval ledger applies to
/// agent approvals (spec-registry.md §9, task-193):
/// - the reviewer's persona matches the gate persona,
/// - `attestation_level >= min_attestation_level`, and
/// - when `stack_hash` is pinned, the reviewer's stack fingerprint matches
///   exactly.
///
/// This is the real persona path, not the auto-pass stub: with no qualifying
/// review the gate FAILS (after waiting up to the gate timeout for a persona
/// agent to submit one), and the failure blocks the merge via
/// `check_gates_for_mr`.
async fn run_persona_review_gate(
    state: &Arc<AppState>,
    gate: &gyre_domain::QualityGate,
    mr_id: &Id,
    persona_path: &str,
    constraints: &GateConstraints,
) -> (GateStatus, String) {
    let persona = persona_name_from_path(persona_path);
    let timeout_secs = gate.timeout_secs.unwrap_or(AGENT_GATE_TIMEOUT_SECS);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(timeout_secs);

    let result = loop {
        match evaluate_persona_reviews(state, mr_id, persona, constraints).await {
            PersonaReview::Qualified(output) => break (GateStatus::Passed, output),
            PersonaReview::Rejected(reason) => break (GateStatus::Failed, reason),
            PersonaReview::Waiting(near_miss) => {
                if tokio::time::Instant::now() >= deadline {
                    let mut msg = format!(
                        "manifest gate: no Approved review from persona '{persona}' satisfying \
                         constraints (min_attestation_level={}, stack_hash={}) within {timeout_secs}s",
                        constraints.min_attestation_level,
                        constraints.stack_hash.as_deref().unwrap_or("any"),
                    );
                    if let Some(reason) = near_miss {
                        msg.push_str(" — ");
                        msg.push_str(&reason);
                    }
                    break (GateStatus::Failed, msg);
                }
                tokio::time::sleep(PERSONA_GATE_POLL_INTERVAL).await;
            }
        }
    };

    // Single-purpose gate agent: torn down once its gate resolved, whatever
    // the outcome (agent-gates.md §Gate Agent Lifecycle — "spawned for one
    // review, torn down after").
    if let Some(agent_id) = &constraints.gate_agent_id {
        teardown_gate_agent(state, agent_id).await;
    }

    result
}

/// Evaluate an MR's reviews against a persona-constrained gate.
///
/// Only reviews from agents running the named persona count (persona from
/// the KV `agent_personas` cache — the same source `build_agent_context`
/// reads). Reviews are append-only, so each reviewer's LATEST review is the
/// one that counts: the normal review cycle (request changes → author
/// fixes → same agent approves) must not leave a stale ChangesRequested
/// vetoing that agent's own later Approved. Approval wins across reviewers
/// (matching `check_review_verdict` and the mem `is_approved` semantics):
/// any qualifying Approved from the persona passes even when another
/// persona reviewer's final verdict is ChangesRequested; with no
/// qualifying approval, a persona agent's final ChangesRequested fails the
/// gate immediately. An `Approved` review passes only when the reviewer's
/// workload attestation meets the constraints (level + pinned stack hash).
async fn evaluate_persona_reviews(
    state: &Arc<AppState>,
    mr_id: &Id,
    persona: &str,
    constraints: &GateConstraints,
) -> PersonaReview {
    let reviews = state.reviews.list_reviews(mr_id).await.unwrap_or_default();

    // Latest review per reviewer. The listing is chronological (mem and
    // SQLite both sort by created_at); `>=` makes the later-seen review win
    // timestamp ties, matching append order.
    let mut latest_per_reviewer: std::collections::HashMap<&str, &Review> =
        std::collections::HashMap::new();
    for review in &reviews {
        let reviewer = review.reviewer_agent_id.as_str();
        match latest_per_reviewer.get(reviewer) {
            Some(prev) if prev.created_at > review.created_at => {}
            _ => {
                latest_per_reviewer.insert(reviewer, review);
            }
        }
    }

    let mut near_miss: Option<String> = None;
    let mut rejection: Option<String> = None;
    for review in latest_per_reviewer.into_values() {
        let Some(reviewer_persona) =
            lookup_reviewer_persona(state, &review.reviewer_agent_id).await
        else {
            continue;
        };
        if reviewer_persona != persona {
            continue;
        }
        match review.decision {
            ReviewDecision::Approved => {
                match check_reviewer_attestation(state, &review.reviewer_agent_id, constraints)
                    .await
                {
                    Ok(()) => {
                        return PersonaReview::Qualified(format!(
                            "manifest gate: persona '{persona}' agent {} approved the MR \
                             (attestation constraints satisfied)",
                            review.reviewer_agent_id
                        ));
                    }
                    Err(reason) => {
                        near_miss = Some(format!(
                            "persona '{persona}' agent {} approved but {reason}",
                            review.reviewer_agent_id
                        ));
                    }
                }
            }
            ReviewDecision::ChangesRequested => {
                // Deferred, not immediate: a qualifying approval from another
                // persona reviewer wins (approval-wins). Only fail fast when
                // no qualifying approval exists.
                if rejection.is_none() {
                    let body = review.body.as_deref().unwrap_or("no feedback provided");
                    rejection = Some(format!(
                        "manifest gate: persona '{persona}' agent {} requested changes: {body}",
                        review.reviewer_agent_id
                    ));
                }
            }
        }
    }
    if let Some(reason) = rejection {
        return PersonaReview::Rejected(reason);
    }
    PersonaReview::Waiting(near_miss)
}

/// Look up an agent's persona name from the KV persona cache (the same
/// source `build_agent_context` reads). Reviewer ids may be recorded with an
/// `agent:` prefix (display form, as `approve_spec` records) or as the raw
/// agent id (as the KV is keyed) — try both key forms.
async fn lookup_reviewer_persona(state: &Arc<AppState>, reviewer_agent_id: &str) -> Option<String> {
    let raw = reviewer_agent_id
        .strip_prefix("agent:")
        .unwrap_or(reviewer_agent_id);
    for key in [reviewer_agent_id, raw] {
        if let Ok(Some(p)) = state.kv_store.kv_get("agent_personas", key).await {
            return Some(p);
        }
    }
    None
}

/// Check a reviewer's workload attestation against the gate constraints
/// (spec-registry.md §16 Agent Approver / Gate Schema; mirrors the §9
/// agent-approval validity rules task-193 implemented for the ledger):
/// `attestation_level >= min_attestation_level` and, when `stack_hash` is
/// pinned, an exact stack-fingerprint match. The level is derived from the
/// reviewer's real workload-attestation record — never invented.
async fn check_reviewer_attestation(
    state: &Arc<AppState>,
    reviewer_agent_id: &str,
    constraints: &GateConstraints,
) -> Result<(), String> {
    let raw = reviewer_agent_id
        .strip_prefix("agent:")
        .unwrap_or(reviewer_agent_id);
    let workload = state
        .kv_store
        .kv_get("workload_attestations", raw)
        .await
        .ok()
        .flatten()
        .and_then(|json| {
            serde_json::from_str::<crate::workload_attestation::WorkloadAttestation>(&json).ok()
        });

    let level = crate::constraint_check::derive_attestation_level(workload.as_ref()) as u32;
    if level < constraints.min_attestation_level {
        return Err(format!(
            "attestation level {level} < required {}",
            constraints.min_attestation_level
        ));
    }
    if let Some(pinned) = &constraints.stack_hash {
        let actual = workload.map(|w| w.stack_fingerprint).unwrap_or_default();
        if &actual != pinned {
            return Err(format!(
                "stack hash mismatch: reviewer stack '{actual}' != pinned '{pinned}'"
            ));
        }
    }
    Ok(())
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
                    "agent_validation gate failed: no agent command configured \
                     (persona={persona}, validation_type={}); \
                     configure the gate's command to spawn a validation agent",
                    gate.validation_type.as_deref().unwrap_or("<none>")
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
    // Scoped validator identity: `validation:report` lets the validator read
    // MR context and nothing else — its contract is the process exit code,
    // so unlike a reviewer it carries no review-submission capability (a
    // validator must not be able to influence review-based approval
    // surfaces). A mint failure fails the gate before anything is spawned —
    // a fabricated fallback token is an identity the auth extractor can
    // never validate, so the agent could not authenticate at all ("cannot
    // determine state" is not "state is fine"). Mirrors
    // build_review_agent_context's fail-closed mint path.
    let gate_agent_id = format!("gate-validate-{}", Uuid::new_v4());
    let gate_token = match state.agent_signing_key.mint_scoped(
        &gate_agent_id,
        gate.id.as_str(),
        "forge",
        &state.base_url,
        gate_token_ttl_secs(gate),
        crate::auth::VALIDATION_REPORT_SCOPE,
    ) {
        Ok(token) => token,
        Err(e) => {
            warn!(
                gate_id = %gate.id,
                mr_id = %mr_id,
                error = %e,
                "agent_validation gate: scoped token mint failed"
            );
            return (
                GateStatus::Failed,
                format!("agent_validation gate failed: scoped token mint failed: {e}"),
            );
        }
    };
    if let Err(e) = state
        .kv_store
        .kv_set("agent_tokens", &gate_agent_id, gate_token.clone())
        .await
    {
        return (
            GateStatus::Failed,
            format!("agent_validation gate failed: token registration failed: {e:#}"),
        );
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

    // Same pinned-SHA spec delivery as the review gate (agent-gates.md
    // §Integration with Agent Gates): validation agents compare behavior
    // against the spec version the code was written for.
    let spec_url = if spec_ref.is_empty() {
        String::new()
    } else {
        format!("{}/api/v1/merge-requests/{}/spec", state.base_url, mr_id)
    };

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
        .env("GYRE_SPEC_URL", &spec_url)
        .env("GYRE_PERSONA", persona)
        .env(
            "GYRE_VALIDATION_TYPE",
            gate.validation_type.as_deref().unwrap_or(""),
        )
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
                    format!(
                        "agent_validation gate: validation passed \
                         (persona={persona}, validation_type={})\n{process_output}",
                        gate.validation_type.as_deref().unwrap_or("<none>")
                    ),
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
                        "agent_validation gate: validation failed \
                         (persona={persona}, validation_type={}, exit {:?}):\n{process_output}",
                        gate.validation_type.as_deref().unwrap_or("<none>"),
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
pub(crate) fn truncate_bytes(s: &str, limit: usize) -> &str {
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

    let result =
        match tokio::time::timeout(Duration::from_secs(timeout_secs), command.output()).await {
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
/// `mr_id` attributes the validation to the MR in scope when there is one
/// (single-entry merge, single-entry revert decision); atomic-group
/// validation and the recovery HEAD health check pass `None` — those runs
/// are attributable to a repo HEAD, not one MR, and the emitted gate
/// analytics events record a null mr_id rather than a fabricated one.
/// Every evaluated command gate records a `gate.passed`/`gate.failed`
/// analytics event (analytics.md §Auto-Emitted Events).
///
/// Gates run inside a detached git worktree checked out at `head_sha`, so
/// they validate the exact merged tree (platform-model.md §6, task-095
/// R2-1). If worktree preparation fails (e.g. mem mode without a repo on
/// disk), gates fall back to the server's working directory.
pub async fn run_post_merge_gates(
    state: &AppState,
    repo: &gyre_domain::Repository,
    head_sha: &str,
    mr_id: Option<&Id>,
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
    let result = run_post_merge_gate_commands(state, repo, mr_id, &gates, cwd.as_deref()).await;

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
    state: &AppState,
    repo: &gyre_domain::Repository,
    mr_id: Option<&Id>,
    gates: &[gyre_domain::QualityGate],
    cwd: Option<&std::path::Path>,
) -> Result<(), String> {
    for gate in gates {
        match &gate.gate_type {
            GateType::TestCommand | GateType::LintCommand => {
                let timeout = gate.timeout_secs.unwrap_or(AGENT_GATE_TIMEOUT_SECS);
                let started_at = now_secs();
                let (status, output) =
                    run_command_in_dir(gate.command.as_deref().unwrap_or("true"), cwd, timeout)
                        .await;
                let finished_at = now_secs();
                // Auto-track the post-merge gate outcome (analytics.md
                // §Auto-Emitted Events — task-146 review R4: post-merge
                // evaluations are quality-gate runs too; a required
                // post-merge failure that triggers the revert/recovery
                // protocol must be visible to gate analytics, exactly like
                // the pre-merge run_gate path). `mr_id` is the MR this
                // validation is attributable to when one is in scope —
                // atomic-group validation and the recovery HEAD health
                // check run against a repo HEAD, not a single MR, and
                // record a null mr_id rather than a fabricated one.
                let gate_type_str = format!("{:?}", gate.gate_type);
                let (event_name, props) = if status == GateStatus::Failed {
                    (
                        "gate.failed",
                        serde_json::json!({
                            "gate_id": gate.id.to_string(),
                            "gate_type": gate_type_str,
                            "mr_id": mr_id.map(|id| id.to_string()),
                            "output_snippet": truncate_bytes(&output, 512),
                        }),
                    )
                } else {
                    (
                        "gate.passed",
                        serde_json::json!({
                            "gate_id": gate.id.to_string(),
                            "gate_type": gate_type_str,
                            "mr_id": mr_id.map(|id| id.to_string()),
                            "duration_secs": finished_at.saturating_sub(started_at),
                        }),
                    )
                };
                let ev = gyre_domain::AnalyticsEvent::new(
                    gyre_common::Id::new(uuid::Uuid::new_v4().to_string()),
                    event_name,
                    None,
                    props,
                    finished_at,
                )
                .with_scope(None, None, Some(&repo.workspace_id), Some(&repo.id));
                let _ = state.analytics.record(&ev).await;
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
            validation_type: None,
            required: true,
            gate_phase: Default::default(),
            timeout_secs: None,
            created_at: now_secs(),
            position: 0,
        }
    }

    fn make_mr_id() -> Id {
        Id::new(Uuid::new_v4().to_string())
    }

    /// Seed a repository row (no persona) with a real on-disk bare git
    /// repository (source branch diverged from main so the MR diff
    /// resolves). Returns (mr_id, repo_id).
    async fn seed_mr_with_repo(state: &Arc<AppState>) -> (Id, Id) {
        let dir = std::env::temp_dir().join(format!("gyre-gate-fixture-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let repo_path = dir.to_str().unwrap().to_string();

        let git_ops = gyre_adapters::Git2OpsAdapter::new();
        git_ops.init_bare(&repo_path).await.unwrap();
        git_ops
            .create_initial_commit(&repo_path, "main")
            .await
            .unwrap();
        git_ops
            .create_branch(&repo_path, "feature-branch", "refs/heads/main")
            .await
            .unwrap();
        git_ops
            .write_file(
                &repo_path,
                "feature-branch",
                "src/widget.rs",
                b"pub fn widget() {}",
                "Add widget",
            )
            .await
            .unwrap();

        let repo_id = Id::new(Uuid::new_v4().to_string());
        let repo = gyre_domain::Repository::new(
            repo_id.clone(),
            Id::new("ws-test"),
            "review-repo",
            &repo_path,
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

    /// Review repair finding 2: production MR spec_refs carry the spec
    /// ledger's BLOB sha with a ledger path ("system/widget.md"), not a
    /// commit sha + full tree path (spawn.rs pins
    /// `spec_ledger.current_sha`). The gate's primary spec channel must
    /// resolve that shape — a commit-only lookup errors on the blob OID and
    /// the gate would fail without ever receiving the spec.
    ///
    /// Runs `build_review_agent_context` directly (the context-gathering
    /// step that feeds GYRE_SPEC_CONTENT / GYRE_SPEC_FILE and backs
    /// GYRE_SPEC_URL), so it needs no loopback listener and no agent
    /// process.
    #[tokio::test]
    async fn agent_review_context_resolves_blob_pinned_spec_ref() {
        let state = test_state();

        // Real bare repo: spec v1 lands on main, then main advances to v2 —
        // the pinned blob must win over the tip.
        let dir = std::env::temp_dir().join(format!("gyre-gate-blobref-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let repo_path = dir.join("bare.git");
        let repo_path_str = repo_path.to_str().unwrap().to_string();

        let git_ops = gyre_adapters::Git2OpsAdapter::new();
        git_ops.init_bare(&repo_path_str).await.unwrap();
        git_ops
            .create_initial_commit(&repo_path_str, "main")
            .await
            .unwrap();
        let v1_commit = git_ops
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
        // Feature branch diverges so the MR diff resolves.
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

        // The production pin: the v1 BLOB sha, resolved from the v1 commit's
        // tree, paired with the LEDGER path (not the repo tree path).
        let blob_sha = {
            let out = std::process::Command::new("git")
                .args(["rev-parse", &format!("{v1_commit}:specs/system/widget.md")])
                .current_dir(&repo_path)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "git rev-parse failed: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        };
        assert_eq!(blob_sha.len(), 40, "blob sha must be a full OID");

        let repo_id = Id::new(Uuid::new_v4().to_string());
        let repo = gyre_domain::Repository::new(
            repo_id.clone(),
            Id::new("ws-test"),
            "blobref-repo",
            &repo_path_str,
            now_secs(),
        );
        state.repos.create(&repo).await.unwrap();

        let persona = gyre_domain::Persona::new(
            Id::new(Uuid::new_v4().to_string()),
            "Test persona test-reviewer".to_string(),
            "test-reviewer".to_string(),
            gyre_domain::PersonaScope::Workspace(Id::new("ws-test")),
            "You are the test-reviewer reviewer.".to_string(),
            now_secs(),
        );
        state.personas.create(&persona).await.unwrap();

        let mr_id = make_mr_id();
        let mut mr = gyre_domain::MergeRequest::new(
            mr_id.clone(),
            repo_id.clone(),
            "Add feature",
            "feature-branch",
            "main",
            now_secs(),
        );
        // Production shape (spawn.rs): ledger path + ledger blob sha.
        mr.spec_ref = Some(format!("system/widget.md@{blob_sha}"));
        mr.workspace_id = Id::new("ws-test");
        state.merge_requests.create(&mr).await.unwrap();

        let mut gate = make_gate(GateType::AgentReview, Some("true".to_string()));
        gate.persona = Some("personas/test-reviewer.md".to_string());
        gate.repo_id = repo_id;

        let ctx = build_review_agent_context(&state, &gate, &mr_id, "personas/test-reviewer.md")
            .await
            .expect("context gathering must resolve the blob-pinned spec_ref");
        assert_eq!(
            ctx.spec_content.as_deref(),
            Some("# Spec v1"),
            "blob-pinned spec_ref must deliver the PINNED blob content, not the tip"
        );
        assert_eq!(ctx.spec_ref, format!("system/widget.md@{blob_sha}"));
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

        let (status, output) = run_agent_review_gate(&state, &gate, &mr_id, None).await;

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

        let (status, output) = run_agent_review_gate(&state, &gate, &mr_id, None).await;

        assert_eq!(status, GateStatus::Failed, "output: {output}");
        assert!(output.contains("not found"), "output: {output}");
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

        let (status, output) = run_agent_review_gate(&state, &gate, &mr_id, None).await;

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

        let (status, output) = run_agent_review_gate(&state, &gate, &mr_id, None).await;

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

        let (status, output) = run_agent_review_gate(&state, &gate, &mr_id, None).await;

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

        let (status, output) = run_agent_review_gate(&state, &gate, &mr_id, None).await;

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
        assert!(output.contains("requested changes"), "output: {output}");
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

        let (status, output) =
            check_review_verdict(&state, &gate, &mr_id, "gate-review-verdict-3", "p", "").await;

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

    /// `validation_type` from the gate config must reach the spawned
    /// validation agent (`GYRE_VALIDATION_TYPE`) and the gate output — the
    /// implementation plan's AgentValidation config carrier.
    #[tokio::test]
    async fn agent_validation_type_reaches_agent_and_output() {
        let state = test_state();
        // `printenv GYRE_VALIDATION_TYPE` exits 0 and echoes the env var:
        // proves both delivery to the process and surfacing in the output.
        let mut gate = make_gate(
            GateType::AgentValidation,
            Some("printenv GYRE_VALIDATION_TYPE".to_string()),
        );
        gate.validation_type = Some("license-scan".to_string());
        let mr_id = make_mr_id();

        let (status, output) = run_agent_validation_gate(&state, &gate, &mr_id).await;

        assert_eq!(status, GateStatus::Passed, "output: {output}");
        assert!(
            output.contains("license-scan"),
            "validation_type not delivered/surfaced: {output}"
        );
        // Attribution: the gate output names the configured check even
        // when the agent prints nothing.
        assert!(
            output.contains("validation_type=license-scan"),
            "validation_type not in gate output header: {output}"
        );
    }

    /// A validation agent that reads `GYRE_VALIDATION_TYPE` as empty (gate
    /// with no validation_type configured) must see an empty string, not a
    /// stale value from a prior run — and the gate still attributes the
    /// check as `<none>`.
    #[tokio::test]
    async fn agent_validation_type_absent_defaults_to_none() {
        let state = test_state();
        let gate = make_gate(GateType::AgentValidation, Some("true".to_string()));
        assert!(gate.validation_type.is_none());
        let mr_id = make_mr_id();

        let (status, output) = run_agent_validation_gate(&state, &gate, &mr_id).await;

        assert_eq!(status, GateStatus::Passed, "output: {output}");
        assert!(
            output.contains("validation_type=<none>"),
            "missing validation_type must be attributed as <none>: {output}"
        );
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

    /// Review F2: a validation agent's token must carry
    /// `validation:report`, NOT `review:submit` — a validator whose
    /// contract is exit-code pass/fail must not hold review-submission
    /// capability. Proves the production mint site: the gate runs a real
    /// `printenv GYRE_VALIDATION_TOKEN` process, its output surfaces the
    /// minted token, and the JWT claims are validated against the state's
    /// signing key (issuer = base_url).
    #[tokio::test]
    async fn agent_validation_token_carries_validation_scope_not_review() {
        let state = test_state();
        let gate = make_gate(
            GateType::AgentValidation,
            Some("printenv GYRE_VALIDATION_TOKEN".to_string()),
        );
        let mr_id = make_mr_id();

        let (status, output) = run_agent_validation_gate(&state, &gate, &mr_id).await;

        assert_eq!(status, GateStatus::Passed, "output: {output}");
        let token = output
            .lines()
            .find_map(|l| l.trim().strip_prefix("ey"))
            .map(|rest| format!("ey{rest}"))
            .unwrap_or_else(|| panic!("no JWT in gate output: {output}"));
        let claims = state
            .agent_signing_key
            .validate(&token, &state.base_url)
            .expect("gate-agent JWT must validate against the server signing key");
        assert_eq!(
            claims.scope,
            crate::auth::VALIDATION_REPORT_SCOPE,
            "validator token must carry validation:report, not review:submit"
        );
        assert_ne!(
            claims.scope,
            crate::auth::REVIEW_SUBMIT_SCOPE,
            "validator must not hold review-submission capability"
        );
    }

    #[tokio::test]
    async fn gate_token_revoked_after_review_completes() {
        let state = test_state();
        let (mr_id, _repo_id) = seed_mr_with_repo_and_persona(&state, "test-reviewer").await;
        let mut gate = make_gate(GateType::AgentReview, Some("true".to_string()));
        gate.persona = Some("personas/test-reviewer.md".to_string());

        run_agent_review_gate(&state, &gate, &mr_id, None).await;

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

    /// Review-token TTL must cover the gate's CONFIGURED timeout, not the
    /// default: a review agent that submits its verdict late in a
    /// `timeout_secs > 360` window authenticates with an expired JWT
    /// otherwise (auth rejects it with 401) and the gate falsely fails
    /// with "exited without submitting a review". Regression test for the
    /// fixed-TTL bug: asserts the minted token's lifetime is at least the
    /// configured window + the 60s submission grace.
    #[tokio::test]
    async fn review_token_ttl_covers_configured_gate_timeout() {
        let state = test_state();
        let (mr_id, _repo_id) = seed_mr_with_repo_and_persona(&state, "slow-reviewer").await;
        let mut gate = make_gate(GateType::AgentReview, None);
        gate.persona = Some("personas/slow-reviewer.md".to_string());
        gate.timeout_secs = Some(700); // > the old fixed 360s token TTL

        let ctx =
            build_review_agent_context(&state, &gate, &mr_id, gate.persona.as_deref().unwrap())
                .await
                .expect("review context must build");

        let claims = state
            .agent_signing_key
            .validate(&ctx.token, &state.base_url)
            .expect("minted review JWT must validate");
        let ttl = claims.exp - claims.iat;
        assert!(
            ttl >= 700 + 60,
            "review token TTL {ttl}s must cover configured timeout_secs=700 + 60s grace; \
             a shorter token discards a verdict submitted within the window"
        );
    }

    /// Same regression on the validation mint site: the validator's scoped
    /// token must live at least as long as the configured gate window.
    /// Mirrors `agent_validation_token_carries_validation_scope_not_review`:
    /// the gate runs a real `printenv GYRE_VALIDATION_TOKEN` process so the
    /// minted token is captured from the production spawn path.
    #[tokio::test]
    async fn validation_token_ttl_covers_configured_gate_timeout() {
        let state = test_state();
        let mut gate = make_gate(
            GateType::AgentValidation,
            Some("printenv GYRE_VALIDATION_TOKEN".to_string()),
        );
        gate.timeout_secs = Some(700); // > the old fixed 360s token TTL
        let mr_id = make_mr_id();

        let (status, output) = run_agent_validation_gate(&state, &gate, &mr_id).await;

        assert_eq!(status, GateStatus::Passed, "output: {output}");
        let token = output
            .lines()
            .find_map(|l| l.trim().strip_prefix("ey"))
            .map(|rest| format!("ey{rest}"))
            .unwrap_or_else(|| panic!("no JWT in gate output: {output}"));
        let claims = state
            .agent_signing_key
            .validate(&token, &state.base_url)
            .expect("gate-agent JWT must validate against the server signing key");
        let ttl = claims.exp - claims.iat;
        assert!(
            ttl >= 700 + 60,
            "validation token TTL {ttl}s must cover configured timeout_secs=700 + 60s grace; \
             a shorter token expires mid-window"
        );
    }

    // ── AgentReview end-to-end protocol (real server, real HTTP, real JWT) ──

    /// True when this environment permits real-server loopback HTTP tests
    /// (bind + accept + client connect). Some CI/sandbox profiles deny
    /// `accept()` on listening sockets (errno 95), which makes every
    /// real-server test — this suite and `api_integration` alike — fail
    /// with connection-reset/000 regardless of code correctness. The e2e
    /// tests assert real protocol behavior and must NOT be weakened to
    /// in-process stubs, so they skip (with a clear message) instead.
    /// Detection is behavioral: bind, connect, accept, exchange bytes.
    async fn loopback_http_works() -> bool {
        let listener = match tokio::net::TcpListener::bind("127.0.0.1:0").await {
            Ok(l) => l,
            Err(_) => return false,
        };
        let addr = listener.local_addr().unwrap();
        let client = tokio::net::TcpStream::connect(addr).await;
        if client.is_err() {
            return false;
        }
        matches!(
            tokio::time::timeout(std::time::Duration::from_secs(2), listener.accept()).await,
            Ok(Ok(_))
        )
    }
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
        if !loopback_http_works().await {
            eprintln!("SKIP: sandbox denies loopback accept(); real-server e2e requires it");
            return;
        }
        // Bind first so the state's base_url (JWT issuer + agent's server
        // URL) points at the live server from the start.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let base_url = format!("http://127.0.0.1:{port}");

        let state = crate::build_state("gyre-test-token", &base_url, None);
        // Seed builtin ABAC policies: build_state wires an empty policy
        // store, and evaluation with zero policies is default-deny, so the
        // driver's review POST would 403. main.rs seeds at startup; a test
        // building its own state must do the same (review F1).
        crate::abac_middleware::seed_builtin_policies(&state)
            .await
            .unwrap();
        let (mr_id, _repo_id, spec_sha) = seed_full_review_fixture(&state).await;

        let app = crate::build_router(state.clone());
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        let driver = review_driver_path();
        let mut gate = make_gate(GateType::AgentReview, Some(format!("{driver} approved")));
        gate.persona = Some("personas/test-reviewer.md".to_string());

        let (status, output) = run_agent_review_gate(&state, &gate, &mr_id, None).await;

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
        if !loopback_http_works().await {
            eprintln!("SKIP: sandbox denies loopback accept(); real-server e2e requires it");
            return;
        }
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let base_url = format!("http://127.0.0.1:{port}");

        let state = crate::build_state("gyre-test-token", &base_url, None);
        // Same as the approved-path test: seed builtin policies or the
        // driver's review POST default-denies (review F1).
        crate::abac_middleware::seed_builtin_policies(&state)
            .await
            .unwrap();
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

        let (status, output) = run_agent_review_gate(&state, &gate, &mr_id, None).await;

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

    /// Review F1 regression guard, runnable without loopback: replicates the
    /// e2e tests' exact setup (build_state + seeded fixture + router from
    /// `crate::build_router`) via a oneshot request and proves the driver's
    /// review submission is ABAC-allowed (201), not default-denied (403).
    /// The e2e tests skip in no-loopback sandboxes; this one always runs,
    /// so a future removal of `seed_builtin_policies` from their setup
    /// fails here first.
    #[tokio::test]
    async fn e2e_review_submission_is_abac_allowed_with_seeded_policies() {
        use tower::ServiceExt; // oneshot

        let state = test_state();
        crate::abac_middleware::seed_builtin_policies(&state)
            .await
            .unwrap();
        let (mr_id, _repo_id, _spec_sha) = seed_full_review_fixture(&state).await;
        let app = crate::build_router(state.clone());

        // Mint and register a reviewer token exactly as the gate does.
        let gate_agent_id = format!("gate-review-{}", Uuid::new_v4());
        let token = state
            .agent_signing_key
            .mint_scoped(
                &gate_agent_id,
                "gate-probe",
                "forge",
                &state.base_url,
                60,
                crate::auth::REVIEW_SUBMIT_SCOPE,
            )
            .unwrap();
        state
            .kv_store
            .kv_set("agent_tokens", &gate_agent_id, token.clone())
            .await
            .unwrap();

        // The driver's exact request shape (forged reviewer id included).
        let body = serde_json::json!({
            "reviewer_agent_id": "forged-id-attempt",
            "decision": "approved",
            "body": "LGTM: satisfies the acceptance criteria."
        });
        let resp = app
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/merge-requests/{}/reviews", mr_id.as_str()))
                    .header("authorization", format!("Bearer {token}"))
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        let status = resp.status();
        let bytes = axum::body::to_bytes(resp.into_body(), 8192).await.unwrap();
        let text = String::from_utf8_lossy(&bytes);
        assert_eq!(
            status,
            axum::http::StatusCode::CREATED,
            "e2e review submission must be allowed once builtin policies are seeded; \
             403 means the e2e setup lost its seed_builtin_policies call (review F1): {text}"
        );
        // Identity binding holds through the same path.
        let reviews = state.reviews.list_reviews(&mr_id).await.unwrap();
        assert_eq!(reviews.len(), 1, "reviews: {reviews:?}");
        assert_eq!(reviews[0].reviewer_agent_id, gate_agent_id);
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
                validation_type: None,
                required: false,
                gate_phase: Default::default(),
                timeout_secs: None,
                created_at: now_secs(),
                position: 0,
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

    // ── Manifest-driven gate selection (spec-registry.md §16, task-195) ─────

    const GATES_MANIFEST_YAML: &str = r#"
version: 1
specs:
  - path: system/identity-security.md
    title: Identity Security
    owner: user:jsell
    gates:
      - persona: security
      - persona: accountability
        min_attestation_level: 2
        stack_hash: sha256:stack-abc
  - path: system/no-gates.md
    title: No Gates
    owner: user:jsell
"#;

    fn parse_gates_manifest() -> crate::spec_registry::SpecManifest {
        crate::spec_registry::parse_manifest(GATES_MANIFEST_YAML)
            .expect("fixture manifest must parse")
    }

    #[test]
    fn manifest_gates_resolved_from_spec_ref() {
        let manifest = parse_gates_manifest();
        let gates = manifest_gates_for_mr(
            Some(&manifest),
            Some("specs/system/identity-security.md@0123456789abcdef0123456789abcdef01234567"),
        );
        assert_eq!(gates.len(), 2, "both manifest personas must be selected");
        assert_eq!(gates[0].persona, "security");
        assert_eq!(
            gates[0].min_attestation_level, 1,
            "min_attestation_level defaults to 1"
        );
        assert_eq!(gates[0].stack_hash, None);
        assert_eq!(gates[1].persona, "accountability");
        assert_eq!(gates[1].min_attestation_level, 2);
        assert_eq!(gates[1].stack_hash.as_deref(), Some("sha256:stack-abc"));
        assert_eq!(gates[0].spec_path, "system/identity-security.md");
    }

    #[test]
    fn manifest_gates_match_bare_path_spec_ref_without_sha() {
        // spawn.rs records bare paths (no @sha) for specs not yet in the
        // ledger — those still name a spec and must select its gates.
        let manifest = parse_gates_manifest();
        let gates = manifest_gates_for_mr(Some(&manifest), Some("system/identity-security.md"));
        assert_eq!(gates.len(), 2);
    }

    #[test]
    fn manifest_gates_empty_for_missing_inputs() {
        let manifest = parse_gates_manifest();
        // No manifest.
        assert!(
            manifest_gates_for_mr(None, Some("specs/system/identity-security.md@aaaa")).is_empty()
        );
        // No spec_ref.
        assert!(manifest_gates_for_mr(Some(&manifest), None).is_empty());
        // Path not in the manifest.
        assert!(
            manifest_gates_for_mr(Some(&manifest), Some("specs/system/unknown.md@aaaa")).is_empty()
        );
        // Entry declares no gates.
        assert!(
            manifest_gates_for_mr(Some(&manifest), Some("specs/system/no-gates.md@aaaa"))
                .is_empty()
        );
    }

    #[test]
    fn manifest_gates_dedupe_repeated_persona() {
        let yaml = r#"
version: 1
specs:
  - path: system/x.md
    title: X
    owner: user:jsell
    gates:
      - persona: security
      - persona: security
        min_attestation_level: 3
      - persona: accountability
"#;
        let manifest = crate::spec_registry::parse_manifest(yaml).unwrap();
        let gates = manifest_gates_for_mr(Some(&manifest), Some("system/x.md@deadbeef"));
        assert_eq!(gates.len(), 2, "repeated persona must not double-run");
        assert_eq!(gates[0].min_attestation_level, 1, "first declaration wins");
    }

    fn level3_workload(
        agent_id: &str,
        stack: &str,
    ) -> crate::workload_attestation::WorkloadAttestation {
        crate::workload_attestation::WorkloadAttestation {
            agent_id: agent_id.to_string(),
            pid: Some(123),
            hostname: "host".to_string(),
            compute_target: "container".to_string(),
            stack_fingerprint: stack.to_string(),
            attested_at: 1_700_000_000,
            alive: true,
            last_verified_at: 1_700_000_000,
            container_id: Some("ctr-1".to_string()),
            image_hash: Some("sha256:img-1".to_string()),
        }
    }

    async fn seed_agent(
        state: &Arc<AppState>,
        agent_id: &str,
        persona: &str,
        workload: Option<crate::workload_attestation::WorkloadAttestation>,
    ) {
        state
            .kv_store
            .kv_set("agent_personas", agent_id, persona.to_string())
            .await
            .unwrap();
        if let Some(w) = workload {
            state
                .kv_store
                .kv_set(
                    "workload_attestations",
                    agent_id,
                    serde_json::to_string(&w).unwrap(),
                )
                .await
                .unwrap();
        }
    }

    async fn seed_review(
        state: &Arc<AppState>,
        mr_id: &Id,
        reviewer: &str,
        decision: ReviewDecision,
    ) {
        let review = Review::new(
            Id::new(Uuid::new_v4().to_string()),
            mr_id.clone(),
            reviewer,
            decision,
            now_secs(),
        );
        state.reviews.submit_review(&review).await.unwrap();
    }

    /// Seed a review with an explicit `created_at` — reviews are append-only
    /// and listed chronologically, so verdict supersession tests need
    /// distinct timestamps.
    async fn seed_review_at(
        state: &Arc<AppState>,
        mr_id: &Id,
        reviewer: &str,
        decision: ReviewDecision,
        created_at: u64,
    ) {
        let review = Review::new(
            Id::new(Uuid::new_v4().to_string()),
            mr_id.clone(),
            reviewer,
            decision,
            created_at,
        );
        state.reviews.submit_review(&review).await.unwrap();
    }

    fn manifest_gate(persona: &str, timeout_secs: u64) -> QualityGate {
        QualityGate {
            id: Id::new(Uuid::new_v4().to_string()),
            repo_id: Id::new(Uuid::new_v4().to_string()),
            name: format!("manifest gate: {persona}"),
            gate_type: GateType::AgentReview,
            command: None,
            required_approvals: None,
            persona: Some(format!("personas/{persona}.md")),
            validation_type: None,
            required: true,
            gate_phase: Default::default(),
            position: 0,
            timeout_secs: Some(timeout_secs),
            created_at: now_secs(),
        }
    }

    fn constraints(min_level: u32, stack_hash: Option<&str>) -> GateConstraints {
        GateConstraints {
            min_attestation_level: min_level,
            stack_hash: stack_hash.map(|s| s.to_string()),
            gate_agent_id: None,
        }
    }

    #[tokio::test]
    async fn persona_gate_passes_with_qualifying_review() {
        let state = test_state();
        let mr_id = make_mr_id();
        seed_agent(
            &state,
            "agent-sec",
            "security",
            Some(level3_workload("agent-sec", "sha256:stack-abc")),
        )
        .await;
        seed_review(&state, &mr_id, "agent-sec", ReviewDecision::Approved).await;

        let gate = manifest_gate("security", 0);
        let (status, output) = run_agent_review_gate(
            &state,
            &gate,
            &mr_id,
            Some(&constraints(3, Some("sha256:stack-abc"))),
        )
        .await;

        assert_eq!(status, GateStatus::Passed, "output: {output}");
        assert!(output.contains("manifest gate"), "output: {output}");
        assert!(output.contains("agent-sec"), "output: {output}");
    }

    #[tokio::test]
    async fn persona_gate_fails_without_qualifying_review() {
        // No reviews at all: the persona-constrained path must FAIL, not
        // auto-approve — the legacy no-command stub would pass here, so this
        // test kills any regression back to the auto-pass stub.
        let state = test_state();
        let mr_id = make_mr_id();

        let gate = manifest_gate("security", 0);
        let (status, output) =
            run_agent_review_gate(&state, &gate, &mr_id, Some(&constraints(1, None))).await;

        assert_eq!(status, GateStatus::Failed, "output: {output}");
        assert!(output.contains("persona 'security'"), "output: {output}");
    }

    #[tokio::test]
    async fn persona_gate_fails_when_attestation_below_minimum() {
        let state = test_state();
        let mr_id = make_mr_id();
        // Agent runs the persona but has NO workload attestation → level 0.
        seed_agent(&state, "agent-sec", "security", None).await;
        seed_review(&state, &mr_id, "agent-sec", ReviewDecision::Approved).await;

        let gate = manifest_gate("security", 0);
        let (status, output) =
            run_agent_review_gate(&state, &gate, &mr_id, Some(&constraints(2, None))).await;

        assert_eq!(status, GateStatus::Failed, "output: {output}");
        assert!(
            output.contains("attestation level 0 < required 2"),
            "output: {output}"
        );
    }

    #[tokio::test]
    async fn persona_gate_fails_on_stack_hash_mismatch() {
        let state = test_state();
        let mr_id = make_mr_id();
        seed_agent(
            &state,
            "agent-sec",
            "security",
            Some(level3_workload("agent-sec", "sha256:other-stack")),
        )
        .await;
        seed_review(&state, &mr_id, "agent-sec", ReviewDecision::Approved).await;

        let gate = manifest_gate("security", 0);
        let (status, output) = run_agent_review_gate(
            &state,
            &gate,
            &mr_id,
            Some(&constraints(1, Some("sha256:pinned-stack"))),
        )
        .await;

        assert_eq!(status, GateStatus::Failed, "output: {output}");
        assert!(output.contains("stack hash mismatch"), "output: {output}");
    }

    #[tokio::test]
    async fn persona_gate_ignores_reviews_from_other_personas() {
        // An approval from an agent running a DIFFERENT persona does not
        // satisfy the gate — the persona constraint is actually enforced.
        let state = test_state();
        let mr_id = make_mr_id();
        seed_agent(
            &state,
            "agent-acc",
            "accountability",
            Some(level3_workload("agent-acc", "sha256:stack-abc")),
        )
        .await;
        seed_review(&state, &mr_id, "agent-acc", ReviewDecision::Approved).await;

        let gate = manifest_gate("security", 0);
        let (status, output) =
            run_agent_review_gate(&state, &gate, &mr_id, Some(&constraints(1, None))).await;

        assert_eq!(status, GateStatus::Failed, "output: {output}");
    }

    #[tokio::test]
    async fn persona_gate_rejects_changes_requested_fast() {
        let state = test_state();
        let mr_id = make_mr_id();
        seed_agent(
            &state,
            "agent-sec",
            "security",
            Some(level3_workload("agent-sec", "sha256:stack-abc")),
        )
        .await;
        seed_review(
            &state,
            &mr_id,
            "agent-sec",
            ReviewDecision::ChangesRequested,
        )
        .await;

        let gate = manifest_gate("security", 60);
        let (status, output) =
            run_agent_review_gate(&state, &gate, &mr_id, Some(&constraints(1, None))).await;

        assert_eq!(status, GateStatus::Failed, "output: {output}");
        assert!(output.contains("requested changes"), "output: {output}");
    }

    #[tokio::test]
    async fn persona_gate_later_approval_supersedes_stale_changes_requested() {
        // Review R1: reviews are append-only, and the normal review cycle is
        // request-changes -> author fixes -> same agent approves. The
        // reviewer's LATEST verdict is the one that counts — a stale
        // ChangesRequested must not permanently veto that agent's own later
        // qualifying Approved. (Fails against first-match-wins ordering.)
        let state = test_state();
        let mr_id = make_mr_id();
        seed_agent(
            &state,
            "agent-sec",
            "security",
            Some(level3_workload("agent-sec", "sha256:stack-abc")),
        )
        .await;
        seed_review_at(
            &state,
            &mr_id,
            "agent-sec",
            ReviewDecision::ChangesRequested,
            1_000,
        )
        .await;
        seed_review_at(&state, &mr_id, "agent-sec", ReviewDecision::Approved, 2_000).await;

        let gate = manifest_gate("security", 0);
        let (status, output) = run_agent_review_gate(
            &state,
            &gate,
            &mr_id,
            Some(&constraints(3, Some("sha256:stack-abc"))),
        )
        .await;

        assert_eq!(status, GateStatus::Passed, "output: {output}");
    }

    #[tokio::test]
    async fn persona_gate_changes_requested_after_approval_fails() {
        // The converse: an approval superseded by a LATER ChangesRequested
        // from the same reviewer does not pass — latest-verdict-wins holds in
        // both directions, so the gate cannot be satisfied by a withdrawn
        // approval.
        let state = test_state();
        let mr_id = make_mr_id();
        seed_agent(
            &state,
            "agent-sec",
            "security",
            Some(level3_workload("agent-sec", "sha256:stack-abc")),
        )
        .await;
        seed_review_at(&state, &mr_id, "agent-sec", ReviewDecision::Approved, 1_000).await;
        seed_review_at(
            &state,
            &mr_id,
            "agent-sec",
            ReviewDecision::ChangesRequested,
            2_000,
        )
        .await;

        let gate = manifest_gate("security", 60);
        let (status, output) =
            run_agent_review_gate(&state, &gate, &mr_id, Some(&constraints(1, None))).await;

        assert_eq!(status, GateStatus::Failed, "output: {output}");
        assert!(output.contains("requested changes"), "output: {output}");
    }

    #[tokio::test]
    async fn gate_agent_torn_down_after_gate_resolves() {
        // agent-gates.md §Gate Agent Lifecycle: the forge-spawned gate agent
        // is torn down once its gate resolves — token revoked (audited),
        // status Stopped, record kept for attribution. Exercised on the
        // passing path here; the failing paths share the same teardown.
        let state = test_state();
        let mr_id = make_mr_id();
        let mut gate_agent = gyre_domain::Agent::new(
            Id::new(Uuid::new_v4().to_string()),
            "gate-security-teardown",
            now_secs(),
        );
        gate_agent.workspace_id = Id::new("ws-gate-agent");
        gate_agent.transition_status(AgentStatus::Active).unwrap();
        state.agents.create(&gate_agent).await.unwrap();
        state
            .kv_store
            .kv_set(
                "agent_personas",
                gate_agent.id.as_str(),
                "security".to_string(),
            )
            .await
            .unwrap();
        state
            .kv_store
            .kv_set(
                "agent_tokens",
                gate_agent.id.as_str(),
                "tok-secret".to_string(),
            )
            .await
            .unwrap();
        state
            .kv_store
            .kv_set(
                "workload_attestations",
                gate_agent.id.as_str(),
                serde_json::to_string(&level3_workload(gate_agent.id.as_str(), "sha256:stack-abc"))
                    .unwrap(),
            )
            .await
            .unwrap();
        seed_review(
            &state,
            &mr_id,
            gate_agent.id.as_str(),
            ReviewDecision::Approved,
        )
        .await;

        let gate = manifest_gate("security", 0);
        let cons = GateConstraints {
            min_attestation_level: 3,
            stack_hash: Some("sha256:stack-abc".to_string()),
            gate_agent_id: Some(gate_agent.id.clone()),
        };
        let (status, output) = run_agent_review_gate(&state, &gate, &mr_id, Some(&cons)).await;
        assert_eq!(status, GateStatus::Passed, "output: {output}");

        let agent = state
            .agents
            .find_by_id(&gate_agent.id)
            .await
            .unwrap()
            .expect("gate agent record is kept for attribution");
        assert_eq!(
            agent.status,
            AgentStatus::Stopped,
            "gate agent must be torn down after its gate resolves"
        );
        let token = state
            .kv_store
            .kv_get("agent_tokens", gate_agent.id.as_str())
            .await
            .unwrap();
        assert!(token.is_none(), "gate agent token must be revoked");
    }

    /// Create a real git repo with `specs/manifest.yaml` (and the spec file)
    /// committed at HEAD, plus a registered Repository + MR carrying spec_ref.
    async fn setup_manifest_mr(
        state: &Arc<AppState>,
        repo_id: &str,
        mr_id: &str,
        spec_ref: Option<&str>,
    ) -> tempfile::TempDir {
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
        std::fs::create_dir_all(dir.path().join("specs/system")).unwrap();
        std::fs::write(dir.path().join("specs/manifest.yaml"), GATES_MANIFEST_YAML).unwrap();
        std::fs::write(
            dir.path().join("specs/system/identity-security.md"),
            "# Identity Security\n",
        )
        .unwrap();
        for args in [
            vec!["-C", &path, "add", "."],
            vec!["-C", &path, "commit", "-m", "c1"],
        ] {
            std::process::Command::new("git")
                .args(&args)
                .output()
                .unwrap();
        }

        // The workspace record the manifest-gate messages are scoped to.
        // emit_event resolves a server-originated message's tenant from the
        // workspace record and SKIPS persistence when it is missing (the
        // anti-forged-scope invariant — a fabricated tenant would re-target
        // delivery), so the fixture must create the workspace exactly as
        // production does when a repo lands in one. Without it the spawned
        // gate agent's ReviewRequest is never delivered.
        let ws = gyre_domain::Workspace::new(
            Id::new("ws-manifest-gates"),
            Id::new("tenant-manifest-gates"),
            "manifest-gates",
            "manifest-gates",
            now_secs(),
        );
        state.workspaces.create(&ws).await.unwrap();

        let repo = gyre_domain::Repository::new(
            Id::new(repo_id),
            ws.id.clone(),
            "manifest-repo",
            path,
            now_secs(),
        );
        state.repos.create(&repo).await.unwrap();
        let mut mr = gyre_domain::MergeRequest::new(
            Id::new(mr_id),
            repo.id.clone(),
            "feat",
            "feat/x",
            "main",
            now_secs(),
        );
        mr.spec_ref = spec_ref.map(|s| s.to_string());
        mr.workspace_id = repo.workspace_id.clone();
        state.merge_requests.create(&mr).await.unwrap();
        dir
    }

    #[tokio::test]
    async fn trigger_gates_runs_manifest_gates_for_spec_ref_mr() {
        let state = test_state();
        let _dir = setup_manifest_mr(
            &state,
            "repo-mg-1",
            "mr-mg-1",
            Some("specs/system/identity-security.md@0123456789abcdef0123456789abcdef01234567"),
        )
        .await;

        trigger_gates_for_mr(state.clone(), Id::new("mr-mg-1"), Id::new("repo-mg-1")).await;

        let results = state.gate_results.list_by_mr_id("mr-mg-1").await.unwrap();
        let manifest_results: Vec<&str> = results
            .iter()
            .map(|r| r.gate_id.as_str())
            .filter(|g| g.starts_with("manifest-gate:"))
            .collect();
        assert_eq!(
            manifest_results.len(),
            2,
            "both manifest personas must produce GateResult rows: {results:?}"
        );
        assert!(
            manifest_results.iter().any(|g| g.ends_with(":security")),
            "results: {results:?}"
        );
        assert!(
            manifest_results
                .iter()
                .any(|g| g.ends_with(":accountability")),
            "results: {results:?}"
        );

        // Merge-blocking participation: the manifest gate results are visible
        // to check_gates_for_mr as required (unknown gate ids default to
        // required=true) — the MR is not mergeable while they run.
        let check = check_gates_for_mr(&state, &Id::new("mr-mg-1")).await;
        assert_eq!(
            check,
            Ok(false),
            "pending/running manifest gate results must hold the MR in the queue"
        );
    }

    #[tokio::test]
    async fn trigger_gates_failed_manifest_gate_blocks_merge() {
        let state = test_state();
        // A failed manifest-gate result must block the merge through the same
        // check_gates_for_mr path the merge processor uses.
        let mr_id = Id::new("mr-mg-block");
        state
            .gate_results
            .save(&GateResult {
                id: Id::new(Uuid::new_v4().to_string()),
                gate_id: Id::new("manifest-gate:repo-x:system/identity-security.md:security"),
                mr_id: mr_id.clone(),
                status: GateStatus::Failed,
                output: Some("no qualifying review".to_string()),
                started_at: None,
                finished_at: None,
            })
            .await
            .unwrap();

        let check = check_gates_for_mr(&state, &mr_id).await;
        assert!(check.is_err(), "failed manifest gate must block the merge");
        assert!(
            check.unwrap_err().contains("manifest-gate:"),
            "the blocking gate must be the manifest gate"
        );
    }

    #[tokio::test]
    async fn trigger_gates_manifest_gate_wins_over_repo_gate_for_same_persona() {
        // Review R2: when a repo-level gate and a manifest gate target the
        // same persona, the MANIFEST gate wins — the manifest is the
        // reviewed per-spec policy artifact (spec-registry.md §16) and
        // carries the min_attestation_level / stack_hash constraints the
        // reviewer must satisfy, which a repo-level gate cannot enforce.
        // The repo-gate-wins alternative here would not silently merge:
        // this repo-level gate has no command, so it fails closed (cannot
        // spawn a reviewer) and would BLOCK the merge — but a command-based
        // repo gate would run an unconstrained reviewer. Manifest-wins runs
        // the persona review under the manifest's constraints instead.
        let state = test_state();
        let _dir = setup_manifest_mr(
            &state,
            "repo-mg-2",
            "mr-mg-2",
            Some("specs/system/identity-security.md@0123456789abcdef0123456789abcdef01234567"),
        )
        .await;

        // Repo-level gate already covering the security persona, with NO
        // command — under repo-gate-wins this configuration fails closed
        // (it cannot spawn a reviewer), not auto-passes; manifest-wins
        // replaces it with the constrained persona review.
        let repo_gate = QualityGate {
            id: Id::new("repo-gate-security"),
            repo_id: Id::new("repo-mg-2"),
            name: "repo security review".to_string(),
            gate_type: GateType::AgentReview,
            command: None,
            required_approvals: None,
            persona: Some("personas/security.md".to_string()),
            validation_type: None,
            required: true,
            gate_phase: Default::default(),
            position: 0,
            timeout_secs: None,
            created_at: now_secs(),
        };
        state.quality_gates.save(&repo_gate).await.unwrap();

        trigger_gates_for_mr(state.clone(), Id::new("mr-mg-2"), Id::new("repo-mg-2")).await;

        let results = state.gate_results.list_by_mr_id("mr-mg-2").await.unwrap();
        let security_results: Vec<&GateResult> = results
            .iter()
            .filter(|r| {
                let g = r.gate_id.as_str();
                g == "repo-gate-security" || g.ends_with(":security")
            })
            .collect();
        assert_eq!(
            security_results.len(),
            1,
            "the security persona must run exactly once (no double-spawn): {results:?}"
        );
        assert_eq!(
            security_results[0].gate_id.as_str(),
            "manifest-gate:repo-mg-2:system/identity-security.md:security",
            "the manifest gate wins over the repo-level gate for the same persona — \
             its attestation constraints are the ones that must be enforced"
        );
        assert!(!results
            .iter()
            .any(|r| r.gate_id.as_str() == "repo-gate-security"));
        assert_eq!(
            results.len(),
            2,
            "exactly the manifest security gate + the accountability manifest gate: {results:?}"
        );
        assert!(
            results
                .iter()
                .any(|r| r.gate_id.as_str().ends_with(":accountability")),
            "the uncovered manifest persona still runs: {results:?}"
        );
    }

    #[tokio::test]
    async fn trigger_gates_spawns_gate_agents_for_manifest_gates() {
        // spec-registry.md §16: "Forge spawns security gate agent and
        // accountability gate agent for this MR" — trigger must create real
        // Agent records carrying the manifest persona (the production writer
        // for the agent_personas namespace), register scoped tokens, and
        // dispatch a ReviewRequest each gate agent runtime polls for.
        let state = test_state();
        let _dir = setup_manifest_mr(
            &state,
            "repo-mg-5",
            "mr-mg-5",
            Some("specs/system/identity-security.md@0123456789abcdef0123456789abcdef01234567"),
        )
        .await;

        trigger_gates_for_mr(state.clone(), Id::new("mr-mg-5"), Id::new("repo-mg-5")).await;

        for persona in ["security", "accountability"] {
            let agent = state
                .agents
                .find_by_name(&format!("gate-{persona}-mr-mg-5"))
                .await
                .unwrap()
                .unwrap_or_else(|| panic!("gate agent for {persona} must be spawned"));
            assert_eq!(agent.status, AgentStatus::Active);
            assert_eq!(agent.workspace_id, Id::new("ws-manifest-gates"));
            assert_eq!(agent.spawned_by.as_deref(), Some("server"));
            let recorded_persona = state
                .kv_store
                .kv_get("agent_personas", agent.id.as_str())
                .await
                .unwrap();
            assert_eq!(
                recorded_persona.as_deref(),
                Some(persona),
                "the gate agent's persona must be recorded in agent_personas"
            );
            let token = state
                .kv_store
                .kv_get("agent_tokens", agent.id.as_str())
                .await
                .unwrap();
            assert!(token.is_some(), "gate agent must have a scoped token");

            // The ReviewRequest the agent runtime polls for (MCP
            // gyre_message_poll / GET messages) — the forge's instruction to
            // review this MR as this persona.
            let msgs = state.messages.list_unacked(&agent.id, 10).await.unwrap();
            let review_req = msgs
                .iter()
                .find(|m| m.kind == gyre_common::message::MessageKind::ReviewRequest);
            let review_req = review_req.unwrap_or_else(|| {
                panic!("ReviewRequest must be dispatched to the {persona} gate agent")
            });
            let payload = review_req
                .payload
                .as_ref()
                .expect("ReviewRequest payload must carry the review context");
            assert_eq!(payload["mr_id"].as_str(), Some("mr-mg-5"));
            assert_eq!(payload["persona"].as_str(), Some(persona));
            assert_eq!(
                payload["min_attestation_level"].as_u64(),
                Some(if persona == "security" { 1 } else { 2 }),
                "the manifest attestation constraints must reach the gate agent"
            );
        }
    }

    #[tokio::test]
    async fn trigger_gates_launches_gate_agent_runtime_with_token() {
        // Review findings (attempt 94b30b1d): the forge-spawned gate agent
        // must actually EXECUTE — a real process launched through the
        // platform's compute-target machinery, receiving its scoped token
        // via GYRE_AUTH_TOKEN (the credential channel every spawned agent
        // uses), with a workload attestation recorded so its own reviews can
        // satisfy the gate's min_attestation_level. The runtime command here
        // is a stand-in that dumps its environment; what is asserted is the
        // launch contract, not the runtime's review behavior.
        let state = test_state();
        let _dir = setup_manifest_mr(
            &state,
            "repo-mg-7",
            "mr-mg-7",
            Some("specs/system/identity-security.md@0123456789abcdef0123456789abcdef01234567"),
        )
        .await;

        // Stand in for the platform agent runtime (/gyre/entrypoint.sh):
        // dump the launched environment, keyed per agent id so concurrent
        // gate agents cannot clobber each other's dumps.
        let dump_dir = tempfile::tempdir().unwrap();
        let script = dump_dir.path().join("runtime.sh");
        let dump_root = dump_dir.path().to_str().unwrap().to_string();
        std::fs::write(
            &script,
            format!("#!/bin/sh\nenv > '{dump_root}/env-'\"$GYRE_AGENT_ID\"\n"),
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        std::env::set_var("GYRE_AGENT_COMMAND", &script);

        trigger_gates_for_mr(state.clone(), Id::new("mr-mg-7"), Id::new("repo-mg-7")).await;
        std::env::remove_var("GYRE_AGENT_COMMAND");

        let agent = state
            .agents
            .find_by_name("gate-security-mr-mg-7")
            .await
            .unwrap()
            .expect("gate agent for security must be spawned");
        let token = state
            .kv_store
            .kv_get("agent_tokens", agent.id.as_str())
            .await
            .unwrap()
            .expect("gate agent token must be registered");

        // The launched runtime received the token and its review context.
        // The dump appears asynchronously — the launch returns as soon as
        // the process is forked — so poll briefly for it.
        let dump_path = dump_dir.path().join(format!("env-{}", agent.id.as_str()));
        let mut dump = None;
        for _ in 0..100 {
            if let Ok(d) = std::fs::read_to_string(&dump_path) {
                // The shell truncates the file before writing it — wait for
                // actual content, not mere existence.
                if d.contains("GYRE_AUTH_TOKEN=") {
                    dump = Some(d);
                    break;
                }
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        let dump = dump.unwrap_or_else(|| {
            panic!(
                "launched runtime must dump its env to {}",
                dump_path.display()
            )
        });
        assert!(
            dump.contains(&format!("GYRE_AUTH_TOKEN={token}")),
            "the launched runtime must receive the gate agent's scoped token: {dump}"
        );
        assert!(dump.contains("GYRE_PERSONA=security"), "dump: {dump}");
        assert!(dump.contains("GYRE_MR_ID=mr-mg-7"), "dump: {dump}");
        assert!(
            dump.contains(
                "GYRE_GATE_ID=manifest-gate:repo-mg-7:system/identity-security.md:security"
            ),
            "dump: {dump}"
        );
        assert!(
            dump.contains("GYRE_MIN_ATTESTATION_LEVEL=1"),
            "dump: {dump}"
        );

        // The launch records the gate agent's workload attestation —
        // without it the agent's own reviews would derive at level 0 and
        // could never satisfy even the default min_attestation_level of 1.
        let att_json = state
            .kv_store
            .kv_get("workload_attestations", agent.id.as_str())
            .await
            .unwrap()
            .expect("the gate agent launch must record a workload attestation");
        let att: crate::workload_attestation::WorkloadAttestation =
            serde_json::from_str(&att_json).unwrap();
        assert!(
            att.pid.is_some(),
            "the launched process's PID must be attested: {att:?}"
        );
        assert!(
            crate::constraint_check::derive_attestation_level(Some(&att)) >= 1,
            "the gate agent's own reviews must be able to satisfy the default \
             min_attestation_level of 1: {att:?}"
        );
    }

    #[tokio::test]
    async fn gate_agent_runtime_process_killed_on_teardown() {
        // agent-gates.md §Gate Agent Lifecycle: teardown terminates the gate
        // agent's runtime process (via its process-registry handle), not
        // just the record — an ephemeral agent must not leave its runtime
        // running after its gate resolved.
        let state = test_state();
        let agent_id = Id::new(Uuid::new_v4().to_string());
        let mut agent = gyre_domain::Agent::new(agent_id.clone(), "gate-security-kill", now_secs());
        agent.workspace_id = Id::new("ws-gate-agent");
        agent.transition_status(AgentStatus::Active).unwrap();
        state.agents.create(&agent).await.unwrap();
        state
            .kv_store
            .kv_set("agent_tokens", agent.id.as_str(), "tok-secret".to_string())
            .await
            .unwrap();

        // A real long-running process standing in for the launched runtime,
        // registered under the agent's id exactly as the launch path does.
        let handle = gyre_ports::ComputeTarget::spawn_process(
            &gyre_adapters::compute::LocalTarget,
            &gyre_ports::SpawnConfig {
                name: "gate-runtime".to_string(),
                command: "sleep".to_string(),
                args: vec!["60".to_string()],
                env: std::collections::HashMap::new(),
                work_dir: "/tmp".to_string(),
            },
        )
        .await
        .unwrap();
        state
            .process_registry
            .lock()
            .await
            .insert(agent.id.to_string(), handle.clone());

        teardown_gate_agent(&state, &agent.id).await;

        let agent = state
            .agents
            .find_by_id(&agent_id)
            .await
            .unwrap()
            .expect("gate agent record is kept for attribution");
        assert_eq!(agent.status, AgentStatus::Stopped);
        assert!(!state
            .process_registry
            .lock()
            .await
            .contains_key(agent_id.as_str()));
        // Give the OS a moment to reap the SIGTERMed process.
        tokio::time::sleep(Duration::from_millis(200)).await;
        let alive =
            gyre_ports::ComputeTarget::is_alive(&gyre_adapters::compute::LocalTarget, &handle)
                .await
                .unwrap();
        assert!(
            !alive,
            "the gate agent's runtime process must be killed on teardown"
        );
    }

    #[tokio::test]
    async fn gate_agent_token_is_scoped_reviewer_jwt() {
        // Review repair: the gate agent's token must carry
        // `review:submit` — submit_review binds the reviewer to the verified
        // token subject ONLY for that scope, so an ordinary "agent"-scoped
        // token would leave the manifest gate's persona/attestation
        // constraints evaluated against an unverified request-body identity.
        // Fails if the mint reverts to AgentSigningKey::mint (scope "agent").
        let state = test_state();
        let _dir = setup_manifest_mr(
            &state,
            "repo-mg-tok",
            "mr-mg-tok",
            Some("specs/system/identity-security.md@0123456789abcdef0123456789abcdef01234567"),
        )
        .await;

        trigger_gates_for_mr(state.clone(), Id::new("mr-mg-tok"), Id::new("repo-mg-tok")).await;

        let agent = state
            .agents
            .find_by_name("gate-security-mr-mg-tok")
            .await
            .unwrap()
            .expect("gate agent for security must be spawned");
        let token = state
            .kv_store
            .kv_get("agent_tokens", agent.id.as_str())
            .await
            .unwrap()
            .expect("gate agent token must be registered");
        let claims = state
            .agent_signing_key
            .validate(&token, &state.base_url)
            .expect("gate agent token must be a server-minted JWT");
        assert_eq!(
            claims.sub,
            agent.id.as_str(),
            "the token subject must be the gate agent itself"
        );
        assert_eq!(
            claims.scope,
            crate::auth::REVIEW_SUBMIT_SCOPE,
            "the gate agent must authenticate with a review:submit-scoped token \
             so submit_review binds its reviewer identity"
        );
        assert_eq!(
            claims.task_id, "manifest-gate:repo-mg-tok:system/identity-security.md:security",
            "the token must be traceable to the gate run that minted it"
        );
    }

    #[tokio::test]
    async fn gate_agent_teardown_routes_kill_through_registered_backend() {
        // Review repair: teardown must kill the runtime via the backend that
        // owns the handle (state.spawned_backends), the same invariant the
        // admin kill path implements — NOT via LocalTarget directly. A
        // container-backend handle routed to LocalTarget is a silent no-op
        // (containers carry pid: None) and an SshBare pid is a REMOTE pid.
        //
        // Fixture: a real local process registered with a Container backend,
        // whose kill routes to `docker rm --force <handle.id>` (a nonexistent
        // container) — that can never terminate the local process. If
        // teardown regressed to a LocalTarget kill, the SIGTERM would kill it
        // and this test fails.
        let state = test_state();
        let agent_id = Id::new(Uuid::new_v4().to_string());
        let mut agent =
            gyre_domain::Agent::new(agent_id.clone(), "gate-security-backend", now_secs());
        agent.workspace_id = Id::new("ws-gate-agent");
        agent.transition_status(AgentStatus::Active).unwrap();
        state.agents.create(&agent).await.unwrap();
        state
            .kv_store
            .kv_set("agent_tokens", agent.id.as_str(), "tok-secret".to_string())
            .await
            .unwrap();

        // A real long-running process standing in for the launched runtime.
        let handle = gyre_ports::ComputeTarget::spawn_process(
            &gyre_adapters::compute::LocalTarget,
            &gyre_ports::SpawnConfig {
                name: "gate-runtime-backend".to_string(),
                command: "sleep".to_string(),
                args: vec!["60".to_string()],
                env: std::collections::HashMap::new(),
                work_dir: "/tmp".to_string(),
            },
        )
        .await
        .unwrap();
        // Register handle + owning backend exactly as the launch path does
        // (register_spawned_agent) — but with a Container backend so the
        // kill must route through it, not through a local pid kill.
        state
            .process_registry
            .lock()
            .await
            .insert(agent.id.to_string(), handle.clone());
        state.spawned_backends.lock().await.insert(
            agent.id.to_string(),
            std::sync::Arc::new(crate::api::spawn::SpawnBackend::Container(
                gyre_adapters::compute::ContainerTarget::new("gyre-agent:test"),
            )),
        );

        teardown_gate_agent(&state, &agent.id).await;

        // No map leak: both registry entries the launch registered are gone.
        assert!(!state
            .process_registry
            .lock()
            .await
            .contains_key(agent_id.as_str()));
        assert!(!state
            .spawned_backends
            .lock()
            .await
            .contains_key(agent_id.as_str()));

        // The kill went to the owning backend: `docker rm --force` on a
        // nonexistent container id cannot terminate the real local process,
        // so it must still be alive. (A LocalTarget kill would have
        // SIGTERMed it.)
        tokio::time::sleep(Duration::from_millis(300)).await;
        let alive =
            gyre_ports::ComputeTarget::is_alive(&gyre_adapters::compute::LocalTarget, &handle)
                .await
                .unwrap();
        assert!(
            alive,
            "teardown must route the kill through the registered backend, not LocalTarget"
        );
    }

    #[tokio::test]
    async fn trigger_gates_reuses_running_gate_agent_on_reenqueue() {
        // Re-enqueue while the gate is still running must not double-spawn
        // the persona's gate agent — the running agent is reused.
        let state = test_state();
        let _dir = setup_manifest_mr(
            &state,
            "repo-mg-6",
            "mr-mg-6",
            Some("specs/system/identity-security.md@0123456789abcdef0123456789abcdef01234567"),
        )
        .await;

        trigger_gates_for_mr(state.clone(), Id::new("mr-mg-6"), Id::new("repo-mg-6")).await;
        trigger_gates_for_mr(state.clone(), Id::new("mr-mg-6"), Id::new("repo-mg-6")).await;

        let agents = state
            .agents
            .list_by_workspace(&Id::new("ws-manifest-gates"))
            .await
            .unwrap();
        let security_agents: Vec<_> = agents
            .iter()
            .filter(|a| a.name.starts_with("gate-security-mr-mg-6"))
            .collect();
        assert_eq!(
            security_agents.len(),
            1,
            "re-enqueue must reuse the running gate agent, not double-spawn: {agents:?}"
        );
    }

    #[tokio::test]
    async fn trigger_gates_no_spec_ref_runs_repo_gates_only() {
        // MR without spec_ref → exactly the repo-level gates it runs today.
        let state = test_state();
        let _dir = setup_manifest_mr(&state, "repo-mg-3", "mr-mg-3", None).await;

        let repo_gate = QualityGate {
            id: Id::new("repo-gate-tests"),
            repo_id: Id::new("repo-mg-3"),
            name: "unit tests".to_string(),
            gate_type: GateType::TestCommand,
            command: Some("true".to_string()),
            required_approvals: None,
            persona: None,
            validation_type: None,
            required: true,
            gate_phase: Default::default(),
            position: 0,
            timeout_secs: None,
            created_at: now_secs(),
        };
        state.quality_gates.save(&repo_gate).await.unwrap();

        trigger_gates_for_mr(state.clone(), Id::new("mr-mg-3"), Id::new("repo-mg-3")).await;

        let results = state.gate_results.list_by_mr_id("mr-mg-3").await.unwrap();
        assert_eq!(results.len(), 1, "no spurious gates: {results:?}");
        assert_eq!(results[0].gate_id.as_str(), "repo-gate-tests");
    }

    #[tokio::test]
    async fn trigger_gates_repo_without_manifest_runs_repo_gates_only() {
        // MR carries a spec_ref, but the repo has no specs/manifest.yaml at
        // HEAD → repo-level gates only, unchanged behavior.
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
        std::fs::write(dir.path().join("README.md"), "x").unwrap();
        for args in [
            vec!["-C", &path, "add", "."],
            vec!["-C", &path, "commit", "-m", "c1"],
        ] {
            std::process::Command::new("git")
                .args(&args)
                .output()
                .unwrap();
        }

        let repo = gyre_domain::Repository::new(
            Id::new("repo-mg-4"),
            Id::new("ws-manifest-gates"),
            "no-manifest-repo",
            path,
            now_secs(),
        );
        state.repos.create(&repo).await.unwrap();
        let mut mr = gyre_domain::MergeRequest::new(
            Id::new("mr-mg-4"),
            repo.id.clone(),
            "feat",
            "feat/y",
            "main",
            now_secs(),
        );
        mr.spec_ref = Some(
            "specs/system/identity-security.md@0123456789abcdef0123456789abcdef01234567"
                .to_string(),
        );
        state.merge_requests.create(&mr).await.unwrap();

        let repo_gate = QualityGate {
            id: Id::new("repo-gate-lint"),
            repo_id: Id::new("repo-mg-4"),
            name: "lint".to_string(),
            gate_type: GateType::LintCommand,
            command: Some("true".to_string()),
            required_approvals: None,
            persona: None,
            validation_type: None,
            required: true,
            gate_phase: Default::default(),
            position: 0,
            timeout_secs: None,
            created_at: now_secs(),
        };
        state.quality_gates.save(&repo_gate).await.unwrap();

        trigger_gates_for_mr(state.clone(), Id::new("mr-mg-4"), Id::new("repo-mg-4")).await;

        let results = state.gate_results.list_by_mr_id("mr-mg-4").await.unwrap();
        assert_eq!(results.len(), 1, "no spurious gates: {results:?}");
        assert_eq!(results[0].gate_id.as_str(), "repo-gate-lint");
    }

    // ─── Auto-emitted analytics events (analytics.md §Auto-Emitted Events) ────

    async fn analytics_events(
        state: &Arc<crate::AppState>,
        name: &str,
    ) -> Vec<gyre_domain::AnalyticsEvent> {
        // state.analytics is an Arc<dyn AnalyticsRepository>; method calls
        // resolve through the trait object without importing the trait here.
        state.analytics.query(Some(name), None, 10).await.unwrap()
    }

    #[tokio::test]
    async fn passing_gate_emits_gate_passed_analytics_event() {
        // gate.passed must carry gate_id, gate_type, mr_id, duration_secs.
        let state = crate::mem::test_state();
        let gate = make_gate(GateType::TestCommand, Some("true".to_string()));
        let mr_id = make_mr_id();
        let result_id = Id::new(Uuid::new_v4().to_string());

        // Store a pending result first (run_gate updates it).
        state
            .gate_results
            .save(&GateResult {
                id: result_id.clone(),
                gate_id: gate.id.clone(),
                mr_id: mr_id.clone(),
                status: GateStatus::Pending,
                output: None,
                started_at: None,
                finished_at: None,
            })
            .await
            .unwrap();

        run_gate(state.clone(), result_id, gate.clone(), mr_id.clone()).await;

        let events = analytics_events(&state, "gate.passed").await;
        assert_eq!(events.len(), 1, "one gate.passed event expected");
        let ev = &events[0];
        assert_eq!(ev.properties["gate_id"], gate.id.to_string());
        assert_eq!(ev.properties["gate_type"], "TestCommand");
        assert_eq!(ev.properties["mr_id"], mr_id.to_string());
        assert!(
            ev.properties["duration_secs"].as_u64().is_some(),
            "duration_secs must be present"
        );
    }

    #[tokio::test]
    async fn failing_gate_emits_gate_failed_analytics_event() {
        // gate.failed must carry gate_id, gate_type, mr_id, output_snippet.
        let state = crate::mem::test_state();
        let gate = make_gate(GateType::TestCommand, Some("false".to_string()));
        let mr_id = make_mr_id();
        let result_id = Id::new(Uuid::new_v4().to_string());

        state
            .gate_results
            .save(&GateResult {
                id: result_id.clone(),
                gate_id: gate.id.clone(),
                mr_id: mr_id.clone(),
                status: GateStatus::Pending,
                output: None,
                started_at: None,
                finished_at: None,
            })
            .await
            .unwrap();

        run_gate(state.clone(), result_id, gate.clone(), mr_id.clone()).await;

        let events = analytics_events(&state, "gate.failed").await;
        assert_eq!(events.len(), 1, "one gate.failed event expected");
        let ev = &events[0];
        assert_eq!(ev.properties["gate_id"], gate.id.to_string());
        assert_eq!(ev.properties["gate_type"], "TestCommand");
        assert_eq!(ev.properties["mr_id"], mr_id.to_string());
        assert!(
            ev.properties["output_snippet"].as_str().is_some(),
            "output_snippet must be present (failure output)"
        );
        // No gate.passed event for a failing gate.
        let passed = analytics_events(&state, "gate.passed").await;
        assert!(passed.is_empty(), "failed gate must not emit gate.passed");
    }
}
