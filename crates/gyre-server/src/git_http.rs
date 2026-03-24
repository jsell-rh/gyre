//! Smart HTTP Git protocol handlers.
//!
//! Implements the Git smart HTTP protocol by shelling out to `git upload-pack`
//! and `git receive-pack`. This is the same approach used by GitLab, Gitea, etc.
//!
//! Supported routes:
//!   GET  /git/:project/:repo/info/refs?service={git-upload-pack|git-receive-pack}
//!   POST /git/:project/:repo/git-upload-pack
//!   POST /git/:project/:repo/git-receive-pack

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

/// Resolve `:project` + `:repo` URL segments to a Repository record.
///
/// * `project`  — the project_id (UUID string) used when the repo was created.
/// * `repo_seg` — the repo segment from the URL, e.g. `my-repo.git`.
async fn resolve_repo(
    state: &Arc<AppState>,
    project: &str,
    repo_seg: &str,
) -> Result<gyre_domain::Repository, Response> {
    let repo_name = repo_seg.strip_suffix(".git").unwrap_or(repo_seg);

    let repos = state
        .repos
        .list_by_project(&Id::new(project))
        .await
        .map_err(|e| git_err(format!("db error: {e}")))?;

    repos
        .into_iter()
        .find(|r| r.name == repo_name)
        .ok_or_else(|| {
            not_found(format!(
                "repo '{repo_name}' not found in project '{project}'"
            ))
        })
}

/// Resolve to just the filesystem path (convenience wrapper).
async fn resolve_repo_path(
    state: &Arc<AppState>,
    project: &str,
    repo_seg: &str,
) -> Result<String, Response> {
    resolve_repo(state, project, repo_seg).await.map(|r| r.path)
}

// ---------------------------------------------------------------------------
// Path params
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct GitPath {
    project: String,
    repo: String,
}

// ---------------------------------------------------------------------------
// GET /git/:project/:repo/info/refs?service=git-{upload,receive}-pack
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct InfoRefsQuery {
    service: String,
}

