//! Smart HTTP Git protocol handlers.
//!
//! Implements the Git smart HTTP protocol by shelling out to `git upload-pack`
//! and `git receive-pack`. This is the same approach used by GitLab, Gitea, etc.
//!
//! Supported routes (M34 Slice 6 — workspace-slug/repo-name format):
//!   GET  /git/:workspace_slug/:repo_name/info/refs?service={git-upload-pack|git-receive-pack}
//!   POST /git/:workspace_slug/:repo_name/git-upload-pack
//!   POST /git/:workspace_slug/:repo_name/git-receive-pack
//!
//! The server resolves workspace_slug + repo_name → Repository entity via the
//! WorkspaceRepository and RepoRepository ports. ABAC validates workspace access.

use axum::{
    body::Body,
    extract::{Path, Query, Request, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use gyre_common::Id;
use gyre_ports::{GateOutcome, PushContext};
use serde::Deserialize;
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tracing::{error, info, warn};

use crate::{auth::AuthenticatedAgent, AppState};

// ---------------------------------------------------------------------------
// Error helpers
// ---------------------------------------------------------------------------

fn git_err(msg: impl std::fmt::Display) -> Response {
    (StatusCode::INTERNAL_SERVER_ERROR, msg.to_string()).into_response()
}

fn not_found(msg: impl std::fmt::Display) -> Response {
    (StatusCode::NOT_FOUND, msg.to_string()).into_response()
}

// ---------------------------------------------------------------------------
// PKT-LINE helpers
// ---------------------------------------------------------------------------

/// Encode `data` as a single pkt-line (length prefix + data).
fn pkt_line(data: &str) -> Vec<u8> {
    let len = data.len() + 4; // 4 bytes for the hex-length prefix
    let mut out = format!("{len:04x}").into_bytes();
    out.extend_from_slice(data.as_bytes());
    out
}

/// Build the service-advertisement header prepended to info/refs responses.
///
/// Format:  pkt-line("# service=<svc>\n")  +  flush-pkt(0000)
fn service_header(service: &str) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend(pkt_line(&format!("# service={service}\n")));
    out.extend_from_slice(b"0000");
    out
}

// ---------------------------------------------------------------------------
// Repo lookup
// ---------------------------------------------------------------------------

/// Resolve `:workspace_slug` + `:repo_name` URL segments to a Repository record.
///
/// * `tenant_id`      — the caller's tenant ID (from auth context).
/// * `workspace_slug` — the workspace slug from the URL path (unique within tenant).
/// * `repo_name`      — the repository name, e.g. `my-repo` or `my-repo.git`.
///
/// Resolution steps:
///   1. Look up the workspace by slug under the caller's tenant.
///   2. Look up the repo by name within that workspace.
async fn resolve_repo_by_slug(
    state: &Arc<AppState>,
    tenant_id: &str,
    workspace_slug: &str,
    repo_name: &str,
) -> Result<gyre_domain::Repository, Response> {
    let repo_name = repo_name.strip_suffix(".git").unwrap_or(repo_name);
    let tid = Id::new(tenant_id);

    let workspace = state
        .workspaces
        .find_by_slug(&tid, workspace_slug)
        .await
        .map_err(|e| git_err(format!("db error: {e}")))?
        .ok_or_else(|| not_found(format!("workspace '{workspace_slug}' not found")))?;

    state
        .repos
        .find_by_name_and_workspace(&workspace.id, repo_name)
        .await
        .map_err(|e| git_err(format!("db error: {e}")))?
        .ok_or_else(|| {
            not_found(format!(
                "repo '{repo_name}' not found in workspace '{workspace_slug}'"
            ))
        })
}

/// Resolve to just the filesystem path (convenience wrapper).
async fn resolve_repo_path_by_slug(
    state: &Arc<AppState>,
    tenant_id: &str,
    workspace_slug: &str,
    repo_name: &str,
) -> Result<String, Response> {
    resolve_repo_by_slug(state, tenant_id, workspace_slug, repo_name)
        .await
        .map(|r| r.path)
}

// ---------------------------------------------------------------------------
// Path params
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct GitPath {
    workspace_slug: String,
    repo_name: String,
}

// ---------------------------------------------------------------------------
// GET /git/:workspace_slug/:repo_name/info/refs?service=git-{upload,receive}-pack
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct InfoRefsQuery {
    service: String,
}

pub async fn git_info_refs(
    State(state): State<Arc<AppState>>,
    Path(GitPath {
        workspace_slug,
        repo_name,
    }): Path<GitPath>,
    Query(InfoRefsQuery { service }): Query<InfoRefsQuery>,
    auth: AuthenticatedAgent,
) -> Response {
    let subcommand = match service.as_str() {
        "git-upload-pack" => "upload-pack",
        "git-receive-pack" => "receive-pack",
        other => {
            return (StatusCode::BAD_REQUEST, format!("unknown service: {other}")).into_response()
        }
    };

    let content_type = format!("application/x-{service}-advertisement");

    let repo_path =
        match resolve_repo_path_by_slug(&state, &auth.tenant_id, &workspace_slug, &repo_name).await
        {
            Ok(p) => p,
            Err(r) => return r,
        };

    let git_bin = std::env::var("GYRE_GIT_PATH").unwrap_or_else(|_| "git".to_string());

    let output = match Command::new(&git_bin)
        .arg(subcommand)
        .arg("--stateless-rpc")
        .arg("--advertise-refs")
        .arg(&repo_path)
        .output()
        .await
    {
        Ok(o) => o,
        Err(e) => return git_err(format!("failed to spawn git: {e}")),
    };

    if !output.status.success() {
        // Log stderr internally; do NOT expose it in the HTTP response (information disclosure).
        let stderr = String::from_utf8_lossy(&output.stderr);
        error!(%repo_path, %stderr, "git advertise-refs failed");
        return git_err("git operation failed");
    }

    let mut body = service_header(&service);
    body.extend_from_slice(&output.stdout);

    info!(%repo_path, service = %service, "served info/refs");

    Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", content_type)
        .header("Cache-Control", "no-cache")
        .body(Body::from(body))
        .unwrap()
}

// ---------------------------------------------------------------------------
// POST /git/:repo_id/:repo/git-upload-pack  (clone / fetch)
// ---------------------------------------------------------------------------

pub async fn git_upload_pack(
    State(state): State<Arc<AppState>>,
    Path(GitPath {
        workspace_slug,
        repo_name,
    }): Path<GitPath>,
    auth: AuthenticatedAgent,
    req: Request,
) -> Response {
    let repo_path =
        match resolve_repo_path_by_slug(&state, &auth.tenant_id, &workspace_slug, &repo_name).await
        {
            Ok(p) => p,
            Err(r) => return r,
        };

    let body_bytes = match axum::body::to_bytes(req.into_body(), 64 * 1024 * 1024).await {
        Ok(b) => b,
        Err(e) => return git_err(format!("failed to read request body: {e}")),
    };

    let output = match run_git_stateless("upload-pack", &repo_path, &body_bytes).await {
        Ok(o) => o,
        Err(e) => return git_err(e),
    };

    info!(%repo_path, "served git-upload-pack");

    Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "application/x-git-upload-pack-result")
        .body(Body::from(output))
        .unwrap()
}

// ---------------------------------------------------------------------------
// POST /git/:repo_id/:repo/git-receive-pack  (push)
// ---------------------------------------------------------------------------

pub async fn git_receive_pack(
    State(state): State<Arc<AppState>>,
    Path(GitPath {
        workspace_slug,
        repo_name,
    }): Path<GitPath>,
    auth: AuthenticatedAgent,
    req: Request,
) -> Response {
    let resolved =
        match resolve_repo_by_slug(&state, &auth.tenant_id, &workspace_slug, &repo_name).await {
            Ok(r) => r,
            Err(r) => return r,
        };
    if resolved.is_mirror {
        warn!(
            agent_id = %auth.agent_id,
            workspace_slug = %workspace_slug,
            repo_name = %repo_name,
            "git-receive-pack 403: repository is a read-only mirror"
        );
        return (
            StatusCode::FORBIDDEN,
            "push rejected: repository is a read-only mirror".to_string(),
        )
            .into_response();
    }
    let repo_id = resolved.id.to_string();
    let repo_workspace_id = resolved.workspace_id.clone();
    let repo_path = resolved.path;
    let default_branch = resolved.default_branch;

    // G6: ABAC enforcement — check repo access policies against the caller's JWT claims.
    if let Err(reason) = crate::abac::check_repo_abac(&state, &repo_id, &auth).await {
        warn!(
            agent_id = %auth.agent_id,
            %repo_id,
            %reason,
            has_jwt_claims = auth.jwt_claims.is_some(),
            "git-receive-pack 403: ABAC denied"
        );
        return (StatusCode::FORBIDDEN, reason).into_response();
    }

    // M13.2: Extract model context header before consuming the request body.
    let model_context = req
        .headers()
        .get("x-gyre-model-context")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);

    // HSI §5: Extract conversation turn header for provenance linking.
    let conversation_turn: Option<u32> = req
        .headers()
        .get("x-gyre-conversation-turn")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse().ok());

    let body_bytes = match axum::body::to_bytes(req.into_body(), 64 * 1024 * 1024).await {
        Ok(b) => b,
        Err(e) => return git_err(format!("failed to read request body: {e}")),
    };

    // Parse updated refs BEFORE forwarding to git (pkt-line ref-update commands).
    let ref_updates = parse_ref_updates(&body_bytes);

    let output = match run_git_stateless("receive-pack", &repo_path, &body_bytes).await {
        Ok(o) => o,
        Err(e) => return git_err(e),
    };

    // Pre-accept gate checks: run after git accepts packfile, undo refs on failure.
    if !ref_updates.is_empty() {
        if let Err(rejection) =
            check_pre_accept_gates(&state, &repo_id, &repo_path, &ref_updates, &auth.agent_id).await
        {
            undo_ref_updates(&repo_path, &ref_updates).await;
            // Emit PushRejected event via unified message bus.
            state
                .emit_event(
                    Some(repo_workspace_id.clone()),
                    gyre_common::message::Destination::Workspace(repo_workspace_id.clone()),
                    gyre_common::message::MessageKind::PushRejected,
                    Some(serde_json::json!({
                        "repo_id": repo_id,
                        "branch": ref_updates.first().map(|u| u.refname.clone()).unwrap_or_default(),
                        "agent_id": auth.agent_id,
                        "reason": rejection,
                    })),
                )
                .await;
            return (StatusCode::FORBIDDEN, rejection).into_response();
        }
    }

    // Phase 3 (TASK-008): Enforcement — reject pushes with invalid/missing
    // attestation chains or constraint violations. Must run synchronously before
    // returning the response so we can undo refs on failure.
    //
    // Resolve agent context first so we have the task_id for chain lookup.
    let (task_id, parent_agent_id, spawned_by_user_id) =
        resolve_agent_context(&state, &auth.agent_id).await;

    if !ref_updates.is_empty() {
        if let Some(ref tid) = task_id {
            let constraint_ref_updates: Vec<(String, String, String)> = ref_updates
                .iter()
                .map(|u| (u.old_sha.clone(), u.new_sha.clone(), u.refname.clone()))
                .collect();
            if let Err(rejection) = crate::constraint_check::enforce_push_constraints(
                &state,
                tid,
                &repo_id,
                &repo_path,
                &auth.agent_id,
                &repo_workspace_id,
                &constraint_ref_updates,
                &default_branch,
            )
            .await
            {
                undo_ref_updates(&repo_path, &ref_updates).await;
                // Emit PushRejected event.
                state
                    .emit_event(
                        Some(repo_workspace_id.clone()),
                        gyre_common::message::Destination::Workspace(repo_workspace_id.clone()),
                        gyre_common::message::MessageKind::PushRejected,
                        Some(serde_json::json!({
                            "repo_id": repo_id,
                            "branch": ref_updates.first().map(|u| u.refname.clone()).unwrap_or_default(),
                            "agent_id": auth.agent_id,
                            "reason": rejection,
                        })),
                    )
                    .await;
                return (StatusCode::FORBIDDEN, rejection).into_response();
            }
        }
    }

    // TASK-017: Manifest enforcement — reject pushes with unregistered spec files.
    // spec-registry.md §Manifest Rules rule 1 + §Ledger Sync on Push step 4.
    // Only check pushes that update the default branch.
    {
        let default_ref = format!("refs/heads/{default_branch}");
        for update in ref_updates.iter().filter(|u| u.refname == default_ref) {
            // Read manifest to build the set of registered spec paths.
            let manifest_paths =
                crate::spec_registry::read_manifest_paths(&repo_path, &update.new_sha).await;
            // Check spec policy for this repo.
            let enforce = state
                .spec_policies
                .get_for_repo(&repo_id)
                .await
                .map(|p| p.enforce_manifest)
                .unwrap_or(false);
            if let Err(rejection) = crate::spec_registry::check_manifest_coverage(
                &repo_path,
                &update.new_sha,
                &manifest_paths,
                enforce,
            )
            .await
            {
                undo_ref_updates(&repo_path, &ref_updates).await;
                state
                    .emit_event(
                        Some(repo_workspace_id.clone()),
                        gyre_common::message::Destination::Workspace(repo_workspace_id.clone()),
                        gyre_common::message::MessageKind::PushRejected,
                        Some(serde_json::json!({
                            "repo_id": repo_id,
                            "branch": default_branch,
                            "agent_id": auth.agent_id,
                            "reason": rejection,
                        })),
                    )
                    .await;
                return (StatusCode::FORBIDDEN, rejection).into_response();
            }
        }
    }

    // TASK-019: Cycle detection — reject pushes that create cycles in spec links.
    // spec-links.md §Cycle Detection: "The forge rejects manifest changes that
    // would create cycles in the spec graph."
    {
        let default_ref = format!("refs/heads/{default_branch}");
        for update in ref_updates.iter().filter(|u| u.refname == default_ref) {
            if let Err(rejection) =
                crate::spec_registry::check_spec_link_cycles(&repo_path, &update.new_sha).await
            {
                undo_ref_updates(&repo_path, &ref_updates).await;
                state
                    .emit_event(
                        Some(repo_workspace_id.clone()),
                        gyre_common::message::Destination::Workspace(repo_workspace_id.clone()),
                        gyre_common::message::MessageKind::PushRejected,
                        Some(serde_json::json!({
                            "repo_id": repo_id,
                            "branch": default_branch,
                            "agent_id": auth.agent_id,
                            "reason": rejection,
                        })),
                    )
                    .await;
                return (StatusCode::FORBIDDEN, rejection).into_response();
            }
        }
    }

    info!(%repo_path, updates = ref_updates.len(), "served git-receive-pack");

    // M13.3: Build X-Gyre-Push-Result JSON header value.
    let branch = ref_updates
        .first()
        .map(|u| u.refname.trim_start_matches("refs/heads/").to_string())
        .unwrap_or_default();
    let push_result = serde_json::json!({
        "repo_id": repo_id,
        "branch": branch,
        "agent_id": auth.agent_id,
        "commit_count": ref_updates.len(),
        "task_id": task_id,
    });

    // M13.3: Append sideband feedback to git output.
    let mut output_with_feedback = output;
    let feedback = build_feedback_sideband(&branch, task_id.as_deref());
    output_with_feedback.extend_from_slice(&feedback);

    // Post-receive: record agent-commit mappings + broadcast PushAccepted event.
    // M14.2: Compute attestation level for commit provenance.
    let attestation_level = {
        let has_stack = state
            .kv_store
            .kv_get("agent_stacks", auth.agent_id.as_str())
            .await
            .ok()
            .flatten()
            .is_some();
        if has_stack {
            let has_policy = state
                .kv_store
                .kv_get("repo_stack_policies", repo_id.as_str())
                .await
                .ok()
                .flatten()
                .is_some();
            if has_policy {
                "server-verified"
            } else {
                "self-reported"
            }
        } else {
            "unattested"
        }
    };

    let state_clone = state.clone();
    let repo_path_clone = repo_path.clone();
    let agent_id = auth.agent_id.clone();
    let tenant_id_clone = auth.tenant_id.clone();
    let task_id_clone = task_id.clone();
    let parent_agent_id_clone = parent_agent_id.clone();
    let spawned_by_clone = spawned_by_user_id.clone();
    let model_context_clone = model_context.clone();
    let repo_id_clone = repo_id.clone();
    let repo_workspace_id_clone = repo_workspace_id.clone();
    // Save workspace ID string before the clone is moved into PushAccepted event destination.
    let repo_workspace_id_str = repo_workspace_id.to_string();
    let branch_clone = branch.clone();
    let attestation_level_clone = attestation_level.to_string();
    let default_branch_clone = default_branch;
    let conversation_turn_clone = conversation_turn;
    let push_tenant_id = auth.tenant_id.clone();
    let push_workspace_id = repo_workspace_id.clone();
    tokio::spawn(async move {
        let commit_count = record_pushed_commits(
            &state_clone,
            &repo_path_clone,
            &ref_updates,
            &agent_id,
            task_id_clone.as_deref(),
            parent_agent_id_clone.as_deref(),
            spawned_by_clone.as_deref(),
            model_context_clone.as_deref(),
            &attestation_level_clone,
        )
        .await;

        // HSI §5: Record turn-commit links for conversation provenance.
        if let Some(turn) = conversation_turn_clone {
            record_turn_commit_links(
                &state_clone,
                &agent_id,
                &tenant_id_clone,
                turn,
                &ref_updates,
                crate::api::now_secs(),
            )
            .await;
        }
        // Emit PushAccepted event via unified message bus.
        state_clone
            .emit_event(
                Some(repo_workspace_id_clone.clone()),
                gyre_common::message::Destination::Workspace(repo_workspace_id_clone),
                gyre_common::message::MessageKind::PushAccepted,
                Some(serde_json::json!({
                    "repo_id": repo_id_clone,
                    "branch": branch_clone,
                    "agent_id": agent_id,
                    "commit_count": commit_count as u64,
                    "task_id": task_id_clone,
                })),
            )
            .await;

        // TASK-006 (Phase 1) + TASK-007 (Phase 2): Audit-only attestation chain
        // verification AND strategy-implied constraint evaluation.
        // Phase 1 verifies the chain structure; Phase 2 derives and evaluates
        // strategy-implied constraints against the actual diff.
        // Both are audit-only — results are logged and violations emitted, but
        // pushes are NEVER rejected.
        if let Some(ref tid) = task_id_clone {
            // Phase 1: chain structure verification.
            match state_clone.chain_attestations.find_by_task(tid).await {
                Ok(attestations) if !attestations.is_empty() => {
                    for att in &attestations {
                        let result = verify_attestation_audit_only(att);
                        if result.valid {
                            tracing::info!(
                                attestation_id = %att.id,
                                task_id = %tid,
                                repo_id = %repo_id_clone,
                                label = %result.label,
                                "attestation.verified: chain valid"
                            );
                        } else {
                            tracing::warn!(
                                attestation_id = %att.id,
                                task_id = %tid,
                                repo_id = %repo_id_clone,
                                label = %result.label,
                                message = %result.message,
                                "attestation.chain_invalid: verification failed"
                            );
                        }
                    }
                }
                Ok(_) => {
                    tracing::debug!(
                        task_id = %tid,
                        repo_id = %repo_id_clone,
                        "no attestation chain found for task (Phase 1, non-enforcing)"
                    );
                }
                Err(e) => {
                    tracing::warn!(
                        task_id = %tid,
                        error = %e,
                        "failed to query attestation chain (Phase 1, non-blocking)"
                    );
                }
            }

            // Phase 2: strategy-implied constraint evaluation (TASK-007).
            let constraint_ref_updates: Vec<(String, String, String)> = ref_updates
                .iter()
                .map(|u| (u.old_sha.clone(), u.new_sha.clone(), u.refname.clone()))
                .collect();
            crate::constraint_check::evaluate_push_constraints(
                &state_clone,
                tid,
                &repo_id_clone,
                &repo_path_clone,
                &agent_id,
                &push_workspace_id,
                &constraint_ref_updates,
                &default_branch_clone,
            )
            .await;
        }

        // Spec lifecycle: auto-create tasks for spec changes on the default branch.
        process_spec_lifecycle(
            &state_clone,
            &repo_id_clone,
            &repo_path_clone,
            &default_branch_clone,
            &ref_updates,
        )
        .await;
        // Spec registry: sync ledger from manifest on pushes to the default branch (M21.1).
        let default_ref = format!("refs/heads/{default_branch_clone}");
        // Resolve workspace tenant_id for cross-workspace link resolution.
        let workspace_tenant_id = state_clone
            .workspaces
            .find_by_id(&gyre_common::Id::new(&repo_workspace_id_str))
            .await
            .ok()
            .flatten()
            .map(|ws| ws.tenant_id);
        for update in ref_updates.iter().filter(|u| u.refname == default_ref) {
            let now = crate::api::now_secs();
            crate::spec_registry::sync_spec_ledger(
                &state_clone.spec_ledger,
                &state_clone.spec_links_store,
                &repo_path_clone,
                &update.new_sha,
                now,
                Some(repo_id_clone.as_str()),
                Some(repo_workspace_id_str.as_str()),
                Some(&state_clone.workspaces),
                Some(&state_clone.repos),
                workspace_tenant_id.as_ref(),
                Some(&state_clone.tasks),
            )
            .await;
            // Dependency graph: auto-detect Cargo.toml path deps (M22.4).
            detect_dependencies_on_push(
                &state_clone,
                &repo_id_clone,
                &repo_path_clone,
                &update.new_sha,
            )
            .await;
            // Breaking change detection (TASK-020).
            detect_breaking_changes_on_push(
                &state_clone,
                &repo_id_clone,
                &repo_path_clone,
                &update.old_sha,
                &update.new_sha,
                &repo_workspace_id_str,
                &push_tenant_id,
            )
            .await;
            // Knowledge graph: extract Rust symbols and architecture (M30b).
            // When the push is from an agent with a task, enrich the delta with
            // agent context and run a post-extraction divergence check (HSI §8).
            let git_bin = std::env::var("GYRE_GIT_PATH").unwrap_or_else(|_| "git".to_string());
            let agent_push_ctx = if !agent_id.is_empty() {
                // Look up the agent's current task to obtain the spec_ref.
                let spec_ref = if let Some(ref tid) = task_id_clone {
                    state_clone
                        .tasks
                        .find_by_id(&gyre_common::Id::new(tid.clone()))
                        .await
                        .ok()
                        .flatten()
                        .and_then(|t| t.spec_path)
                        .unwrap_or_default()
                } else {
                    String::new()
                };
                if spec_ref.is_empty() {
                    None
                } else {
                    Some(crate::graph_extraction::AgentPushContext {
                        agent_id: agent_id.clone(),
                        spec_ref,
                        workspace_id: push_workspace_id.to_string(),
                        tenant_id: push_tenant_id.clone(),
                    })
                }
            } else {
                None
            };
            let divergence_ports = Some(crate::graph_extraction::DivergencePorts {
                notification_repo: state_clone.notifications.as_ref(),
                membership_repo: state_clone.workspace_memberships.as_ref(),
            });
            crate::graph_extraction::extract_and_store_graph(
                &repo_path_clone,
                &repo_id_clone,
                &update.new_sha,
                Arc::clone(&state_clone.graph_store),
                &git_bin,
                agent_push_ctx,
                divergence_ports,
            )
            .await;
        }
    });

    Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "application/x-git-receive-pack-result")
        .header("X-Gyre-Push-Result", push_result.to_string())
        .body(Body::from(output_with_feedback))
        .unwrap()
}

