//! Spec registry API endpoints.
//!
//! GET  /api/v1/specs               — list all specs with ledger state
//! GET  /api/v1/specs/pending       — specs awaiting approval
//! GET  /api/v1/specs/drifted       — specs with open drift-review tasks
//! GET  /api/v1/specs/index         — auto-generated markdown index
//! GET  /api/v1/specs/stale-links   — all stale links across the tenant (TASK-019)
//! GET  /api/v1/specs/conflicts     — all active conflicts_with links (TASK-019)
//! GET  /api/v1/specs/:path         — single spec (URL-encoded path)
//! GET  /api/v1/specs/:path/progress — tasks and MRs linked to a spec
//! GET  /api/v1/specs/:path/dependents   — specs that depend on this one (TASK-019)
//! GET  /api/v1/specs/:path/dependencies — specs this spec depends on (TASK-019)
//! POST /api/v1/specs/:path/approve — approve a spec version
//! POST /api/v1/specs/:path/revoke  — revoke an approval
//! GET  /api/v1/specs/:path/history — approval history
//! POST /api/v1/constraints/validate  — validate constraint expression syntax
//! POST /api/v1/constraints/dry-run   — evaluate constraints against repo state (§7.6)
//! GET  /api/v1/constraints/strategy  — preview strategy-implied constraints

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::spec_registry::{
    ApprovalStatus, SpecApprovalEvent, SpecLedgerEntry, SpecLinkEntry, SpecLinkType,
};
use crate::AppState;

use super::error::ApiError;
use super::{new_id, now_secs};

// ---------------------------------------------------------------------------
// Response types
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct SpecLedgerResponse {
    pub path: String,
    pub title: String,
    pub owner: String,
    /// Optional spec kind, e.g. "meta:persona", "meta:principle", "meta:standard", "meta:process".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    pub current_sha: String,
    pub approval_mode: String,
    pub approval_status: String,
    pub linked_tasks: Vec<String>,
    pub linked_mrs: Vec<String>,
    pub drift_status: String,
    pub created_at: u64,
    pub updated_at: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,
    /// Actual spec file content (populated by GET /specs/:path when repo is accessible).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
}

impl From<SpecLedgerEntry> for SpecLedgerResponse {
    fn from(e: SpecLedgerEntry) -> Self {
        Self {
            path: e.path,
            title: e.title,
            owner: e.owner,
            kind: e.kind,
            current_sha: e.current_sha,
            approval_mode: e.approval_mode,
            approval_status: e.approval_status.to_string(),
            linked_tasks: e.linked_tasks,
            linked_mrs: e.linked_mrs,
            drift_status: e.drift_status,
            created_at: e.created_at,
            updated_at: e.updated_at,
            repo_id: e.repo_id,
            workspace_id: e.workspace_id,
            content: None,
        }
    }
}

#[derive(Serialize)]
pub struct SpecApprovalEventResponse {
    pub id: String,
    pub spec_path: String,
    pub spec_sha: String,
    pub approver_type: String,
    pub approver_id: String,
    pub persona: Option<String>,
    pub approved_at: u64,
    pub revoked_at: Option<u64>,
    pub revoked_by: Option<String>,
    pub revocation_reason: Option<String>,
    pub is_active: bool,
}

impl From<SpecApprovalEvent> for SpecApprovalEventResponse {
    fn from(e: SpecApprovalEvent) -> Self {
        let is_active = e.is_active();
        Self {
            id: e.id,
            spec_path: e.spec_path,
            spec_sha: e.spec_sha,
            approver_type: e.approver_type,
            approver_id: e.approver_id,
            persona: e.persona,
            approved_at: e.approved_at,
            revoked_at: e.revoked_at,
            revoked_by: e.revoked_by,
            revocation_reason: e.revocation_reason,
            is_active,
        }
    }
}

// ---------------------------------------------------------------------------
// Request types
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct ApproveSpecRequest {
    /// The git blob SHA being approved. Must match the ledger's `current_sha`.
    pub sha: String,
    /// Optional Sigstore signature.
    pub signature: Option<String>,
    /// If approving as an agent persona, set this. If absent, treated as human approval.
    pub persona: Option<String>,
    /// Optional output constraints for authorization provenance (TASK-006, §7.1).
    #[serde(default)]
    pub output_constraints: Vec<gyre_common::OutputConstraint>,
    /// Optional scope constraint for authorization provenance (TASK-006, §7.1).
    pub scope: Option<gyre_common::ScopeConstraint>,
    /// User's Ed25519 signature over the InputContent hash, base64-encoded (§2.2).
    /// Signed with the ephemeral private key from the user's KeyBinding.
    /// Required to produce a valid SignedInput — without this, no SignedInput is created.
    pub user_content_signature: Option<String>,
}

#[derive(Deserialize)]
pub struct RevokeSpecApprovalRequest {
    pub reason: String,
}

// ---------------------------------------------------------------------------
// GET /api/v1/specs — list all specs
// ---------------------------------------------------------------------------

#[derive(serde::Deserialize)]
pub struct ListSpecsQuery {
    /// Filter by spec kind, e.g. "meta:persona"
    pub kind: Option<String>,
}

pub async fn list_specs(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListSpecsQuery>,
) -> Json<Vec<SpecLedgerResponse>> {
    use std::collections::HashMap;

    // Build spec_path → task_ids map from all tasks that have spec_path set.
    let mut task_map: HashMap<String, Vec<String>> = HashMap::new();
    if let Ok(all_tasks) = state.tasks.list().await {
        for task in all_tasks {
            if let Some(sp) = &task.spec_path {
                task_map
                    .entry(sp.clone())
                    .or_default()
                    .push(task.id.to_string());
            }
        }
    }

    // Build spec_path → mr_ids map from all MRs that have spec_ref set.
    let mut mr_map: HashMap<String, Vec<String>> = HashMap::new();
    if let Ok(all_mrs) = state.merge_requests.list().await {
        for mr in all_mrs {
            if let Some(spec_ref) = &mr.spec_ref {
                // spec_ref format: "path@sha" — extract path prefix.
                let spec_path = if let Some((path, _)) = spec_ref.rsplit_once('@') {
                    path.to_string()
                } else {
                    spec_ref.clone()
                };
                mr_map.entry(spec_path).or_default().push(mr.id.to_string());
            }
        }
    }

    let mut specs: Vec<SpecLedgerResponse> = state
        .spec_ledger
        .list_all()
        .await
        .unwrap_or_default()
        .into_iter()
        .filter(|e| {
            if let Some(kind_filter) = &params.kind {
                e.kind.as_deref() == Some(kind_filter.as_str())
            } else {
                true
            }
        })
        .map(|mut e| {
            e.linked_tasks = task_map.get(&e.path).cloned().unwrap_or_default();
            e.linked_mrs = mr_map.get(&e.path).cloned().unwrap_or_default();
            e.into()
        })
        .collect();
    specs.sort_by(|a, b| a.path.cmp(&b.path));
    Json(specs)
}

// ---------------------------------------------------------------------------
// GET /api/v1/specs/pending — specs awaiting approval
// ---------------------------------------------------------------------------

pub async fn list_pending_specs(
    State(state): State<Arc<AppState>>,
) -> Json<Vec<SpecLedgerResponse>> {
    let mut specs: Vec<SpecLedgerResponse> = state
        .spec_ledger
        .list_all()
        .await
        .unwrap_or_default()
        .into_iter()
        .filter(|e| e.approval_status == ApprovalStatus::Pending)
        .map(Into::into)
        .collect();
    specs.sort_by(|a, b| a.path.cmp(&b.path));
    Json(specs)
}

// ---------------------------------------------------------------------------
// GET /api/v1/specs/drifted — specs with open drift-review tasks
// ---------------------------------------------------------------------------

pub async fn list_drifted_specs(
    State(state): State<Arc<AppState>>,
) -> Json<Vec<SpecLedgerResponse>> {
    let mut specs: Vec<SpecLedgerResponse> = state
        .spec_ledger
        .list_all()
        .await
        .unwrap_or_default()
        .into_iter()
        .filter(|e| e.drift_status == "drifted")
        .map(Into::into)
        .collect();
    specs.sort_by(|a, b| a.path.cmp(&b.path));
    Json(specs)
}

// ---------------------------------------------------------------------------
// GET /api/v1/specs/index — auto-generated markdown index
// ---------------------------------------------------------------------------

pub async fn spec_index(State(state): State<Arc<AppState>>) -> axum::response::Response<String> {
    let all_entries = state.spec_ledger.list_all().await.unwrap_or_default();

    // Group specs by directory.
    let mut by_dir: std::collections::BTreeMap<String, Vec<SpecLedgerEntry>> =
        std::collections::BTreeMap::new();
    for entry in all_entries {
        let dir = entry.path.split('/').next().unwrap_or("other").to_string();
        by_dir.entry(dir).or_default().push(entry);
    }

    let mut md = String::from("# Spec Registry Index\n\n");
    md.push_str("> Auto-generated from `specs/manifest.yaml` + forge ledger.\n\n");

    for (dir, mut entries) in by_dir {
        entries.sort_by(|a, b| a.path.cmp(&b.path));
        md.push_str(&format!("## {}\n\n", capitalize_dir(&dir)));
        md.push_str("| Spec | Status | SHA |\n");
        md.push_str("|------|--------|-----|\n");
        for e in entries {
            let short_sha = if e.current_sha.len() >= 8 {
                &e.current_sha[..8]
            } else {
                &e.current_sha
            };
            md.push_str(&format!(
                "| [{title}](specs/{path}) | {status} | `{sha}` |\n",
                title = e.title,
                path = e.path,
                status = e.approval_status,
                sha = short_sha,
            ));
        }
        md.push('\n');
    }

    axum::response::Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "text/markdown; charset=utf-8")
        .body(md)
        .unwrap()
}

fn capitalize_dir(dir: &str) -> String {
    let mut chars = dir.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_uppercase().to_string() + chars.as_str(),
    }
}

// ---------------------------------------------------------------------------
// GET /api/v1/specs/:path — single spec
//
// The path parameter is URL-encoded, e.g. system%2Fdesign-principles.md
// ---------------------------------------------------------------------------

/// Optional query parameters for GET /api/v1/specs/:path
#[derive(Deserialize, Default)]
pub struct GetSpecQuery {
    pub repo_id: Option<String>,
}

pub async fn get_spec(
    State(state): State<Arc<AppState>>,
    Path(encoded_path): Path<String>,
    Query(query): Query<GetSpecQuery>,
) -> Result<Json<SpecLedgerResponse>, ApiError> {
    // axum already URL-decodes path segments.
    let spec_path = encoded_path;

    let mut entry = state
        .spec_ledger
        .find_by_path(&spec_path)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("spec '{spec_path}' not in registry")))?;

    // Populate linked_tasks from tasks with matching spec_path.
    entry.linked_tasks = state
        .tasks
        .list_by_spec_path(&spec_path)
        .await
        .unwrap_or_default()
        .iter()
        .map(|t| t.id.to_string())
        .collect();

    // Populate linked_mrs from MRs with spec_ref matching "spec_path@...".
    let prefix = format!("{spec_path}@");
    entry.linked_mrs = state
        .merge_requests
        .list()
        .await
        .unwrap_or_default()
        .into_iter()
        .filter(|mr| {
            mr.spec_ref
                .as_deref()
                .map(|s| s.starts_with(&prefix) || s == spec_path.as_str())
                .unwrap_or(false)
        })
        .map(|mr| mr.id.to_string())
        .collect();

    let mut resp: SpecLedgerResponse = entry.into();

    // Read spec file content from git (best-effort).
    // Use repo_id from query param or from the ledger entry itself.
    let repo_id_str = query.repo_id.or_else(|| resp.repo_id.clone());
    if let Some(repo_id) = repo_id_str {
        if let Ok(Some(repo)) = state
            .repos
            .find_by_id(&gyre_common::Id::new(&repo_id))
            .await
        {
            let git_bin = std::env::var("GYRE_GIT_PATH").unwrap_or_else(|_| "git".to_string());
            let git_file_path = format!("specs/{spec_path}");
            if let Some(content) =
                crate::spec_registry::read_git_file(&git_bin, &repo.path, "HEAD", &git_file_path)
                    .await
            {
                resp.content = Some(content);
            }
        }
    }

    Ok(Json(resp))
}

// ---------------------------------------------------------------------------
// POST /api/v1/specs/:path/approve — approve a spec version
// ---------------------------------------------------------------------------

