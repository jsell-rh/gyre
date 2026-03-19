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

/// Resolve `:project` + `:repo` URL segments to a filesystem path.
///
/// * `project`  — the project_id (UUID string) used when the repo was created.
/// * `repo_seg` — the repo segment from the URL, e.g. `my-repo.git`.
async fn resolve_repo_path(
    state: &Arc<AppState>,
    project: &str,
    repo_seg: &str,
) -> Result<String, Response> {
    let repo_name = repo_seg.strip_suffix(".git").unwrap_or(repo_seg);

    let repos = state
        .repos
        .list_by_project(&Id::new(project))
        .await
        .map_err(|e| git_err(format!("db error: {e}")))?;

    let repo = repos
        .into_iter()
        .find(|r| r.name == repo_name)
        .ok_or_else(|| {
            not_found(format!(
                "repo '{repo_name}' not found in project '{project}'"
            ))
        })?;

    Ok(repo.path)
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
        let stderr = String::from_utf8_lossy(&output.stderr);
        error!(%repo_path, %stderr, "git advertise-refs failed");
        return git_err(format!("git {subcommand} failed: {stderr}"));
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
    let repo_path = match resolve_repo_path(&state, &project, &repo).await {
        Ok(p) => p,
        Err(r) => return r,
    };

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

    info!(%repo_path, updates = ref_updates.len(), "served git-receive-pack");

    // Post-receive: record agent-commit mappings for newly pushed commits.
    let state_clone = state.clone();
    let repo_path_clone = repo_path.clone();
    let agent_id = auth.agent_id.clone();
    tokio::spawn(async move {
        record_pushed_commits(&state_clone, &repo_path_clone, &ref_updates, &agent_id).await;
    });

    Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "application/x-git-receive-pack-result")
        .body(Body::from(output))
        .unwrap()
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
        let stderr = String::from_utf8_lossy(&out.stderr);
        error!(subcommand, %repo_path, %stderr, "git command failed");
        return Err(format!("git {subcommand} failed: {stderr}"));
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

async fn record_pushed_commits(
    state: &Arc<AppState>,
    repo_path: &str,
    updates: &[RefUpdate],
    agent_id: &str,
) {
    if updates.is_empty() {
        return;
    }

    // Find the repo record by path.
    let all_repos = match state.repos.list().await {
        Ok(r) => r,
        Err(e) => {
            warn!("post-receive: failed to list repos: {e}");
            return;
        }
    };
    let repo = match all_repos.iter().find(|r| r.path == repo_path) {
        Some(r) => r.clone(),
        None => {
            warn!(%repo_path, "post-receive: repo not found");
            return;
        }
    };

    let git_bin = std::env::var("GYRE_GIT_PATH").unwrap_or_else(|_| "git".to_string());

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
            );
            if let Err(e) = state.agent_commits.record(&mapping).await {
                warn!(%sha, "post-receive: failed to record commit: {e}");
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
}
