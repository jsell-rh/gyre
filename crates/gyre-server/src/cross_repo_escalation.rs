//! Cross-repo spec escalation protocol (platform-model.md §3).
//!
//! When a repo orchestrator needs something from another repo it escalates
//! to the workspace orchestrator with a structured
//! [`EscalationPayload`](gyre_common::EscalationPayload) on the existing
//! `MessageKind::Escalation` Directed-tier message:
//!
//! - `ImplementationNeeded` — the target repo's spec already describes the
//!   artifact but it is not implemented yet. The workspace orchestrator
//!   creates a cross-repo implementation task in the target repo, records
//!   the task dependency from the requesting task, links the MR dependency
//!   when both MRs are known, and notifies the target repo's orchestrator
//!   (spawning one on demand when none is live — the same routing as the
//!   SpecApproved signal chain).
//!
//! - `SpecChangeNeeded` — the target repo's spec does not describe the
//!   artifact. The workspace orchestrator drafts the spec change in the
//!   target repo (a `spec-edit/*` branch + MR carrying the drafted spec
//!   content, mirroring the specs/save flow), records the MR dependency
//!   from the requesting MR onto the spec MR, and fans out priority-2
//!   `SpecApprovalRequested` notifications with the approve/reject inline
//!   action body. **The human gate:** a cross-repo spec change only lands
//!   when a human approves it; approval fires `SpecApproved`, whose signal
//!   chain auto-creates the implementation task in the target repo.
//!
//! Entry point: [`escalate`] — invoked by the MCP tool
//! `gyre_escalate_cross_repo` (repo-orchestrator tier). The message is
//! delivered to the workspace orchestrator's inbox (persisted, ack-based —
//! pollable via `message.poll`), then processed server-side under the
//! per-workspace orchestrator lock, mirroring the SpecApproved
//! interception's "drive the orchestrator's processing server-side" shape.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use gyre_common::message::{Destination, Message, MessageKind, MessageOrigin};
use gyre_common::{EscalationPayload, Id, NotificationType};
use gyre_domain::{
    Agent, DependencySource, MergeRequest, MergeRequestDependency, OrchestratorType, Repository,
    Task, TaskPriority, TaskType, WorkspaceRole,
};
use serde_json::json;

use crate::AppState;

/// What the escalation run produced — surfaced to the escalating caller
/// (MCP tool result) and used by tests to pin the protocol's effects.
#[derive(Debug, Clone, Default)]
pub struct EscalationOutcome {
    /// The bus message id of the delivered Escalation.
    pub message_id: String,
    /// ImplementationNeeded: the cross-repo task created in the target repo.
    pub created_task_id: Option<String>,
    /// SpecChangeNeeded: the spec-edit MR created in the target repo.
    pub created_mr_id: Option<String>,
    /// ImplementationNeeded: the target repo's orchestrator that was
    /// notified (spawned on demand when none was live).
    pub notified_repo_orchestrator_id: Option<String>,
}

/// Deliver the escalation to the workspace orchestrator and process it.
///
/// `caller_agent_id` is the escalating repo orchestrator's agent id (the
/// message origin); `caller_tenant_id` is the caller's resolved tenant
/// scope (the message's tenant — derived from the authenticated caller,
/// never fabricated here).
pub async fn escalate(
    state: &Arc<AppState>,
    caller_agent_id: &Id,
    caller_tenant_id: &str,
    payload: &EscalationPayload,
) -> Result<EscalationOutcome> {
    // Containment before any side effect: both repos must exist and live
    // in the same workspace (the escalation is workspace-scoped — the
    // workspace orchestrator's scope IS the boundary).
    let source_repo = load_repo(state, &payload.source_repo_id).await?;
    let target_repo = load_repo(state, &payload.target_repo_id).await?;
    let workspace_id = source_repo.workspace_id.clone();
    if target_repo.workspace_id != workspace_id {
        return Err(anyhow!(
            "cross-repo escalation requires both repos in the same workspace: {} lives in {}, {} in {}",
            payload.source_repo_id,
            workspace_id,
            payload.target_repo_id,
            target_repo.workspace_id
        ));
    }
    if source_repo.id == target_repo.id {
        return Err(anyhow!(
            "cross-repo escalation requires distinct source and target repos"
        ));
    }

    // Serialize per workspace (same "exactly-one-active" registry mutex the
    // SpecApproved chain holds).
    let lock = state
        .orchestrator_registry
        .workspace_lock(&workspace_id)
        .await;
    let _guard = lock.lock().await;

    // Ensure a live workspace orchestrator exists (spawn on demand — the
    // same task-093 core the SpecApproved chain uses, with the workspace's
    // REAL tenant resolved from the workspace record).
    let orchestrator =
        crate::signal_chain::ensure_workspace_orchestrator(state, &workspace_id).await?;

    // Deliver the Directed-tier Escalation message to the orchestrator's
    // inbox: signed with the server key, persisted (ack-based), broadcast
    // to WS clients, dispatched to bus consumers — the same send path as
    // the MCP `message.send` tool. The message stays pollable via
    // `message.poll` until the processing below acks it.
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    let mut msg = Message {
        id: Id::new(uuid::Uuid::new_v4().to_string()),
        tenant_id: Id::new(caller_tenant_id.to_string()),
        from: MessageOrigin::Agent(caller_agent_id.clone()),
        workspace_id: Some(workspace_id.clone()),
        to: Destination::Agent(orchestrator.id.clone()),
        kind: MessageKind::Escalation,
        payload: Some(payload.to_json()),
        created_at: now_ms,
        signature: None,
        key_id: None,
        acknowledged: false,
    };
    let (sig, kid) = crate::signing::sign_message(state, &msg);
    msg.signature = Some(sig);
    msg.key_id = Some(kid);
    if let Err(e) = state.messages.store(&msg).await {
        return Err(anyhow!("failed to store escalation message: {e}"));
    }
    let _ = state.message_broadcast_tx.send(msg.clone());
    if let Err(tokio::sync::mpsc::error::TrySendError::Full(_)) =
        state.message_dispatch_tx.try_send(msg.clone())
    {
        tracing::warn!(
            "escalation: message dispatch channel full; bus consumers dropped the message (best-effort tier)"
        );
    }

    // Drive the orchestrator's processing server-side (it "completes after
    // processing its inbox" — the same shape as the SpecApproved chain).
    let mut outcome = process_escalation(state, &orchestrator, &msg).await?;
    outcome.message_id = msg.id.to_string();
    Ok(outcome)
}