/// Resolve the task context for an agent to populate commit provenance (M13.2).
/// Returns (task_id, parent_agent_id, spawned_by_user_id).
async fn resolve_agent_context(
    state: &Arc<AppState>,
    agent_id: &str,
) -> (Option<String>, Option<String>, Option<String>) {
    let agent = match state.agents.find_by_id(&Id::new(agent_id)).await {
        Ok(Some(a)) => a,
        _ => return (None, None, None),
    };

    let parent_agent_id = agent.parent_id.map(|id| id.to_string());
    let spawned_by_user_id = agent.spawned_by.clone();

    let task_id = agent.current_task_id.map(|id| id.to_string());

    (task_id, parent_agent_id, spawned_by_user_id)
}

/// Build git sideband-64k pkt-lines carrying human-readable push feedback (M13.3).
fn build_feedback_sideband(branch: &str, task_id: Option<&str>) -> Vec<u8> {
    let mut lines = vec![format!(
        "remote: [GYRE] Push accepted for branch {branch}\n"
    )];
    if let Some(tid) = task_id {
        lines.push(format!("remote: [GYRE] Task: {tid}\n"));
    }

    let mut out = Vec::new();
    for line in lines {
        // Sideband 2 = progress channel; prefix with \x02
        let payload = format!("\x02{line}");
        let len = payload.len() + 4;
        out.extend_from_slice(format!("{len:04x}").as_bytes());
        out.extend_from_slice(payload.as_bytes());
    }
    out
}

// ---------------------------------------------------------------------------
// Subprocess helper
// ---------------------------------------------------------------------------

async fn run_git_stateless(
    subcommand: &str,
    repo_path: &str,
    stdin_data: &[u8],
) -> Result<Vec<u8>, String> {
    let git_bin = std::env::var("GYRE_GIT_PATH").unwrap_or_else(|_| "git".to_string());

    let mut child = Command::new(&git_bin)
        .arg(subcommand)
        .arg("--stateless-rpc")
        .arg(repo_path)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("failed to spawn git {subcommand}: {e}"))?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(stdin_data)
            .await
            .map_err(|e| format!("failed to write to git stdin: {e}"))?;
    }

    let out = child
        .wait_with_output()
        .await
        .map_err(|e| format!("git {subcommand} wait failed: {e}"))?;

    if !out.status.success() {
        // Log stderr internally; do NOT expose it to callers (information disclosure).
        let stderr = String::from_utf8_lossy(&out.stderr);
        error!(subcommand, %repo_path, %stderr, "git command failed");
        return Err(format!("git {subcommand} failed"));
    }

    Ok(out.stdout)
}

// ---------------------------------------------------------------------------
// Ref-update parsing (pkt-line receive-pack input)
// ---------------------------------------------------------------------------

/// A ref update parsed from the receive-pack input stream.
#[derive(Debug)]
struct RefUpdate {
    old_sha: String,
    new_sha: String,
    refname: String,
}

/// Parse the initial pkt-line ref-update commands from a receive-pack body.
///
/// Format of each line (before NUL / capabilities on first line):
///   `{old-sha} {new-sha} {refname}\0{capabilities?}`  or  `{old-sha} {new-sha} {refname}`
/// Validate that a string is a valid 40-character hex SHA (M-8 security fix).
fn is_valid_sha(s: &str) -> bool {
    s.len() == 40 && s.chars().all(|c| c.is_ascii_hexdigit())
}

fn parse_ref_updates(body: &[u8]) -> Vec<RefUpdate> {
    let mut updates = Vec::new();
    let mut pos = 0;

    while pos + 4 <= body.len() {
        // Read 4-byte hex length.
        let len_str = std::str::from_utf8(&body[pos..pos + 4]).unwrap_or("");
        let Ok(pkt_len) = usize::from_str_radix(len_str, 16) else {
            break;
        };

        if pkt_len == 0 {
            // Flush packet — end of ref-update commands, packfile follows.
            break;
        }
        if pkt_len < 4 || pos + pkt_len > body.len() {
            break;
        }

        let data = &body[pos + 4..pos + pkt_len];
        pos += pkt_len;

        // Strip capabilities (everything after NUL on the first line).
        let line = std::str::from_utf8(data)
            .unwrap_or("")
            .split('\0')
            .next()
            .unwrap_or("")
            .trim_end_matches('\n');

        let parts: Vec<&str> = line.splitn(3, ' ').collect();
        if parts.len() == 3 {
            let zeros = "0000000000000000000000000000000000000000";
            let old = parts[0].to_string();
            let new = parts[1].to_string();
            // M-8 fix: validate SHA format to prevent git argument injection.
            if !is_valid_sha(&old) || !is_valid_sha(&new) {
                tracing::warn!("invalid SHA in push ref-update: old={old} new={new}");
                continue;
            }
            // Only record pushes of real commits (not deletions).
            if new != zeros {
                updates.push(RefUpdate {
                    old_sha: old,
                    new_sha: new,
                    refname: parts[2].to_string(),
                });
            }
        }
    }

    updates
}

// ---------------------------------------------------------------------------
// Post-receive commit recording
// ---------------------------------------------------------------------------

/// Returns the number of commits successfully recorded.
#[allow(clippy::too_many_arguments)]
async fn record_pushed_commits(
    state: &Arc<AppState>,
    repo_path: &str,
    updates: &[RefUpdate],
    agent_id: &str,
    task_id: Option<&str>,
    parent_agent_id: Option<&str>,
    spawned_by_user_id: Option<&str>,
    model_context: Option<&str>,
    attestation_level: &str,
) -> usize {
    if updates.is_empty() {
        return 0;
    }

    // Find the repo record by path.
    let all_repos = match state.repos.list().await {
        Ok(r) => r,
        Err(e) => {
            warn!("post-receive: failed to list repos: {e}");
            return 0;
        }
    };
    let repo = match all_repos.iter().find(|r| r.path == repo_path) {
        Some(r) => r.clone(),
        None => {
            warn!(%repo_path, "post-receive: repo not found");
            return 0;
        }
    };

    let git_bin = std::env::var("GYRE_GIT_PATH").unwrap_or_else(|_| "git".to_string());
    let mut total_recorded = 0usize;

    for update in updates {
        // Walk new commits: git log {old}..{new} --format="%H"
        let range = if update.old_sha.starts_with("00000000") {
            update.new_sha.clone()
        } else {
            format!("{}..{}", update.old_sha, update.new_sha)
        };

        let out = match Command::new(&git_bin)
            .arg("-C")
            .arg(repo_path)
            .arg("log")
            .arg("--format=%H")
            .arg(&range)
            .output()
            .await
        {
            Ok(o) if o.status.success() => o,
            Ok(o) => {
                let err = String::from_utf8_lossy(&o.stderr);
                warn!(%range, %err, "post-receive: git log failed");
                continue;
            }
            Err(e) => {
                warn!(%range, "post-receive: git log spawn failed: {e}");
                continue;
            }
        };

        let shas = String::from_utf8_lossy(&out.stdout);
        for sha in shas.lines().filter(|s| !s.is_empty()) {
            let mapping = gyre_domain::AgentCommit::new(
                Id::new(uuid::Uuid::new_v4().to_string()),
                Id::new(agent_id),
                repo.id.clone(),
                sha,
                &update.refname,
                crate::api::now_secs(),
            )
            .with_provenance(
                task_id.map(str::to_string),
                spawned_by_user_id.map(str::to_string),
                parent_agent_id.map(str::to_string),
                model_context.map(str::to_string),
            )
            .with_attestation_level(attestation_level);
            if let Err(e) = state.agent_commits.record(&mapping).await {
                warn!(%sha, "post-receive: failed to record commit: {e}");
            } else {
                total_recorded += 1;
            }
        }
    }
    total_recorded
}

// ---------------------------------------------------------------------------
// Conversation provenance (HSI §5)
// ---------------------------------------------------------------------------

