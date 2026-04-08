use axum::{
    extract::{Path, Query, State},
    Json,
};
use gyre_common::Id;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

use crate::AppState;

use super::error::ApiError;

// ── Validation ────────────────────────────────────────────────────────────────

/// Validate a git ref identifier (SHA hex or tag/branch name).
/// Allows: hex digits, alphanumeric, dots, hyphens, underscores, forward slashes.
/// Rejects: spaces, `--`, null bytes, shell metacharacters.
fn is_valid_ref(s: &str) -> bool {
    if s.is_empty() || s.len() > 255 {
        return false;
    }
    // Disallow double-dash (git flag injection) and leading dash.
    if s.starts_with('-') || s.contains("--") {
        return false;
    }
    s.chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '/'))
}

// ── Request / Response types ──────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct AibomQuery {
    pub from: Option<String>,
    pub to: Option<String>,
}

#[derive(Serialize)]
pub struct AibomRange {
    pub from: Option<String>,
    pub to: Option<String>,
}

#[derive(Serialize)]
pub struct AibomAgent {
    pub id: String,
    pub name: String,
    pub model: Option<String>,
    pub commit_count: usize,
    pub attestation_level: String,
}

#[derive(Serialize)]
pub struct AibomCommit {
    pub sha: String,
    pub agent_id: String,
    pub task_id: Option<String>,
    pub timestamp: u64,
    pub attestation_level: String,
    /// Full attestation chain for this commit (§7.3, Phase 4).
    /// Replaces the flat `stack_attestation` field with chain attestation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chain_attestation: Option<serde_json::Value>,
}

#[derive(Serialize)]
pub struct AibomResponse {
    pub aibom_version: String,
    pub repo_id: String,
    pub range: AibomRange,
    pub agents: Vec<AibomAgent>,
    pub commits: Vec<AibomCommit>,
    pub total_commits: usize,
    pub attested_percentage: f64,
    /// Total attestation chains found for commits in range.
    pub chain_attested_count: usize,
}

/// Resolve attestation level: prefer the stored value (M14.2), fall back to heuristic.
///
/// The heuristic now uses `model_context` (which carries `wl_*` JWT claims and
/// `stack_hash`) rather than the removed `ralph_step` field.
fn resolve_attestation_level(stored: Option<&str>, model_context: Option<&str>) -> &'static str {
    if let Some(lvl) = stored {
        match lvl {
            "server-verified" => return "server-verified",
            "self-reported" => return "self-reported",
            _ => {}
        }
    }
    // Heuristic: model_context present → agent used structured model context,
    // which carries stack_hash / wl_* JWT claims → self-reported at minimum.
    if model_context.is_some() {
        "server-verified"
    } else {
        "unattested"
    }
}

// ── Handler ───────────────────────────────────────────────────────────────────

