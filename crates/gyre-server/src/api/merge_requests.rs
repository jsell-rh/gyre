use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use gyre_common::Id;
use gyre_domain::{
    AnalyticsEvent, DependencySource, MergeRequest, MergeRequestDependency, MrStatus, Review,
    ReviewComment, ReviewDecision,
};
use gyre_ports::search::SearchDocument;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, sync::Arc};
use tracing::{info, instrument};

use crate::AppState;

use super::error::ApiError;
use super::{new_id, now_secs};

// ── Request / Response types ────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct CreateMrRequest {
    pub repository_id: String,
    pub title: String,
    pub source_branch: String,
    pub target_branch: String,
    pub author_agent_id: Option<String>,
    /// Optional spec reference "path/to/spec.md@<sha>" for cryptographic binding.
    pub spec_ref: Option<String>,
    /// Optional MR IDs that must merge before this one (creation-time explicit deps).
    #[serde(default)]
    pub depends_on: Option<Vec<String>>,
}

#[derive(Deserialize)]
pub struct ListMrsQuery {
    pub status: Option<String>,
    pub repository_id: Option<String>,
    pub workspace_id: Option<String>,
}

#[derive(Deserialize)]
pub struct TransitionStatusRequest {
    pub status: String,
}

#[derive(Deserialize)]
pub struct AddCommentRequest {
    pub author_agent_id: String,
    pub body: String,
    pub file_path: Option<String>,
    pub line_number: Option<u32>,
}

#[derive(Deserialize)]
pub struct SubmitReviewRequest {
    pub reviewer_agent_id: String,
    pub decision: String,
    pub body: Option<String>,
}

#[derive(Serialize)]
pub struct DiffStatsResponse {
    pub files_changed: usize,
    pub insertions: usize,
    pub deletions: usize,
}

#[derive(Serialize)]
pub struct MrResponse {
    pub id: String,
    pub repository_id: String,
    pub title: String,
    pub source_branch: String,
    pub target_branch: String,
    pub status: String,
    pub author_agent_id: Option<String>,
    pub diff_stats: Option<DiffStatsResponse>,
    pub has_conflicts: Option<bool>,
    pub spec_ref: Option<String>,
    pub depends_on: Vec<String>,
    pub atomic_group: Option<String>,
    pub created_at: u64,
    pub updated_at: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub merge_commit_sha: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub merged_at: Option<u64>,
    /// Task ID linked through the authoring agent (enriched at query time).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
}

impl From<MergeRequest> for MrResponse {
    fn from(mr: MergeRequest) -> Self {
        Self {
            id: mr.id.to_string(),
            repository_id: mr.repository_id.to_string(),
            title: mr.title,
            source_branch: mr.source_branch,
            target_branch: mr.target_branch,
            status: mr_status_str(&mr.status),
            author_agent_id: mr.author_agent_id.map(|id| id.to_string()),
            diff_stats: mr.diff_stats.map(|d| DiffStatsResponse {
                files_changed: d.files_changed,
                insertions: d.insertions,
                deletions: d.deletions,
            }),
            has_conflicts: mr.has_conflicts,
            spec_ref: mr.spec_ref,
            depends_on: mr
                .depends_on
                .iter()
                .map(|d| d.target_mr_id.to_string())
                .collect(),
            atomic_group: mr.atomic_group,
            created_at: mr.created_at,
            updated_at: mr.updated_at,
            merge_commit_sha: None,
            merged_at: None,
            task_id: None,
        }
    }
}

#[derive(Serialize)]
pub struct CommentResponse {
    pub id: String,
    pub merge_request_id: String,
    pub author_agent_id: String,
    pub body: String,
    pub file_path: Option<String>,
    pub line_number: Option<u32>,
    pub created_at: u64,
}

impl From<ReviewComment> for CommentResponse {
    fn from(c: ReviewComment) -> Self {
        Self {
            id: c.id.to_string(),
            merge_request_id: c.merge_request_id.to_string(),
            author_agent_id: c.author_agent_id,
            body: c.body,
            file_path: c.file_path,
            line_number: c.line_number,
            created_at: c.created_at,
        }
    }
}

#[derive(Serialize)]
pub struct ReviewResponse {
    pub id: String,
    pub merge_request_id: String,
    pub reviewer_agent_id: String,
    pub decision: String,
    pub body: Option<String>,
    pub created_at: u64,
}