pub async fn git_info_refs(
    State(state): State<Arc<AppState>>,
    Path(GitPath { project, repo }): Path<GitPath>,
    Query(InfoRefsQuery { service }): Query<InfoRefsQuery>,
    _auth: AuthenticatedAgent,
) -> Response {
    let subcommand = match service.as_str() {
        "git-upload-pack" => "upload-pack",
        "git-receive-pack" => "receive-pack",
        other => {
            return (StatusCode::BAD_REQUEST, format!("unknown service: {other}")).into_response()
        }
    };

    let content_type = format!("application/x-{service}-advertisement");

    let repo_path = match resolve_repo_path(&state, &project, &repo).await {
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
// POST /git/:project/:repo/git-upload-pack  (clone / fetch)
// ---------------------------------------------------------------------------

pub async fn git_upload_pack(
    State(state): State<Arc<AppState>>,
    Path(GitPath { project, repo }): Path<GitPath>,
    _auth: AuthenticatedAgent,
    req: Request,
) -> Response {
    let repo_path = match resolve_repo_path(&state, &project, &repo).await {
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
// POST /git/:project/:repo/git-receive-pack  (push)
// ---------------------------------------------------------------------------

pub async fn git_receive_pack(
    State(state): State<Arc<AppState>>,
    Path(GitPath { project, repo }): Path<GitPath>,
    auth: AuthenticatedAgent,
    req: Request,
) -> Response {
    let resolved = match resolve_repo(&state, &project, &repo).await {
        Ok(r) => r,
        Err(r) => return r,
    };
    if resolved.is_mirror {
        return (
            StatusCode::FORBIDDEN,
            "push rejected: repository is a read-only mirror".to_string(),
        )
            .into_response();
    }
    let repo_id = resolved.id.to_string();
    let repo_path = resolved.path;
    let default_branch = resolved.default_branch;

    // G6: ABAC enforcement — check repo access policies against the caller's JWT claims.
    if let Err(reason) = crate::abac::check_repo_abac(&state, &repo_id, &auth).await {
        return (StatusCode::FORBIDDEN, reason).into_response();
    }

    // M13.2: Extract model context header before consuming the request body.
    let model_context = req
        .headers()
        .get("x-gyre-model-context")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);

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
            // Broadcast PushRejected domain event.
            let _ = state
                .event_tx
                .send(crate::domain_events::DomainEvent::PushRejected {
                    repo_id: repo_id.clone(),
                    branch: ref_updates
                        .first()
                        .map(|u| u.refname.clone())
                        .unwrap_or_default(),
                    agent_id: auth.agent_id.clone(),
                    reason: rejection.clone(),
                });
            return (StatusCode::FORBIDDEN, rejection).into_response();
        }
    }

    info!(%repo_path, updates = ref_updates.len(), "served git-receive-pack");

    // M13.2: Resolve agent context (task_id, ralph_step, parent_agent_id, spawned_by) for provenance.
    let (task_id, ralph_step, parent_agent_id, spawned_by_user_id) =
        resolve_agent_context(&state, &auth.agent_id).await;

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
        "ralph_step": ralph_step,
    });

    // M13.3: Append sideband feedback to git output.
    let mut output_with_feedback = output;
    let feedback = build_feedback_sideband(&branch, task_id.as_deref(), ralph_step.as_deref());
    output_with_feedback.extend_from_slice(&feedback);

    // Post-receive: record agent-commit mappings + broadcast PushAccepted event.
    // M14.2: Compute attestation level for commit provenance.
    let attestation_level = {
        let stacks = state.agent_stacks.lock().await;
        let has_stack = stacks.contains_key(auth.agent_id.as_str());
        drop(stacks);
        if has_stack {
            let policies = state.repo_stack_policies.lock().await;
            if policies.contains_key(repo_id.as_str()) {
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
    let task_id_clone = task_id.clone();
    let ralph_step_clone = ralph_step.clone();
    let parent_agent_id_clone = parent_agent_id.clone();
    let spawned_by_clone = spawned_by_user_id.clone();
    let model_context_clone = model_context.clone();
    let repo_id_clone = repo_id.clone();
    let branch_clone = branch.clone();
    let attestation_level_clone = attestation_level.to_string();
    let default_branch_clone = default_branch;
    tokio::spawn(async move {
        let commit_count = record_pushed_commits(
            &state_clone,
            &repo_path_clone,
            &ref_updates,
            &agent_id,
            task_id_clone.as_deref(),
            ralph_step_clone.as_deref(),
            parent_agent_id_clone.as_deref(),
            spawned_by_clone.as_deref(),
            model_context_clone.as_deref(),
            &attestation_level_clone,
        )
        .await;
        // Broadcast PushAccepted domain event.
        let _ = state_clone
            .event_tx
            .send(crate::domain_events::DomainEvent::PushAccepted {
                repo_id: repo_id_clone.clone(),
                branch: branch_clone,
                agent_id,
                commit_count,
                task_id: task_id_clone,
                ralph_step: ralph_step_clone,
            });
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
        for update in ref_updates.iter().filter(|u| u.refname == default_ref) {
            let now = crate::api::now_secs();
            crate::spec_registry::sync_spec_ledger(
                &state_clone.spec_ledger,
                &state_clone.spec_links_store,
                &repo_path_clone,
                &update.new_sha,
                now,
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
            // Knowledge graph: extract Rust symbols and architecture (M30b).
            let git_bin = std::env::var("GYRE_GIT_PATH").unwrap_or_else(|_| "git".to_string());
            crate::graph_extraction::extract_and_store_graph(
                &repo_path_clone,
                &repo_id_clone,
                &update.new_sha,
                state_clone.graph_store.as_ref(),
                &git_bin,
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
/// Returns (task_id, ralph_step, parent_agent_id, spawned_by_user_id).
async fn resolve_agent_context(
    state: &Arc<AppState>,
    agent_id: &str,
) -> (
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
) {
    let agent = match state.agents.find_by_id(&Id::new(agent_id)).await {
        Ok(Some(a)) => a,
        _ => return (None, None, None, None),
    };

    let parent_agent_id = agent.parent_id.map(|id| id.to_string());
    let spawned_by_user_id = agent.spawned_by.clone();

    let task_id = match &agent.current_task_id {
        Some(tid) => tid.to_string(),
        None => return (None, None, parent_agent_id, spawned_by_user_id),
    };

    // Derive ralph_step from the task's current status.
    let ralph_step = match state.tasks.find_by_id(&Id::new(&task_id)).await {
        Ok(Some(task)) => {
            use gyre_domain::TaskStatus;
            let step = match task.status {
                TaskStatus::Backlog => "spec",
                TaskStatus::InProgress => "implement",
                TaskStatus::Review => "review",
                TaskStatus::Done => "merge",
                TaskStatus::Blocked => "implement",
            };
            Some(step.to_string())
        }
        _ => None,
    };

    (
        Some(task_id),
        ralph_step,
        parent_agent_id,
        spawned_by_user_id,
    )
}

/// Build git sideband-64k pkt-lines carrying human-readable push feedback (M13.3).
fn build_feedback_sideband(
    branch: &str,
    task_id: Option<&str>,
    ralph_step: Option<&str>,
) -> Vec<u8> {
    let mut lines = vec![format!(
        "remote: [GYRE] Push accepted for branch {branch}\n"
    )];
    if let Some(tid) = task_id {
        lines.push(format!("remote: [GYRE] Task: {tid}\n"));
    }
    if let Some(step) = ralph_step {
        lines.push(format!("remote: [GYRE] Ralph step: {step}\n"));
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
    ralph_step: Option<&str>,
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

    // Derive RalphStep from the string for provenance.
    let ralph_step_enum = ralph_step.and_then(gyre_domain::RalphStep::from_str);

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
                ralph_step_enum.clone(),
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
    let gate_names = {
        let gates = state.repo_push_gates.lock().await;
        gates.get(repo_id).cloned().unwrap_or_default()
    };
    if gate_names.is_empty() {
        return Ok(());
    }

    // M14.2: Resolve agent stack fingerprint and repo policy once for all ref updates.
    let stack_fingerprint = {
        let stacks = state.agent_stacks.lock().await;
        stacks.get(agent_id).map(|s| s.fingerprint())
    };
    let required_fingerprint = {
        let policies = state.repo_stack_policies.lock().await;
        policies.get(repo_id).cloned()
    };

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
                    let mut approvals = state.spec_approvals.lock().await;
                    let mut invalidated = 0usize;
                    for approval in approvals.values_mut() {
                        if approval.is_active()
                            && stale_paths.iter().any(|&p| approval.spec_path == p)
                        {
                            approval.revoked_at = Some(now);
                            approval.revoked_by = Some("system:spec-lifecycle".to_string());
                            approval.revocation_reason = Some(format!(
                                "spec file {} in push to {}",
                                match status_char {
                                    'M' => "modified",
                                    'D' => "deleted",
                                    'R' => "renamed",
                                    _ => "changed",
                                },
                                default_branch
                            ));
                            invalidated += 1;
                        }
                    }
                    drop(approvals);
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

            match state.tasks.create(&task).await {
                Err(e) => warn!(title, "spec-lifecycle: failed to create task: {e}"),
                Ok(()) => {
                    info!(title, "spec-lifecycle: created task for spec change");
                    let _ = state
                        .event_tx
                        .send(crate::domain_events::DomainEvent::SpecChanged {
                            repo_id: repo_id.to_string(),
                            spec_path: path,
                            change_kind: match status_char {
                                'A' => "added",
                                'M' => "modified",
                                'D' => "deleted",
                                'R' => "renamed",
                                _ => "unknown",
                            }
                            .to_string(),
                            task_id: task_id.to_string(),
                        });
                    let _ = state
                        .event_tx
                        .send(crate::domain_events::DomainEvent::TaskCreated {
                            id: task_id.to_string(),
                        });
                }
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

/// Post-push auto-detection: scan Cargo.toml changes for path deps pointing to sibling repos.
///
/// On pushes to the default branch, reads the Cargo.toml at the new SHA and creates
/// DependencyEdge records for any path deps whose basename matches a known Gyre repo name.
pub(crate) async fn detect_dependencies_on_push(
    state: &Arc<AppState>,
    repo_id: &str,
    repo_path: &str,
    new_sha: &str,
) {
    use gyre_common::Id;
    use gyre_domain::{DependencyEdge, DependencyType, DetectionMethod};

    let git_bin = std::env::var("GYRE_GIT_PATH").unwrap_or_else(|_| "git".to_string());

    // Check whether Cargo.toml was changed in this commit.
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
    let has_cargo_toml = changed_files
        .lines()
        .any(|f| f == "Cargo.toml" || f.ends_with("/Cargo.toml"));

    if !has_cargo_toml {
        return;
    }

    // Read Cargo.toml content from the new commit.
    let toml_out = match Command::new(&git_bin)
        .arg("-C")
        .arg(repo_path)
        .arg("show")
        .arg(format!("{new_sha}:Cargo.toml"))
        .output()
        .await
    {
        Ok(o) if o.status.success() => o.stdout,
        _ => return,
    };

    let toml_content = String::from_utf8_lossy(&toml_out);
    let path_deps = detect_cargo_path_deps(&toml_content);

    if path_deps.is_empty() {
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

    for path_dep in path_deps {
        // Extract basename: "../other-repo" -> "other-repo"
        let basename = std::path::Path::new(&path_dep)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(&path_dep)
            .to_string();

        if let Some(target_repo) = all_repos.iter().find(|r| r.name == basename) {
            if target_repo.id.as_str() == repo_id {
                continue;
            }

            let edge = DependencyEdge::new(
                Id::new(uuid::Uuid::new_v4().to_string()),
                source_id.clone(),
                target_repo.id.clone(),
                DependencyType::Code,
                "Cargo.toml",
                basename.as_str(),
                DetectionMethod::CargoToml,
                now,
            );

            if let Err(e) = state.dependencies.save(&edge).await {
                warn!("dep-detection: failed to save edge: {e}");
            } else {
                tracing::info!(
                    source_repo = repo_id,
                    target_repo = %target_repo.id,
                    "auto-detected Cargo.toml path dependency"
                );
            }
        }
    }
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
    use gyre_domain::Repository;
    use http::{Request, StatusCode};
    use std::sync::Arc;
    use tempfile::TempDir;
    use tower::ServiceExt;

    // Build a router with git routes and a real bare repo.
    async fn git_app_with_repo() -> (Router, Arc<crate::AppState>, TempDir, String, String) {
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
        let project_id = "test-proj-id";

        let repo = Repository {
            id: Id::new("repo-1"),
            project_id: Id::new(project_id),
            name: "my-repo".to_string(),
            path: repo_path.to_str().unwrap().to_string(),
            default_branch: "main".to_string(),
            created_at: 0,
            is_mirror: false,
            mirror_url: None,
            mirror_interval_secs: None,
            last_mirror_sync: None,
            workspace_id: None,
        };
        state.repos.create(&repo).await.unwrap();

        let repo_path_str = repo_path.to_str().unwrap().to_string();

        let app = Router::new()
            .route("/git/:project/:repo/info/refs", get(git_info_refs))
            .route("/git/:project/:repo/git-upload-pack", post(git_upload_pack))
            .route(
                "/git/:project/:repo/git-receive-pack",
                post(git_receive_pack),
            )
            .with_state(state.clone());

        (app, state, tmp, project_id.to_string(), repo_path_str)
    }

    fn auth_header() -> &'static str {
        "Bearer test-token"
    }

    #[tokio::test]
    async fn info_refs_upload_pack_without_auth_returns_401() {
        let (app, _state, _tmp, project, _path) = git_app_with_repo().await;
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/git/{project}/my-repo.git/info/refs?service=git-upload-pack"
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
        let (app, _state, _tmp, project, _path) = git_app_with_repo().await;
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/git/{project}/my-repo.git/info/refs?service=git-upload-pack"
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
        let (app, _state, _tmp, project, _path) = git_app_with_repo().await;
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/git/{project}/my-repo.git/info/refs?service=git-upload-pack"
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
        let (app, _state, _tmp, project, _path) = git_app_with_repo().await;
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/git/{project}/my-repo.git/info/refs?service=git-receive-pack"
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
        let (app, _state, _tmp, project, _path) = git_app_with_repo().await;
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/git/{project}/my-repo.git/info/refs?service=git-bogus"
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
        let (app, _state, _tmp, project, _path) = git_app_with_repo().await;
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/git/{project}/no-such-repo.git/info/refs?service=git-upload-pack"
                    ))
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
        let (app, _state, _tmp, project, _path) = git_app_with_repo().await;
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/git/{project}/my-repo.git/info/refs?service=git-upload-pack"
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
        let (app, state, _tmp, project, _path) = git_app_with_repo().await;
        state
            .agent_tokens
            .lock()
            .await
            .insert("agent-7".to_string(), "my-agent-token".to_string());

        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/git/{project}/my-repo.git/info/refs?service=git-upload-pack"
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
        let (_, state, _tmp, project, _path) = git_app_with_repo().await;

        let app = Router::new()
            .route("/git/:project/:repo/info/refs", get(git_info_refs))
            .route("/git/:project/:repo/git-upload-pack", post(git_upload_pack))
            .route(
                "/git/:project/:repo/git-receive-pack",
                post(git_receive_pack),
            )
            .with_state(state);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let clone_dir = TempDir::new().unwrap();
        let url = format!("http://127.0.0.1:{port}/git/{project}/my-repo.git");

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
}