/// GET /api/v1/repos/:id/aibom
///   ?from={tag_or_sha}  — lower bound commit SHA (optional)
///   ?to={tag_or_sha}    — upper bound commit SHA (optional)
pub async fn get_aibom(
    State(state): State<Arc<AppState>>,
    Path(repo_id): Path<String>,
    Query(params): Query<AibomQuery>,
) -> Result<Json<AibomResponse>, ApiError> {
    // Validate from/to ref names to prevent git flag injection (CISO).
    if let Some(ref from) = params.from {
        if !is_valid_ref(from) {
            return Err(ApiError::InvalidInput(
                "invalid 'from' ref: must be a valid SHA or tag name".to_string(),
            ));
        }
    }
    if let Some(ref to) = params.to {
        if !is_valid_ref(to) {
            return Err(ApiError::InvalidInput(
                "invalid 'to' ref: must be a valid SHA or tag name".to_string(),
            ));
        }
    }

    // Verify the repo exists.
    state
        .repos
        .find_by_id(&Id::new(&repo_id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("repo {repo_id} not found")))?;

    // Resolve timestamp bounds from from/to SHAs.
    let from_ts = if let Some(ref sha) = params.from {
        state
            .agent_commits
            .find_by_commit(sha)
            .await?
            .filter(|ac| ac.repository_id.as_str() == repo_id)
            .map(|ac| ac.timestamp)
    } else {
        None
    };

    let to_ts = if let Some(ref sha) = params.to {
        state
            .agent_commits
            .find_by_commit(sha)
            .await?
            .filter(|ac| ac.repository_id.as_str() == repo_id)
            .map(|ac| ac.timestamp)
    } else {
        None
    };

    // Load all commits for the repo, then filter by timestamp range if requested.
    let all_commits = state.agent_commits.find_by_repo(&Id::new(&repo_id)).await?;

    let commits: Vec<_> = all_commits
        .into_iter()
        .filter(|ac| {
            if let Some(lo) = from_ts {
                if ac.timestamp < lo {
                    return false;
                }
            }
            if let Some(hi) = to_ts {
                if ac.timestamp > hi {
                    return false;
                }
            }
            true
        })
        .collect();

    // Build per-agent summary.
    // agent_id -> (name, model, commit_count, any_server_verified, any_self_reported)
    let mut agent_map: HashMap<String, (String, Option<String>, usize, bool, bool)> =
        HashMap::new();

    for ac in &commits {
        let agent_id_str = ac.agent_id.to_string();
        let entry = agent_map.entry(agent_id_str.clone()).or_insert_with(|| {
            // Default name is the id; we'll update after the loop.
            (agent_id_str.clone(), None, 0, false, false)
        });
        entry.2 += 1;
        let lvl =
            resolve_attestation_level(ac.attestation_level.as_deref(), ac.model_context.as_deref());
        if lvl == "server-verified" {
            entry.3 = true;
        } else if lvl == "self-reported" {
            entry.4 = true;
        }
        // Prefer the model from the first commit that has it.
        if entry.1.is_none() {
            if let Some(ctx_str) = &ac.model_context {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(ctx_str) {
                    if let Some(model) = v.get("model").and_then(|m| m.as_str()) {
                        entry.1 = Some(model.to_string());
                    }
                }
            }
        }
    }

    // Resolve agent names via the agent repository.
    let mut aibom_agents: Vec<AibomAgent> = Vec::new();
    for (agent_id, (_, model, commit_count, has_server, has_self)) in &agent_map {
        let name = match state.agents.find_by_id(&Id::new(agent_id)).await {
            Ok(Some(a)) => a.name,
            _ => agent_id.clone(),
        };
        let level = if *has_server {
            "server-verified"
        } else if *has_self {
            "self-reported"
        } else {
            "unattested"
        };
        aibom_agents.push(AibomAgent {
            id: agent_id.clone(),
            name,
            model: model.clone(),
            commit_count: *commit_count,
            attestation_level: level.to_string(),
        });
    }
    // Sort agents by commit_count descending for consistent output.
    aibom_agents.sort_by(|a, b| b.commit_count.cmp(&a.commit_count));

    // Build per-commit list with chain attestation data (§7.3, Phase 4).
    let mut chain_attested_count = 0usize;
    let mut aibom_commits: Vec<AibomCommit> = Vec::with_capacity(commits.len());
    for ac in &commits {
        let level =
            resolve_attestation_level(ac.attestation_level.as_deref(), ac.model_context.as_deref());

        // Look up chain attestation for this commit.
        let chain_attestation = match state
            .chain_attestations
            .find_by_commit(&ac.commit_sha)
            .await
        {
            Ok(Some(att)) => {
                // Load the full chain for this attestation.
                let chain = state
                    .chain_attestations
                    .load_chain(&att.id)
                    .await
                    .unwrap_or_default();
                if !chain.is_empty() {
                    chain_attested_count += 1;
                    // Include chain summary (not the full chain to keep response size manageable).
                    Some(serde_json::json!({
                        "attestation_id": att.id,
                        "chain_depth": att.metadata.chain_depth,
                        "has_signed_input": chain.iter().any(|a| matches!(a.input, gyre_common::AttestationInput::Signed(_))),
                        "constraint_count": chain.iter().flat_map(|a| match &a.input {
                            gyre_common::AttestationInput::Signed(si) => si.output_constraints.clone(),
                            gyre_common::AttestationInput::Derived(di) => di.output_constraints.clone(),
                        }).count(),
                        "gate_attestation_count": chain.iter().flat_map(|a| a.output.gate_results.iter()).count(),
                        "chain_node_count": chain.len(),
                    }))
                } else {
                    None
                }
            }
            _ => None,
        };

        aibom_commits.push(AibomCommit {
            sha: ac.commit_sha.clone(),
            agent_id: ac.agent_id.to_string(),
            task_id: ac.task_id.clone(),
            timestamp: ac.timestamp,
            attestation_level: level.to_string(),
            chain_attestation,
        });
    }

    let total = aibom_commits.len();
    let attested = aibom_commits
        .iter()
        .filter(|c| c.attestation_level != "unattested")
        .count();
    let attested_percentage = if total == 0 {
        0.0
    } else {
        (attested as f64 / total as f64) * 100.0
    };

    Ok(Json(AibomResponse {
        aibom_version: "1.1".to_string(),
        repo_id: repo_id.clone(),
        range: AibomRange {
            from: params.from,
            to: params.to,
        },
        agents: aibom_agents,
        commits: aibom_commits,
        total_commits: total,
        attested_percentage,
        chain_attested_count,
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
    async fn aibom_empty_repo() {
        let (app, repo_id) = create_repo(app()).await;
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/repos/{repo_id}/aibom"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json["aibom_version"], "1.1");
        assert_eq!(json["total_commits"], 0);
        assert_eq!(json["attested_percentage"], 0.0);
        assert!(json["agents"].as_array().unwrap().is_empty());
        assert!(json["commits"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn aibom_repo_not_found() {
        let resp = app()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/repos/no-such-repo/aibom")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn aibom_with_commits() {
        let state = test_state();
        let app = crate::api::api_router().with_state(state.clone());
        let (app, repo_id) = create_repo(app).await;

        // Record two commits: one with model_context (server-verified), one without (unattested).
        let c1 = AgentCommit::new(
            Id::new("c1"),
            Id::new("agent-1"),
            Id::new(&repo_id),
            "sha-abc",
            "refs/heads/feat/x",
            1000,
        )
        .with_provenance(Some("TASK-001".to_string()), None, None, None);
        let c2 = AgentCommit::new(
            Id::new("c2"),
            Id::new("agent-1"),
            Id::new(&repo_id),
            "sha-def",
            "refs/heads/feat/x",
            2000,
        )
        .with_provenance(None, None, None, None);
        state.agent_commits.record(&c1).await.unwrap();
        state.agent_commits.record(&c2).await.unwrap();

        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/repos/{repo_id}/aibom"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json["total_commits"], 2);
        // Both unattested (no model_context) → 0% attested
        assert_eq!(json["attested_percentage"], 0.0);
        assert_eq!(json["agents"].as_array().unwrap().len(), 1);
        let agent = &json["agents"][0];
        assert_eq!(agent["commit_count"], 2);
        assert_eq!(agent["attestation_level"], "unattested");
    }

    #[tokio::test]
    async fn aibom_attestation_levels() {
        let state = test_state();
        let app = crate::api::api_router().with_state(state.clone());
        let (app, repo_id) = create_repo(app).await;

        // server-verified: has model_context
        let c1 = AgentCommit::new(
            Id::new("cv1"),
            Id::new("agent-a"),
            Id::new(&repo_id),
            "sha-111",
            "refs/heads/main",
            1000,
        )
        .with_provenance(
            None,
            None,
            None,
            Some(r#"{"model":"claude-opus-4"}"#.to_string()),
        );
        state.agent_commits.record(&c1).await.unwrap();

        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/repos/{repo_id}/aibom"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        let commits = json["commits"].as_array().unwrap();
        assert_eq!(commits[0]["attestation_level"], "server-verified");
        assert_eq!(json["attested_percentage"], 100.0);
        let agents = json["agents"].as_array().unwrap();
        assert_eq!(agents[0]["model"], "claude-opus-4");
        assert_eq!(agents[0]["attestation_level"], "server-verified");
    }
}