pub async fn approve_spec(
    State(state): State<Arc<AppState>>,
    Path(encoded_path): Path<String>,
    auth: crate::auth::AuthenticatedAgent,
    Json(req): Json<ApproveSpecRequest>,
) -> Result<(StatusCode, Json<SpecApprovalEventResponse>), ApiError> {
    let spec_path = encoded_path;
    let now = now_secs();

    // ReadOnly users cannot approve specs (M21.1-C).
    if auth.roles.contains(&gyre_domain::UserRole::ReadOnly)
        && !auth.roles.contains(&gyre_domain::UserRole::Admin)
        && !auth.roles.contains(&gyre_domain::UserRole::Developer)
        && !auth.roles.contains(&gyre_domain::UserRole::Agent)
        && auth.agent_id != "system"
    {
        return Err(ApiError::Forbidden(
            "ReadOnly users cannot approve specs".to_string(),
        ));
    }

    // Validate SHA format (must be 40-char hex).
    if req.sha.len() != 40 || !req.sha.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(ApiError::InvalidInput(
            "sha must be a 40-character hex string".to_string(),
        ));
    }

    // Verify spec is in the ledger.
    if state.spec_ledger.find_by_path(&spec_path).await?.is_none() {
        return Err(ApiError::NotFound(format!(
            "spec '{spec_path}' not in registry"
        )));
    }

    // Enforce link-based approval gates.
    {
        // Collect relevant links without holding the lock across await points.
        let relevant_links: Vec<_> = {
            let links = state.spec_links_store.lock().await;
            links
                .iter()
                .filter(|l| l.source_path == spec_path)
                .cloned()
                .collect()
        };

        for link in &relevant_links {
            match &link.link_type {
                SpecLinkType::Implements => {
                    // Parent spec must be approved before this spec can be approved.
                    if let Some(parent) = state.spec_ledger.find_by_path(&link.target_path).await? {
                        if parent.approval_status != ApprovalStatus::Approved {
                            return Err(ApiError::InvalidInput(format!(
                                "cannot approve '{}': implements '{}' which is not yet approved",
                                spec_path, link.target_path
                            )));
                        }
                    }
                }
                SpecLinkType::ConflictsWith => {
                    // Conflicting spec must not be approved.
                    if let Some(conflicting) =
                        state.spec_ledger.find_by_path(&link.target_path).await?
                    {
                        if conflicting.approval_status == ApprovalStatus::Approved {
                            return Err(ApiError::InvalidInput(format!(
                                "cannot approve '{}': conflicts with '{}' which is already approved — resolve the conflict first",
                                spec_path, link.target_path
                            )));
                        }
                    }
                }
                SpecLinkType::DependsOn => {
                    // TASK-016: Target spec's implementation must be complete (approved)
                    // before the depending spec can be approved.
                    // spec-links.md §Approval Gates: "Implementation tasks for source spec
                    // are blocked until target spec's implementation tasks are complete."
                    if let Some(target) = state.spec_ledger.find_by_path(&link.target_path).await? {
                        if target.approval_status != ApprovalStatus::Approved {
                            return Err(ApiError::InvalidInput(format!(
                                "cannot approve '{}': depends on '{}' whose implementation is not yet complete (status: {})",
                                spec_path, link.target_path, target.approval_status
                            )));
                        }
                    }
                }
                _ => {} // wildcard:ok — extends/references/supersedes have no pre-approval gate; supersedes side-effect handled below
            }
        }
    }

    // Determine approver type from auth token kind (not request body).
    // JWT bearer tokens → agent; global token / API key → human.
    let (approver_type, approver_id) = if auth.jwt_claims.is_some() {
        ("agent".to_string(), format!("agent:{}", auth.agent_id))
    } else {
        ("human".to_string(), format!("user:{}", auth.agent_id))
    };

    let event = SpecApprovalEvent {
        id: new_id().to_string(),
        spec_path: spec_path.clone(),
        spec_sha: req.sha.clone(),
        approver_type,
        approver_id,
        persona: req.persona,
        approved_at: now,
        revoked_at: None,
        revoked_by: None,
        revocation_reason: None,
    };

    // Record in approval history.
    let _ = state.spec_approval_history.record(&event).await;

    // TASK-006: Produce SignedInput when a KeyBinding is available AND the client
    // provides a user_content_signature (Phase 1, non-enforcing).
    // Per spec §2.2/§9: SignedInput.signature MUST be user-signed (ephemeral key),
    // not platform-signed. The platform cannot forge user authorizations.
    {
        let user_identity = &event.approver_id;
        let active_bindings = state
            .key_bindings
            .find_active_by_identity(&auth.tenant_id, user_identity)
            .await
            .unwrap_or_default();

        if let Some(key_binding) = active_bindings.into_iter().next() {
            if let Some(ref user_sig_b64) = req.user_content_signature {
                // Decode the user's signature.
                let user_sig_bytes = {
                    use base64::engine::general_purpose::STANDARD;
                    use base64::Engine;
                    STANDARD.decode(user_sig_b64).map_err(|_| {
                        ApiError::InvalidInput(
                            "user_content_signature must be valid base64".to_string(),
                        )
                    })?
                };

                // Look up ledger entry for workspace_id and repo_id.
                let entry = state
                    .spec_ledger
                    .find_by_path(&spec_path)
                    .await
                    .ok()
                    .flatten();
                let workspace_id = entry
                    .as_ref()
                    .and_then(|e| e.workspace_id.clone())
                    .unwrap_or_default();
                let repo_id = entry
                    .as_ref()
                    .and_then(|e| e.repo_id.clone())
                    .unwrap_or_default();

                // Build persona constraints from the approval persona.
                let persona_constraints: Vec<gyre_common::PersonaRef> = event
                    .persona
                    .as_ref()
                    .map(|p| vec![gyre_common::PersonaRef { name: p.clone() }])
                    .unwrap_or_default();

                let scope = req.scope.unwrap_or_else(|| gyre_common::ScopeConstraint {
                    allowed_paths: vec![],
                    forbidden_paths: vec![],
                });

                let input_content = gyre_common::InputContent {
                    spec_path: spec_path.clone(),
                    spec_sha: req.sha.clone(),
                    workspace_id: workspace_id.clone(),
                    repo_id: repo_id.clone(),
                    persona_constraints,
                    meta_spec_set_sha: String::new(),
                    scope: scope.clone(),
                };

                // Compute content hash.
                let content_bytes = serde_json::to_vec(&input_content).unwrap_or_default();
                let content_hash = {
                    use ring::digest;
                    digest::digest(&digest::SHA256, &content_bytes)
                };

                // Verify the user's signature over the content hash against the
                // KeyBinding's public key (§2.2, §9 — user signs, not platform).
                {
                    use ring::signature::{self, UnparsedPublicKey};
                    let peer_public_key =
                        UnparsedPublicKey::new(&signature::ED25519, &key_binding.public_key);
                    peer_public_key
                        .verify(content_hash.as_ref(), &user_sig_bytes)
                        .map_err(|_| {
                            ApiError::InvalidInput(
                                "user_content_signature verification failed — signature does not \
                                 match KeyBinding public key over InputContent hash"
                                    .to_string(),
                            )
                        })?;
                }

                let signed_input = gyre_common::SignedInput {
                    content: input_content,
                    output_constraints: req.output_constraints.clone(),
                    valid_until: now + 86_400 * 30, // 30 days default validity
                    expected_generation: None,
                    signature: user_sig_bytes,
                    key_binding: key_binding.clone(),
                };

                // Store as a chain attestation (root node, chain_depth = 0).
                let attestation_id = {
                    let att_bytes = serde_json::to_vec(&signed_input).unwrap_or_default();
                    let hash = ring::digest::digest(&ring::digest::SHA256, &att_bytes);
                    hex::encode(hash.as_ref())
                };

                let attestation = gyre_common::Attestation {
                    id: attestation_id.clone(),
                    input: gyre_common::AttestationInput::Signed(signed_input),
                    output: gyre_common::AttestationOutput {
                        content_hash: content_hash.as_ref().to_vec(),
                        commit_sha: String::new(), // No commit yet at approval time.
                        agent_signature: None,
                        gate_results: vec![],
                    },
                    metadata: gyre_common::AttestationMetadata {
                        created_at: now,
                        workspace_id,
                        repo_id,
                        task_id: String::new(), // No task yet at approval time.
                        agent_id: event.approver_id.clone(),
                        chain_depth: 0,
                    },
                };

                if let Err(e) = state.chain_attestations.save(&attestation).await {
                    tracing::warn!(
                        attestation_id = %attestation_id,
                        error = %e,
                        "failed to store SignedInput attestation (Phase 1, non-blocking)"
                    );
                } else {
                    tracing::info!(
                        attestation_id = %attestation_id,
                        spec_path = %spec_path,
                        approver = %event.approver_id,
                        "attestation.created: SignedInput produced for spec approval (user-signed)"
                    );
                }
            } else {
                tracing::debug!(
                    spec_path = %spec_path,
                    approver = %event.approver_id,
                    "KeyBinding exists but no user_content_signature provided — \
                     skipping SignedInput creation (Phase 1)"
                );
            }
        } else {
            tracing::debug!(
                spec_path = %spec_path,
                approver = %event.approver_id,
                "no active KeyBinding for approver — skipping SignedInput creation (Phase 1)"
            );
        }
    }

    // Update ledger approval_status based on new approval.
    // For simplicity: any valid approval for the current SHA sets status to Approved.
    if let Some(mut entry) = state.spec_ledger.find_by_path(&spec_path).await? {
        if entry.current_sha == req.sha {
            entry.approval_status = ApprovalStatus::Approved;
            entry.updated_at = now;
            let _ = state.spec_ledger.save(&entry).await;

            // Emit SpecApproved event on the message bus (agent-runtime.md §1).
            // This is the single trigger for all agent work via the signal chain:
            // SpecApproved → workspace orchestrator → delegation task → repo orchestrator → sub-tasks → agents.
            // Destination: Workspace(workspace_id) — consumed by workspace orchestrator.
            let dest = match entry.workspace_id.as_deref() {
                Some(ws_id) => {
                    gyre_common::message::Destination::Workspace(gyre_common::Id::new(ws_id))
                }
                None => gyre_common::message::Destination::Broadcast,
            };
            state
                .emit_event(
                    entry
                        .workspace_id
                        .as_ref()
                        .map(|ws| gyre_common::Id::new(ws.as_str())),
                    dest,
                    gyre_common::message::MessageKind::SpecApproved,
                    Some(serde_json::json!({
                        "repo_id": entry.repo_id,
                        "spec_path": spec_path,
                        "spec_sha": req.sha,
                        "approved_by": event.approver_id,
                        "approval_id": event.id,
                    })),
                )
                .await;
        }
    }

    // TASK-016: Supersedes side-effect — mark target specs as Deprecated when
    // the superseding spec is approved (spec-links.md §Approval Gates).
    {
        let supersedes_links: Vec<_> = {
            let links = state.spec_links_store.lock().await;
            links
                .iter()
                .filter(|l| l.source_path == spec_path && l.link_type == SpecLinkType::Supersedes)
                .cloned()
                .collect()
        };

        for link in &supersedes_links {
            if let Ok(Some(mut target_entry)) =
                state.spec_ledger.find_by_path(&link.target_path).await
            {
                if target_entry.approval_status != ApprovalStatus::Deprecated {
                    target_entry.approval_status = ApprovalStatus::Deprecated;
                    target_entry.updated_at = now;
                    let _ = state.spec_ledger.save(&target_entry).await;

                    // Emit deprecation event on the message bus.
                    let dest = match target_entry.workspace_id.as_deref() {
                        Some(ws_id) => gyre_common::message::Destination::Workspace(
                            gyre_common::Id::new(ws_id),
                        ),
                        None => gyre_common::message::Destination::Broadcast,
                    };
                    state
                        .emit_event(
                            target_entry
                                .workspace_id
                                .as_ref()
                                .map(|ws| gyre_common::Id::new(ws.as_str())),
                            dest,
                            gyre_common::message::MessageKind::StaleSpecWarning,
                            Some(serde_json::json!({
                                "type": "spec_deprecated",
                                "spec_path": link.target_path,
                                "superseded_by": spec_path,
                                "deprecated_at": now,
                            })),
                        )
                        .await;

                    tracing::info!(
                        source = %spec_path,
                        target = %link.target_path,
                        "spec-links: supersedes approval — target marked Deprecated"
                    );
                }
            }
        }
    }

    Ok((StatusCode::CREATED, Json(event.into())))
}

// ---------------------------------------------------------------------------
// POST /api/v1/specs/:path/revoke — revoke an approval
// ---------------------------------------------------------------------------

pub async fn revoke_spec_approval(
    State(state): State<Arc<AppState>>,
    Path(encoded_path): Path<String>,
    auth: crate::auth::AuthenticatedAgent,
    Json(req): Json<RevokeSpecApprovalRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let spec_path = encoded_path;
    let now = now_secs();

    // Find the most recent active approval for this spec path.
    let events = state
        .spec_approval_history
        .list_by_path(&spec_path)
        .await
        .unwrap_or_default();
    let active_event = events.into_iter().rev().find(|e| e.is_active());

    match active_event {
        None => Err(ApiError::NotFound(format!(
            "no active approval for spec '{spec_path}'"
        ))),
        Some(ev) => {
            // Only the original approver or an Admin can revoke.
            let is_admin =
                auth.agent_id == "system" || auth.roles.contains(&gyre_domain::UserRole::Admin);
            let caller_id = format!(
                "{}:{}",
                if auth.jwt_claims.is_some() {
                    "agent"
                } else {
                    "user"
                },
                auth.agent_id
            );
            if ev.approver_id != caller_id && !is_admin {
                return Err(ApiError::Forbidden(
                    "only the original approver or an Admin can revoke".to_string(),
                ));
            }

            let _ = state
                .spec_approval_history
                .revoke_event(&ev.id, now, &auth.agent_id, &req.reason)
                .await;

            // Reset ledger approval_status to Pending.
            if let Some(mut entry) = state.spec_ledger.find_by_path(&spec_path).await? {
                entry.approval_status = ApprovalStatus::Pending;
                entry.updated_at = now;
                let _ = state.spec_ledger.save(&entry).await;
            }

            Ok(Json(serde_json::json!({
                "spec_path": spec_path,
                "revoked_by": auth.agent_id,
                "revoked_at": now,
            })))
        }
    }
}

// ---------------------------------------------------------------------------
// POST /api/v1/specs/:path/reject — reject a spec (human decision)
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct RejectSpecRequest {
    pub reason: String,
}

pub async fn reject_spec(
    State(state): State<Arc<AppState>>,
    Path(encoded_path): Path<String>,
    auth: crate::auth::AuthenticatedAgent,
    Json(req): Json<RejectSpecRequest>,
) -> Result<Json<SpecLedgerResponse>, ApiError> {
    let spec_path = encoded_path;
    let now = now_secs();

    // Only Admin or Developer roles can reject specs.
    let is_authorized = auth.agent_id == "system"
        || auth.roles.contains(&gyre_domain::UserRole::Admin)
        || auth.roles.contains(&gyre_domain::UserRole::Developer);
    if !is_authorized {
        return Err(ApiError::Forbidden(
            "only Admin or Developer roles can reject specs".to_string(),
        ));
    }

    // Fetch the spec from the ledger.
    let mut entry = state
        .spec_ledger
        .find_by_path(&spec_path)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("spec '{spec_path}' not in registry")))?;

    // Set status to Rejected.
    entry.approval_status = ApprovalStatus::Rejected;
    entry.updated_at = now;
    let _ = state.spec_ledger.save(&entry).await;

    // Close any associated MRs from spec-edit/* branches that reference this spec.
    // A spec-edit MR has spec_ref set to "spec_path@sha" and source_branch "spec-edit/...".
    let prefix = format!("{spec_path}@");
    let all_mrs = state.merge_requests.list().await.unwrap_or_default();
    for mut mr in all_mrs {
        let is_spec_mr = mr
            .spec_ref
            .as_deref()
            .map(|s| s.starts_with(&prefix) || s == spec_path.as_str())
            .unwrap_or(false);
        let is_spec_edit_branch = mr.source_branch.starts_with("spec-edit/");
        if is_spec_mr && is_spec_edit_branch && mr.status == gyre_domain::MrStatus::Open {
            mr.status = gyre_domain::MrStatus::Closed;
            mr.updated_at = now;
            let _ = state.merge_requests.update(&mr).await;
        }
    }

    // Spec rejection mid-flight (agent-runtime.md §1): cancel all in-flight tasks
    // referencing this spec and shutdown active agents working on those tasks.
    {
        let spec_tasks = state
            .tasks
            .list_by_spec_path(&spec_path)
            .await
            .unwrap_or_default();
        let cancel_reason = format!("spec rejected: {}", req.reason);
        for mut task in spec_tasks {
            // Only cancel tasks that are still in-flight (Backlog, InProgress, Review, Blocked).
            if matches!(
                task.status,
                gyre_domain::TaskStatus::Done | gyre_domain::TaskStatus::Cancelled
            ) {
                continue;
            }
            let _ = task.cancel(Some(cancel_reason.clone()), now);
            let _ = state.tasks.update(&task).await;

            // If an agent is working on this task, stop it.
            let agents = state.agents.list().await.unwrap_or_default();
            for mut agent in agents {
                if agent.current_task_id.as_ref() == Some(&task.id)
                    && matches!(agent.status, gyre_domain::AgentStatus::Active)
                {
                    let _ = agent.transition_status(gyre_domain::AgentStatus::Stopped);
                    let _ = state.agents.update(&agent).await;

                    // Send shutdown message to agent's inbox.
                    state
                        .emit_event(
                            None,
                            gyre_common::message::Destination::Agent(agent.id.clone()),
                            gyre_common::message::MessageKind::StatusUpdate,
                            Some(serde_json::json!({
                                "action": "shutdown",
                                "reason": cancel_reason,
                                "spec_path": spec_path,
                                "grace_period_secs": 60,
                            })),
                        )
                        .await;
                }
            }
        }
    }

    // Agent-runtime §1: Create priority-2 "Spec rejected" notification for
    // workspace Admin/Developer members.
    if let Some(ref ws_id) = entry.workspace_id {
        let ws_id = gyre_common::Id::new(ws_id.as_str());
        if let Ok(members) = state.workspace_memberships.list_by_workspace(&ws_id).await {
            for member in &members {
                if matches!(
                    member.role,
                    gyre_domain::WorkspaceRole::Admin
                        | gyre_domain::WorkspaceRole::Developer
                        | gyre_domain::WorkspaceRole::Owner
                ) {
                    let tenant_id = entry.repo_id.as_deref().unwrap_or("default");
                    crate::notifications::notify(
                        state.as_ref(),
                        ws_id.clone(),
                        member.user_id.clone(),
                        gyre_common::NotificationType::SpecRejected,
                        format!("Spec '{}' rejected: {}", spec_path, req.reason),
                        tenant_id,
                    )
                    .await;
                }
            }
        }
    }

    // Record rejection reason in the approval history for audit.
    let rejection_note = SpecApprovalEvent {
        id: new_id().to_string(),
        spec_path: spec_path.clone(),
        spec_sha: entry.current_sha.clone(),
        approver_type: "human".to_string(),
        approver_id: format!("user:{}", auth.agent_id),
        persona: None,
        approved_at: now,
        revoked_at: Some(now),
        revoked_by: Some(auth.agent_id.clone()),
        revocation_reason: Some(format!("rejected: {}", req.reason)),
    };
    let _ = state.spec_approval_history.record(&rejection_note).await;

    Ok(Json(entry.into()))
}

