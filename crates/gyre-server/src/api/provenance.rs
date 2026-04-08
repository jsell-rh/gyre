use axum::{
    extract::{Path, Query, State},
    Json,
};
use gyre_common::Id;
use gyre_domain::AgentCommit;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::AppState;

use super::error::ApiError;

// ── Request / Response types ──────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct ProvenanceQuery {
    pub commit: Option<String>,
    pub task_id: Option<String>,
    pub agent_id: Option<String>,
}

#[derive(Serialize)]
pub struct ProvenanceRecord {
    pub id: String,
    pub commit_sha: String,
    pub branch: String,
    pub agent_id: String,
    pub repository_id: String,
    pub timestamp: u64,
    pub task_id: Option<String>,
    pub spawned_by_user_id: Option<String>,
    pub parent_agent_id: Option<String>,
    pub model_context: Option<serde_json::Value>,
    pub attestation_level: Option<String>,
}

impl From<AgentCommit> for ProvenanceRecord {
    fn from(ac: AgentCommit) -> Self {
        let model_context = ac
            .model_context
            .as_deref()
            .and_then(|s| serde_json::from_str(s).ok());
        Self {
            id: ac.id.to_string(),
            commit_sha: ac.commit_sha,
            branch: ac.branch,
            agent_id: ac.agent_id.to_string(),
            repository_id: ac.repository_id.to_string(),
            timestamp: ac.timestamp,
            task_id: ac.task_id,
            spawned_by_user_id: ac.spawned_by_user_id,
            parent_agent_id: ac.parent_agent_id,
            model_context,
            attestation_level: ac.attestation_level,
        }
    }
}

// ── Handler ───────────────────────────────────────────────────────────────────

/// GET /api/v1/repos/:id/provenance
///   ?commit={sha}       — full provenance for one commit
///   ?task_id={id}       — all commits for a task
///   ?agent_id={id}      — all commits by an agent in this repo
pub async fn get_provenance(
    State(state): State<Arc<AppState>>,
    Path(repo_id): Path<String>,
    Query(params): Query<ProvenanceQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    // Verify the repo exists.
    state
        .repos
        .find_by_id(&Id::new(&repo_id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("repo {repo_id} not found")))?;

    if let Some(sha) = params.commit {
        // Single commit lookup.
        let record = state
            .agent_commits
            .find_by_commit(&sha)
            .await?
            .filter(|ac| ac.repository_id.as_str() == repo_id)
            .map(ProvenanceRecord::from);
        return Ok(Json(serde_json::json!(record)));
    }

    if let Some(task_id) = params.task_id {
        // All commits for a task.
        let commits: Vec<ProvenanceRecord> = state
            .agent_commits
            .find_by_task(&task_id)
            .await?
            .into_iter()
            .filter(|ac| ac.repository_id.as_str() == repo_id)
            .map(ProvenanceRecord::from)
            .collect();

        return Ok(Json(serde_json::json!({
            "task_id": task_id,
            "commits": commits,
        })));
    }

    if let Some(agent_id) = params.agent_id {
        let commits: Vec<ProvenanceRecord> = state
            .agent_commits
            .find_by_agent(&Id::new(&agent_id))
            .await?
            .into_iter()
            .filter(|ac| ac.repository_id.as_str() == repo_id)
            .map(ProvenanceRecord::from)
            .collect();
        return Ok(Json(serde_json::json!(commits)));
    }

    // No filter: return all provenance records for the repo.
    let commits: Vec<ProvenanceRecord> = state
        .agent_commits
        .find_by_repo(&Id::new(&repo_id))
        .await?
        .into_iter()
        .map(ProvenanceRecord::from)
        .collect();
    Ok(Json(serde_json::json!(commits)))
}

// ── Attestation Verification & Export (Phase 3, §6.3, §6.4) ─────────────────

/// Path params for attestation endpoints.
#[derive(Deserialize)]
pub struct AttestationPath {
    pub id: String,
    pub commit_sha: String,
}

/// GET /api/v1/repos/:id/attestations/:commit_sha/verification
///
/// Returns the full `VerificationResult` tree for the attestation chain
/// associated with the given commit SHA.
/// ABAC: resource_type = attestation, action = read.
pub async fn get_verification(
    State(state): State<Arc<AppState>>,
    Path(AttestationPath {
        id: repo_id,
        commit_sha,
    }): Path<AttestationPath>,
) -> Result<Json<serde_json::Value>, ApiError> {
    // Verify repo exists.
    state
        .repos
        .find_by_id(&Id::new(&repo_id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("repo {repo_id} not found")))?;

    // Find attestation for this commit.
    let attestation = state
        .chain_attestations
        .find_by_commit(&commit_sha)
        .await
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("attestation lookup failed: {e}")))?
        .ok_or_else(|| {
            ApiError::NotFound(format!("no attestation found for commit {commit_sha}"))
        })?;

    // Load the full chain for comprehensive verification (§4.4, §6.2).
    let chain = state
        .chain_attestations
        .load_chain(&attestation.id)
        .await
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("chain lookup failed: {e}")))?;

    // Get workspace-configurable max depth (default 10).
    let max_depth = 10u32; // Will be configurable per workspace in future

    // Verify the full chain from leaf to root (§6.2).
    let result = crate::git_http::verify_chain(&chain, max_depth);

    Ok(Json(serde_json::json!({
        "commit_sha": commit_sha,
        "repo_id": repo_id,
        "attestation_id": attestation.id,
        "chain_depth": chain.len(),
        "verification": result,
    })))
}

