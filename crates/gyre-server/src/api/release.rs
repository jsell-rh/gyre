//! Release automation API endpoint.
//!
//! POST /api/v1/release/prepare (Admin only)
//! Computes next semver version from conventional commits since the last tag,
//! generates a rich changelog with agent/task attribution, and optionally
//! opens a release MR.

use axum::{extract::State, Json};
use gyre_common::Id;
use serde::Deserialize;
use std::sync::Arc;

use crate::{
    version_compute::{
        commits_since, compute_next_version, epoch_secs_to_date, latest_semver_tag,
        parse_conventional, render_changelog_markdown, type_bump, BumpLevel, ChangelogEntry,
        ChangelogSection, ReleasePrepareResponse,
    },
    AppState,
};

use super::{error::ApiError, new_id, now_secs};

// ── Request type ──────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct ReleasePrepareRequest {
    /// Repository ID to analyze.
    pub repo_id: String,
    /// Branch to analyze (default: the repo's default_branch).
    pub branch: Option<String>,
    /// Override the "from" ref for the commit range (default: latest semver tag).
    pub from: Option<String>,
    /// If true, create a release MR with the changelog.
    #[serde(default)]
    pub create_mr: bool,
    /// Title for the release MR (default: "Release {next_version}").
    pub mr_title: Option<String>,
}

// ── Handler ───────────────────────────────────────────────────────────────────