/// One workspace-orchestrator escalation run: dispatch on the escalation
/// type, then ack the consumed escalation message (targeted ack — other
/// inbox messages stay unacked for their real consumers).
pub async fn process_escalation(
    state: &Arc<AppState>,
    orchestrator: &Agent,
    message: &Message,
) -> Result<EscalationOutcome> {
    let payload = message
        .payload
        .as_ref()
        .and_then(EscalationPayload::from_json)
        .ok_or_else(|| {
            anyhow!(
                "escalation message {} carries no parseable EscalationPayload",
                message.id
            )
        })?;

    // Containment: the orchestrator only acts inside its own workspace —
    // both repos must resolve to real rows in that workspace.
    let workspace_id = orchestrator.workspace_id.clone();
    let source_repo = load_repo_in_workspace(state, &payload.source_repo_id, &workspace_id).await?;
    let target_repo = load_repo_in_workspace(state, &payload.target_repo_id, &workspace_id).await?;
    if source_repo.id == target_repo.id {
        return Err(anyhow!(
            "cross-repo escalation requires distinct source and target repos"
        ));
    }

    let mut outcome = match payload.escalation_type {
        gyre_common::EscalationType::ImplementationNeeded => {
            handle_implementation_needed(state, orchestrator, &payload, &source_repo, &target_repo)
                .await?
        }
        gyre_common::EscalationType::SpecChangeNeeded => {
            handle_spec_change_needed(state, orchestrator, &payload, &source_repo, &target_repo)
                .await?
        }
    };

    // Acknowledge the consumed escalation (ack-based Directed delivery).
    if let Err(e) = state
        .messages
        .acknowledge(&message.id, &orchestrator.id)
        .await
    {
        tracing::warn!(
            message_id = %message.id,
            "escalation: failed to acknowledge consumed Escalation message: {e}"
        );
    }
    outcome.message_id = message.id.to_string();
    Ok(outcome)
}

// ── ImplementationNeeded branch ─────────────────────────────────────────────

/// Repo B's spec describes the artifact but it is not implemented yet:
/// create the cross-repo task in Repo B, record the task dependency,
/// link the MR dependency when both MRs are known, and notify Repo B's
/// orchestrator.
async fn handle_implementation_needed(
    state: &Arc<AppState>,
    _orchestrator: &Agent,
    payload: &EscalationPayload,
    source_repo: &Repository,
    target_repo: &Repository,
) -> Result<EscalationOutcome> {
    // 1. Cross-repo implementation task in the target repo (real task row,
    //    scoped to the target repo's repo+workspace).
    let now = crate::api::now_secs();
    let mut task = Task::new(
        crate::api::new_id(),
        format!(
            "Implement {} (cross-repo request from {})",
            payload.needed_artifact, source_repo.name
        ),
        now,
    );
    let mut description = format!(
        "Cross-repo escalation (platform-model.md §3): repo {} needs `{}`. Reason: {}.",
        payload.source_repo_id, payload.needed_artifact, payload.reason
    );
    if let Some(task_id) = &payload.task_id {
        description.push_str(&format!(" Requested by task {task_id}."));
    }
    description.push_str(
        " The target repo's spec already describes this artifact; implement it so the \
         requesting repo's work can proceed.",
    );
    task.description = Some(description);
    task.task_type = Some(TaskType::Implementation);
    task.priority = TaskPriority::High;
    task.labels = vec!["cross-repo".to_string(), "escalation".to_string()];
    task.repo_id = target_repo.id.clone();
    task.workspace_id = target_repo.workspace_id.clone();
    state
        .tasks
        .create(&task)
        .await
        .context("escalation: create cross-repo task")?;

    // 2. Task dependency: the requesting task depends on the new task.
    if let Some(task_id) = &payload.task_id {
        let tid = Id::new(task_id);
        match state.tasks.find_by_id(&tid).await {
            Ok(Some(mut src_task)) => {
                // The requesting task must live in the source repo (the
                // escalation's own containment — task id is caller input).
                if src_task.repo_id == source_repo.id && !src_task.depends_on.contains(&task.id) {
                    src_task.depends_on.push(task.id.clone());
                    state
                        .tasks
                        .update(&src_task)
                        .await
                        .context("escalation: link requesting task dependency")?;
                } else if src_task.repo_id != source_repo.id {
                    tracing::warn!(
                        task_id = %tid,
                        "escalation: requesting task is not in the source repo; task dependency not recorded"
                    );
                }
            }
            Ok(None) => {
                tracing::warn!(
                    task_id = %tid,
                    "escalation: requesting task not found; task dependency not recorded"
                );
            }
            Err(e) => {
                tracing::warn!(task_id = %tid, "escalation: task lookup failed: {e}");
            }
        }
    }

    // 3. MR dependency when both ends are known (the requesting MR waits on
    //    the target repo's MR that implements the artifact).
    if let (Some(source_mr_id), Some(target_mr_id)) = (&payload.source_mr_id, &payload.target_mr_id)
    {
        if let Err(e) = add_mr_dependency(
            state,
            source_mr_id,
            source_repo,
            target_mr_id,
            target_repo,
            &format!(
                "cross-repo escalation: {} needs `{}`",
                source_repo.name, payload.needed_artifact
            ),
        )
        .await
        {
            tracing::warn!("escalation: MR dependency not recorded: {e:#}");
        }
    }

    // 4. Notify the target repo's orchestrator (spawn on demand when none
    //    is live — the workspace orchestrator's job per §3). The
    //    TaskAssignment Directed message lands in its inbox, pollable via
    //    message.poll.
    let repo_orchestrator = ensure_repo_orchestrator(state, target_repo).await?;
    state
        .emit_event(
            target_repo.workspace_id.clone(),
            Destination::Agent(repo_orchestrator.id.clone()),
            MessageKind::TaskAssignment,
            Some(json!({
                "task_id": task.id.to_string(),
                "reason": payload.reason,
                "source_repo_id": payload.source_repo_id,
                "needed_artifact": payload.needed_artifact,
            })),
        )
        .await;

    Ok(EscalationOutcome {
        created_task_id: Some(task.id.to_string()),
        notified_repo_orchestrator_id: Some(repo_orchestrator.id.to_string()),
        ..Default::default()
    })
}