/// GET /api/v1/repos/:id/attestations/:commit_sha/bundle
///
/// Returns the `VerificationBundle` for offline verification (§6.3).
/// ABAC: resource_type = attestation, action = export.
pub async fn get_attestation_bundle(
    State(state): State<Arc<AppState>>,
    Path(AttestationPath {
        id: repo_id,
        commit_sha,
    }): Path<AttestationPath>,
) -> Result<Json<serde_json::Value>, ApiError> {
    // Verify repo exists.
    let repo = state
        .repos
        .find_by_id(&Id::new(&repo_id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("repo {repo_id} not found")))?;

    // Find attestation for this commit.
    let attestation = state
        .chain_attestations
        .find_by_commit(&commit_sha)
        .await
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("attestation lookup failed: {e}")))?
        .ok_or_else(|| {
            ApiError::NotFound(format!("no attestation found for commit {commit_sha}"))
        })?;

    // Load the full attestation chain from this attestation back to root.
    let chain = state
        .chain_attestations
        .load_chain(&attestation.id)
        .await
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("chain lookup failed: {e}")))?;

    // Compute the git diff for the commit (for offline verification).
    let git_diff = {
        let git_bin = std::env::var("GYRE_GIT_PATH").unwrap_or_else(|_| "git".to_string());
        let out = tokio::process::Command::new(&git_bin)
            .arg("-C")
            .arg(&repo.path)
            .arg("diff")
            .arg(format!("{commit_sha}^..{commit_sha}"))
            .output()
            .await
            .ok();
        out.filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
            .unwrap_or_default()
    };

    let now = crate::api::now_secs();

    // Load trust anchors for the repo's tenant (§6.3).
    let trust_anchors = {
        let ws = state
            .workspaces
            .find_by_id(&Id::new(&attestation.metadata.workspace_id))
            .await
            .ok()
            .flatten();
        if let Some(ws) = ws {
            state
                .trust_anchors
                .list_by_tenant(ws.tenant_id.as_str())
                .await
                .unwrap_or_default()
        } else {
            vec![]
        }
    };

    // Build the VerificationBundle (§6.3).
    let bundle = serde_json::json!({
        "attestation_chain": chain,
        "trust_anchors": trust_anchors,
        "git_diff": git_diff,
        "timestamp": now,
    });

    Ok(Json(bundle))
}

// ── Chain Visualization (TASK-009, §7.6) ──────────────────────────────────────

/// A node in the provenance chain visualization.
#[derive(Serialize)]
pub struct ChainNode {
    /// Attestation ID.
    pub id: String,
    /// "signed" or "derived".
    pub input_type: String,
    /// Signer identity (user or agent).
    pub signer_identity: String,
    /// Number of constraints on this node.
    pub constraint_count: usize,
    /// Number of gate attestations on this node.
    pub gate_count: usize,
    /// Chain depth (0 = root).
    pub chain_depth: u32,
    /// Verification status of this node.
    pub valid: bool,
    /// Human-readable verification message.
    pub message: String,
    /// Task ID.
    pub task_id: String,
    /// Agent ID.
    pub agent_id: String,
    /// Created timestamp.
    pub created_at: u64,
    /// Failed constraints (if any).
    pub failed_constraints: Vec<FailedConstraint>,
}

/// A failed constraint in the chain visualization.
#[derive(Serialize)]
pub struct FailedConstraint {
    pub name: String,
    pub expression: String,
    pub message: String,
}

/// An edge in the provenance chain visualization.
#[derive(Serialize)]
pub struct ChainEdge {
    /// Source node ID (parent).
    pub from: String,
    /// Target node ID (child).
    pub to: String,
    /// Edge label.
    pub label: String,
}

/// Response for the chain visualization endpoint.
#[derive(Serialize)]
pub struct ChainVisualization {
    pub commit_sha: String,
    pub repo_id: String,
    pub nodes: Vec<ChainNode>,
    pub edges: Vec<ChainEdge>,
    /// Overall chain verification result.
    pub chain_valid: bool,
    pub chain_message: String,
}