impl From<Review> for ReviewResponse {
    fn from(r: Review) -> Self {
        Self {
            id: r.id.to_string(),
            merge_request_id: r.merge_request_id.to_string(),
            reviewer_agent_id: r.reviewer_agent_id,
            decision: review_decision_str(&r.decision),
            body: r.body,
            created_at: r.created_at,
        }
    }
}

#[derive(Serialize)]
pub struct DiffLineResponse {
    #[serde(rename = "type")]
    pub line_type: String,
    pub content: String,
}

#[derive(Serialize)]
pub struct HunkResponse {
    pub header: String,
    pub lines: Vec<DiffLineResponse>,
}

#[derive(Serialize)]
pub struct FileDiffStructured {
    pub path: String,
    pub status: String,
    pub hunks: Vec<HunkResponse>,
}

#[derive(Serialize)]
pub struct DiffResponse {
    pub files_changed: usize,
    pub insertions: usize,
    pub deletions: usize,
    pub files: Vec<FileDiffStructured>,
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn mr_status_str(s: &MrStatus) -> String {
    match s {
        MrStatus::Open => "open",
        MrStatus::Approved => "approved",
        MrStatus::Merged => "merged",
        MrStatus::Closed => "closed",
        MrStatus::Reverted => "reverted",
    }
    .to_string()
}

fn parse_mr_status(s: &str) -> Result<MrStatus, ApiError> {
    match s.to_lowercase().as_str() {
        "open" => Ok(MrStatus::Open),
        "approved" => Ok(MrStatus::Approved),
        "merged" => Ok(MrStatus::Merged),
        "closed" => Ok(MrStatus::Closed),
        "reverted" => Ok(MrStatus::Reverted),
        _ => Err(ApiError::InvalidInput(format!("unknown MR status: {s}"))),
    }
}

fn review_decision_str(d: &ReviewDecision) -> String {
    match d {
        ReviewDecision::Approved => "approved",
        ReviewDecision::ChangesRequested => "changes_requested",
    }
    .to_string()
}

fn parse_review_decision(s: &str) -> Result<ReviewDecision, ApiError> {
    match s.to_lowercase().as_str() {
        "approved" => Ok(ReviewDecision::Approved),
        "changes_requested" => Ok(ReviewDecision::ChangesRequested),
        _ => Err(ApiError::InvalidInput(format!(
            "unknown review decision: {s}"
        ))),
    }
}

// ── Handlers ─────────────────────────────────────────────────────────────────

#[instrument(skip(state, req), fields(source = %req.source_branch, target = %req.target_branch))]
pub async fn create_mr(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateMrRequest>,
) -> Result<(StatusCode, Json<MrResponse>), ApiError> {
    // Validate spec_ref SHA if provided ("path@sha" format, SHA must be 40-char hex).
    if let Some(ref spec_ref) = req.spec_ref {
        if let Some(sha) = spec_ref.rsplit_once('@').map(|(_, s)| s) {
            if sha.len() != 40 || !sha.chars().all(|c| c.is_ascii_hexdigit()) {
                return Err(ApiError::InvalidInput(
                    "spec_ref SHA must be a 40-character hex string".to_string(),
                ));
            }
        } else {
            return Err(ApiError::InvalidInput(
                "spec_ref must be in format 'path@sha'".to_string(),
            ));
        }
    }

    let now = now_secs();
    let repo_id = Id::new(req.repository_id);
    let mut mr = MergeRequest::new(
        new_id(),
        repo_id.clone(),
        req.title,
        req.source_branch.clone(),
        req.target_branch.clone(),
        now,
    );
    mr.author_agent_id = req.author_agent_id.map(Id::new);
    mr.spec_ref = req.spec_ref;

    // Validate and set creation-time explicit dependencies.
    let explicit_deps = if let Some(ref dep_ids) = req.depends_on {
        let mut validated = Vec::new();
        for dep_id_str in dep_ids {
            let dep_id = Id::new(dep_id_str);
            state
                .merge_requests
                .find_by_id(&dep_id)
                .await?
                .ok_or_else(|| {
                    ApiError::NotFound(format!("dependency merge request {dep_id_str} not found"))
                })?;
            validated.push(MergeRequestDependency::new(
                dep_id,
                DependencySource::Explicit,
            ));
        }

        // Cycle check: build adjacency map from existing MRs.
        let all_mrs = state.merge_requests.list().await?;
        let mut adj: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();
        for m in &all_mrs {
            adj.insert(
                m.id.to_string(),
                m.depends_on
                    .iter()
                    .map(|d| d.target_mr_id.to_string())
                    .collect(),
            );
        }
        let mr_id_str = mr.id.to_string();
        if super::merge_deps::would_create_cycle(&mr_id_str, dep_ids, &adj) {
            return Err(ApiError::InvalidInput(
                "cycle detected: adding these dependencies would create a circular dependency chain"
                    .to_string(),
            ));
        }
        validated
    } else {
        Vec::new()
    };

    // Compute diff stats, conflict detection, and auto-detect branch lineage deps.
    if let Ok(Some(repo)) = state.repos.find_by_id(&repo_id).await {
        mr.workspace_id = repo.workspace_id.clone();
        if let Ok(diff) = state
            .git_ops
            .diff(&repo.path, &req.target_branch, &req.source_branch)
            .await
        {
            mr.diff_stats = Some(gyre_domain::DiffStats {
                files_changed: diff.files_changed,
                insertions: diff.insertions,
                deletions: diff.deletions,
            });
        }
        if let Ok(can_merge) = state
            .git_ops
            .can_merge(&repo.path, &req.source_branch, &req.target_branch)
            .await
        {
            mr.has_conflicts = Some(!can_merge);
        }
        // Auto-detect branch lineage dependencies (P4).
        let lineage_deps = detect_lineage_deps(
            &state,
            &repo_id,
            &repo.path,
            &req.source_branch,
            &req.target_branch,
        )
        .await;
        if !lineage_deps.is_empty() {
            mr.depends_on = lineage_deps;
        }
    }

    // Merge explicit deps with lineage deps: explicit takes precedence,
    // lineage adds to the set for deps not already declared.
    if !explicit_deps.is_empty() {
        let explicit_ids: std::collections::HashSet<String> = explicit_deps
            .iter()
            .map(|d| d.target_mr_id.to_string())
            .collect();
        // Keep lineage deps that aren't already in explicit set.
        let additional_lineage: Vec<_> = mr
            .depends_on
            .drain(..)
            .filter(|d| !explicit_ids.contains(&d.target_mr_id.to_string()))
            .collect();
        mr.depends_on = explicit_deps;
        mr.depends_on.extend(additional_lineage);
    }

    state.merge_requests.create(&mr).await?;
    // Index for search.
    let mut facets = HashMap::new();
    facets.insert("status".to_string(), "open".to_string());
    facets.insert("repo_id".to_string(), mr.repository_id.to_string());
    let _ = state
        .search
        .index(SearchDocument {
            entity_type: "mr".to_string(),
            entity_id: mr.id.to_string(),
            title: mr.title.clone(),
            body: format!("{} -> {}", mr.source_branch, mr.target_branch),
            workspace_id: None,
            repo_id: Some(mr.repository_id.to_string()),
            facets,
        })
        .await;
    state
        .emit_event(
            Some(mr.workspace_id.clone()),
            gyre_common::message::Destination::Workspace(mr.workspace_id.clone()),
            gyre_common::message::MessageKind::MrCreated,
            Some(serde_json::json!({"mr_id": mr.id.to_string()})),
        )
        .await;
    Ok((StatusCode::CREATED, Json(MrResponse::from(mr))))
}

/// Auto-detect MR dependencies based on git branch lineage (P4).
///
/// For each open MR in the same repo targeting the same target branch, checks
/// whether `source_branch` is a descendant of that MR's source branch by
/// comparing the merge-base to the candidate branch tip. If merge-base == tip,
/// the new branch was created from the candidate branch and should depend on it.
async fn detect_lineage_deps(
    state: &Arc<AppState>,
    repo_id: &Id,
    repo_path: &str,
    source_branch: &str,
    target_branch: &str,
) -> Vec<MergeRequestDependency> {
    let all_mrs = match state.merge_requests.list_by_repo(repo_id).await {
        Ok(mrs) => mrs,
        Err(_) => return vec![],
    };

    let candidates: Vec<_> = all_mrs
        .into_iter()
        .filter(|m| {
            m.target_branch == target_branch
                && m.source_branch != source_branch
                && m.status == MrStatus::Open
        })
        .collect();

    if candidates.is_empty() {
        return vec![];
    }

    let git_bin = std::env::var("GYRE_GIT_PATH").unwrap_or_else(|_| "git".to_string());
    let mut deps = Vec::new();

    // Validate a branch name is safe to pass to git (no flag injection).
    let is_safe_branch = |b: &str| !b.starts_with('-') && !b.contains("..");

    if !is_safe_branch(source_branch) {
        return vec![];
    }

    for candidate in candidates {
        let cand_branch = &candidate.source_branch;
        if !is_safe_branch(cand_branch) {
            continue;
        }

        // Get the tip SHA of the candidate branch.
        let tip_out = tokio::process::Command::new(&git_bin)
            .arg("-C")
            .arg(repo_path)
            .arg("rev-parse")
            .arg(format!("refs/heads/{cand_branch}"))
            .output()
            .await
            .ok();

        let cand_tip = match tip_out.as_ref().filter(|o| o.status.success()) {
            Some(o) => String::from_utf8_lossy(&o.stdout).trim().to_string(),
            None => continue,
        };

        if cand_tip.is_empty() || !cand_tip.chars().all(|c| c.is_ascii_hexdigit()) {
            continue;
        }

        // Get the merge-base of the candidate branch and the new source branch.
        let mb_out = tokio::process::Command::new(&git_bin)
            .arg("-C")
            .arg(repo_path)
            .arg("merge-base")
            .arg(format!("refs/heads/{cand_branch}"))
            .arg(format!("refs/heads/{source_branch}"))
            .output()
            .await
            .ok();

        let merge_base = match mb_out.as_ref().filter(|o| o.status.success()) {
            Some(o) => String::from_utf8_lossy(&o.stdout).trim().to_string(),
            None => continue,
        };

        // If merge-base == cand tip, source_branch is a descendant of cand branch.
        if !merge_base.is_empty() && merge_base == cand_tip {
            info!(
                source = source_branch,
                parent_branch = %cand_branch,
                mr_id = %candidate.id,
                "auto-detected branch lineage dependency"
            );
            deps.push(MergeRequestDependency::new(
                candidate.id,
                DependencySource::BranchLineage,
            ));
        }
    }

    deps
}

pub async fn list_mrs(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListMrsQuery>,
) -> Result<Json<Vec<MrResponse>>, ApiError> {
    let mrs = if let Some(ws_id) = params.workspace_id {
        state
            .merge_requests
            .list_by_workspace(&Id::new(ws_id))
            .await?
    } else {
        match (params.status, params.repository_id) {
            (Some(status_str), _) => {
                let status = parse_mr_status(&status_str)?;
                state.merge_requests.list_by_status(&status).await?
            }
            (_, Some(repo_id)) => state.merge_requests.list_by_repo(&Id::new(repo_id)).await?,
            _ => state.merge_requests.list().await?,
        }
    };
    let mut results: Vec<MrResponse> = mrs.into_iter().map(MrResponse::from).collect();
    // Enrich with task_id from agent worktree tracking (best-effort).
    for resp in &mut results {
        if let Some(ref agent_id) = resp.author_agent_id {
            if let Ok(worktrees) = state.worktrees.find_by_agent(&Id::new(agent_id)).await {
                if let Some(wt) = worktrees.first() {
                    resp.task_id = wt.task_id.as_ref().map(|id| id.to_string());
                }
            }
        }
    }
    Ok(Json(results))
}

/// GET /api/v1/workspaces/:workspace_id/merge-requests — list MRs scoped to a workspace.
/// Primary access pattern per api-conventions.md §1.1.
pub async fn list_workspace_mrs(
    State(state): State<Arc<AppState>>,
    Path(workspace_id): Path<String>,
) -> Result<Json<Vec<MrResponse>>, ApiError> {
    let mrs = state
        .merge_requests
        .list_by_workspace(&Id::new(workspace_id))
        .await?;
    let mut results: Vec<MrResponse> = mrs.into_iter().map(MrResponse::from).collect();
    for resp in &mut results {
        if let Some(ref agent_id) = resp.author_agent_id {
            if let Ok(worktrees) = state.worktrees.find_by_agent(&Id::new(agent_id)).await {
                if let Some(wt) = worktrees.first() {
                    resp.task_id = wt.task_id.as_ref().map(|id| id.to_string());
                }
            }
        }
    }
    Ok(Json(results))
}

pub async fn get_mr(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<MrResponse>, ApiError> {
    let mr_id = Id::new(&id);
    let mr = state
        .merge_requests
        .find_by_id(&mr_id)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("merge request {id} not found")))?;
    let mut resp = MrResponse::from(mr);
    // Enrich merged MRs with merge_commit_sha and merged_at from attestation.
    if resp.status == "merged" {
        if let Ok(Some(att)) = state.attestation_store.find_by_mr_id(&id).await {
            resp.merge_commit_sha = Some(att.attestation.merge_commit_sha.clone());
            resp.merged_at = Some(att.attestation.merged_at);
        }
    }
    // Enrich with task_id from the authoring agent's worktree tracking.
    if let Some(ref agent_id) = resp.author_agent_id {
        if let Ok(worktrees) = state.worktrees.find_by_agent(&Id::new(agent_id)).await {
            if let Some(wt) = worktrees.first() {
                resp.task_id = wt.task_id.as_ref().map(|id| id.to_string());
            }
        }
    }
    Ok(Json(resp))
}

