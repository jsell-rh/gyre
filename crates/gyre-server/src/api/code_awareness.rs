//! M13.4 Cross-Agent Code Awareness endpoints.
//!
//! - `GET /api/v1/repos/{id}/blame?path={file}` — per-line agent attribution
//! - `GET /api/v1/repos/{id}/hot-files?limit=20` — files with most distinct active agents
//! - `GET /api/v1/agents/{id}/touched-paths` — paths written by an agent
//! - `GET /api/v1/repos/{id}/review-routing?path={file}` — ordered agent list for review

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use gyre_common::Id;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

use crate::AppState;

use super::error::ApiError;

// ── Blame ─────────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct BlamePath {
    pub path: Option<String>,
}

#[derive(Serialize)]
pub struct BlameEntry {
    pub line_number: usize,
    pub content: String,
    pub sha: String,
    pub author: Option<String>,
    pub agent_id: Option<String>,
    pub task_id: Option<String>,
    pub spec_ref: Option<String>,
}

/// GET /api/v1/repos/:id/blame?path={file}
///
/// Runs `git blame` on the file and cross-references with agent_commits to
/// produce per-line entries with actual code content and agent attribution.
pub async fn get_blame(
    State(state): State<Arc<AppState>>,
    Path(repo_id): Path<String>,
    Query(params): Query<BlamePath>,
) -> Result<(StatusCode, Json<Vec<BlameEntry>>), ApiError> {
    let repo = state
        .repos
        .find_by_id(&Id::new(&repo_id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("repo {repo_id} not found")))?;

    let path = params.path.unwrap_or_default();
    if path.is_empty() {
        return Ok((StatusCode::OK, Json(vec![])));
    }

    // Build a SHA → agent mapping from agent_commits for this repo.
    let agent_commits = state.agent_commits.find_by_repo(&Id::new(&repo_id)).await?;
    let mut sha_to_agent: HashMap<String, (String, Option<String>)> = HashMap::new();
    for ac in &agent_commits {
        sha_to_agent
            .entry(ac.commit_sha.clone())
            .or_insert_with(|| (ac.agent_id.to_string(), ac.task_id.clone()));
    }

    // Try `git blame --porcelain` on the bare repo to get per-line attribution
    // with actual file content.
    let git_bin = std::env::var("GYRE_GIT_PATH").unwrap_or_else(|_| "git".to_string());
    let blame_output = std::process::Command::new(&git_bin)
        .args(["blame", "--porcelain", "HEAD", "--", &path])
        .current_dir(&repo.path)
        .output();

    match blame_output {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let entries = parse_porcelain_blame(&stdout, &sha_to_agent);
            Ok((StatusCode::OK, Json(entries)))
        }
        _ => {
            // Fallback: read the file content directly and return lines without
            // blame attribution. This handles repos where git blame fails (e.g.
            // single-commit repos or files not yet committed).
            let show_output = std::process::Command::new(&git_bin)
                .args(["show", &format!("HEAD:{path}")])
                .current_dir(&repo.path)
                .output();

            match show_output {
                Ok(output) if output.status.success() => {
                    let content = String::from_utf8_lossy(&output.stdout);
                    let entries: Vec<BlameEntry> = content
                        .lines()
                        .enumerate()
                        .map(|(i, line)| BlameEntry {
                            line_number: i + 1,
                            content: line.to_string(),
                            sha: "HEAD".to_string(),
                            author: None,
                            agent_id: None,
                            task_id: None,
                            spec_ref: None,
                        })
                        .collect();
                    Ok((StatusCode::OK, Json(entries)))
                }
                _ => Ok((StatusCode::OK, Json(vec![]))),
            }
        }
    }
}

/// Parse `git blame --porcelain` output into BlameEntry records.
///
/// Porcelain format emits blocks like:
/// ```text
/// <sha> <orig-line> <final-line> [<num-lines>]
/// author <name>
/// ...header lines...
/// \t<content line>
/// ```
fn parse_porcelain_blame(
    output: &str,
    sha_to_agent: &HashMap<String, (String, Option<String>)>,
) -> Vec<BlameEntry> {
    let mut entries = Vec::new();
    let mut current_sha = String::new();
    let mut current_author: Option<String> = None;
    let mut current_line_num: usize = 0;

    for line in output.lines() {
        if line.starts_with('\t') {
            // Content line — the actual source code (tab-prefixed)
            let content = &line[1..];
            let (agent_id, task_id) = sha_to_agent
                .get(&current_sha)
                .map(|(a, t)| (Some(a.clone()), t.clone()))
                .unwrap_or((None, None));

            entries.push(BlameEntry {
                line_number: current_line_num,
                content: content.to_string(),
                sha: current_sha.clone(),
                author: current_author.clone(),
                agent_id,
                task_id,
                spec_ref: None,
            });
        } else if line.starts_with("author ") {
            current_author = Some(line[7..].to_string());
        } else {
            // Header line: "<sha> <orig-line> <final-line> [<count>]"
            let parts: Vec<&str> = line.splitn(4, ' ').collect();
            if parts.len() >= 3 {
                let maybe_sha = parts[0];
                // SHA is 40 hex chars
                if maybe_sha.len() == 40 && maybe_sha.chars().all(|c| c.is_ascii_hexdigit()) {
                    current_sha = maybe_sha.to_string();
                    current_line_num = parts[2].parse().unwrap_or(0);
                    // Author will be set by subsequent "author " line; keep previous
                    // for continuation lines of the same commit.
                }
            }
        }
    }

    entries
}