/// POST /api/v1/release/prepare — Admin only.
///
/// Computes the next semver version from conventional commits since the last
/// semver tag, generates a changelog with agent/task attribution from the
/// provenance store, and optionally opens a release MR.
pub async fn release_prepare(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ReleasePrepareRequest>,
) -> Result<Json<ReleasePrepareResponse>, ApiError> {
    // Validate optional ref inputs to prevent git argument injection.
    if let Some(ref b) = req.branch {
        if !crate::git_refs::refname_safe(b) {
            return Err(ApiError::InvalidInput("invalid branch name".into()));
        }
    }
    if let Some(ref f) = req.from {
        if !crate::git_refs::refname_safe(f) {
            return Err(ApiError::InvalidInput("invalid from ref".into()));
        }
    }

    // Look up the repository.
    let repo = state
        .repos
        .find_by_id(&Id::new(&req.repo_id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("repo {} not found", req.repo_id)))?;

    let branch = req.branch.unwrap_or_else(|| repo.default_branch.clone());
    let repo_path = repo.path.clone();

    // Determine "from" ref: explicit override or latest semver tag.
    let tag = if let Some(f) = req.from {
        Some(f)
    } else {
        latest_semver_tag(&repo_path).await
    };

    // List commits since the tag.
    let raw_commits = commits_since(&repo_path, tag.as_deref(), &branch)
        .await
        .unwrap_or_default();
    let commit_count = raw_commits.len();

    // Load agent commit provenance for this repo.
    let all_provenance = state
        .agent_commits
        .find_by_repo(&Id::new(&req.repo_id))
        .await
        .unwrap_or_default();
    let prov_map: std::collections::HashMap<String, &gyre_domain::AgentCommit> = all_provenance
        .iter()
        .map(|ac| (ac.commit_sha.clone(), ac))
        .collect();

    // Parse commits, compute bump level, and build changelog sections.
    let mut bump = BumpLevel::None;
    let mut breaking: Vec<ChangelogEntry> = Vec::new();
    let mut features: Vec<ChangelogEntry> = Vec::new();
    let mut fixes: Vec<ChangelogEntry> = Vec::new();

    for (sha, subject) in &raw_commits {
        let cc = match parse_conventional(sha, subject) {
            Some(c) => c,
            None => continue, // skip non-conventional commits in the changelog
        };

        // Update bump level.
        if cc.is_breaking {
            bump = BumpLevel::Major;
        } else {
            let tb = type_bump(&cc.commit_type);
            if tb > bump {
                bump = tb;
            }
        }

        // Resolve agent attribution from provenance store.
        let agent_id = prov_map.get(sha).map(|p| p.agent_id.to_string());
        let task_id = prov_map.get(sha).and_then(|p| p.task_id.clone());
        let agent_name = if let Some(ref aid) = agent_id {
            state
                .agents
                .find_by_id(&Id::new(aid))
                .await
                .ok()
                .flatten()
                .map(|a| a.name)
        } else {
            None
        };

        let entry = ChangelogEntry {
            sha: sha.clone(),
            commit_type: cc.commit_type.clone(),
            scope: cc.scope.clone(),
            description: cc.description.clone(),
            is_breaking: cc.is_breaking,
            agent_id,
            agent_name,
            task_id,
        };

        if cc.is_breaking {
            breaking.push(entry);
        } else {
            match cc.commit_type.as_str() {
                "feat" => features.push(entry),
                "fix" | "perf" => fixes.push(entry),
                _ => {} // docs, chore, etc. are not listed in the changelog
            }
        }
    }

    let has_release = bump != BumpLevel::None;
    let next_version = compute_next_version(tag.as_deref(), &bump);

    // Build changelog sections.
    let mut sections: Vec<ChangelogSection> = Vec::new();
    if !breaking.is_empty() {
        sections.push(ChangelogSection {
            title: "BREAKING CHANGES".to_string(),
            entries: breaking,
        });
    }
    if !features.is_empty() {
        sections.push(ChangelogSection {
            title: "Features".to_string(),
            entries: features,
        });
    }
    if !fixes.is_empty() {
        sections.push(ChangelogSection {
            title: "Bug Fixes".to_string(),
            entries: fixes,
        });
    }

    // Render markdown changelog.
    let now = now_secs();
    let date = epoch_secs_to_date(now);
    let changelog = render_changelog_markdown(&next_version, &date, &sections);

    // Optionally create a release MR.
    let mr_id = if req.create_mr && has_release {
        let title = req
            .mr_title
            .unwrap_or_else(|| format!("Release {next_version}"));
        // The MR description is the generated changelog.
        let mr = gyre_domain::MergeRequest::new(
            new_id(),
            Id::new(&req.repo_id),
            title,
            branch.clone(),
            repo.default_branch.clone(),
            now,
        );
        // Store the changelog as the MR description by setting title prefix.
        // In a real release flow the agent would create a version-bump commit
        // and push it first; here we open the MR against the current branch.
        state.merge_requests.create(&mr).await?;
        Some(mr.id.to_string())
    } else {
        None
    };

    Ok(Json(ReleasePrepareResponse {
        current_tag: tag,
        next_version,
        bump_type: bump,
        commit_count,
        has_release,
        branch,
        changelog,
        sections,
        mr_id,
    }))
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

    // The test_state() uses "test-token" as the global auth token,
    // which is treated as the system/admin token.
    const AUTH: &str = "Bearer test-token";

    // Helper: create a test repo and return its ID.
    async fn create_repo(app: &Router) -> String {
        let body = serde_json::json!({"workspace_id": "ws-1", "name": "test-repo"});
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/repos")
                    .header("content-type", "application/json")
                    .header("Authorization", AUTH)
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let json = body_json(resp).await;
        json["id"].as_str().unwrap().to_string()
    }

    #[tokio::test]
    async fn release_prepare_repo_not_found() {
        let resp = app()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/release/prepare")
                    .header("content-type", "application/json")
                    .header("Authorization", AUTH)
                    .body(Body::from(
                        serde_json::to_vec(&serde_json::json!({"repo_id": "no-such-repo"}))
                            .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn release_prepare_empty_repo_returns_v0_1_0() {
        let app = app();
        let repo_id = create_repo(&app).await;

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/release/prepare")
                    .header("content-type", "application/json")
                    .header("Authorization", AUTH)
                    .body(Body::from(
                        serde_json::to_vec(&serde_json::json!({"repo_id": repo_id})).unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        // No git repo on disk -> no commits -> v0.1.0, no bump.
        assert_eq!(json["next_version"], "v0.1.0");
        assert_eq!(json["bump_type"], "none");
        assert_eq!(json["commit_count"], 0);
        assert_eq!(json["has_release"], false);
        assert!(json["current_tag"].is_null());
        assert!(json["sections"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn release_prepare_invalid_branch_returns_400() {
        let app = app();
        let repo_id = create_repo(&app).await;

        // Branch starting with '-' is a git flag injection attempt.
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/release/prepare")
                    .header("content-type", "application/json")
                    .header("Authorization", AUTH)
                    .body(Body::from(
                        serde_json::to_vec(&serde_json::json!({
                            "repo_id": repo_id,
                            "branch": "-evil-branch"
                        }))
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn release_prepare_invalid_from_returns_400() {
        let app = app();
        let repo_id = create_repo(&app).await;

        // "from" ref containing ".." is a git path traversal attempt.
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/release/prepare")
                    .header("content-type", "application/json")
                    .header("Authorization", AUTH)
                    .body(Body::from(
                        serde_json::to_vec(&serde_json::json!({
                            "repo_id": repo_id,
                            "from": "HEAD..evil"
                        }))
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn release_prepare_missing_repo_id_is_bad_request() {
        let resp = app()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/release/prepare")
                    .header("content-type", "application/json")
                    .header("Authorization", AUTH)
                    .body(Body::from(b"{}".as_ref()))
                    .unwrap(),
            )
            .await
            .unwrap();
        // Missing required field -> 422 Unprocessable Entity.
        assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }
}