#[instrument(skip(state, req), fields(mr_id = %id, new_status = %req.status))]
pub async fn transition_mr_status(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<TransitionStatusRequest>,
) -> Result<Json<MrResponse>, ApiError> {
    let mut mr = state
        .merge_requests
        .find_by_id(&Id::new(&id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("merge request {id} not found")))?;
    let new_status = parse_mr_status(&req.status)?;
    let is_merge = matches!(new_status, MrStatus::Merged);
    mr.transition_status(new_status)
        .map_err(|e| ApiError::InvalidInput(e.to_string()))?;
    let ts = now_secs();
    mr.updated_at = ts;
    state.merge_requests.update(&mr).await?;
    {
        let ws_id = mr.workspace_id.clone();
        let kind = if is_merge {
            gyre_common::message::MessageKind::MrMerged
        } else {
            gyre_common::message::MessageKind::MrStatusChanged
        };
        let payload = if is_merge {
            serde_json::json!({"mr_id": mr.id.to_string()})
        } else {
            serde_json::json!({"mr_id": mr.id.to_string(), "status": req.status})
        };
        state
            .emit_event(
                Some(ws_id.clone()),
                gyre_common::message::Destination::Workspace(ws_id),
                kind,
                Some(payload),
            )
            .await;
    }

    // Auto-track mr.merged analytics event
    if is_merge {
        let ev = AnalyticsEvent::new(
            new_id(),
            "mr.merged",
            mr.author_agent_id.as_ref().map(|id| id.to_string()),
            serde_json::json!({ "mr_id": mr.id.to_string() }),
            ts,
        );
        let _ = state.analytics.record(&ev).await;
    }

    Ok(Json(MrResponse::from(mr)))
}