// ── SpecChangeNeeded branch ─────────────────────────────────────────────────

/// Repo B's spec does not describe the artifact: draft the spec change in
/// Repo B (spec-edit branch + MR carrying the drafted content), link the
/// MR dependency from the requesting MR onto the spec MR, and fan out the
/// priority-2 human approval notifications (the human gate).
async fn handle_spec_change_needed(
    state: &Arc<AppState>,
    orchestrator: &Agent,
    payload: &EscalationPayload,
    source_repo: &Repository,
    target_repo: &Repository,
) -> Result<EscalationOutcome> {
    // 1. Draft path + content.
    let spec_path = draft_spec_path(&payload.needed_artifact);
    let content = draft_spec_content(payload, source_repo, &spec_path);

    // 2. Spec-edit branch + MR in the target repo (the same flow shape as
    //    POST /repos/:id/specs/save: feature branch off the default branch,
    //    content committed there, MR targeting the default branch — the
    //    change only lands when the MR merges).
    let slug = crate::api::specs_assist::spec_path_slug(&spec_path);
    let branch_prefix = format!("spec-edit/{slug}");
    let short_uuid: String = uuid::Uuid::new_v4()
        .simple()
        .to_string()
        .chars()
        .take(8)
        .collect();
    let branch_name = format!("{branch_prefix}-{short_uuid}");
    state
        .git_ops
        .create_branch(&target_repo.path, &branch_name, &target_repo.default_branch)
        .await
        .map_err(|e| anyhow!("escalation: create spec-edit branch: {e:?}"))?;
    let commit_message = format!(
        "Draft spec change: {spec_path} (cross-repo escalation from {})",
        source_repo.name
    );
    state
        .git_ops
        .write_file(
            &target_repo.path,
            &branch_name,
            &spec_path,
            content.as_bytes(),
            &commit_message,
        )
        .await
        .map_err(|e| anyhow!("escalation: write drafted spec content: {e:?}"))?;

    let now = crate::api::now_secs();
    let mut mr = MergeRequest::new(
        crate::api::new_id(),
        target_repo.id.clone(),
        target_repo.workspace_id.clone(),
        format!("Spec change: {spec_path}"),
        branch_name.clone(),
        target_repo.default_branch.clone(),
        now,
    );
    mr.author_agent_id = Some(orchestrator.id.clone());
    state
        .merge_requests
        .create(&mr)
        .await
        .context("escalation: create spec-change MR")?;

    // 3. MR dependency: the requesting repo's MR waits on the spec MR.
    if let Some(source_mr_id) = &payload.source_mr_id {
        if let Err(e) = add_mr_dependency(
            state,
            source_mr_id,
            source_repo,
            mr.id.as_str(),
            target_repo,
            &format!(
                "cross-repo spec change: {} needs `{}`",
                source_repo.name, payload.needed_artifact
            ),
        )
        .await
        {
            tracing::warn!("escalation: MR dependency not recorded: {e:#}");
        }
    }

    // 4. Priority-2 human approval notifications (the human gate). The body
    //    carries the same inline-action fields the specs/save flow uses
    //    (spec_path/spec_sha/mr_id/branch/repo_id — the Inbox Approve
    //    action submits body.spec_sha) plus the cross-repo context. The
    //    spec_sha is the blob SHA of the drafted content on the branch —
    //    never "" (a fail-open that would push the approve action onto the
    //    legacy pre-edit fallback).
    let git_bin = std::env::var("GYRE_GIT_PATH").unwrap_or_else(|_| "git".to_string());
    let blob_sha = crate::api::specs_assist::spec_blob_sha(
        &git_bin,
        &target_repo.path,
        &branch_name,
        &spec_path,
        &content,
    )
    .await;
    let notif_body = json!({
        "spec_path": spec_path,
        "spec_sha": blob_sha,
        "mr_id": mr.id.to_string(),
        "mr_title": mr.title.clone(),
        "branch": branch_name.clone(),
        "repo_id": target_repo.id.to_string(),
        "escalation": "cross-repo-spec-change",
        "source_repo_id": payload.source_repo_id,
        "needed_artifact": payload.needed_artifact,
        "reason": payload.reason,
    })
    .to_string();

    // Tenant resolved from the workspace record — never fabricated. A
    // dangling workspace reference skips the fan-out (skip-and-log) rather
    // than notifying under a fabricated identity.
    let ws = state.workspaces.find_by_id(&target_repo.workspace_id).await;
    match ws {
        Ok(Some(ws_record)) => {
            let tenant_id = ws_record.tenant_id.to_string();
            // Fan out to every workspace Admin/Developer/Owner member — the
            // roles allowed to approve (the same recipient policy as the
            // specs/save flow). No eligible members → deliver to "system"
            // so the global-admin Inbox still shows the pending approval.
            let members = state
                .workspace_memberships
                .list_by_workspace(&target_repo.workspace_id)
                .await
                .unwrap_or_default();
            let mut recipients: Vec<Id> = members
                .iter()
                .filter(|m| {
                    matches!(
                        m.role,
                        WorkspaceRole::Admin | WorkspaceRole::Developer | WorkspaceRole::Owner
                    )
                })
                .map(|m| m.user_id.clone())
                .collect();
            if recipients.is_empty() {
                recipients.push(Id::new("system"));
            }
            for uid in recipients {
                crate::notifications::notify_rich(
                    state.as_ref(),
                    target_repo.workspace_id.clone(),
                    uid,
                    NotificationType::SpecApprovalRequested,
                    format!("Cross-repo spec change pending approval: {spec_path}"),
                    &tenant_id,
                    Some(notif_body.clone()),
                    Some(mr.id.to_string()),
                    Some(target_repo.id.to_string()),
                    "mr",
                    None,
                )
                .await;
            }
        }
        Ok(None) => {
            tracing::warn!(
                workspace_id = %target_repo.workspace_id,
                "escalation: target repo's workspace not found; human approval notifications skipped (tenant scope cannot be resolved)"
            );
        }
        Err(e) => {
            tracing::warn!(
                workspace_id = %target_repo.workspace_id,
                "escalation: workspace lookup failed; human approval notifications skipped: {e}"
            );
        }
    }

    Ok(EscalationOutcome {
        created_mr_id: Some(mr.id.to_string()),
        ..Default::default()
    })
}

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Poll-side receipt path (platform-model.md §3 item 3: the workspace
/// orchestrator receives the Escalation via `message.poll`): process any
/// UNACKED structured cross-repo escalation sitting in the workspace
/// orchestrator's inbox. This is the receipt/crash-recovery path for
/// escalations whose synchronous processing at delivery time never ran or
/// failed (e.g. a server restart between message store and processing) —
/// a healthy delivery is processed and acked exactly once by `escalate`.
///
/// Only messages whose payload parses as a cross-repo `EscalationPayload`
/// are processed: the generic escalations other server components deliver
/// to the orchestrator's inbox (stale-agent detector, spec-lifecycle
/// patrol, merge processor) carry their own `{reason, context}` payloads,
/// do not parse as cross-repo escalations, and stay unacked for their real
/// consumers. Errors are logged, never fail the poll.
pub async fn drive_pending_escalations(
    state: &Arc<AppState>,
    orchestrator_id: &Id,
    workspace_id: &Id,
) {
    // Identity check against real state, not just the token: the polling
    // agent must BE the live workspace orchestrator of that workspace.
    let orchestrator = match state.agents.find_by_id(orchestrator_id).await {
        Ok(Some(a)) => a,
        _ => return,
    };
    if orchestrator.orchestrator_type != OrchestratorType::WorkspaceOrchestrator
        || orchestrator.workspace_id != *workspace_id
    {
        return;
    }
    let unacked = match state.messages.list_unacked(orchestrator_id, 1000).await {
        Ok(u) => u,
        Err(e) => {
            tracing::warn!(
                orchestrator_id = %orchestrator_id,
                "escalation: could not list unacked inbox messages: {e}"
            );
            return;
        }
    };
    let pending: Vec<Message> = unacked
        .into_iter()
        .filter(|m| {
            m.kind == MessageKind::Escalation
                && m.payload
                    .as_ref()
                    .and_then(EscalationPayload::from_json)
                    .is_some()
        })
        .collect();
    if pending.is_empty() {
        return;
    }
    // Same per-workspace serialization as the delivery-time path.
    let lock = state
        .orchestrator_registry
        .workspace_lock(workspace_id)
        .await;
    let _guard = lock.lock().await;
    for msg in pending {
        if let Err(e) = process_escalation(state, &orchestrator, &msg).await {
            tracing::warn!(
                message_id = %msg.id,
                "escalation: poll-side processing failed (message stays unacked for retry): {e:#}"
            );
        }
    }
}

/// Load a repo by id (must exist).
async fn load_repo(state: &Arc<AppState>, repo_id: &str) -> Result<Repository> {
    state
        .repos
        .find_by_id(&Id::new(repo_id))
        .await
        .map_err(|e| anyhow!("repo lookup failed: {e}"))?
        .ok_or_else(|| anyhow!("repo {repo_id} not found"))
}

/// Load a repo by id and verify it lives in the given workspace
/// (containment — the escalation only acts inside the orchestrator's own
/// workspace).
async fn load_repo_in_workspace(
    state: &Arc<AppState>,
    repo_id: &str,
    workspace_id: &Id,
) -> Result<Repository> {
    let repo = load_repo(state, repo_id).await?;
    if repo.workspace_id != *workspace_id {
        return Err(anyhow!(
            "repo {repo_id} is not in workspace {workspace_id} — cross-repo escalation is workspace-scoped"
        ));
    }
    Ok(repo)
}

/// Find the live repo orchestrator for a repo, spawning one on demand via
/// the task-093 core when none is live (system principal; the REAL tenant
/// resolved from the workspace record — never fabricated).
async fn ensure_repo_orchestrator(state: &Arc<AppState>, repo: &Repository) -> Result<Agent> {
    if let Some(existing) = crate::signal_chain::find_live_repo_orchestrator(state, repo).await? {
        return Ok(existing);
    }
    let ws = state
        .workspaces
        .find_by_id(&repo.workspace_id)
        .await
        .map_err(|e| anyhow!("workspace lookup failed: {e}"))?
        .ok_or_else(|| {
            anyhow!(
                "workspace {} for repo {} not found — the tenant scope for the \
                 orchestrator token cannot be determined",
                repo.workspace_id,
                repo.id
            )
        })?;
    let (agent, _token, _launch) = crate::api::orchestrator::spawn_repo_orchestrator_core(
        state,
        repo.id.as_str(),
        crate::api::orchestrator::SpawnOrchestratorRequest::default(),
        "system",
        ws.tenant_id.as_str(),
    )
    .await
    .map_err(|e| anyhow!("repo orchestrator spawn: {e:?}"))?;
    tracing::info!(
        orchestrator_id = %agent.id,
        repo_id = %repo.id,
        "escalation: spawned repo orchestrator for cross-repo task notification"
    );
    Ok(agent)
}

/// Record `source_mr_id depends_on target_mr_id` (agent-declared dependency
/// with the escalation reason). Both MRs must exist and live in the
/// escalation's source/target repos — the dependency is cross-repo by
/// construction, and a fabricated link to a foreign repo's MR is refused.
/// Cycle-checked against the full MR graph so an escalation cannot create
/// a circular dependency chain.
async fn add_mr_dependency(
    state: &Arc<AppState>,
    source_mr_id: &str,
    source_repo: &Repository,
    target_mr_id: &str,
    target_repo: &Repository,
    reason: &str,
) -> Result<()> {
    let src_id = Id::new(source_mr_id);
    let tgt_id = Id::new(target_mr_id);
    let mut src_mr = state
        .merge_requests
        .find_by_id(&src_id)
        .await
        .map_err(|e| anyhow!("MR lookup failed: {e}"))?
        .ok_or_else(|| anyhow!("source MR {source_mr_id} not found"))?;
    let tgt_mr = state
        .merge_requests
        .find_by_id(&tgt_id)
        .await
        .map_err(|e| anyhow!("MR lookup failed: {e}"))?
        .ok_or_else(|| anyhow!("target MR {target_mr_id} not found"))?;
    if src_mr.repository_id != source_repo.id {
        return Err(anyhow!(
            "source MR {source_mr_id} is not in the escalation's source repo"
        ));
    }
    if tgt_mr.repository_id != target_repo.id {
        return Err(anyhow!(
            "target MR {target_mr_id} is not in the escalation's target repo"
        ));
    }
    if src_mr.depends_on.iter().any(|d| d.target_mr_id == tgt_id) {
        return Ok(()); // already linked
    }

    // Cycle check over the full MR graph (same helper the dependency API
    // and MR creation use).
    let all_mrs = state
        .merge_requests
        .list()
        .await
        .map_err(|e| anyhow!("MR listing failed: {e}"))?;
    let mut adj: HashMap<String, Vec<String>> = HashMap::new();
    for m in &all_mrs {
        adj.insert(
            m.id.to_string(),
            m.depends_on
                .iter()
                .map(|d| d.target_mr_id.to_string())
                .collect(),
        );
    }
    if crate::api::merge_deps::would_create_cycle(
        &src_id.to_string(),
        &[target_mr_id.to_string()],
        &adj,
    ) {
        return Err(anyhow!(
            "cycle detected: MR {source_mr_id} -> {target_mr_id} would create a circular dependency chain"
        ));
    }

    src_mr.depends_on.push(
        MergeRequestDependency::new(tgt_id, DependencySource::AgentDeclared).with_reason(reason),
    );
    src_mr.updated_at = crate::api::now_secs();
    state
        .merge_requests
        .update(&src_mr)
        .await
        .map_err(|e| anyhow!("MR update failed: {e}"))?;
    Ok(())
}

/// Resolve the drafted spec's path from the needed artifact: an explicit
/// `.md` path is used as-is (normalized under `specs/`), an interface name
/// is slugified under `specs/system/` (the convention the spec registry's
/// manifest paths follow).
fn draft_spec_path(needed_artifact: &str) -> String {
    let artifact = needed_artifact.trim().trim_matches('/');
    if artifact.ends_with(".md") {
        if artifact.starts_with("specs/") || artifact.contains('/') {
            artifact.to_string()
        } else {
            format!("specs/{artifact}")
        }
    } else {
        let mut kebab = String::new();
        let mut prev_dash = true; // suppress leading dashes
        for ch in artifact.chars() {
            if ch.is_ascii_alphanumeric() {
                kebab.push(ch.to_ascii_lowercase());
                prev_dash = false;
            } else if !prev_dash {
                kebab.push('-');
                prev_dash = true;
            }
        }
        while kebab.ends_with('-') {
            kebab.pop();
        }
        if kebab.is_empty() {
            "specs/system/cross-repo-artifact.md".to_string()
        } else {
            format!("specs/system/{kebab}.md")
        }
    }
}

/// The drafted spec content the human reviews: a structured draft carrying
/// the escalation's requirement (what is needed, why, from which repo) —
/// the workspace orchestrator's draft, subject to the human gate.
fn draft_spec_content(
    payload: &EscalationPayload,
    source_repo: &Repository,
    spec_path: &str,
) -> String {
    let mut content = format!(
        "# {artifact}\n\n\
         > Status: **draft** — cross-repo spec change (platform-model.md §3).\n\
         > Drafted by the workspace orchestrator from an escalation by repo `{source_repo}`.\n\
         > Cross-repo spec changes always require human approval before landing.\n\n\
         ## Requirement\n\n\
         Repo `{source_repo}` needs `{artifact}` from this repo.\n\n\
         Reason: {reason}\n",
        artifact = payload.needed_artifact,
        source_repo = source_repo.name,
        reason = payload.reason,
    );
    if let Some(task_id) = &payload.task_id {
        content.push_str(&format!("\nRequested by task `{task_id}`.\n"));
    }
    content.push_str(&format!(
        "\n## Scope\n\n\
         This spec (`{spec_path}`) must describe `{}` such that the requesting\n\
         repo's work can proceed. A human approver reviews this draft: approving it\n\
         merges the spec change and auto-creates the implementation task in this\n\
         repo; rejecting it notifies the workspace orchestrator to find an\n\
         alternative.\n",
        payload.needed_artifact
    ));
    content
}

#[cfg(test)]
mod tests {
    use super::*;
    use gyre_domain::{
        ApprovalStatus, OrchestratorType, PersonaApprovalStatus, PersonaScope, SpecLedgerEntry,
        Workspace,
    };

    /// Fixture mirroring the signal-chain fixture: production-parity
    /// seeding (built-in meta-specs + approved orchestrator personas), a
    /// workspace, and two repos in it.
    async fn escalation_state() -> Arc<AppState> {
        let state = crate::mem::test_state();
        crate::seed_builtin_meta_specs(&state).await;
        let ws = Workspace::new(Id::new("ws-1"), Id::new("t1"), "Ws", "ws", 0);
        state.workspaces.create(&ws).await.unwrap();
        for rid in ["repo-a", "repo-b"] {
            let repo = Repository::new(
                Id::new(rid),
                Id::new("ws-1"),
                rid,
                format!("/tmp/gyre-test-{rid}"),
                0,
            );
            state.repos.create(&repo).await.unwrap();
        }
        for slug in ["workspace-orchestrator", "repo-orchestrator"] {
            let mut persona = gyre_domain::Persona::new(
                Id::new(format!("{slug}-persona")),
                slug,
                slug,
                PersonaScope::Tenant(Id::new("t1")),
                "test orchestrator persona",
                0,
            );
            persona.approval_status = PersonaApprovalStatus::Approved;
            state.personas.create(&persona).await.unwrap();
        }
        // A spec ledger entry keeps persona/spec resolution on the path
        // production exercises (the spawn cores consult the ledger).
        let entry = SpecLedgerEntry {
            path: "specs/system/auth.md".to_string(),
            title: "Auth".to_string(),
            owner: "owner".to_string(),
            kind: None,
            current_sha: "abc123".to_string(),
            approval_mode: "single".to_string(),
            approval_status: ApprovalStatus::Pending,
            linked_tasks: vec![],
            linked_mrs: vec![],
            drift_status: "clean".to_string(),
            created_at: 0,
            updated_at: 0,
            repo_id: Some("repo-a".to_string()),
            workspace_id: Some("ws-1".to_string()),
        };
        state.spec_ledger.save(&entry).await.unwrap();
        state
    }

    fn impl_payload() -> EscalationPayload {
        EscalationPayload {
            source_repo_id: "repo-a".to_string(),
            target_repo_id: "repo-b".to_string(),
            needed_artifact: "UserPort".to_string(),
            reason: "TASK-042 needs UserPort from repo-b".to_string(),
            task_id: Some("task-042".to_string()),
            source_mr_id: None,
            target_mr_id: None,
            escalation_type: gyre_common::EscalationType::ImplementationNeeded,
        }
    }

    fn spec_change_payload() -> EscalationPayload {
        EscalationPayload {
            escalation_type: gyre_common::EscalationType::SpecChangeNeeded,
            ..impl_payload()
        }
    }

    fn now_secs() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }

    // The escalation protocol end-to-end (ImplementationNeeded): the
    // Directed Escalation lands in the workspace orchestrator's inbox, a
    // cross-repo implementation task is created in the TARGET repo, the
    // requesting task depends on it, Repo B's orchestrator is notified via
    // a TaskAssignment in its inbox, and the escalation is acked.
    #[tokio::test]
    async fn implementation_needed_creates_cross_repo_task_and_notifies_repo_orchestrator() {
        let state = escalation_state().await;

        // The requesting task in repo-a.
        let mut src_task = Task::new(Id::new("task-042"), "TASK-042", now_secs());
        src_task.repo_id = Id::new("repo-a");
        src_task.workspace_id = Id::new("ws-1");
        state.tasks.create(&src_task).await.unwrap();

        let caller = Id::new("repo-a-orchestrator");
        let outcome = escalate(&state, &caller, "t1", &impl_payload())
            .await
            .expect("escalation must process");

        // A live workspace orchestrator was spawned (or reused).
        let agents = state
            .agents
            .list_by_workspace(&Id::new("ws-1"))
            .await
            .unwrap();
        let ws_orch = agents
            .iter()
            .find(|a| a.orchestrator_type == OrchestratorType::WorkspaceOrchestrator)
            .expect("workspace orchestrator exists");

        // The Escalation message was delivered to its inbox and acked
        // (consumed by the processing run).
        let inbox = state
            .messages
            .list_after(&ws_orch.id, 0, None, 100)
            .await
            .unwrap();
        let esc = inbox
            .iter()
            .find(|m| m.kind == MessageKind::Escalation)
            .expect("Escalation delivered to workspace orchestrator inbox");
        assert!(esc.acknowledged, "processed escalation must be acked");
        assert_eq!(esc.from, MessageOrigin::Agent(caller));
        assert_eq!(esc.id.to_string(), outcome.message_id);

        // Cross-repo task created in the TARGET repo (repo-b), typed
        // Implementation, cross-repo labeled.
        let created = state
            .tasks
            .find_by_id(&Id::new(outcome.created_task_id.as_deref().unwrap()))
            .await
            .unwrap()
            .expect("cross-repo task exists");
        assert_eq!(created.repo_id, Id::new("repo-b"));
        assert_eq!(created.workspace_id, Id::new("ws-1"));
        assert_eq!(created.task_type, Some(TaskType::Implementation));
        assert!(created.labels.contains(&"cross-repo".to_string()));
        assert!(created.title.contains("UserPort"));

        // The requesting task now depends on the created task.
        let src_task = state
            .tasks
            .find_by_id(&Id::new("task-042"))
            .await
            .unwrap()
            .unwrap();
        assert!(
            src_task.depends_on.contains(&created.id),
            "requesting task must depend on the cross-repo task, got {:?}",
            src_task.depends_on
        );

        // Repo B's orchestrator was notified: a TaskAssignment sits in its
        // inbox naming the created task.
        let repo_orch_id = outcome
            .notified_repo_orchestrator_id
            .as_deref()
            .expect("repo orchestrator notified");
        let repo_orch_inbox = state
            .messages
            .list_after(&Id::new(repo_orch_id), 0, None, 100)
            .await
            .unwrap();
        let assignment = repo_orch_inbox
            .iter()
            .find(|m| m.kind == MessageKind::TaskAssignment)
            .expect("TaskAssignment delivered to repo-b orchestrator");
        assert_eq!(
            assignment.payload.as_ref().unwrap().get("task_id"),
            Some(&json!(created.id.to_string()))
        );
    }

    // The escalation is workspace-scoped: a target repo in another
    // workspace is refused before any side effect.
    #[tokio::test]
    async fn escalation_across_workspaces_is_refused() {
        let state = escalation_state().await;
        let ws2 = Workspace::new(Id::new("ws-2"), Id::new("t1"), "Ws2", "ws2", 0);
        state.workspaces.create(&ws2).await.unwrap();
        let repo = Repository::new(
            Id::new("repo-c"),
            Id::new("ws-2"),
            "repo-c",
            "/tmp/gyre-test-repo-c",
            0,
        );
        state.repos.create(&repo).await.unwrap();

        let mut payload = impl_payload();
        payload.target_repo_id = "repo-c".to_string();
        let err = escalate(&state, &Id::new("repo-a-orchestrator"), "t1", &payload)
            .await
            .expect_err("cross-workspace escalation must be refused");
        assert!(
            err.to_string().contains("same workspace"),
            "unexpected error: {err:#}"
        );
        // No workspace orchestrator was spawned for ws-1 as a side effect.
        let agents = state
            .agents
            .list_by_workspace(&Id::new("ws-1"))
            .await
            .unwrap();
        assert!(agents
            .iter()
            .all(|a| a.orchestrator_type != OrchestratorType::WorkspaceOrchestrator));
    }

    // MR dependency wiring (ImplementationNeeded with both MRs known): the
    // source repo's MR gains an agent-declared dependency on the target
    // repo's MR, and the merge processor's dependency gate is what blocks
    // it (merge_processor::dependencies_satisfied reads the same field).
    #[tokio::test]
    async fn implementation_needed_links_mr_dependency_cross_repo() {
        let state = escalation_state().await;

        // Source MR in repo-a, target MR in repo-b.
        let mut src_mr = MergeRequest::new(
            Id::new("mr-a"),
            Id::new("repo-a"),
            Id::new("ws-1"),
            "Repo A work",
            "feat/a",
            "main",
            now_secs(),
        );
        src_mr.author_agent_id = Some(Id::new("repo-a-orchestrator"));
        state.merge_requests.create(&src_mr).await.unwrap();
        let tgt_mr = MergeRequest::new(
            Id::new("mr-b"),
            Id::new("repo-b"),
            Id::new("ws-1"),
            "Repo B UserPort",
            "feat/b",
            "main",
            now_secs(),
        );
        state.merge_requests.create(&tgt_mr).await.unwrap();

        let mut payload = impl_payload();
        payload.source_mr_id = Some("mr-a".to_string());
        payload.target_mr_id = Some("mr-b".to_string());
        escalate(&state, &Id::new("repo-a-orchestrator"), "t1", &payload)
            .await
            .expect("escalation must process");

        let src_mr = state
            .merge_requests
            .find_by_id(&Id::new("mr-a"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(src_mr.depends_on.len(), 1);
        assert_eq!(src_mr.depends_on[0].target_mr_id, Id::new("mr-b"));
        assert_eq!(src_mr.depends_on[0].source, DependencySource::AgentDeclared);
        assert!(src_mr.depends_on[0]
            .reason
            .as_deref()
            .unwrap_or_default()
            .contains("UserPort"));

        // The merge processor's dependency gate blocks the source MR while
        // the dependency MR is unmerged (this is the same function the
        // merge loop calls before merging a candidate).
        let satisfied =
            crate::merge_processor::dependencies_satisfied(state.as_ref(), &Id::new("mr-a"))
                .await
                .unwrap();
        assert!(
            !satisfied,
            "source MR must be blocked while the dependency MR is open"
        );
    }

    // The SpecChangeNeeded human gate: the draft lands on a spec-edit
    // branch of a REAL git repo (write-through), the MR targets the
    // default branch, the priority-2 SpecApprovalRequested notification
    // carries the inline-action body (spec_path/spec_sha/mr_id), and the
    // requesting MR depends on the spec MR.
    #[tokio::test]
    async fn spec_change_needed_drafts_spec_mr_notification_and_dependency() {
        use gyre_ports::GitOpsPort as _;

        let git = std::sync::Arc::new(gyre_adapters::Git2OpsAdapter::new());
        let state = crate::mem::test_state_with_git_ops(git.clone());
        crate::seed_builtin_meta_specs(&state).await;
        let ws = Workspace::new(Id::new("ws-1"), Id::new("t1"), "Ws", "ws", 0);
        state.workspaces.create(&ws).await.unwrap();

        // Real on-disk bare repos for both ends.
        let mut repo_paths = Vec::new();
        let mut dirs = Vec::new();
        for name in ["repo-a", "repo-b"] {
            let dir = tempfile::tempdir().expect("tempdir");
            let path = dir.path().join(format!("{name}.git"));
            let path_str = path.to_str().unwrap().to_string();
            git.init_bare(&path_str).await.expect("init_bare");
            git.create_initial_commit(&path_str, "main")
                .await
                .expect("initial commit");
            let repo = Repository::new(Id::new(name), Id::new("ws-1"), name, &path_str, 0);
            state.repos.create(&repo).await.unwrap();
            repo_paths.push(path_str);
            dirs.push(dir);
        }

        for slug in ["workspace-orchestrator", "repo-orchestrator"] {
            let mut persona = gyre_domain::Persona::new(
                Id::new(format!("{slug}-persona")),
                slug,
                slug,
                PersonaScope::Tenant(Id::new("t1")),
                "test orchestrator persona",
                0,
            );
            persona.approval_status = PersonaApprovalStatus::Approved;
            state.personas.create(&persona).await.unwrap();
        }

        // A human approver in the workspace + the requesting MR in repo-a.
        let membership = gyre_domain::WorkspaceMembership::new(
            Id::new("mem-1"),
            Id::new("user-admin"),
            Id::new("ws-1"),
            WorkspaceRole::Admin,
            Id::new("inviter"),
            0,
        );
        state
            .workspace_memberships
            .create(&membership)
            .await
            .unwrap();
        let mut src_mr = MergeRequest::new(
            Id::new("mr-a"),
            Id::new("repo-a"),
            Id::new("ws-1"),
            "Repo A work",
            "feat/a",
            "main",
            now_secs(),
        );
        src_mr.author_agent_id = Some(Id::new("repo-a-orchestrator"));
        state.merge_requests.create(&src_mr).await.unwrap();

        let mut payload = spec_change_payload();
        payload.source_mr_id = Some("mr-a".to_string());
        let outcome = escalate(&state, &Id::new("repo-a-orchestrator"), "t1", &payload)
            .await
            .expect("escalation must process");

        // The spec MR exists in repo-b, open, targeting main from a
        // spec-edit branch.
        let mr_id = outcome.created_mr_id.as_deref().unwrap();
        let mr = state
            .merge_requests
            .find_by_id(&Id::new(mr_id))
            .await
            .unwrap()
            .expect("spec-change MR exists");
        assert_eq!(mr.repository_id, Id::new("repo-b"));
        assert_eq!(mr.target_branch, "main");
        assert!(mr.source_branch.starts_with("spec-edit/"));
        assert_eq!(mr.status, gyre_domain::MrStatus::Open);

        // Write-through: the branch exists in real git and carries the
        // drafted content; main does NOT carry it (approval-gated).
        let repo_b_path = &repo_paths[1];
        assert!(git
            .branch_exists(repo_b_path, &mr.source_branch)
            .await
            .unwrap());
        let spec_path = "specs/system/user-port.md".to_string();
        let content = git
            .read_file(repo_b_path, &mr.source_branch, &spec_path)
            .await
            .unwrap()
            .expect("drafted spec content committed to the branch");
        let content_str = String::from_utf8(content).unwrap();
        assert!(content_str.contains("UserPort"));
        assert!(content_str.contains("TASK-042 needs UserPort from repo-b"));
        assert!(content_str.contains("human approval"));
        assert!(git
            .read_file(repo_b_path, "main", &spec_path)
            .await
            .unwrap()
            .is_none());

        // Priority-2 human notification with the inline-action body.
        let notifs = state
            .notifications
            .list_for_user(
                &Id::new("user-admin"),
                None,
                None,
                None,
                None,
                None,
                &[],
                50,
                0,
            )
            .await
            .unwrap();
        let n = notifs
            .iter()
            .find(|n| {
                n.notification_type == NotificationType::SpecApprovalRequested
                    && n.body
                        .as_deref()
                        .map(|b| b.contains(mr_id))
                        .unwrap_or(false)
            })
            .expect("cross-repo spec-change approval notification created");
        // Priority 2 (HSI §8): the type's rank — the human gate is a
        // high-priority Inbox item.
        assert_eq!(n.notification_type.default_priority_rank(), 2);
        let body: serde_json::Value = serde_json::from_str(n.body.as_deref().unwrap()).unwrap();
        assert_eq!(body["spec_path"], json!(spec_path));
        assert_eq!(body["repo_id"], json!("repo-b"));
        assert_eq!(body["source_repo_id"], json!("repo-a"));
        assert_eq!(body["escalation"], json!("cross-repo-spec-change"));
        // spec_sha is a real 40-hex git blob SHA of the drafted content —
        // never empty (the approve action submits it).
        let sha = body["spec_sha"].as_str().unwrap();
        assert_eq!(
            sha.len(),
            40,
            "spec_sha must be a git blob SHA, got {sha:?}"
        );
        assert!(sha.chars().all(|c| c.is_ascii_hexdigit()));

        // The requesting MR depends on the spec MR.
        let src_mr = state
            .merge_requests
            .find_by_id(&Id::new("mr-a"))
            .await
            .unwrap()
            .unwrap();
        assert!(src_mr.depends_on.iter().any(|d| d.target_mr_id == mr.id));
    }

    // draft_spec_path: interface names slugify under specs/system/, explicit
    // .md paths pass through (normalized under specs/).
    #[test]
    fn draft_spec_path_variants() {
        assert_eq!(draft_spec_path("UserPort"), "specs/system/user-port.md");
        assert_eq!(
            draft_spec_path("HTTPClientFactory"),
            "specs/system/http-client-factory.md"
        );
        assert_eq!(
            draft_spec_path("specs/system/user-port.md"),
            "specs/system/user-port.md"
        );
        assert_eq!(draft_spec_path("ports/auth.md"), "ports/auth.md");
        assert_eq!(draft_spec_path("auth.md"), "specs/auth.md");
    }

    // An unknown escalation_type is rejected by the payload parser, not
    // defaulted — defaulting one branch would silently skip the human gate.
    #[test]
    fn unknown_escalation_type_is_rejected_not_defaulted() {
        let mut payload = impl_payload().to_json();
        payload["escalation_type"] = json!("whatever");
        assert!(EscalationPayload::from_json(&payload).is_none());
    }

    fn inbox_message(payload: serde_json::Value, to: &Id) -> Message {
        Message {
            id: Id::new(uuid::Uuid::new_v4().to_string()),
            tenant_id: Id::new("t1"),
            from: MessageOrigin::Agent(Id::new("repo-a-orchestrator")),
            workspace_id: Some(Id::new("ws-1")),
            to: Destination::Agent(to.clone()),
            kind: MessageKind::Escalation,
            payload: Some(payload),
            created_at: 0,
            signature: None,
            key_id: None,
            acknowledged: false,
        }
    }

    // The poll-side receipt path (message.poll by the workspace
    // orchestrator): an unacked structured escalation left in the inbox
    // (delivery-time processing never ran — e.g. a restart between store
    // and process) is processed and acked on poll, while a GENERIC
    // escalation (reason/context only, from the stale-agent detector et
    // al.) stays unacked for its real consumers.
    #[tokio::test]
    async fn poll_side_processing_handles_unacked_escalations_only() {
        let state = escalation_state().await;
        let orchestrator =
            crate::signal_chain::ensure_workspace_orchestrator(&state, &Id::new("ws-1"))
                .await
                .expect("workspace orchestrator");

        // A structured cross-repo escalation and a generic escalation,
        // both unacked in the orchestrator's inbox.
        let structured = inbox_message(impl_payload().to_json(), &orchestrator.id);
        let generic = inbox_message(
            json!({"reason": "agent stale", "context": "stale-agent detector"}),
            &orchestrator.id,
        );
        state.messages.store(&structured).await.unwrap();
        state.messages.store(&generic).await.unwrap();

        drive_pending_escalations(&state, &orchestrator.id, &Id::new("ws-1")).await;

        // The structured escalation was processed: cross-repo task in the
        // target repo, message acked.
        let tasks = state.tasks.list().await.unwrap();
        assert!(
            tasks.iter().any(|t| t.repo_id == Id::new("repo-b")),
            "poll-side processing must create the cross-repo task"
        );
        let unacked = state
            .messages
            .list_unacked(&orchestrator.id, 100)
            .await
            .unwrap();
        assert!(
            unacked.iter().all(|m| m.id != structured.id),
            "processed escalation must be acked"
        );
        // The generic escalation stays unacked — it is not a cross-repo
        // escalation and belongs to its real consumers.
        assert!(
            unacked.iter().any(|m| m.id == generic.id),
            "generic escalation must stay unacked for its real consumer"
        );
    }
}