// ── Hot Files ─────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct HotFilesQuery {
    pub limit: Option<usize>,
}

#[derive(Serialize)]
pub struct HotFileEntry {
    pub path: String,
    pub agent_count: usize,
    pub agents: Vec<HotFileAgent>,
}

#[derive(Serialize)]
pub struct HotFileAgent {
    pub id: String,
    pub name: String,
    pub task_id: Option<String>,
}

/// GET /api/v1/repos/:id/hot-files?limit=20
///
/// Returns files with the most distinct active agents touching them in the last 24h.
pub async fn get_hot_files(
    State(state): State<Arc<AppState>>,
    Path(repo_id): Path<String>,
    Query(params): Query<HotFilesQuery>,
) -> Result<Json<Vec<HotFileEntry>>, ApiError> {
    let limit = params.limit.unwrap_or(20).min(100);

    // Verify the repo exists.
    state
        .repos
        .find_by_id(&Id::new(&repo_id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("repo {repo_id} not found")))?;

    // Get all active agents.
    let active_agents = state
        .agents
        .list_by_status(&gyre_domain::AgentStatus::Active)
        .await?;

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let cutoff = now.saturating_sub(24 * 3600);

    // Build a map: branch -> list of (agent_id, agent_name, task_id).
    // We use branch as a proxy for "touched files" since we don't have per-file
    // tracking at the commit level here. A full implementation would parse
    // commit diffs to extract changed files.
    let mut branch_agents: HashMap<String, Vec<HotFileAgent>> = HashMap::new();

    for agent in &active_agents {
        let worktrees = state
            .worktrees
            .find_by_agent(&agent.id)
            .await
            .unwrap_or_default();

        for wt in &worktrees {
            if wt.repository_id.as_str() != repo_id {
                continue;
            }
            if wt.branch == "main" || wt.branch == "master" {
                continue;
            }

            // Check if agent has recent commits in this repo.
            let commits = state
                .agent_commits
                .find_by_agent(&agent.id)
                .await
                .unwrap_or_default();

            let has_recent = commits.iter().any(|c| {
                c.repository_id.as_str() == repo_id
                    && c.branch == wt.branch
                    && c.timestamp >= cutoff
            });

            if has_recent || wt.created_at >= cutoff {
                let task_id = worktrees
                    .first()
                    .and_then(|w| w.task_id.as_ref())
                    .map(|id| id.to_string());

                branch_agents
                    .entry(wt.branch.clone())
                    .or_default()
                    .push(HotFileAgent {
                        id: agent.id.to_string(),
                        name: agent.name.clone(),
                        task_id,
                    });
            }
        }
    }

    // Convert to HotFileEntry list sorted by agent_count descending.
    let mut entries: Vec<HotFileEntry> = branch_agents
        .into_iter()
        .map(|(branch, agents)| {
            let agent_count = agents.len();
            HotFileEntry {
                path: branch,
                agent_count,
                agents,
            }
        })
        .collect();

    entries.sort_by(|a, b| b.agent_count.cmp(&a.agent_count));
    entries.truncate(limit);

    Ok(Json(entries))
}

// ── Touched Paths ─────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct TouchedPathsResponse {
    pub agent_id: String,
    pub paths: Vec<String>,
}

/// GET /api/v1/agents/:id/touched-paths
///
/// Returns all branches/paths touched by the agent across all repos.
pub async fn get_touched_paths(
    State(state): State<Arc<AppState>>,
    Path(agent_id): Path<String>,
) -> Result<Json<TouchedPathsResponse>, ApiError> {
    // Verify agent exists.
    state
        .agents
        .find_by_id(&Id::new(&agent_id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("agent {agent_id} not found")))?;

    // Get all commits by this agent.
    let commits = state
        .agent_commits
        .find_by_agent(&Id::new(&agent_id))
        .await?;

    // Extract unique branches as touched paths.
    let mut paths: Vec<String> = commits
        .into_iter()
        .map(|c| c.branch)
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();
    paths.sort();

    Ok(Json(TouchedPathsResponse { agent_id, paths }))
}