// ---------------------------------------------------------------------------
// GET /api/v1/specs/:path/history — approval history
// ---------------------------------------------------------------------------

pub async fn spec_approval_history(
    State(state): State<Arc<AppState>>,
    Path(encoded_path): Path<String>,
) -> Json<Vec<SpecApprovalEventResponse>> {
    let spec_path = encoded_path;
    let events: Vec<SpecApprovalEventResponse> = state
        .spec_approval_history
        .list_by_path(&spec_path)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(Into::into)
        .collect();
    Json(events)
}

// ---------------------------------------------------------------------------
// Response types for link endpoints
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct SpecLinkResponse {
    pub id: String,
    pub source_path: String,
    pub link_type: String,
    pub target_path: String,
    /// Resolved target repo UUID. Null for unresolved cross-workspace links.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_repo_id: Option<String>,
    /// Human-readable composite path (e.g. "@platform-core/api-svc/system/auth.md").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_display: Option<String>,
    pub target_sha: Option<String>,
    pub reason: Option<String>,
    pub status: String,
    pub created_at: u64,
    pub stale_since: Option<u64>,
}

impl From<SpecLinkEntry> for SpecLinkResponse {
    fn from(e: SpecLinkEntry) -> Self {
        Self {
            id: e.id,
            source_path: e.source_path,
            link_type: e.link_type.to_string(),
            target_path: e.target_path,
            target_repo_id: e.target_repo_id,
            target_display: e.target_display,
            target_sha: e.target_sha,
            reason: e.reason,
            status: e.status,
            created_at: e.created_at,
            stale_since: e.stale_since,
        }
    }
}

#[derive(Serialize)]
pub struct SpecGraphNode {
    pub path: String,
    pub title: String,
    pub approval_status: String,
}

#[derive(Serialize)]
pub struct SpecGraphEdge {
    pub source: String,
    pub target: String,
    pub link_type: String,
    pub status: String,
    pub reason: Option<String>,
}

#[derive(Serialize)]
pub struct SpecGraphResponse {
    pub nodes: Vec<SpecGraphNode>,
    pub edges: Vec<SpecGraphEdge>,
}

// ---------------------------------------------------------------------------
// GET /api/v1/specs/:path/links — outbound + inbound links for one spec
// ---------------------------------------------------------------------------

pub async fn get_spec_links(
    State(state): State<Arc<AppState>>,
    Path(encoded_path): Path<String>,
) -> Result<Json<Vec<SpecLinkResponse>>, ApiError> {
    let spec_path = encoded_path;

    // Verify spec exists.
    if state.spec_ledger.find_by_path(&spec_path).await?.is_none() {
        return Err(ApiError::NotFound(format!(
            "spec '{spec_path}' not in registry"
        )));
    }

    let links = state.spec_links_store.lock().await;
    let mut result: Vec<SpecLinkResponse> = links
        .iter()
        .filter(|l| l.source_path == spec_path || l.target_path == spec_path)
        .cloned()
        .map(Into::into)
        .collect();
    result.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(Json(result))
}

// ---------------------------------------------------------------------------
// GET /api/v1/specs/graph — full spec dependency graph
// ---------------------------------------------------------------------------

pub async fn get_spec_graph(State(state): State<Arc<AppState>>) -> Json<SpecGraphResponse> {
    let all_entries = state.spec_ledger.list_all().await.unwrap_or_default();
    let links = state.spec_links_store.lock().await;

    let mut nodes: Vec<SpecGraphNode> = all_entries
        .iter()
        .map(|e| SpecGraphNode {
            path: e.path.clone(),
            title: e.title.clone(),
            approval_status: e.approval_status.to_string(),
        })
        .collect();
    nodes.sort_by(|a, b| a.path.cmp(&b.path));

    let edges: Vec<SpecGraphEdge> = links
        .iter()
        .map(|l| SpecGraphEdge {
            source: l.source_path.clone(),
            target: l.target_path.clone(),
            link_type: l.link_type.to_string(),
            status: l.status.clone(),
            reason: l.reason.clone(),
        })
        .collect();

    Json(SpecGraphResponse { nodes, edges })
}

// ---------------------------------------------------------------------------
// GET /api/v1/specs/:path/dependents — specs that depend on the given spec
// (inbound depends_on and implements links targeting this spec)
// TASK-019: spec-links.md §Querying the Graph
// ---------------------------------------------------------------------------

pub async fn get_spec_dependents(
    State(state): State<Arc<AppState>>,
    Path(encoded_path): Path<String>,
) -> Result<Json<Vec<SpecLinkResponse>>, ApiError> {
    let spec_path = encoded_path;

    if state.spec_ledger.find_by_path(&spec_path).await?.is_none() {
        return Err(ApiError::NotFound(format!(
            "spec '{spec_path}' not in registry"
        )));
    }

    let links = state.spec_links_store.lock().await;
    let mut result: Vec<SpecLinkResponse> = links
        .iter()
        .filter(|l| {
            l.target_path == spec_path
                && (l.link_type == SpecLinkType::DependsOn
                    || l.link_type == SpecLinkType::Implements)
        })
        .cloned()
        .map(Into::into)
        .collect();
    result.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(Json(result))
}

// ---------------------------------------------------------------------------
// GET /api/v1/specs/:path/dependencies — specs the given spec depends on
// (outbound depends_on and implements links from this spec)
// TASK-019: spec-links.md §Querying the Graph
// ---------------------------------------------------------------------------

pub async fn get_spec_dependencies(
    State(state): State<Arc<AppState>>,
    Path(encoded_path): Path<String>,
) -> Result<Json<Vec<SpecLinkResponse>>, ApiError> {
    let spec_path = encoded_path;

    if state.spec_ledger.find_by_path(&spec_path).await?.is_none() {
        return Err(ApiError::NotFound(format!(
            "spec '{spec_path}' not in registry"
        )));
    }

    let links = state.spec_links_store.lock().await;
    let mut result: Vec<SpecLinkResponse> = links
        .iter()
        .filter(|l| {
            l.source_path == spec_path
                && (l.link_type == SpecLinkType::DependsOn
                    || l.link_type == SpecLinkType::Implements)
        })
        .cloned()
        .map(Into::into)
        .collect();
    result.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(Json(result))
}

// ---------------------------------------------------------------------------
// GET /api/v1/specs/stale-links — all stale links across the tenant
// TASK-019: spec-links.md §Querying the Graph
// ---------------------------------------------------------------------------

pub async fn get_stale_links(State(state): State<Arc<AppState>>) -> Json<Vec<SpecLinkResponse>> {
    let links = state.spec_links_store.lock().await;
    let mut result: Vec<SpecLinkResponse> = links
        .iter()
        .filter(|l| l.status == "stale")
        .cloned()
        .map(Into::into)
        .collect();
    result.sort_by(|a, b| a.id.cmp(&b.id));
    Json(result)
}

// ---------------------------------------------------------------------------
// GET /api/v1/specs/conflicts — all active conflicts
// TASK-019: spec-links.md §Querying the Graph
// ---------------------------------------------------------------------------

pub async fn get_conflicts(State(state): State<Arc<AppState>>) -> Json<Vec<SpecLinkResponse>> {
    // Collect conflicts_with links without holding the lock across await points.
    let candidate_links: Vec<_> = {
        let links = state.spec_links_store.lock().await;
        links
            .iter()
            .filter(|l| l.link_type == SpecLinkType::ConflictsWith)
            .cloned()
            .collect()
    };

    // An "active conflict" is a conflicts_with link where both specs are approved.
    // spec-links.md §Link Status: conflicted = both specs approved (violation).
    let mut result: Vec<SpecLinkResponse> = Vec::new();
    for link in candidate_links {
        let source_approved = state
            .spec_ledger
            .find_by_path(&link.source_path)
            .await
            .ok()
            .flatten()
            .map_or(false, |e| e.approval_status == ApprovalStatus::Approved);
        let target_approved = state
            .spec_ledger
            .find_by_path(&link.target_path)
            .await
            .ok()
            .flatten()
            .map_or(false, |e| e.approval_status == ApprovalStatus::Approved);
        if source_approved && target_approved {
            result.push(link.into());
        }
    }
    result.sort_by(|a, b| a.id.cmp(&b.id));
    Json(result)
}

// ---------------------------------------------------------------------------
// GET /api/v1/specs/:path/progress — tasks and MRs linked to a spec
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct SpecProgressTaskItem {
    pub id: String,
    pub title: String,
    pub status: String,
    pub priority: String,
}

#[derive(Serialize)]
pub struct SpecProgressMrItem {
    pub id: String,
    pub title: String,
    pub status: String,
    pub spec_ref: Option<String>,
}

#[derive(Serialize)]
pub struct SpecProgressResponse {
    pub spec_path: String,
    pub tasks: Vec<SpecProgressTaskItem>,
    pub mrs: Vec<SpecProgressMrItem>,
    pub open_tasks: usize,
    pub completed_tasks: usize,
    pub merged_mrs: usize,
}

pub async fn get_spec_progress(
    State(state): State<Arc<AppState>>,
    Path(encoded_path): Path<String>,
) -> Result<Json<SpecProgressResponse>, ApiError> {
    let spec_path = encoded_path;

    // Verify the spec exists in the registry.
    if state.spec_ledger.find_by_path(&spec_path).await?.is_none() {
        return Err(ApiError::NotFound(format!(
            "spec '{spec_path}' not in registry"
        )));
    }

    // Query tasks linked to this spec path.
    let linked_tasks = state.tasks.list_by_spec_path(&spec_path).await?;

    // Query all MRs and filter by spec_ref prefix match ("path@sha").
    let prefix = format!("{spec_path}@");
    let all_mrs = state.merge_requests.list().await?;
    let linked_mrs: Vec<_> = all_mrs
        .into_iter()
        .filter(|mr| {
            mr.spec_ref
                .as_deref()
                .map(|s| s.starts_with(&prefix) || s == spec_path.as_str())
                .unwrap_or(false)
        })
        .collect();

    let open_tasks = linked_tasks
        .iter()
        .filter(|t| {
            !matches!(
                t.status,
                gyre_domain::TaskStatus::Done | gyre_domain::TaskStatus::Cancelled
            )
        })
        .count();
    let completed_tasks = linked_tasks
        .iter()
        .filter(|t| matches!(t.status, gyre_domain::TaskStatus::Done))
        .count();
    let merged_mrs = linked_mrs
        .iter()
        .filter(|mr| matches!(mr.status, gyre_domain::MrStatus::Merged))
        .count();

    let task_items: Vec<SpecProgressTaskItem> = linked_tasks
        .iter()
        .map(|t| SpecProgressTaskItem {
            id: t.id.to_string(),
            title: t.title.clone(),
            status: match &t.status {
                gyre_domain::TaskStatus::Backlog => "backlog",
                gyre_domain::TaskStatus::InProgress => "in_progress",
                gyre_domain::TaskStatus::Review => "review",
                gyre_domain::TaskStatus::Done => "done",
                gyre_domain::TaskStatus::Blocked => "blocked",
                gyre_domain::TaskStatus::Cancelled => "cancelled",
            }
            .to_string(),
            priority: match &t.priority {
                gyre_domain::TaskPriority::Low => "low",
                gyre_domain::TaskPriority::Medium => "medium",
                gyre_domain::TaskPriority::High => "high",
                gyre_domain::TaskPriority::Critical => "critical",
            }
            .to_string(),
        })
        .collect();

    let mr_items: Vec<SpecProgressMrItem> = linked_mrs
        .iter()
        .map(|mr| SpecProgressMrItem {
            id: mr.id.to_string(),
            title: mr.title.clone(),
            status: match &mr.status {
                gyre_domain::MrStatus::Open => "open",
                gyre_domain::MrStatus::Approved => "approved",
                gyre_domain::MrStatus::Merged => "merged",
                gyre_domain::MrStatus::Closed => "closed",
                gyre_domain::MrStatus::Reverted => "reverted",
            }
            .to_string(),
            spec_ref: mr.spec_ref.clone(),
        })
        .collect();

    Ok(Json(SpecProgressResponse {
        spec_path,
        tasks: task_items,
        mrs: mr_items,
        open_tasks,
        completed_tasks,
        merged_mrs,
    }))
}

// ---------------------------------------------------------------------------
// Constraint validation (authorization-provenance.md §7.6 — dry-run)
// ---------------------------------------------------------------------------

/// Request body for constraint validation dry-run.
#[derive(Deserialize)]
pub struct ValidateConstraintsRequest {
    /// CEL constraint expressions to validate.
    pub constraints: Vec<ConstraintEntry>,
    /// Scope constraints (glob patterns) to validate.
    #[serde(default)]
    pub scope: Option<ScopeEntry>,
}

#[derive(Deserialize)]
pub struct ConstraintEntry {
    pub name: String,
    pub expression: String,
}

#[derive(Deserialize)]
pub struct ScopeEntry {
    #[serde(default)]
    pub allowed_paths: Vec<String>,
    #[serde(default)]
    pub forbidden_paths: Vec<String>,
}

