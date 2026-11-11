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
        required: true,
        gate_phase: Default::default(),
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

    // Scoped token: a JWT minted by the server's OIDC provider and
    // registered in agent_tokens so the gate agent runtime can authenticate
    // (same registration spawn_agent_core uses). Falls back to an opaque
    // token when minting fails — the KV registration is what authenticates.
    let token = state
        .agent_signing_key
        .mint(
            agent.id.as_str(),
            "",
            "server",
            &state.base_url,
            GATE_AGENT_TOKEN_TTL_SECS,
        )
        .unwrap_or_else(|e| {
            warn!("gate agent JWT mint failed, falling back to UUID token: {e}");
            format!("gyre_gate_{}", Uuid::new_v4().simple())
        });
    if let Err(e) = state
        .kv_store
        .kv_set("agent_tokens", agent.id.as_str(), token.clone())
        .await
    {
        warn!(
            agent_id = %agent.id,
            error = format!("{e:#}"),
            "manifest gate: failed to register gate agent token"
        );
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
/// submitting its verdict"). Kills its runtime process if one is still
/// registered (the admin kill path's pattern), revokes its token (audited
/// via AgentTokenRevoked) and transitions it to Stopped — the record is kept
/// so its reviews stay attributed.
async fn teardown_gate_agent(state: &Arc<AppState>, agent_id: &Id) {
    let Ok(Some(mut agent)) = state.agents.find_by_id(agent_id).await else {
        return;
    };
    // Kill the runtime process if it is still running (best-effort — the
    // handle may already be gone if the process exited on its own).
    if let Some(handle) = state
        .process_registry
        .lock()
        .await
        .remove(agent_id.as_str())
    {
        if let Err(e) =
            gyre_ports::ComputeTarget::kill_process(&gyre_adapters::compute::LocalTarget, &handle)
                .await
        {
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
    // MANIFEST gate wins: it carries the min_attestation_level / stack_hash
    // constraints the manifest declares for this spec, which the repo-level
    // gate cannot enforce — skipping the manifest gate instead would let a
    // no-command repo-level AgentReview gate auto-pass and merge without
    // them. The manifest gate is not a downgrade of a working repo-level
    // command gate: it spawns a real gate agent runtime (launched through
    // the same compute-target machinery `spawn_agent_core` uses, with a
    // workload attestation — see `launch_gate_agent_runtime`), so the
    // persona actually reviews either way. The repo-level duplicate for
    // that persona is skipped, so the persona still runs exactly once.
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
        // A missing MR means the workspace scope cannot be determined — skip
        // the workspace-scoped event and log rather than fabricating an
        // identity (task-097 F3; see emit_reconciliation_completed).
        if let Some(ws_id) = workspace_id {
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
        } else {
            warn!(
                mr_id = %mr_id,
                gate_id = %gate.id,
                "skipping GateFailure event: MR workspace could not be resolved"
            );
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

/// Run an AgentReview gate.
///
/// If the gate has a `command` configured, spawns that command as a subprocess
/// with MR context injected via environment variables. The subprocess is
/// expected to submit a review via the API using GYRE_REVIEW_TOKEN, then exit.
/// Gate passes if an Approved review is found after the process completes.
///
/// If the gate carries manifest-declared attestation constraints
/// (spec-registry.md §16), runs the persona-constrained review path, which
/// never auto-passes: it requires an Approved review from an agent running
/// the named persona whose workload attestation satisfies the constraints.
///
/// Otherwise (no command, no constraints) falls back to checking existing
/// reviews and auto-approving if none are found (legacy repo-level behavior,
/// useful for testing without a real agent).
async fn run_agent_review_gate(
    state: &Arc<AppState>,
    gate: &gyre_domain::QualityGate,
    mr_id: &Id,
    constraints: Option<&GateConstraints>,
) -> (GateStatus, String) {
    let persona = gate.persona.as_deref().unwrap_or("personas/default.md");

    if let Some(cmd) = &gate.command {
        run_review_agent_process(state, gate, mr_id, cmd, persona).await
    } else if let Some(constraints) = constraints {
        run_persona_review_gate(state, gate, mr_id, persona, constraints).await
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
    let _ = state
        .kv_store
        .kv_set("agent_tokens", &gate_agent_id, gate_token.clone())
        .await;

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
                let (status, output) =
                    run_command_in_dir(gate.command.as_deref().unwrap_or("true"), cwd, timeout)
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
            gate_phase: Default::default(),
            timeout_secs: None,
            created_at: now_secs(),
        }
    }

    fn make_mr_id() -> Id {
        Id::new(Uuid::new_v4().to_string())
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

    // ── AgentReview stub path (no command) ──────────────────────────────────

    #[tokio::test]
    async fn agent_review_stub_no_existing_reviews_auto_approves() {
        let state = test_state();
        let gate = make_gate(GateType::AgentReview, None);
        let mr_id = make_mr_id();

        let (status, output) = run_agent_review_gate(&state, &gate, &mr_id, None).await;

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

        let (status, output) = run_agent_review_gate(&state, &gate, &mr_id, None).await;

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
        let gate = make_gate(
            GateType::AgentReview,
            Some("/nonexistent/review-agent".to_string()),
        );
        let mr_id = make_mr_id();

        let (status, output) = run_agent_review_gate(&state, &gate, &mr_id, None).await;

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
        let gate = make_gate(GateType::AgentReview, Some("true".to_string()));
        let mr_id = make_mr_id();

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
            required: true,
            gate_phase: Default::default(),
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

        let repo = gyre_domain::Repository::new(
            Id::new(repo_id),
            Id::new("ws-manifest-gates"),
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
        // same persona, the MANIFEST gate wins — it carries the
        // min_attestation_level / stack_hash constraints the manifest
        // declares, which the repo-level gate cannot enforce. Skipping the
        // manifest gate instead would let this no-command repo-level
        // AgentReview gate auto-pass and merge without the constraints.
        let state = test_state();
        let _dir = setup_manifest_mr(
            &state,
            "repo-mg-2",
            "mr-mg-2",
            Some("specs/system/identity-security.md@0123456789abcdef0123456789abcdef01234567"),
        )
        .await;

        // Repo-level gate already covering the security persona, with NO
        // command — the legacy configuration whose stub auto-passes.
        let repo_gate = QualityGate {
            id: Id::new("repo-gate-security"),
            repo_id: Id::new("repo-mg-2"),
            name: "repo security review".to_string(),
            gate_type: GateType::AgentReview,
            command: None,
            required_approvals: None,
            persona: Some("personas/security.md".to_string()),
            required: true,
            gate_phase: Default::default(),
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
            required: true,
            gate_phase: Default::default(),
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
            required: true,
            gate_phase: Default::default(),
            timeout_secs: None,
            created_at: now_secs(),
        };
        state.quality_gates.save(&repo_gate).await.unwrap();

        trigger_gates_for_mr(state.clone(), Id::new("mr-mg-4"), Id::new("repo-mg-4")).await;

        let results = state.gate_results.list_by_mr_id("mr-mg-4").await.unwrap();
        assert_eq!(results.len(), 1, "no spurious gates: {results:?}");
        assert_eq!(results[0].gate_id.as_str(), "repo-gate-lint");
    }
}
