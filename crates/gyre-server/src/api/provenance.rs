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
/// verification algorithm (6 phases):
///   Phase 1: Verify the input chain (structural + crypto)
///   Phase 2: Collect all constraints (explicit + strategy-implied + gate)
///   Phase 3: Build CEL context from actual output (diff)
///   Phase 4: Evaluate all constraints
///   Phase 5: Verify output signatures
///   Phase 6: Verify the jj-squash commit signature (task-107, Layer 3)
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
    // Accumulate constraints from the full chain (explicit + gate); the
    // accumulator also surfaces the root SignedInput (root-first chain
    // ordering), replacing a separate chain walk here.
    let chain_constraints = crate::git_http::accumulate_chain_constraints(&chain);
    let signed_input = chain_constraints.root_input.as_ref();

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

    let required_attestation_level =
        crate::constraint_check::get_repo_required_attestation_level(&state, &repo_id).await;
    let strategy_constraints = if let Some(si) = signed_input {
        gyre_domain::constraint_evaluator::derive_strategy_constraints(
            &si.content,
            trust_level.as_deref(),
            required_attestation_level,
        )
    } else {
        vec![]
    };

    // ── Phase 3: Build CEL context from actual output ──
    // Build agent context. Fail closed: an unresolvable workspace required
    // meta-spec set must not evaluate §3.2 constraints against a fabricated
    // empty agent SHA — the verification reports the error instead of
    // silently passing an equality it could not evaluate.
    let agent_ctx = crate::constraint_check::build_agent_context(
        &state,
        &attestation.metadata.agent_id,
        &attestation.metadata.task_id,
        &Id::new(&attestation.metadata.workspace_id),
    )
    .await
    .map_err(|e| {
        ApiError::Internal(anyhow::anyhow!(
            "failed to resolve agent context (workspace meta-spec set): {e}"
        ))
    })?;

    // Collect all constraints.
    let all_constraints = gyre_domain::constraint_evaluator::collect_all_constraints(
        &chain_constraints.explicit_constraints,
        &strategy_constraints,
        &chain_constraints.gate_constraints,
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

    // ── Phase 6: Commit signature verification (task-107, identity-security.md
    // §Layer 3) ──
    // Surface the jj-squash commit signature status in the provenance chain
    // API. Unsigned commits (e.g. GYRE_SIGNING_MODE=none) are reported as a
    // neutral node, not a failure; a stored signature that fails to verify
    // fails the overall result.
    let commit_sig_node =
        match crate::sigstore::verify_state_commit_signature(&state, &repo_id, &commit_sha).await {
            Ok(Some(result)) => gyre_common::VerificationResult {
                valid: result.valid,
                label: "commit_signature".to_string(),
                message: if result.valid {
                    format!(
                        "commit signature verified (mode={:?}, signer={}, task={}, spawned_by={})",
                        result.sigstore_mode, result.signer_id, result.task_id, result.spawned_by
                    )
                } else {
                    format!(
                        "commit signature verification FAILED (mode={:?}): {}",
                        result.sigstore_mode,
                        result.reason.as_deref().unwrap_or("unknown reason")
                    )
                },
                children: vec![],
            },
            Ok(None) => gyre_common::VerificationResult {
                valid: true,
                label: "commit_signature".to_string(),
                message: "no commit signature record for this commit (unsigned)".to_string(),
                children: vec![],
            },
            Err(e) => gyre_common::VerificationResult {
                valid: false,
                label: "commit_signature".to_string(),
                message: format!("commit signature lookup failed: {e}"),
                children: vec![],
            },
        };

    // Combine Phase 1 (chain structure), Phase 4 (constraint evaluation),
    // Phase 5 (output signatures), and Phase 6 (commit signature) into the
    // overall verification result.
    let overall_valid = chain_result.valid
        && constraint_result.as_ref().is_none_or(|r| r.valid)
        && output_sig_result.valid
        && commit_sig_node.valid;
    let overall_message = if overall_valid {
        "all verification phases passed".to_string()
    } else if !chain_result.valid {
        format!("chain verification failed: {}", chain_result.message)
    } else if !output_sig_result.valid {
        format!(
            "output signature verification failed: {}",
            output_sig_result.message
        )
    } else if !commit_sig_node.valid {
        format!(
            "commit signature verification failed: {}",
            commit_sig_node.message
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
            children.push(commit_sig_node);
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

// ── SLSA provenance mapping (supply-chain.md §SLSA Provenance Integration) ───

#[derive(Deserialize)]
pub struct SlsaProvenanceQuery {
    /// Ref (SHA, tag, or branch) the statement attests to. Defaults to the
    /// repo's default branch HEAD.
    #[serde(rename = "ref")]
    pub git_ref: Option<String>,
}

/// Map a Gyre attestation level (supply-chain.md §Attestation Levels) to a
/// SLSA Build Level:
/// - Level 1 (raw git push, no attestation) → SLSA 0 — no provenance.
/// - Level 2 (Gyre CLI, signed self-report) → SLSA 1–2: authenticated
///   provenance; the repo's committed `gyre-stack.lock` supplies the
///   version-controlled build process, so Level 2 maps to SLSA 2 when the
///   release ref pins a lockfile and SLSA 1 otherwise.
/// - Level 3 (Gyre-managed runtime) → SLSA 3 (hermetic, infrastructure-
///   attested build environment).
fn gyre_level_to_slsa(gyre_level: u8, lockfile_pinned: bool) -> u8 {
    match gyre_level {
        3 => 3,
        2 => {
            if lockfile_pinned {
                2
            } else {
                1
            }
        }
        _ => 0,
    }
}

/// GET /api/v1/repos/:id/slsa-provenance
///   ?ref={sha_or_tag} — the ref the statement attests to (default: the
///                     repo's default branch HEAD)
///
/// Maps Gyre's provenance data to the SLSA provenance attestation format
/// (in-toto Statement v1, predicateType https://slsa.dev/provenance/v1).
/// The subject is the resolved ref's commit; `predicate.internalParameters`
/// carries the Gyre-specific mapping — per-commit agent attribution with
/// Gyre attestation levels and their SLSA Build Level equivalents, and the
/// overall SLSA Build Level of the subject (the weakest link: the minimum
/// across all agent-attributed commits reachable from the subject ref,
/// since one unattested commit caps what the artifact can claim). Commits
/// on unrelated or deleted branches are not part of the subject's
/// provenance and are excluded from the attribution and the level.
pub async fn get_slsa_provenance(
    State(state): State<Arc<AppState>>,
    Path(repo_id): Path<String>,
    Query(params): Query<SlsaProvenanceQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let repo = state
        .repos
        .find_by_id(&Id::new(&repo_id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("repo {repo_id} not found")))?;

    // Validate the caller-supplied ref against git flag injection.
    if let Some(r) = &params.git_ref {
        if !super::aibom::is_valid_ref(r) {
            return Err(ApiError::InvalidInput(
                "invalid 'ref': must be a valid SHA or tag name".to_string(),
            ));
        }
    }

    let git_bin = std::env::var("GYRE_GIT_PATH").unwrap_or_else(|_| "git".to_string());
    let ref_used = params
        .git_ref
        .clone()
        .unwrap_or_else(|| format!("refs/heads/{}", repo.default_branch));
    let sha = super::aibom::git_rev_parse(&git_bin, &repo.path, &ref_used)
        .await?
        .ok_or_else(|| {
            ApiError::NotFound(format!("cannot resolve ref '{ref_used}' in repo {repo_id}"))
        })?;

    // A committed gyre-stack.lock at the subject ref supplies the
    // version-controlled build process (SLSA 2 requirement) for Level-2
    // (CLI) contributions.
    let lockfile_pinned =
        crate::spec_registry::read_git_file(&git_bin, &repo.path, &sha, "gyre-stack.lock")
            .await
            .is_some_and(|content| {
                gyre_domain::stack::StackLockfile::parse(&content)
                    .map(|l| l.verify_integrity().is_ok())
                    .unwrap_or(false)
            });

    // The statement attests the subject's history, not the repo: scope the
    // agent-commit attribution to commits reachable from the subject ref
    // (rev-list of the resolved SHA). A record targeting a SHA on an
    // unrelated or deleted branch would otherwise appear in the per-commit
    // attribution and cap the subject's SLSA Build Level.
    let history = super::aibom::git_rev_list(&git_bin, &repo.path, &sha)
        .await?
        .ok_or_else(|| {
            ApiError::Internal(anyhow::anyhow!(
                "cannot enumerate commits reachable from '{sha}' in repo {repo_id}"
            ))
        })?;
    let history_set: std::collections::HashSet<&str> = history.iter().map(String::as_str).collect();

    // Per-commit agent attribution with Gyre level → SLSA Build Level.
    let commits = state
        .agent_commits
        .find_by_repo(&repo.id)
        .await
        .map_err(ApiError::Internal)?;
    let mut per_commit: Vec<serde_json::Value> = Vec::new();
    let mut slsa_build_level: Option<u8> = None;
    let mut earliest_ts: Option<u64> = None;
    for ac in &commits {
        if !history_set.contains(ac.commit_sha.as_str()) {
            continue;
        }
        let gyre_level =
            gyre_domain::stack::attestation_level_from_label(ac.attestation_level.as_deref());
        let commit_slsa = gyre_level_to_slsa(gyre_level, lockfile_pinned);
        slsa_build_level = Some(match slsa_build_level {
            Some(min) => min.min(commit_slsa),
            None => commit_slsa,
        });
        earliest_ts = Some(match earliest_ts {
            Some(t) => t.min(ac.timestamp),
            None => ac.timestamp,
        });
        per_commit.push(serde_json::json!({
            "commit_sha": ac.commit_sha,
            "agent_id": ac.agent_id.to_string(),
            "gyre_attestation_level": gyre_level,
            "attestation_level": ac.attestation_level,
            "slsa_build_level": commit_slsa,
        }));
    }
    // No agent-attributed commits in the subject's history → nothing
    // provenance-bearing to claim beyond SLSA 0.
    let slsa_build_level = slsa_build_level.unwrap_or(0);

    let repo_name = repo.name.clone();
    let workspace_uri = format!(
        "git+gyre://{workspace}/{repo_name}",
        workspace = repo.workspace_id
    );

    let statement = serde_json::json!({
        "_type": "https://in-toto.io/Statement/v1",
        "subject": [{
            "name": repo_name,
            "digest": { "gitCommit": sha },
        }],
        "predicateType": "https://slsa.dev/provenance/v1",
        "predicate": {
            "buildDefinition": {
                "buildType": "https://gyre.dev/buildtypes/agent-contribution/v1",
                "externalParameters": {
                    "repository": repo_id,
                    "ref": ref_used,
                },
                "internalParameters": {
                    "gyre": {
                        "slsaBuildLevel": slsa_build_level,
                        "lockfilePinned": lockfile_pinned,
                        "levelMapping": {
                            "gyre_level_1_raw_push": 0,
                            "gyre_level_2_cli": if lockfile_pinned { 2 } else { 1 },
                            "gyre_level_3_managed": 3,
                        },
                        "commits": per_commit,
                    },
                },
                "resolvedDependencies": [{
                    "uri": workspace_uri,
                    "digest": { "gitCommit": sha },
                }],
            },
            "runDetails": {
                "builder": {
                    "id": format!("https://gyre.dev/forge/{repo_id}"),
                },
                "metadata": {
                    "invocationId": format!("urn:gyre:slsa-provenance:{repo_id}:{sha}"),
                    "startedOn": earliest_ts
                        .map(gyre_domain::decisions::format_iso8601_utc),
                },
            },
        },
    });

    Ok(Json(statement))
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

    /// `create_repo` with an explicit repo name (unique per call so parallel
    /// git-backed tests never share a git dir under the shared `repos_root`).
    async fn create_repo_named(app: Router, name: &str) -> (Router, String) {
        let body = serde_json::json!({"workspace_id": "ws-1", "name": name});
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
        assert_eq!(resp.status(), StatusCode::CREATED);
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

    // ── SLSA provenance mapping (supply-chain.md §SLSA Provenance) ────────

    /// Real-git setup: an API-created repo record (the API's `init_bare` is
    /// NoopGitOps under `test_state`, so the bare git dir + initial commit
    /// are created here) plus `extra` file commits, returning the repo id,
    /// repo path, and final HEAD SHA. The repo name is unique per call —
    /// parallel tests in this binary share `repos_root` (keyed by pid), so
    /// same-named repos would collide on the same git dir.
    async fn git_repo_with_commits(
        extra: &[(&str, String)],
    ) -> (
        Router,
        std::sync::Arc<crate::AppState>,
        String,
        String,
        String,
    ) {
        let state = test_state();
        let app = crate::api::api_router().with_state(state.clone());
        let repo_name = format!("slsa-{}", uuid::Uuid::new_v4().simple());
        let (app, repo_id) = create_repo_named(app, &repo_name).await;
        let repo = state
            .repos
            .find_by_id(&Id::new(&repo_id))
            .await
            .unwrap()
            .expect("repo record");
        let repo_path = repo.path.clone();

        // Real bare repo.
        std::fs::create_dir_all(&repo_path).unwrap();
        let out = std::process::Command::new("git")
            .args(["init", "--bare", &repo_path])
            .output()
            .unwrap();
        assert!(out.status.success(), "git init --bare failed");

        let run_git = |args: &[&str]| {
            let out = std::process::Command::new("git")
                .arg("-C")
                .arg(&repo_path)
                .args(args)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "git {args:?} failed: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        };

        // Initial (parentless) commit on an empty tree.
        let child = std::process::Command::new("git")
            .arg("-C")
            .arg(&repo_path)
            .arg("mktree")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let empty_out = child.wait_with_output().unwrap();
        assert!(
            empty_out.status.success(),
            "mktree (empty tree) failed: {}",
            String::from_utf8_lossy(&empty_out.stderr)
        );
        let empty_tree = String::from_utf8_lossy(&empty_out.stdout)
            .trim()
            .to_string();
        let mut head = run_git(&[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "commit-tree",
            &empty_tree,
            "-m",
            "chore: init",
        ]);
        for (path, content) in extra {
            let blob = write_blob(&repo_path, content);
            // `mktree` is single-level: nested paths (e.g. `src/a.rs`) need
            // intermediate trees, and a failure here must never surface as
            // an opaque bare `commit-tree failed` one step later.
            let tree = write_tree(&repo_path, &[(path, blob)]);
            head = run_git(&[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.com",
                "commit-tree",
                &tree,
                "-p",
                &head,
                "-m",
                "test commit",
            ]);
        }
        run_git(&["update-ref", "refs/heads/main", &head]);

        (app, state, repo_id, repo_path, head)
    }

    /// Write a blob into the repo's object store, returning its SHA.
    /// Panics with git's stderr on failure — an unchecked failure here
    /// otherwise surfaces later as an opaque bare `commit-tree failed`.
    fn write_blob(repo_path: &str, content: &str) -> String {
        use std::io::Write as _;
        let mut child = std::process::Command::new("git")
            .arg("-C")
            .arg(repo_path)
            .args(["hash-object", "-w", "--stdin"])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .as_mut()
            .unwrap()
            .write_all(content.as_bytes())
            .unwrap();
        let out = child.wait_with_output().unwrap();
        assert!(
            out.status.success(),
            "hash-object failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    /// Build a tree object from (path, blob-sha) entries, creating
    /// intermediate trees for nested paths (`mktree` is single-level and
    /// rejects paths containing slashes). Panics with git's stderr on
    /// failure.
    fn write_tree(repo_path: &str, files: &[(&str, String)]) -> String {
        use std::io::Write as _;
        let mut lines: Vec<String> = Vec::new();
        let mut subdirs: std::collections::BTreeMap<&str, Vec<(&str, String)>> =
            std::collections::BTreeMap::new();
        for (path, sha) in files {
            match path.split_once('/') {
                None => lines.push(format!("100644 blob {sha}\t{path}")),
                Some((seg, rest)) => subdirs.entry(seg).or_default().push((rest, sha.clone())),
            }
        }
        for (seg, entries) in subdirs {
            let subtree = write_tree(repo_path, &entries);
            lines.push(format!("040000 tree {subtree}\t{seg}"));
        }
        lines.sort();
        let input = lines.join("\n");
        let mut child = std::process::Command::new("git")
            .arg("-C")
            .arg(repo_path)
            .arg("mktree")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .as_mut()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
        let out = child.wait_with_output().unwrap();
        assert!(
            out.status.success(),
            "mktree failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    /// The root (parentless) commit SHA of `refs/heads/main` in a bare repo
    /// built by `git_repo_with_commits` — a commit genuinely reachable in
    /// the subject's history.
    fn root_commit_sha(repo_path: &str) -> String {
        let out = std::process::Command::new("git")
            .args([
                "-C",
                repo_path,
                "rev-list",
                "--max-parents=0",
                "refs/heads/main",
            ])
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "rev-list --max-parents=0 failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    fn record_commit(
        state: &std::sync::Arc<crate::AppState>,
        id: &str,
        repo_id: &str,
        sha: &str,
        agent: &str,
        level: Option<&str>,
    ) {
        let ac = AgentCommit::new(
            Id::new(id),
            Id::new(agent),
            Id::new(repo_id),
            sha,
            "refs/heads/main",
            1_700_000_000,
        )
        .with_attestation_level(level.unwrap_or("unattested"));
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current()
                .block_on(state.agent_commits.record(&ac))
                .unwrap();
        });
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn slsa_provenance_statement_shape_and_level_mapping() {
        // HEAD and its root commit are both agent-attributed
        // server-verified (→ SLSA 3). A third record targets a SHA on no
        // reachable branch — the statement attests the subject's history
        // only, so that record must neither appear in the per-commit
        // attribution nor cap the subject's level.
        let (app, state, repo_id, repo_path, head) =
            git_repo_with_commits(&[("src/a.rs", "fn a() {}".to_string())]).await;
        let root = root_commit_sha(&repo_path);
        record_commit(
            &state,
            "sc-1",
            &repo_id,
            &head,
            "agent-3",
            Some("server-verified"),
        );
        record_commit(
            &state,
            "sc-2",
            &repo_id,
            &root,
            "agent-3",
            Some("server-verified"),
        );
        record_commit(
            &state,
            "sc-3",
            &repo_id,
            "sha-on-deleted-branch",
            "agent-1",
            None,
        );

        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/repos/{repo_id}/slsa-provenance"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;

        // in-toto Statement v1 envelope.
        assert_eq!(json["_type"], "https://in-toto.io/Statement/v1");
        assert_eq!(json["predicateType"], "https://slsa.dev/provenance/v1");
        let subject = &json["subject"][0];
        assert_eq!(subject["digest"]["gitCommit"], head);
        // The subject names the attested repo — `git_repo_with_commits`
        // creates it under a unique `slsa-<uuid>` name.
        assert!(
            subject["name"].as_str().unwrap().starts_with("slsa-"),
            "subject name must be the repo name, got {}",
            subject["name"]
        );

        // Builder + invocation identity.
        let run_details = &json["predicate"]["runDetails"];
        assert!(run_details["builder"]["id"]
            .as_str()
            .unwrap()
            .starts_with("https://gyre.dev/forge/"));
        assert!(run_details["metadata"]["invocationId"]
            .as_str()
            .unwrap()
            .contains(&head));

        // Gyre mapping: per-commit levels scoped to the subject's history.
        let gyre = &json["predicate"]["buildDefinition"]["internalParameters"]["gyre"];
        assert_eq!(
            gyre["slsaBuildLevel"], 3,
            "a record on an unreachable ref must not cap the subject's level"
        );
        let commits = gyre["commits"].as_array().unwrap();
        let by_sha = |sha: &str| {
            commits
                .iter()
                .find(|c| c["commit_sha"] == sha)
                .unwrap_or_else(|| panic!("no per-commit entry for {sha}"))
                .clone()
        };
        let verified = by_sha(&head);
        assert_eq!(verified["gyre_attestation_level"], 3);
        assert_eq!(verified["slsa_build_level"], 3);
        let root_entry = by_sha(&root);
        assert_eq!(root_entry["gyre_attestation_level"], 3);
        assert_eq!(root_entry["slsa_build_level"], 3);
        // The unreachable record is scoped out of the statement entirely.
        assert!(
            commits
                .iter()
                .all(|c| c["commit_sha"] != "sha-on-deleted-branch"),
            "agent-commit records outside the subject's history must not appear"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn slsa_weakest_link_among_reachable_commits() {
        // The root commit is reachable from the subject but unattested
        // (raw push, level 1 → SLSA 0): the weakest link in the subject's
        // history caps the claim at 0.
        let (app, state, repo_id, repo_path, head) =
            git_repo_with_commits(&[("src/d.rs", "fn d() {}".to_string())]).await;
        let root = root_commit_sha(&repo_path);
        record_commit(
            &state,
            "sc-7",
            &repo_id,
            &head,
            "agent-3",
            Some("server-verified"),
        );
        record_commit(&state, "sc-8", &repo_id, &root, "agent-1", None);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/repos/{repo_id}/slsa-provenance"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        let gyre = &json["predicate"]["buildDefinition"]["internalParameters"]["gyre"];
        assert_eq!(
            gyre["slsaBuildLevel"], 0,
            "an unattested commit in the subject's history caps the claim"
        );
        let commits = gyre["commits"].as_array().unwrap();
        let unattested = commits
            .iter()
            .find(|c| c["commit_sha"] == root)
            .expect("root commit is reachable and must be attributed")
            .clone();
        assert_eq!(unattested["gyre_attestation_level"], 1);
        assert_eq!(unattested["slsa_build_level"], 0);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn slsa_level2_maps_to_1_without_lockfile_2_with() {
        // Without a lockfile: CLI (self-reported, level 2) → SLSA 1.
        let (app, state, repo_id, _repo_path, head) =
            git_repo_with_commits(&[("src/b.rs", "fn b() {}".to_string())]).await;
        record_commit(
            &state,
            "sc-3",
            &repo_id,
            &head,
            "agent-2",
            Some("self-reported"),
        );

        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/repos/{repo_id}/slsa-provenance"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        let gyre = &json["predicate"]["buildDefinition"]["internalParameters"]["gyre"];
        assert_eq!(gyre["slsaBuildLevel"], 1);
        assert_eq!(gyre["lockfilePinned"], false);
        assert_eq!(gyre["levelMapping"]["gyre_level_2_cli"], 1);

        // With a committed, internally-consistent gyre-stack.lock: the
        // lockfile supplies the version-controlled build process → SLSA 2.
        let stack = gyre_domain::stack::AgentStack {
            agents_md_hash: "h".to_string(),
            hooks: vec![],
            mcp_servers: vec![],
            model: "m".to_string(),
            cli_version: "1".to_string(),
            settings_hash: "s".to_string(),
            persona_hash: None,
        };
        let lock = gyre_domain::stack::StackLockfile::from_stack(&stack, 0);
        let (app2, state2, repo_id2, _p2, head2) =
            git_repo_with_commits(&[("gyre-stack.lock", lock.to_toml().unwrap())]).await;
        record_commit(
            &state2,
            "sc-4",
            &repo_id2,
            &head2,
            "agent-2",
            Some("self-reported"),
        );

        let resp = app2
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/repos/{repo_id2}/slsa-provenance"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        let gyre = &json["predicate"]["buildDefinition"]["internalParameters"]["gyre"];
        assert_eq!(gyre["slsaBuildLevel"], 2);
        assert_eq!(gyre["lockfilePinned"], true);
        assert_eq!(gyre["levelMapping"]["gyre_level_2_cli"], 2);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn slsa_provenance_ref_validation_and_resolution() {
        let (app, _state, repo_id, _repo_path, head) =
            git_repo_with_commits(&[("src/c.rs", "fn c() {}".to_string())]).await;

        // Explicit ref resolves to the same subject.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/api/v1/repos/{repo_id}/slsa-provenance?ref=refs/heads/main"
                    ))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json["subject"][0]["digest"]["gitCommit"], head);

        // Flag-injection ref → 400.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/api/v1/repos/{repo_id}/slsa-provenance?ref=--upload-pack%3Devil"
                    ))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        // Unresolvable ref → 404.
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/api/v1/repos/{repo_id}/slsa-provenance?ref=refs/tags/missing"
                    ))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }
}