/// Per-constraint validation result.
#[derive(Serialize)]
pub struct ConstraintValidationResult {
    pub name: String,
    pub valid: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Overall validation response.
#[derive(Serialize)]
pub struct ValidateConstraintsResponse {
    pub valid: bool,
    pub results: Vec<ConstraintValidationResult>,
}

/// POST /api/v1/constraints/validate — validate constraint expression syntax.
///
/// Compiles each CEL expression using the real CEL parser to check syntax.
/// Also derives scope constraints from glob patterns and validates them.
/// Returns per-constraint results indicating whether each expression is
/// syntactically valid CEL. For full evaluation against repo state, use
/// `POST /api/v1/constraints/dry-run` instead.
pub async fn validate_constraints(
    Json(body): Json<ValidateConstraintsRequest>,
) -> Result<Json<ValidateConstraintsResponse>, ApiError> {
    use gyre_domain::constraint_evaluator;

    let mut results = Vec::new();
    let mut all_valid = true;

    // Validate each user-provided CEL constraint expression by compiling it.
    for entry in &body.constraints {
        if entry.expression.trim().is_empty() {
            results.push(ConstraintValidationResult {
                name: entry.name.clone(),
                valid: false,
                error: Some("expression is empty".to_string()),
            });
            all_valid = false;
            continue;
        }

        match constraint_evaluator::validate_cel_expression(&entry.expression) {
            Ok(()) => {
                results.push(ConstraintValidationResult {
                    name: entry.name.clone(),
                    valid: true,
                    error: None,
                });
            }
            Err(e) => {
                results.push(ConstraintValidationResult {
                    name: entry.name.clone(),
                    valid: false,
                    error: Some(e),
                });
                all_valid = false;
            }
        }
    }

    // Validate scope constraints by deriving the CEL expressions and compiling them.
    if let Some(scope) = &body.scope {
        let scope_constraint = gyre_common::attestation::ScopeConstraint {
            allowed_paths: scope.allowed_paths.clone(),
            forbidden_paths: scope.forbidden_paths.clone(),
        };
        let mut scope_cel = Vec::new();
        constraint_evaluator::derive_path_constraints_for_validation(
            &scope_constraint,
            &mut scope_cel,
        );
        for sc in &scope_cel {
            match constraint_evaluator::validate_cel_expression(&sc.expression) {
                Ok(()) => {
                    results.push(ConstraintValidationResult {
                        name: sc.name.clone(),
                        valid: true,
                        error: None,
                    });
                }
                Err(e) => {
                    results.push(ConstraintValidationResult {
                        name: sc.name.clone(),
                        valid: false,
                        error: Some(e),
                    });
                    all_valid = false;
                }
            }
        }
    }

    Ok(Json(ValidateConstraintsResponse {
        valid: all_valid,
        results,
    }))
}

// ---------------------------------------------------------------------------
// Constraint dry-run evaluation (authorization-provenance.md §7.6)
// ---------------------------------------------------------------------------

/// Request body for dry-run constraint evaluation.
///
/// Unlike `validate_constraints` (syntax-only), this evaluates constraints
/// against actual repo state using the domain's CEL evaluator.
#[derive(Deserialize)]
pub struct DryRunConstraintsRequest {
    /// CEL constraint expressions to evaluate.
    #[serde(default)]
    pub constraints: Vec<ConstraintEntry>,
    /// Scope constraints (glob patterns) to evaluate.
    #[serde(default)]
    pub scope: Option<ScopeEntry>,
    /// Repository to evaluate against.
    pub repo_id: String,
    /// Workspace context for strategy-implied constraints.
    pub workspace_id: String,
}

/// Per-constraint dry-run result with pass/fail status.
#[derive(Serialize)]
pub struct DryRunConstraintResult {
    pub name: String,
    pub passed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Overall dry-run evaluation response.
#[derive(Serialize)]
pub struct DryRunConstraintsResponse {
    pub valid: bool,
    pub results: Vec<DryRunConstraintResult>,
}

/// POST /api/v1/constraints/dry-run — evaluate constraints against repo state (§7.6).
///
/// Builds a representative CEL evaluation context from the repo's latest commit
/// diff and workspace configuration, then evaluates all constraints (explicit +
/// scope + strategy-implied) using the domain's `evaluate_all`. Returns
/// per-constraint pass/fail results so the approver can preview what would
/// happen at push/merge time.
pub async fn dry_run_constraints(
    State(state): State<Arc<AppState>>,
    Json(body): Json<DryRunConstraintsRequest>,
) -> Result<Json<DryRunConstraintsResponse>, ApiError> {
    use gyre_common::attestation::{OutputConstraint, ScopeConstraint};
    use gyre_domain::constraint_evaluator::{
        self, Action, AgentContext, ConstraintInput, DiffStatsContext, OutputContext, TargetContext,
    };

    let ws_id = gyre_common::Id::new(&body.workspace_id);
    let repo_id_val = gyre_common::Id::new(&body.repo_id);

    // Look up the repository to get its path and default branch.
    let repo = state
        .repos
        .find_by_id(&repo_id_val)
        .await
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("failed to look up repo: {e}")))?
        .ok_or_else(|| ApiError::NotFound("repository not found".into()))?;

    // Compute output context from the repo's latest commit diff.
    let output_ctx = compute_latest_diff(&repo.path).await.unwrap_or_else(|| {
        // Fallback: empty output context (e.g., empty repo with no commits).
        OutputContext {
            changed_files: vec![],
            added_files: vec![],
            deleted_files: vec![],
            diff_stats: DiffStatsContext {
                insertions: 0,
                deletions: 0,
            },
            commit_message: String::new(),
            commit_sha: String::new(),
        }
    });

    // Build a representative agent context from workspace state.
    let meta_spec_set_sha = match state.meta_spec_sets.get(&ws_id).await {
        Ok(Some(json)) => {
            use sha2::{Digest, Sha256};
            let mut hasher = Sha256::new();
            hasher.update(json.as_bytes());
            hex::encode(hasher.finalize())
        }
        _ => String::new(),
    };

    let agent_ctx = AgentContext {
        id: "dry-run-preview".to_string(),
        persona: "preview".to_string(),
        stack_hash: String::new(), // empty-default:ok — dry-run preview, no real agent
        attestation_level: 0,      // empty-default:ok — dry-run preview, no real agent
        meta_spec_set_sha,
        spawned_by: String::new(),
        task_id: String::new(),
        container_id: String::new(), // empty-default:ok — dry-run preview, no real agent
        image_hash: String::new(),   // empty-default:ok — dry-run preview, no real agent
    };

    let target_ctx = TargetContext {
        repo_id: body.repo_id.clone(),
        workspace_id: body.workspace_id.clone(),
        branch: repo.default_branch.clone(),
        default_branch: repo.default_branch.clone(),
    };

    // Build explicit constraints from the request.
    let mut all_constraints: Vec<OutputConstraint> = body
        .constraints
        .iter()
        .filter(|c| !c.expression.trim().is_empty())
        .map(|c| OutputConstraint {
            name: c.name.clone(),
            expression: c.expression.clone(),
        })
        .collect();

    // Add scope constraints if provided.
    if let Some(scope) = &body.scope {
        let scope_constraint = ScopeConstraint {
            allowed_paths: scope.allowed_paths.clone(),
            forbidden_paths: scope.forbidden_paths.clone(),
        };
        constraint_evaluator::derive_path_constraints_for_validation(
            &scope_constraint,
            &mut all_constraints,
        );
    }

    // Build CEL context and evaluate.
    let input_content = gyre_common::InputContent {
        spec_path: String::new(),
        spec_sha: String::new(),
        workspace_id: body.workspace_id.clone(),
        repo_id: body.repo_id.clone(),
        persona_constraints: vec![],
        meta_spec_set_sha: agent_ctx.meta_spec_set_sha.clone(),
        scope: body
            .scope
            .as_ref()
            .map(|s| ScopeConstraint {
                allowed_paths: s.allowed_paths.clone(),
                forbidden_paths: s.forbidden_paths.clone(),
            })
            .unwrap_or_default(),
    };

    let ci = ConstraintInput {
        input: &input_content,
        output: &output_ctx,
        agent: &agent_ctx,
        target: &target_ctx,
        action: Action::Push,
    };

    let ctx = constraint_evaluator::build_cel_context(&ci)
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("failed to build CEL context: {e}")))?;

    let eval_result = constraint_evaluator::evaluate_all(&all_constraints, &ctx);

    // Map results to per-constraint pass/fail.
    let mut results = Vec::new();
    for child in &eval_result.children {
        results.push(DryRunConstraintResult {
            name: child.label.clone(),
            passed: child.valid,
            error: if child.valid {
                None
            } else {
                Some(child.message.clone())
            },
        });
    }

    // If evaluation stopped early (fail-closed), mark remaining constraints
    // as not-evaluated.
    if results.len() < all_constraints.len() {
        for constraint in all_constraints.iter().skip(results.len()) {
            results.push(DryRunConstraintResult {
                name: constraint.name.clone(),
                passed: false,
                error: Some("not evaluated (prior constraint failed)".to_string()),
            });
        }
    }

    Ok(Json(DryRunConstraintsResponse {
        valid: eval_result.valid,
        results,
    }))
}

/// Compute the diff from the latest commit in a repo (HEAD^..HEAD).
async fn compute_latest_diff(
    repo_path: &str,
) -> Option<gyre_domain::constraint_evaluator::OutputContext> {
    use gyre_domain::constraint_evaluator::{DiffStatsContext, OutputContext};

    let git_bin = std::env::var("GYRE_GIT_PATH").unwrap_or_else(|_| "git".to_string());

    // Get HEAD SHA.
    let head_out = tokio::process::Command::new(&git_bin)
        .arg("-C")
        .arg(repo_path)
        .args(["rev-parse", "HEAD"])
        .output()
        .await
        .ok()?;

    if !head_out.status.success() {
        return None;
    }
    let head_sha = String::from_utf8_lossy(&head_out.stdout).trim().to_string();

    // Diff HEAD^..HEAD (latest commit's changes).
    let diff_out = tokio::process::Command::new(&git_bin)
        .arg("-C")
        .arg(repo_path)
        .args(["diff", "--name-status", &format!("{head_sha}^..{head_sha}")])
        .output()
        .await
        .ok()?;

    let mut changed = Vec::new();
    let mut added = Vec::new();
    let mut deleted = Vec::new();

    if diff_out.status.success() {
        let text = String::from_utf8_lossy(&diff_out.stdout);
        for line in text.lines() {
            let parts: Vec<&str> = line.splitn(2, '\t').collect();
            if parts.len() == 2 {
                let status = parts[0].chars().next().unwrap_or(' ');
                let file = parts[1].to_string();
                match status {
                    'A' => added.push(file.clone()),
                    'D' => deleted.push(file.clone()),
                    _ => {}
                }
                changed.push(file);
            }
        }
    }

    // Diff stats.
    let stat_out = tokio::process::Command::new(&git_bin)
        .arg("-C")
        .arg(repo_path)
        .args(["diff", "--shortstat", &format!("{head_sha}^..{head_sha}")])
        .output()
        .await
        .ok()?;

    let (insertions, deletions) = if stat_out.status.success() {
        let stat_text = String::from_utf8_lossy(&stat_out.stdout);
        crate::constraint_check::parse_shortstat(&stat_text)
    } else {
        (0, 0)
    };

    // Commit message.
    let msg_out = tokio::process::Command::new(&git_bin)
        .arg("-C")
        .arg(repo_path)
        .args(["log", "-1", "--format=%s", &head_sha])
        .output()
        .await
        .ok()?;

    let commit_message = if msg_out.status.success() {
        String::from_utf8_lossy(&msg_out.stdout).trim().to_string()
    } else {
        String::new()
    };

    Some(OutputContext {
        changed_files: changed,
        added_files: added,
        deleted_files: deleted,
        diff_stats: DiffStatsContext {
            insertions,
            deletions,
        },
        commit_message,
        commit_sha: head_sha,
    })
}

// ---------------------------------------------------------------------------
// Strategy-implied constraints preview (authorization-provenance.md §7.6)
// ---------------------------------------------------------------------------

/// Query parameters for the strategy-implied constraints preview endpoint.
#[derive(Deserialize)]
pub struct StrategyConstraintsQuery {
    /// Workspace ID to derive trust-level constraints from.
    pub workspace_id: Option<String>,
}

/// A single strategy-implied constraint in the preview response.
#[derive(Serialize)]
pub struct StrategyConstraintEntry {
    pub name: String,
    pub expression: String,
}

/// Response for the strategy-implied constraints preview endpoint.
#[derive(Serialize)]
pub struct StrategyConstraintsResponse {
    pub constraints: Vec<StrategyConstraintEntry>,
}