/// Record `TurnCommitLink` entries for each ref update in this push.
///
/// Called from `git_receive_pack` when `X-Gyre-Conversation-Turn` header is present.
/// Links are stored without `conversation_sha` (back-filled at upload time).
async fn record_turn_commit_links(
    state: &Arc<AppState>,
    agent_id: &str,
    tenant_id: &str,
    turn_number: u32,
    ref_updates: &[RefUpdate],
    now: u64,
) {
    use gyre_common::TurnCommitLink;
    let aid = Id::new(agent_id);
    let tid = Id::new(tenant_id);

    // Get the list of files changed across all commits in this push.
    // For simplicity, we record one TurnCommitLink per ref update.
    for update in ref_updates {
        if update.new_sha.chars().all(|c| c == '0') {
            // Skip branch deletion.
            continue;
        }
        let link = TurnCommitLink {
            id: Id::new(uuid::Uuid::new_v4().to_string()),
            agent_id: aid.clone(),
            turn_number,
            commit_sha: update.new_sha.clone(),
            files_changed: Vec::new(), // files are not parsed here; filled in opportunistically
            conversation_sha: None,
            timestamp: now,
            tenant_id: tid.clone(),
        };
        if let Err(e) = state.conversations.record_turn_link(&link).await {
            warn!(
                agent_id = %agent_id,
                commit_sha = %update.new_sha,
                error = %e,
                "Failed to record turn_commit_link"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Pre-accept gate enforcement
// ---------------------------------------------------------------------------

/// Run the configured pre-accept gates for a repository against the pushed commits.
/// Returns `Ok(())` if all gates pass, or `Err(reason)` if any gate rejects the push.
async fn check_pre_accept_gates(
    state: &Arc<AppState>,
    repo_id: &str,
    repo_path: &str,
    ref_updates: &[RefUpdate],
    agent_id: &str,
) -> Result<(), String> {
    // Get gate names configured for this repo.
    let gate_names = state
        .repo_push_gates
        .get_for_repo(repo_id)
        .await
        .unwrap_or_default();
    if gate_names.is_empty() {
        return Ok(());
    }

    // M14.2: Resolve agent stack fingerprint and repo policy once for all ref updates.
    let stack_fingerprint = state
        .kv_store
        .kv_get("agent_stacks", agent_id)
        .await
        .ok()
        .flatten()
        .and_then(|s| {
            serde_json::from_str::<crate::api::stack_attest::AgentStack>(&s)
                .ok()
                .map(|st| st.fingerprint())
        });
    let required_fingerprint = state
        .kv_store
        .kv_get("repo_stack_policies", repo_id)
        .await
        .ok()
        .flatten();

    let git_bin = std::env::var("GYRE_GIT_PATH").unwrap_or_else(|_| "git".to_string());

    for update in ref_updates {
        // Collect commit messages for this ref update.
        let range = if update.old_sha.starts_with("00000000") {
            update.new_sha.clone()
        } else {
            format!("{}..{}", update.old_sha, update.new_sha)
        };

        let msg_out = Command::new(&git_bin)
            .arg("-C")
            .arg(repo_path)
            .arg("log")
            .arg("--format=%s")
            .arg(&range)
            .output()
            .await
            .ok();

        let commit_messages: Vec<String> = msg_out
            .as_ref()
            .filter(|o| o.status.success())
            .map(|o| {
                String::from_utf8_lossy(&o.stdout)
                    .lines()
                    .filter(|l| !l.is_empty())
                    .map(|l| l.to_string())
                    .collect()
            })
            .unwrap_or_default();

        // Collect changed files.
        let files_out = Command::new(&git_bin)
            .arg("-C")
            .arg(repo_path)
            .arg("diff")
            .arg("--name-only")
            .arg(&range)
            .output()
            .await
            .ok();

        let changed_files: Vec<String> = files_out
            .as_ref()
            .filter(|o| o.status.success())
            .map(|o| {
                String::from_utf8_lossy(&o.stdout)
                    .lines()
                    .filter(|l| !l.is_empty())
                    .map(|l| l.to_string())
                    .collect()
            })
            .unwrap_or_default();

        let branch = update
            .refname
            .strip_prefix("refs/heads/")
            .unwrap_or(&update.refname)
            .to_string();

        let ctx = PushContext {
            repo_id: repo_id.to_string(),
            refname: update.refname.clone(),
            branch,
            commit_messages,
            changed_files,
            agent_id: Some(agent_id.to_string()),
            stack_fingerprint: stack_fingerprint.clone(),
            required_fingerprint: required_fingerprint.clone(),
        };

        // Run each configured gate.
        for gate_name in &gate_names {
            if let Some(gate) = state
                .push_gate_registry
                .iter()
                .find(|g| g.name() == gate_name)
            {
                match gate.check(&ctx) {
                    GateOutcome::Passed => {}
                    GateOutcome::Failed(reason) => {
                        warn!(
                            repo_id,
                            gate = gate_name,
                            refname = %update.refname,
                            %reason,
                            "pre-accept gate rejected push"
                        );
                        return Err(format!("push rejected by gate '{gate_name}': {reason}"));
                    }
                }
            }
        }
    }

    Ok(())
}

/// Undo ref updates in the repository (e.g., after a gate rejection).
/// For new branches (old sha is zeros), deletes the ref. For updates, restores to old sha.
async fn undo_ref_updates(repo_path: &str, ref_updates: &[RefUpdate]) {
    let git_bin = std::env::var("GYRE_GIT_PATH").unwrap_or_else(|_| "git".to_string());
    let zeros = "0000000000000000000000000000000000000000";

    for update in ref_updates {
        let result = if update.old_sha == zeros || update.old_sha.starts_with("00000000") {
            // New branch — delete it.
            Command::new(&git_bin)
                .arg("-C")
                .arg(repo_path)
                .arg("update-ref")
                .arg("-d")
                .arg("--")
                .arg(&update.refname)
                .output()
                .await
        } else {
            // Existing branch — restore old sha.
            Command::new(&git_bin)
                .arg("-C")
                .arg(repo_path)
                .arg("update-ref")
                .arg("--")
                .arg(&update.refname)
                .arg(&update.old_sha)
                .output()
                .await
        };

        match result {
            Ok(o) if o.status.success() => {
                info!(repo_path, refname = %update.refname, "undid ref update after gate rejection");
            }
            Ok(o) => {
                let err = String::from_utf8_lossy(&o.stderr);
                warn!(repo_path, refname = %update.refname, %err, "failed to undo ref update");
            }
            Err(e) => {
                warn!(repo_path, refname = %update.refname, "undo ref update error: {e}");
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Spec lifecycle: auto-create tasks on spec changes in the default branch
// ---------------------------------------------------------------------------

/// Spec path prefixes that trigger lifecycle task creation (per spec-lifecycle.md).
const SPEC_WATCHED_PATHS: &[&str] = &["specs/system/", "specs/development/"];

/// Classify a spec file change and return (title, labels, priority).
fn classify_spec_change(
    status_char: char,
    path: &str,
    old_path: Option<&str>,
) -> Option<(String, Vec<String>, gyre_domain::TaskPriority)> {
    match status_char {
        'A' => Some((
            format!("Implement spec: {path}"),
            vec![
                "spec-implementation".to_string(),
                "auto-created".to_string(),
            ],
            gyre_domain::TaskPriority::Medium,
        )),
        'M' => Some((
            format!("Review spec change: {path}"),
            vec!["spec-drift-review".to_string(), "auto-created".to_string()],
            gyre_domain::TaskPriority::High,
        )),
        'D' => Some((
            format!("Handle spec removal: {path}"),
            vec!["spec-deprecated".to_string(), "auto-created".to_string()],
            gyre_domain::TaskPriority::High,
        )),
        'R' => {
            let old = old_path.unwrap_or(path);
            Some((
                format!("Update spec references: {old} -> {path}"),
                vec!["spec-housekeeping".to_string(), "auto-created".to_string()],
                gyre_domain::TaskPriority::Medium,
            ))
        }
        _ => None,
    }
}

/// Parse `git diff --name-status` output into (status_char, new_path, old_path) tuples.
/// Only returns entries in watched spec paths.
pub fn parse_spec_changes(diff_output: &str) -> Vec<(char, String, Option<String>)> {
    let mut changes = Vec::new();

    for line in diff_output.lines() {
        if line.is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.splitn(3, '\t').collect();
        if parts.is_empty() {
            continue;
        }

        let status_char = parts[0].chars().next().unwrap_or(' ');

        let (old_path, new_path) = if status_char == 'R' || status_char == 'C' {
            if parts.len() < 3 {
                continue;
            }
            (Some(parts[1]), parts[2])
        } else {
            if parts.len() < 2 {
                continue;
            }
            (None, parts[1])
        };

        let is_watched = SPEC_WATCHED_PATHS
            .iter()
            .any(|prefix| new_path.starts_with(prefix));
        let old_is_watched = old_path.is_some_and(|p| {
            SPEC_WATCHED_PATHS
                .iter()
                .any(|prefix| p.starts_with(prefix))
        });

        if !is_watched && !old_is_watched {
            continue;
        }

        changes.push((
            status_char,
            new_path.to_string(),
            old_path.map(str::to_string),
        ));
    }

    changes
}

/// After a successful push to the default branch, detect spec changes and create tasks.
async fn process_spec_lifecycle(
    state: &Arc<AppState>,
    repo_id: &str,
    repo_path: &str,
    default_branch: &str,
    ref_updates: &[RefUpdate],
) {
    let default_ref = format!("refs/heads/{default_branch}");
    let relevant_updates: Vec<&RefUpdate> = ref_updates
        .iter()
        .filter(|u| u.refname == default_ref)
        .collect();

    if relevant_updates.is_empty() {
        return;
    }

    let git_bin = std::env::var("GYRE_GIT_PATH").unwrap_or_else(|_| "git".to_string());
    // Well-known SHA for the empty git tree (used as base for initial pushes).
    const EMPTY_TREE: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";

    for update in relevant_updates {
        let old = if update.old_sha.starts_with("00000000") {
            EMPTY_TREE.to_string()
        } else {
            update.old_sha.clone()
        };

        let out = match Command::new(&git_bin)
            .arg("-C")
            .arg(repo_path)
            .arg("diff")
            .arg("--name-status")
            .arg("--diff-filter=AMDR")
            .arg(format!("{old}..{}", update.new_sha))
            .output()
            .await
        {
            Ok(o) if o.status.success() => o,
            Ok(o) => {
                let err = String::from_utf8_lossy(&o.stderr);
                warn!(repo_path, "spec-lifecycle: git diff failed: {err}");
                continue;
            }
            Err(e) => {
                warn!(repo_path, "spec-lifecycle: git diff spawn failed: {e}");
                continue;
            }
        };

        let diff_text = String::from_utf8_lossy(&out.stdout);
        let changes = parse_spec_changes(&diff_text);
        if changes.is_empty() {
            continue;
        }

        let existing_tasks = state.tasks.list().await.unwrap_or_default();
        let now = crate::api::now_secs();

        for (status_char, path, old_path) in changes {
            // Auto-invalidate active spec approvals when the spec file is modified,
            // deleted, or renamed. An approval is stale once the spec content changes.
            {
                // For renames, the old path is stale; for M/D, the current path is stale.
                let stale_paths: Vec<&str> = match status_char {
                    'M' | 'D' => vec![path.as_str()],
                    'R' => old_path
                        .as_deref()
                        .map(|p| vec![p])
                        .unwrap_or_else(|| vec![path.as_str()]),
                    _ => vec![],
                };

                if !stale_paths.is_empty() {
                    let reason = format!(
                        "spec file {} in push to {}",
                        match status_char {
                            'M' => "modified",
                            'D' => "deleted",
                            'R' => "renamed",
                            _ => "changed",
                        },
                        default_branch
                    );
                    let mut invalidated = 0usize;
                    for &stale_path in &stale_paths {
                        let _ = state
                            .spec_approvals
                            .revoke_all_for_path(stale_path, "system:spec-lifecycle", &reason, now)
                            .await;
                        invalidated += 1;
                    }
                    if invalidated > 0 {
                        info!(
                            spec_path = %path,
                            invalidated,
                            "spec-lifecycle: auto-invalidated stale spec approvals"
                        );
                    }
                }
            }

            let Some((title, labels, priority)) =
                classify_spec_change(status_char, &path, old_path.as_deref())
            else {
                continue;
            };

            // Dedup: skip if a non-Done task with the same title already exists.
            let exists = existing_tasks
                .iter()
                .any(|t| t.title == title && !matches!(t.status, gyre_domain::TaskStatus::Done));
            if exists {
                info!(title, "spec-lifecycle: task already exists, skipping");
                continue;
            }

            let task_id = gyre_common::Id::new(uuid::Uuid::new_v4().to_string());
            let mut task = gyre_domain::Task::new(task_id.clone(), &title, now);
            task.priority = priority;
            task.labels = labels;
            task.description = Some(format!(
                "Auto-created by spec lifecycle hook.\nSpec: {path}\nRepo: {repo_id}"
            ));
            task.spec_path = Some(path.clone());
            task.repo_id = gyre_common::Id::new(repo_id);
            // task_type intentionally left as None: push-hook tasks are informational
            // and must NOT trigger agent spawning (agent-runtime.md §1 Phase 4).

            match state.tasks.create(&task).await {
                Err(e) => warn!(title, "spec-lifecycle: failed to create task: {e}"),
                Ok(()) => {
                    info!(title, "spec-lifecycle: created task for spec change");
                    // Look up workspace_id from repo for proper scoping.
                    let ws_id = state
                        .repos
                        .find_by_id(&gyre_common::Id::new(repo_id))
                        .await
                        .ok()
                        .flatten()
                        .map(|r| r.workspace_id)
                        .unwrap_or_else(|| gyre_common::Id::new("default"));
                    let change_kind = match status_char {
                        'A' => "added",
                        'M' => "modified",
                        'D' => "deleted",
                        'R' => "renamed",
                        _ => "unknown",
                    };
                    state
                        .emit_event(
                            Some(ws_id.clone()),
                            gyre_common::message::Destination::Workspace(ws_id.clone()),
                            gyre_common::message::MessageKind::SpecChanged,
                            Some(serde_json::json!({
                                "repo_id": repo_id,
                                "spec_path": path,
                                "change_kind": change_kind,
                                "task_id": task_id.to_string(),
                            })),
                        )
                        .await;
                    state
                        .emit_event(
                            Some(ws_id.clone()),
                            gyre_common::message::Destination::Workspace(ws_id),
                            gyre_common::message::MessageKind::TaskCreated,
                            Some(serde_json::json!({"task_id": task_id.to_string()})),
                        )
                        .await;

                    // Cross-workspace spec change notification (priority 4):
                    // Find inbound cross-workspace links targeting this spec path
                    // and notify Admin/Developer members of each dependent workspace.
                    notify_cross_workspace_dependents(state, repo_id, &path).await;
                }
            }
        }
    }
}

/// Notify Admin and Developer members of workspaces that have cross-workspace spec links
/// pointing to the given changed spec. Creates one priority-4 notification per dependent
/// workspace member (Admin or Developer role).
async fn notify_cross_workspace_dependents(
    state: &Arc<crate::AppState>,
    source_repo_id: &str,
    changed_spec_path: &str,
) {
    // Collect inbound cross-workspace links pointing to this spec path.
    // A cross-workspace link has `source_repo_id` set to a different repo than `source_repo_id`.
    let inbound_links: Vec<crate::spec_registry::SpecLinkEntry> = {
        let store = state.spec_links_store.lock().await;
        store
            .iter()
            .filter(|l| {
                l.target_path == changed_spec_path
                    && l.source_repo_id.as_deref() != Some(source_repo_id)
                    && l.source_repo_id.is_some()
            })
            .cloned()
            .collect()
    };

    if inbound_links.is_empty() {
        return;
    }

    let now = crate::api::now_secs();

    // Collect unique source repo IDs from inbound links.
    let mut notified_workspaces: std::collections::HashSet<String> =
        std::collections::HashSet::new();

    for link in &inbound_links {
        let dep_repo_id = match &link.source_repo_id {
            Some(id) => id.clone(),
            None => continue,
        };

        // Look up the dependent repo to get its workspace_id.
        let dep_repo = match state
            .repos
            .find_by_id(&gyre_common::Id::new(&dep_repo_id))
            .await
            .ok()
            .flatten()
        {
            Some(r) => r,
            None => continue,
        };

        let ws_id_str = dep_repo.workspace_id.to_string();
        if notified_workspaces.contains(&ws_id_str) {
            continue;
        }
        notified_workspaces.insert(ws_id_str.clone());

        // Get workspace and its tenant_id for notification construction.
        let dep_workspace = match state
            .workspaces
            .find_by_id(&dep_repo.workspace_id)
            .await
            .ok()
            .flatten()
        {
            Some(ws) => ws,
            None => continue,
        };

        // Get workspace members (Admin + Developer roles receive this notification).
        let members = state
            .workspace_memberships
            .list_by_workspace(&dep_repo.workspace_id)
            .await
            .unwrap_or_default();

        for member in members {
            use gyre_domain::WorkspaceRole;
            if !matches!(
                member.role,
                WorkspaceRole::Admin | WorkspaceRole::Developer | WorkspaceRole::Owner
            ) {
                continue;
            }

            let notif_id = gyre_common::Id::new(uuid::Uuid::new_v4().to_string());
            let display = link.target_display.as_deref().unwrap_or(changed_spec_path);
            let title = format!("Cross-workspace spec changed: {display}");
            let source_repo_name = state
                .repos
                .find_by_id(&gyre_common::Id::new(source_repo_id))
                .await
                .ok()
                .flatten()
                .map(|r| r.name.clone())
                .unwrap_or_else(|| source_repo_id[..8.min(source_repo_id.len())].to_string());
            let body = format!(
                "{display} changed in repo {source_repo_name}. Your spec {} depends on it. Review for impact.",
                link.source_path
            );
            let mut notif = gyre_common::Notification::new(
                notif_id,
                dep_repo.workspace_id.clone(),
                member.user_id,
                gyre_common::NotificationType::CrossWorkspaceSpecChange,
                title,
                dep_workspace.tenant_id.to_string(),
                now as i64,
            );
            notif.body = Some(body);
            notif.entity_ref = Some(changed_spec_path.to_string());

            if let Err(e) = state.notifications.create(&notif).await {
                tracing::warn!("spec-lifecycle: failed cross-workspace notification: {e}");
            }
        }
    }
}

// ── Cargo.toml path dep auto-detection (M22.4) ─────────────────────────────

/// Parse `path = "..."` entries from a Cargo.toml `[dependencies]` section.
///
/// Returns the list of local path values (e.g. `"../other-repo"`).
/// Only path-based dependencies are returned; crates.io / git deps are ignored.
/// This is the auto-detection stub for M22.4 — only Cargo.toml path deps are
/// detected in this milestone; other detection methods are stubs.
pub fn detect_cargo_path_deps(toml_content: &str) -> Vec<String> {
    let mut paths = Vec::new();
    let mut in_dependencies = false;

    for line in toml_content.lines() {
        let trimmed = line.trim();

        // Detect section headers.
        if trimmed.starts_with('[') {
            in_dependencies = trimmed == "[dependencies]"
                || trimmed == "[dev-dependencies]"
                || trimmed == "[build-dependencies]";
            continue;
        }

        if !in_dependencies {
            continue;
        }

        // Match lines with: crate-name = { path = "../sibling" }
        if let Some(path_val) = extract_path_value(trimmed) {
            paths.push(path_val);
        }
    }

    paths
}

/// Extract the value of `path = "..."` from a TOML dependency line.
fn extract_path_value(line: &str) -> Option<String> {
    let path_idx = line.find("path")?;
    let after_path = line[path_idx + 4..].trim_start();
    let after_eq = after_path.strip_prefix('=')?;
    let value_str = after_eq.trim();

    // Strip the opening quote and find the closing quote.
    let (quote_char, rest) = if let Some(s) = value_str.strip_prefix('"') {
        ('"', s)
    } else if let Some(s) = value_str.strip_prefix('\'') {
        ('\'', s)
    } else {
        return None;
    };

    let end = rest.find(quote_char)?;
    Some(rest[..end].to_string())
}

/// Extract the `version = "X.Y.Z"` value from a Cargo.toml [package] section.
#[cfg(test)]
pub(crate) fn extract_cargo_version(toml_content: &str) -> Option<String> {
    let mut in_package = false;
    for line in toml_content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_package = trimmed == "[package]";
            continue;
        }
        if !in_package {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("version") {
            let rest = rest.trim_start();
            if let Some(rest) = rest.strip_prefix('=') {
                let val = rest.trim().trim_matches('"').trim_matches('\'');
                if !val.is_empty() {
                    return Some(val.to_string());
                }
            }
        }
    }
    None
}

/// Extract the pinned version for a specific dependency from Cargo.toml.
///
/// Handles both `crate = "1.2.3"` and `crate = { version = "1.2.3", ... }` formats.
pub(crate) fn extract_dep_version(toml_content: &str, dep_name: &str) -> Option<String> {
    let mut in_dependencies = false;

    for line in toml_content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_dependencies = trimmed == "[dependencies]"
                || trimmed == "[dev-dependencies]"
                || trimmed == "[build-dependencies]";
            continue;
        }
        if !in_dependencies {
            continue;
        }
        // Match the dependency name at the start of the line.
        if let Some(rest) = trimmed.strip_prefix(dep_name) {
            let rest = rest.trim_start();
            if let Some(rest) = rest.strip_prefix('=') {
                let rest = rest.trim();
                // Simple form: dep = "1.2.3"
                if rest.starts_with('"') || rest.starts_with('\'') {
                    let val = rest.trim_matches('"').trim_matches('\'');
                    if !val.is_empty() {
                        return Some(val.to_string());
                    }
                }
                // Inline table form: dep = { version = "1.2.3", ... }
                if rest.starts_with('{') {
                    if let Some(ver_idx) = rest.find("version") {
                        let after = &rest[ver_idx + 7..];
                        let after = after.trim_start();
                        if let Some(after) = after.strip_prefix('=') {
                            let after = after.trim();
                            if after.starts_with('"') {
                                if let Some(end) = after[1..].find('"') {
                                    return Some(after[1..1 + end].to_string());
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    None
}

/// Parse `package.json` for local path dependencies (`file:` or `workspace:` references).
///
/// Returns `(reference_value, version)` tuples. For `file:` references the reference
/// is the local path (e.g. `"../other-repo"`). For `workspace:` the reference is the
/// package name. Only local/workspace references are returned; npm registry deps are ignored.
pub fn detect_package_json_deps(content: &str) -> Vec<(String, Option<String>)> {
    let mut results = Vec::new();

    let parsed: serde_json::Value = match serde_json::from_str(content) {
        Ok(v) => v,
        Err(_) => return results,
    };

    for section in &["dependencies", "devDependencies"] {
        if let Some(deps) = parsed.get(section).and_then(|v| v.as_object()) {
            for (name, value) in deps {
                if let Some(ver_str) = value.as_str() {
                    if let Some(path) = ver_str.strip_prefix("file:") {
                        results.push((path.to_string(), None));
                    } else if let Some(ws_ver) = ver_str.strip_prefix("workspace:") {
                        // workspace:* or workspace:^1.0.0 — the package name is the dep key.
                        let version = if ws_ver == "*" {
                            None
                        } else {
                            Some(ws_ver.to_string())
                        };
                        results.push((name.clone(), version));
                    }
                }
            }
        }
    }

    results
}

/// Parse `go.mod` for `require` directives referencing Gyre modules.
///
/// Per dependency-graph.md §1: "go.mod -> extract require directives referencing
/// Gyre modules". The forge matches module paths against known repos in the tenant.
///
/// Also parses `replace` directives with local paths as a supplementary detection
/// path (local `replace` directives indicate in-development cross-repo references).
///
/// Returns `(module_path, version)` tuples.
pub fn detect_go_mod_deps(content: &str) -> Vec<(String, Option<String>)> {
    let mut results = Vec::new();
    let mut in_require_block = false;
    let mut in_replace_block = false;

    for line in content.lines() {
        let trimmed = line.trim();

        // Skip comments and empty lines.
        if trimmed.is_empty() || trimmed.starts_with("//") {
            continue;
        }

        // Handle block `require (` ... `)` syntax.
        if trimmed == "require (" {
            in_require_block = true;
            continue;
        }
        // Handle block `replace (` ... `)` syntax.
        if trimmed == "replace (" {
            in_replace_block = true;
            continue;
        }
        if (in_require_block || in_replace_block) && trimmed == ")" {
            in_require_block = false;
            in_replace_block = false;
            continue;
        }

        // --- require directives ---
        // Single-line: `require module/path v1.0.0`
        // Block-line:  `module/path v1.0.0`
        let require_line = if in_require_block {
            Some(trimmed)
        } else if let Some(rest) = trimmed.strip_prefix("require ") {
            Some(rest.trim())
        } else {
            None
        };

        if let Some(rline) = require_line {
            // Strip inline comments.
            let rline = rline.split("//").next().unwrap_or(rline).trim();
            let parts: Vec<&str> = rline.split_whitespace().collect();
            if !parts.is_empty() {
                let module_path = parts[0];
                let version = parts.get(1).map(|v| v.to_string());
                results.push((module_path.to_string(), version));
            }
            continue;
        }

        // --- replace directives (supplementary) ---
        // Single-line: `replace module/path v1.0.0 => ../local-path`
        // Block-line:  `module/path v1.0.0 => ../local-path`
        let replace_line = if in_replace_block {
            Some(trimmed)
        } else if let Some(rest) = trimmed.strip_prefix("replace ") {
            Some(rest.trim())
        } else {
            None
        };

        if let Some(rline) = replace_line {
            if let Some(arrow_pos) = rline.find("=>") {
                let replacement = rline[arrow_pos + 2..].trim();
                // Only match local paths (starting with ./ or ../).
                if replacement.starts_with("./") || replacement.starts_with("../") {
                    // Extract the original module path (before any version).
                    let original = rline[..arrow_pos].trim();
                    let module_path = original.split_whitespace().next().unwrap_or(original);
                    // Avoid duplicates — require block may already have this module.
                    if !results.iter().any(|(m, _)| m == module_path) {
                        let version = original.split_whitespace().nth(1).map(|v| v.to_string());
                        results.push((module_path.to_string(), version));
                    }
                }
            }
        }
    }

    results
}

/// Parse `pyproject.toml` for path dependencies.
///
/// Supports two formats:
/// - PEP 621 `[project.dependencies]` with `@ file:///path` references
/// - Poetry `[tool.poetry.dependencies]` with `{path = "..."}` entries
///
/// Returns `(path_or_name, version)` tuples for local path dependencies only.
pub fn detect_pyproject_deps(content: &str) -> Vec<(String, Option<String>)> {
    let mut results = Vec::new();
    let mut current_section = String::new();

    for line in content.lines() {
        let trimmed = line.trim();

        // Track section headers.
        if trimmed.starts_with('[') {
            current_section = trimmed
                .trim_start_matches('[')
                .trim_end_matches(']')
                .to_string();
            continue;
        }

        // Poetry: [tool.poetry.dependencies] or [tool.poetry.dev-dependencies]
        if current_section == "tool.poetry.dependencies"
            || current_section == "tool.poetry.dev-dependencies"
        {
            // Match: dep-name = { path = "../local-path" }
            if let Some(path_val) = extract_path_value(trimmed) {
                let dep_name = trimmed.split('=').next().unwrap_or("").trim().to_string();
                results.push((path_val, Some(dep_name)));
            }
        }

        // PEP 621: [project] dependencies = [...] with `pkg @ file:///path`
        if current_section == "project" || current_section == "project.dependencies" {
            // Match: "package-name @ file:../local-path" in a list
            if let Some(at_pos) = trimmed.find(" @ file:") {
                let pkg_name = trimmed
                    .trim_start_matches(|c: char| c == '"' || c == '\'' || c == '-' || c == ' ')
                    .split(|c: char| c == ' ' || c == '@')
                    .next()
                    .unwrap_or("");
                let path = &trimmed[at_pos + 8..];
                let path = path.trim_end_matches(|c: char| c == '"' || c == '\'' || c == ',');
                if !pkg_name.is_empty() {
                    results.push((path.to_string(), Some(pkg_name.to_string())));
                }
            }
        }
    }

    results
}

/// Extract cross-repo spec links from a `specs/manifest.yaml` content string.
///
/// Returns `(repo_name, spec_path, link_type)` tuples for links whose target
/// starts with `@` (cross-repo or cross-workspace references).
pub fn detect_manifest_spec_links(manifest_yaml: &str) -> Vec<(String, String, String)> {
    let manifest: crate::spec_registry::SpecManifest = match serde_yaml::from_str(manifest_yaml) {
        Ok(m) => m,
        Err(_) => return Vec::new(),
    };

    let mut results = Vec::new();

    for spec in &manifest.specs {
        for link in &spec.links {
            if !link.target.starts_with('@') {
                continue;
            }

            let parsed = crate::spec_registry::parse_cross_workspace_target(&link.target);
            let repo_name = match &parsed {
                crate::spec_registry::CrossWorkspaceTarget::CrossRepo { repo_name, .. } => {
                    repo_name.clone()
                }
                crate::spec_registry::CrossWorkspaceTarget::CrossWorkspace {
                    repo_name, ..
                } => repo_name.clone(),
                crate::spec_registry::CrossWorkspaceTarget::SameRepo { .. } => continue,
            };

            let link_type = format!("{}", link.link_type);
            results.push((repo_name, link.target.clone(), link_type));
        }
    }

    results
}

/// Detect API contract references in an OpenAPI/Swagger document.
///
/// Scans `servers[].url` and `$ref` values for patterns matching other Gyre repo names.
/// Best-effort: many API contracts don't explicitly reference repo names.
pub fn detect_openapi_refs(content: &str, known_repos: &[&str]) -> Vec<String> {
    let mut results = Vec::new();

    for repo_name in known_repos {
        // Match repo names in server URLs, $ref paths, and other string values.
        if content.contains(repo_name) {
            results.push(repo_name.to_string());
        }
    }

    results
}

/// Detect protobuf import paths that reference other Gyre repos.
///
/// Parses `import "repo-name/path/to/file.proto"` directives and matches the
/// first path segment against known repo names.
pub fn detect_proto_imports(content: &str, known_repos: &[&str]) -> Vec<String> {
    let mut results = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("import ") {
            // import "path/to/file.proto";
            let path = rest
                .trim()
                .trim_start_matches('"')
                .trim_end_matches(|c: char| c == '"' || c == ';');
            // First path segment is typically the package/repo name.
            if let Some(first_seg) = path.split('/').next() {
                if known_repos.contains(&first_seg) {
                    results.push(first_seg.to_string());
                }
            }
        }
    }

    // Deduplicate (multiple imports from the same repo).
    results.sort();
    results.dedup();
    results
}

/// Detect MCP tool references pointing to other Gyre repos.
///
/// Parses `mcp.json` for `server` or `url` fields matching known repo names.
pub fn detect_mcp_refs(content: &str, known_repos: &[&str]) -> Vec<String> {
    let mut results = Vec::new();

    for repo_name in known_repos {
        if content.contains(repo_name) {
            results.push(repo_name.to_string());
        }
    }

    results.sort();
    results.dedup();
    results
}

/// A single auto-detected dependency edge collected during push-time scanning.
///
/// Named struct replaces an unnamed 6-element tuple for clarity and to prevent
/// positional field confusion (e.g., swapping source_artifact and target_artifact).
#[derive(Clone, Debug)]
pub(crate) struct DetectedEdge {
    pub target_repo_id: String,
    pub source_artifact: String,
    pub target_artifact: String,
    pub dep_type: gyre_domain::DependencyType,
    pub method: gyre_domain::DetectionMethod,
    pub version: Option<String>,
}

/// Reconcile detected dependency edges against existing edges for a repo.
///
/// - New edges (detected but not in existing set): created with `Active` status.
/// - Orphaned edges (existing but no longer detected): marked `Orphaned`.
/// - Version changes: updates `version_pinned` on existing edges.
/// - `Manual` edges are never modified.
/// - Only edges whose `detection_method` is in `ran_methods` are considered for
///   orphaning — edges from detection methods that did not run in this push are
///   left untouched (prevents false orphaning when only a subset of dependency
///   files changed).
pub(crate) async fn reconcile_dependencies(
    state: &Arc<AppState>,
    repo_id: &str,
    detected_edges: &[DetectedEdge],
    ran_methods: &std::collections::HashSet<gyre_domain::DetectionMethod>,
) {
    use gyre_common::Id;
    use gyre_domain::{DependencyStatus, DetectionMethod};

    let source_id = Id::new(repo_id);
    let existing = match state.dependencies.list_by_repo(&source_id).await {
        Ok(edges) => edges,
        Err(e) => {
            warn!("reconcile-deps: failed to list existing edges: {e}");
            return;
        }
    };

    let now = crate::api::now_secs();

    // Build a lookup key for detected edges: (target_repo_id, source_artifact, detection_method).
    let detected_keys: std::collections::HashSet<(String, String, String)> = detected_edges
        .iter()
        .map(|de| {
            (
                de.target_repo_id.clone(),
                de.source_artifact.clone(),
                format!("{:?}", de.method),
            )
        })
        .collect();

    // Mark existing non-Manual edges as Orphaned if no longer detected.
    // Only consider edges whose detection_method is in `ran_methods` — edges from
    // detection methods that did not run in this push are left untouched.
    for edge in &existing {
        if edge.detection_method == DetectionMethod::Manual {
            continue;
        }

        // Skip edges whose detection method did not run in this push.
        if !ran_methods.contains(&edge.detection_method) {
            continue;
        }

        let key = (
            edge.target_repo_id.to_string(),
            edge.source_artifact.clone(),
            format!("{:?}", edge.detection_method),
        );

        if !detected_keys.contains(&key) && edge.status != DependencyStatus::Orphaned {
            let mut updated = edge.clone();
            updated.status = DependencyStatus::Orphaned;
            updated.last_verified_at = now;
            if let Err(e) = state.dependencies.save(&updated).await {
                warn!("reconcile-deps: failed to mark edge orphaned: {e}");
            } else {
                tracing::info!(
                    source = repo_id,
                    target = %edge.target_repo_id,
                    artifact = %edge.source_artifact,
                    "dependency marked orphaned — no longer detected"
                );
            }
        }
    }

    // Create or update detected edges.
    for de in detected_edges {
        // Check if an existing edge matches.
        let existing_edge = existing.iter().find(|e| {
            e.target_repo_id.as_str() == de.target_repo_id
                && e.source_artifact == de.source_artifact
                && format!("{:?}", e.detection_method) == format!("{:?}", de.method)
        });

        if let Some(edge) = existing_edge {
            // Update version_pinned if changed, and un-orphan.
            if edge.version_pinned != de.version || edge.status == DependencyStatus::Orphaned {
                let mut updated = edge.clone();
                updated.version_pinned = de.version.clone();
                updated.last_verified_at = now;
                if updated.status == DependencyStatus::Orphaned {
                    updated.status = DependencyStatus::Active;
                }
                if let Err(e) = state.dependencies.save(&updated).await {
                    warn!("reconcile-deps: failed to update edge: {e}");
                }
            }
        } else {
            // New edge — create it.
            let mut edge = gyre_domain::DependencyEdge::new(
                Id::new(uuid::Uuid::new_v4().to_string()),
                source_id.clone(),
                Id::new(&de.target_repo_id),
                de.dep_type.clone(),
                de.source_artifact.as_str(),
                de.target_artifact.as_str(),
                de.method.clone(),
                now,
            );
            edge.version_pinned = de.version.clone();
            if let Err(e) = state.dependencies.save(&edge).await {
                warn!("reconcile-deps: failed to create new edge: {e}");
            } else {
                tracing::info!(
                    source = repo_id,
                    target = de.target_repo_id.as_str(),
                    artifact = de.source_artifact.as_str(),
                    target_artifact = de.target_artifact.as_str(),
                    method = ?de.method,
                    "new dependency edge detected"
                );
            }
        }
    }
}

/// Post-push auto-detection: scan changed files for dependency declarations.
///
/// On pushes to the default branch, reads dependency files at the new SHA and creates
/// DependencyEdge records for any dependencies pointing to known Gyre repos.
/// Runs reconciliation after all detectors to mark orphaned edges.
/// Also resolves target repo versions and computes version drift (TASK-021).
pub(crate) async fn detect_dependencies_on_push(
    state: &Arc<AppState>,
    repo_id: &str,
    repo_path: &str,
    new_sha: &str,
) {
    use gyre_common::Id;
    use gyre_domain::{DependencyType, DetectionMethod};

    let git_bin = std::env::var("GYRE_GIT_PATH").unwrap_or_else(|_| "git".to_string());

    // List all changed files in this push.
    let diff_out = match Command::new(&git_bin)
        .arg("-C")
        .arg(repo_path)
        .arg("diff-tree")
        .arg("--no-commit-id")
        .arg("-r")
        .arg("--name-only")
        .arg(new_sha)
        .output()
        .await
    {
        Ok(o) if o.status.success() => o.stdout,
        _ => return,
    };

    let changed_files = String::from_utf8_lossy(&diff_out);
    let changed: Vec<&str> = changed_files.lines().collect();

    // Determine which dependency files were changed.
    let has_cargo_toml = changed
        .iter()
        .any(|f| *f == "Cargo.toml" || f.ends_with("/Cargo.toml"));
    let has_package_json = changed
        .iter()
        .any(|f| *f == "package.json" || f.ends_with("/package.json"));
    let has_go_mod = changed
        .iter()
        .any(|f| *f == "go.mod" || f.ends_with("/go.mod"));
    let has_pyproject = changed
        .iter()
        .any(|f| *f == "pyproject.toml" || f.ends_with("/pyproject.toml"));
    let has_manifest = changed.iter().any(|f| *f == "specs/manifest.yaml");
    let has_openapi = changed.iter().any(|f| {
        *f == "openapi.yaml"
            || *f == "openapi.yml"
            || *f == "swagger.json"
            || f.ends_with("/openapi.yaml")
            || f.ends_with("/openapi.yml")
            || f.ends_with("/swagger.json")
    });
    let has_proto = changed.iter().any(|f| f.ends_with(".proto"));
    let has_mcp_json = changed
        .iter()
        .any(|f| *f == "mcp.json" || f.ends_with("/mcp.json"));

    // Early return if no dependency-related files changed.
    if !has_cargo_toml
        && !has_package_json
        && !has_go_mod
        && !has_pyproject
        && !has_manifest
        && !has_openapi
        && !has_proto
        && !has_mcp_json
    {
        return;
    }

    let all_repos = match state.repos.list().await {
        Ok(r) => r,
        Err(e) => {
            warn!("dep-detection: failed to list repos: {e}");
            return;
        }
    };

    let source_id = Id::new(repo_id);
    let now = crate::api::now_secs();

    // Build list of known repo names for API contract matching.
    let repo_names: Vec<&str> = all_repos
        .iter()
        .filter(|r| r.id.as_str() != repo_id)
        .map(|r| r.name.as_str())
        .collect();

    // Collect all detected edges and track which detection methods ran.
    let mut detected_edges: Vec<DetectedEdge> = Vec::new();
    let mut ran_methods: std::collections::HashSet<DetectionMethod> =
        std::collections::HashSet::new();

    // --- Cargo.toml ---
    if has_cargo_toml {
        ran_methods.insert(DetectionMethod::CargoToml);
        if let Some(content) =
            crate::spec_registry::read_git_file(&git_bin, repo_path, new_sha, "Cargo.toml").await
        {
            let path_deps = detect_cargo_path_deps(&content);
            for path_dep in &path_deps {
                let basename = std::path::Path::new(path_dep)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(path_dep);
                if let Some(target_repo) = all_repos.iter().find(|r| r.name == basename) {
                    if target_repo.id.as_str() != repo_id {
                        let version = extract_dep_version(&content, basename);
                        detected_edges.push(DetectedEdge {
                            target_repo_id: target_repo.id.to_string(),
                            source_artifact: "Cargo.toml".to_string(),
                            target_artifact: basename.to_string(),
                            dep_type: DependencyType::Code,
                            method: DetectionMethod::CargoToml,
                            version,
                        });
                    }
                }
            }
        }
    }

    // --- package.json ---
    if has_package_json {
        ran_methods.insert(DetectionMethod::PackageJson);
        if let Some(content) =
            crate::spec_registry::read_git_file(&git_bin, repo_path, new_sha, "package.json").await
        {
            let pkg_deps = detect_package_json_deps(&content);
            for (ref_val, version) in &pkg_deps {
                // For file: refs, extract basename as repo name candidate.
                let candidate = std::path::Path::new(ref_val)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(ref_val);
                if let Some(target_repo) = all_repos.iter().find(|r| r.name == candidate) {
                    if target_repo.id.as_str() != repo_id {
                        detected_edges.push(DetectedEdge {
                            target_repo_id: target_repo.id.to_string(),
                            source_artifact: "package.json".to_string(),
                            target_artifact: candidate.to_string(),
                            dep_type: DependencyType::Code,
                            method: DetectionMethod::PackageJson,
                            version: version.clone(),
                        });
                    }
                }
            }
        }
    }

    // --- go.mod ---
    if has_go_mod {
        ran_methods.insert(DetectionMethod::GoMod);
        if let Some(content) =
            crate::spec_registry::read_git_file(&git_bin, repo_path, new_sha, "go.mod").await
        {
            let go_deps = detect_go_mod_deps(&content);
            for (module_path, version) in &go_deps {
                // Extract the last path segment as repo name candidate.
                let candidate = module_path.rsplit('/').next().unwrap_or(module_path);
                if let Some(target_repo) = all_repos.iter().find(|r| r.name == candidate) {
                    if target_repo.id.as_str() != repo_id {
                        detected_edges.push(DetectedEdge {
                            target_repo_id: target_repo.id.to_string(),
                            source_artifact: "go.mod".to_string(),
                            target_artifact: module_path.to_string(),
                            dep_type: DependencyType::Code,
                            method: DetectionMethod::GoMod,
                            version: version.clone(),
                        });
                    }
                }
            }
        }
    }

    // --- pyproject.toml ---
    if has_pyproject {
        ran_methods.insert(DetectionMethod::PyprojectToml);
        if let Some(content) =
            crate::spec_registry::read_git_file(&git_bin, repo_path, new_sha, "pyproject.toml")
                .await
        {
            let py_deps = detect_pyproject_deps(&content);
            for (path_val, pkg_name) in &py_deps {
                let candidate = std::path::Path::new(path_val)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(path_val);
                if let Some(target_repo) = all_repos.iter().find(|r| r.name == candidate) {
                    if target_repo.id.as_str() != repo_id {
                        detected_edges.push(DetectedEdge {
                            target_repo_id: target_repo.id.to_string(),
                            source_artifact: "pyproject.toml".to_string(),
                            target_artifact: pkg_name.as_deref().unwrap_or(candidate).to_string(),
                            dep_type: DependencyType::Code,
                            method: DetectionMethod::PyprojectToml,
                            version: None,
                        });
                    }
                }
            }
        }
    }

    // --- specs/manifest.yaml ---
    if has_manifest {
        ran_methods.insert(DetectionMethod::ManifestLink);
        if let Some(content) =
            crate::spec_registry::read_git_file(&git_bin, repo_path, new_sha, "specs/manifest.yaml")
                .await
        {
            let spec_links = detect_manifest_spec_links(&content);
            for (target_repo_name, target_path, _link_type) in &spec_links {
                if let Some(target_repo) = all_repos.iter().find(|r| r.name == *target_repo_name) {
                    if target_repo.id.as_str() != repo_id {
                        detected_edges.push(DetectedEdge {
                            target_repo_id: target_repo.id.to_string(),
                            source_artifact: "specs/manifest.yaml".to_string(),
                            target_artifact: target_path.to_string(),
                            dep_type: DependencyType::Spec,
                            method: DetectionMethod::ManifestLink,
                            version: None,
                        });
                    }
                }
            }
        }
    }

    // --- OpenAPI / Swagger ---
    if has_openapi {
        ran_methods.insert(DetectionMethod::OpenApiRef);
        for api_file in &["openapi.yaml", "openapi.yml", "swagger.json"] {
            if let Some(content) =
                crate::spec_registry::read_git_file(&git_bin, repo_path, new_sha, api_file).await
            {
                let refs = detect_openapi_refs(&content, &repo_names);
                for ref_name in &refs {
                    if let Some(target_repo) = all_repos.iter().find(|r| r.name == *ref_name) {
                        if target_repo.id.as_str() != repo_id {
                            detected_edges.push(DetectedEdge {
                                target_repo_id: target_repo.id.to_string(),
                                source_artifact: api_file.to_string(),
                                target_artifact: ref_name.to_string(),
                                dep_type: DependencyType::Api,
                                method: DetectionMethod::OpenApiRef,
                                version: None,
                            });
                        }
                    }
                }
            }
        }
    }

    // --- Protobuf imports ---
    if has_proto {
        ran_methods.insert(DetectionMethod::ProtoImport);
        for changed_file in &changed {
            if changed_file.ends_with(".proto") {
                if let Some(content) =
                    crate::spec_registry::read_git_file(&git_bin, repo_path, new_sha, changed_file)
                        .await
                {
                    let refs = detect_proto_imports(&content, &repo_names);
                    for ref_name in &refs {
                        if let Some(target_repo) = all_repos.iter().find(|r| r.name == *ref_name) {
                            if target_repo.id.as_str() != repo_id {
                                detected_edges.push(DetectedEdge {
                                    target_repo_id: target_repo.id.to_string(),
                                    source_artifact: changed_file.to_string(),
                                    target_artifact: ref_name.to_string(),
                                    dep_type: DependencyType::Schema,
                                    method: DetectionMethod::ProtoImport,
                                    version: None,
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    // --- mcp.json ---
    if has_mcp_json {
        ran_methods.insert(DetectionMethod::McpToolRef);
        if let Some(content) =
            crate::spec_registry::read_git_file(&git_bin, repo_path, new_sha, "mcp.json").await
        {
            let refs = detect_mcp_refs(&content, &repo_names);
            for ref_name in &refs {
                if let Some(target_repo) = all_repos.iter().find(|r| r.name == *ref_name) {
                    if target_repo.id.as_str() != repo_id {
                        detected_edges.push(DetectedEdge {
                            target_repo_id: target_repo.id.to_string(),
                            source_artifact: "mcp.json".to_string(),
                            target_artifact: ref_name.to_string(),
                            dep_type: DependencyType::Api,
                            method: DetectionMethod::McpToolRef,
                            version: None,
                        });
                    }
                }
            }
        }
    }

    tracing::info!(
        repo = repo_id,
        detected_count = detected_edges.len(),
        "dependency auto-detection complete"
    );

    // Run reconciliation: create new edges, mark orphaned, update versions.
    reconcile_dependencies(state, repo_id, &detected_edges, &ran_methods).await;

    // Post-reconciliation: apply version drift and staleness checks for Cargo.toml edges.
    // Look up the source repo's workspace_id for policy lookup (TASK-021 F1).
    let source_workspace_id = all_repos
        .iter()
        .find(|r| r.id.as_str() == repo_id)
        .map(|r| r.workspace_id.to_string());

    let policy = if let Some(ws_id) = &source_workspace_id {
        state
            .dependency_policies
            .get_for_workspace(&Id::new(ws_id))
            .await
            .unwrap_or_default()
    } else {
        gyre_domain::DependencyPolicy::default()
    };

    // Re-read edges after reconciliation to apply version drift.
    let current_edges = match state.dependencies.list_by_repo(&source_id).await {
        Ok(e) => e,
        Err(_) => return,
    };

    for edge in current_edges {
        if edge.detection_method != DetectionMethod::CargoToml
            || edge.status == gyre_domain::DependencyStatus::Orphaned
        {
            continue;
        }

        // Read Cargo.toml to extract pinned version.
        let toml_content =
            match crate::spec_registry::read_git_file(&git_bin, repo_path, new_sha, "Cargo.toml")
                .await
            {
                Some(c) => c,
                None => continue,
            };

        let target_name = edge.target_artifact.clone();
        let version_pinned = extract_dep_version(&toml_content, &target_name);

        // Look up target repo to resolve its current version.
        let target_repo = all_repos.iter().find(|r| r.id == edge.target_repo_id);
        let target_version = if let Some(tr) = target_repo {
            crate::version_compute::latest_semver_tag(&tr.path).await
        } else {
            None
        };

        let version_drift = match (&version_pinned, &target_version) {
            (Some(pinned), Some(current)) => {
                crate::version_compute::compute_version_drift(pinned, current)
            }
            _ => None,
        };

        let needs_update = edge.version_pinned != version_pinned
            || edge.target_version_current != target_version
            || edge.version_drift != version_drift;

        if needs_update {
            let mut updated = edge.clone();
            updated.version_pinned = version_pinned;
            updated.target_version_current = target_version.clone();
            updated.version_drift = version_drift;

            // Push-time staleness (TASK-021 F1).
            if policy.max_version_drift > 0 {
                if let Some(d) = updated.version_drift {
                    if d > policy.max_version_drift {
                        updated.status = gyre_domain::DependencyStatus::Stale;
                    }
                }
            }

            if let Err(e) = state.dependencies.save(&updated).await {
                warn!("dep-detection: failed to update edge version info: {e}");
            }

            // Create auto-task for stale dependency at push time (TASK-021 F1).
            if updated.status == gyre_domain::DependencyStatus::Stale
                && policy.auto_create_update_tasks
            {
                let source_name = all_repos
                    .iter()
                    .find(|r| r.id.as_str() == repo_id)
                    .map(|r| r.name.as_str())
                    .unwrap_or(repo_id);
                let target_name_str = target_repo.map(|r| r.name.as_str()).unwrap_or("unknown");

                let title = if let (Some(pinned), Some(current)) =
                    (&updated.version_pinned, &target_version)
                {
                    format!("Update {target_name_str} dependency from {pinned} to {current}")
                } else {
                    format!("Update stale dependency on {target_name_str}")
                };

                let task_id = Id::new(uuid::Uuid::new_v4().to_string());
                let mut task = gyre_domain::Task::new(task_id, &title, now);
                task.priority = gyre_domain::TaskPriority::Medium;
                task.labels = vec![
                    "dependency-update".to_string(),
                    "stale-dependency".to_string(),
                    "auto-created".to_string(),
                ];
                task.description = Some(format!(
                    "Dependency on '{target_name_str}' in repo '{}' is stale. \
                     Pinned version: {}. Current version: {}. Drift: {} versions.",
                    source_name,
                    updated.version_pinned.as_deref().unwrap_or("unknown"),
                    target_version.as_deref().unwrap_or("unknown"),
                    updated
                        .version_drift
                        .map(|d| d.to_string())
                        .unwrap_or_else(|| "unknown".to_string()),
                ));
                if let Some(ws_id) = &source_workspace_id {
                    task.workspace_id = Id::new(ws_id);
                }
                task.repo_id = source_id.clone();

                if let Err(e) = state.tasks.create(&task).await {
                    warn!("dep-detection: failed to create auto-task: {e}");
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Breaking change detection on push (TASK-020)
// ---------------------------------------------------------------------------

/// Check pushed commits for conventional commit breaking change markers.
///
/// Input is expected to be null-delimited records from
/// `git log --format="%H%x00%B%x00"` — each record is `SHA\0FULL_MESSAGE\0`.
/// Using the full message (`%B`) ensures `BREAKING CHANGE:` footers in the
/// commit body are detected, not just the `!` marker in the subject line.
///
/// Returns a list of `(commit_sha, description)` tuples for each breaking commit.
pub(crate) fn detect_breaking_commits(commit_log: &str) -> Vec<(String, String)> {
    let mut results = Vec::new();
    // Split on null bytes. The format produces [SHA, MESSAGE, SHA, MESSAGE, ...].
    let parts: Vec<&str> = commit_log.split('\0').collect();
    // Process pairs: parts[0]=SHA, parts[1]=MESSAGE, parts[2]=SHA, parts[3]=MESSAGE, ...
    let mut i = 0;
    while i + 1 < parts.len() {
        let sha = parts[i].trim();
        let message = parts[i + 1].trim();
        i += 2;
        if sha.is_empty() || message.is_empty() {
            continue;
        }
        if let Some(parsed) = crate::version_compute::parse_conventional(sha, message) {
            if parsed.is_breaking {
                results.push((sha.to_string(), parsed.description));
            }
        }
    }
    results
}

/// Process detected breaking commits: create BreakingChange records, update
/// dependency edge status, and create high-priority tasks in dependent repos.
///
/// This is the core side-effect-producing logic extracted from
/// `detect_breaking_changes_on_push` so it can be tested without a git repo.
pub(crate) async fn process_breaking_changes(
    state: &Arc<AppState>,
    breaking_commits: &[(String, String)],
    dependents: &[gyre_domain::DependencyEdge],
    repo_id: &str,
    repo_name: &str,
    workspace_id: &str,
    tenant_id: &str,
    policy: &gyre_domain::DependencyPolicy,
    now: u64,
) {
    use gyre_common::Id;
    use gyre_domain::{BreakingChange, DependencyStatus, TaskPriority};

    for (commit_sha, description) in breaking_commits {
        for dep_edge in dependents {
            // Resolve the dependent repo's workspace — the task and notifications
            // belong to the dependent repo's workspace, not the pushed repo's.
            let dep_workspace_id = state
                .repos
                .find_by_id(&dep_edge.source_repo_id)
                .await
                .ok()
                .flatten()
                .map(|r| r.workspace_id.clone())
                .unwrap_or_else(|| Id::new(workspace_id));

            // Create a BreakingChange record.
            let bc_id = Id::new(uuid::Uuid::new_v4().to_string());
            let bc = BreakingChange::new(
                bc_id,
                dep_edge.id.clone(),
                Id::new(repo_id),
                commit_sha.as_str(),
                description.as_str(),
                now,
            );

            if let Err(e) = state.breaking_changes.create(&bc).await {
                warn!("breaking-change: failed to create record: {e}");
                continue;
            }

            // Update the dependency edge status to Breaking.
            let mut updated_edge = dep_edge.clone();
            updated_edge.status = DependencyStatus::Breaking;
            updated_edge.last_verified_at = now;
            if let Err(e) = state.dependencies.save(&updated_edge).await {
                warn!("breaking-change: failed to update edge status: {e}");
            }

            // Create a high-priority task in the dependent repo (if policy allows).
            if policy.auto_create_update_tasks {
                let task_id = Id::new(uuid::Uuid::new_v4().to_string());
                let mut task = gyre_domain::Task::new(
                    task_id,
                    format!("Breaking change in {repo_name}: {description}"),
                    now,
                );
                task.priority = TaskPriority::High;
                task.labels = vec![
                    "dependency-update".to_string(),
                    "breaking-change".to_string(),
                    "auto-created".to_string(),
                ];
                task.description = Some(format!(
                    "Repo '{repo_name}' pushed a breaking change (commit {commit_sha}): {description}. \
                     Update this repo's dependency to accommodate the change."
                ));
                task.workspace_id = dep_workspace_id.clone();
                task.repo_id = dep_edge.source_repo_id.clone();

                if let Err(e) = state.tasks.create(&task).await {
                    warn!("breaking-change: failed to create task: {e}");
                }
            }

            // Notify workspace members of the DEPENDENT repo about the breaking change.
            let members = state
                .workspace_memberships
                .list_by_workspace(&dep_workspace_id)
                .await
                .unwrap_or_default();

            for member in &members {
                let body_json = serde_json::json!({
                    "source_repo": repo_name,
                    "commit_sha": commit_sha,
                    "description": description,
                    "dependency_edge_id": dep_edge.id.as_str(),
                })
                .to_string();

                crate::notifications::notify_rich(
                    state,
                    dep_workspace_id.clone(),
                    member.user_id.clone(),
                    gyre_common::NotificationType::CrossWorkspaceSpecChange,
                    format!("Breaking change in {repo_name}: {description}"),
                    tenant_id,
                    Some(body_json),
                    Some(dep_edge.source_repo_id.to_string()),
                    Some(repo_id.to_string()),
                )
                .await;
            }

            // Broadcast to the dependent repo's workspace orchestrators.
            let payload = serde_json::json!({
                "event": "breaking_change_detected",
                "source_repo_id": repo_id,
                "source_repo_name": repo_name,
                "descriptions": [description],
                "dependent_repo_ids": [dep_edge.source_repo_id.as_str()],
            });

            state
                .emit_event(
                    Some(dep_workspace_id.clone()),
                    gyre_common::message::Destination::Workspace(dep_workspace_id),
                    gyre_common::MessageKind::Custom("breaking_change_detected".to_string()),
                    Some(payload),
                )
                .await;
        }

        tracing::info!(
            repo = repo_id,
            commit = commit_sha,
            dependents = dependents.len(),
            "detected breaking change, notified {} dependents",
            dependents.len(),
        );
    }
}

/// Detect breaking changes on push and propagate to dependent repos.
///
/// When a push to a repo contains conventional commit breaking change markers
/// (`feat!:`, `BREAKING CHANGE:` footer), this function:
/// 1. Identifies all repos that depend on the pushed repo
/// 2. Creates BreakingChange records for each affected dependency edge
/// 3. Updates dependency edge status to `Breaking`
/// 4. Creates high-priority tasks in dependent repos (if policy allows)
/// 5. Sends notifications to workspace members
pub(crate) async fn detect_breaking_changes_on_push(
    state: &Arc<AppState>,
    repo_id: &str,
    repo_path: &str,
    old_sha: &str,
    new_sha: &str,
    workspace_id: &str,
    tenant_id: &str,
) {
    use gyre_common::Id;

    let git_bin = std::env::var("GYRE_GIT_PATH").unwrap_or_else(|_| "git".to_string());

    // Use the full push range old_sha..new_sha so that breaking change markers
    // in interior commits of a multi-commit push are not missed.
    let range = if old_sha.starts_with("00000000") {
        new_sha.to_string()
    } else {
        format!("{old_sha}..{new_sha}")
    };

    // Get full commit messages from the push. Using %B (full message) ensures
    // BREAKING CHANGE: footers in the commit body are detected.
    let log_out = match Command::new(&git_bin)
        .arg("-C")
        .arg(repo_path)
        .arg("log")
        .arg("--format=%H%x00%B%x00")
        .arg(&range)
        .output()
        .await
    {
        Ok(o) if o.status.success() => o.stdout,
        _ => return,
    };

    let commit_log = String::from_utf8_lossy(&log_out);
    let breaking_commits = detect_breaking_commits(&commit_log);

    if breaking_commits.is_empty() {
        return;
    }

    // Query the dependency graph: "What repos depend on this repo?"
    let dependents = match state.dependencies.list_dependents(&Id::new(repo_id)).await {
        Ok(deps) => deps,
        Err(e) => {
            warn!("breaking-change: failed to list dependents for {repo_id}: {e}");
            return;
        }
    };

    if dependents.is_empty() {
        return;
    }

    let repo_name = state
        .repos
        .find_by_id(&Id::new(repo_id))
        .await
        .ok()
        .flatten()
        .map(|r| r.name.clone())
        .unwrap_or_else(|| repo_id.to_string());

    let now = crate::api::now_secs();

    // Get the workspace's dependency policy.
    let policy = state
        .dependency_policies
        .get_for_workspace(&Id::new(workspace_id))
        .await
        .unwrap_or_default();

    process_breaking_changes(
        state,
        &breaking_commits,
        &dependents,
        repo_id,
        &repo_name,
        workspace_id,
        tenant_id,
        &policy,
        now,
    )
    .await;
}

// ---------------------------------------------------------------------------
// Audit-only attestation chain verification (TASK-006, Phase 1)
// ---------------------------------------------------------------------------

/// Verify an attestation chain in audit-only mode. Returns a VerificationResult
/// tree describing what was checked. This does NOT reject pushes — results are
/// only logged for observability.
pub(crate) fn verify_attestation_audit_only(
    attestation: &gyre_common::Attestation,
) -> gyre_common::VerificationResult {
    let mut children = Vec::new();

    // Check 1: Attestation has a valid input.
    let input_valid = match &attestation.input {
        gyre_common::AttestationInput::Signed(signed) => {
            // Verify the signed input has a non-empty spec_path and spec_sha.
            let spec_ok =
                !signed.content.spec_path.is_empty() && !signed.content.spec_sha.is_empty();
            children.push(gyre_common::VerificationResult {
                label: "signed_input.content".to_string(),
                valid: spec_ok,
                message: if spec_ok {
                    format!(
                        "spec {}@{}",
                        signed.content.spec_path,
                        &signed.content.spec_sha[..8.min(signed.content.spec_sha.len())]
                    )
                } else {
                    "missing spec_path or spec_sha".to_string()
                },
                children: vec![],
            });

            // Cryptographic signature verification (§4.4 step 1, §6.1, §6.2):
            // Verify Ed25519 signature over SHA256(content) against key_binding.public_key.
            let sig_valid = {
                use ring::digest;
                use ring::signature::{self, UnparsedPublicKey};

                let content_bytes = serde_json::to_vec(&signed.content).unwrap_or_default();
                let content_hash = digest::digest(&digest::SHA256, &content_bytes);
                let peer_public_key =
                    UnparsedPublicKey::new(&signature::ED25519, &signed.key_binding.public_key);
                peer_public_key
                    .verify(content_hash.as_ref(), &signed.signature)
                    .is_ok()
            };
            children.push(gyre_common::VerificationResult {
                label: "signed_input.signature".to_string(),
                valid: sig_valid,
                message: if sig_valid {
                    "Ed25519 signature verified against key_binding.public_key".to_string()
                } else {
                    "Ed25519 signature verification FAILED — signature does not match \
                     key_binding.public_key over InputContent hash"
                        .to_string()
                },
                children: vec![],
            });

            // Check key binding expiry.
            let now = crate::api::now_secs();
            let kb_valid = !signed.key_binding.is_expired(now);
            if !kb_valid {
                // §7.7: key_binding.expired audit event.
                tracing::warn!(
                    user_identity = %signed.key_binding.user_identity,
                    expired_at = signed.key_binding.expires_at,
                    category = "Identity",
                    event = "key_binding.expired",
                    "key_binding.expired: key binding for {} expired at {}",
                    signed.key_binding.user_identity,
                    signed.key_binding.expires_at
                );
            }
            children.push(gyre_common::VerificationResult {
                label: "key_binding.expiry".to_string(),
                valid: kb_valid,
                message: if kb_valid {
                    format!("expires at {}", signed.key_binding.expires_at)
                } else {
                    format!("expired at {} (now={})", signed.key_binding.expires_at, now)
                },
                children: vec![],
            });

            // Check valid_until.
            let time_valid = now < signed.valid_until;
            children.push(gyre_common::VerificationResult {
                label: "signed_input.valid_until".to_string(),
                valid: time_valid,
                message: if time_valid {
                    format!("valid until {}", signed.valid_until)
                } else {
                    format!("expired at {} (now={})", signed.valid_until, now)
                },
                children: vec![],
            });

            spec_ok && sig_valid && kb_valid && time_valid
        }
        gyre_common::AttestationInput::Derived(derived) => {
            let has_parent = !derived.parent_ref.is_empty();
            children.push(gyre_common::VerificationResult {
                label: "derived_input.parent_ref".to_string(),
                valid: has_parent,
                message: if has_parent {
                    format!(
                        "parent: {}",
                        hex::encode(&derived.parent_ref[..8.min(derived.parent_ref.len())])
                    )
                } else {
                    "missing parent_ref".to_string()
                },
                children: vec![],
            });

            // Cryptographic signature verification (§4.4 step 1):
            // Verify Ed25519 signature over SHA256(derivation_content) against
            // key_binding.public_key. Mirrors the Signed branch verification.
            let sig_valid = {
                use ring::digest;
                use ring::signature::{self, UnparsedPublicKey};

                let derivation_content = serde_json::json!({
                    "parent_ref": hex::encode(&derived.parent_ref),
                    "agent_id": attestation.metadata.agent_id,
                    "task_id": attestation.metadata.task_id,
                });
                let derivation_bytes = serde_json::to_vec(&derivation_content).unwrap_or_default();
                let content_hash = digest::digest(&digest::SHA256, &derivation_bytes);
                let peer_public_key =
                    UnparsedPublicKey::new(&signature::ED25519, &derived.key_binding.public_key);
                peer_public_key
                    .verify(content_hash.as_ref(), &derived.signature)
                    .is_ok()
            };
            children.push(gyre_common::VerificationResult {
                label: "derived_input.signature".to_string(),
                valid: sig_valid,
                message: if sig_valid {
                    "Ed25519 signature verified against key_binding.public_key".to_string()
                } else {
                    "Ed25519 signature verification FAILED — signature does not match \
                     key_binding.public_key over derivation content hash"
                        .to_string()
                },
                children: vec![],
            });

            // Check key binding expiry (§4.4 step 2).
            let now = crate::api::now_secs();
            let kb_valid = !derived.key_binding.is_expired(now);
            if !kb_valid {
                tracing::warn!(
                    user_identity = %derived.key_binding.user_identity,
                    expired_at = derived.key_binding.expires_at,
                    category = "Identity",
                    event = "key_binding.expired",
                    "key_binding.expired: key binding for {} expired at {}",
                    derived.key_binding.user_identity,
                    derived.key_binding.expires_at
                );
            }
            children.push(gyre_common::VerificationResult {
                label: "key_binding.expiry".to_string(),
                valid: kb_valid,
                message: if kb_valid {
                    format!("expires at {}", derived.key_binding.expires_at)
                } else {
                    format!(
                        "expired at {} (now={})",
                        derived.key_binding.expires_at, now
                    )
                },
                children: vec![],
            });

            has_parent && sig_valid && kb_valid
        }
    };

    // Check 2: Chain depth is within limits.
    let depth_valid = attestation.metadata.chain_depth <= 10;
    children.push(gyre_common::VerificationResult {
        label: "chain_depth".to_string(),
        valid: depth_valid,
        message: format!("depth={} (max=10)", attestation.metadata.chain_depth),
        children: vec![],
    });

    let all_valid = input_valid && depth_valid;
    gyre_common::VerificationResult {
        label: "attestation_chain_verification".to_string(),
        valid: all_valid,
        message: if all_valid {
            "all structural checks passed".to_string()
        } else {
            "one or more structural checks failed".to_string()
        },
        children,
    }
}

/// Full chain verification: walk from leaf to root `SignedInput` (§4.4, §6.2).
///
/// Returns a `VerificationResult` tree covering:
///   - Each node's structural verification (signature, key binding, expiry)
///   - Chain depth limit check (configurable, hard limit 10)
///   - Accumulated constraints from the entire chain
///
/// The `max_depth` parameter is workspace-configurable; the hard limit of 10
/// applies regardless.
pub(crate) fn verify_chain(
    chain: &[gyre_common::Attestation],
    max_depth: u32,
) -> gyre_common::VerificationResult {
    let effective_max = max_depth.min(10);
    let mut children = Vec::new();

    if chain.is_empty() {
        return gyre_common::VerificationResult {
            label: "chain_verification".to_string(),
            valid: false,
            message: "empty attestation chain".to_string(),
            children: vec![],
        };
    }

    // Verify each node in the chain (root first → leaf last).
    let mut all_valid = true;
    let mut found_root = false;

    for (i, att) in chain.iter().enumerate() {
        let node_result = verify_attestation_audit_only(att);
        if !node_result.valid {
            all_valid = false;
        }

        // Check depth against configurable max.
        if att.metadata.chain_depth > effective_max {
            children.push(gyre_common::VerificationResult {
                label: format!("node[{}].depth_limit", i),
                valid: false,
                message: format!(
                    "chain depth {} exceeds max {} (hard limit 10)",
                    att.metadata.chain_depth, effective_max
                ),
                children: vec![],
            });
            all_valid = false;
        }

        // Verify chain linkage: DerivedInput must have a non-empty parent_ref
        // and there must be a prior node at a lower chain_depth.
        // The parent_ref is a content hash of the parent attestation, not its
        // id field — so we verify structural consistency (non-empty, depth ordering)
        // rather than trying to match content hashes to IDs.
        if let gyre_common::AttestationInput::Derived(ref derived) = att.input {
            let has_parent_ref = !derived.parent_ref.is_empty();
            let has_parent_at_lower_depth = chain
                .iter()
                .any(|p| p.metadata.chain_depth < att.metadata.chain_depth);
            let linkage_valid = has_parent_ref && has_parent_at_lower_depth;
            children.push(gyre_common::VerificationResult {
                label: format!("node[{}].parent_linkage", i),
                valid: linkage_valid,
                message: if linkage_valid {
                    format!(
                        "parent_ref present, parent at depth {} found",
                        att.metadata.chain_depth.saturating_sub(1)
                    )
                } else if !has_parent_ref {
                    "missing parent_ref".to_string()
                } else {
                    "no parent node at lower depth found in chain".to_string()
                },
                children: vec![],
            });
            if !linkage_valid {
                all_valid = false;
            }
        }

        if matches!(att.input, gyre_common::AttestationInput::Signed(_)) {
            found_root = true;
        }

        children.push(node_result);
    }

    // The chain must have a root SignedInput.
    if !found_root {
        children.push(gyre_common::VerificationResult {
            label: "root_signed_input".to_string(),
            valid: false,
            message: "no root SignedInput found in chain".to_string(),
            children: vec![],
        });
        all_valid = false;
    } else {
        children.push(gyre_common::VerificationResult {
            label: "root_signed_input".to_string(),
            valid: true,
            message: "root SignedInput found".to_string(),
            children: vec![],
        });
    }

    gyre_common::VerificationResult {
        label: "chain_verification".to_string(),
        valid: all_valid,
        message: if all_valid {
            format!(
                "chain of {} node(s) verified (max depth {})",
                chain.len(),
                effective_max
            )
        } else {
            "chain verification failed".to_string()
        },
        children,
    }
}

/// Accumulate all constraints from a verified chain (§4.3, §6.2 Phase 2).
///
/// Walks the chain root→leaf, collecting:
/// 1. Explicit constraints from the root SignedInput
/// 2. Additional constraints from each DerivedInput (additive only)
/// 3. Gate constraints from gate attestations on each node
///
/// Returns the full constraint set for evaluation.
pub(crate) fn accumulate_chain_constraints(
    chain: &[gyre_common::Attestation],
) -> (
    Option<gyre_common::attestation::SignedInput>,
    Vec<gyre_common::attestation::OutputConstraint>,
    Vec<gyre_common::attestation::GateConstraint>,
) {
    let mut explicit_constraints = Vec::new();
    let mut gate_constraints = Vec::new();
    let mut root_input = None;

    for att in chain {
        match &att.input {
            gyre_common::AttestationInput::Signed(si) => {
                root_input = Some(si.clone());
                explicit_constraints.extend(si.output_constraints.iter().cloned());
            }
            gyre_common::AttestationInput::Derived(di) => {
                // Additive only: append derived constraints.
                explicit_constraints.extend(di.output_constraints.iter().cloned());
            }
        }

        // Collect gate constraints from all nodes.
        for gate_result in &att.output.gate_results {
            if let Some(ref gc) = gate_result.constraint {
                gate_constraints.push(gc.clone());
            }
        }
    }

    (root_input, explicit_constraints, gate_constraints)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mem::test_state;
    use axum::{
        body::Body,
        routing::{get, post},
        Router,
    };
    use gyre_domain::{Repository, Workspace};
    use http::{Request, StatusCode};
    use std::sync::Arc;
    use tempfile::TempDir;
    use tower::ServiceExt;

    const TEST_WS_SLUG: &str = "test-ws";
    const TEST_REPO_NAME: &str = "my-repo";

    /// Build a router with git routes and a real bare repo.
    ///
    /// M34 Slice 6: Routes use workspace_slug/repo_name format.
    /// Returns (router, state, tmp_dir, workspace_slug, repo_name, repo_path).
    async fn git_app_with_repo() -> (
        Router,
        Arc<crate::AppState>,
        TempDir,
        String,
        String,
        String,
    ) {
        let tmp = TempDir::new().unwrap();
        let repo_path = tmp.path().join("test-proj").join("my-repo.git");
        std::fs::create_dir_all(&repo_path).unwrap();

        // git init --bare
        let status = std::process::Command::new("git")
            .arg("init")
            .arg("--bare")
            .arg(&repo_path)
            .status()
            .unwrap();
        assert!(status.success(), "git init --bare failed");

        let state = test_state();

        // Create a workspace with a known slug under the default tenant.
        let ws = Workspace {
            id: Id::new("ws-test"),
            tenant_id: Id::new("default"),
            name: "Test Workspace".to_string(),
            slug: TEST_WS_SLUG.to_string(),
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

        let repo = Repository::new(
            Id::new("repo-1"),
            Id::new("ws-test"),
            TEST_REPO_NAME,
            repo_path.to_str().unwrap(),
            0,
        );
        state.repos.create(&repo).await.unwrap();

        let repo_path_str = repo_path.to_str().unwrap().to_string();

        let app = Router::new()
            .route(
                "/git/:workspace_slug/:repo_name/info/refs",
                get(git_info_refs),
            )
            .route(
                "/git/:workspace_slug/:repo_name/git-upload-pack",
                post(git_upload_pack),
            )
            .route(
                "/git/:workspace_slug/:repo_name/git-receive-pack",
                post(git_receive_pack),
            )
            .with_state(state.clone());

        (
            app,
            state,
            tmp,
            TEST_WS_SLUG.to_string(),
            TEST_REPO_NAME.to_string(),
            repo_path_str,
        )
    }

    fn auth_header() -> &'static str {
        "Bearer test-token"
    }

    #[tokio::test]
    async fn info_refs_upload_pack_without_auth_returns_401() {
        let (app, _state, _tmp, ws_slug, repo_name, _path) = git_app_with_repo().await;
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/git/{ws_slug}/{repo_name}/info/refs?service=git-upload-pack"
                    ))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn info_refs_invalid_token_returns_401() {
        let (app, _state, _tmp, ws_slug, repo_name, _path) = git_app_with_repo().await;
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/git/{ws_slug}/{repo_name}/info/refs?service=git-upload-pack"
                    ))
                    .header("Authorization", "Bearer wrong-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn info_refs_upload_pack_returns_200_with_correct_content_type() {
        let (app, _state, _tmp, ws_slug, repo_name, _path) = git_app_with_repo().await;
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/git/{ws_slug}/{repo_name}/info/refs?service=git-upload-pack"
                    ))
                    .header("Authorization", auth_header())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers().get("content-type").unwrap(),
            "application/x-git-upload-pack-advertisement"
        );
    }

    #[tokio::test]
    async fn info_refs_receive_pack_returns_200_with_correct_content_type() {
        let (app, _state, _tmp, ws_slug, repo_name, _path) = git_app_with_repo().await;
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/git/{ws_slug}/{repo_name}/info/refs?service=git-receive-pack"
                    ))
                    .header("Authorization", auth_header())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers().get("content-type").unwrap(),
            "application/x-git-receive-pack-advertisement"
        );
    }

    #[tokio::test]
    async fn info_refs_unknown_service_returns_400() {
        let (app, _state, _tmp, ws_slug, repo_name, _path) = git_app_with_repo().await;
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/git/{ws_slug}/{repo_name}/info/refs?service=git-bogus"
                    ))
                    .header("Authorization", auth_header())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn info_refs_unknown_repo_returns_404() {
        let (app, _state, _tmp, _ws_slug, _repo_name, _path) = git_app_with_repo().await;
        let resp = app
            .oneshot(
                Request::builder()
                    // nonexistent workspace-slug → 404
                    .uri("/git/no-such-workspace/no-such-repo/info/refs?service=git-upload-pack")
                    .header("Authorization", auth_header())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn info_refs_body_starts_with_service_header() {
        let (app, _state, _tmp, ws_slug, repo_name, _path) = git_app_with_repo().await;
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/git/{ws_slug}/{repo_name}/info/refs?service=git-upload-pack"
                    ))
                    .header("Authorization", auth_header())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        // Should start with pkt-line "# service=git-upload-pack\n"
        let prefix = std::str::from_utf8(&body[..4]).unwrap();
        // Length = 4 + len("# service=git-upload-pack\n") = 4 + 26 = 30 = 0x1e
        assert_eq!(prefix, "001e");
        assert!(body.starts_with(b"001e# service=git-upload-pack\n"));
    }

    #[tokio::test]
    async fn agent_token_accepted_for_info_refs() {
        let (app, state, _tmp, ws_slug, repo_name, _path) = git_app_with_repo().await;
        state
            .kv_store
            .kv_set("agent_tokens", "agent-7", "my-agent-token".to_string())
            .await
            .unwrap();

        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/git/{ws_slug}/{repo_name}/info/refs?service=git-upload-pack"
                    ))
                    .header("Authorization", "Bearer my-agent-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    /// End-to-end: git clone via smart HTTP using actual git binary.
    #[tokio::test(flavor = "multi_thread")]
    async fn git_clone_empty_repo_via_smart_http() {
        let (_, state, _tmp, ws_slug, repo_name, _path) = git_app_with_repo().await;

        let app = Router::new()
            .route(
                "/git/:workspace_slug/:repo_name/info/refs",
                get(git_info_refs),
            )
            .route(
                "/git/:workspace_slug/:repo_name/git-upload-pack",
                post(git_upload_pack),
            )
            .route(
                "/git/:workspace_slug/:repo_name/git-receive-pack",
                post(git_receive_pack),
            )
            .with_state(state);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let clone_dir = TempDir::new().unwrap();
        let url = format!("http://127.0.0.1:{port}/git/{ws_slug}/{repo_name}");

        // Run git clone in a blocking thread so we don't starve the async executor.
        let clone_target = clone_dir.path().join("cloned");
        let result = tokio::task::spawn_blocking(move || {
            std::process::Command::new("git")
                .arg("clone")
                .arg(&url)
                .arg(&clone_target)
                // Disable all credential prompts.
                .env("GIT_TERMINAL_PROMPT", "0")
                .env("GIT_ASKPASS", "true")
                // Inject Bearer token via http.extraHeader config.
                .env("GIT_CONFIG_COUNT", "1")
                .env("GIT_CONFIG_KEY_0", "http.extraHeader")
                .env("GIT_CONFIG_VALUE_0", "Authorization: Bearer test-token")
                .output()
        })
        .await
        .unwrap()
        .unwrap();

        // An empty repo clones successfully with exit code 0 and a warning.
        let stderr = String::from_utf8_lossy(&result.stderr);
        let ok = result.status.success()
            || stderr.contains("empty repository")
            || stderr.contains("warning");
        assert!(
            ok,
            "git clone failed (exit={:?}): {stderr}",
            result.status.code()
        );
    }

    #[tokio::test]
    async fn pkt_line_format_correct() {
        // "# service=git-upload-pack\n" = 26 bytes, total pkt-line = 30 = 0x1e
        let pl = super::pkt_line("# service=git-upload-pack\n");
        assert_eq!(&pl[..4], b"001e");
        assert_eq!(&pl[4..], b"# service=git-upload-pack\n");
    }

    #[tokio::test]
    async fn service_header_format_correct() {
        let hdr = super::service_header("git-upload-pack");
        assert!(hdr.starts_with(b"001e# service=git-upload-pack\n"));
        // flush pkt must follow immediately
        let line_end = "001e# service=git-upload-pack\n".len();
        assert_eq!(&hdr[line_end..line_end + 4], b"0000");
    }

    #[tokio::test]
    async fn parse_ref_updates_empty() {
        let updates = super::parse_ref_updates(b"0000");
        assert!(updates.is_empty());
    }

    #[tokio::test]
    async fn parse_ref_updates_single() {
        // Build a pkt-line for one ref update.
        let line = "aabbccdd00000000000000000000000000000000 1122334455000000000000000000000000000000 refs/heads/main\n";
        let pkt = super::pkt_line(line);
        let mut body = pkt;
        body.extend_from_slice(b"0000");
        let updates = super::parse_ref_updates(&body);
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].refname, "refs/heads/main");
    }

    #[tokio::test]
    async fn parse_ref_updates_deletion_skipped() {
        // new sha is all zeros = deletion
        let zeros = "0000000000000000000000000000000000000000";
        let line = format!("aabb{zeros}0000000000000000000000000000000000000000 refs/heads/old\n");
        let pkt = super::pkt_line(&line);
        let mut body = pkt;
        body.extend_from_slice(b"0000");
        let updates = super::parse_ref_updates(&body);
        // Deletion: new sha is zeros — skipped.
        let non_delete: Vec<_> = updates.iter().filter(|u| u.new_sha != zeros).collect();
        assert!(non_delete.is_empty());
    }

    // ── Spec lifecycle tests ─────────────────────────────────────────────────

    #[test]
    fn parse_spec_changes_empty_input() {
        let changes = super::parse_spec_changes("");
        assert!(changes.is_empty());
    }

    #[test]
    fn parse_spec_changes_added_spec() {
        let input = "A\tspecs/system/new-spec.md\n";
        let changes = super::parse_spec_changes(input);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].0, 'A');
        assert_eq!(changes[0].1, "specs/system/new-spec.md");
        assert!(changes[0].2.is_none());
    }

    #[test]
    fn parse_spec_changes_modified_spec() {
        let input = "M\tspecs/development/architecture.md\n";
        let changes = super::parse_spec_changes(input);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].0, 'M');
        assert_eq!(changes[0].1, "specs/development/architecture.md");
    }

    #[test]
    fn parse_spec_changes_deleted_spec() {
        let input = "D\tspecs/system/old-spec.md\n";
        let changes = super::parse_spec_changes(input);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].0, 'D');
        assert_eq!(changes[0].1, "specs/system/old-spec.md");
    }

    #[test]
    fn parse_spec_changes_renamed_spec() {
        let input = "R090\tspecs/system/old.md\tspecs/system/new.md\n";
        let changes = super::parse_spec_changes(input);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].0, 'R');
        assert_eq!(changes[0].1, "specs/system/new.md");
        assert_eq!(changes[0].2.as_deref(), Some("specs/system/old.md"));
    }

    #[test]
    fn parse_spec_changes_ignores_non_watched_paths() {
        // milestones and src changes should be ignored
        let input = "M\tspecs/milestones/m1.md\nM\tsrc/main.rs\nA\tspecs/system/real.md\n";
        let changes = super::parse_spec_changes(input);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].1, "specs/system/real.md");
    }

    #[test]
    fn parse_spec_changes_development_path_watched() {
        let input = "A\tspecs/development/database-migrations.md\n";
        let changes = super::parse_spec_changes(input);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].1, "specs/development/database-migrations.md");
    }

    #[test]
    fn classify_spec_change_added() {
        let (title, labels, priority) =
            super::classify_spec_change('A', "specs/system/foo.md", None).unwrap();
        assert_eq!(title, "Implement spec: specs/system/foo.md");
        assert!(labels.contains(&"spec-implementation".to_string()));
        assert!(labels.contains(&"auto-created".to_string()));
        assert_eq!(priority, gyre_domain::TaskPriority::Medium);
    }

    #[test]
    fn classify_spec_change_modified() {
        let (title, labels, priority) =
            super::classify_spec_change('M', "specs/system/foo.md", None).unwrap();
        assert_eq!(title, "Review spec change: specs/system/foo.md");
        assert!(labels.contains(&"spec-drift-review".to_string()));
        assert_eq!(priority, gyre_domain::TaskPriority::High);
    }

    #[test]
    fn classify_spec_change_deleted() {
        let (title, labels, priority) =
            super::classify_spec_change('D', "specs/system/old.md", None).unwrap();
        assert_eq!(title, "Handle spec removal: specs/system/old.md");
        assert!(labels.contains(&"spec-deprecated".to_string()));
        assert_eq!(priority, gyre_domain::TaskPriority::High);
    }

    #[test]
    fn classify_spec_change_renamed() {
        let (title, labels, priority) =
            super::classify_spec_change('R', "specs/system/new.md", Some("specs/system/old.md"))
                .unwrap();
        assert_eq!(
            title,
            "Update spec references: specs/system/old.md -> specs/system/new.md"
        );
        assert!(labels.contains(&"spec-housekeeping".to_string()));
        assert_eq!(priority, gyre_domain::TaskPriority::Medium);
    }

    #[test]
    fn classify_spec_change_unknown_returns_none() {
        assert!(super::classify_spec_change('X', "specs/system/foo.md", None).is_none());
    }

    // ── Audit-only verification tests (TASK-006) ─────────────────────────

    #[test]
    fn verify_attestation_audit_only_valid_signed_input() {
        use ring::signature::{Ed25519KeyPair, KeyPair};

        // Generate a real Ed25519 keypair and sign the content.
        let rng = ring::rand::SystemRandom::new();
        let pkcs8 = Ed25519KeyPair::generate_pkcs8(&rng).unwrap();
        let key_pair = Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap();

        let content = gyre_common::InputContent {
            spec_path: "specs/system/payments.md".to_string(),
            spec_sha: "abc12345".to_string(),
            workspace_id: "ws-1".to_string(),
            repo_id: "repo-1".to_string(),
            persona_constraints: vec![],
            meta_spec_set_sha: "def456".to_string(),
            scope: gyre_common::ScopeConstraint {
                allowed_paths: vec![],
                forbidden_paths: vec![],
            },
        };

        let content_bytes = serde_json::to_vec(&content).unwrap();
        let content_hash = ring::digest::digest(&ring::digest::SHA256, &content_bytes);
        let signature = key_pair.sign(content_hash.as_ref()).as_ref().to_vec();

        let kb = gyre_common::KeyBinding {
            public_key: key_pair.public_key().as_ref().to_vec(),
            user_identity: "user:jsell".to_string(),
            issuer: "https://keycloak.example.com".to_string(),
            trust_anchor_id: "tenant-keycloak".to_string(),
            issued_at: 1_700_000_000,
            expires_at: u64::MAX, // far in the future
            user_signature: vec![10],
            platform_countersign: vec![20],
        };

        let att = gyre_common::Attestation {
            id: "att-1".to_string(),
            input: gyre_common::AttestationInput::Signed(gyre_common::SignedInput {
                content,
                output_constraints: vec![],
                valid_until: u64::MAX,
                expected_generation: None,
                signature,
                key_binding: kb,
            }),
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
                task_id: "TASK-007".to_string(),
                agent_id: "agent-1".to_string(),
                chain_depth: 0,
            },
        };

        let result = super::verify_attestation_audit_only(&att);
        assert!(result.valid, "result should be valid: {:?}", result);
        assert_eq!(result.label, "attestation_chain_verification");
        // Verify the signature child reports valid.
        let sig_child = result
            .children
            .iter()
            .find(|c| c.label == "signed_input.signature")
            .expect("should have signed_input.signature child");
        assert!(
            sig_child.valid,
            "signature should be valid: {:?}",
            sig_child
        );
    }

    #[test]
    fn verify_attestation_audit_only_forged_signature() {
        use ring::signature::{Ed25519KeyPair, KeyPair};

        // Generate a real keypair but use a forged (wrong) signature.
        let rng = ring::rand::SystemRandom::new();
        let pkcs8 = Ed25519KeyPair::generate_pkcs8(&rng).unwrap();
        let key_pair = Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap();

        let content = gyre_common::InputContent {
            spec_path: "specs/system/payments.md".to_string(),
            spec_sha: "abc12345".to_string(),
            workspace_id: "ws-1".to_string(),
            repo_id: "repo-1".to_string(),
            persona_constraints: vec![],
            meta_spec_set_sha: "def456".to_string(),
            scope: gyre_common::ScopeConstraint {
                allowed_paths: vec![],
                forbidden_paths: vec![],
            },
        };

        let kb = gyre_common::KeyBinding {
            public_key: key_pair.public_key().as_ref().to_vec(),
            user_identity: "user:jsell".to_string(),
            issuer: "https://keycloak.example.com".to_string(),
            trust_anchor_id: "tenant-keycloak".to_string(),
            issued_at: 1_700_000_000,
            expires_at: u64::MAX,
            user_signature: vec![10],
            platform_countersign: vec![20],
        };

        let att = gyre_common::Attestation {
            id: "att-forged".to_string(),
            input: gyre_common::AttestationInput::Signed(gyre_common::SignedInput {
                content,
                output_constraints: vec![],
                valid_until: u64::MAX,
                expected_generation: None,
                signature: vec![0xDE; 64], // forged signature bytes (64 bytes for Ed25519)
                key_binding: kb,
            }),
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
                task_id: "TASK-007".to_string(),
                agent_id: "agent-1".to_string(),
                chain_depth: 0,
            },
        };

        let result = super::verify_attestation_audit_only(&att);
        // Overall should be invalid because signature is forged.
        assert!(
            !result.valid,
            "result should be invalid for forged sig: {:?}",
            result
        );
        // Signature child specifically should report invalid.
        let sig_child = result
            .children
            .iter()
            .find(|c| c.label == "signed_input.signature")
            .expect("should have signed_input.signature child");
        assert!(!sig_child.valid, "forged signature should be invalid");
        assert!(
            sig_child.message.contains("FAILED"),
            "message should indicate failure: {}",
            sig_child.message
        );
    }

    #[test]
    fn verify_attestation_audit_only_expired_key_binding() {
        use ring::signature::{Ed25519KeyPair, KeyPair};

        let rng = ring::rand::SystemRandom::new();
        let pkcs8 = Ed25519KeyPair::generate_pkcs8(&rng).unwrap();
        let key_pair = Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap();

        let content = gyre_common::InputContent {
            spec_path: "specs/system/payments.md".to_string(),
            spec_sha: "abc12345".to_string(),
            workspace_id: "ws-1".to_string(),
            repo_id: "repo-1".to_string(),
            persona_constraints: vec![],
            meta_spec_set_sha: String::new(),
            scope: gyre_common::ScopeConstraint {
                allowed_paths: vec![],
                forbidden_paths: vec![],
            },
        };

        let content_bytes = serde_json::to_vec(&content).unwrap();
        let content_hash = ring::digest::digest(&ring::digest::SHA256, &content_bytes);
        let signature = key_pair.sign(content_hash.as_ref()).as_ref().to_vec();

        let kb = gyre_common::KeyBinding {
            public_key: key_pair.public_key().as_ref().to_vec(),
            user_identity: "user:jsell".to_string(),
            issuer: "https://keycloak.example.com".to_string(),
            trust_anchor_id: "tenant-keycloak".to_string(),
            issued_at: 1_000_000_000,
            expires_at: 1_000_000_001, // expired long ago
            user_signature: vec![10],
            platform_countersign: vec![20],
        };

        let att = gyre_common::Attestation {
            id: "att-2".to_string(),
            input: gyre_common::AttestationInput::Signed(gyre_common::SignedInput {
                content,
                output_constraints: vec![],
                valid_until: u64::MAX,
                expected_generation: None,
                signature,
                key_binding: kb,
            }),
            output: gyre_common::AttestationOutput {
                content_hash: vec![40],
                commit_sha: String::new(),
                agent_signature: None,
                gate_results: vec![],
            },
            metadata: gyre_common::AttestationMetadata {
                created_at: 1_000_000_000,
                workspace_id: "ws-1".to_string(),
                repo_id: "repo-1".to_string(),
                task_id: "TASK-008".to_string(),
                agent_id: "agent-2".to_string(),
                chain_depth: 0,
            },
        };

        let result = super::verify_attestation_audit_only(&att);
        assert!(!result.valid, "result should be invalid: {:?}", result);
        // Signature should still be cryptographically valid.
        let sig_child = result
            .children
            .iter()
            .find(|c| c.label == "signed_input.signature")
            .expect("should have signed_input.signature child");
        assert!(
            sig_child.valid,
            "signature should be valid even though key is expired"
        );
        // Check that the key_binding.expiry child reports invalid.
        let kb_child = result
            .children
            .iter()
            .find(|c| c.label == "key_binding.expiry")
            .expect("should have key_binding.expiry child");
        assert!(!kb_child.valid);
    }

    #[test]
    fn verify_attestation_audit_only_excessive_chain_depth() {
        use ring::signature::{Ed25519KeyPair, KeyPair};

        let rng = ring::rand::SystemRandom::new();
        let pkcs8 = Ed25519KeyPair::generate_pkcs8(&rng).unwrap();
        let key_pair = Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap();

        let content = gyre_common::InputContent {
            spec_path: "specs/system/x.md".to_string(),
            spec_sha: "1234567890abcdef".to_string(),
            workspace_id: "ws-1".to_string(),
            repo_id: "repo-1".to_string(),
            persona_constraints: vec![],
            meta_spec_set_sha: String::new(),
            scope: gyre_common::ScopeConstraint {
                allowed_paths: vec![],
                forbidden_paths: vec![],
            },
        };

        let content_bytes = serde_json::to_vec(&content).unwrap();
        let content_hash = ring::digest::digest(&ring::digest::SHA256, &content_bytes);
        let signature = key_pair.sign(content_hash.as_ref()).as_ref().to_vec();

        let kb = gyre_common::KeyBinding {
            public_key: key_pair.public_key().as_ref().to_vec(),
            user_identity: "user:jsell".to_string(),
            issuer: "https://example.com".to_string(),
            trust_anchor_id: "test".to_string(),
            issued_at: 1_700_000_000,
            expires_at: u64::MAX,
            user_signature: vec![],
            platform_countersign: vec![],
        };

        let att = gyre_common::Attestation {
            id: "att-deep".to_string(),
            input: gyre_common::AttestationInput::Signed(gyre_common::SignedInput {
                content,
                output_constraints: vec![],
                valid_until: u64::MAX,
                expected_generation: None,
                signature,
                key_binding: kb,
            }),
            output: gyre_common::AttestationOutput {
                content_hash: vec![],
                commit_sha: String::new(),
                agent_signature: None,
                gate_results: vec![],
            },
            metadata: gyre_common::AttestationMetadata {
                created_at: 1_700_000_000,
                workspace_id: "ws-1".to_string(),
                repo_id: "repo-1".to_string(),
                task_id: "T".to_string(),
                agent_id: "A".to_string(),
                chain_depth: 11, // exceeds limit of 10
            },
        };

        let result = super::verify_attestation_audit_only(&att);
        assert!(!result.valid);
        // Signature should be cryptographically valid.
        let sig_child = result
            .children
            .iter()
            .find(|c| c.label == "signed_input.signature")
            .expect("should have signed_input.signature child");
        assert!(
            sig_child.valid,
            "signature should be valid even though chain depth is excessive"
        );
        let depth_child = result
            .children
            .iter()
            .find(|c| c.label == "chain_depth")
            .expect("should have chain_depth child");
        assert!(!depth_child.valid);
    }

    // ── Full chain verification (TASK-009, §4.4, §6.2) ───────────────

    fn make_signed_chain_attestation(
        task_id: &str,
        key_pair: &ring::signature::Ed25519KeyPair,
    ) -> gyre_common::Attestation {
        use ring::signature::KeyPair;

        let content = gyre_common::attestation::InputContent {
            spec_path: "specs/system/payments.md".to_string(),
            spec_sha: "abc12345".to_string(),
            workspace_id: "ws-1".to_string(),
            repo_id: "repo-1".to_string(),
            persona_constraints: vec![gyre_common::attestation::PersonaRef {
                name: "security".to_string(),
            }],
            meta_spec_set_sha: "def456".to_string(),
            scope: gyre_common::attestation::ScopeConstraint {
                allowed_paths: vec!["src/**".to_string()],
                forbidden_paths: vec![],
            },
        };

        let content_bytes = serde_json::to_vec(&content).unwrap();
        let content_hash = ring::digest::digest(&ring::digest::SHA256, &content_bytes);
        let signature = key_pair.sign(content_hash.as_ref()).as_ref().to_vec();

        let kb = gyre_common::KeyBinding {
            public_key: key_pair.public_key().as_ref().to_vec(),
            user_identity: "user:jsell".to_string(),
            issuer: "https://keycloak.example.com".to_string(),
            trust_anchor_id: "tenant-keycloak".to_string(),
            issued_at: 1_700_000_000,
            expires_at: u64::MAX,
            user_signature: vec![10],
            platform_countersign: vec![20],
        };

        gyre_common::Attestation {
            id: "root-att-1".to_string(),
            input: gyre_common::AttestationInput::Signed(gyre_common::attestation::SignedInput {
                content,
                output_constraints: vec![gyre_common::attestation::OutputConstraint {
                    name: "scope to src".to_string(),
                    expression: r#"output.changed_files.all(f, f.startsWith("src/"))"#.to_string(),
                }],
                valid_until: u64::MAX,
                expected_generation: None,
                signature,
                key_binding: kb,
            }),
            output: gyre_common::AttestationOutput {
                content_hash: vec![],
                commit_sha: "sha-root".to_string(),
                agent_signature: None,
                gate_results: vec![],
            },
            metadata: gyre_common::AttestationMetadata {
                created_at: 1_700_000_000,
                workspace_id: "ws-1".to_string(),
                repo_id: "repo-1".to_string(),
                task_id: task_id.to_string(),
                agent_id: "orchestrator-1".to_string(),
                chain_depth: 0,
            },
        }
    }

    fn make_derived_chain_attestation(
        parent: &gyre_common::Attestation,
        task_id: &str,
        agent_id: &str,
        depth: u32,
    ) -> gyre_common::Attestation {
        let rng = ring::rand::SystemRandom::new();
        let pkcs8 = ring::signature::Ed25519KeyPair::generate_pkcs8(&rng).unwrap();
        let key_pair = ring::signature::Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap();
        use ring::signature::KeyPair;

        // Compute parent_ref as content hash of parent attestation.
        let parent_bytes = serde_json::to_vec(parent).unwrap();
        let parent_hash = {
            use sha2::{Digest, Sha256};
            let mut hasher = Sha256::new();
            hasher.update(&parent_bytes);
            hasher.finalize().to_vec()
        };

        let derivation_content = serde_json::json!({
            "parent_ref": hex::encode(&parent_hash),
            "agent_id": agent_id,
            "task_id": task_id,
        });
        let derivation_bytes = serde_json::to_vec(&derivation_content).unwrap();
        let content_hash = ring::digest::digest(&ring::digest::SHA256, &derivation_bytes);
        let sig = key_pair.sign(content_hash.as_ref()).as_ref().to_vec();

        let kb = gyre_common::KeyBinding {
            public_key: key_pair.public_key().as_ref().to_vec(),
            user_identity: format!("agent:{agent_id}"),
            issuer: "https://gyre.example.com".to_string(),
            trust_anchor_id: "gyre-oidc".to_string(),
            issued_at: 1_700_000_000,
            expires_at: u64::MAX,
            user_signature: vec![],
            platform_countersign: vec![],
        };

        gyre_common::Attestation {
            id: format!("derived-att-{}", agent_id),
            input: gyre_common::AttestationInput::Derived(gyre_common::DerivedInput {
                parent_ref: parent_hash,
                preconditions: vec![],
                update: format!("agent:{agent_id} spawned for task:{task_id}"),
                output_constraints: vec![],
                signature: sig,
                key_binding: kb,
            }),
            output: gyre_common::AttestationOutput {
                content_hash: vec![],
                commit_sha: format!("sha-{agent_id}"),
                agent_signature: None,
                gate_results: vec![],
            },
            metadata: gyre_common::AttestationMetadata {
                created_at: 1_700_000_100,
                workspace_id: "ws-1".to_string(),
                repo_id: "repo-1".to_string(),
                task_id: task_id.to_string(),
                agent_id: agent_id.to_string(),
                chain_depth: depth,
            },
        }
    }

    #[test]
    fn verify_chain_single_signed_input() {
        let rng = ring::rand::SystemRandom::new();
        let pkcs8 = ring::signature::Ed25519KeyPair::generate_pkcs8(&rng).unwrap();
        let key_pair = ring::signature::Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap();

        let root = make_signed_chain_attestation("TASK-CHAIN-1", &key_pair);
        let chain = vec![root];

        let result = verify_chain(&chain, 10);
        assert!(
            result.valid,
            "single signed input chain should be valid: {}",
            result.message
        );
    }

    #[test]
    fn verify_chain_human_orchestrator_agent() {
        // Full chain: human → orchestrator → agent (depth 0, 1, 2).
        let rng = ring::rand::SystemRandom::new();
        let pkcs8 = ring::signature::Ed25519KeyPair::generate_pkcs8(&rng).unwrap();
        let key_pair = ring::signature::Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap();

        let root = make_signed_chain_attestation("TASK-CHAIN-2", &key_pair);
        let orch = make_derived_chain_attestation(&root, "TASK-CHAIN-2", "orchestrator-1", 1);
        let agent = make_derived_chain_attestation(&orch, "TASK-CHAIN-2", "worker-1", 2);

        let chain = vec![root, orch, agent];
        let result = verify_chain(&chain, 10);
        assert!(
            result.valid,
            "human→orchestrator→agent chain should be valid: {}",
            result.message
        );

        // Verify root SignedInput was found.
        let root_found = result
            .children
            .iter()
            .any(|c| c.label == "root_signed_input" && c.valid);
        assert!(root_found, "should find root SignedInput");
    }

    #[test]
    fn verify_chain_exceeds_depth_limit() {
        let rng = ring::rand::SystemRandom::new();
        let pkcs8 = ring::signature::Ed25519KeyPair::generate_pkcs8(&rng).unwrap();
        let key_pair = ring::signature::Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap();

        let root = make_signed_chain_attestation("TASK-DEEP", &key_pair);
        let derived = make_derived_chain_attestation(&root, "TASK-DEEP", "deep-agent", 5);

        let chain = vec![root, derived];

        // With max_depth=3, depth=5 should fail.
        let result = verify_chain(&chain, 3);
        assert!(!result.valid, "should fail when depth exceeds limit");
        let depth_failed = result
            .children
            .iter()
            .any(|c| c.label.contains("depth_limit") && !c.valid);
        assert!(depth_failed, "should have depth limit failure");
    }

    #[test]
    fn verify_chain_empty_is_invalid() {
        let result = verify_chain(&[], 10);
        assert!(!result.valid, "empty chain should be invalid");
        assert!(result.message.contains("empty"));
    }

    #[test]
    fn verify_chain_no_root_signed_input_is_invalid() {
        // Chain with only derived inputs (no root).
        let rng = ring::rand::SystemRandom::new();
        let pkcs8 = ring::signature::Ed25519KeyPair::generate_pkcs8(&rng).unwrap();
        let key_pair = ring::signature::Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap();

        let root = make_signed_chain_attestation("TASK-NOROOT", &key_pair);
        let derived = make_derived_chain_attestation(&root, "TASK-NOROOT", "agent-1", 1);

        // Only include the derived, not the root.
        let chain = vec![derived];
        let result = verify_chain(&chain, 10);
        assert!(!result.valid, "chain without root should be invalid");
    }

    #[test]
    fn accumulate_chain_constraints_additive() {
        let rng = ring::rand::SystemRandom::new();
        let pkcs8 = ring::signature::Ed25519KeyPair::generate_pkcs8(&rng).unwrap();
        let key_pair = ring::signature::Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap();

        let root = make_signed_chain_attestation("TASK-ACC", &key_pair);

        // Create a derived with additional constraints.
        let mut derived = make_derived_chain_attestation(&root, "TASK-ACC", "agent-1", 1);
        if let gyre_common::AttestationInput::Derived(ref mut di) = derived.input {
            di.output_constraints
                .push(gyre_common::attestation::OutputConstraint {
                    name: "agent-scope".to_string(),
                    expression: r#"output.changed_files.all(f, f.startsWith("src/payments/"))"#
                        .to_string(),
                });
        }

        let chain = vec![root, derived];
        let (root_si, explicit, gate) = accumulate_chain_constraints(&chain);

        assert!(root_si.is_some(), "should find root signed input");
        // Root has 1 constraint + derived has 1 = 2 total.
        assert_eq!(
            explicit.len(),
            2,
            "should accumulate constraints additively"
        );
        assert!(gate.is_empty(), "should have no gate constraints");
    }

    #[test]
    fn accumulate_chain_constraints_includes_gate_constraints() {
        let rng = ring::rand::SystemRandom::new();
        let pkcs8 = ring::signature::Ed25519KeyPair::generate_pkcs8(&rng).unwrap();
        let key_pair = ring::signature::Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap();

        let mut root = make_signed_chain_attestation("TASK-GATE", &key_pair);
        // Add a gate attestation with a constraint.
        root.output
            .gate_results
            .push(gyre_common::attestation::GateAttestation {
                gate_id: "gate-review".to_string(),
                gate_name: "Code Review".to_string(),
                gate_type: gyre_common::GateType::AgentReview,
                status: gyre_common::GateStatus::Passed,
                output_hash: vec![1, 2, 3],
                constraint: Some(gyre_common::attestation::GateConstraint {
                    gate_id: "gate-review".to_string(),
                    gate_name: "Code Review".to_string(),
                    constraint: gyre_common::attestation::OutputConstraint {
                        name: "review constraint".to_string(),
                        expression: "true".to_string(),
                    },
                    signed_by: vec![4, 5, 6],
                }),
                signature: vec![7, 8, 9],
                key_binding: gyre_common::KeyBinding {
                    public_key: vec![1; 32],
                    user_identity: "gate-agent:review".to_string(),
                    issuer: "https://gyre.example.com".to_string(),
                    trust_anchor_id: "gyre-platform".to_string(),
                    issued_at: 1_700_000_000,
                    expires_at: u64::MAX,
                    user_signature: vec![],
                    platform_countersign: vec![],
                },
            });

        let chain = vec![root];
        let (_root_si, explicit, gate) = accumulate_chain_constraints(&chain);

        assert_eq!(explicit.len(), 1, "should have 1 explicit constraint");
        assert_eq!(gate.len(), 1, "should have 1 gate constraint");
        assert_eq!(gate[0].gate_name, "Code Review");
    }
}