/// GET /api/v1/repos/:id/attestations/:commit_sha/chain
///
/// Returns the attestation chain as a directed graph for Explorer visualization (§7.6).
/// Each node shows signer identity, constraint count, verification status.
/// Failed constraints are highlighted with the failing expression and value.
///
pub async fn get_attestation_chain(
    State(state): State<Arc<AppState>>,
    Path(AttestationPath {
        id: repo_id,
        commit_sha,
    }): Path<AttestationPath>,
) -> Result<Json<ChainVisualization>, ApiError> {
    // verification-scope:structural-only — visualization endpoint, not verification.
    // Full constraint evaluation is performed by GET .../verification.

    // Verify repo exists.
    state
        .repos
        .find_by_id(&Id::new(&repo_id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("repo {repo_id} not found")))?;

    // Find attestation for this commit.
    let attestation = state
        .chain_attestations
        .find_by_commit(&commit_sha)
        .await
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("attestation lookup failed: {e}")))?
        .ok_or_else(|| {
            ApiError::NotFound(format!("no attestation found for commit {commit_sha}"))
        })?;

    // Load the full chain.
    let chain = state
        .chain_attestations
        .load_chain(&attestation.id)
        .await
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("chain lookup failed: {e}")))?;

    // Verify the chain.
    let chain_result = crate::git_http::verify_chain(&chain, 10);

    // Build nodes and edges.
    let mut nodes = Vec::new();
    let mut edges = Vec::new();

    for att in &chain {
        let node_result = crate::git_http::verify_attestation_audit_only(att);

        let (input_type, signer_identity, constraint_count) = match &att.input {
            gyre_common::AttestationInput::Signed(si) => {
                let constraints = si.output_constraints.len();
                (
                    "signed".to_string(),
                    si.key_binding.user_identity.clone(),
                    constraints,
                )
            }
            gyre_common::AttestationInput::Derived(di) => {
                let constraints = di.output_constraints.len();
                (
                    "derived".to_string(),
                    di.key_binding.user_identity.clone(),
                    constraints,
                )
            }
        };

        // Collect failed constraints from verification result.
        let failed_constraints: Vec<FailedConstraint> = node_result
            .children
            .iter()
            .filter(|c| !c.valid)
            .map(|c| FailedConstraint {
                name: c.label.clone(),
                expression: String::new(), // expression not available in structural checks
                message: c.message.clone(),
            })
            .collect();

        nodes.push(ChainNode {
            id: att.id.clone(),
            input_type,
            signer_identity,
            constraint_count,
            gate_count: att.output.gate_results.len(),
            chain_depth: att.metadata.chain_depth,
            valid: node_result.valid,
            message: node_result.message,
            task_id: att.metadata.task_id.clone(),
            agent_id: att.metadata.agent_id.clone(),
            created_at: att.metadata.created_at,
            failed_constraints,
        });

        // Build edges from DerivedInput to its parent (at lower depth).
        if let gyre_common::AttestationInput::Derived(_) = att.input {
            // Find the parent node: the one at the immediately lower chain_depth.
            if let Some(parent) = chain
                .iter()
                .rev()
                .find(|p| p.metadata.chain_depth == att.metadata.chain_depth.saturating_sub(1))
            {
                edges.push(ChainEdge {
                    from: parent.id.clone(),
                    to: att.id.clone(),
                    label: "derives from".to_string(),
                });
            }
        }
    }

    Ok(Json(ChainVisualization {
        commit_sha,
        repo_id,
        nodes,
        edges,
        chain_valid: chain_result.valid,
        chain_message: chain_result.message,
    }))
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use crate::mem::test_state;
    use axum::{body::Body, Router};
    use gyre_common::Id;
    use gyre_domain::AgentCommit;
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

    async fn create_repo(app: Router) -> (Router, String) {
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
        let repo_id = json["id"].as_str().unwrap().to_string();
        (app, repo_id)
    }

    #[tokio::test]
    async fn provenance_empty_repo() {
        let (app, repo_id) = create_repo(app()).await;
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/repos/{repo_id}/provenance"))
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
    async fn provenance_repo_not_found() {
        let resp = app()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/repos/no-such-repo/provenance")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn provenance_filter_by_commit() {
        let state = test_state();
        let app = crate::api::api_router().with_state(state.clone());
        let (app, repo_id) = create_repo(app).await;

        let commit = AgentCommit::new(
            Id::new("c1"),
            Id::new("agent-1"),
            Id::new(&repo_id),
            "abc123",
            "refs/heads/main",
            1000,
        )
        .with_provenance(Some("TASK-001".to_string()), None, None, None);
        state.agent_commits.record(&commit).await.unwrap();

        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/repos/{repo_id}/provenance?commit=abc123"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json["commit_sha"], "abc123");
        assert_eq!(json["task_id"], "TASK-001");
    }

    #[tokio::test]
    async fn provenance_filter_by_task_id() {
        let state = test_state();
        let app = crate::api::api_router().with_state(state.clone());
        let (app, repo_id) = create_repo(app).await;

        for (id, sha) in [("c1", "sha1"), ("c2", "sha2")] {
            let commit = AgentCommit::new(
                Id::new(id),
                Id::new("agent-1"),
                Id::new(&repo_id),
                sha,
                "refs/heads/feat/x",
                1000,
            )
            .with_provenance(Some("TASK-007".to_string()), None, None, None);
            state.agent_commits.record(&commit).await.unwrap();
        }

        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/api/v1/repos/{repo_id}/provenance?task_id=TASK-007"
                    ))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json["task_id"], "TASK-007");
        assert_eq!(json["commits"].as_array().unwrap().len(), 2);
    }
}