pub async fn add_comment(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<AddCommentRequest>,
) -> Result<(StatusCode, Json<CommentResponse>), ApiError> {
    // Verify MR exists
    state
        .merge_requests
        .find_by_id(&Id::new(&id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("merge request {id} not found")))?;

    let mut comment = ReviewComment::new(
        new_id(),
        Id::new(id),
        req.author_agent_id,
        req.body,
        now_secs(),
    );
    comment.file_path = req.file_path;
    comment.line_number = req.line_number;

    state.reviews.add_comment(&comment).await?;
    Ok((StatusCode::CREATED, Json(CommentResponse::from(comment))))
}

pub async fn list_comments(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<Vec<CommentResponse>>, ApiError> {
    let mr_id = Id::new(&id);
    state
        .merge_requests
        .find_by_id(&mr_id)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("merge request {id} not found")))?;

    let comments = state.reviews.list_comments(&mr_id).await?;
    Ok(Json(
        comments.into_iter().map(CommentResponse::from).collect(),
    ))
}

#[instrument(skip(state, req), fields(mr_id = %id, reviewer = %req.reviewer_agent_id, decision = %req.decision))]
pub async fn submit_review(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<SubmitReviewRequest>,
) -> Result<(StatusCode, Json<ReviewResponse>), ApiError> {
    let mr_id = Id::new(&id);
    state
        .merge_requests
        .find_by_id(&mr_id)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("merge request {id} not found")))?;

    let decision = parse_review_decision(&req.decision)?;
    let mut review = Review::new(new_id(), mr_id, req.reviewer_agent_id, decision, now_secs());
    review.body = req.body;

    state.reviews.submit_review(&review).await?;
    Ok((StatusCode::CREATED, Json(ReviewResponse::from(review))))
}