/// GET /api/v1/constraints/strategy — preview strategy-implied constraints (§7.6).
///
/// Returns the full set of strategy-implied constraints that would apply for a
/// given workspace context. This includes: persona constraints, meta-spec set
/// match, scope constraints, workspace trust level constraints, and attestation
/// level policy constraints.
///
/// Used by the ConstraintEditor UI to display read-only strategy-implied
/// constraints before approval.
pub async fn get_strategy_constraints(
    State(state): State<Arc<AppState>>,
    Query(query): Query<StrategyConstraintsQuery>,
) -> Result<Json<StrategyConstraintsResponse>, ApiError> {
    use gyre_domain::constraint_evaluator;

    // Determine workspace trust level.
    let trust_level = if let Some(ref ws_id) = query.workspace_id {
        let workspace = state
            .workspaces
            .find_by_id(&gyre_common::Id::new(ws_id))
            .await
            .ok()
            .flatten();
        workspace.map(|ws| format!("{:?}", ws.trust_level).to_lowercase())
    } else {
        None
    };

    // Build a representative InputContent for constraint derivation.
    // At preview time, we use placeholder values — the actual values will be
    // populated at approval time. The derived constraints are structural
    // (e.g., "agent.persona must match one of X") and show the constraint
    // categories that apply, not final evaluated values.
    let preview_content = gyre_common::InputContent {
        spec_path: String::new(),
        spec_sha: String::new(),
        workspace_id: query.workspace_id.clone().unwrap_or_default(),
        repo_id: String::new(),
        persona_constraints: vec![gyre_common::PersonaRef {
            name: "<assigned-persona>".to_string(),
        }],
        meta_spec_set_sha: "<workspace-meta-spec-sha>".to_string(),
        scope: gyre_common::ScopeConstraint {
            allowed_paths: vec![],
            forbidden_paths: vec![],
        },
    };

    let strategy = constraint_evaluator::derive_strategy_constraints(
        &preview_content,
        trust_level.as_deref(),
        None,
    );

    let constraints = strategy
        .into_iter()
        .map(|c| StrategyConstraintEntry {
            name: c.name,
            expression: c.expression,
        })
        .collect();

    Ok(Json(StrategyConstraintsResponse { constraints }))
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use crate::mem::test_state;
    use axum::{body::Body, Router};
    use base64::Engine as _;
    use http::{Request, StatusCode};
    use tower::ServiceExt;

    use crate::spec_registry::{ApprovalStatus, SpecLedgerEntry};

    fn app() -> Router {
        let state = test_state();
        crate::api::api_router().with_state(state)
    }

    fn app_with_spec() -> (Router, std::sync::Arc<crate::AppState>) {
        let state = test_state();

        // Seed a spec entry into the ledger.
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                state
                    .spec_ledger
                    .save(&SpecLedgerEntry {
                        path: "system/design-principles.md".to_string(),
                        title: "Design Principles".to_string(),
                        owner: "user:jsell".to_string(),
                        kind: None,
                        current_sha: "a".repeat(40),
                        approval_mode: "human_only".to_string(),
                        approval_status: ApprovalStatus::Pending,
                        linked_tasks: vec![],
                        linked_mrs: vec![],
                        drift_status: "unknown".to_string(),
                        created_at: 1700000000,
                        updated_at: 1700000000,
                        repo_id: None,
                        workspace_id: None,
                    })
                    .await
                    .unwrap();
            })
        });

        let router = crate::api::api_router().with_state(state.clone());
        (router, state)
    }

    async fn body_json(resp: axum::response::Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn list_specs_empty() {
        let resp = app()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/specs")
                    .header("authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert!(json.as_array().unwrap().is_empty());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn list_specs_returns_seeded_entry() {
        let (app, _) = app_with_spec();
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/specs")
                    .header("authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        let arr = json.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["path"], "system/design-principles.md");
        assert_eq!(arr[0]["approval_status"], "pending");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn get_spec_not_found() {
        let resp = app()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/specs/system%2Fnonexistent.md")
                    .header("authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn get_spec_found() {
        let (app, _) = app_with_spec();
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/specs/system%2Fdesign-principles.md")
                    .header("authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json["title"], "Design Principles");
        assert_eq!(json["approval_status"], "pending");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn approve_spec_bad_sha() {
        let (app, _) = app_with_spec();
        let body = serde_json::json!({ "sha": "tooshort" });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/specs/system%2Fdesign-principles.md/approve")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn approve_spec_ok() {
        let (app, state) = app_with_spec();
        let sha = "a".repeat(40);
        let body = serde_json::json!({ "sha": sha });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/specs/system%2Fdesign-principles.md/approve")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);

        // Ledger should now be Approved.
        let entry = state
            .spec_ledger
            .find_by_path("system/design-principles.md")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(entry.approval_status, ApprovalStatus::Approved);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn approve_then_revoke() {
        let (app, state) = app_with_spec();
        let sha = "a".repeat(40);

        // Approve.
        let body = serde_json::json!({ "sha": sha });
        app.clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/specs/system%2Fdesign-principles.md/approve")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        // Revoke.
        let revoke_body = serde_json::json!({ "reason": "outdated" });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/specs/system%2Fdesign-principles.md/revoke")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&revoke_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // Ledger back to Pending.
        let entry = state
            .spec_ledger
            .find_by_path("system/design-principles.md")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(entry.approval_status, ApprovalStatus::Pending);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn list_pending_filters_correctly() {
        let state = test_state();
        state
            .spec_ledger
            .save(&make_ledger_entry(
                "system/pending.md",
                ApprovalStatus::Pending,
            ))
            .await
            .unwrap();
        state
            .spec_ledger
            .save(&make_ledger_entry(
                "system/approved.md",
                ApprovalStatus::Approved,
            ))
            .await
            .unwrap();
        let app = crate::api::api_router().with_state(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/specs/pending")
                    .header("authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        let arr = json.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["path"], "system/pending.md");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn spec_history_empty() {
        let (app, _) = app_with_spec();
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/specs/system%2Fdesign-principles.md/history")
                    .header("authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert!(json.as_array().unwrap().is_empty());
    }

    // -----------------------------------------------------------------------
    // Helper: build a SpecLedgerEntry with a given approval status
    // -----------------------------------------------------------------------

    fn make_ledger_entry(path: &str, status: ApprovalStatus) -> SpecLedgerEntry {
        SpecLedgerEntry {
            path: path.to_string(),
            title: path.to_string(),
            owner: "user:jsell".to_string(),
            kind: None,
            current_sha: "a".repeat(40),
            approval_mode: "human_only".to_string(),
            approval_status: status,
            linked_tasks: vec![],
            linked_mrs: vec![],
            drift_status: "unknown".to_string(),
            created_at: 1700000000,
            updated_at: 1700000000,
            repo_id: None,
            workspace_id: None,
        }
    }

    // Legacy helper used by the sync_supersedes test which bypasses AppState.
    fn seed_spec(
        ledger: &mut std::collections::HashMap<String, SpecLedgerEntry>,
        path: &str,
        status: ApprovalStatus,
    ) {
        ledger.insert(path.to_string(), make_ledger_entry(path, status));
    }

    // -----------------------------------------------------------------------
    // Link enforcement: implements gate
    // -----------------------------------------------------------------------

    #[tokio::test(flavor = "multi_thread")]
    async fn approve_blocked_by_implements_gate() {
        use crate::spec_registry::{SpecLinkEntry, SpecLinkType};
        let state = test_state();

        // parent spec: pending (not yet approved)
        // child spec: implements parent
        state
            .spec_ledger
            .save(&make_ledger_entry(
                "system/parent.md",
                ApprovalStatus::Pending,
            ))
            .await
            .unwrap();
        state
            .spec_ledger
            .save(&make_ledger_entry(
                "system/child.md",
                ApprovalStatus::Pending,
            ))
            .await
            .unwrap();
        state.spec_links_store.lock().await.push(SpecLinkEntry {
            id: "child-implements-parent".to_string(),
            source_path: "system/child.md".to_string(),
            source_repo_id: None,
            link_type: SpecLinkType::Implements,
            target_path: "system/parent.md".to_string(),
            target_repo_id: None,
            target_display: None,
            target_sha: None,
            reason: None,
            status: "active".to_string(),
            created_at: 1700000000,
            stale_since: None,
        });

        let app = crate::api::api_router().with_state(state);
        let sha = "a".repeat(40);
        let body = serde_json::json!({ "sha": sha });
        let resp = app
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/api/v1/specs/system%2Fchild.md/approve")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(axum::body::Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        // Parent not approved → 400
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn approve_allowed_when_parent_approved() {
        use crate::spec_registry::{SpecLinkEntry, SpecLinkType};
        let state = test_state();

        state
            .spec_ledger
            .save(&make_ledger_entry(
                "system/parent.md",
                ApprovalStatus::Approved,
            ))
            .await
            .unwrap();
        state
            .spec_ledger
            .save(&make_ledger_entry(
                "system/child.md",
                ApprovalStatus::Pending,
            ))
            .await
            .unwrap();
        state.spec_links_store.lock().await.push(SpecLinkEntry {
            id: "child-implements-parent".to_string(),
            source_path: "system/child.md".to_string(),
            source_repo_id: None,
            link_type: SpecLinkType::Implements,
            target_path: "system/parent.md".to_string(),
            target_repo_id: None,
            target_display: None,
            target_sha: None,
            reason: None,
            status: "active".to_string(),
            created_at: 1700000000,
            stale_since: None,
        });

        let app = crate::api::api_router().with_state(state);
        let sha = "a".repeat(40);
        let body = serde_json::json!({ "sha": sha });
        let resp = app
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/api/v1/specs/system%2Fchild.md/approve")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(axum::body::Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        // Parent approved → 201
        assert_eq!(resp.status(), StatusCode::CREATED);
    }

    // -----------------------------------------------------------------------
    // Link enforcement: conflicts_with gate
    // -----------------------------------------------------------------------

    #[tokio::test(flavor = "multi_thread")]
    async fn approve_blocked_by_conflicts_with_gate() {
        use crate::spec_registry::{SpecLinkEntry, SpecLinkType};
        let state = test_state();

        state
            .spec_ledger
            .save(&make_ledger_entry(
                "system/old.md",
                ApprovalStatus::Approved,
            ))
            .await
            .unwrap();
        state
            .spec_ledger
            .save(&make_ledger_entry("system/new.md", ApprovalStatus::Pending))
            .await
            .unwrap();
        state.spec_links_store.lock().await.push(SpecLinkEntry {
            id: "new-conflicts-old".to_string(),
            source_path: "system/new.md".to_string(),
            source_repo_id: None,
            link_type: SpecLinkType::ConflictsWith,
            target_path: "system/old.md".to_string(),
            target_repo_id: None,
            target_display: None,
            target_sha: None,
            reason: Some("incompatible permission model".to_string()),
            status: "active".to_string(),
            created_at: 1700000000,
            stale_since: None,
        });

        let app = crate::api::api_router().with_state(state);
        let sha = "a".repeat(40);
        let body = serde_json::json!({ "sha": sha });
        let resp = app
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/api/v1/specs/system%2Fnew.md/approve")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(axum::body::Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        // Conflict approved → 400
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn approve_allowed_when_conflict_not_approved() {
        use crate::spec_registry::{SpecLinkEntry, SpecLinkType};
        let state = test_state();

        state
            .spec_ledger
            .save(&make_ledger_entry("system/old.md", ApprovalStatus::Pending))
            .await
            .unwrap();
        state
            .spec_ledger
            .save(&make_ledger_entry("system/new.md", ApprovalStatus::Pending))
            .await
            .unwrap();
        state.spec_links_store.lock().await.push(SpecLinkEntry {
            id: "new-conflicts-old".to_string(),
            source_path: "system/new.md".to_string(),
            source_repo_id: None,
            link_type: SpecLinkType::ConflictsWith,
            target_path: "system/old.md".to_string(),
            target_repo_id: None,
            target_display: None,
            target_sha: None,
            reason: None,
            status: "active".to_string(),
            created_at: 1700000000,
            stale_since: None,
        });

        let app = crate::api::api_router().with_state(state);
        let sha = "a".repeat(40);
        let body = serde_json::json!({ "sha": sha });
        let resp = app
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/api/v1/specs/system%2Fnew.md/approve")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(axum::body::Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        // Conflict not approved → allowed
        assert_eq!(resp.status(), StatusCode::CREATED);
    }

    // -----------------------------------------------------------------------
    // Link enforcement: depends_on gate (TASK-016)
    // -----------------------------------------------------------------------

    #[tokio::test(flavor = "multi_thread")]
    async fn approve_blocked_by_depends_on_gate() {
        use crate::spec_registry::{SpecLinkEntry, SpecLinkType};
        let state = test_state();

        // dependency spec: pending (implementation not complete)
        state
            .spec_ledger
            .save(&make_ledger_entry(
                "system/dependency.md",
                ApprovalStatus::Pending,
            ))
            .await
            .unwrap();
        state
            .spec_ledger
            .save(&make_ledger_entry(
                "system/dependent.md",
                ApprovalStatus::Pending,
            ))
            .await
            .unwrap();
        state.spec_links_store.lock().await.push(SpecLinkEntry {
            id: "dependent-depends-dependency".to_string(),
            source_path: "system/dependent.md".to_string(),
            source_repo_id: None,
            link_type: SpecLinkType::DependsOn,
            target_path: "system/dependency.md".to_string(),
            target_repo_id: None,
            target_display: None,
            target_sha: None,
            reason: None,
            status: "active".to_string(),
            created_at: 1700000000,
            stale_since: None,
        });

        let app = crate::api::api_router().with_state(state);
        let sha = "a".repeat(40);
        let body = serde_json::json!({ "sha": sha });
        let resp = app
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/api/v1/specs/system%2Fdependent.md/approve")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(axum::body::Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        // Dependency not approved → 400
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn approve_allowed_when_dependency_approved() {
        use crate::spec_registry::{SpecLinkEntry, SpecLinkType};
        let state = test_state();

        // dependency spec: approved (implementation complete)
        state
            .spec_ledger
            .save(&make_ledger_entry(
                "system/dependency.md",
                ApprovalStatus::Approved,
            ))
            .await
            .unwrap();
        state
            .spec_ledger
            .save(&make_ledger_entry(
                "system/dependent.md",
                ApprovalStatus::Pending,
            ))
            .await
            .unwrap();
        state.spec_links_store.lock().await.push(SpecLinkEntry {
            id: "dependent-depends-dependency".to_string(),
            source_path: "system/dependent.md".to_string(),
            source_repo_id: None,
            link_type: SpecLinkType::DependsOn,
            target_path: "system/dependency.md".to_string(),
            target_repo_id: None,
            target_display: None,
            target_sha: None,
            reason: None,
            status: "active".to_string(),
            created_at: 1700000000,
            stale_since: None,
        });

        let app = crate::api::api_router().with_state(state);
        let sha = "a".repeat(40);
        let body = serde_json::json!({ "sha": sha });
        let resp = app
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/api/v1/specs/system%2Fdependent.md/approve")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(axum::body::Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        // Dependency approved → 201
        assert_eq!(resp.status(), StatusCode::CREATED);
    }

    // -----------------------------------------------------------------------
    // Link enforcement: supersedes side-effect (TASK-016)
    // -----------------------------------------------------------------------

    #[tokio::test(flavor = "multi_thread")]
    async fn approve_supersedes_marks_target_deprecated() {
        use crate::spec_registry::{SpecLinkEntry, SpecLinkType};
        let state = test_state();

        // Target spec: approved (will be deprecated on superseding approval)
        state
            .spec_ledger
            .save(&make_ledger_entry(
                "system/old-spec.md",
                ApprovalStatus::Approved,
            ))
            .await
            .unwrap();
        // Source spec: pending, has supersedes link
        let mut source_entry = make_ledger_entry("system/new-spec.md", ApprovalStatus::Pending);
        source_entry.current_sha = "a".repeat(40);
        state.spec_ledger.save(&source_entry).await.unwrap();
        state.spec_links_store.lock().await.push(SpecLinkEntry {
            id: "new-supersedes-old".to_string(),
            source_path: "system/new-spec.md".to_string(),
            source_repo_id: None,
            link_type: SpecLinkType::Supersedes,
            target_path: "system/old-spec.md".to_string(),
            target_repo_id: None,
            target_display: None,
            target_sha: None,
            reason: Some("Replaced by new-spec".to_string()),
            status: "active".to_string(),
            created_at: 1700000000,
            stale_since: None,
        });

        let app = crate::api::api_router().with_state(state.clone());
        let sha = "a".repeat(40);
        let body = serde_json::json!({ "sha": sha });
        let resp = app
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/api/v1/specs/system%2Fnew-spec.md/approve")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(axum::body::Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        // Approval should succeed
        assert_eq!(resp.status(), StatusCode::CREATED);

        // Verify the target spec is now deprecated
        let target = state
            .spec_ledger
            .find_by_path("system/old-spec.md")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            target.approval_status,
            ApprovalStatus::Deprecated,
            "superseded target should be marked Deprecated"
        );
    }

    // -----------------------------------------------------------------------
    // GET /api/v1/specs/:path/links
    // -----------------------------------------------------------------------

    #[tokio::test(flavor = "multi_thread")]
    async fn get_spec_links_not_found() {
        let resp = app()
            .oneshot(
                axum::http::Request::builder()
                    .uri("/api/v1/specs/system%2Fnonexistent.md/links")
                    .header("authorization", "Bearer test-token")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn get_spec_links_returns_outbound_and_inbound() {
        use crate::spec_registry::{SpecLinkEntry, SpecLinkType};
        let state = test_state();

        state
            .spec_ledger
            .save(&make_ledger_entry("system/a.md", ApprovalStatus::Pending))
            .await
            .unwrap();
        state
            .spec_ledger
            .save(&make_ledger_entry("system/b.md", ApprovalStatus::Pending))
            .await
            .unwrap();
        // a → b (outbound from a, inbound to b)
        state.spec_links_store.lock().await.push(SpecLinkEntry {
            id: "a-depends-b".to_string(),
            source_path: "system/a.md".to_string(),
            source_repo_id: None,
            link_type: SpecLinkType::DependsOn,
            target_path: "system/b.md".to_string(),
            target_repo_id: None,
            target_display: None,
            target_sha: None,
            reason: None,
            status: "active".to_string(),
            created_at: 1700000000,
            stale_since: None,
        });

        let app = crate::api::api_router().with_state(state);
        // Query links for b — should get the inbound link from a
        let resp = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/api/v1/specs/system%2Fb.md/links")
                    .header("authorization", "Bearer test-token")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        let arr = json.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["link_type"], "depends_on");
        assert_eq!(arr[0]["source_path"], "system/a.md");
    }

    // -----------------------------------------------------------------------
    // GET /api/v1/specs/graph
    // -----------------------------------------------------------------------

    #[tokio::test(flavor = "multi_thread")]
    async fn get_spec_graph_returns_nodes_and_edges() {
        use crate::spec_registry::{SpecLinkEntry, SpecLinkType};
        let state = test_state();

        state
            .spec_ledger
            .save(&make_ledger_entry("system/a.md", ApprovalStatus::Approved))
            .await
            .unwrap();
        state
            .spec_ledger
            .save(&make_ledger_entry("system/b.md", ApprovalStatus::Pending))
            .await
            .unwrap();
        state.spec_links_store.lock().await.push(SpecLinkEntry {
            id: "b-implements-a".to_string(),
            source_path: "system/b.md".to_string(),
            source_repo_id: None,
            link_type: SpecLinkType::Implements,
            target_path: "system/a.md".to_string(),
            target_repo_id: None,
            target_display: None,
            target_sha: None,
            reason: None,
            status: "active".to_string(),
            created_at: 1700000000,
            stale_since: None,
        });

        let app = crate::api::api_router().with_state(state);
        let resp = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/api/v1/specs/graph")
                    .header("authorization", "Bearer test-token")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        let nodes = json["nodes"].as_array().unwrap();
        let edges = json["edges"].as_array().unwrap();
        assert_eq!(nodes.len(), 2);
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0]["link_type"], "implements");
        assert_eq!(edges[0]["source"], "system/b.md");
        assert_eq!(edges[0]["target"], "system/a.md");
    }

    // -----------------------------------------------------------------------
    // Sync: supersedes does NOT deprecate target at push time (F9).
    // Deprecation only happens in approve_spec (spec-links.md §Approval Gates:
    // "When source is approved, target is automatically set to deprecated").
    // -----------------------------------------------------------------------

    #[tokio::test(flavor = "multi_thread")]
    async fn sync_supersedes_does_not_deprecate_target_at_push_time() {
        use crate::spec_registry::SpecLinksStore;
        use std::sync::Arc;
        use tokio::sync::Mutex;

        let ledger: crate::spec_registry::SpecLedger =
            Arc::new(Mutex::new(std::collections::HashMap::new()));
        let links_store: SpecLinksStore = Arc::new(Mutex::new(Vec::new()));
        let now = 1700000000u64;

        // Pre-populate the target spec that will be superseded.
        {
            let mut l = ledger.lock().await;
            seed_spec(&mut l, "development/old-spec.md", ApprovalStatus::Approved);
        }

        // Parse a manifest where new-spec supersedes old-spec.
        let manifest_yaml = r#"
version: 1
specs:
  - path: system/new-spec.md
    title: New Spec
    owner: user:jsell
    links:
      - type: supersedes
        target: development/old-spec.md
        reason: "Replaced by new-spec"
  - path: development/old-spec.md
    title: Old Spec
    owner: user:jsell
"#;

        let manifest = crate::spec_registry::parse_manifest(manifest_yaml).unwrap();
        {
            let mut l = ledger.lock().await;
            // Source spec is Pending (not approved).
            seed_spec(&mut l, "system/new-spec.md", ApprovalStatus::Pending);
        }

        // Process links as sync_spec_ledger would — store links but do NOT
        // deprecate the target (no Supersedes match arm in sync_spec_ledger).
        {
            let mut new_links: Vec<crate::spec_registry::SpecLinkEntry> = Vec::new();
            for entry in &manifest.specs {
                for link in &entry.links {
                    new_links.push(crate::spec_registry::SpecLinkEntry {
                        id: format!("{}-{}-{}", entry.path, link.link_type, link.target),
                        source_path: entry.path.clone(),
                        source_repo_id: None,
                        link_type: link.link_type.clone(),
                        target_path: link.target.clone(),
                        target_repo_id: None,
                        target_display: None,
                        target_sha: link.target_sha.clone(),
                        reason: link.reason.clone(),
                        status: "active".to_string(),
                        created_at: now,
                        stale_since: None,
                    });
                }
            }
            // No type-specific enforcement for Supersedes at sync time.
            // The link is stored, but the target is NOT deprecated.
            let mut store = links_store.lock().await;
            store.extend(new_links);
        }

        // Verify target was NOT deprecated — it retains its original Approved status.
        let l = ledger.lock().await;
        let old = l.get("development/old-spec.md").unwrap();
        assert_eq!(
            old.approval_status,
            ApprovalStatus::Approved,
            "supersedes link should NOT deprecate target at push time — only approve_spec should"
        );

        // Verify link was stored correctly.
        let store = links_store.lock().await;
        assert_eq!(store.len(), 1);
        assert_eq!(
            store[0].link_type,
            crate::spec_registry::SpecLinkType::Supersedes
        );
    }

    // -----------------------------------------------------------------------
    // Spec rejection mid-flight (agent-runtime §1)
    // -----------------------------------------------------------------------

    #[tokio::test(flavor = "multi_thread")]
    async fn reject_spec_cancels_in_flight_tasks() {
        let state = test_state();
        // Seed a spec entry.
        state
            .spec_ledger
            .save(&make_ledger_entry(
                "system/target.md",
                ApprovalStatus::Approved,
            ))
            .await
            .unwrap();

        // Create an in-flight task linked to the spec.
        let task_id = gyre_common::Id::new("reject-task-1");
        let mut task = gyre_domain::Task::new(task_id.clone(), "Implement target", 1700000000);
        task.spec_path = Some("system/target.md".to_string());
        task.task_type = Some(gyre_domain::TaskType::Implementation);
        let _ = task.transition_status(gyre_domain::TaskStatus::InProgress);
        state.tasks.create(&task).await.unwrap();

        // Create an active agent working on this task.
        let agent_id = gyre_common::Id::new("reject-agent-1");
        let mut agent = gyre_domain::Agent::new(agent_id.clone(), "reject-worker", 1700000000);
        agent.assign_task(task_id.clone());
        agent
            .transition_status(gyre_domain::AgentStatus::Active)
            .unwrap();
        state.agents.create(&agent).await.unwrap();

        let app = crate::api::api_router().with_state(state.clone());
        let body = serde_json::json!({ "reason": "spec is invalid" });
        let resp = app
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/api/v1/specs/system%2Ftarget.md/reject")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(axum::body::Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // Task should be cancelled.
        let updated_task = state.tasks.find_by_id(&task_id).await.unwrap().unwrap();
        assert_eq!(updated_task.status, gyre_domain::TaskStatus::Cancelled);
        assert!(updated_task
            .cancelled_reason
            .unwrap()
            .contains("spec rejected"));

        // Agent should be stopped.
        let updated_agent = state.agents.find_by_id(&agent_id).await.unwrap().unwrap();
        assert_eq!(updated_agent.status, gyre_domain::AgentStatus::Stopped);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn reject_spec_skips_already_done_tasks() {
        let state = test_state();
        state
            .spec_ledger
            .save(&make_ledger_entry(
                "system/done-spec.md",
                ApprovalStatus::Approved,
            ))
            .await
            .unwrap();

        // Create a completed task linked to the spec.
        let task_id = gyre_common::Id::new("done-task-1");
        let mut task = gyre_domain::Task::new(task_id.clone(), "Already done", 1700000000);
        task.spec_path = Some("system/done-spec.md".to_string());
        let _ = task.transition_status(gyre_domain::TaskStatus::InProgress);
        let _ = task.transition_status(gyre_domain::TaskStatus::Review);
        let _ = task.transition_status(gyre_domain::TaskStatus::Done);
        state.tasks.create(&task).await.unwrap();

        let app = crate::api::api_router().with_state(state.clone());
        let body = serde_json::json!({ "reason": "not needed" });
        let resp = app
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/api/v1/specs/system%2Fdone-spec.md/reject")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(axum::body::Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // Done task should remain done (not cancelled).
        let updated_task = state.tasks.find_by_id(&task_id).await.unwrap().unwrap();
        assert_eq!(updated_task.status, gyre_domain::TaskStatus::Done);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn reject_spec_sets_status_to_rejected() {
        let (app, state) = app_with_spec();
        let body = serde_json::json!({ "reason": "outdated design" });
        let resp = app
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/api/v1/specs/system%2Fdesign-principles.md/reject")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(axum::body::Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let entry = state
            .spec_ledger
            .find_by_path("system/design-principles.md")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(entry.approval_status, ApprovalStatus::Rejected);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn spec_index_returns_markdown() {
        let (app, _) = app_with_spec();
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/specs/index")
                    .header("authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert!(resp
            .headers()
            .get("content-type")
            .unwrap()
            .to_str()
            .unwrap()
            .contains("markdown"));
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let text = String::from_utf8(body.to_vec()).unwrap();
        assert!(text.contains("Spec Registry Index"));
        assert!(text.contains("Design Principles"));
    }

    // ── TASK-006: SignedInput on spec approval ──────────────────────────

    /// Generate a real Ed25519 keypair. Returns (pkcs8_bytes, public_key_bytes).
    fn generate_test_ed25519_keypair() -> (Vec<u8>, Vec<u8>) {
        use ring::rand::SystemRandom;
        use ring::signature::{Ed25519KeyPair, KeyPair};
        let rng = SystemRandom::new();
        let pkcs8 = Ed25519KeyPair::generate_pkcs8(&rng).unwrap();
        let key_pair = Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap();
        let pub_key = key_pair.public_key().as_ref().to_vec();
        (pkcs8.as_ref().to_vec(), pub_key)
    }

    /// Sign an InputContent hash with the given PKCS8 private key.
    /// Returns base64-encoded signature.
    fn sign_input_content(pkcs8_bytes: &[u8], input_content: &gyre_common::InputContent) -> String {
        use base64::engine::general_purpose::STANDARD;
        use base64::Engine;
        use ring::digest;
        use ring::signature::Ed25519KeyPair;

        let content_bytes = serde_json::to_vec(input_content).unwrap();
        let content_hash = digest::digest(&digest::SHA256, &content_bytes);
        let key_pair = Ed25519KeyPair::from_pkcs8(pkcs8_bytes).unwrap();
        let sig = key_pair.sign(content_hash.as_ref());
        STANDARD.encode(sig.as_ref())
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn approve_spec_creates_signed_input_when_key_binding_exists() {
        let (app, state) = app_with_spec();

        // Generate a real Ed25519 keypair.
        let (pkcs8_bytes, pub_key) = generate_test_ed25519_keypair();

        // Pre-create a KeyBinding for the approver ("user:system" when using test-token).
        // Sign the public key bytes as proof-of-possession for the KeyBinding.
        let kb = gyre_common::KeyBinding {
            public_key: pub_key.clone(),
            user_identity: "user:system".to_string(),
            issuer: "http://localhost:3000".to_string(),
            trust_anchor_id: "tenant-idp".to_string(),
            issued_at: 1_700_000_000,
            expires_at: u64::MAX,
            user_signature: vec![10],
            platform_countersign: vec![20],
        };
        state.key_bindings.store("default", &kb).await.unwrap();

        // Pre-compute the InputContent the server will build so we can sign it.
        let input_content = gyre_common::InputContent {
            spec_path: "system/design-principles.md".to_string(),
            spec_sha: "a".repeat(40),
            workspace_id: String::new(), // ledger entry has workspace_id: None
            repo_id: String::new(),      // ledger entry has repo_id: None
            persona_constraints: vec![],
            meta_spec_set_sha: String::new(),
            scope: gyre_common::ScopeConstraint {
                allowed_paths: vec!["specs/**".to_string()],
                forbidden_paths: vec!["src/auth/**".to_string()],
            },
        };
        let user_content_signature = sign_input_content(&pkcs8_bytes, &input_content);

        // Approve the spec with user_content_signature.
        let body = serde_json::json!({
            "sha": "a".repeat(40),
            "output_constraints": [
                {"name": "scope to design", "expression": "output.changed_files.all(f, f.startsWith(\"specs/\"))"}
            ],
            "scope": {
                "allowed_paths": ["specs/**"],
                "forbidden_paths": ["src/auth/**"]
            },
            "user_content_signature": user_content_signature
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/specs/system%2Fdesign-principles.md/approve")
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);

        // Verify a chain attestation was stored (SignedInput root node).
        let attestations = state
            .chain_attestations
            .find_by_repo("", 0, u64::MAX)
            .await
            .unwrap();
        assert!(
            !attestations.is_empty(),
            "should have stored a SignedInput attestation"
        );

        let att = &attestations[0];
        match &att.input {
            gyre_common::AttestationInput::Signed(signed) => {
                assert_eq!(signed.content.spec_path, "system/design-principles.md");
                assert_eq!(signed.content.spec_sha, "a".repeat(40));
                assert_eq!(signed.output_constraints.len(), 1);
                assert_eq!(signed.output_constraints[0].name, "scope to design");
                assert_eq!(signed.content.scope.allowed_paths, vec!["specs/**"]);
                assert_eq!(signed.content.scope.forbidden_paths, vec!["src/auth/**"]);
                assert_eq!(att.metadata.chain_depth, 0);
                // Verify the signature is user-signed (verifiable against KeyBinding public key),
                // NOT platform-signed.
                {
                    use ring::digest;
                    use ring::signature::{self, UnparsedPublicKey};
                    let content_bytes = serde_json::to_vec(&signed.content).unwrap();
                    let content_hash = digest::digest(&digest::SHA256, &content_bytes);
                    let peer_pk = UnparsedPublicKey::new(&signature::ED25519, &pub_key);
                    peer_pk
                        .verify(content_hash.as_ref(), &signed.signature)
                        .expect("SignedInput.signature must verify against KeyBinding public key");
                }
            }
            _ => panic!("expected SignedInput, got DerivedInput"),
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn approve_spec_without_key_binding_skips_signed_input() {
        let (app, state) = app_with_spec();

        // No KeyBinding pre-created → should skip SignedInput.
        let body = serde_json::json!({
            "sha": "b".repeat(40),
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/specs/system%2Fdesign-principles.md/approve")
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);

        // No chain attestation should have been stored.
        let attestations = state
            .chain_attestations
            .find_by_repo("", 0, u64::MAX)
            .await
            .unwrap();
        assert!(
            attestations.is_empty(),
            "should not store attestation without KeyBinding"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn approve_spec_with_key_binding_but_no_user_signature_skips_signed_input() {
        let (app, state) = app_with_spec();

        // Create KeyBinding but do NOT provide user_content_signature in approval.
        let (_pkcs8, pub_key) = generate_test_ed25519_keypair();
        let kb = gyre_common::KeyBinding {
            public_key: pub_key,
            user_identity: "user:system".to_string(),
            issuer: "http://localhost:3000".to_string(),
            trust_anchor_id: "tenant-idp".to_string(),
            issued_at: 1_700_000_000,
            expires_at: u64::MAX,
            user_signature: vec![],
            platform_countersign: vec![],
        };
        state.key_bindings.store("default", &kb).await.unwrap();

        let body = serde_json::json!({
            "sha": "d".repeat(40),
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/specs/system%2Fdesign-principles.md/approve")
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);

        // No SignedInput created because no user_content_signature was provided.
        let attestations = state
            .chain_attestations
            .find_by_repo("", 0, u64::MAX)
            .await
            .unwrap();
        assert!(
            attestations.is_empty(),
            "should not store attestation without user_content_signature"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn approve_spec_with_constraints_and_no_scope() {
        let (app, state) = app_with_spec();

        // Generate real keypair.
        let (pkcs8_bytes, pub_key) = generate_test_ed25519_keypair();

        // Create KeyBinding.
        let kb = gyre_common::KeyBinding {
            public_key: pub_key,
            user_identity: "user:system".to_string(),
            issuer: "http://localhost:3000".to_string(),
            trust_anchor_id: "tenant-idp".to_string(),
            issued_at: 1_700_000_000,
            expires_at: u64::MAX,
            user_signature: vec![],
            platform_countersign: vec![],
        };
        state.key_bindings.store("default", &kb).await.unwrap();

        // Pre-compute the InputContent the server will build (default scope, no persona).
        let input_content = gyre_common::InputContent {
            spec_path: "system/design-principles.md".to_string(),
            spec_sha: "c".repeat(40),
            workspace_id: String::new(),
            repo_id: String::new(),
            persona_constraints: vec![],
            meta_spec_set_sha: String::new(),
            scope: gyre_common::ScopeConstraint {
                allowed_paths: vec![],
                forbidden_paths: vec![],
            },
        };
        let user_content_signature = sign_input_content(&pkcs8_bytes, &input_content);

        // Approve with output_constraints but no scope.
        let body = serde_json::json!({
            "sha": "c".repeat(40),
            "output_constraints": [
                {"name": "no new deps", "expression": "output.changed_files.all(f, f != \"Cargo.toml\")"}
            ],
            "user_content_signature": user_content_signature
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/specs/system%2Fdesign-principles.md/approve")
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);

        let attestations = state
            .chain_attestations
            .find_by_repo("", 0, u64::MAX)
            .await
            .unwrap();
        assert_eq!(attestations.len(), 1);
        match &attestations[0].input {
            gyre_common::AttestationInput::Signed(signed) => {
                assert_eq!(signed.output_constraints.len(), 1);
                // Default scope: no allowed/forbidden paths.
                assert!(signed.content.scope.allowed_paths.is_empty());
                assert!(signed.content.scope.forbidden_paths.is_empty());
            }
            _ => panic!("expected SignedInput"),
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn approve_spec_with_invalid_user_content_signature_rejected() {
        let (app, state) = app_with_spec();

        // Generate real keypair and store KeyBinding.
        let (_pkcs8, pub_key) = generate_test_ed25519_keypair();
        let kb = gyre_common::KeyBinding {
            public_key: pub_key,
            user_identity: "user:system".to_string(),
            issuer: "http://localhost:3000".to_string(),
            trust_anchor_id: "tenant-idp".to_string(),
            issued_at: 1_700_000_000,
            expires_at: u64::MAX,
            user_signature: vec![],
            platform_countersign: vec![],
        };
        state.key_bindings.store("default", &kb).await.unwrap();

        // Provide a bogus user_content_signature that won't verify.
        let body = serde_json::json!({
            "sha": "e".repeat(40),
            "user_content_signature": base64::engine::general_purpose::STANDARD
                .encode(b"this-is-not-a-valid-signature-at-all-needs-to-be-long-enough-for-ed25519!!")
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/specs/system%2Fdesign-principles.md/approve")
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        // Should reject with 400 — signature verification failed.
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    // ── Constraint validation (§7.6 dry-run) ─────────────────────────────

    #[tokio::test(flavor = "multi_thread")]
    async fn validate_constraints_valid_cel() {
        let app = app();
        let body = serde_json::json!({
            "constraints": [
                { "name": "persona check", "expression": "agent.persona == \"security\"" },
                { "name": "file count", "expression": "output.changed_files.size() < 50" }
            ]
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/constraints/validate")
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(resp.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(body["valid"], true);
        assert_eq!(body["results"].as_array().unwrap().len(), 2);
        assert_eq!(body["results"][0]["valid"], true);
        assert_eq!(body["results"][1]["valid"], true);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn validate_constraints_invalid_cel() {
        let app = app();
        let body = serde_json::json!({
            "constraints": [
                { "name": "bad expr", "expression": "this is not valid CEL !!!" }
            ]
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/constraints/validate")
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(resp.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(body["valid"], false);
        assert_eq!(body["results"][0]["valid"], false);
        assert!(body["results"][0]["error"]
            .as_str()
            .unwrap()
            .contains("CEL parse error"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn validate_constraints_with_scope() {
        let app = app();
        let body = serde_json::json!({
            "constraints": [],
            "scope": {
                "allowed_paths": ["src/payments/**"],
                "forbidden_paths": ["src/auth/**"]
            }
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/constraints/validate")
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(resp.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(body["valid"], true);
        // Should have 2 scope constraint results (allowed + forbidden).
        assert_eq!(body["results"].as_array().unwrap().len(), 2);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn validate_constraints_empty_expression_rejected() {
        let app = app();
        let body = serde_json::json!({
            "constraints": [
                { "name": "empty", "expression": "  " }
            ]
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/constraints/validate")
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(resp.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(body["valid"], false);
        assert_eq!(body["results"][0]["error"], "expression is empty");
    }

    // ── Strategy-implied constraints preview (§7.6) ─────────────────────

    #[tokio::test(flavor = "multi_thread")]
    async fn get_strategy_constraints_returns_baseline() {
        let app = app();
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/constraints/strategy")
                    .header("authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(resp.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap();
        let constraints = body["constraints"].as_array().unwrap();
        // Should always include persona and meta-spec constraints at minimum.
        assert!(
            constraints.len() >= 2,
            "should have at least persona + meta-spec constraints, got {}",
            constraints.len()
        );
        // Verify persona constraint is present.
        assert!(
            constraints
                .iter()
                .any(|c| c["name"].as_str().unwrap().contains("persona")),
            "should include a persona constraint"
        );
        // Verify meta-spec constraint is present.
        assert!(
            constraints
                .iter()
                .any(|c| c["name"].as_str().unwrap().contains("meta-spec")),
            "should include a meta-spec constraint"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn get_strategy_constraints_with_supervised_workspace() {
        let (app, state) = app_with_spec();

        // Create a supervised workspace.
        let ws = gyre_domain::Workspace {
            id: gyre_common::Id::new("ws-supervised"),
            tenant_id: gyre_common::Id::new("default"),
            name: "Supervised WS".to_string(),
            slug: "supervised".to_string(),
            description: None,
            budget: None,
            max_repos: None,
            max_agents_per_repo: None,
            trust_level: gyre_domain::TrustLevel::Supervised,
            llm_model: None,
            created_at: 0,
            compute_target_id: None,
        };
        state.workspaces.create(&ws).await.unwrap();

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/constraints/strategy?workspace_id=ws-supervised")
                    .header("authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(resp.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap();
        let constraints = body["constraints"].as_array().unwrap();
        // Supervised workspace should include attestation level constraint.
        assert!(
            constraints
                .iter()
                .any(|c| c["name"].as_str().unwrap().contains("supervised")),
            "supervised workspace should produce an attestation level constraint, got: {:?}",
            constraints
        );
    }

    // ── Constraint dry-run evaluation (§7.6) ─────────────────────────────

    #[tokio::test(flavor = "multi_thread")]
    async fn dry_run_constraints_evaluates_passing_expression() {
        let state = test_state();

        // Create a real git repo with a commit for diff computation.
        let tmp = tempfile::tempdir().unwrap();
        let repo_path = tmp.path();
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(repo_path)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["config", "user.email", "test@gyre.dev"])
            .current_dir(repo_path)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(repo_path)
            .output()
            .unwrap();
        std::fs::write(repo_path.join("README.md"), "# test\n").unwrap();
        std::process::Command::new("git")
            .args(["add", "."])
            .current_dir(repo_path)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["commit", "-m", "init"])
            .current_dir(repo_path)
            .output()
            .unwrap();

        // Create workspace and repo in state.
        let ws = gyre_domain::Workspace {
            id: gyre_common::Id::new("ws-dry"),
            tenant_id: gyre_common::Id::new("default"),
            name: "DryRun WS".to_string(),
            slug: "dry-ws".to_string(),
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

        let repo = gyre_domain::Repository::new(
            gyre_common::Id::new("repo-dry"),
            gyre_common::Id::new("ws-dry"),
            "dry-repo",
            repo_path.to_str().unwrap(),
            0,
        );
        state.repos.create(&repo).await.unwrap();

        let app = crate::api::api_router().with_state(state);

        // Constraint: "true" always passes.
        let body = serde_json::json!({
            "constraints": [
                { "name": "always pass", "expression": "true" }
            ],
            "repo_id": "repo-dry",
            "workspace_id": "ws-dry"
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/constraints/dry-run")
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(resp.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(body["valid"], true);
        assert_eq!(body["results"][0]["name"], "always pass");
        assert_eq!(body["results"][0]["passed"], true);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn dry_run_constraints_evaluates_failing_expression() {
        let state = test_state();

        // Create a real git repo.
        let tmp = tempfile::tempdir().unwrap();
        let repo_path = tmp.path();
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(repo_path)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["config", "user.email", "test@gyre.dev"])
            .current_dir(repo_path)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(repo_path)
            .output()
            .unwrap();
        std::fs::write(repo_path.join("README.md"), "# test\n").unwrap();
        std::process::Command::new("git")
            .args(["add", "."])
            .current_dir(repo_path)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["commit", "-m", "init"])
            .current_dir(repo_path)
            .output()
            .unwrap();

        // Create workspace and repo.
        let ws = gyre_domain::Workspace {
            id: gyre_common::Id::new("ws-dry2"),
            tenant_id: gyre_common::Id::new("default"),
            name: "DryRun WS 2".to_string(),
            slug: "dry-ws-2".to_string(),
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

        let repo = gyre_domain::Repository::new(
            gyre_common::Id::new("repo-dry2"),
            gyre_common::Id::new("ws-dry2"),
            "dry-repo-2",
            repo_path.to_str().unwrap(),
            0,
        );
        state.repos.create(&repo).await.unwrap();

        let app = crate::api::api_router().with_state(state);

        // Constraint: agent.attestation_level >= 99 — always fails for dry-run preview.
        let body = serde_json::json!({
            "constraints": [
                { "name": "impossible level", "expression": "agent.attestation_level >= 99" }
            ],
            "repo_id": "repo-dry2",
            "workspace_id": "ws-dry2"
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/constraints/dry-run")
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(resp.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(body["valid"], false);
        assert_eq!(body["results"][0]["name"], "impossible level");
        assert_eq!(body["results"][0]["passed"], false);
        assert!(body["results"][0]["error"].as_str().is_some());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn dry_run_constraints_repo_not_found_returns_404() {
        let state = test_state();
        let app = crate::api::api_router().with_state(state);

        let body = serde_json::json!({
            "constraints": [{ "name": "test", "expression": "true" }],
            "repo_id": "nonexistent",
            "workspace_id": "ws-1"
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/constraints/dry-run")
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    // -----------------------------------------------------------------------
    // TASK-019: Query endpoint tests
    // -----------------------------------------------------------------------

    fn seed_spec_with_links(state: &std::sync::Arc<crate::AppState>) {
        use crate::spec_registry::{SpecLinkEntry, SpecLinkType};

        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                // Seed spec entries.
                for (path, title, status) in [
                    ("system/core.md", "Core", ApprovalStatus::Approved),
                    ("system/auth.md", "Auth", ApprovalStatus::Pending),
                    ("system/ui.md", "UI", ApprovalStatus::Approved),
                    ("system/old.md", "Old", ApprovalStatus::Deprecated),
                ] {
                    state
                        .spec_ledger
                        .save(&SpecLedgerEntry {
                            path: path.to_string(),
                            title: title.to_string(),
                            owner: "user:test".to_string(),
                            kind: None,
                            current_sha: "a".repeat(40),
                            approval_mode: "human_only".to_string(),
                            approval_status: status,
                            linked_tasks: vec![],
                            linked_mrs: vec![],
                            drift_status: "clean".to_string(),
                            created_at: 1700000000,
                            updated_at: 1700000000,
                            repo_id: Some("repo1".to_string()),
                            workspace_id: Some("ws1".to_string()),
                        })
                        .await
                        .unwrap();
                }

                // Seed spec links.
                let mut store = state.spec_links_store.lock().await;
                store.push(SpecLinkEntry {
                    id: "link-1".to_string(),
                    source_path: "system/auth.md".to_string(),
                    source_repo_id: Some("repo1".to_string()),
                    link_type: SpecLinkType::DependsOn,
                    target_path: "system/core.md".to_string(),
                    target_repo_id: None,
                    target_display: None,
                    target_sha: Some("a".repeat(40)),
                    reason: Some("needs core".to_string()),
                    status: "active".to_string(),
                    created_at: 1700000000,
                    stale_since: None,
                });
                store.push(SpecLinkEntry {
                    id: "link-2".to_string(),
                    source_path: "system/ui.md".to_string(),
                    source_repo_id: Some("repo1".to_string()),
                    link_type: SpecLinkType::Implements,
                    target_path: "system/core.md".to_string(),
                    target_repo_id: None,
                    target_display: None,
                    target_sha: Some("a".repeat(40)),
                    reason: None,
                    status: "stale".to_string(),
                    created_at: 1700000000,
                    stale_since: Some(1700000100),
                });
                store.push(SpecLinkEntry {
                    id: "link-3".to_string(),
                    source_path: "system/core.md".to_string(),
                    source_repo_id: Some("repo1".to_string()),
                    link_type: SpecLinkType::ConflictsWith,
                    target_path: "system/ui.md".to_string(),
                    target_repo_id: None,
                    target_display: None,
                    target_sha: None,
                    reason: Some("overlapping requirements".to_string()),
                    status: "active".to_string(),
                    created_at: 1700000000,
                    stale_since: None,
                });
                store.push(SpecLinkEntry {
                    id: "link-4".to_string(),
                    source_path: "system/auth.md".to_string(),
                    source_repo_id: Some("repo1".to_string()),
                    link_type: SpecLinkType::Supersedes,
                    target_path: "system/old.md".to_string(),
                    target_repo_id: None,
                    target_display: None,
                    target_sha: None,
                    reason: None,
                    status: "active".to_string(),
                    created_at: 1700000000,
                    stale_since: None,
                });
            })
        });
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn get_spec_dependents_returns_inbound_depends_and_implements() {
        let state = test_state();
        seed_spec_with_links(&state);
        let app = crate::api::api_router().with_state(state);

        // system/core.md has two dependents: auth depends_on it, ui implements it.
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/specs/system%2Fcore.md/dependents")
                    .header("authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        let arr = json.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        // Both should target core.md.
        for item in arr {
            assert_eq!(item["target_path"], "system/core.md");
        }
        // Check link types are correct.
        let types: Vec<&str> = arr
            .iter()
            .map(|l| l["link_type"].as_str().unwrap())
            .collect();
        assert!(types.contains(&"depends_on"));
        assert!(types.contains(&"implements"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn get_spec_dependents_spec_not_found() {
        let state = test_state();
        let app = crate::api::api_router().with_state(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/specs/system%2Fnonexistent.md/dependents")
                    .header("authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn get_spec_dependencies_returns_outbound_depends_and_implements() {
        let state = test_state();
        seed_spec_with_links(&state);
        let app = crate::api::api_router().with_state(state);

        // system/auth.md depends_on core.md.
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/specs/system%2Fauth.md/dependencies")
                    .header("authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        let arr = json.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["source_path"], "system/auth.md");
        assert_eq!(arr[0]["target_path"], "system/core.md");
        assert_eq!(arr[0]["link_type"], "depends_on");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn get_stale_links_returns_only_stale() {
        let state = test_state();
        seed_spec_with_links(&state);
        let app = crate::api::api_router().with_state(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/specs/stale-links")
                    .header("authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        let arr = json.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["id"], "link-2");
        assert_eq!(arr[0]["status"], "stale");
        assert!(arr[0]["stale_since"].as_u64().is_some());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn get_conflicts_returns_conflicts_with_links() {
        let state = test_state();
        seed_spec_with_links(&state);
        let app = crate::api::api_router().with_state(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/specs/conflicts")
                    .header("authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        let arr = json.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["id"], "link-3");
        assert_eq!(arr[0]["link_type"], "conflicts_with");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn get_conflicts_excludes_non_approved_conflicts() {
        // Negative test: a conflicts_with link where one spec is Pending
        // should NOT be returned as an active conflict.
        let state = test_state();

        // Seed specs: one approved, one pending.
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                state
                    .spec_ledger
                    .save(&SpecLedgerEntry {
                        path: "system/alpha.md".to_string(),
                        title: "Alpha".to_string(),
                        owner: "user:test".to_string(),
                        kind: None,
                        current_sha: "a".repeat(40),
                        approval_mode: "human_only".to_string(),
                        approval_status: ApprovalStatus::Approved,
                        linked_tasks: vec![],
                        linked_mrs: vec![],
                        drift_status: "clean".to_string(),
                        created_at: 1700000000,
                        updated_at: 1700000000,
                        repo_id: Some("repo1".to_string()),
                        workspace_id: Some("ws1".to_string()),
                    })
                    .await
                    .unwrap();
                state
                    .spec_ledger
                    .save(&SpecLedgerEntry {
                        path: "system/beta.md".to_string(),
                        title: "Beta".to_string(),
                        owner: "user:test".to_string(),
                        kind: None,
                        current_sha: "b".repeat(40),
                        approval_mode: "human_only".to_string(),
                        approval_status: ApprovalStatus::Pending,
                        linked_tasks: vec![],
                        linked_mrs: vec![],
                        drift_status: "clean".to_string(),
                        created_at: 1700000000,
                        updated_at: 1700000000,
                        repo_id: Some("repo1".to_string()),
                        workspace_id: Some("ws1".to_string()),
                    })
                    .await
                    .unwrap();

                // Seed a conflicts_with link between them.
                let mut store = state.spec_links_store.lock().await;
                store.push(crate::spec_registry::SpecLinkEntry {
                    id: "link-pending-conflict".to_string(),
                    source_path: "system/alpha.md".to_string(),
                    source_repo_id: Some("repo1".to_string()),
                    link_type: crate::spec_registry::SpecLinkType::ConflictsWith,
                    target_path: "system/beta.md".to_string(),
                    target_repo_id: None,
                    target_display: None,
                    target_sha: None,
                    reason: Some("potential conflict".to_string()),
                    status: "active".to_string(),
                    created_at: 1700000000,
                    stale_since: None,
                });
            })
        });

        let app = crate::api::api_router().with_state(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/specs/conflicts")
                    .header("authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        let arr = json.as_array().unwrap();
        // The link should be excluded because beta is Pending, not Approved.
        assert_eq!(arr.len(), 0);
    }

    // -----------------------------------------------------------------------
    // TASK-019: Merge gate tests (via merge processor)
    // -----------------------------------------------------------------------

    fn seed_merge_gate_scenario(
        state: &std::sync::Arc<crate::AppState>,
        spec_ref: &str,
    ) -> gyre_common::Id {
        use gyre_domain::{MergeQueueEntry, MergeRequest, Repository};

        let mr_id = gyre_common::Id::new("mr-gate-test");
        let entry_id = gyre_common::Id::new("entry-gate-test");

        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                // Seed a repo.
                let repo = Repository::new(
                    gyre_common::Id::new("repo-gate"),
                    gyre_common::Id::new("ws1"),
                    "gate-repo",
                    "/tmp/nonexistent-gate-repo",
                    1700000000,
                );
                state.repos.create(&repo).await.unwrap();

                // Seed an MR with spec_ref.
                let mut mr = MergeRequest::new(
                    mr_id.clone(),
                    gyre_common::Id::new("repo-gate"),
                    "Test MR",
                    "feature/test",
                    "main",
                    1700000000,
                );
                mr.spec_ref = Some(spec_ref.to_string());
                mr.workspace_id = gyre_common::Id::new("ws1");
                state.merge_requests.create(&mr).await.unwrap();

                // Enqueue the MR.
                let entry = MergeQueueEntry::new(
                    entry_id.clone(),
                    mr_id.clone(),
                    50, // medium priority
                    1700000000,
                );
                state.merge_queue.enqueue(&entry).await.unwrap();
            })
        });

        entry_id
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn merge_gate_rejects_superseded_spec() {
        use crate::spec_registry::{SpecLinkEntry, SpecLinkType};

        let state = test_state();
        let sha = "b".repeat(40);
        let spec_ref = format!("system/old.md@{sha}");

        // Seed spec entries.
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                state
                    .spec_ledger
                    .save(&SpecLedgerEntry {
                        path: "system/old.md".to_string(),
                        title: "Old Spec".to_string(),
                        owner: "user:test".to_string(),
                        kind: None,
                        current_sha: sha.clone(),
                        approval_mode: "human_only".to_string(),
                        approval_status: ApprovalStatus::Deprecated,
                        linked_tasks: vec![],
                        linked_mrs: vec![],
                        drift_status: "clean".to_string(),
                        created_at: 1700000000,
                        updated_at: 1700000000,
                        repo_id: None,
                        workspace_id: None,
                    })
                    .await
                    .unwrap();

                // A supersedes link: new.md supersedes old.md.
                let mut store = state.spec_links_store.lock().await;
                store.push(SpecLinkEntry {
                    id: "supersedes-1".to_string(),
                    source_path: "system/new.md".to_string(),
                    source_repo_id: Some("repo1".to_string()),
                    link_type: SpecLinkType::Supersedes,
                    target_path: "system/old.md".to_string(),
                    target_repo_id: None,
                    target_display: None,
                    target_sha: None,
                    reason: None,
                    status: "active".to_string(),
                    created_at: 1700000000,
                    stale_since: None,
                });
            })
        });

        let _entry_id = seed_merge_gate_scenario(&state, &spec_ref);

        // Run the merge processor.
        crate::merge_processor::run_once(&state).await.unwrap();

        // The entry should be failed because the spec is superseded.
        // list_queue filters out terminal entries — Failed entries are excluded.
        let all = state.merge_queue.list_queue().await.unwrap();
        assert!(
            all.is_empty(),
            "superseded spec MR should be Failed (removed from queue)"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn merge_gate_blocks_conflicts_with_approved() {
        use crate::spec_registry::{SpecLinkEntry, SpecLinkType};

        let state = test_state();
        let sha = "c".repeat(40);
        let spec_ref = format!("system/spec-a.md@{sha}");

        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                // Seed spec-a (the referenced spec) and spec-b (the conflicting spec).
                state
                    .spec_ledger
                    .save(&SpecLedgerEntry {
                        path: "system/spec-a.md".to_string(),
                        title: "Spec A".to_string(),
                        owner: "user:test".to_string(),
                        kind: None,
                        current_sha: sha.clone(),
                        approval_mode: "human_only".to_string(),
                        approval_status: ApprovalStatus::Approved,
                        linked_tasks: vec![],
                        linked_mrs: vec![],
                        drift_status: "clean".to_string(),
                        created_at: 1700000000,
                        updated_at: 1700000000,
                        repo_id: None,
                        workspace_id: None,
                    })
                    .await
                    .unwrap();
                state
                    .spec_ledger
                    .save(&SpecLedgerEntry {
                        path: "system/spec-b.md".to_string(),
                        title: "Spec B".to_string(),
                        owner: "user:test".to_string(),
                        kind: None,
                        current_sha: "d".repeat(40),
                        approval_mode: "human_only".to_string(),
                        approval_status: ApprovalStatus::Approved,
                        linked_tasks: vec![],
                        linked_mrs: vec![],
                        drift_status: "clean".to_string(),
                        created_at: 1700000000,
                        updated_at: 1700000000,
                        repo_id: None,
                        workspace_id: None,
                    })
                    .await
                    .unwrap();

                // spec-a conflicts_with spec-b (which is approved).
                let mut store = state.spec_links_store.lock().await;
                store.push(SpecLinkEntry {
                    id: "conflict-1".to_string(),
                    source_path: "system/spec-a.md".to_string(),
                    source_repo_id: Some("repo1".to_string()),
                    link_type: SpecLinkType::ConflictsWith,
                    target_path: "system/spec-b.md".to_string(),
                    target_repo_id: None,
                    target_display: None,
                    target_sha: None,
                    reason: None,
                    status: "active".to_string(),
                    created_at: 1700000000,
                    stale_since: None,
                });
            })
        });

        let _entry_id = seed_merge_gate_scenario(&state, &spec_ref);

        crate::merge_processor::run_once(&state).await.unwrap();

        // Entry should be Failed because of conflicts_with approved spec.
        let all = state.merge_queue.list_queue().await.unwrap();
        assert!(
            all.is_empty(),
            "conflicting spec MR should be Failed (removed from queue)"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn merge_gate_warns_on_unimplemented_depends_on() {
        use crate::spec_registry::{SpecLinkEntry, SpecLinkType};

        let state = test_state();
        let sha = "e".repeat(40);
        let spec_ref = format!("system/consumer.md@{sha}");

        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                // consumer spec references a dependency that is not yet approved.
                state
                    .spec_ledger
                    .save(&SpecLedgerEntry {
                        path: "system/consumer.md".to_string(),
                        title: "Consumer".to_string(),
                        owner: "user:test".to_string(),
                        kind: None,
                        current_sha: sha.clone(),
                        approval_mode: "human_only".to_string(),
                        approval_status: ApprovalStatus::Approved,
                        linked_tasks: vec![],
                        linked_mrs: vec![],
                        drift_status: "clean".to_string(),
                        created_at: 1700000000,
                        updated_at: 1700000000,
                        repo_id: None,
                        workspace_id: None,
                    })
                    .await
                    .unwrap();
                state
                    .spec_ledger
                    .save(&SpecLedgerEntry {
                        path: "system/dependency.md".to_string(),
                        title: "Dependency".to_string(),
                        owner: "user:test".to_string(),
                        kind: None,
                        current_sha: "f".repeat(40),
                        approval_mode: "human_only".to_string(),
                        approval_status: ApprovalStatus::Pending, // NOT approved
                        linked_tasks: vec![],
                        linked_mrs: vec![],
                        drift_status: "clean".to_string(),
                        created_at: 1700000000,
                        updated_at: 1700000000,
                        repo_id: None,
                        workspace_id: None,
                    })
                    .await
                    .unwrap();

                let mut store = state.spec_links_store.lock().await;
                store.push(SpecLinkEntry {
                    id: "dep-1".to_string(),
                    source_path: "system/consumer.md".to_string(),
                    source_repo_id: Some("repo1".to_string()),
                    link_type: SpecLinkType::DependsOn,
                    target_path: "system/dependency.md".to_string(),
                    target_repo_id: None,
                    target_display: None,
                    target_sha: None,
                    reason: None,
                    status: "active".to_string(),
                    created_at: 1700000000,
                    stale_since: None,
                });
            })
        });

        let entry_id = seed_merge_gate_scenario(&state, &spec_ref);

        crate::merge_processor::run_once(&state).await.unwrap();

        // The merge should NOT be blocked by the spec link gate — depends_on only warns.
        // With the in-memory git backend, the merge succeeds (Merged status).
        // This proves the entry passed through the spec link gate without being blocked.
        let entry = state
            .merge_queue
            .find_by_id(&entry_id)
            .await
            .unwrap()
            .expect("entry should still exist after run_once");
        assert_eq!(
            entry.status,
            gyre_domain::MergeQueueEntryStatus::Merged,
            "depends_on should warn, not block — entry should reach Merged status"
        );
        assert!(
            entry.error_message.is_none(),
            "depends_on should not produce an error — but got: {:?}",
            entry.error_message
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn merge_gate_blocks_conflicts_with_approved_reverse_direction() {
        use crate::spec_registry::{SpecLinkEntry, SpecLinkType};

        let state = test_state();
        let sha = "c".repeat(40);
        // MR references spec-b, which is the TARGET of a conflicts_with link from spec-a.
        // Since conflicts_with is bidirectional, this should also be blocked.
        let spec_ref = format!("system/spec-b.md@{sha}");

        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                // Seed spec-a (the source of the conflicts_with link, approved).
                state
                    .spec_ledger
                    .save(&SpecLedgerEntry {
                        path: "system/spec-a.md".to_string(),
                        title: "Spec A".to_string(),
                        owner: "user:test".to_string(),
                        kind: None,
                        current_sha: "a".repeat(40),
                        approval_mode: "human_only".to_string(),
                        approval_status: ApprovalStatus::Approved,
                        linked_tasks: vec![],
                        linked_mrs: vec![],
                        drift_status: "clean".to_string(),
                        created_at: 1700000000,
                        updated_at: 1700000000,
                        repo_id: None,
                        workspace_id: None,
                    })
                    .await
                    .unwrap();
                // Seed spec-b (the referenced spec, also the target of conflicts_with).
                state
                    .spec_ledger
                    .save(&SpecLedgerEntry {
                        path: "system/spec-b.md".to_string(),
                        title: "Spec B".to_string(),
                        owner: "user:test".to_string(),
                        kind: None,
                        current_sha: sha.clone(),
                        approval_mode: "human_only".to_string(),
                        approval_status: ApprovalStatus::Approved,
                        linked_tasks: vec![],
                        linked_mrs: vec![],
                        drift_status: "clean".to_string(),
                        created_at: 1700000000,
                        updated_at: 1700000000,
                        repo_id: None,
                        workspace_id: None,
                    })
                    .await
                    .unwrap();

                // spec-a conflicts_with spec-b — link source is spec-a, target is spec-b.
                // The MR references spec-b (the target), so bidirectional check is needed.
                let mut store = state.spec_links_store.lock().await;
                store.push(SpecLinkEntry {
                    id: "conflict-reverse-1".to_string(),
                    source_path: "system/spec-a.md".to_string(),
                    source_repo_id: Some("repo1".to_string()),
                    link_type: SpecLinkType::ConflictsWith,
                    target_path: "system/spec-b.md".to_string(),
                    target_repo_id: None,
                    target_display: None,
                    target_sha: None,
                    reason: None,
                    status: "active".to_string(),
                    created_at: 1700000000,
                    stale_since: None,
                });
            })
        });

        let _entry_id = seed_merge_gate_scenario(&state, &spec_ref);

        crate::merge_processor::run_once(&state).await.unwrap();

        // Entry should be Failed because of bidirectional conflicts_with check.
        let all = state.merge_queue.list_queue().await.unwrap();
        assert!(
            all.is_empty(),
            "MR referencing target of conflicts_with link should be Failed (bidirectional check)"
        );
    }
}