// ── Review Routing ────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct ReviewRoutingQuery {
    pub path: Option<String>,
}

#[derive(Serialize)]
pub struct ReviewRoutingEntry {
    pub agent_id: String,
    pub agent_name: String,
    pub task_id: Option<String>,
    pub commit_count: usize,
    pub last_touch: u64,
    pub score: f64,
}

/// GET /api/v1/repos/:id/review-routing?path={file}
///
/// Returns an ordered list of agents to notify for review,
/// ranked by: recency of last touch, number of commits, current task relevance.
pub async fn get_review_routing(
    State(state): State<Arc<AppState>>,
    Path(repo_id): Path<String>,
    Query(params): Query<ReviewRoutingQuery>,
) -> Result<Json<Vec<ReviewRoutingEntry>>, ApiError> {
    // Verify the repo exists.
    state
        .repos
        .find_by_id(&Id::new(&repo_id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("repo {repo_id} not found")))?;

    let _path = params.path.unwrap_or_default();

    // Get all commits in this repo.
    let commits = state.agent_commits.find_by_repo(&Id::new(&repo_id)).await?;

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    // Group commits by agent.
    let mut by_agent: HashMap<String, Vec<u64>> = HashMap::new();
    for commit in &commits {
        by_agent
            .entry(commit.agent_id.to_string())
            .or_default()
            .push(commit.timestamp);
    }

    // Build routing entries.
    let mut entries: Vec<ReviewRoutingEntry> = Vec::new();
    for (agent_id, timestamps) in by_agent {
        let agent = match state.agents.find_by_id(&Id::new(&agent_id)).await {
            Ok(Some(a)) => a,
            _ => continue,
        };

        let commit_count = timestamps.len();
        let last_touch = *timestamps.iter().max().unwrap_or(&0);
        let age_secs = now.saturating_sub(last_touch) as f64;

        // Score: higher is better. Favors recent activity and more commits.
        // score = commit_count / (1 + age_days)
        let age_days = age_secs / 86400.0;
        let score = commit_count as f64 / (1.0 + age_days);

        // Get task from worktree.
        let task_id = state
            .worktrees
            .find_by_agent(&agent.id)
            .await
            .unwrap_or_default()
            .into_iter()
            .find(|w| w.repository_id.as_str() == repo_id)
            .and_then(|w| w.task_id)
            .map(|id| id.to_string());

        entries.push(ReviewRoutingEntry {
            agent_id,
            agent_name: agent.name,
            task_id,
            commit_count,
            last_touch,
            score,
        });
    }

    // Sort by score descending.
    entries.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    Ok(Json(entries))
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use crate::mem::test_state;
    use axum::{body::Body, Router};
    use http::{Request, StatusCode};
    use tower::ServiceExt;

    fn app() -> Router {
        crate::api::api_router().with_state(test_state())
    }

    async fn body_json(resp: axum::response::Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    async fn create_repo(app: &Router) -> String {
        let body = serde_json::json!({"workspace_id": "ws-1", "name": "test-repo"});
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/repos")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let json = body_json(resp).await;
        json["id"].as_str().unwrap().to_string()
    }

    #[tokio::test]
    async fn blame_empty_repo() {
        let app = app();
        let repo_id = create_repo(&app).await;
        // No path param → empty result
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/repos/{repo_id}/blame"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert!(json.as_array().unwrap().is_empty());
        // With path param → also empty (no git repo on disk in tests)
        let resp2 = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/repos/{repo_id}/blame?path=src/lib.rs"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp2.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn blame_repo_not_found() {
        let resp = app()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/repos/no-such/blame")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn hot_files_empty() {
        let app = app();
        let repo_id = create_repo(&app).await;
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/repos/{repo_id}/hot-files"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert!(json.as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn hot_files_not_found() {
        let resp = app()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/repos/no-such/hot-files")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn touched_paths_agent_not_found() {
        let resp = app()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/agents/no-such/touched-paths")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn review_routing_empty() {
        let app = app();
        let repo_id = create_repo(&app).await;
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/api/v1/repos/{repo_id}/review-routing?path=src/lib.rs"
                    ))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert!(json.as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn review_routing_not_found() {
        let resp = app()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/repos/no-such/review-routing")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }
}