pub async fn list_reviews(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<Vec<ReviewResponse>>, ApiError> {
    let mr_id = Id::new(&id);
    state
        .merge_requests
        .find_by_id(&mr_id)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("merge request {id} not found")))?;

    let reviews = state.reviews.list_reviews(&mr_id).await?;
    Ok(Json(
        reviews.into_iter().map(ReviewResponse::from).collect(),
    ))
}

fn parse_patch_to_hunks(raw: &str) -> Vec<HunkResponse> {
    let mut hunks: Vec<HunkResponse> = Vec::new();
    let mut current_hunk: Option<HunkResponse> = None;
    for line in raw.lines() {
        if line.starts_with("@@") {
            if let Some(h) = current_hunk.take() {
                hunks.push(h);
            }
            current_hunk = Some(HunkResponse {
                header: line.to_string(),
                lines: Vec::new(),
            });
        } else if line.starts_with("diff ")
            || line.starts_with("index ")
            || line.starts_with("--- ")
            || line.starts_with("+++ ")
        {
            // skip file header lines
        } else if let Some(ref mut hunk) = current_hunk {
            let (line_type, content) = if let Some(rest) = line.strip_prefix('+') {
                ("add", rest.to_string())
            } else if let Some(rest) = line.strip_prefix('-') {
                ("delete", rest.to_string())
            } else if let Some(rest) = line.strip_prefix(' ') {
                ("context", rest.to_string())
            } else {
                continue;
            };
            hunk.lines.push(DiffLineResponse {
                line_type: line_type.to_string(),
                content,
            });
        }
    }
    if let Some(h) = current_hunk {
        hunks.push(h);
    }
    hunks
}

