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
/// associated with the given commit SHA. Implements the complete §6.2
/// verification algorithm (5 phases):
///   Phase 1: Verify the input chain (structural + crypto)
///   Phase 2: Collect all constraints (explicit + strategy-implied + gate)
///   Phase 3: Build CEL context from actual output (diff)
///   Phase 4: Evaluate all constraints
///   Phase 5: Verify output signatures
/// ABAC: resource_type = attestation, action = read.
pub async fn get_verification(
    State(state): State<Arc<AppState>>,
    Path(AttestationPath {
        id: repo_id,
        commit_sha,
    }): Path<AttestationPath>,
) -> Result<Json<serde_json::Value>, ApiError> {
    // Verify repo exists and get repo data (needed for path + default_branch).
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

    // Load the full chain for comprehensive verification (§4.4, §6.2).
    let chain = state
        .chain_attestations
        .load_chain(&attestation.id)
        .await
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("chain lookup failed: {e}")))?;

    // Get workspace-configurable max depth (default 10).
    let max_depth = 10u32; // Will be configurable per workspace in future

    // ── Phase 1: Verify the input chain (structural + crypto) ──
    let chain_result = crate::git_http::verify_chain(&chain, max_depth);

    // If chain structure is invalid, return early with the failure.
    if !chain_result.valid {
        return Ok(Json(serde_json::json!({
            "commit_sha": commit_sha,
            "repo_id": repo_id,
            "attestation_id": attestation.id,
            "chain_depth": chain.len(),
            "verification": chain_result,
            "constraint_results": null,
        })));
    }

    // ── Phase 2: Collect all constraints ──
    // Find root SignedInput.
    let signed_input = chain.iter().find_map(|att| match &att.input {
        gyre_common::AttestationInput::Signed(si) => Some(si),
        _ => None,
    });

    // Accumulate constraints from the full chain (explicit + gate).
    let (_root, explicit_from_chain, gate_from_chain) =
        crate::git_http::accumulate_chain_constraints(&chain);

    // Derive strategy-implied constraints from workspace config.
    let workspace = state
        .workspaces
        .find_by_id(&Id::new(&attestation.metadata.workspace_id))
        .await
        .ok()
        .flatten();
    let trust_level = workspace
        .as_ref()
        .map(|ws| format!("{:?}", ws.trust_level).to_lowercase());

    let mut strategy_constraints = if let Some(si) = signed_input {
        gyre_domain::constraint_evaluator::derive_strategy_constraints(
            &si.content,
            trust_level.as_deref(),
            None,
        )
    } else {
        vec![]
    };

    // ── Phase 3: Build CEL context from actual output ──
    // Build agent context.
    let agent_ctx = crate::constraint_check::build_agent_context(
        &state,
        &attestation.metadata.agent_id,
        &attestation.metadata.task_id,
        &Id::new(&attestation.metadata.workspace_id),
    )
    .await;

    // Guard: remove attestation-level constraints when the agent's level is unknown.
    if agent_ctx.attestation_level == 0 {
        strategy_constraints.retain(|c| !c.expression.contains("agent.attestation_level"));
    }

    // Collect all constraints.
    let all_constraints = gyre_domain::constraint_evaluator::collect_all_constraints(
        &explicit_from_chain,
        &strategy_constraints,
        &gate_from_chain,
    );

    // Compute the diff for the commit.
    let diff_info = crate::constraint_check::compute_commit_diff(&repo.path, &commit_sha).await;

    // Build target context.
    let target_ctx = gyre_domain::constraint_evaluator::TargetContext {
        repo_id: repo_id.clone(),
        workspace_id: attestation.metadata.workspace_id.clone(),
        branch: String::new(), // not available from attestation alone
        default_branch: repo.default_branch.clone(),
    };

    // ── Phase 4: Evaluate all constraints ──
    let constraint_result = if !all_constraints.is_empty() {
        if let Some(ref diff) = diff_info {
            if let Some(si) = signed_input {
                let ci = gyre_domain::constraint_evaluator::ConstraintInput {
                    input: &si.content,
                    output: diff,
                    agent: &agent_ctx,
                    target: &target_ctx,
                    action: gyre_domain::constraint_evaluator::Action::Push,
                };
                match gyre_domain::constraint_evaluator::build_cel_context(&ci) {
                    Ok(ctx) => {
                        let eval_result =
                            gyre_domain::constraint_evaluator::evaluate_all(&all_constraints, &ctx);
                        Some(eval_result)
                    }
                    Err(e) => Some(gyre_common::VerificationResult {
                        valid: false,
                        label: "constraint_evaluation".to_string(),
                        message: format!("failed to build CEL context: {e}"),
                        children: vec![],
                    }),
                }
            } else {
                Some(gyre_common::VerificationResult {
                    valid: false,
                    label: "constraint_evaluation".to_string(),
                    message: "no SignedInput found in chain — cannot evaluate constraints"
                        .to_string(),
                    children: vec![],
                })
            }
        } else {
            Some(gyre_common::VerificationResult {
                valid: false,
                label: "constraint_evaluation".to_string(),
                message: "could not compute commit diff for constraint evaluation".to_string(),
                children: vec![],
            })
        }
    } else {
        // No constraints to evaluate — pass.
        Some(gyre_common::VerificationResult {
            valid: true,
            label: "constraint_evaluation".to_string(),
            message: "no constraints to evaluate".to_string(),
            children: vec![],
        })
    };

    // ── Phase 5: Output signature verification (§6.2) ──
    // Verify agent_signature and gate result signatures for each attestation
    // in the chain. verify_chain only checks INPUT signatures (SignedInput,
    // DerivedInput); this phase checks OUTPUT signatures.
    let output_sig_result = verify_output_signatures(&chain);

    // Combine Phase 1 (chain structure), Phase 4 (constraint evaluation),
    // and Phase 5 (output signatures) into the overall verification result.
    let overall_valid = chain_result.valid
        && constraint_result.as_ref().map_or(true, |r| r.valid)
        && output_sig_result.valid;
    let overall_message = if overall_valid {
        "all verification phases passed".to_string()
    } else if !chain_result.valid {
        format!("chain verification failed: {}", chain_result.message)
    } else if !output_sig_result.valid {
        format!(
            "output signature verification failed: {}",
            output_sig_result.message
        )
    } else {
        constraint_result
            .as_ref()
            .map(|r| format!("constraint evaluation failed: {}", r.message))
            .unwrap_or_else(|| "constraint evaluation incomplete".to_string())
    };

    let overall = gyre_common::VerificationResult {
        valid: overall_valid,
        label: "full_verification".to_string(),
        message: overall_message,
        children: {
            let mut children = vec![chain_result];
            if let Some(cr) = constraint_result {
                children.push(cr);
            }
            children.push(output_sig_result);
            children
        },
    };

    Ok(Json(serde_json::json!({
        "commit_sha": commit_sha,
        "repo_id": repo_id,
        "attestation_id": attestation.id,
        "chain_depth": chain.len(),
        "constraints_evaluated": all_constraints.len(),
        "verification": overall,
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

// ── Phase 5: Output signature verification (§6.2) ───────────────────────────

/// Verify output signatures for all attestations in a chain (§6.2 Phase 5).
///
/// For each attestation:
///   (a) If `attestation.output.agent_signature` is not None, verify it against
///       `attestation.output.content_hash` using the agent's key binding public key.
///   (b) For each gate result, verify `gate.signature` against the canonical
///       signable bytes (via `GateAttestation::signable_bytes()`) using
///       `gate.key_binding.public_key`.
fn verify_output_signatures(chain: &[gyre_common::Attestation]) -> gyre_common::VerificationResult {
    use ring::signature::{self, UnparsedPublicKey};

    let mut children = Vec::new();
    let mut all_valid = true;

    for (i, att) in chain.iter().enumerate() {
        // (a) Verify agent_signature if present.
        if let Some(ref agent_sig) = att.output.agent_signature {
            // The agent signs over the content_hash. The key binding is from
            // the attestation's input (the agent that produced the output).
            let agent_pub_key = match &att.input {
                gyre_common::AttestationInput::Signed(si) => &si.key_binding.public_key,
                gyre_common::AttestationInput::Derived(di) => {
                    // For derived inputs, the key_binding belongs to the spawner.
                    // The agent's own key is not directly in the attestation input.
                    // Agent signature verification requires the agent's public key
                    // which is stored separately. For now, use the key binding from
                    // the input — if the agent signed with a different key, this will
                    // correctly fail verification.
                    &di.key_binding.public_key
                }
            };

            let peer_key = UnparsedPublicKey::new(&signature::ED25519, agent_pub_key);
            let sig_valid = peer_key.verify(&att.output.content_hash, agent_sig).is_ok();

            if !sig_valid {
                all_valid = false;
            }

            children.push(gyre_common::VerificationResult {
                label: format!("node[{}].agent_signature", i),
                valid: sig_valid,
                message: if sig_valid {
                    "agent output signature verified against content_hash".to_string()
                } else {
                    "agent output signature verification FAILED".to_string()
                },
                children: vec![],
            });
        }

        // (b) Verify each gate result signature using the shared signable_bytes()
        // helper to ensure sign/verify message parity (checklist §44).
        for (gi, gate) in att.output.gate_results.iter().enumerate() {
            let peer_key =
                UnparsedPublicKey::new(&signature::ED25519, &gate.key_binding.public_key);
            let sign_bytes = gate.signable_bytes();
            let sig_valid = peer_key.verify(&sign_bytes, &gate.signature).is_ok();

            if !sig_valid {
                all_valid = false;
            }

            children.push(gyre_common::VerificationResult {
                label: format!("node[{}].gate[{}].signature", i, gi),
                valid: sig_valid,
                message: if sig_valid {
                    format!(
                        "gate '{}' signature verified against output_hash",
                        gate.gate_name
                    )
                } else {
                    format!("gate '{}' signature verification FAILED", gate.gate_name)
                },
                children: vec![],
            });
        }
    }

    gyre_common::VerificationResult {
        label: "output_signature_verification".to_string(),
        valid: all_valid,
        message: if children.is_empty() {
            "no output signatures to verify".to_string()
        } else if all_valid {
            format!("all {} output signature(s) verified", children.len())
        } else {
            "one or more output signature verifications failed".to_string()
        },
        children,
    }
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

    /// Round-trip sign-then-verify test for gate attestation signatures.
    ///
    /// Signs a GateAttestation using the same code path as gate_executor,
    /// then verifies it using verify_output_signatures. This catches
    /// sign/verify message mismatch (checklist §44, review F6).
    #[tokio::test]
    async fn gate_signature_round_trip_sign_verify() {
        use gyre_common::{
            gate::{GateStatus, GateType},
            Attestation, AttestationInput, AttestationMetadata, AttestationOutput, GateAttestation,
            InputContent, KeyBinding, ScopeConstraint, SignedInput,
        };
        use sha2::{Digest, Sha256};

        let signing_key = crate::auth::AgentSigningKey::generate();

        // Build a GateAttestation with a real signature via signable_bytes().
        let output_hash = Sha256::digest(b"gate output content").to_vec();
        let key_binding = KeyBinding {
            public_key: signing_key.public_key_bytes.clone(),
            user_identity: "gate-agent:test-gate".to_string(),
            issuer: "http://localhost:3000".to_string(),
            trust_anchor_id: "gyre-platform".to_string(),
            issued_at: 1000,
            expires_at: 9999,
            user_signature: vec![],
            platform_countersign: vec![],
        };

        let mut gate_att = GateAttestation {
            gate_id: "gate-1".to_string(),
            gate_name: "test-gate".to_string(),
            gate_type: GateType::AgentReview,
            status: GateStatus::Passed,
            output_hash,
            constraint: None,
            signature: vec![],
            key_binding: key_binding.clone(),
        };

        // Sign using signable_bytes() — same path as gate_executor.
        let sign_bytes = gate_att.signable_bytes();
        gate_att.signature = signing_key.sign_bytes(&sign_bytes);

        // Wrap in an Attestation so verify_output_signatures can process it.
        let attestation = Attestation {
            id: "att-1".to_string(),
            input: AttestationInput::Signed(SignedInput {
                content: InputContent {
                    spec_path: "specs/test.md".to_string(),
                    spec_sha: "abc123".to_string(),
                    workspace_id: "ws-1".to_string(),
                    repo_id: "repo-1".to_string(),
                    persona_constraints: vec![],
                    meta_spec_set_sha: String::new(),
                    scope: ScopeConstraint {
                        allowed_paths: vec![],
                        forbidden_paths: vec![],
                    },
                },
                output_constraints: vec![],
                signature: vec![],
                key_binding: key_binding.clone(),
                valid_until: 9999,
                expected_generation: None,
            }),
            output: AttestationOutput {
                content_hash: vec![0u8; 32],
                commit_sha: "abc123def".to_string(),
                agent_signature: None,
                gate_results: vec![gate_att],
            },
            metadata: AttestationMetadata {
                created_at: 1000,
                workspace_id: "ws-1".to_string(),
                repo_id: "repo-1".to_string(),
                task_id: "TASK-TEST".to_string(),
                agent_id: "agent-1".to_string(),
                chain_depth: 0,
            },
        };

        // Verify — should succeed because sign and verify use the same message.
        let result = super::verify_output_signatures(&[attestation.clone()]);
        assert!(
            result.valid,
            "Gate signature round-trip verification must succeed: {}",
            result.message
        );
        assert_eq!(result.children.len(), 1);
        assert!(result.children[0].valid);

        // Mutate gate_name and verify it fails — proves verification is meaningful.
        let mut mutated = attestation;
        mutated.output.gate_results[0].gate_name = "tampered-name".to_string();
        let bad_result = super::verify_output_signatures(&[mutated]);
        assert!(
            !bad_result.valid,
            "Tampered gate attestation must fail verification"
        );
        assert!(!bad_result.children[0].valid);
    }
}
