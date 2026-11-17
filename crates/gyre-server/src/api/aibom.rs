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
use crate::aibom_spec_registry::AibomSpecRegistry;

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
    /// Spec registry manifest state (spec-registry.md §Supply Chain):
    /// ledger statuses, approval chains, manifest version, and spec-to-code
    /// binding status.
    pub spec_registry: AibomSpecRegistry,
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
    let repo = state
        .repos
        .find_by_id(&Id::new(&repo_id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("repo {repo_id} not found")))?;

    // Spec registry manifest state (spec-registry.md §Supply Chain): every
    // ledger status, the approval chain for approved specs, the manifest
    // version, and spec-to-code binding status. Storage failures propagate —
    // an AIBOM must not silently under-report spec provenance.
    let spec_registry = crate::aibom_spec_registry::build_spec_registry_section(&state, &repo)
        .await
        .map_err(ApiError::Internal)?;

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
                    // Include the full attestation chain per commit (§7.3, supply-chain.md §5).
                    // The full chain is required for offline verification — a summary
                    // cannot be used to reconstruct or verify the attestation chain.
                    Some(serde_json::json!({
                        "attestation_id": att.id,
                        "chain_depth": att.metadata.chain_depth,
                        "chain": chain,
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
        aibom_version: "1.2".to_string(),
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
        spec_registry,
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
        assert_eq!(json["aibom_version"], "1.2");
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

    #[tokio::test]
    async fn aibom_includes_spec_registry_section() {
        let state = test_state();
        let app = crate::api::api_router().with_state(state.clone());
        let (app, repo_id) = create_repo(app).await;

        // Seed the spec ledger for this repo: one approved spec with an
        // approval event, one pending spec.
        let sha = format!("{:040}", 7);
        let entry =
            |path: &str, status: gyre_domain::ApprovalStatus| gyre_domain::SpecLedgerEntry {
                path: path.to_string(),
                title: format!("Spec {path}"),
                owner: "owner-1".to_string(),
                kind: None,
                current_sha: sha.clone(),
                approval_mode: "human_and_agent".to_string(),
                approval_status: status,
                linked_tasks: Vec::new(),
                linked_mrs: Vec::new(),
                drift_status: "clean".to_string(),
                created_at: 100,
                updated_at: 100,
                repo_id: Some(repo_id.clone()),
                workspace_id: None,
            };
        state
            .spec_ledger
            .save(&entry(
                "system/auth.md",
                gyre_domain::ApprovalStatus::Approved,
            ))
            .await
            .unwrap();
        state
            .spec_ledger
            .save(&entry(
                "system/wip.md",
                gyre_domain::ApprovalStatus::Pending,
            ))
            .await
            .unwrap();
        state
            .spec_approval_history
            .record(&gyre_domain::SpecApprovalEvent {
                id: "evt-1".to_string(),
                spec_path: "system/auth.md".to_string(),
                spec_sha: sha.clone(),
                approver_type: "human".to_string(),
                approver_id: "user:alice".to_string(),
                persona: None,
                attestation_level: None,
                stack_hash: None,
                approved_at: 1711324800,
                revoked_at: None,
                revoked_by: None,
                revocation_reason: None,
            })
            .await
            .unwrap();

        // Seed a merged MR with a spec_ref that WAS approved at merge time.
        let mut mr = gyre_domain::MergeRequest::new(
            Id::new("mr-aibom-1"),
            Id::new(&repo_id),
            Id::new("ws-1"),
            "feat: auth",
            "feat/auth",
            "main",
            10,
        );
        mr.spec_ref = Some(format!("system/auth.md@{sha}"));
        state.merge_requests.create(&mr).await.unwrap();
        let appr = gyre_domain::SpecApproval::new(
            Id::new("appr-aibom-1"),
            "system/auth.md",
            &sha,
            "user:alice",
        );
        state.spec_approvals.create(&appr).await.unwrap();
        state
            .spec_approvals
            .approve(&Id::new("appr-aibom-1"), 900)
            .await
            .expect("approve transition");
        mr.transition_status(gyre_domain::MrStatus::Approved)
            .unwrap();
        mr.transition_status(gyre_domain::MrStatus::Merged).unwrap();
        mr.updated_at = 1000;
        state.merge_requests.update(&mr).await.unwrap();

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

        // The spec_registry section carries the manifest state.
        let sr = &json["spec_registry"];
        assert_eq!(sr["total_specs"], 2);
        assert_eq!(sr["approved"], 1);
        assert_eq!(sr["pending"], 1);
        // No git directory behind the test repo → manifest version absent.
        assert!(sr["manifest_version"].is_null());

        let specs = sr["specs"].as_array().unwrap();
        assert_eq!(specs.len(), 2);
        let auth = specs
            .iter()
            .find(|s| s["path"] == "system/auth.md")
            .unwrap();
        assert_eq!(auth["status"], "approved");
        assert_eq!(auth["approved_at"], 1711324800);
        assert_eq!(auth["approved_by"], serde_json::json!(["user:alice"]));
        assert_eq!(auth["approval_chain"][0]["approver_id"], "user:alice");
        assert_eq!(auth["approval_chain"][0]["approved_at"], 1711324800);

        // Spec-to-code binding: the merged MR's pinned spec was approved
        // before the merge.
        let bindings = sr["spec_code_bindings"].as_array().unwrap();
        assert_eq!(bindings.len(), 1);
        assert_eq!(bindings[0]["mr_id"], "mr-aibom-1");
        assert_eq!(bindings[0]["approved_at_merge_time"], true);
        assert_eq!(sr["bindings_summary"]["approved_at_merge_time"], 1);
    }
}