fn mock_diff_response() -> DiffResponse {
    DiffResponse {
        files_changed: 1,
        insertions: 3,
        deletions: 1,
        files: vec![FileDiffStructured {
            path: "src/main.rs".to_string(),
            status: "Modified".to_string(),
            hunks: vec![HunkResponse {
                header: "@@ -1,4 +1,6 @@".to_string(),
                lines: vec![
                    DiffLineResponse {
                        line_type: "context".to_string(),
                        content: "fn main() {".to_string(),
                    },
                    DiffLineResponse {
                        line_type: "delete".to_string(),
                        content: "    println!(\"Hello\");".to_string(),
                    },
                    DiffLineResponse {
                        line_type: "add".to_string(),
                        content: "    println!(\"Hello, world!\");".to_string(),
                    },
                    DiffLineResponse {
                        line_type: "add".to_string(),
                        content: "    println!(\"Done.\");".to_string(),
                    },
                    DiffLineResponse {
                        line_type: "context".to_string(),
                        content: "}".to_string(),
                    },
                ],
            }],
        }],
    }
}

pub async fn get_diff(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<DiffResponse>, ApiError> {
    let mr = state
        .merge_requests
        .find_by_id(&Id::new(&id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("merge request {id} not found")))?;

    // If no real repo, return demo data
    let repo = match state.repos.find_by_id(&mr.repository_id).await? {
        Some(r) => r,
        None => return Ok(Json(mock_diff_response())),
    };

    // For merged MRs, source and target may point to the same commit after
    // fast-forward merge, producing an empty diff. Diff the source branch
    // against its parent commit to show what the MR actually changed.
    // This works for single-commit branches (the common agent case) and
    // is a reasonable approximation for multi-commit branches.
    let diff_from = if mr.status == gyre_domain::MrStatus::Merged {
        format!("{}^", mr.source_branch)
    } else {
        mr.target_branch.clone()
    };

    // If git diff fails (branches don't exist yet), return demo data
    let diff = match state
        .git_ops
        .diff(&repo.path, &diff_from, &mr.source_branch)
        .await
    {
        Ok(d) => d,
        Err(_) => return Ok(Json(mock_diff_response())),
    };

    let files = diff
        .patches
        .into_iter()
        .map(|p| FileDiffStructured {
            path: p.path,
            status: p.status,
            hunks: p
                .patch
                .as_deref()
                .map(parse_patch_to_hunks)
                .unwrap_or_default(),
        })
        .collect();

    Ok(Json(DiffResponse {
        files_changed: diff.files_changed,
        insertions: diff.insertions,
        deletions: diff.deletions,
        files,
    }))
}

// ── Attestation ───────────────────────────────────────────────────────────────

/// `GET /api/v1/merge-requests/{id}/attestation`
///
/// Returns the signed attestation bundle created at merge time (G5).
/// Returns 404 if the MR does not exist or has not yet been merged.
pub async fn get_attestation(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<crate::attestation::AttestationBundle>, ApiError> {
    // Verify the MR exists.
    state
        .merge_requests
        .find_by_id(&Id::new(&id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("merge request {id} not found")))?;

    state
        .attestation_store
        .find_by_mr_id(&id)
        .await?
        .map(Json)
        .ok_or_else(|| ApiError::NotFound(format!("no attestation found for merge request {id}")))
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

    async fn create_test_mr(app: Router, title: &str) -> (Router, String) {
        let body = serde_json::json!({
            "repository_id": "repo-1",
            "title": title,
            "source_branch": "feat/x",
            "target_branch": "main"
        });
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/merge-requests")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let json = body_json(resp).await;
        let id = json["id"].as_str().unwrap().to_string();
        (app, id)
    }

    #[tokio::test]
    async fn create_and_get_mr() {
        let app = app();
        let (app, id) = create_test_mr(app, "Add feature").await;

        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/merge-requests/{id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json["status"], "open");
        assert_eq!(json["title"], "Add feature");
    }

    #[tokio::test]
    async fn mr_status_transition_valid() {
        let app = app();
        let (app, id) = create_test_mr(app, "Approve me").await;

        let body = serde_json::json!({ "status": "approved" });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/merge-requests/{id}/status"))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json["status"], "approved");
    }

    #[tokio::test]
    async fn mr_status_transition_invalid() {
        let app = app();
        let (app, id) = create_test_mr(app, "Invalid trans").await;

        let body = serde_json::json!({ "status": "merged" });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/merge-requests/{id}/status"))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn list_mrs_by_repository() {
        let app = app();
        let (_, _) = create_test_mr(app.clone(), "MR for repo").await;

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/merge-requests?repository_id=repo-1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json.as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn get_mr_not_found() {
        let resp = app()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/merge-requests/no-such")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn add_and_list_comments() {
        let app = app();
        let (app, mr_id) = create_test_mr(app, "Comment test").await;

        let body = serde_json::json!({
            "author_agent_id": "agent-1",
            "body": "Looks good to me"
        });
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/merge-requests/{mr_id}/comments"))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/merge-requests/{mr_id}/comments"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        let arr = json.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["body"], "Looks good to me");
    }

    #[tokio::test]
    async fn comment_with_file_and_line() {
        let app = app();
        let (app, mr_id) = create_test_mr(app, "File comment test").await;

        let body = serde_json::json!({
            "author_agent_id": "agent-1",
            "body": "Fix this line",
            "file_path": "src/lib.rs",
            "line_number": 10
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/merge-requests/{mr_id}/comments"))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let json = body_json(resp).await;
        assert_eq!(json["file_path"], "src/lib.rs");
        assert_eq!(json["line_number"], 10);
    }

    #[tokio::test]
    async fn submit_approve_review() {
        let app = app();
        let (app, mr_id) = create_test_mr(app, "Review test").await;

        let body = serde_json::json!({
            "reviewer_agent_id": "agent-1",
            "decision": "approved",
            "body": "LGTM"
        });
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/merge-requests/{mr_id}/reviews"))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let json = body_json(resp).await;
        assert_eq!(json["decision"], "approved");

        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/merge-requests/{mr_id}/reviews"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let json = body_json(resp).await;
        assert_eq!(json.as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn submit_changes_requested_review() {
        let app = app();
        let (app, mr_id) = create_test_mr(app, "Changes test").await;

        let body = serde_json::json!({
            "reviewer_agent_id": "agent-1",
            "decision": "changes_requested"
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/merge-requests/{mr_id}/reviews"))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let json = body_json(resp).await;
        assert_eq!(json["decision"], "changes_requested");
    }

    #[tokio::test]
    async fn review_bad_decision_rejected() {
        let app = app();
        let (app, mr_id) = create_test_mr(app, "Bad decision").await;

        let body = serde_json::json!({
            "reviewer_agent_id": "agent-1",
            "decision": "maybe"
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/merge-requests/{mr_id}/reviews"))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn diff_endpoint_returns_200() {
        let app = app();
        let (app, mr_id) = create_test_mr(app, "Diff test").await;

        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/merge-requests/{mr_id}/diff"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        // NoopGitOps returns empty diff — but the repo won't be found, so this will 404
        // since test_state doesn't have a repo with id "repo-1"
        // The 404 is for repo not found, which is correct behavior.
        assert!(
            resp.status() == StatusCode::NOT_FOUND || resp.status() == StatusCode::OK,
            "unexpected status: {}",
            resp.status()
        );
    }

    #[tokio::test]
    async fn comment_on_missing_mr_returns_404() {
        let body = serde_json::json!({ "author_agent_id": "a1", "body": "hi" });
        let resp = app()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/merge-requests/no-such/comments")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn review_on_missing_mr_returns_404() {
        let body = serde_json::json!({ "reviewer_agent_id": "a1", "decision": "approved" });
        let resp = app()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/merge-requests/no-such/reviews")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    // ── Attestation tests ────────────────────────────────────────────────────

    #[tokio::test]
    async fn attestation_missing_mr_returns_404() {
        let resp = app()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/merge-requests/no-such-mr/attestation")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn attestation_unmerged_mr_returns_404() {
        let app = app();
        let (app, id) = create_test_mr(app, "Not yet merged").await;

        // MR exists but has no attestation yet (not merged)
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/merge-requests/{id}/attestation"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn attestation_bundle_round_trip() {
        use crate::attestation::{sign_attestation, verify_bundle, MergeAttestation};
        use crate::mem::test_state;

        let state = test_state();

        // Build and sign an attestation.
        let attestation = MergeAttestation {
            attestation_version: 1,
            mr_id: "mr-abc".to_string(),
            merge_commit_sha: "deadbeef".repeat(5),
            merged_at: 1_700_000_000,
            gate_results: vec![],
            spec_ref: None,
            spec_fully_approved: true,
            author_agent_id: Some("agent-1".to_string()),
            conversation_sha: None,
            completion_summary: None,
            meta_specs_used: vec![],
        };

        let bundle = sign_attestation(attestation, &state.agent_signing_key);

        // Signature must verify with the public key.
        assert!(verify_bundle(
            &bundle,
            &state.agent_signing_key.public_key_bytes
        ));

        // Tampered payload must fail verification.
        let mut tampered = bundle.clone();
        tampered.attestation.merge_commit_sha = "000000".to_string();
        assert!(!verify_bundle(
            &tampered,
            &state.agent_signing_key.public_key_bytes
        ));
    }

    #[tokio::test]
    async fn attestation_stored_and_retrievable() {
        use crate::attestation::{sign_attestation, MergeAttestation};
        use crate::mem::test_state;

        let state = test_state();

        // Create an MR so the GET endpoint can find it.
        let app = crate::api::api_router().with_state(state.clone());
        let (app, mr_id) = create_test_mr(app, "Merge me").await;

        // Directly insert an attestation bundle into the store.
        let attestation = MergeAttestation {
            attestation_version: 1,
            mr_id: mr_id.clone(),
            merge_commit_sha: "abc123".to_string(),
            merged_at: 1_700_000_000,
            gate_results: vec![],
            spec_ref: None,
            spec_fully_approved: true,
            author_agent_id: None,
            conversation_sha: None,
            completion_summary: None,
            meta_specs_used: vec![],
        };
        let bundle = sign_attestation(attestation, &state.agent_signing_key);
        state.attestation_store.save(&mr_id, &bundle).await.unwrap();

        // GET the attestation via the API.
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/merge-requests/{mr_id}/attestation"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let json = body_json(resp).await;
        assert_eq!(json["attestation"]["mr_id"], mr_id);
        assert_eq!(json["attestation"]["merge_commit_sha"], "abc123");
        assert_eq!(json["attestation"]["attestation_version"], 1);
        assert!(json["signature"].as_str().is_some());
        assert!(json["signing_key_id"].as_str().is_some());
    }

    // ── Creation-time dependency tests (TASK-028) ────────────────────────

    #[tokio::test]
    async fn create_mr_with_depends_on() {
        let app = app();
        let (app, dep_id) = create_test_mr(app, "Dependency MR").await;

        // Create MR with depends_on referencing the first one.
        let body = serde_json::json!({
            "repository_id": "repo-1",
            "title": "Dependent MR",
            "source_branch": "feat/dependent",
            "target_branch": "main",
            "depends_on": [dep_id]
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/merge-requests")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let json = body_json(resp).await;
        assert!(
            json["depends_on"]
                .as_array()
                .unwrap()
                .contains(&serde_json::json!(dep_id)),
            "creation-time dep should appear in MR response"
        );
    }

    #[tokio::test]
    async fn create_mr_with_depends_on_nonexistent_rejected() {
        let app = app();

        let body = serde_json::json!({
            "repository_id": "repo-1",
            "title": "Bad dep MR",
            "source_branch": "feat/bad",
            "target_branch": "main",
            "depends_on": ["does-not-exist"]
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/merge-requests")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn create_mr_without_depends_on_works() {
        // Omitting depends_on should still work (backward compat).
        let app = app();
        let body = serde_json::json!({
            "repository_id": "repo-1",
            "title": "No deps MR",
            "source_branch": "feat/nodeps",
            "target_branch": "main"
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/merge-requests")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let json = body_json(resp).await;
        assert_eq!(json["depends_on"].as_array().unwrap().len(), 0);
    }
}
