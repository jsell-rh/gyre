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
use gyre_domain::{AuditEvent, AuditEventType, AuditOutcome};
use gyre_ports::{GateOutcome, PushContext};
use serde::Deserialize;
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tracing::{debug, error, info, warn};

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
///      For the genuine system principal (the global dev/system token —
///      `is_system_principal()`, never a bare agent_id string match), a
///      tenant-scoped miss falls back to a slug-only lookup: the system
///      principal is the bootstrap principal that created the tenant and
///      workspace (gyre bootstrap --dev creates tenant "dev" while the
///      system principal's tenant_id stays "default"), so a strict filter
///      would 404 the bootstrap flow's own push. The fallback grants
///      nothing the token lacks elsewhere (it bypasses ABAC on REST), and
///      the repo is still resolved inside the workspace the slug names.
///   2. Look up the repo by name within that workspace.
async fn resolve_repo_by_slug(
    state: &Arc<AppState>,
    auth: &crate::auth::AuthenticatedAgent,
    workspace_slug: &str,
    repo_name: &str,
) -> Result<gyre_domain::Repository, Response> {
    let repo_name = repo_name.strip_suffix(".git").unwrap_or(repo_name);

    let mut workspace = state
        .workspaces
        .find_by_slug(&Id::new(&auth.tenant_id), workspace_slug)
        .await
        .map_err(|e| git_err(format!("db error: {e}")))?;
    if workspace.is_none() && auth.is_system_principal() {
        // System-principal fallback (see doc comment). `find_by_slug` is
        // tenant-scoped, so the fallback scans by slug alone. The system
        // principal is the bootstrap principal that created the tenant and
        // workspace; a strict tenant filter would 404 the bootstrap flow's
        // own push (gyre bootstrap --dev creates tenant "dev" while the
        // system principal's tenant_id stays "default").
        workspace = state
            .workspaces
            .list()
            .await
            .map_err(|e| git_err(format!("db error: {e}")))?
            .into_iter()
            .find(|ws| ws.slug == workspace_slug);
    }
    let workspace =
        workspace.ok_or_else(|| not_found(format!("workspace '{workspace_slug}' not found")))?;

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

// ---------------------------------------------------------------------------
// Repo access enforcement (user-management.md §Repo Access)
// ---------------------------------------------------------------------------

/// Enforce repo access for a user principal: repo access is inherited from
/// workspace membership (per the §Repo Access table) and per-repo overrides
/// can restrict but not expand it.
///
/// - The system principal (bootstrap token) is allowed.
/// - Agent tokens and other principals without a user identity are allowed:
///   agents are scoped to their repos at spawn time and their pushes run
///   through the gate chain; membership is a human-authorization concept.
/// - Global TenantAdmins are allowed (tenant-wide administration).
/// - Every other user principal MUST hold a membership in the repo's
///   workspace whose effective role (workspace role capped by any per-repo
///   override) carries `perm`; otherwise 403 with an `RbacDenied` audit event.
async fn authorize_repo_access(
    state: &Arc<AppState>,
    auth: &AuthenticatedAgent,
    repo: &gyre_domain::Repository,
    perm: gyre_domain::RepoPermission,
) -> Result<(), Box<Response>> {
    if auth.is_system_principal() || auth.user_id.is_none() {
        return Ok(());
    }
    if auth.roles.contains(&gyre_domain::UserRole::Admin) {
        return Ok(());
    }
    let Some(user_id) = &auth.user_id else {
        return Ok(());
    };

    let denial = |reason: String, effective: Option<gyre_domain::WorkspaceRole>| -> Response {
        warn!(
            user_id = %user_id,
            repo_id = %repo.id,
            permission = perm.as_str(),
            "git http 403: repo access denied ({reason})"
        );
        // RbacDenied audit event (observability.md §Audit Event Types):
        // the membership/override check rejected the operation.
        let event = AuditEvent::new(
            Id::new(uuid::Uuid::new_v4().to_string()),
            AuditEventType::RbacDenied,
            Some(Id::new(auth.agent_id.clone())),
            Some(user_id.clone()),
            None,
            Some(repo.workspace_id.clone()),
            Some(repo.id.clone()),
            "repo".to_string(),
            Some(repo.id.to_string()),
            AuditOutcome::Blocked,
            serde_json::json!({
                "permission": perm.as_str(),
                "reason": reason,
                "effective_role": effective.map(|r| r.as_str()),
            }),
            None,
            None,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        );
        let state = Arc::clone(state);
        tokio::spawn(async move {
            if let Err(e) = state.audit.record(&event).await {
                error!("failed to record RbacDenied audit event: {e}");
            }
        });
        (
            StatusCode::FORBIDDEN,
            format!("repo access denied: {reason}"),
        )
            .into_response()
    };

    let membership = match state
        .workspace_memberships
        .find_by_user_and_workspace(user_id, &repo.workspace_id)
        .await
    {
        Ok(Some(m)) => m,
        Ok(None) => {
            return Err(Box::new(denial(
                "no workspace membership".to_string(),
                None,
            )));
        }
        Err(e) => return Err(Box::new(git_err(format!("db error: {e}")))),
    };

    // Per-repo override caps the workspace role (restrict-only).
    let override_role = match state
        .repo_access_overrides
        .find_by_repo_and_user(&repo.id, user_id)
        .await
    {
        Ok(o) => o.map(|o| o.restricted_role),
        Err(e) => return Err(Box::new(git_err(format!("db error: {e}")))),
    };
    let effective = gyre_domain::effective_repo_role(membership.role, override_role);

    if !effective.has_repo_permission(perm) {
        return Err(Box::new(denial(
            format!(
                "effective role {} lacks the {} permission",
                effective.as_str(),
                perm.as_str()
            ),
            Some(effective),
        )));
    }
    Ok(())
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

    // Repo access enforcement (user-management.md §Repo Access): fetching
    // (upload-pack) needs Read, pushing (receive-pack) needs Write.
    let needed_perm = if service == "git-upload-pack" {
        gyre_domain::RepoPermission::Read
    } else {
        gyre_domain::RepoPermission::Write
    };
    let repo = match resolve_repo_by_slug(&state, &auth, &workspace_slug, &repo_name).await {
        Ok(r) => r,
        Err(r) => return r,
    };

    // repo-lifecycle.md §4 step 5: archived repos are read-only. Reject the
    // push at the service-advertisement stage with an `ERR` pkt-line — the
    // git-smart-HTTP convention for service-level errors — so the client
    // actually displays the reason ("fatal: remote error: ... "). A plain
    // HTTP 403 here would hide the body from `git push` output entirely.
    // The receive-pack POST handler keeps its own archived 403 as
    // defense-in-depth for clients that skip the advertisement.
    if subcommand == "receive-pack" && repo.is_archived() {
        warn!(
            agent_id = %auth.agent_id,
            workspace_slug = %workspace_slug,
            repo_name = %repo_name,
            "info/refs receive-pack advertisement rejected: repository is archived"
        );
        let mut body = service_header(&service);
        body.extend(pkt_line("ERR push rejected: repository is archived\n"));
        return Response::builder()
            .status(StatusCode::OK)
            .header("Content-Type", content_type)
            .header("Cache-Control", "no-cache")
            .body(Body::from(body))
            .unwrap();
    }

    if let Err(resp) = authorize_repo_access(&state, &auth, &repo, needed_perm).await {
        return *resp;
    }
    let repo_path = repo.path.clone();

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
// task-104: source control audit events (observability.md §Audit Event Types)
// ---------------------------------------------------------------------------

/// Record a `GitClone` audit event — an agent cloned/fetched a repo over
/// smart HTTP (`git-upload-pack`).
async fn emit_git_clone_audit(
    state: &AppState,
    repo: &gyre_domain::Repository,
    auth: &AuthenticatedAgent,
) {
    let repo_id = repo.id.to_string();
    let event = AuditEvent::new(
        Id::new(uuid::Uuid::new_v4().to_string()),
        AuditEventType::GitClone,
        Some(Id::new(auth.agent_id.clone())),
        auth.user_id.clone(),
        None,
        Some(repo.workspace_id.clone()),
        Some(repo.id.clone()),
        "repo".to_string(),
        Some(repo_id.clone()),
        AuditOutcome::Success,
        serde_json::json!({
            "repo_id": repo_id,
            "agent_id": auth.agent_id,
        }),
        None,
        None,
        crate::api::now_secs(),
    );
    crate::api::audit::emit_server_audit_event(state, event).await;
}

/// Result of one pre-accept gate evaluation for a push.
#[derive(Debug, Clone)]
pub(crate) struct PushGateOutcomeEntry {
    pub gate_name: String,
    pub passed: bool,
    pub reason: Option<String>,
}

/// A pre-accept gate rejection: which gate rejected and why.
pub(crate) struct PushGateRejection {
    pub gate_name: String,
    pub reason: String,
    pub branch: String,
}

/// Serialize gate outcomes for the `push_gate_results` detail field of a
/// `GitPushAccepted` audit event.
fn gate_results_json(outcomes: &[PushGateOutcomeEntry]) -> serde_json::Value {
    serde_json::Value::Array(
        outcomes
            .iter()
            .map(|o| {
                serde_json::json!({
                    "gate": o.gate_name,
                    "passed": o.passed,
                    "reason": o.reason,
                })
            })
            .collect(),
    )
}

/// Record a `GitPushRejected` audit event — a push gate rejected the push
/// (observability.md: detail fields repo_id, branch, gate_name, reason).
async fn emit_git_push_rejected_audit(
    state: &AppState,
    repo_id: &str,
    workspace_id: &Id,
    agent_id: &str,
    branch: &str,
    gate_name: &str,
    reason: &str,
) {
    let event = AuditEvent::new(
        Id::new(uuid::Uuid::new_v4().to_string()),
        AuditEventType::GitPushRejected,
        Some(Id::new(agent_id)),
        None,
        None,
        Some(workspace_id.clone()),
        Some(Id::new(repo_id)),
        "repo".to_string(),
        Some(repo_id.to_string()),
        AuditOutcome::Blocked,
        serde_json::json!({
            "repo_id": repo_id,
            "branch": branch,
            "gate_name": gate_name,
            "reason": reason,
        }),
        None,
        None,
        crate::api::now_secs(),
    );
    crate::api::audit::emit_server_audit_event(state, event).await;
}

/// Record a `GitPushAccepted` audit event for one accepted ref update
/// (observability.md: detail fields repo_id, branch, commit_sha, agent_id,
/// push_gate_results).
async fn emit_git_push_accepted_audit(
    state: &AppState,
    repo_id: &str,
    workspace_id: &Id,
    agent_id: &str,
    branch: &str,
    commit_sha: &str,
    gate_results: &[PushGateOutcomeEntry],
) {
    let event = AuditEvent::new(
        Id::new(uuid::Uuid::new_v4().to_string()),
        AuditEventType::GitPushAccepted,
        Some(Id::new(agent_id)),
        None,
        None,
        Some(workspace_id.clone()),
        Some(Id::new(repo_id)),
        "repo".to_string(),
        Some(repo_id.to_string()),
        AuditOutcome::Success,
        serde_json::json!({
            "repo_id": repo_id,
            "branch": branch,
            "commit_sha": commit_sha,
            "agent_id": agent_id,
            "push_gate_results": gate_results_json(gate_results),
        }),
        None,
        None,
        crate::api::now_secs(),
    );
    crate::api::audit::emit_server_audit_event(state, event).await;
}

/// Record a `SpecChanged` audit event — a spec file was modified in a push to
/// the default branch (observability.md: detail fields repo_id, spec_path,
/// change_kind, task_id).
async fn emit_spec_changed_audit(
    state: &AppState,
    repo_id: &str,
    workspace_id: Option<&Id>,
    spec_path: &str,
    change_kind: &str,
    task_id: Option<&str>,
) {
    let event = AuditEvent::new(
        Id::new(uuid::Uuid::new_v4().to_string()),
        AuditEventType::SpecChanged,
        None,
        None,
        None,
        workspace_id.cloned(),
        Some(Id::new(repo_id)),
        "spec".to_string(),
        Some(spec_path.to_string()),
        AuditOutcome::Success,
        serde_json::json!({
            "repo_id": repo_id,
            "spec_path": spec_path,
            "change_kind": change_kind,
            "task_id": task_id,
        }),
        None,
        None,
        crate::api::now_secs(),
    );
    crate::api::audit::emit_server_audit_event(state, event).await;
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
    let repo = match resolve_repo_by_slug(&state, &auth, &workspace_slug, &repo_name).await {
        Ok(r) => r,
        Err(r) => return r,
    };
    // Repo access enforcement: clone/fetch needs Read
    // (user-management.md §Repo Access).
    if let Err(resp) =
        authorize_repo_access(&state, &auth, &repo, gyre_domain::RepoPermission::Read).await
    {
        return *resp;
    }
    let repo_path = repo.path.clone();

    let body_bytes = match axum::body::to_bytes(req.into_body(), 64 * 1024 * 1024).await {
        Ok(b) => b,
        Err(e) => return git_err(format!("failed to read request body: {e}")),
    };

    let output = match run_git_stateless("upload-pack", &repo_path, &body_bytes).await {
        Ok(o) => o,
        Err(e) => return git_err(e),
    };

    info!(%repo_path, "served git-upload-pack");

    // task-104: GitClone audit event (observability.md §Audit Event Types) —
    // an agent cloned/fetched the repo over smart HTTP.
    emit_git_clone_audit(&state, &repo, &auth).await;

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
    // Task-134: scoped gate-agent tokens are read-only — a `review:submit`
    // reviewer or `validation:report` validator may read MR context, never
    // push code (agent-gates.md §Gate Agent Lifecycle: "read-only access").
    // Caller-scoped capability, exact-match (review F3): checked before
    // workspace/repo resolution so the denial does not depend on (or leak
    // the existence of) the target.
    if let Some(scope) = auth
        .jwt_claims
        .as_ref()
        .and_then(|c| c.get("scope"))
        .and_then(|s| s.as_str())
    {
        if scope == crate::auth::REVIEW_SUBMIT_SCOPE
            || scope == crate::auth::VALIDATION_REPORT_SCOPE
        {
            warn!(
                agent_id = %auth.agent_id,
                workspace_slug = %workspace_slug,
                repo_name = %repo_name,
                "git-receive-pack 403: scoped gate-agent token cannot push"
            );
            return (
                StatusCode::FORBIDDEN,
                "push rejected: gate-agent scoped tokens are read-only".to_string(),
            )
                .into_response();
        }
    }

    let resolved = match resolve_repo_by_slug(&state, &auth, &workspace_slug, &repo_name).await {
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
    // repo-lifecycle.md §4 Archive step 5: archived repos are read-only —
    // reject the push before the packfile is processed.
    if resolved.is_archived() {
        warn!(
            agent_id = %auth.agent_id,
            workspace_slug = %workspace_slug,
            repo_name = %repo_name,
            "git-receive-pack 403: repository is archived"
        );
        return (
            StatusCode::FORBIDDEN,
            "push rejected: repository is archived".to_string(),
        )
            .into_response();
    }
    // Repo access enforcement: pushing needs Write
    // (user-management.md §Repo Access).
    if let Err(resp) =
        authorize_repo_access(&state, &auth, &resolved, gyre_domain::RepoPermission::Write).await
    {
        return *resp;
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
        // task-104: AbacDenied audit event — the repo-level ABAC check
        // rejected the push outside the ABAC middleware.
        crate::abac_middleware::record_abac_denied(
            &state,
            &auth,
            "repo",
            "push",
            None,
            "POST /git/:workspace_slug/:repo_name/git-receive-pack",
        )
        .await;
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

    // Review repair (task-104): receive-pack reports per-ref rejections
    // (non-fast-forward, stale client, `receive.denyNonFastForwards`, ref
    // lock contention) in the report-status stream back to the client — the
    // process still exits 0. The ref updates parsed from the request are
    // therefore only *requests*. Verify which of them actually landed
    // before any push-acceptance signaling runs: a declined update must not
    // produce a GitPushAccepted audit event (spec trigger: "Smart HTTP push
    // accepted") or any other post-receive acceptance processing.
    let applied_updates = verify_applied_ref_updates(&repo_path, &ref_updates).await;

    // Pre-accept gate checks: run after git accepts packfile, undo refs on failure.
    // task-104: gate outcomes are captured so the GitPushAccepted audit event
    // carries the spec'd push_gate_results detail field.
    let mut push_gate_outcomes: Vec<PushGateOutcomeEntry> = Vec::new();
    if !ref_updates.is_empty() {
        match check_pre_accept_gates(&state, &repo_id, &repo_path, &ref_updates, &auth.agent_id)
            .await
        {
            Ok(outcomes) => push_gate_outcomes = outcomes,
            Err(rejection) => {
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
                            "reason": format!("push rejected by gate '{}': {}", rejection.gate_name, rejection.reason),
                        })),
                    )
                    .await;
                // task-104: GitPushRejected audit event (gate rejection).
                emit_git_push_rejected_audit(
                    &state,
                    &repo_id,
                    &repo_workspace_id,
                    &auth.agent_id,
                    &rejection.branch,
                    &rejection.gate_name,
                    &rejection.reason,
                )
                .await;
                return (
                    StatusCode::FORBIDDEN,
                    format!(
                        "push rejected by gate '{}': {}",
                        rejection.gate_name, rejection.reason
                    ),
                )
                    .into_response();
            }
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
                // task-104: GitPushRejected audit event (constraint gate).
                emit_git_push_rejected_audit(
                    &state,
                    &repo_id,
                    &repo_workspace_id,
                    &auth.agent_id,
                    &ref_updates
                        .first()
                        .map(|u| u.refname.trim_start_matches("refs/heads/").to_string())
                        .unwrap_or_default(),
                    "constraint-enforcement",
                    &rejection,
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
                // task-104: GitPushRejected audit event (manifest gate).
                emit_git_push_rejected_audit(
                    &state,
                    &repo_id,
                    &repo_workspace_id,
                    &auth.agent_id,
                    &default_branch,
                    "manifest-coverage",
                    &rejection,
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
                // task-104: GitPushRejected audit event (spec-link cycle gate).
                emit_git_push_rejected_audit(
                    &state,
                    &repo_id,
                    &repo_workspace_id,
                    &auth.agent_id,
                    &default_branch,
                    "spec-link-cycles",
                    &rejection,
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
    let push_gate_outcomes_clone = push_gate_outcomes.clone();
    tokio::spawn(async move {
        let commit_count = record_pushed_commits(
            &state_clone,
            &repo_path_clone,
            &applied_updates,
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
                &applied_updates,
                crate::api::now_secs(),
            )
            .await;
        }
        // Emit PushAccepted event via unified message bus — only when git
        // actually applied at least one ref update. A push git declined in
        // full (e.g. every update non-fast-forward) is not an accepted push.
        if !applied_updates.is_empty() {
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
        }

        // task-104: GitPushAccepted audit event (observability.md §Audit
        // Event Types) — one event per ref update git actually applied
        // (verified via rev-parse; git-declined updates emit nothing),
        // carrying the branch, commit SHA, and the pre-accept gate results.
        for update in applied_updates.iter() {
            emit_git_push_accepted_audit(
                &state_clone,
                &repo_id_clone,
                &push_workspace_id,
                &agent_id,
                update
                    .refname
                    .strip_prefix("refs/heads/")
                    .unwrap_or(&update.refname),
                &update.new_sha,
                &push_gate_outcomes_clone,
            )
            .await;
        }

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

                    // TASK-061 (§7.2): Populate attestation chain ABAC subject
                    // attributes and evaluate policies with action=push,
                    // resource_type=attestation. Audit-only — logged but not enforced.
                    if let Some(leaf) = attestations.iter().max_by_key(|a| a.metadata.chain_depth) {
                        let chain = state_clone
                            .chain_attestations
                            .load_chain(&leaf.id)
                            .await
                            .unwrap_or_default();
                        let chain_depth = leaf.metadata.chain_depth;
                        let signer = gyre_domain::root_signer(&chain);
                        let cc = gyre_domain::constraint_count(&chain) as i64;

                        let mut ctx = crate::policy_engine::AttributeContext::default();
                        ctx.set("subject.type", "agent");
                        ctx.set("subject.id", &agent_id);
                        ctx.set("subject.tenant_id", &push_tenant_id);
                        ctx.set_number("subject.chain_depth", chain_depth as i64);
                        ctx.set_number("subject.constraint_count", cc);
                        if let Some(ref rs) = signer {
                            ctx.set("subject.root_signer", rs);
                        }
                        // An attestation chain was located for this push — the
                        // attribute `builtin:require-signed-authorization`
                        // (§7.2) keys its unless-clause on.
                        ctx.set_bool("subject.attestation_chain_present", true);
                        ctx.set("resource.type", "attestation");
                        ctx.set("resource.repo_id", &repo_id_clone);

                        let policies = state_clone.policies.list().await.unwrap_or_default();
                        let eval_result =
                            crate::policy_engine::evaluate(policies, &ctx, "push", "attestation");

                        tracing::info!(
                            task_id = %tid,
                            repo_id = %repo_id_clone,
                            chain_depth = chain_depth,
                            root_signer = ?signer,
                            constraint_count = cc,
                            decision = ?eval_result.effect,
                            matched_policy = ?eval_result.matched_policy,
                            "attestation ABAC evaluation for push (audit-only)" // enforcement-mode:ok
                        );
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
            // Only for ref updates git actually applied.
            let constraint_ref_updates: Vec<(String, String, String)> = applied_updates
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

        // Spec lifecycle: auto-create tasks for spec changes on the default
        // branch — only for ref updates git actually applied.
        process_spec_lifecycle(
            &state_clone,
            &repo_id_clone,
            &repo_path_clone,
            &default_branch_clone,
            &applied_updates,
        )
        .await;

        // Repo-defined personas (platform-model.md §2): detect persona file
        // changes on the default branch, update the entity, reset approval,
        // auto-create the approval task and notify the owner.
        process_persona_changes(
            &state_clone,
            &repo_id_clone,
            &repo_path_clone,
            &default_branch_clone,
            &ref_updates,
        )
        .await;

        // merge-dependencies.md §2: re-evaluate branch lineage dependencies
        // when a push updates branches. Covers both directions:
        // - the pushed branch itself was rebased/forked (own MR re-checked),
        // - the pushed branch is a *parent* in a lineage edge (spec §2
        //   item 3: "if the parent MR is later rebased, the dependency is
        //   re-evaluated") — the dependent child's edge is re-checked even
        //   though the child's branch never moved.
        let pushed_branches: Vec<String> = applied_updates
            .iter()
            .filter_map(|u| u.refname.strip_prefix("refs/heads/").map(str::to_string))
            .collect();
        if !pushed_branches.is_empty() {
            for mr_id in crate::api::merge_requests::mrs_needing_lineage_reeval(
                &state_clone,
                &gyre_common::Id::new(&repo_id_clone),
                &pushed_branches,
            )
            .await
            {
                crate::api::merge_requests::reevaluate_lineage_deps(&state_clone, &mr_id).await;
            }
        }
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
        for update in applied_updates.iter().filter(|u| u.refname == default_ref) {
            let now = crate::api::now_secs();
            crate::spec_registry::sync_spec_ledger(
                &state_clone.spec_ledger,
                &state_clone.spec_links_store,
                &state_clone.spec_link_repo,
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
            // search.md §Index Updates: "Spec pushed -> index updated (content
            // extracted from git blob)". Runs after the ledger sync so
            // approval_status/owner facets reflect this push.
            crate::search_indexer::index_repo_specs(
                &state_clone,
                &repo_id_clone,
                &repo_workspace_id_str,
                &repo_path_clone,
                &update.new_sha,
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
            // §9 spec-assertion notifications are scoped by the repo's
            // workspace — every push type notifies, not just agent pushes.
            let notification_scope = Some(crate::graph_extraction::PushNotificationScope {
                workspace_id: repo_workspace_id_str.clone(),
                tenant_id: push_tenant_id.clone(),
            });
            crate::graph_extraction::extract_and_store_graph(
                &repo_path_clone,
                &repo_id_clone,
                &update.new_sha,
                Arc::clone(&state_clone.graph_store),
                &git_bin,
                agent_push_ctx,
                divergence_ports,
                Arc::clone(&state_clone.spec_assertion_results),
                notification_scope,
                Arc::new(crate::graph_extraction::CallGraphTelemetry {
                    kv: Arc::clone(&state_clone.kv_store),
                    metrics: Arc::clone(&state_clone.metrics),
                }),
            )
            .await;
        }

        // search.md §Index Updates: code search (file content) is indexed
        // asynchronously on push via the background job registry — eventually
        // consistent, never inline in the push path. The job walks the default
        // branch, so only default-branch pushes need to trigger it.
        if ref_updates
            .iter()
            .any(|u| u.refname == format!("refs/heads/{default_branch_clone}"))
        {
            let st = Arc::clone(&state_clone);
            tokio::spawn(async move {
                if let Err(e) = st
                    .job_registry
                    .trigger("search_code_index", Arc::clone(&st))
                    .await
                {
                    tracing::warn!("search-index: code index job failed: {e}");
                }
            });
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
            // search.md §Index Updates: "Commit pushed -> commit messages
            // indexed" — message and author are read from the git object store.
            crate::search_indexer::index_pushed_commit(
                state,
                &repo.id.to_string(),
                &repo.workspace_id.to_string(),
                repo_path,
                sha,
                &update.refname,
                agent_id,
            )
            .await;
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
/// Returns `Ok(outcomes)` — one entry per evaluated gate — if all gates pass,
/// or `Err(rejection)` naming the rejecting gate if any gate rejects the push.
/// The outcomes feed the `push_gate_results` detail of the GitPushAccepted
/// audit event (task-104).
async fn check_pre_accept_gates(
    state: &Arc<AppState>,
    repo_id: &str,
    repo_path: &str,
    ref_updates: &[RefUpdate],
    agent_id: &str,
) -> Result<Vec<PushGateOutcomeEntry>, PushGateRejection> {
    // Get gate names configured for this repo.
    let gate_names = state
        .repo_push_gates
        .get_for_repo(repo_id)
        .await
        .unwrap_or_default();
    if gate_names.is_empty() {
        return Ok(Vec::new());
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
        .flatten()
        .map(|value| crate::api::stack_attest::parse_stack_policy(&value).fingerprint);

    let git_bin = std::env::var("GYRE_GIT_PATH").unwrap_or_else(|_| "git".to_string());

    let mut outcomes: Vec<PushGateOutcomeEntry> = Vec::new();
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
            branch: branch.clone(),
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
                    GateOutcome::Passed => outcomes.push(PushGateOutcomeEntry {
                        gate_name: gate_name.clone(),
                        passed: true,
                        reason: None,
                    }),
                    GateOutcome::Failed(reason) => {
                        warn!(
                            repo_id,
                            gate = gate_name,
                            refname = %update.refname,
                            %reason,
                            "pre-accept gate rejected push"
                        );
                        return Err(PushGateRejection {
                            gate_name: gate_name.clone(),
                            reason,
                            branch: branch.clone(),
                        });
                    }
                }
            }
        }
    }

    Ok(outcomes)
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

/// Verify which of the requested ref updates `git receive-pack` actually
/// applied (task-104 review repair).
///
/// receive-pack reports per-ref rejections (non-fast-forward, stale client,
/// `receive.denyNonFastForwards`, ref lock contention) in the report-status
/// stream back to the client — the process still exits 0. A ref update parsed
/// from the request body is therefore only a *request*. Confirm each ref now
/// points at the requested new SHA (`git rev-parse <refname> == new_sha`);
/// updates git declined are dropped so no acceptance signaling runs for them.
async fn verify_applied_ref_updates(repo_path: &str, ref_updates: &[RefUpdate]) -> Vec<RefUpdate> {
    let git_bin = std::env::var("GYRE_GIT_PATH").unwrap_or_else(|_| "git".to_string());
    let mut applied = Vec::with_capacity(ref_updates.len());

    for update in ref_updates {
        // Refnames come from the client request body; only plain ref paths
        // are ever handed to the git subprocess.
        if !update.refname.starts_with("refs/") {
            warn!(
                repo_path,
                refname = %update.refname,
                "ref update with non-'refs/' refname; treating as not applied"
            );
            continue;
        }
        let resolved = Command::new(&git_bin)
            .arg("-C")
            .arg(repo_path)
            .arg("rev-parse")
            .arg("--verify")
            // Refnames are client-controlled; never let one parse as an option.
            .arg("--end-of-options")
            .arg(&update.refname)
            .output()
            .await;
        let is_applied = match resolved {
            Ok(o) if o.status.success() => {
                let sha = String::from_utf8_lossy(&o.stdout)
                    .trim()
                    .to_ascii_lowercase();
                sha == update.new_sha.to_ascii_lowercase()
            }
            _ => false,
        };
        if is_applied {
            applied.push(RefUpdate {
                old_sha: update.old_sha.clone(),
                new_sha: update.new_sha.clone(),
                refname: update.refname.clone(),
            });
        } else {
            warn!(
                repo_path,
                refname = %update.refname,
                new_sha = %update.new_sha,
                "git receive-pack declined this ref update; not recording push acceptance for it"
            );
        }
    }

    applied
}

// ---------------------------------------------------------------------------
// Spec lifecycle: auto-create tasks on spec changes in the default branch
// ---------------------------------------------------------------------------

/// Classify a spec file change and return (title, labels, priority).
///
/// Priorities come from the repo's `SpecLifecycleConfig`
/// (spec-lifecycle.md §Configuration): new/rename -> `default_priority_new`,
/// modified -> `default_priority_modified`, deleted ->
/// `default_priority_deleted`.
fn classify_spec_change(
    status_char: char,
    path: &str,
    old_path: Option<&str>,
    config: &gyre_domain::SpecLifecycleConfig,
) -> Option<(String, Vec<String>, gyre_domain::TaskPriority)> {
    match status_char {
        'A' => Some((
            format!("Implement spec: {path}"),
            vec![
                "spec-implementation".to_string(),
                "auto-created".to_string(),
            ],
            config.default_priority_new.clone(),
        )),
        'M' => Some((
            format!("Review spec change: {path}"),
            vec!["spec-drift-review".to_string(), "auto-created".to_string()],
            config.default_priority_modified.clone(),
        )),
        'D' => Some((
            format!("Handle spec removal: {path}"),
            vec!["spec-deprecated".to_string(), "auto-created".to_string()],
            config.default_priority_deleted.clone(),
        )),
        'R' => {
            let old = old_path.unwrap_or(path);
            Some((
                format!("Update spec references: {old} -> {path}"),
                vec!["spec-housekeeping".to_string(), "auto-created".to_string()],
                config.default_priority_new.clone(),
            ))
        }
        _ => None,
    }
}

/// Parse `git diff --name-status` output into (status_char, new_path, old_path)
/// tuples. Returns every entry, unfiltered — which entries actually trigger
/// lifecycle work is decided downstream: the spec-manifest gate for task
/// creation (spec-registry.md §15, `should_create_lifecycle_task`), the
/// repo's `SpecLifecycleConfig` watched prefixes as the manifest-less
/// fallback, and persona-file detection for the persona push hook.
fn parse_name_status(diff_output: &str) -> Vec<(char, String, Option<String>)> {
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

        changes.push((
            status_char,
            new_path.to_string(),
            old_path.map(str::to_string),
        ));
    }

    changes
}

/// Decide whether a changed spec file should create a lifecycle task
/// (spec-registry.md §15 — manifest-driven, not path-prefix driven).
///
/// - `Some(manifest)` (a manifest exists at the pushed HEAD): the manifest is
///   the single source of truth. A task is created only when the changed file
///   has a manifest entry whose effective `auto_create_tasks` (per-entry
///   override, else the `defaults:` section) is true. Files not registered in
///   the manifest create no lifecycle task.
/// - `None` (manifest-less repo): the repo's `SpecLifecycleConfig` watched /
///   ignored path prefixes decide (spec-lifecycle.md §Configuration — the
///   `[spec_lifecycle]` config block is superseded by the manifest only when
///   a manifest exists), so existing repos keep working. A rename (or copy)
///   triggers the fallback when **either** side of the rename sits under a
///   watched prefix, matching the pre-manifest behavior.
pub fn should_create_lifecycle_task(
    manifest: Option<&crate::spec_registry::SpecManifest>,
    config: &gyre_domain::SpecLifecycleConfig,
    status_char: char,
    path: &str,
    old_path: Option<&str>,
) -> bool {
    // Change kinds that don't classify into a task never create one.
    if classify_spec_change(status_char, path, old_path, config).is_none() {
        return false;
    }
    match manifest {
        // Manifest mode matches on the NEW path only (task-194 instruction:
        // "For renames (R), match on the new path").
        Some(m) => crate::spec_registry::manifest_entry_for_path(m, path)
            .map(|e| e.effective_auto_create_tasks(&m.defaults))
            .unwrap_or(false),
        None => config.is_watched(path) || old_path.is_some_and(|p| config.is_watched(p)),
    }
}

// ---------------------------------------------------------------------------
// Repo-defined personas: push detection + approval reset
// (platform-model.md §2 Repo-Defined Personas)
// ---------------------------------------------------------------------------

/// Directory holding repo-defined persona files (platform-model.md §2).
const PERSONA_FILES_DIR: &str = "specs/personas/";

/// True for repo-defined persona files: `*.md` directly under `specs/personas/`
/// (e.g. `specs/personas/security.md`). Nested paths are not persona files —
/// the spec's example set is flat.
fn is_persona_file(path: &str) -> bool {
    match path.strip_prefix(PERSONA_FILES_DIR) {
        Some(rest) => !rest.is_empty() && rest.ends_with(".md") && !rest.contains('/'),
        None => false,
    }
}

/// Derive the persona slug from a persona file path:
/// `specs/personas/security.md` → `security`.
fn persona_slug_from_path(path: &str) -> Option<String> {
    let rest = path.strip_prefix(PERSONA_FILES_DIR)?;
    let slug = rest.strip_suffix(".md")?;
    if slug.is_empty() || slug.contains('/') {
        return None;
    }
    Some(slug.to_string())
}

/// Parse `git diff --name-status` output, returning only repo-defined persona
/// file changes (entries where the old or the new side is a persona file).
pub fn parse_persona_changes(diff_output: &str) -> Vec<(char, String, Option<String>)> {
    parse_name_status(diff_output)
        .into_iter()
        .filter(|(_, new_path, old_path)| {
            is_persona_file(new_path) || old_path.as_deref().is_some_and(is_persona_file)
        })
        .collect()
}

/// Build the auto-created approval task for a persona change
/// (platform-model.md §2: "Creates a task: Persona change requires human
/// approval").
fn persona_approval_task(
    slug: &str,
    persona_path: &str,
    repo_id: &Id,
    workspace_id: &Id,
    change_kind: &str,
    now: u64,
) -> gyre_domain::Task {
    let title = format!("Persona change requires human approval: {slug}");
    let task_id = Id::new(uuid::Uuid::new_v4().to_string());
    let mut task = gyre_domain::Task::new(task_id, &title, now);
    task.priority = gyre_domain::TaskPriority::High;
    task.labels = vec!["persona-change".to_string(), "auto-created".to_string()];
    task.description = Some(format!(
        "Auto-created by the persona push hook (platform-model.md §2).\n\
         Persona file: {persona_path}\nRepo: {repo_id}\nChange: {change_kind}\n\
         The persona's approval was reset to Pending; a human must re-approve \
         before the persona is used again."
    ));
    task.repo_id = repo_id.clone();
    task.workspace_id = workspace_id.clone();
    task.spec_path = Some(persona_path.to_string());
    // task_type intentionally left as None: push-hook tasks are informational
    // and must NOT trigger agent spawning (agent-runtime.md §1 Phase 4).
    task
}

/// Read a file's content at a specific commit (`git show <sha>:<path>`).
/// Returns `None` on failure or when the content is not valid UTF-8
/// (persona prompts are markdown; mangled bytes would corrupt the prompt).
async fn read_file_at(git_bin: &str, repo_path: &str, sha: &str, path: &str) -> Option<String> {
    let out = Command::new(git_bin)
        .arg("-C")
        .arg(repo_path)
        .arg("show")
        .arg(format!("{sha}:{path}"))
        .output()
        .await
        .ok()?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        warn!(repo_path, path, "persona-push: git show failed: {err}");
        return None;
    }
    match String::from_utf8(out.stdout) {
        Ok(content) => Some(content),
        Err(_) => {
            warn!(
                repo_path,
                path, "persona-push: file content is not valid UTF-8; skipping"
            );
            None
        }
    }
}

/// After a successful push to the default branch, detect repo-defined persona
/// file changes and reset their approval (platform-model.md §2
/// Repo-Defined Personas). Runs in the same post-receive pipeline as
/// spec-lifecycle processing.
///
/// For each changed `specs/personas/<slug>.md` file:
/// 1. the persona entity (looked up by slug + `PersonaScope::Repo`) is updated
///    with the new file content and a recomputed `content_hash` — or a new
///    repo-scoped persona is created as `Pending` when none exists yet,
/// 2. approval is reset to `Pending` (`approved_by`/`approved_at` cleared,
///    `version` incremented, `updated_at` refreshed),
/// 3. a "Persona change requires human approval" task is auto-created
///    (deduplicated against open tasks with the same title),
/// 4. the persona owner is notified via the inbox.
///
/// Deleted persona files have no new content: the entity keeps its last
/// registered prompt, but its approval still resets — a removed persona file
/// must not keep running on a stale approval.
async fn process_persona_changes(
    state: &Arc<AppState>,
    repo_id: &str,
    repo_path: &str,
    default_branch: &str,
    ref_updates: &[RefUpdate],
) {
    let default_ref = format!("refs/heads/{default_branch}");

    let git_bin = std::env::var("GYRE_GIT_PATH").unwrap_or_else(|_| "git".to_string());
    // Well-known SHA for the empty git tree (used as base for initial pushes).
    const EMPTY_TREE: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";

    for update in ref_updates.iter().filter(|u| u.refname == default_ref) {
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
                warn!(repo_path, "persona-push: git diff failed: {err}");
                continue;
            }
            Err(e) => {
                warn!(repo_path, "persona-push: git diff spawn failed: {e}");
                continue;
            }
        };

        let changes = parse_persona_changes(&String::from_utf8_lossy(&out.stdout));
        if changes.is_empty() {
            continue;
        }

        // Scope resolution: the repo row carries the workspace (task scoping)
        // and its workspace carries the tenant (notification scoping). A
        // lookup failure means the scope cannot be determined — skip the
        // affected entity rather than fabricating a scope identity
        // (task-097 F3 class).
        let Some(repo) = state
            .repos
            .find_by_id(&Id::new(repo_id))
            .await
            .ok()
            .flatten()
        else {
            warn!(
                repo_id,
                "persona-push: repo not found; skipping persona processing"
            );
            return;
        };
        let tenant_id = state
            .workspaces
            .find_by_id(&repo.workspace_id)
            .await
            .ok()
            .flatten()
            .map(|ws| ws.tenant_id.to_string());

        let existing_tasks = state.tasks.list().await.unwrap_or_default();
        let now = crate::api::now_secs();
        let repo_scope = gyre_domain::PersonaScope::Repo(repo.id.clone());

        for (status_char, path, old_path) in changes {
            // A change can affect two persona paths: a rename carries content
            // for the new path and removes the old one.
            let mut affected: Vec<(&str, bool)> = Vec::new(); // (path, has_new_content)
            match status_char {
                'A' | 'M' => affected.push((path.as_str(), true)),
                'D' => affected.push((path.as_str(), false)),
                'R' => {
                    if let Some(old) = old_path.as_deref() {
                        affected.push((old, false));
                    }
                    affected.push((path.as_str(), true));
                }
                _ => {}
            }

            for (persona_path, has_content) in affected {
                let Some(slug) = persona_slug_from_path(persona_path) else {
                    continue;
                };

                let content = if has_content {
                    match read_file_at(&git_bin, repo_path, &update.new_sha, persona_path).await {
                        Some(c) => Some(c),
                        None => continue,
                    }
                } else {
                    None
                };

                let existing = state
                    .personas
                    .find_by_slug_and_scope(&slug, &repo_scope)
                    .await
                    .ok()
                    .flatten();

                // Step 1+2: update the entity (or register a new one) and
                // reset approval to Pending.
                let persona_after: Option<gyre_domain::Persona> = match existing {
                    Some(mut p) => {
                        if let Some(c) = content {
                            p.system_prompt = c;
                            p.refresh_content_hash();
                        }
                        p.version += 1;
                        p.approval_status = gyre_domain::PersonaApprovalStatus::Pending;
                        p.approved_by = None;
                        p.approved_at = None;
                        p.updated_at = now;
                        if let Err(e) = state.personas.update(&p).await {
                            warn!(slug, "persona-push: failed to update persona: {e}");
                            continue;
                        }
                        info!(
                            slug,
                            "persona-push: updated persona and reset approval to Pending"
                        );
                        Some(p)
                    }
                    None => match content {
                        Some(c) => {
                            let p = gyre_domain::Persona::new(
                                Id::new(uuid::Uuid::new_v4().to_string()),
                                slug.clone(),
                                slug.clone(),
                                repo_scope.clone(),
                                c,
                                now,
                            );
                            if let Err(e) = state.personas.create(&p).await {
                                warn!(slug, "persona-push: failed to create persona: {e}");
                                continue;
                            }
                            info!(
                                slug,
                                "persona-push: registered new repo-defined persona (Pending)"
                            );
                            Some(p)
                        }
                        None => {
                            // Deleted persona file with no registered entity —
                            // nothing to reset, nothing to approve.
                            info!(
                                slug,
                                "persona-push: deleted persona file has no registered entity; skipping"
                            );
                            continue;
                        }
                    },
                };

                let change_kind = match (status_char, has_content) {
                    ('A', true) => "added",
                    ('M', true) => "modified",
                    ('R', true) => "renamed",
                    ('D', false) => "deleted",
                    ('R', false) => "renamed (old path removed)",
                    _ => "changed",
                };

                // Step 3: auto-create the human-approval task.
                let title = format!("Persona change requires human approval: {slug}");
                let task_exists = existing_tasks.iter().any(|t| {
                    t.title == title && !matches!(t.status, gyre_domain::TaskStatus::Done)
                });
                if task_exists {
                    info!(
                        title,
                        "persona-push: approval task already exists, skipping"
                    );
                } else {
                    let task = persona_approval_task(
                        &slug,
                        persona_path,
                        &repo.id,
                        &repo.workspace_id,
                        change_kind,
                        now,
                    );
                    match state.tasks.create(&task).await {
                        Err(e) => warn!(title, "persona-push: failed to create task: {e}"),
                        Ok(()) => {
                            info!(
                                title,
                                "persona-push: created approval task for persona change"
                            );
                            state
                                .emit_event(
                                    Some(repo.workspace_id.clone()),
                                    gyre_common::message::Destination::Workspace(
                                        repo.workspace_id.clone(),
                                    ),
                                    gyre_common::message::MessageKind::TaskCreated,
                                    Some(serde_json::json!({"task_id": task.id.to_string()})),
                                )
                                .await;
                        }
                    }
                }

                // Step 4: notify the persona owner. A persona discovered from
                // git has no owner yet, and an owner that does not resolve to
                // a known user (e.g. an agent id) has no inbox to deliver to —
                // skip rather than write an undeliverable notification.
                let notify_owner = persona_after
                    .as_ref()
                    .and_then(|p| p.owner.clone())
                    .filter(|o| !o.is_empty());
                if let Some(owner) = notify_owner {
                    let owner_id = Id::new(owner.clone());
                    let user_exists = state
                        .users
                        .find_by_id(&owner_id)
                        .await
                        .ok()
                        .flatten()
                        .is_some();
                    match (user_exists, tenant_id.as_deref()) {
                        (false, _) => {
                            info!(
                                slug,
                                owner,
                                "persona-push: owner is not a known user; skipping notification"
                            );
                        }
                        (true, None) => {
                            warn!(
                                slug,
                                "persona-push: workspace tenant could not be resolved; skipping owner notification"
                            );
                        }
                        (true, Some(tenant)) => {
                            let mut notif = gyre_common::Notification::new(
                                Id::new(uuid::Uuid::new_v4().to_string()),
                                repo.workspace_id.clone(),
                                owner_id,
                                gyre_common::NotificationType::PersonaChangePendingApproval,
                                title,
                                tenant,
                                now as i64,
                            );
                            notif.body = Some(format!(
                                "Persona \"{slug}\" was {change_kind} in {persona_path} \
                                 (repo {}). Its approval was reset to Pending and requires \
                                 human review.",
                                repo.name
                            ));
                            notif.source.entity_type = "persona".to_string();
                            notif.source.entity_id = slug.clone();
                            notif.source.repo_id = Some(repo.id.clone());
                            if let Err(e) = state.notifications.create(&notif).await {
                                warn!(slug, "persona-push: failed to notify persona owner: {e}");
                            }
                        }
                    }
                }
            }
        }
    }
}

/// After a successful push to the default branch, detect spec changes and create tasks.
async fn process_spec_lifecycle(
    state: &Arc<AppState>,
    repo_id: &str,
    repo_path: &str,
    default_branch: &str,
    ref_updates: &[RefUpdate],
) {
    // Per-repo config (spec-lifecycle.md §Configuration); defaults when unset.
    let config = state
        .spec_lifecycle_configs
        .get_for_repo(repo_id)
        .await
        .unwrap_or_default();
    if !config.enabled {
        info!(repo_id, "spec-lifecycle: disabled for repo, skipping");
        return;
    }

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
        // Parse the diff unfiltered: task-creation eligibility is decided by
        // the manifest gate (`should_create_lifecycle_task`), or by the
        // config watched-prefix fallback in manifest-less repos — not here.
        let changes = parse_name_status(&diff_text);
        if changes.is_empty() {
            continue;
        }

        // Manifest-driven lifecycle policy (spec-registry.md §15): the
        // manifest at the pushed HEAD is the single source of truth for
        // which spec files trigger lifecycle tasks and their priority.
        // When the repo has no manifest, `should_create_lifecycle_task`
        // falls back to the config's watched-prefix rules.
        let manifest = crate::spec_registry::read_manifest(repo_path, &update.new_sha).await;
        if manifest.is_none() {
            debug!(
                repo_path,
                "spec-lifecycle: no specs/manifest.yaml at HEAD — using path-prefix fallback"
            );
        }

        // Resolve the repo's workspace scope once per push. Events are scoped
        // to the workspace the repo actually belongs to; if the repo cannot be
        // resolved there is no valid scope to emit under, so skip the events
        // and log rather than fabricating one (task-097 F3 class). Approval
        // invalidation is independent of the repo row: it is keyed by spec
        // path against the on-disk repo, so it still runs below.
        let ws_id = match state.repos.find_by_id(&gyre_common::Id::new(repo_id)).await {
            Ok(Some(repo)) => Some(repo.workspace_id),
            Ok(None) => {
                warn!(
                    repo_id,
                    "spec-lifecycle: repo not found, skipping workspace events"
                );
                None
            }
            Err(e) => {
                warn!(
                    repo_id,
                    "spec-lifecycle: repo lookup failed: {e}, skipping workspace events"
                );
                None
            }
        };
        let existing_tasks = state.tasks.list().await.unwrap_or_default();
        let now = crate::api::now_secs();

        for (status_char, path, old_path) in changes {
            // Auto-invalidate active spec approvals when the spec file is modified,
            // deleted, or renamed (configurable, spec-lifecycle.md §Configuration).
            if config.auto_invalidate_approvals {
                // For renames, the old path is stale; for M/D, the current path is stale.
                // Diff paths are git paths ("specs/system/foo.md"); approval rows are
                // keyed by ledger paths ("system/foo.md" — spec_registry strips the
                // specs/ prefix). Normalize so revocation actually matches rows.
                fn ledger_path(p: &str) -> &str {
                    p.strip_prefix("specs/").unwrap_or(p)
                }
                let stale_paths: Vec<&str> = match status_char {
                    'M' | 'D' => vec![path.as_str()],
                    'R' => old_path
                        .as_deref()
                        .map(|p| vec![p])
                        .unwrap_or_else(|| vec![path.as_str()]),
                    _ => vec![],
                };
                // Only `specs/…` files carry approval-ledger rows; the
                // unfiltered diff also lists source and doc files, which
                // have no approvals to invalidate.
                let stale_paths: Vec<&str> = stale_paths
                    .into_iter()
                    .filter(|p| p.starts_with("specs/"))
                    .collect();

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
                    for stale_path in &stale_paths {
                        let _ = state
                            .spec_approvals
                            .revoke_all_for_path(
                                ledger_path(stale_path),
                                "system:spec-lifecycle",
                                &reason,
                                now,
                            )
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

            if !should_create_lifecycle_task(
                manifest.as_ref(),
                &config,
                status_char,
                &path,
                old_path.as_deref(),
            ) {
                debug!(
                    spec_path = %path,
                    "spec-lifecycle: change excluded from task creation by manifest policy"
                );
                continue;
            }

            let Some((title, labels, priority)) =
                classify_spec_change(status_char, &path, old_path.as_deref(), &config)
            else {
                continue;
            };
            let change_kind = match status_char {
                'A' => "added",
                'M' => "modified",
                'D' => "deleted",
                'R' => "renamed",
                _ => "unknown",
            };

            // Per-spec priority override (spec-registry.md §15): apply the
            // manifest-declared priority when present, else keep the
            // change-kind default from `classify_spec_change` (which itself
            // honors the repo's SpecLifecycleConfig default priorities).
            let priority = manifest
                .as_ref()
                .and_then(|m| {
                    crate::spec_registry::manifest_entry_for_path(m, &path)
                        .and_then(|e| e.effective_priority(&m.defaults))
                })
                .unwrap_or(priority);

            // Dedup: skip if a non-Done task with the same title already exists
            // (configurable, spec-lifecycle.md §Configuration).
            if config.dedup_open_tasks {
                let exists = existing_tasks.iter().any(|t| {
                    t.title == title && !matches!(t.status, gyre_domain::TaskStatus::Done)
                });
                if exists {
                    info!(title, "spec-lifecycle: task already exists, skipping");
                    // task-104: SpecChanged audit event — the spec file changed on
                    // the default branch regardless of task dedup; no new task id.
                    // Scope comes from the per-push ws_id resolution above
                    // (None when unresolvable — never a fabricated identity).
                    emit_spec_changed_audit(
                        state,
                        repo_id,
                        ws_id.as_ref(),
                        &path,
                        change_kind,
                        None,
                    )
                    .await;
                    continue;
                }
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

            // Workspace scope for the SpecChanged audit event comes from the
            // per-push ws_id resolution above: a lookup failure means the
            // scope cannot be determined, so the audit event is recorded
            // without a workspace scope rather than fabricating a "default"
            // identity (task-097 F3 class).
            match state.tasks.create(&task).await {
                Err(e) => {
                    warn!(title, "spec-lifecycle: failed to create task: {e}");
                    emit_spec_changed_audit(
                        state,
                        repo_id,
                        ws_id.as_ref(),
                        &path,
                        change_kind,
                        None,
                    )
                    .await;
                }
                Ok(()) => {
                    info!(title, "spec-lifecycle: created task for spec change");
                    // task-104: SpecChanged audit event (observability.md
                    // §Audit Event Types) with the auto-created task id.
                    emit_spec_changed_audit(
                        state,
                        repo_id,
                        ws_id.as_ref(),
                        &path,
                        change_kind,
                        Some(task_id.as_str()),
                    )
                    .await;
                    // ws_id resolved once per push above; None when the repo
                    // row is unresolvable — skip workspace-scoped events
                    // rather than emitting under a fabricated scope.
                    if let Some(ws_id) = ws_id.as_ref() {
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
                                gyre_common::message::Destination::Workspace(ws_id.clone()),
                                gyre_common::message::MessageKind::TaskCreated,
                                Some(serde_json::json!({"task_id": task_id.to_string()})),
                            )
                            .await;
                    }

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
            notif.source.entity_type = "spec".to_string();
            notif.source.entity_id = changed_spec_path.to_string();

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

    // Resolve the source repo's workspace_id for domain event emission (TASK-048 F5).
    let source_workspace_id = state
        .repos
        .find_by_id(&source_id)
        .await
        .ok()
        .flatten()
        .map(|r| r.workspace_id);

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

                // Emit domain event for new dependency detection (TASK-048 F5).
                // The spec §4 says "New dependency detected → create edge, log activity event."
                // emit_event() broadcasts via the unified message bus to WebSocket consumers
                // (orchestrators, dashboard UI). tracing::info! alone is infrastructure logging.
                if let Some(ws_id) = &source_workspace_id {
                    let payload = serde_json::json!({
                        "event": "dependency_detected",
                        "source_repo_id": repo_id,
                        "target_repo_id": de.target_repo_id,
                        "source_artifact": de.source_artifact,
                        "target_artifact": de.target_artifact,
                        "detection_method": serde_json::to_value(&de.method).unwrap_or_default(),
                        "dependency_type": serde_json::to_value(&de.dep_type).unwrap_or_default(),
                    });

                    state
                        .emit_event(
                            Some(ws_id.clone()),
                            gyre_common::message::Destination::Workspace(ws_id.clone()),
                            gyre_common::MessageKind::Custom("dependency_detected".to_string()),
                            Some(payload),
                        )
                        .await;
                }
            }
        }
    }
}

/// List all files in a repo at a given SHA that end with the specified extension.
///
/// Uses `git ls-tree -r --name-only <sha>` to enumerate all files, then filters
/// by the given extension suffix (e.g., ".proto"). Used by multi-file detection
/// methods that must scan ALL files of a type — not just changed ones — to produce
/// a complete edge set for reconciliation (see TASK-048 F4).
async fn list_files_by_extension(
    git_bin: &str,
    repo_path: &str,
    sha: &str,
    extension: &str,
) -> Vec<String> {
    let out = Command::new(git_bin)
        .arg("-C")
        .arg(repo_path)
        .arg("ls-tree")
        .arg("-r")
        .arg("--name-only")
        .arg(sha)
        .output()
        .await;

    let out = match out {
        Ok(o) if o.status.success() => o,
        _ => return Vec::new(),
    };

    let text = String::from_utf8_lossy(&out.stdout);
    text.lines()
        .filter(|line| line.ends_with(extension))
        .map(|line| line.to_string())
        .collect()
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
    // Proto is a multi-file detection method (many *.proto files per repo), so when
    // triggered we must scan ALL .proto files — not just changed ones. Scanning only
    // changed files while inserting ProtoImport into ran_methods would cause
    // reconciliation to falsely orphan edges from unchanged .proto files (TASK-048 F4).
    if has_proto {
        ran_methods.insert(DetectionMethod::ProtoImport);
        // Enumerate all .proto files in the repo at the pushed commit.
        let all_proto_files = list_files_by_extension(&git_bin, repo_path, new_sha, ".proto").await;
        for proto_file in &all_proto_files {
            if let Some(content) =
                crate::spec_registry::read_git_file(&git_bin, repo_path, new_sha, proto_file).await
            {
                let refs = detect_proto_imports(&content, &repo_names);
                for ref_name in &refs {
                    if let Some(target_repo) = all_repos.iter().find(|r| r.name == *ref_name) {
                        if target_repo.id.as_str() != repo_id {
                            detected_edges.push(DetectedEdge {
                                target_repo_id: target_repo.id.to_string(),
                                source_artifact: proto_file.to_string(),
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

    // domain-event:ok — summary log; per-edge domain events emitted by reconcile_dependencies
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
    // non-atomic-create:ok - per-dependent push side effects: the
    // BreakingChange record is the durable artifact (created first, failure
    // skips the dependent); the policy-gated auto-task is a best-effort
    // advisory record whose failure is warn-and-continue so one dependent's
    // task-creation failure does not drop the remaining dependents'
    // records and notifications.
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
                    // user-management.md §Notification Entity: the breaking
                    // change taxonomy type, not the generic
                    // CrossWorkspaceSpecChange stand-in.
                    gyre_common::NotificationType::BreakingChangeDetected,
                    format!("Breaking change in {repo_name}: {description}"),
                    tenant_id,
                    Some(body_json),
                    Some(dep_edge.source_repo_id.to_string()),
                    Some(repo_id.to_string()),
                    "repo",
                    None,
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

/// Context binding verification for the root `SignedInput` (§2.4 Replay Prevention).
///
/// Compares the signed `InputContent` against the actual target of the current
/// push/merge. A `SignedInput` that authorizes work on repo-A must not be
/// replayable to authorize work on repo-B; a modified spec (different SHA)
/// must require a new approval; an `expected_generation` pin is only valid for
/// that specific deployment generation of the task.
///
/// `task_spec_sha` — the task's currently approved spec SHA (`None` when it
/// cannot be resolved; the spec_sha binding is then skipped rather than
/// failing on absent data). `task_generation` — the task's CURRENT persisted
/// deployment generation. Both are looked up from real state by the caller;
/// this function performs no lookups itself.
///
/// `valid_until` is NOT re-checked here — it is enforced by
/// `verify_attestation_audit_only` (signed_input.valid_until node) and must
/// not be duplicated.
///
/// Fail closed: every present binding must match exactly; a mismatch yields
/// `valid: false` with a `context_binding.*` child naming the failing binding.
pub(crate) fn verify_context_binding(
    root: &gyre_common::SignedInput,
    target_repo_id: &str,
    target_workspace_id: &str,
    task_spec_sha: Option<&str>,
    task_generation: Option<u32>,
) -> gyre_common::VerificationResult {
    let mut children = Vec::new();
    let mut all_valid = true;

    // Binding 1: repo_id — the input cannot be replayed to a different repo.
    let repo_ok = root.content.repo_id == target_repo_id;
    children.push(gyre_common::VerificationResult {
        label: "context_binding.repo_id".to_string(),
        valid: repo_ok,
        message: if repo_ok {
            format!("signed repo_id {} matches target", root.content.repo_id)
        } else {
            format!(
                "signed repo_id {} does not match target repo {} — authorization is \
                 bound to a different repository",
                root.content.repo_id, target_repo_id
            )
        },
        children: vec![],
    });
    if !repo_ok {
        all_valid = false;
    }

    // Binding 2: workspace_id — the input cannot be replayed across workspaces.
    let ws_ok = root.content.workspace_id == target_workspace_id;
    children.push(gyre_common::VerificationResult {
        label: "context_binding.workspace_id".to_string(),
        valid: ws_ok,
        message: if ws_ok {
            format!(
                "signed workspace_id {} matches target",
                root.content.workspace_id
            )
        } else {
            format!(
                "signed workspace_id {} does not match target workspace {} — \
                 authorization is bound to a different workspace",
                root.content.workspace_id, target_workspace_id
            )
        },
        children: vec![],
    });
    if !ws_ok {
        all_valid = false;
    }

    // Binding 3: spec_sha — a modified spec requires a new approval and new
    // SignedInput. Only compared when the task's current approved SHA is
    // resolvable; an empty signed SHA cannot match any real spec and fails.
    if let Some(current_sha) = task_spec_sha {
        let spec_ok = !root.content.spec_sha.is_empty() && root.content.spec_sha == current_sha;
        children.push(gyre_common::VerificationResult {
            label: "context_binding.spec_sha".to_string(),
            valid: spec_ok,
            message: if spec_ok {
                format!(
                    "signed spec_sha {} matches current approved spec",
                    root.content.spec_sha.chars().take(8).collect::<String>()
                )
            } else {
                format!(
                    "signed spec_sha {} does not match current approved spec {} — \
                     the spec changed after this authorization was signed",
                    root.content.spec_sha, current_sha
                )
            },
            children: vec![],
        });
        if !spec_ok {
            all_valid = false;
        }
    }

    // Binding 4: expected_generation — when present, the input is only valid
    // for that specific deployment generation of the task (prevents replay of
    // an old authorization after reassignment). Absent → no pin, no check.
    if let Some(expected) = root.expected_generation {
        let gen_ok = task_generation == Some(expected);
        children.push(gyre_common::VerificationResult {
            label: "context_binding.expected_generation".to_string(),
            valid: gen_ok,
            message: if gen_ok {
                format!("signed expected_generation {expected} matches task generation")
            } else {
                format!(
                    "signed expected_generation {expected} does not match current task \
                     generation {} — the task was reassigned after this authorization \
                     was signed",
                    task_generation
                        .map(|g| g.to_string())
                        .unwrap_or_else(|| "unavailable".to_string())
                )
            },
            children: vec![],
        });
        if !gen_ok {
            all_valid = false;
        }
    }

    gyre_common::VerificationResult {
        label: "context_binding".to_string(),
        valid: all_valid,
        message: if all_valid {
            "signed input context matches the verification target".to_string()
        } else {
            "context binding failed — signed input targets a different context".to_string()
        },
        children,
    }
}

/// Constraints accumulated from a verified chain (§4.3, §6.2 Phase 2).
/// Named carrier (TASK-048 F1): the previous positional tuple was a
/// field-swap hazard.
pub(crate) struct ChainConstraints {
    /// The root SignedInput of the chain, if it carries one.
    pub root_input: Option<gyre_common::attestation::SignedInput>,
    /// Explicit constraints from the root SignedInput plus additional
    /// constraints from each DerivedInput (additive only).
    pub explicit_constraints: Vec<gyre_common::attestation::OutputConstraint>,
    /// Gate constraints from gate attestations on each node.
    pub gate_constraints: Vec<gyre_common::attestation::GateConstraint>,
}

/// Accumulate all constraints from a verified chain (§4.3, §6.2 Phase 2).
///
/// Walks the chain root→leaf, collecting:
/// 1. Explicit constraints from the root SignedInput
/// 2. Additional constraints from each DerivedInput (additive only)
/// 3. Gate constraints from gate attestations on each node
///
/// Returns the full constraint set for evaluation.
pub(crate) fn accumulate_chain_constraints(chain: &[gyre_common::Attestation]) -> ChainConstraints {
    let mut explicit_constraints = Vec::new();
    let mut gate_constraints = Vec::new();
    let mut root_input = None;

    for att in chain {
        match &att.input {
            gyre_common::AttestationInput::Signed(si) => {
                // Root-first chain ordering (see `verify_chain`): the first
                // Signed input is the chain root.
                if root_input.is_none() {
                    root_input = Some(si.clone());
                }
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

    ChainConstraints {
        root_input,
        explicit_constraints,
        gate_constraints,
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

    #[tokio::test]
    async fn system_token_resolves_workspace_in_foreign_tenant() {
        // Dev-mode bootstrap shape (task-099): `gyre bootstrap --dev`
        // creates tenant "dev" and a workspace under it, but the global
        // system token's tenant_id is "default". The git smart-HTTP push
        // the bootstrap flow performs must still resolve the workspace —
        // via the system-principal slug-only fallback — or the dev-mode
        // spec-registry step 404s forever while reporting success-ish
        // output. Non-system principals keep the strict tenant filter.
        let state = test_state();
        let tmp = TempDir::new().unwrap();
        let repo_path = tmp.path().join("dev-demo.git");
        std::fs::create_dir_all(&repo_path).unwrap();
        let ok = std::process::Command::new("git")
            .args(["init", "--bare", "-b", "main"])
            .arg(&repo_path)
            .status()
            .unwrap()
            .success();
        assert!(ok, "git init --bare failed");

        // Workspace under a NON-"default" tenant — the dev-mode topology.
        let ws = Workspace {
            id: Id::new("ws-dev"),
            tenant_id: Id::new("t-dev"),
            name: "Default".to_string(),
            slug: "default".to_string(),
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
            Id::new("repo-dev"),
            Id::new("ws-dev"),
            "gyre-demo",
            repo_path.to_str().unwrap(),
            0,
        );
        state.repos.create(&repo).await.unwrap();

        let app = Router::new()
            .route(
                "/git/:workspace_slug/:repo_name/info/refs",
                get(git_info_refs),
            )
            .with_state(state.clone());

        // System token (tenant "default") resolves the foreign-tenant
        // workspace via the fallback.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/git/default/gyre-demo/info/refs?service=git-upload-pack")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "system token must resolve the dev-mode (foreign-tenant) workspace"
        );

        // A user principal from an unrelated tenant is still contained.
        // (agent token path: Agent role, tenant "default".)
        let mut user = gyre_domain::User::new(Id::new("u-x"), "ext-x", "someone", 0);
        user.tenant_id = Some(Id::new("t-other"));
        state.users.create(&user).await.unwrap();
        let raw_key = "gyre_test_git_foreign_key";
        state
            .api_keys
            .create(&crate::auth::hash_api_key(raw_key), &user.id, "test")
            .await
            .unwrap();
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/git/default/gyre-demo/info/refs?service=git-upload-pack")
                    .header("Authorization", format!("Bearer {raw_key}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::NOT_FOUND,
            "non-system principals must keep the strict tenant filter"
        );
    }

    #[tokio::test]
    async fn repo_access_enforced_on_git_transport_for_user_principals() {
        // user-management.md §Repo Access on the git smart-HTTP transport —
        // the enforcement point for clone/fetch/push. The rest of this suite
        // authenticates with the system token, which authorize_repo_access
        // admits unconditionally; these assertions use real user principals
        // (API keys + workspace memberships) so the membership/override
        // check actually gates the transport and regresses loudly if the
        // Read/Write permission mapping or the restrict-only override
        // logic breaks.
        let (app, state, _tmp, ws_slug, repo_name, _repo_path) = git_app_with_repo().await;

        async fn member_user(
            state: &Arc<crate::AppState>,
            ws_id: &str,
            role: gyre_domain::WorkspaceRole,
            key: &str,
        ) -> Id {
            let id = Id::new(format!("user-{key}"));
            let mut user =
                gyre_domain::User::new(id.clone(), format!("ext-{key}"), format!("u-{key}"), 0);
            user.tenant_id = Some(Id::new("default"));
            user.global_role = gyre_domain::GlobalRole::Member;
            user.roles = vec![gyre_domain::UserRole::Developer];
            state.users.create(&user).await.unwrap();
            let mut m = gyre_domain::WorkspaceMembership::new(
                Id::new(format!("m-{key}")),
                id.clone(),
                Id::new(ws_id),
                role,
                Id::new("system"),
                0,
            );
            m.accept(0);
            state.workspace_memberships.create(&m).await.unwrap();
            state
                .api_keys
                .create(&crate::auth::hash_api_key(key), &id, "test")
                .await
                .unwrap();
            id
        }

        let dev_id = member_user(
            &state,
            "ws-test",
            gyre_domain::WorkspaceRole::Developer,
            "gyre-test-dev-key",
        )
        .await;
        let _viewer_id = member_user(
            &state,
            "ws-test",
            gyre_domain::WorkspaceRole::Viewer,
            "gyre-test-viewer-key",
        )
        .await;
        // A user of the same tenant with NO workspace membership.
        let mut outsider =
            gyre_domain::User::new(Id::new("user-outsider"), "ext-out", "outsider", 0);
        outsider.tenant_id = Some(Id::new("default"));
        outsider.global_role = gyre_domain::GlobalRole::Member;
        outsider.roles = vec![gyre_domain::UserRole::Developer];
        state.users.create(&outsider).await.unwrap();
        state
            .api_keys
            .create(
                &crate::auth::hash_api_key("gyre-test-outsider-key"),
                &outsider.id,
                "test",
            )
            .await
            .unwrap();

        let info_refs =
            |service: &str| format!("/git/{ws_slug}/{repo_name}/info/refs?service={service}");

        // Effective role → transport permission per the §Repo Access
        // table: Developer reads and writes, Viewer reads only, non-members
        // nothing.
        for (key, service, expected) in [
            ("gyre-test-dev-key", "git-upload-pack", StatusCode::OK),
            ("gyre-test-dev-key", "git-receive-pack", StatusCode::OK),
            ("gyre-test-viewer-key", "git-upload-pack", StatusCode::OK),
            (
                "gyre-test-viewer-key",
                "git-receive-pack",
                StatusCode::FORBIDDEN,
            ),
            (
                "gyre-test-outsider-key",
                "git-upload-pack",
                StatusCode::FORBIDDEN,
            ),
        ] {
            let resp = app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri(info_refs(service))
                        .header("Authorization", format!("Bearer {key}"))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), expected, "{key} on {service}");
        }

        // A per-repo override restricting the Developer to Viewer on this
        // repo: fetch (Read) stays allowed, push (Write) is denied — the
        // override restricts, never expands.
        let over = gyre_domain::RepoAccessOverride::new(
            Id::new("override-git-1"),
            Id::new("repo-1"),
            dev_id.clone(),
            gyre_domain::WorkspaceRole::Viewer,
            Id::new("system"),
            0,
        );
        state.repo_access_overrides.create(&over).await.unwrap();

        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(info_refs("git-upload-pack"))
                    .header("Authorization", "Bearer gyre-test-dev-key")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "a Developer restricted to Viewer keeps read (fetch) access"
        );

        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(info_refs("git-receive-pack"))
                    .header("Authorization", "Bearer gyre-test-dev-key")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::FORBIDDEN,
            "a Developer restricted to Viewer loses write (push) access"
        );
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
    async fn info_refs_receive_pack_archived_repo_returns_err_pkt_line() {
        let (app, state, _tmp, ws_slug, repo_name, _path) = git_app_with_repo().await;

        // Archive the repo (repo-lifecycle.md §4 step 5).
        let mut repo = state
            .repos
            .find_by_id(&Id::new("repo-1"))
            .await
            .unwrap()
            .expect("repo exists");
        repo.archive();
        state.repos.update(&repo).await.unwrap();

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
        // The advertisement is a valid 200 response whose pkt-line stream
        // carries an ERR line — the git-smart-HTTP convention that makes
        // `git push` print "fatal: remote error: push rejected: repository is
        // archived" instead of a bare HTTP 403 whose body git never shows.
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers().get("content-type").unwrap(),
            "application/x-git-receive-pack-advertisement"
        );
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let text = String::from_utf8_lossy(&body);
        // Exact pkt-line framing: the service header pkt-line (001f = 4 + 27
        // bytes of "# service=git-receive-pack\n"), a flush-pkt, then the
        // ERR pkt-line. Hardcoded so a framing regression in pkt_line()/
        // service_header() fails here rather than being mirrored in.
        assert!(
            text.starts_with("001f# service=git-receive-pack\n0000"),
            "advertisement must start with the service header pkt-line + flush-pkt, got: {text}"
        );
        assert!(
            text.contains("ERR push rejected: repository is archived"),
            "archived repo advertisement must carry the ERR pkt-line, got: {text}"
        );
    }

    #[tokio::test]
    async fn info_refs_upload_pack_archived_repo_still_serves_clone_advertisement() {
        // Archived repos are read-only, not invisible: clone/fetch
        // (git-upload-pack) must keep working (repo-lifecycle.md §4 step 5).
        let (app, state, _tmp, ws_slug, repo_name, _path) = git_app_with_repo().await;

        let mut repo = state
            .repos
            .find_by_id(&Id::new("repo-1"))
            .await
            .unwrap()
            .expect("repo exists");
        repo.archive();
        state.repos.update(&repo).await.unwrap();

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
        let text = String::from_utf8_lossy(&body);
        assert!(
            !text.contains("ERR "),
            "upload-pack advertisement must not be rejected for archived repo, got: {text}"
        );
    }

    #[tokio::test]
    async fn receive_pack_archived_repo_returns_403() {
        let (app, state, _tmp, ws_slug, repo_name, _path) = git_app_with_repo().await;

        // Archive the repo (repo-lifecycle.md §4 step 5).
        let mut repo = state
            .repos
            .find_by_id(&Id::new("repo-1"))
            .await
            .unwrap()
            .expect("repo exists");
        repo.archive();
        state.repos.update(&repo).await.unwrap();

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/git/{ws_slug}/{repo_name}/git-receive-pack"))
                    .header("Authorization", auth_header())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let text = String::from_utf8_lossy(&body);
        assert!(
            text.contains("archived"),
            "rejection must explain the repo is archived, got: {text}"
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
    fn parse_name_status_empty_input() {
        let changes = super::parse_name_status("");
        assert!(changes.is_empty());
    }

    #[test]
    fn parse_name_status_added_spec() {
        let input = "A\tspecs/system/new-spec.md\n";
        let changes = super::parse_name_status(input);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].0, 'A');
        assert_eq!(changes[0].1, "specs/system/new-spec.md");
        assert!(changes[0].2.is_none());
    }

    #[test]
    fn parse_name_status_modified_spec() {
        let input = "M\tspecs/development/architecture.md\n";
        let changes = super::parse_name_status(input);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].0, 'M');
        assert_eq!(changes[0].1, "specs/development/architecture.md");
    }

    #[test]
    fn parse_name_status_deleted_spec() {
        let input = "D\tspecs/system/old-spec.md\n";
        let changes = super::parse_name_status(input);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].0, 'D');
        assert_eq!(changes[0].1, "specs/system/old-spec.md");
    }

    #[test]
    fn parse_name_status_renamed_spec() {
        let input = "R090\tspecs/system/old.md\tspecs/system/new.md\n";
        let changes = super::parse_name_status(input);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].0, 'R');
        assert_eq!(changes[0].1, "specs/system/new.md");
        assert_eq!(changes[0].2.as_deref(), Some("specs/system/old.md"));
    }

    #[test]
    fn parse_name_status_returns_all_paths_unfiltered() {
        // Parsing is path-agnostic: which entries create lifecycle tasks is
        // decided by `should_create_lifecycle_task` (the manifest gate for
        // manifest repos, the config watched prefixes otherwise) — specs/
        // files outside the watched prefixes must reach that gate
        // (spec-registry.md §15), and non-specs files are excluded there.
        let input =
            "M\tspecs/milestones/m1.md\nM\tsrc/main.rs\nM\tspecs/coverage/system/spec-registry.md\n";
        let changes = super::parse_name_status(input);
        assert_eq!(changes.len(), 3);
        assert_eq!(changes[2].1, "specs/coverage/system/spec-registry.md");
    }

    #[test]
    fn parse_name_status_development_path() {
        let input = "A\tspecs/development/database-migrations.md\n";
        let changes = super::parse_name_status(input);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].1, "specs/development/database-migrations.md");
    }

    #[test]
    fn classify_spec_change_added() {
        let (title, labels, priority) =
            super::classify_spec_change('A', "specs/system/foo.md", None, &Default::default())
                .unwrap();
        assert_eq!(title, "Implement spec: specs/system/foo.md");
        assert!(labels.contains(&"spec-implementation".to_string()));
        assert!(labels.contains(&"auto-created".to_string()));
        assert_eq!(priority, gyre_domain::TaskPriority::Medium);
    }

    #[test]
    fn classify_spec_change_modified() {
        let (title, labels, priority) =
            super::classify_spec_change('M', "specs/system/foo.md", None, &Default::default())
                .unwrap();
        assert_eq!(title, "Review spec change: specs/system/foo.md");
        assert!(labels.contains(&"spec-drift-review".to_string()));
        assert_eq!(priority, gyre_domain::TaskPriority::High);
    }

    #[test]
    fn classify_spec_change_deleted() {
        let (title, labels, priority) =
            super::classify_spec_change('D', "specs/system/old.md", None, &Default::default())
                .unwrap();
        assert_eq!(title, "Handle spec removal: specs/system/old.md");
        assert!(labels.contains(&"spec-deprecated".to_string()));
        assert_eq!(priority, gyre_domain::TaskPriority::High);
    }

    #[test]
    fn classify_spec_change_renamed() {
        let (title, labels, priority) = super::classify_spec_change(
            'R',
            "specs/system/new.md",
            Some("specs/system/old.md"),
            &Default::default(),
        )
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
        assert!(
            super::classify_spec_change('X', "specs/system/foo.md", None, &Default::default())
                .is_none()
        );
    }

    #[test]
    fn classify_spec_change_respects_configured_priorities() {
        // spec-lifecycle.md §Configuration: default_priority_* must override
        // the built-in Medium/High/High for task creation.
        let config = gyre_domain::SpecLifecycleConfig {
            default_priority_new: gyre_domain::TaskPriority::Critical,
            default_priority_modified: gyre_domain::TaskPriority::Low,
            default_priority_deleted: gyre_domain::TaskPriority::Medium,
            ..Default::default()
        };
        let (_, _, p_new) =
            super::classify_spec_change('A', "specs/system/foo.md", None, &config).unwrap();
        let (_, _, p_mod) =
            super::classify_spec_change('M', "specs/system/foo.md", None, &config).unwrap();
        let (_, _, p_del) =
            super::classify_spec_change('D', "specs/system/foo.md", None, &config).unwrap();
        let (_, _, p_ren) = super::classify_spec_change(
            'R',
            "specs/system/new.md",
            Some("specs/system/old.md"),
            &config,
        )
        .unwrap();
        assert_eq!(p_new, gyre_domain::TaskPriority::Critical);
        assert_eq!(p_mod, gyre_domain::TaskPriority::Low);
        assert_eq!(p_del, gyre_domain::TaskPriority::Medium);
        assert_eq!(p_ren, gyre_domain::TaskPriority::Critical);
    }

    #[test]
    fn should_create_lifecycle_task_no_manifest_custom_watched_paths() {
        // Custom watched paths replace the default prefixes entirely
        // (spec-lifecycle.md §Configuration) — including paths outside
        // specs/, which the unfiltered diff parser still delivers to the gate.
        let config = gyre_domain::SpecLifecycleConfig {
            watched_paths: vec!["docs/specs/".to_string()],
            ..Default::default()
        };
        assert!(super::should_create_lifecycle_task(
            None,
            &config,
            'A',
            "docs/specs/new.md",
            None
        ));
        assert!(!super::should_create_lifecycle_task(
            None,
            &config,
            'M',
            "specs/system/foo.md",
            None
        ));
    }

    #[test]
    fn should_create_lifecycle_task_no_manifest_ignored_overrides_watched() {
        // Ignored prefixes take precedence over watched ones.
        let config = gyre_domain::SpecLifecycleConfig {
            watched_paths: vec!["specs/".to_string()],
            ignored_paths: vec!["specs/milestones/".to_string()],
            ..Default::default()
        };
        assert!(super::should_create_lifecycle_task(
            None,
            &config,
            'M',
            "specs/system/a.md",
            None
        ));
        assert!(!super::should_create_lifecycle_task(
            None,
            &config,
            'M',
            "specs/milestones/b.md",
            None
        ));
    }

    // ── process_spec_lifecycle: per-repo config end-to-end ────────────────

    /// Set up a real bare repo with two pushed commits to main:
    /// one adding `specs/system/a.md`, one adding `docs/guides/b.md`.
    /// Returns (state, repo_id, tmp_dir, first_commit_sha, second_commit_sha).
    async fn spec_lifecycle_fixture() -> (Arc<crate::AppState>, String, TempDir, String, String) {
        let tmp = TempDir::new().unwrap();
        let bare = tmp.path().join("repo.git");
        let status = std::process::Command::new("git")
            .args(["init", "--bare", "--initial-branch=main", "-q"])
            .arg(&bare)
            .status()
            .unwrap();
        assert!(status.success(), "git init --bare failed");

        // Clone to a work dir, commit the spec files, push to main.
        let work = tmp.path().join("work");
        std::fs::create_dir_all(&work).unwrap();
        let bare_str = bare.to_str().unwrap().to_string();
        let work_str = work.to_str().unwrap().to_string();
        tokio::task::spawn_blocking(move || {
            fn git(dir: &str, args: &[&str]) {
                let out = std::process::Command::new("git")
                    .args(["-C", dir])
                    .args(args)
                    .env("GIT_CONFIG_COUNT", "2")
                    .env("GIT_CONFIG_KEY_0", "user.email")
                    .env("GIT_CONFIG_VALUE_0", "t@t")
                    .env("GIT_CONFIG_KEY_1", "user.name")
                    .env("GIT_CONFIG_VALUE_1", "T")
                    .output()
                    .unwrap();
                assert!(
                    out.status.success(),
                    "git {} failed: {}",
                    args.join(" "),
                    String::from_utf8_lossy(&out.stderr)
                );
            }
            // Local clone of the (empty) bare repo, then two commits.
            let clone = std::process::Command::new("git")
                .args(["clone", &bare_str, &work_str])
                .output()
                .unwrap();
            assert!(
                clone.status.success()
                    || String::from_utf8_lossy(&clone.stderr).contains("empty repository"),
                "clone of empty bare repo failed: {}",
                String::from_utf8_lossy(&clone.stderr)
            );
            git(&work_str, &["checkout", "-q", "-b", "main"]);
            std::fs::create_dir_all(std::path::Path::new(&work_str).join("specs/system")).unwrap();
            std::fs::write(
                std::path::Path::new(&work_str).join("specs/system/a.md"),
                "# A\n",
            )
            .unwrap();
            git(&work_str, &["add", "."]);
            git(&work_str, &["commit", "-q", "-m", "feat(x): add spec a"]);
            git(&work_str, &["push", "-q", "origin", "HEAD:refs/heads/main"]);

            git(&work_str, &["pull", "-q", "origin", "main"]);
            std::fs::create_dir_all(std::path::Path::new(&work_str).join("docs/guides")).unwrap();
            std::fs::write(
                std::path::Path::new(&work_str).join("docs/guides/b.md"),
                "# B\n",
            )
            .unwrap();
            git(&work_str, &["add", "."]);
            git(&work_str, &["commit", "-q", "-m", "feat(x): add guide b"]);
            git(&work_str, &["push", "-q", "origin", "HEAD:refs/heads/main"]);
        })
        .await
        .unwrap();

        // Read the pushed SHAs from the bare repo.
        let first = {
            let out = std::process::Command::new("git")
                .args(["-C", bare.to_str().unwrap(), "rev-parse", "main~1"])
                .output()
                .unwrap();
            assert!(out.status.success());
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        };
        let second = {
            let out = std::process::Command::new("git")
                .args(["-C", bare.to_str().unwrap(), "rev-parse", "main"])
                .output()
                .unwrap();
            assert!(out.status.success());
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        };

        let state = test_state();
        let repo = Repository::new(
            Id::new("repo-slc"),
            Id::new("ws-test"),
            "slc-repo",
            bare.to_str().unwrap(),
            0,
        );
        state.repos.create(&repo).await.unwrap();

        (state, "repo-slc".to_string(), tmp, first, second)
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn spec_lifecycle_disabled_skips_task_creation() {
        // spec-lifecycle.md §Configuration: `enabled = false` must skip
        // processing entirely — no task, no approval invalidation.
        // Zero old_sha models the initial push (hook substitutes the
        // empty-tree SHA itself).
        let (state, repo_id, _tmp, _first, second) = spec_lifecycle_fixture().await;
        let zeros = "0000000000000000000000000000000000000000".to_string();

        // Disable via the same port the PUT handler writes through.
        state
            .spec_lifecycle_configs
            .set_for_repo(
                &repo_id,
                gyre_domain::SpecLifecycleConfig {
                    enabled: false,
                    ..Default::default()
                },
            )
            .await
            .unwrap();

        let updates = vec![super::RefUpdate {
            old_sha: zeros,
            new_sha: second,
            refname: "refs/heads/main".to_string(),
        }];
        let repo_path = state
            .repos
            .find_by_id(&Id::new(&repo_id))
            .await
            .unwrap()
            .unwrap()
            .path
            .clone();
        super::process_spec_lifecycle(&state, &repo_id, &repo_path, "main", &updates).await;

        let tasks = state.tasks.list().await.unwrap();
        assert!(
            tasks.iter().all(|t| t.repo_id.to_string() != repo_id),
            "no tasks must be created when spec lifecycle is disabled"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn spec_lifecycle_per_repo_config_drives_filtering_and_priority() {
        // Custom config: watch docs/guides instead, with a custom priority.
        // This must replace the default watched prefixes entirely — the
        // specs/system file in the same diff must NOT produce a task.
        let (state, repo_id, _tmp, _first, second) = spec_lifecycle_fixture().await;
        let zeros = "0000000000000000000000000000000000000000".to_string();

        state
            .spec_lifecycle_configs
            .set_for_repo(
                &repo_id,
                gyre_domain::SpecLifecycleConfig {
                    watched_paths: vec!["docs/guides/".to_string()],
                    default_priority_new: gyre_domain::TaskPriority::Critical,
                    ..Default::default()
                },
            )
            .await
            .unwrap();

        let updates = vec![super::RefUpdate {
            old_sha: zeros,
            new_sha: second,
            refname: "refs/heads/main".to_string(),
        }];
        let repo_path = state
            .repos
            .find_by_id(&Id::new(&repo_id))
            .await
            .unwrap()
            .unwrap()
            .path
            .clone();
        super::process_spec_lifecycle(&state, &repo_id, &repo_path, "main", &updates).await;

        let tasks = state.tasks.list().await.unwrap();
        let created: Vec<&gyre_domain::Task> = tasks
            .iter()
            .filter(|t| t.repo_id.to_string() == repo_id)
            .collect();
        assert_eq!(
            created.len(),
            1,
            "exactly one task for the watched docs/guides path, got: {created:?}"
        );
        let task = created[0];
        assert_eq!(task.spec_path.as_deref(), Some("docs/guides/b.md"));
        assert_eq!(task.priority, gyre_domain::TaskPriority::Critical);
        assert!(
            task.labels.contains(&"spec-implementation".to_string()),
            "labels: {:?}",
            task.labels
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn spec_lifecycle_dedup_skip_still_records_spec_changed_audit() {
        // Rebase union of task-109 (config-driven dedup gate) and task-104
        // (SpecChanged audit): when dedup skips task creation, the audit
        // event must still fire — with task_id null.
        let (state, repo_id, _tmp, first, _second) = spec_lifecycle_fixture().await;
        let zeros = "0000000000000000000000000000000000000000".to_string();

        let updates = vec![super::RefUpdate {
            old_sha: zeros.clone(),
            new_sha: first.clone(),
            refname: "refs/heads/main".to_string(),
        }];
        let repo_path = state
            .repos
            .find_by_id(&Id::new(&repo_id))
            .await
            .unwrap()
            .unwrap()
            .path
            .clone();

        // Same diff processed twice: the second pass hits the open-task
        // dedup gate (default config: dedup_open_tasks = true).
        super::process_spec_lifecycle(&state, &repo_id, &repo_path, "main", &updates).await;
        super::process_spec_lifecycle(&state, &repo_id, &repo_path, "main", &updates).await;

        let tasks = state.tasks.list().await.unwrap();
        let created: Vec<&gyre_domain::Task> = tasks
            .iter()
            .filter(|t| t.repo_id.to_string() == repo_id)
            .collect();
        assert_eq!(
            created.len(),
            1,
            "dedup must collapse the repeated change into one task"
        );

        let events = wait_for_audit_event(&state, "spec_changed").await;
        let dedup_events: Vec<&gyre_domain::AuditEvent> = events
            .iter()
            .filter(|e| e.detail["task_id"].is_null())
            .collect();
        assert_eq!(
            dedup_events.len(),
            1,
            "the dedup-skipped pass must record exactly one task_id-less SpecChanged event, got: {events:?}"
        );
        assert_eq!(dedup_events[0].detail["spec_path"], "specs/system/a.md");
        // The first pass recorded its event referencing the auto-created task.
        let with_task: Vec<&gyre_domain::AuditEvent> = events
            .iter()
            .filter(|e| e.detail["task_id"].as_str().is_some())
            .collect();
        assert_eq!(with_task.len(), 1, "events: {events:?}");
    }

    // ── Manifest-driven lifecycle gating (spec-registry.md §15) ──────────

    const GATE_MANIFEST: &str = r#"
version: 1
specs:
  - path: system/spec-a.md
    title: Spec A
    owner: user:test
    auto_create_tasks: false
  - path: system/spec-b.md
    title: Spec B
    owner: user:test
    auto_create_tasks: true
  - path: coverage/coverage-matrix.md
    title: Coverage Matrix
    owner: user:test
"#;

    #[test]
    fn should_create_lifecycle_task_manifest_gate() {
        let m = crate::spec_registry::parse_manifest(GATE_MANIFEST).unwrap();
        // Registered with auto_create_tasks: false → never spawns a task,
        // regardless of change kind.
        assert!(!super::should_create_lifecycle_task(
            Some(&m),
            &Default::default(),
            'M',
            "specs/system/spec-a.md",
            None
        ));
        assert!(!super::should_create_lifecycle_task(
            Some(&m),
            &Default::default(),
            'A',
            "specs/system/spec-a.md",
            None
        ));
        assert!(!super::should_create_lifecycle_task(
            Some(&m),
            &Default::default(),
            'D',
            "specs/system/spec-a.md",
            None
        ));
        // Registered with auto_create_tasks: true → task.
        assert!(super::should_create_lifecycle_task(
            Some(&m),
            &Default::default(),
            'M',
            "specs/system/spec-b.md",
            None
        ));
        // Unregistered specs/ file → no task (manifest is the source of truth).
        assert!(!super::should_create_lifecycle_task(
            Some(&m),
            &Default::default(),
            'M',
            "specs/system/unregistered.md",
            None
        ));
        // Registered under a non-watched prefix: the manifest, not the path
        // prefix, decides — defaults.auto_create_tasks is true here.
        assert!(super::should_create_lifecycle_task(
            Some(&m),
            &Default::default(),
            'M',
            "specs/coverage/coverage-matrix.md",
            None
        ));
        // Manifest-mode renames match on the NEW path only (task-194
        // instruction): a rename whose old path is registered but whose new
        // path is not registered creates no task.
        assert!(!super::should_create_lifecycle_task(
            Some(&m),
            &Default::default(),
            'R',
            "specs/milestones/renamed-away.md",
            Some("specs/system/spec-b.md")
        ));
        // …while a rename onto a registered path creates one.
        assert!(super::should_create_lifecycle_task(
            Some(&m),
            &Default::default(),
            'R',
            "specs/system/spec-b.md",
            Some("specs/milestones/renamed-from.md")
        ));
    }

    #[test]
    fn should_create_lifecycle_task_defaults_section_applies() {
        // defaults.auto_create_tasks: false — per-entry override wins.
        let yaml = "version: 1\ndefaults:\n  auto_create_tasks: false\nspecs:\n  - path: system/opt-in.md\n    title: Opt In\n    owner: user:test\n    auto_create_tasks: true\n  - path: system/opt-out.md\n    title: Opt Out\n    owner: user:test\n";
        let m = crate::spec_registry::parse_manifest(yaml).unwrap();
        assert!(super::should_create_lifecycle_task(
            Some(&m),
            &Default::default(),
            'M',
            "specs/system/opt-in.md",
            None
        ));
        assert!(!super::should_create_lifecycle_task(
            Some(&m),
            &Default::default(),
            'M',
            "specs/system/opt-out.md",
            None
        ));
    }

    #[test]
    fn should_create_lifecycle_task_no_manifest_falls_back_to_prefixes() {
        // Manifest-less repo: legacy path-prefix watch rules keep working.
        assert!(super::should_create_lifecycle_task(
            None,
            &Default::default(),
            'M',
            "specs/system/foo.md",
            None
        ));
        assert!(super::should_create_lifecycle_task(
            None,
            &Default::default(),
            'A',
            "specs/development/bar.md",
            None
        ));
        assert!(!super::should_create_lifecycle_task(
            None,
            &Default::default(),
            'M',
            "specs/milestones/m1.md",
            None
        ));
        assert!(!super::should_create_lifecycle_task(
            None,
            &Default::default(),
            'M',
            "src/main.rs",
            None
        ));
        // Unclassified change kinds never create tasks.
        assert!(!super::should_create_lifecycle_task(
            None,
            &Default::default(),
            'X',
            "specs/system/foo.md",
            None
        ));
    }

    #[test]
    fn should_create_lifecycle_task_no_manifest_rename_either_side_watched() {
        // Manifest-less repo, renames: the legacy prefix fallback admits a
        // rename when EITHER side sits under a watched prefix (matching base
        // pre-manifest `parse_spec_changes` + `classify_spec_change('R')`, which always
        // created a task). Regression test for the task-194 review finding:
        // the None arm previously checked only the new path.
        // Old path watched, new path under specs/ but unwatched → task.
        assert!(super::should_create_lifecycle_task(
            None,
            &Default::default(),
            'R',
            "specs/milestones/a.md",
            Some("specs/system/foo.md")
        ));
        // Old path watched, new path outside specs/ entirely → task.
        assert!(super::should_create_lifecycle_task(
            None,
            &Default::default(),
            'R',
            "docs/b.md",
            Some("specs/system/foo.md")
        ));
        // New path watched, old path unwatched → task (already worked).
        assert!(super::should_create_lifecycle_task(
            None,
            &Default::default(),
            'R',
            "specs/system/new.md",
            Some("specs/milestones/old.md")
        ));
        // Neither side watched → no task.
        assert!(!super::should_create_lifecycle_task(
            None,
            &Default::default(),
            'R',
            "docs/b.md",
            Some("specs/milestones/old.md")
        ));
        // Neither side watched, both outside specs/ → no task.
        assert!(!super::should_create_lifecycle_task(
            None,
            &Default::default(),
            'R',
            "docs/b.md",
            Some("docs/old.md")
        ));
    }

    /// Build a real (non-bare) git repo in a temp dir with one commit per
    /// entry; each commit is a list of (path, contents) files. Returns
    /// (temp_dir, repo_path, commit_shas_in_order).
    fn git_repo_with_commits(commits: &[Vec<(&str, String)>]) -> (TempDir, String, Vec<String>) {
        let tmp = TempDir::new().unwrap();
        let repo_path = tmp.path().join("repo");
        std::fs::create_dir_all(&repo_path).unwrap();

        let git = |args: &[&str]| {
            let out = std::process::Command::new("git")
                .arg("-C")
                .arg(&repo_path)
                .args(args)
                .env("GIT_AUTHOR_NAME", "test")
                .env("GIT_AUTHOR_EMAIL", "test@example.com")
                .env("GIT_COMMITTER_NAME", "test")
                .env("GIT_COMMITTER_EMAIL", "test@example.com")
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "git {:?} failed: {}",
                args,
                String::from_utf8_lossy(&out.stderr)
            );
        };

        git(&["init", "-b", "main"]);

        let mut shas = Vec::new();
        for files in commits {
            for (path, contents) in files {
                let full = repo_path.join(path);
                if let Some(parent) = full.parent() {
                    std::fs::create_dir_all(parent).unwrap();
                }
                std::fs::write(&full, contents).unwrap();
                git(&["add", "--", path]);
            }
            git(&["commit", "-m", "test commit"]);
            let out = std::process::Command::new("git")
                .arg("-C")
                .arg(&repo_path)
                .arg("rev-parse")
                .arg("HEAD")
                .output()
                .unwrap();
            assert!(out.status.success());
            shas.push(String::from_utf8_lossy(&out.stdout).trim().to_string());
        }

        (tmp, repo_path.to_str().unwrap().to_string(), shas)
    }

    /// A test state with a workspace (ws-test) and a repo (repo-1) whose
    /// storage path points at the given git repo, so lifecycle task events
    /// can resolve their workspace scope.
    async fn state_with_registered_repo(repo_path: &str) -> Arc<crate::AppState> {
        let state = test_state();
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
            repo_path,
            0,
        );
        state.repos.create(&repo).await.unwrap();
        state
    }

    #[tokio::test]
    async fn spec_lifecycle_manifest_gates_task_creation_e2e() {
        // Manifest marks spec-a auto_create_tasks: false, spec-b true (with a
        // priority override); spec-c is not registered at all.
        let manifest = r#"
version: 1
specs:
  - path: system/spec-a.md
    title: Spec A
    owner: user:test
    auto_create_tasks: false
  - path: system/spec-b.md
    title: Spec B
    owner: user:test
    auto_create_tasks: true
    priority: Critical
"#
        .to_string();

        let (_tmp, repo_path, shas) = git_repo_with_commits(&[
            vec![
                ("specs/manifest.yaml", manifest),
                ("specs/system/spec-a.md", "# A v1".to_string()),
                ("specs/system/spec-b.md", "# B v1".to_string()),
            ],
            vec![
                ("specs/system/spec-a.md", "# A v2".to_string()),
                ("specs/system/spec-b.md", "# B v2".to_string()),
                ("specs/system/spec-c.md", "# C v1".to_string()),
            ],
        ]);

        let state = state_with_registered_repo(&repo_path).await;
        let mut events = state.message_broadcast_tx.subscribe();

        let update = super::RefUpdate {
            old_sha: shas[0].clone(),
            new_sha: shas[1].clone(),
            refname: "refs/heads/main".to_string(),
        };
        super::process_spec_lifecycle(&state, "repo-1", &repo_path, "main", &[update]).await;

        let tasks = state.tasks.list().await.unwrap();
        let titles: Vec<&str> = tasks.iter().map(|t| t.title.as_str()).collect();
        assert_eq!(
            tasks.len(),
            1,
            "expected exactly one lifecycle task, got: {titles:?}"
        );
        assert_eq!(tasks[0].title, "Review spec change: specs/system/spec-b.md");
        assert_eq!(
            tasks[0].spec_path.as_deref(),
            Some("specs/system/spec-b.md")
        );
        // Per-spec priority override from the manifest (§15), not the
        // change-kind default (High).
        assert_eq!(tasks[0].priority, gyre_domain::TaskPriority::Critical);
        // No task for spec-a (auto_create_tasks: false) or the unregistered
        // spec-c — asserted by title AND by spec_path.
        assert!(tasks.iter().all(|t| {
            t.spec_path.as_deref() != Some("specs/system/spec-a.md")
                && t.spec_path.as_deref() != Some("specs/system/spec-c.md")
        }));
        assert!(!titles.iter().any(|t| t.contains("spec-a")));
        assert!(!titles.iter().any(|t| t.contains("spec-c")));

        // Event tier: exactly one SpecChanged and one TaskCreated, both for
        // spec-b — none for the excluded specs.
        let mut spec_changed: Vec<serde_json::Value> = Vec::new();
        let mut task_created = 0usize;
        while let Ok(msg) = events.try_recv() {
            use gyre_common::message::MessageKind;
            match msg.kind {
                MessageKind::SpecChanged => {
                    spec_changed.push(msg.payload.unwrap_or_default());
                }
                MessageKind::TaskCreated => {
                    task_created += 1;
                }
                _ => {}
            }
        }
        assert_eq!(
            spec_changed.len(),
            1,
            "SpecChanged events: {spec_changed:?}"
        );
        assert_eq!(
            spec_changed[0]["spec_path"].as_str(),
            Some("specs/system/spec-b.md")
        );
        assert_eq!(task_created, 1);
    }

    #[tokio::test]
    async fn spec_lifecycle_without_manifest_uses_prefix_fallback_e2e() {
        // No specs/manifest.yaml at HEAD: legacy path-prefix behavior must
        // keep working (no regression for manifest-less repos).
        let (_tmp, repo_path, shas) = git_repo_with_commits(&[
            vec![
                ("specs/system/watched.md", "# v1".to_string()),
                ("specs/milestones/unwatched.md", "# v1".to_string()),
            ],
            vec![
                ("specs/system/watched.md", "# v2".to_string()),
                ("specs/milestones/unwatched.md", "# v2".to_string()),
            ],
        ]);

        let state = state_with_registered_repo(&repo_path).await;
        let update = super::RefUpdate {
            old_sha: shas[0].clone(),
            new_sha: shas[1].clone(),
            refname: "refs/heads/main".to_string(),
        };
        super::process_spec_lifecycle(&state, "repo-1", &repo_path, "main", &[update]).await;

        let tasks = state.tasks.list().await.unwrap();
        let titles: Vec<&str> = tasks.iter().map(|t| t.title.as_str()).collect();
        assert_eq!(tasks.len(), 1, "tasks: {titles:?}");
        assert_eq!(
            tasks[0].title,
            "Review spec change: specs/system/watched.md"
        );
        // No manifest → no priority override: change-kind default (High).
        assert_eq!(tasks[0].priority, gyre_domain::TaskPriority::High);
        assert!(!titles.iter().any(|t| t.contains("unwatched")));
    }

    #[tokio::test]
    async fn spec_lifecycle_without_manifest_rename_from_watched_prefix_e2e() {
        // Manifest-less repo, renames whose OLD path sits under a watched
        // prefix: the legacy fallback must still create the 'Update spec
        // references' task (base fefde2946 behavior — AC3 'no regression').
        // Regression test for the task-194 review finding: the manifest-less
        // arm of `should_create_lifecycle_task` previously checked only the
        // NEW path, dropping both of these cases.
        let tmp = TempDir::new().unwrap();
        let repo_path = tmp.path().join("repo");
        std::fs::create_dir_all(&repo_path).unwrap();

        let git = |args: &[&str]| {
            let out = std::process::Command::new("git")
                .arg("-C")
                .arg(&repo_path)
                .args(args)
                .env("GIT_AUTHOR_NAME", "test")
                .env("GIT_AUTHOR_EMAIL", "test@example.com")
                .env("GIT_COMMITTER_NAME", "test")
                .env("GIT_COMMITTER_EMAIL", "test@example.com")
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "git {:?} failed: {}",
                args,
                String::from_utf8_lossy(&out.stderr)
            );
        };

        git(&["init", "-b", "main"]);
        std::fs::create_dir_all(repo_path.join("specs/system")).unwrap();
        std::fs::write(repo_path.join("specs/system/foo.md"), "# v1\n").unwrap();
        std::fs::write(repo_path.join("specs/system/bar.md"), "# v1\n").unwrap();
        git(&["add", "-A"]);
        git(&["commit", "-m", "c1"]);
        let old_sha = std::process::Command::new("git")
            .arg("-C")
            .arg(&repo_path)
            .arg("rev-parse")
            .arg("HEAD")
            .output()
            .unwrap();
        let old_sha = String::from_utf8_lossy(&old_sha.stdout).trim().to_string();

        // Rename one watched spec to an unwatched specs/ path, another out
        // of specs/ entirely — both old paths are watched, so base created
        // tasks for both.
        std::fs::create_dir_all(repo_path.join("specs/milestones")).unwrap();
        std::fs::create_dir_all(repo_path.join("docs")).unwrap();
        git(&["mv", "specs/system/foo.md", "specs/milestones/a.md"]);
        git(&["mv", "specs/system/bar.md", "docs/b.md"]);
        git(&["commit", "-m", "c2"]);
        let new_sha = std::process::Command::new("git")
            .arg("-C")
            .arg(&repo_path)
            .arg("rev-parse")
            .arg("HEAD")
            .output()
            .unwrap();
        let new_sha = String::from_utf8_lossy(&new_sha.stdout).trim().to_string();

        // Sanity: the commit-to-commit diff must show these as renames (R),
        // otherwise the test would not exercise the rename code path.
        {
            let out = std::process::Command::new("git")
                .arg("-C")
                .arg(&repo_path)
                .arg("diff")
                .arg("--name-status")
                .arg(format!("{old_sha}..{new_sha}"))
                .output()
                .unwrap();
            let diff = String::from_utf8_lossy(&out.stdout).to_string();
            assert!(
                diff.contains("R100\tspecs/system/foo.md\tspecs/milestones/a.md"),
                "diff: {diff}"
            );
            assert!(
                diff.contains("R100\tspecs/system/bar.md\tdocs/b.md"),
                "diff: {diff}"
            );
        }

        let repo_path_str = repo_path.to_str().unwrap().to_string();
        let state = state_with_registered_repo(&repo_path_str).await;
        let update = super::RefUpdate {
            old_sha,
            new_sha,
            refname: "refs/heads/main".to_string(),
        };
        super::process_spec_lifecycle(&state, "repo-1", &repo_path_str, "main", &[update]).await;

        let tasks = state.tasks.list().await.unwrap();
        let titles: Vec<&str> = tasks.iter().map(|t| t.title.as_str()).collect();
        assert_eq!(tasks.len(), 2, "tasks: {titles:?}");
        assert!(
            titles
                .contains(&"Update spec references: specs/system/foo.md -> specs/milestones/a.md"),
            "tasks: {titles:?}"
        );
        assert!(
            titles.contains(&"Update spec references: specs/system/bar.md -> docs/b.md"),
            "tasks: {titles:?}"
        );
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
        let acc = accumulate_chain_constraints(&chain);

        assert!(acc.root_input.is_some(), "should find root signed input");
        // Root has 1 constraint + derived has 1 = 2 total.
        assert_eq!(
            acc.explicit_constraints.len(),
            2,
            "should accumulate constraints additively"
        );
        assert!(
            acc.gate_constraints.is_empty(),
            "should have no gate constraints"
        );
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
        let acc = accumulate_chain_constraints(&chain);

        assert_eq!(
            acc.explicit_constraints.len(),
            1,
            "should have 1 explicit constraint"
        );
        assert_eq!(
            acc.gate_constraints.len(),
            1,
            "should have 1 gate constraint"
        );
        assert_eq!(acc.gate_constraints[0].gate_name, "Code Review");
    }

    /// Spec Approval Ledger regression (agent-gates.md §Spec Approval Ledger):
    /// a push that modifies a spec file must auto-invalidate active approvals.
    ///
    /// Approval rows are keyed by ledger paths ("system/foo.md") while the push
    /// diff carries git paths ("specs/system/foo.md"); without normalization
    /// the revocation silently matched nothing and stale approvals stayed
    /// active (forge then merged against a spec version nobody approved).
    #[tokio::test(flavor = "multi_thread")]
    async fn push_modifying_spec_revokes_ledger_approvals() {
        let state = test_state();

        // Real repo with two commits: v1 of the spec, then a modification.
        let tmp = TempDir::new().unwrap();
        let repo_path = tmp.path().join("spec-repo");
        std::fs::create_dir_all(repo_path.join("specs/system")).unwrap();
        std::fs::write(
            repo_path.join("specs/system/payments.md"),
            "# Payments v1\n",
        )
        .unwrap();

        let git = |args: &[&str]| {
            let out = std::process::Command::new("git")
                .args(args)
                .current_dir(&repo_path)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "git {:?} failed: {}",
                args,
                String::from_utf8_lossy(&out.stderr)
            );
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        };
        git(&["init", "-b", "main"]);
        git(&["config", "user.email", "test@test.com"]);
        git(&["config", "user.name", "Test"]);
        git(&["add", "."]);
        git(&["commit", "-m", "v1"]);
        let old_sha = git(&["rev-parse", "HEAD"]);

        // Active approval keyed by the LEDGER path (as approve_spec records).
        let approval = gyre_domain::SpecApproval::new(
            gyre_common::Id::new("apr-stale"),
            "system/payments.md",
            "a".repeat(40),
            "user:approver",
        );
        state.spec_approvals.create(&approval).await.unwrap();
        state
            .spec_approvals
            .approve(&approval.id, 1700000000)
            .await
            .unwrap()
            .expect("entry exists");
        assert!(state
            .spec_approvals
            .find_by_id(&approval.id)
            .await
            .unwrap()
            .unwrap()
            .is_active());

        // Second commit modifies the spec file on main.
        std::fs::write(
            repo_path.join("specs/system/payments.md"),
            "# Payments v2\n",
        )
        .unwrap();
        git(&["add", "."]);
        git(&["commit", "-m", "v2"]);
        let new_sha = git(&["rev-parse", "HEAD"]);

        // Drive the push-time lifecycle hook (default-branch update).
        process_spec_lifecycle(
            &state,
            "repo-stale",
            repo_path.to_str().unwrap(),
            "main",
            &[RefUpdate {
                old_sha,
                new_sha,
                refname: "refs/heads/main".to_string(),
            }],
        )
        .await;

        // The approval must now be Revoked — the spec content changed.
        let reloaded = state
            .spec_approvals
            .find_by_id(&approval.id)
            .await
            .unwrap()
            .expect("approval row survives invalidation");
        assert_eq!(
            reloaded.status(),
            gyre_domain::spec_approval::ApprovalStatus::Revoked,
            "push modifying spec must revoke the ledger approval (it was keyed by ledger path, diff by git path)"
        );
        assert_eq!(
            reloaded.approved_at, None,
            "mutual exclusivity: approved_at cleared"
        );
        assert!(reloaded
            .revocation_reason
            .as_deref()
            .unwrap()
            .contains("modified"));

        // And the revoked approval no longer authorizes merges (forge check).
        let spec_ref = format!("system/payments.md@{}", "a".repeat(40));
        assert!(
            crate::api::gates::verify_spec_ref(&state, &spec_ref)
                .await
                .is_err(),
            "revoked approval must fail verify_spec_ref"
        );
    }

    /// TASK-167 / merge-dependencies.md §2: an end-to-end push through
    /// `git_receive_pack` (stateless receive-pack body, real git) must
    /// trigger lineage re-evaluation in post-receive processing. The
    /// parent MR's branch is pushed; the child's stale lineage edge must
    /// be dropped even though the child's branch never moved.
    #[tokio::test(flavor = "multi_thread")]
    async fn push_rebases_parent_branch_drops_child_lineage_dep() {
        let (app, state, _tmp, ws_slug, repo_name, repo_path) = git_app_with_repo().await;

        // Author commits with git plumbing directly in the bare repo.
        let git = |args: &[&str]| -> String {
            let out = std::process::Command::new("git")
                .arg("-C")
                .arg(&repo_path)
                .args(args)
                .env("GIT_AUTHOR_NAME", "t")
                .env("GIT_AUTHOR_EMAIL", "t@t")
                .env("GIT_COMMITTER_NAME", "t")
                .env("GIT_COMMITTER_EMAIL", "t@t")
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "git {:?} failed: {}",
                args,
                String::from_utf8_lossy(&out.stderr)
            );
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        };
        let commit_on = |branch: &str, parent: Option<&str>, msg: &str| -> String {
            let tree = git(&["mktree"]);
            let sha = match parent {
                Some(p) => git(&["commit-tree", &tree, "-p", p, "-m", msg]),
                None => git(&["commit-tree", &tree, "-m", msg]),
            };
            git(&["update-ref", &format!("refs/heads/{branch}"), &sha]);
            sha
        };

        // main → feat/a (MR A) → feat/b (MR B, forked from feat/a).
        let base = commit_on("main", None, "base");
        let a = commit_on("feat/a", Some(&base), "feat A");
        let _b = commit_on("feat/b", Some(&a), "feat B on A");

        let repo = state
            .repos
            .list()
            .await
            .unwrap()
            .into_iter()
            .find(|r| r.name == TEST_REPO_NAME)
            .unwrap();
        let mr_a_id = {
            let mut mr = gyre_domain::MergeRequest::new(
                gyre_common::Id::new("mr-a"),
                repo.id.clone(),
                "A",
                "feat/a".to_string(),
                "main".to_string(),
                0,
            );
            mr.workspace_id = gyre_common::Id::new("ws-test");
            state.merge_requests.create(&mr).await.unwrap();
            mr.id.to_string()
        };
        let mr_b_id = {
            let mut mr = gyre_domain::MergeRequest::new(
                gyre_common::Id::new("mr-b"),
                repo.id.clone(),
                "B",
                "feat/b".to_string(),
                "main".to_string(),
                0,
            );
            mr.workspace_id = gyre_common::Id::new("ws-test");
            state.merge_requests.create(&mr).await.unwrap();
            mr.id.to_string()
        };

        // B's MR is created via the domain path (no REST in this module's
        // harness) — seed its lineage edge by running the creation-time
        // detection directly, exactly what create_mr does.
        let lineage = crate::api::merge_requests::detect_lineage_deps(
            &state,
            &repo.id,
            &repo.path,
            "feat/b",
            &repo.default_branch,
        )
        .await;
        assert_eq!(lineage.len(), 1, "B must detect lineage on A");
        {
            let mut mr_b = state
                .merge_requests
                .find_by_id(&gyre_common::Id::new(&mr_b_id))
                .await
                .unwrap()
                .unwrap();
            mr_b.depends_on = lineage;
            state.merge_requests.update(&mr_b).await.unwrap();
        }

        // Rebase feat/a onto a fresh main commit: the rebased commit is
        // authored on a temp branch so feat/a's current tip stays the
        // pre-rebase commit; the push below then moves feat/a for real
        // (non-fast-forward is accepted by default in a bare repo).
        let new_main = commit_on("main", Some(&base), "main moves on");
        let a2 = commit_on("feat/a2", Some(&new_main), "A rebased");

        // Push feat/a through the real HTTP handler: pkt-line
        // `{old} {new} refs/heads/feat/a` + flush-pkt + a packfile
        // containing the new commit (receive-pack requires the objects,
        // even though they already exist in the repo).
        let old_a = git(&["rev-parse", "refs/heads/feat/a"]);
        let line = format!("{old_a} {a2} refs/heads/feat/a");
        let mut body = super::pkt_line(&line);
        body.extend_from_slice(b"0000");
        let pack = {
            let mut child = std::process::Command::new("git")
                .arg("-C")
                .arg(&repo_path)
                .args(["pack-objects", "--stdout"])
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .spawn()
                .unwrap();
            use std::io::Write as _;
            child
                .stdin
                .take()
                .unwrap()
                .write_all(format!("{a2}\n").as_bytes())
                .unwrap();
            let out = child.wait_with_output().unwrap();
            assert!(out.status.success(), "pack-objects failed");
            out.stdout
        };
        assert!(!pack.is_empty(), "pack-objects produced empty pack");
        body.extend_from_slice(&pack);
        // The push must actually move the ref (verified below before
        // asserting on the lineage effect).

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/git/{ws_slug}/{repo_name}/git-receive-pack"))
                    .header("Authorization", "Bearer test-token")
                    .header("content-type", "application/x-git-receive-pack-request")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // The push itself must have moved feat/a (else the test would pass
        // vacuously — parse_ref_updates only records non-deletion updates
        // with valid SHAs).
        assert_eq!(
            git(&["rev-parse", "refs/heads/feat/a"]),
            a2,
            "receive-pack did not move refs/heads/feat/a to the rebased commit"
        );

        // Post-receive processing runs in a spawned task — poll until the
        // child's lineage edge is dropped (bounded).
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            let mr_b = state
                .merge_requests
                .find_by_id(&gyre_common::Id::new(&mr_b_id))
                .await
                .unwrap()
                .unwrap();
            if mr_b.depends_on.is_empty() {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "lineage re-evaluation after parent-branch push never ran; \
                 mr_b still depends on: {:?}",
                mr_b.depends_on
            );
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }

        // The parent's own MR stays dep-free (detection is one-directional).
        let mr_a = state
            .merge_requests
            .find_by_id(&gyre_common::Id::new(&mr_a_id))
            .await
            .unwrap()
            .unwrap();
        assert!(mr_a.depends_on.is_empty());
    }

    // -- task-104: source control audit events (observability.md §Audit
    //    Event Types) -----------------------------------------------

    async fn audit_events_of(
        state: &crate::AppState,
        event_type: &str,
    ) -> Vec<gyre_domain::AuditEvent> {
        gyre_ports::AuditRepository::query(
            &*state.audit,
            &gyre_ports::AuditQueryFilter {
                event_type: Some(event_type.to_string()),
                limit: 100,
                ..Default::default()
            },
        )
        .await
        .unwrap()
    }

    /// Poll the audit store until at least one event of the given type
    /// appears (post-receive processing runs in a spawned task), bounded.
    async fn wait_for_audit_event(
        state: &crate::AppState,
        event_type: &str,
    ) -> Vec<gyre_domain::AuditEvent> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            let events = audit_events_of(state, event_type).await;
            if !events.is_empty() {
                return events;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "no '{event_type}' audit event was recorded within 30s"
            );
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    }

    /// Run a git command in the repo with stdin input; returns trimmed stdout.
    fn git_with_input(repo_path: &str, args: &[&str], input: &str) -> String {
        use std::io::Write as _;
        let mut child = std::process::Command::new("git")
            .arg("-C")
            .arg(repo_path)
            .args(args)
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@t")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@t")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
        let out = child.wait_with_output().unwrap();
        assert!(
            out.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn push_records_git_push_accepted_audit_event() {
        let (app, state, _tmp, ws_slug, repo_name, repo_path) = git_app_with_repo().await;

        let git = |args: &[&str]| -> String {
            let out = std::process::Command::new("git")
                .arg("-C")
                .arg(&repo_path)
                .args(args)
                .env("GIT_AUTHOR_NAME", "t")
                .env("GIT_AUTHOR_EMAIL", "t@t")
                .env("GIT_COMMITTER_NAME", "t")
                .env("GIT_COMMITTER_EMAIL", "t@t")
                .output()
                .unwrap();
            assert!(out.status.success(), "git {:?} failed", args);
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        };

        // Configure the conventional-commit gate so the accepted event
        // carries a real push_gate_results entry.
        state
            .repo_push_gates
            .set_for_repo("repo-1", vec!["conventional-commit".to_string()])
            .await
            .unwrap();

        let tree = git(&["mktree"]);
        let base = git(&["commit-tree", &tree, "-m", "chore: base"]);
        git(&["update-ref", "refs/heads/main", &base]);
        let tip = git(&["commit-tree", &tree, "-p", &base, "-m", "feat: move main"]);
        git(&["update-ref", "refs/heads/main", &tip]);

        // Push body: ref update command + flush + packfile with the objects.
        let line = format!("{base} {tip} refs/heads/main");
        let mut body = super::pkt_line(&line);
        body.extend_from_slice(b"0000");
        let pack = {
            use std::io::Write as _;
            let mut child = std::process::Command::new("git")
                .arg("-C")
                .arg(&repo_path)
                .args(["pack-objects", "--stdout"])
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .spawn()
                .unwrap();
            child
                .stdin
                .take()
                .unwrap()
                .write_all(format!("{tip}\n").as_bytes())
                .unwrap();
            let out = child.wait_with_output().unwrap();
            assert!(out.status.success(), "pack-objects failed");
            out.stdout
        };
        body.extend_from_slice(&pack);

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/git/{ws_slug}/{repo_name}/git-receive-pack"))
                    .header("Authorization", "Bearer test-token")
                    .header("content-type", "application/x-git-receive-pack-request")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            git(&["rev-parse", "refs/heads/main"]),
            tip,
            "receive-pack did not move refs/heads/main"
        );

        let events = wait_for_audit_event(&state, "git_push_accepted").await;
        assert_eq!(events.len(), 1, "one event per ref update");
        let e = &events[0];
        assert_eq!(e.detail["repo_id"], "repo-1");
        assert_eq!(e.detail["branch"], "main");
        assert_eq!(e.detail["commit_sha"], tip.as_str());
        assert_eq!(e.detail["agent_id"], "system");
        // The configured gate passed and its outcome is recorded.
        let gates = e.detail["push_gate_results"].as_array().unwrap();
        assert_eq!(gates.len(), 1);
        assert_eq!(gates[0]["gate"], "conventional-commit");
        assert_eq!(gates[0]["passed"], true);
        // Envelope scoping.
        assert_eq!(e.agent_id.as_ref().unwrap().as_str(), "system");
        assert_eq!(e.repo_id.as_ref().unwrap().as_str(), "repo-1");
        assert_eq!(e.workspace_id.as_ref().unwrap().as_str(), "ws-test");
        assert_eq!(e.outcome, gyre_domain::AuditOutcome::Success);
        assert_eq!(e.resource_type, "repo");
    }

    /// Review repair regression test (task-104): `git receive-pack` declines
    /// a non-fast-forward ref update in its report-status stream while the
    /// process still exits 0. The stale-client push below (server ref at
    /// `tip`, client pushes `base -> divergent`) must NOT record a
    /// GitPushAccepted audit event — the spec trigger is "Smart HTTP push
    /// accepted", and the update never landed.
    #[tokio::test(flavor = "multi_thread")]
    async fn push_declined_by_git_records_no_git_push_accepted_audit_event() {
        let (app, state, _tmp, ws_slug, repo_name, repo_path) = git_app_with_repo().await;

        let git = |args: &[&str]| -> String {
            let out = std::process::Command::new("git")
                .arg("-C")
                .arg(&repo_path)
                .args(args)
                .env("GIT_AUTHOR_NAME", "t")
                .env("GIT_AUTHOR_EMAIL", "t@t")
                .env("GIT_COMMITTER_NAME", "t")
                .env("GIT_COMMITTER_EMAIL", "t@t")
                .output()
                .unwrap();
            assert!(out.status.success(), "git {:?} failed", args);
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        };

        // Server history: base <- tip, with refs/heads/main at `tip`.
        let tree = git(&["mktree"]);
        let base = git(&["commit-tree", &tree, "-m", "chore: base"]);
        git(&["update-ref", "refs/heads/main", &base]);
        let tip = git(&["commit-tree", &tree, "-p", &base, "-m", "feat: move main"]);
        git(&["update-ref", "refs/heads/main", &tip]);
        // Stale client diverged from `base`; its update is non-fast-forward
        // relative to the server's `tip`.
        let stale = git(&[
            "commit-tree",
            &tree,
            "-p",
            &base,
            "-m",
            "feat: stale client",
        ]);

        // Push body: stale client sends `base -> stale` (its outdated view of
        // the ref) plus a packfile containing the stale commit.
        let line = format!("{base} {stale} refs/heads/main");
        let mut body = super::pkt_line(&line);
        body.extend_from_slice(b"0000");
        let pack = {
            use std::io::Write as _;
            let mut child = std::process::Command::new("git")
                .arg("-C")
                .arg(&repo_path)
                .args(["pack-objects", "--stdout"])
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .spawn()
                .unwrap();
            child
                .stdin
                .take()
                .unwrap()
                .write_all(format!("{stale}\n").as_bytes())
                .unwrap();
            let out = child.wait_with_output().unwrap();
            assert!(out.status.success(), "pack-objects failed");
            out.stdout
        };
        body.extend_from_slice(&pack);

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/git/{ws_slug}/{repo_name}/git-receive-pack"))
                    .header("Authorization", "Bearer test-token")
                    .header("content-type", "application/x-git-receive-pack-request")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        // receive-pack exits 0 even though it declined the ref update —
        // the rejection travels in the report-status stream, so the HTTP
        // response stays 200 with git's own report.
        assert_eq!(resp.status(), StatusCode::OK);
        // The ref update was NOT applied: main still points at `tip`.
        assert_eq!(
            git(&["rev-parse", "refs/heads/main"]),
            tip,
            "receive-pack must not have moved refs/heads/main"
        );

        // Give the spawned post-receive task ample opportunity to (wrongly)
        // emit acceptance events, then assert none exist.
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        let accepted = audit_events_of(&state, "git_push_accepted").await;
        assert!(
            accepted.is_empty(),
            "git declined the ref update, but git_push_accepted events were recorded: {}",
            serde_json::to_string_pretty(
                &accepted
                    .iter()
                    .map(|e| e.detail.clone())
                    .collect::<Vec<_>>()
            )
            .unwrap()
        );
        // A git-level decline is not a gate rejection either — the spec
        // reserves GitPushRejected for "push rejected by gate".
        let rejected = audit_events_of(&state, "git_push_rejected").await;
        assert!(rejected.is_empty());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn push_rejected_by_gate_records_git_push_rejected_audit_event() {
        let (app, state, _tmp, ws_slug, repo_name, repo_path) = git_app_with_repo().await;

        let git = |args: &[&str]| -> String {
            let out = std::process::Command::new("git")
                .arg("-C")
                .arg(&repo_path)
                .args(args)
                .env("GIT_AUTHOR_NAME", "t")
                .env("GIT_AUTHOR_EMAIL", "t@t")
                .env("GIT_COMMITTER_NAME", "t")
                .env("GIT_COMMITTER_EMAIL", "t@t")
                .output()
                .unwrap();
            assert!(out.status.success(), "git {:?} failed", args);
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        };

        state
            .repo_push_gates
            .set_for_repo("repo-1", vec!["conventional-commit".to_string()])
            .await
            .unwrap();

        let tree = git(&["mktree"]);
        let base = git(&["commit-tree", &tree, "-m", "chore: base"]);
        git(&["update-ref", "refs/heads/main", &base]);
        // Non-conventional commit message: the gate must reject the push.
        let tip = git(&[
            "commit-tree",
            &tree,
            "-p",
            &base,
            "-m",
            "not conventional at all",
        ]);
        git(&["update-ref", "refs/heads/main", &tip]);

        let line = format!("{base} {tip} refs/heads/main");
        let mut body = super::pkt_line(&line);
        body.extend_from_slice(b"0000");
        let pack = {
            use std::io::Write as _;
            let mut child = std::process::Command::new("git")
                .arg("-C")
                .arg(&repo_path)
                .args(["pack-objects", "--stdout"])
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .spawn()
                .unwrap();
            child
                .stdin
                .take()
                .unwrap()
                .write_all(format!("{tip}\n").as_bytes())
                .unwrap();
            let out = child.wait_with_output().unwrap();
            assert!(out.status.success(), "pack-objects failed");
            out.stdout
        };
        body.extend_from_slice(&pack);

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/git/{ws_slug}/{repo_name}/git-receive-pack"))
                    .header("Authorization", "Bearer test-token")
                    .header("content-type", "application/x-git-receive-pack-request")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
        // The ref must have been rolled back (existing undo behavior).
        assert_eq!(git(&["rev-parse", "refs/heads/main"]), base);

        // The rejection is synchronous — the audit event is recorded before
        // the response is returned, but query anyway for uniformity.
        let events = wait_for_audit_event(&state, "git_push_rejected").await;
        assert_eq!(events.len(), 1);
        let e = &events[0];
        assert_eq!(e.detail["repo_id"], "repo-1");
        assert_eq!(e.detail["branch"], "main");
        assert_eq!(e.detail["gate_name"], "conventional-commit");
        assert!(
            e.detail["reason"]
                .as_str()
                .unwrap()
                .contains("conventional"),
            "rejection reason should name the violation: {}",
            e.detail["reason"]
        );
        assert_eq!(e.outcome, gyre_domain::AuditOutcome::Blocked);
        assert_eq!(e.agent_id.as_ref().unwrap().as_str(), "system");
        // No accepted event may exist for the rejected push.
        let accepted = audit_events_of(&state, "git_push_accepted").await;
        assert!(accepted.is_empty());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn push_spec_change_records_spec_changed_audit_event() {
        let (app, state, _tmp, ws_slug, repo_name, repo_path) = git_app_with_repo().await;

        // Build a commit whose tree adds specs/system/new-spec.md.
        let blob = git_with_input(&repo_path, &["hash-object", "-w", "--stdin"], "# spec");
        let tree_system = git_with_input(
            &repo_path,
            &["mktree"],
            &format!("100644 blob {blob}\tnew-spec.md"),
        );
        let tree_specs = git_with_input(
            &repo_path,
            &["mktree"],
            &format!("040000 tree {tree_system}\tsystem"),
        );
        let tree = git_with_input(
            &repo_path,
            &["mktree"],
            &format!("040000 tree {tree_specs}\tspecs"),
        );
        let sha = git_with_input(
            &repo_path,
            &["commit-tree", &tree, "-m", "docs: add spec"],
            "",
        );
        git_with_input(&repo_path, &["update-ref", "refs/heads/main", &sha], "");

        // Push creating refs/heads/main (old SHA = zeros).
        let zeros = "0000000000000000000000000000000000000000".to_string();
        let line = format!("{zeros} {sha} refs/heads/main");
        let mut body = super::pkt_line(&line);
        body.extend_from_slice(b"0000");
        let pack = {
            use std::io::Write as _;
            let mut child = std::process::Command::new("git")
                .arg("-C")
                .arg(&repo_path)
                .args(["pack-objects", "--stdout"])
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .spawn()
                .unwrap();
            child
                .stdin
                .take()
                .unwrap()
                .write_all(format!("{sha}\n").as_bytes())
                .unwrap();
            let out = child.wait_with_output().unwrap();
            assert!(out.status.success(), "pack-objects failed");
            out.stdout
        };
        body.extend_from_slice(&pack);

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/git/{ws_slug}/{repo_name}/git-receive-pack"))
                    .header("Authorization", "Bearer test-token")
                    .header("content-type", "application/x-git-receive-pack-request")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let events = wait_for_audit_event(&state, "spec_changed").await;
        assert_eq!(events.len(), 1);
        let e = &events[0];
        assert_eq!(e.detail["repo_id"], "repo-1");
        assert_eq!(e.detail["spec_path"], "specs/system/new-spec.md");
        assert_eq!(e.detail["change_kind"], "added");
        assert!(
            e.detail["task_id"].as_str().is_some(),
            "spec change must reference the auto-created task"
        );
        assert_eq!(e.repo_id.as_ref().unwrap().as_str(), "repo-1");
        assert_eq!(e.resource_type, "spec");
        assert_eq!(e.resource_id.as_deref(), Some("specs/system/new-spec.md"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn clone_records_git_clone_audit_event() {
        let (app, state, _tmp, ws_slug, repo_name, repo_path) = git_app_with_repo().await;

        // Seed the bare repo with a commit so the clone actually negotiates
        // and fetches objects (an empty-repo clone skips the upload-pack
        // POST entirely — nothing to fetch).
        {
            let git = |args: &[&str]| -> String {
                let out = std::process::Command::new("git")
                    .arg("-C")
                    .arg(&repo_path)
                    .args(args)
                    .env("GIT_AUTHOR_NAME", "t")
                    .env("GIT_AUTHOR_EMAIL", "t@t")
                    .env("GIT_COMMITTER_NAME", "t")
                    .env("GIT_COMMITTER_EMAIL", "t@t")
                    .output()
                    .unwrap();
                assert!(out.status.success(), "git {:?} failed", args);
                String::from_utf8_lossy(&out.stdout).trim().to_string()
            };
            let tree = git(&["mktree"]);
            let sha = git(&["commit-tree", &tree, "-m", "chore: init"]);
            git(&["update-ref", "refs/heads/main", &sha]);
        }

        // Serve on a real listener so a real `git clone` runs through the
        // smart-HTTP handlers (the sandbox permits local TCP listeners).
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let clone_dir = TempDir::new().unwrap();
        let url = format!("http://127.0.0.1:{port}/git/{ws_slug}/{repo_name}");
        let clone_target = clone_dir.path().join("cloned");
        let result = tokio::task::spawn_blocking(move || {
            std::process::Command::new("git")
                .arg("clone")
                .arg(&url)
                .arg(&clone_target)
                .env("GIT_TERMINAL_PROMPT", "0")
                .env("GIT_ASKPASS", "true")
                .env("GIT_CONFIG_COUNT", "1")
                .env("GIT_CONFIG_KEY_0", "http.extraHeader")
                .env("GIT_CONFIG_VALUE_0", "Authorization: Bearer test-token")
                .output()
        })
        .await
        .unwrap()
        .unwrap();
        let stderr = String::from_utf8_lossy(&result.stderr);
        assert!(
            result.status.success(),
            "git clone failed (exit={:?}): {stderr}",
            result.status.code()
        );

        let events = wait_for_audit_event(&state, "git_clone").await;
        assert_eq!(events.len(), 1, "one git_clone event per upload-pack");
        let e = &events[0];
        assert_eq!(e.detail["repo_id"], "repo-1");
        assert_eq!(e.detail["agent_id"], "system");
        assert_eq!(e.agent_id.as_ref().unwrap().as_str(), "system");
        assert_eq!(e.repo_id.as_ref().unwrap().as_str(), "repo-1");
        assert_eq!(e.workspace_id.as_ref().unwrap().as_str(), "ws-test");
        assert_eq!(e.outcome, gyre_domain::AuditOutcome::Success);
        assert_eq!(e.resource_type, "repo");
    }

    // ── Repo-defined persona push detection (platform-model.md §2) ──────────

    /// SHA-256 of `prompt` + empty capabilities joined — the persona
    /// content-hash contract for a persona with no capabilities
    /// (domain `Persona::hash_content`).
    fn persona_content_hash(prompt: &str) -> String {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(prompt.as_bytes());
        format!("{:x}", hasher.finalize())
    }

    /// Run git in the bare repo, assert success, return trimmed stdout.
    fn git_bare(repo_path: &str, args: &[&str]) -> String {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(repo_path)
            .args(args)
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@t")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@t")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    /// Write a blob with the given content; returns its SHA.
    fn write_blob(repo_path: &str, content: &str) -> String {
        use std::io::Write as _;
        let mut child = std::process::Command::new("git")
            .arg("-C")
            .arg(repo_path)
            .args(["hash-object", "-w", "--stdin"])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(content.as_bytes())
            .unwrap();
        let out = child.wait_with_output().unwrap();
        assert!(out.status.success(), "hash-object failed");
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    /// Build a tree object from (path, blob-sha) entries, creating
    /// intermediate trees for nested paths (`mktree` is single-level).
    fn write_tree(repo_path: &str, files: &[(&str, &str)]) -> String {
        let mut lines: Vec<String> = Vec::new();
        let mut subdirs: std::collections::BTreeMap<&str, Vec<(&str, &str)>> =
            std::collections::BTreeMap::new();
        for (path, sha) in files {
            match path.split_once('/') {
                None => lines.push(format!("100644 blob {sha}\t{path}")),
                Some((seg, rest)) => subdirs.entry(seg).or_default().push((rest, sha)),
            }
        }
        for (seg, entries) in subdirs {
            let subtree = write_tree(repo_path, &entries);
            lines.push(format!("040000 tree {subtree}\t{seg}"));
        }
        lines.sort();
        let input = lines.join("\n");
        use std::io::Write as _;
        let mut child = std::process::Command::new("git")
            .arg("-C")
            .arg(repo_path)
            .args(["mktree"])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
        let out = child.wait_with_output().unwrap();
        assert!(out.status.success(), "mktree failed");
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    /// Create a commit with the given (path, content) files on top of
    /// `parent` (or a root commit when None) WITHOUT updating any ref.
    fn commit_files(
        repo_path: &str,
        parent: Option<&str>,
        msg: &str,
        files: &[(&str, &str)],
    ) -> String {
        let blob_shas: Vec<String> = files
            .iter()
            .map(|(_, c)| write_blob(repo_path, c))
            .collect();
        let entries: Vec<(&str, &str)> = files
            .iter()
            .zip(&blob_shas)
            .map(|((p, _), s)| (*p, s.as_str()))
            .collect();
        let tree = write_tree(repo_path, &entries);
        match parent {
            Some(p) => git_bare(repo_path, &["commit-tree", &tree, "-p", p, "-m", msg]),
            None => git_bare(repo_path, &["commit-tree", &tree, "-m", msg]),
        }
    }

    /// Push `new` onto `refname` (expected current value `old`) through the
    /// real receive-pack HTTP handler. Asserts the push is accepted and the
    /// ref actually moved — a push that did not move the ref would make the
    /// post-receive assertions pass vacuously.
    async fn push_ref(
        app: &Router,
        ws_slug: &str,
        repo_name: &str,
        repo_path: &str,
        old: &str,
        new: &str,
        refname: &str,
    ) {
        let line = format!("{old} {new} {refname}");
        let mut body = super::pkt_line(&line);
        body.extend_from_slice(b"0000");
        let pack = {
            let mut child = std::process::Command::new("git")
                .arg("-C")
                .arg(repo_path)
                .args(["pack-objects", "--stdout"])
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .spawn()
                .unwrap();
            use std::io::Write as _;
            child
                .stdin
                .take()
                .unwrap()
                .write_all(format!("{new}\n").as_bytes())
                .unwrap();
            let out = child.wait_with_output().unwrap();
            assert!(out.status.success(), "pack-objects failed");
            out.stdout
        };
        assert!(!pack.is_empty(), "pack-objects produced empty pack");
        body.extend_from_slice(&pack);

        let uri = format!("/git/{ws_slug}/{repo_name}/git-receive-pack");
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(uri)
                    .header("Authorization", auth_header())
                    .header("content-type", "application/x-git-receive-pack-request")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            git_bare(repo_path, &["rev-parse", refname]),
            new,
            "receive-pack did not move {refname}"
        );
    }

    /// Poll `state.tasks.list()` until a task with the given title exists
    /// (bounded) — the post-receive pipeline runs in a spawned task.
    async fn wait_for_task_title(state: &Arc<crate::AppState>, title: &str) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            let tasks = state.tasks.list().await.unwrap();
            if tasks.iter().any(|t| t.title == title) {
                return;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "task {title:?} was never created"
            );
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    }

    /// Poll until the user has a notification of the given kind. Persona
    /// owner notifications are written by a post-receive stage that runs
    /// AFTER the approval task is created, so the notification can lag the
    /// task — checking it exactly once races the background job.
    async fn wait_for_notification(
        state: &Arc<crate::AppState>,
        user_id: &str,
        kind: gyre_common::NotificationType,
    ) -> gyre_common::Notification {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            let notifs = state
                .notifications
                .list_for_user(&Id::new(user_id), None, None, None, None, 100, 0)
                .await
                .unwrap();
            if let Some(n) = notifs.into_iter().find(|n| n.notification_type == kind) {
                return n;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "notification of kind {kind:?} for user {user_id:?} was never created"
            );
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    }

    #[test]
    fn parse_persona_changes_added_modified_deleted() {
        let input =
            "A\tspecs/personas/new.md\nM\tspecs/personas/security.md\nD\tspecs/personas/old.md\n";
        let changes = super::parse_persona_changes(input);
        assert_eq!(changes.len(), 3);
        assert_eq!(changes[0], ('A', "specs/personas/new.md".to_string(), None));
        assert_eq!(
            changes[1],
            ('M', "specs/personas/security.md".to_string(), None)
        );
        assert_eq!(changes[2], ('D', "specs/personas/old.md".to_string(), None));
    }

    #[test]
    fn parse_persona_changes_renamed() {
        let input = "R090\tspecs/personas/old.md\tspecs/personas/new.md\n";
        let changes = super::parse_persona_changes(input);
        assert_eq!(changes.len(), 1);
        assert_eq!(
            changes[0],
            (
                'R',
                "specs/personas/new.md".to_string(),
                Some("specs/personas/old.md".to_string())
            )
        );
    }

    #[test]
    fn parse_persona_changes_ignores_non_persona_paths() {
        // Spec-lifecycle paths, source files, nested persona paths, and
        // non-markdown files in the persona dir are all ignored.
        let input = "M\tspecs/system/architecture.md\nM\tsrc/main.rs\n\
                     A\tspecs/personas/nested/deep.md\nA\tspecs/personas/notes.txt\n\
                     M\tspecs/README.md\n";
        let changes = super::parse_persona_changes(input);
        assert!(changes.is_empty(), "got: {changes:?}");
    }

    #[test]
    fn parse_persona_changes_rename_into_persona_dir() {
        // A rename whose OLD side is a persona file is still a persona change.
        let input = "R100\tspecs/personas/gone.md\tdocs/gone.md\n";
        let changes = super::parse_persona_changes(input);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].0, 'R');
        assert_eq!(changes[0].1, "docs/gone.md");
        assert_eq!(changes[0].2.as_deref(), Some("specs/personas/gone.md"));
    }

    #[test]
    fn persona_slug_from_path_direct_files_only() {
        assert_eq!(
            super::persona_slug_from_path("specs/personas/security.md"),
            Some("security".to_string())
        );
        assert_eq!(
            super::persona_slug_from_path("specs/personas/domain-reviewer.md"),
            Some("domain-reviewer".to_string())
        );
        assert_eq!(
            super::persona_slug_from_path("specs/personas/nested/deep.md"),
            None
        );
        assert_eq!(super::persona_slug_from_path("specs/system/x.md"), None);
        assert_eq!(super::persona_slug_from_path("specs/personas/.md"), None);
    }

    #[test]
    fn persona_approval_task_shape() {
        let task = super::persona_approval_task(
            "security",
            "specs/personas/security.md",
            &Id::new("repo-1"),
            &Id::new("ws-test"),
            "modified",
            4242,
        );
        assert_eq!(
            task.title,
            "Persona change requires human approval: security"
        );
        assert_eq!(
            task.labels,
            vec!["persona-change".to_string(), "auto-created".to_string()]
        );
        assert_eq!(task.priority, gyre_domain::TaskPriority::High);
        assert_eq!(task.repo_id, Id::new("repo-1"));
        assert_eq!(task.workspace_id, Id::new("ws-test"));
        assert_eq!(
            task.spec_path.as_deref(),
            Some("specs/personas/security.md")
        );
        // Push-hook tasks must never trigger agent spawning.
        assert!(task.task_type.is_none());
        assert_eq!(task.created_at, 4242);
    }

    /// End-to-end: pushing a modification of `specs/personas/security.md`
    /// through the real receive-pack handler updates the repo-scoped persona
    /// entity (content + content_hash + version), resets its approval to
    /// Pending, clears approved_by/approved_at, auto-creates the approval
    /// task, and notifies the owner (platform-model.md §2).
    #[tokio::test(flavor = "multi_thread")]
    async fn push_persona_change_updates_entity_resets_approval_and_notifies_owner() {
        let (app, state, _tmp, ws_slug, repo_name, repo_path) = git_app_with_repo().await;

        // The persona owner must be a real user — the hook notifies owners
        // through the inbox.
        let owner = gyre_domain::User::new(Id::new("user-1"), "ext-user-1", "user-1", 0);
        state.users.create(&owner).await.unwrap();

        // Seed an APPROVED repo-scoped persona whose prompt matches the git
        // state before the push.
        let mut persona = gyre_domain::Persona::new(
            Id::new("persona-security"),
            "Security Agent",
            "security",
            gyre_domain::PersonaScope::Repo(Id::new("repo-1")),
            "old prompt",
            1000,
        );
        persona.owner = Some("user-1".to_string());
        persona.approval_status = gyre_domain::PersonaApprovalStatus::Approved;
        persona.approved_by = Some("user-1".to_string());
        persona.approved_at = Some(1234);
        persona.updated_at = 1234;
        state.personas.create(&persona).await.unwrap();
        let hash_before = persona.content_hash.clone();

        // git state: main holds specs/personas/security.md with the old
        // content (authored with plumbing — no push, so no hook runs yet).
        let c0 = commit_files(
            &repo_path,
            None,
            "base",
            &[("specs/personas/security.md", "old prompt")],
        );
        git_bare(&repo_path, &["update-ref", "refs/heads/main", &c0]);

        // Push a commit that modifies the persona file.
        let c1 = commit_files(
            &repo_path,
            Some(&c0),
            "change persona",
            &[
                ("specs/personas/security.md", "new prompt v1"),
                ("src/main.rs", "fn main() {}"),
            ],
        );
        push_ref(
            &app,
            &ws_slug,
            &repo_name,
            &repo_path,
            &c0,
            &c1,
            "refs/heads/main",
        )
        .await;

        // Post-receive processing runs in a spawned task — poll until the
        // persona entity reflects the push.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        let updated = loop {
            let p = state
                .personas
                .find_by_slug_and_scope(
                    "security",
                    &gyre_domain::PersonaScope::Repo(Id::new("repo-1")),
                )
                .await
                .unwrap()
                .expect("persona must exist");
            if p.system_prompt == "new prompt v1" {
                break p;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "persona was never updated; still: {:?}",
                p.system_prompt
            );
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        };

        // Content, hash, and version updated.
        assert_eq!(updated.system_prompt, "new prompt v1");
        assert_eq!(updated.content_hash, persona_content_hash("new prompt v1"));
        assert_ne!(updated.content_hash, hash_before);
        assert_eq!(
            updated.version, 2,
            "version must increment on content change"
        );
        // Approval reset.
        assert_eq!(
            updated.approval_status,
            gyre_domain::PersonaApprovalStatus::Pending
        );
        assert_eq!(updated.approved_by, None);
        assert_eq!(updated.approved_at, None);
        assert!(updated.updated_at >= 1234);
        // Scope and identity preserved.
        assert_eq!(updated.id, Id::new("persona-security"));
        assert_eq!(
            updated.scope,
            gyre_domain::PersonaScope::Repo(Id::new("repo-1"))
        );
        assert_eq!(updated.owner.as_deref(), Some("user-1"));

        // Approval task auto-created with the contract title/labels/priority.
        let title = "Persona change requires human approval: security";
        wait_for_task_title(&state, title).await;
        let tasks = state.tasks.list().await.unwrap();
        let task = tasks
            .iter()
            .find(|t| t.title == title)
            .expect("approval task must exist");
        assert_eq!(
            task.labels,
            vec!["persona-change".to_string(), "auto-created".to_string()]
        );
        assert_eq!(task.priority, gyre_domain::TaskPriority::High);
        assert_eq!(task.repo_id, Id::new("repo-1"));
        assert_eq!(task.workspace_id, Id::new("ws-test"));
        assert_eq!(
            task.spec_path.as_deref(),
            Some("specs/personas/security.md")
        );
        // Exactly one approval task for this persona (no duplicates).
        assert_eq!(tasks.iter().filter(|t| t.title == title).count(), 1);

        // The owner was notified (polled: the notification is written after
        // the approval task by the same background job).
        let notif = wait_for_notification(
            &state,
            "user-1",
            gyre_common::NotificationType::PersonaChangePendingApproval,
        )
        .await;
        assert_eq!(notif.title, title);
        assert_eq!(notif.source.entity_type, "persona");
        assert_eq!(notif.source.entity_id, "security");
        assert_eq!(notif.source.repo_id, Some(Id::new("repo-1")));
        assert_eq!(notif.workspace_id, Id::new("ws-test"));
        assert!(notif.body.as_deref().unwrap().contains("security"));
    }

    /// End-to-end: pushing a NEW persona file registers a fresh
    /// repo-scoped persona entity with Pending approval (no prior entity,
    /// no owner yet → task but no notification).
    #[tokio::test(flavor = "multi_thread")]
    async fn push_new_persona_file_registers_pending_repo_scoped_entity() {
        let (app, state, _tmp, ws_slug, repo_name, repo_path) = git_app_with_repo().await;

        let c0 = commit_files(&repo_path, None, "base", &[("src/main.rs", "fn main() {}")]);
        git_bare(&repo_path, &["update-ref", "refs/heads/main", &c0]);

        let c1 = commit_files(
            &repo_path,
            Some(&c0),
            "add persona",
            &[
                ("src/main.rs", "fn main() {}"),
                ("specs/personas/domain-reviewer.md", "review prompt"),
            ],
        );
        push_ref(
            &app,
            &ws_slug,
            &repo_name,
            &repo_path,
            &c0,
            &c1,
            "refs/heads/main",
        )
        .await;

        // Poll until the new entity is registered.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        let persona = loop {
            let p = state
                .personas
                .find_by_slug_and_scope(
                    "domain-reviewer",
                    &gyre_domain::PersonaScope::Repo(Id::new("repo-1")),
                )
                .await
                .unwrap();
            if let Some(p) = p {
                break p;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "new persona entity was never registered"
            );
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        };

        assert_eq!(
            persona.scope,
            gyre_domain::PersonaScope::Repo(Id::new("repo-1"))
        );
        assert_eq!(persona.system_prompt, "review prompt");
        assert_eq!(persona.content_hash, persona_content_hash("review prompt"));
        assert_eq!(persona.version, 1);
        assert_eq!(
            persona.approval_status,
            gyre_domain::PersonaApprovalStatus::Pending
        );
        assert_eq!(persona.approved_by, None);
        assert_eq!(persona.approved_at, None);
        assert_eq!(persona.owner, None);

        // Approval task auto-created.
        wait_for_task_title(
            &state,
            "Persona change requires human approval: domain-reviewer",
        )
        .await;

        // No owner on a newly discovered persona → no notification for anyone.
        let recent = state.notifications.list_recent(100).await.unwrap();
        assert!(
            recent.iter().all(|n| n.notification_type
                != gyre_common::NotificationType::PersonaChangePendingApproval),
            "no owner notification expected for a persona without an owner"
        );
    }

    /// End-to-end: a push with no persona file changes must not touch persona
    /// entities, must not create persona approval tasks, and must not notify.
    /// The push includes a `specs/system/` change so spec-lifecycle processing
    /// provably runs (positive control); the assertion then verifies persona
    /// state is byte-identical after the full pipeline (graph-extraction delta
    /// for the pushed commit is the pipeline's last write).
    #[tokio::test(flavor = "multi_thread")]
    async fn push_without_persona_changes_leaves_personas_untouched() {
        let (app, state, _tmp, ws_slug, repo_name, repo_path) = git_app_with_repo().await;

        let owner = gyre_domain::User::new(Id::new("user-1"), "ext-user-1", "user-1", 0);
        state.users.create(&owner).await.unwrap();

        let mut persona = gyre_domain::Persona::new(
            Id::new("persona-untouched"),
            "Security Agent",
            "security",
            gyre_domain::PersonaScope::Repo(Id::new("repo-1")),
            "stable prompt",
            1000,
        );
        persona.owner = Some("user-1".to_string());
        persona.approval_status = gyre_domain::PersonaApprovalStatus::Approved;
        persona.approved_by = Some("user-1".to_string());
        persona.approved_at = Some(1234);
        state.personas.create(&persona).await.unwrap();
        let snapshot = persona.clone();

        let c0 = commit_files(&repo_path, None, "base", &[("src/main.rs", "fn main() {}")]);
        git_bare(&repo_path, &["update-ref", "refs/heads/main", &c0]);

        // Non-persona changes only: a Rust package (a Cargo.toml makes the
        // knowledge-graph extractor produce nodes — its delta record is the
        // pipeline's last write and our completion marker), a source file,
        // and a specs/system/ file (the latter proves the post-receive
        // pipeline ran past spec lifecycle — persona processing is the
        // immediately following stage).
        let cargo_toml =
            "[package]\nname = \"test-proj\"\nversion = \"0.1.0\"\nedition = \"2021\"\n";
        let c1 = commit_files(
            &repo_path,
            Some(&c0),
            "no persona changes",
            &[
                ("Cargo.toml", cargo_toml),
                ("src/main.rs", "fn main() { /* v2 */ }"),
                ("specs/system/other.md", "# other\n"),
            ],
        );
        push_ref(
            &app,
            &ws_slug,
            &repo_name,
            &repo_path,
            &c0,
            &c1,
            "refs/heads/main",
        )
        .await;

        // Positive control 1: spec-lifecycle task for the specs/system/ file.
        wait_for_task_title(&state, "Implement spec: specs/system/other.md").await;
        // Positive control 2: the pipeline's final stage (knowledge-graph
        // extraction) records a delta for the pushed commit.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            let deltas = state
                .graph_store
                .list_deltas(&Id::new("repo-1"), None, None)
                .await
                .unwrap();
            if deltas.iter().any(|d| d.commit_sha == c1) {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "post-receive pipeline never completed (no graph delta for {c1})"
            );
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }

        // Persona state is byte-identical to the pre-push snapshot.
        let after = state
            .personas
            .find_by_slug_and_scope(
                "security",
                &gyre_domain::PersonaScope::Repo(Id::new("repo-1")),
            )
            .await
            .unwrap()
            .expect("persona must still exist");
        assert_eq!(after.system_prompt, snapshot.system_prompt);
        assert_eq!(after.content_hash, snapshot.content_hash);
        assert_eq!(after.version, snapshot.version);
        assert_eq!(
            after.approval_status,
            gyre_domain::PersonaApprovalStatus::Approved,
            "approval must NOT reset when no persona file changed"
        );
        assert_eq!(after.approved_by, snapshot.approved_by);
        assert_eq!(after.approved_at, snapshot.approved_at);
        assert_eq!(after.owner, snapshot.owner);

        // No persona approval tasks, no persona notifications.
        let tasks = state.tasks.list().await.unwrap();
        assert!(
            tasks.iter().all(|t| !t
                .title
                .starts_with("Persona change requires human approval")),
            "no persona approval task expected: {tasks:?}"
        );
        let recent = state.notifications.list_recent(100).await.unwrap();
        assert!(recent
            .iter()
            .all(|n| n.notification_type
                != gyre_common::NotificationType::PersonaChangePendingApproval));
    }
}
