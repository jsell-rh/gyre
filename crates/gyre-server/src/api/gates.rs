//! Quality gate CRUD, gate-result endpoints, and spec approval ledger.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use gyre_common::Id;
use gyre_domain::{GatePhase, GateResult, GateStatus, GateType, QualityGate, SpecApproval};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::instrument;

use crate::AppState;

use super::error::ApiError;
use super::{new_id, now_secs};

// ---------------------------------------------------------------------------
// Request / Response types
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct CreateGateRequest {
    pub name: String,
    pub gate_type: GateType,
    /// Shell command (required for TestCommand / LintCommand).
    pub command: Option<String>,
    /// Minimum approvals (required for RequiredApprovals).
    pub required_approvals: Option<u32>,
    /// Persona path (used by AgentReview / AgentValidation).
    pub persona: Option<String>,
    /// Domain-specific check identifier for AgentValidation gates
    /// (e.g. "license-scan"). Delivered to the validation agent as
    /// `GYRE_VALIDATION_TYPE`.
    pub validation_type: Option<String>,
    /// When false, gate is advisory only — failures do not block merging. Defaults to true.
    pub required: Option<bool>,
    /// When this gate runs: "pre_merge" (blocking, before merge) or
    /// "post_merge" (validation against the new default-branch HEAD after
    /// merge). Defaults to "pre_merge".
    pub gate_phase: Option<GatePhase>,
    /// Command timeout in seconds. Defaults to the system default (300s).
    pub timeout_secs: Option<u64>,
}

#[derive(Serialize)]
pub struct GateResponse {
    pub id: String,
    pub repo_id: String,
    pub name: String,
    pub gate_type: String,
    pub command: Option<String>,
    pub required_approvals: Option<u32>,
    pub persona: Option<String>,
    /// Domain-specific check identifier for AgentValidation gates.
    pub validation_type: Option<String>,
    /// Whether this gate is blocking (true) or advisory-only (false).
    pub required: bool,
    /// "pre_merge" or "post_merge".
    pub gate_phase: String,
    /// Command timeout in seconds (None = system default).
    pub timeout_secs: Option<u64>,
    /// Position of this gate in the repo's ordered gate chain (0-based).
    pub position: u32,
    pub created_at: u64,
}

impl From<QualityGate> for GateResponse {
    fn from(g: QualityGate) -> Self {
        Self {
            id: g.id.to_string(),
            repo_id: g.repo_id.to_string(),
            name: g.name,
            gate_type: gate_type_str(&g.gate_type),
            command: g.command,
            required_approvals: g.required_approvals,
            persona: g.persona,
            validation_type: g.validation_type,
            required: g.required,
            gate_phase: g.gate_phase.as_str().to_string(),
            timeout_secs: g.timeout_secs,
            position: g.position,
            created_at: g.created_at,
        }
    }
}

fn gate_type_str(t: &GateType) -> String {
    match t {
        GateType::TestCommand => "test_command",
        GateType::LintCommand => "lint_command",
        GateType::RequiredApprovals => "required_approvals",
        GateType::AgentReview => "agent_review",
        GateType::AgentValidation => "agent_validation",
        GateType::TraceCapture => "trace_capture",
    }
    .to_string()
}

#[derive(Serialize)]
pub struct GateResultResponse {
    pub id: String,
    pub gate_id: String,
    pub mr_id: String,
    pub status: String,
    pub output: Option<String>,
    pub started_at: Option<u64>,
    pub finished_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gate_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gate_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
}

impl From<GateResult> for GateResultResponse {
    fn from(r: GateResult) -> Self {
        Self {
            id: r.id.to_string(),
            gate_id: r.gate_id.to_string(),
            mr_id: r.mr_id.to_string(),
            status: gate_status_str(&r.status),
            output: r.output,
            started_at: r.started_at,
            finished_at: r.finished_at,
            gate_name: None,
            gate_type: None,
            required: None,
            command: None,
        }
    }
}

fn gate_status_str(s: &GateStatus) -> String {
    match s {
        GateStatus::Pending => "pending",
        GateStatus::Running => "running",
        GateStatus::Passed => "passed",
        GateStatus::Failed => "failed",
    }
    .to_string()
}

// ---------------------------------------------------------------------------
// Spec Approval types
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct SpecApprovalResponse {
    pub id: String,
    pub spec_path: String,
    pub spec_sha: String,
    pub approver_id: String,
    pub signature: Option<String>,
    pub approved_at: Option<u64>,
    pub revoked_at: Option<u64>,
    pub revoked_by: Option<String>,
    pub revocation_reason: Option<String>,
    /// When a human reviewer rejected this approval (None = not rejected).
    /// Distinguishes rejection from revocation for ledger consumers
    /// (e.g. the Explorer evaluative lens approval icons).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rejected_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rejected_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rejected_by: Option<String>,
    /// Derived from which timestamp column is non-null (never stored).
    pub status: String,
    /// True when this entry actively approves the spec SHA.
    pub active: bool,
}

impl From<SpecApproval> for SpecApprovalResponse {
    fn from(a: SpecApproval) -> Self {
        let active = a.is_active();
        let status = a.status().to_string();
        Self {
            id: a.id.to_string(),
            spec_path: a.spec_path,
            spec_sha: a.spec_sha,
            approver_id: a.approver_id,
            signature: a.signature,
            approved_at: a.approved_at,
            revoked_at: a.revoked_at,
            revoked_by: a.revoked_by,
            revocation_reason: a.revocation_reason,
            rejected_at: a.rejected_at,
            rejected_reason: a.rejected_reason,
            rejected_by: a.rejected_by,
            status,
            active,
        }
    }
}

// ---------------------------------------------------------------------------
// Handlers — Quality Gates
// ---------------------------------------------------------------------------

/// POST /api/v1/repos/:id/gates — create a quality gate for a repo.
#[instrument(skip(state, req), fields(repo_id = %repo_id))]
pub async fn create_gate(
    State(state): State<Arc<AppState>>,
    Path(repo_id): Path<String>,
    Json(req): Json<CreateGateRequest>,
) -> Result<(StatusCode, Json<GateResponse>), ApiError> {
    // Validate the repo exists.
    state
        .repos
        .find_by_id(&Id::new(&repo_id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("repo {repo_id} not found")))?;

    // Duplicate guard: gate names are unique per repo. Without this, every
    // re-run of `gyre bootstrap` re-POSTs the same default gate set and
    // duplicates each row — every subsequent MR then runs each gate once
    // per bootstrap run. The listed chain also decides the new gate's
    // position (append to the end of the ordered chain).
    let existing = state.quality_gates.list_by_repo_id(&repo_id).await?;
    if existing.iter().any(|g| g.name == req.name) {
        return Err(ApiError::Conflict(format!(
            "gate '{}' already exists for repo {repo_id}",
            req.name
        )));
    }

    // Validate required fields per gate type.
    match req.gate_type {
        GateType::TestCommand | GateType::LintCommand => {
            if req.command.is_none() {
                return Err(ApiError::InvalidInput(
                    "command is required for test_command / lint_command gates".to_string(),
                ));
            }
        }
        GateType::RequiredApprovals => {
            if req.required_approvals.is_none() {
                return Err(ApiError::InvalidInput(
                    "required_approvals is required for required_approvals gate".to_string(),
                ));
            }
        }
        GateType::AgentReview | GateType::AgentValidation => {
            // persona is optional — defaults to "personas/default.md" at execution time.
        }
        GateType::TraceCapture => {
            // config is optional — defaults are applied at execution time.
        }
    }

    // Next free position in the repo's chain: gates appended via the
    // single-gate API go to the end of the chain (agent-gates.md §Gate
    // Chains — ordered policies). The chain-config endpoint is the
    // authority for full reordering.
    let next_position = existing
        .iter()
        .map(|g| g.position)
        .max()
        .map_or(0, |m| m + 1);

    let gate = QualityGate {
        id: new_id(),
        repo_id: Id::new(repo_id),
        name: req.name,
        gate_type: req.gate_type,
        command: req.command,
        required_approvals: req.required_approvals,
        persona: req.persona,
        validation_type: req.validation_type,
        required: req.required.unwrap_or(true),
        gate_phase: req.gate_phase.unwrap_or_default(),
        timeout_secs: req.timeout_secs,
        position: next_position,
        created_at: now_secs(),
    };

    state.quality_gates.save(&gate).await?;

    Ok((StatusCode::CREATED, Json(GateResponse::from(gate))))
}

/// GET /api/v1/repos/:id/gates — list quality gates for a repo.
pub async fn list_gates(
    State(state): State<Arc<AppState>>,
    Path(repo_id): Path<String>,
) -> Result<Json<Vec<GateResponse>>, ApiError> {
    let mut result: Vec<GateResponse> = state
        .quality_gates
        .list_by_repo_id(&repo_id)
        .await?
        .into_iter()
        .map(GateResponse::from)
        .collect();
    // Chain order: position first, creation order as tiebreak (rows written
    // before the position column existed all sit at 0). Mirrors the
    // adapters' ORDER BY so the API is ordered regardless of backend.
    result.sort_by_key(|g| (g.position, g.created_at));
    Ok(Json(result))
}

/// DELETE /api/v1/repos/:id/gates/:gate_id — remove a gate.
pub async fn delete_gate(
    State(state): State<Arc<AppState>>,
    Path((repo_id, gate_id)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    match state.quality_gates.find_by_id(&gate_id).await? {
        None => Err(ApiError::NotFound(format!("gate {gate_id} not found"))),
        Some(g) if g.repo_id.as_str() != repo_id => {
            Err(ApiError::NotFound(format!("gate {gate_id} not found")))
        }
        _ => {
            state.quality_gates.delete(&gate_id).await?;
            Ok(StatusCode::NO_CONTENT)
        }
    }
}

// ---------------------------------------------------------------------------
// Gate chain configuration (agent-gates.md §Gate Chains)
// ---------------------------------------------------------------------------

/// One `[[gates]]` entry of a repo's gate chain configuration.
///
/// Mirrors the spec's TOML surface exactly (`name`, `type`, `command`,
/// `persona`, `min_approvals`, `required`) plus the optional
/// `validation_type` / `timeout_secs` knobs the single-gate API supports.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GateChainEntry {
    pub name: String,
    /// Gate type. Accepts the spec's PascalCase names (`TestCommand`,
    /// `AgentReview`, ...) and the internal snake_case wire forms
    /// (`test_command`, `agent_review`, ...).
    #[serde(rename = "type")]
    pub gate_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub persona: Option<String>,
    /// Minimum approvals (RequiredApprovals gates). Named `min_approvals`
    /// per the spec's TOML.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_approvals: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub validation_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_secs: Option<u64>,
}

/// The full `[[gates]]` chain document (TOML).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GateChainToml {
    pub gates: Vec<GateChainEntry>,
}

/// Parse a gate type name from chain configuration. Accepts the spec's
/// PascalCase names and the internal snake_case wire forms.
fn parse_gate_type_name(s: &str) -> Option<GateType> {
    match s {
        "TestCommand" | "test_command" => Some(GateType::TestCommand),
        "LintCommand" | "lint_command" => Some(GateType::LintCommand),
        "RequiredApprovals" | "required_approvals" => Some(GateType::RequiredApprovals),
        "AgentReview" | "agent_review" => Some(GateType::AgentReview),
        "AgentValidation" | "agent_validation" => Some(GateType::AgentValidation),
        "TraceCapture" | "trace_capture" => Some(GateType::TraceCapture),
        _ => None,
    }
}

/// Serialize a gate type back to the spec's PascalCase TOML form.
fn gate_type_toml_name(t: &GateType) -> &'static str {
    match t {
        GateType::TestCommand => "TestCommand",
        GateType::LintCommand => "LintCommand",
        GateType::RequiredApprovals => "RequiredApprovals",
        GateType::AgentReview => "AgentReview",
        GateType::AgentValidation => "AgentValidation",
        GateType::TraceCapture => "TraceCapture",
    }
}

impl From<&QualityGate> for GateChainEntry {
    fn from(g: &QualityGate) -> Self {
        Self {
            name: g.name.clone(),
            gate_type: gate_type_toml_name(&g.gate_type).to_string(),
            command: g.command.clone(),
            persona: g.persona.clone(),
            min_approvals: g.required_approvals,
            validation_type: g.validation_type.clone(),
            required: Some(g.required),
            timeout_secs: g.timeout_secs,
        }
    }
}

/// Validate a parsed chain BEFORE any mutation so an invalid chain never
/// partially applies. Returns per-entry parsed gate types.
fn validate_chain(chain: &GateChainToml) -> Result<Vec<GateType>, ApiError> {
    let mut seen = std::collections::HashSet::new();
    let mut types = Vec::with_capacity(chain.gates.len());
    for (i, entry) in chain.gates.iter().enumerate() {
        if entry.name.trim().is_empty() {
            return Err(ApiError::InvalidInput(format!(
                "gate chain entry {i}: name must not be empty"
            )));
        }
        if !seen.insert(entry.name.clone()) {
            return Err(ApiError::InvalidInput(format!(
                "gate chain entry {i}: duplicate gate name '{}'",
                entry.name
            )));
        }
        let gate_type = parse_gate_type_name(&entry.gate_type).ok_or_else(|| {
            ApiError::InvalidInput(format!(
                "gate chain entry {i} ('{}'): unknown gate type '{}'",
                entry.name, entry.gate_type
            ))
        })?;
        match gate_type {
            GateType::TestCommand | GateType::LintCommand => {
                if entry
                    .command
                    .as_deref()
                    .map(str::trim)
                    .unwrap_or("")
                    .is_empty()
                {
                    return Err(ApiError::InvalidInput(format!(
                        "gate chain entry {i} ('{}'): command is required for {} gates",
                        entry.name, entry.gate_type
                    )));
                }
            }
            GateType::RequiredApprovals => {
                if entry.min_approvals.is_none() {
                    return Err(ApiError::InvalidInput(format!(
                        "gate chain entry {i} ('{}'): min_approvals is required for RequiredApprovals gates",
                        entry.name
                    )));
                }
            }
            GateType::AgentReview | GateType::AgentValidation | GateType::TraceCapture => {}
        }
        types.push(gate_type);
    }
    Ok(types)
}

/// PUT /api/v1/repos/:id/gate-chain — replace the repo's gate chain.
///
/// Body: the chain as TOML text (the spec's `[[gates]]` format),
/// content-type `application/toml` or `text/plain`.
///
/// Semantics: the chain REPLACES the repo's configured PRE-MERGE gates —
/// entries are applied in order (position = array index), gates whose
/// names disappear from the chain are deleted, and gates whose names
/// persist keep their ids (so existing GateResults stay linked to their
/// gate). Validation happens entirely before the first mutation: an
/// invalid chain is a 400 and leaves the current chain untouched.
///
/// Post-merge gates (`gate_phase = post_merge`, configured via
/// `PUT /repos/:id/post-merge-gates`) are NOT part of the chain — the
/// chain TOML cannot express a gate phase — so they are neither deleted
/// nor re-phased by this endpoint. A chain entry whose name collides with
/// an existing post-merge gate is rejected (gate names are unique per
/// repo, enforced by `create_gate` for POST /gates as well).
pub async fn set_gate_chain(
    State(state): State<Arc<AppState>>,
    Path(repo_id): Path<String>,
    headers: axum::http::HeaderMap,
    body: axum::body::Bytes,
) -> Result<(StatusCode, Json<Vec<GateResponse>>), ApiError> {
    // Validate the repo exists.
    state
        .repos
        .find_by_id(&Id::new(&repo_id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("repo {repo_id} not found")))?;

    // Content-type gate: the chain is TOML. Accept the explicit type, plain
    // text, or no header; anything else (notably JSON — a different format
    // with different field semantics) is a 415, not a parse gamble.
    let content_type = headers
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.split(';').next().unwrap_or(v).trim().to_ascii_lowercase())
        .unwrap_or_default();
    if !matches!(
        content_type.as_str(),
        "application/toml" | "text/plain" | ""
    ) {
        return Err(ApiError::UnsupportedMediaType(format!(
            "gate chain configuration must be TOML (application/toml or text/plain), got '{content_type}'"
        )));
    }

    let text = std::str::from_utf8(&body)
        .map_err(|e| ApiError::InvalidInput(format!("gate chain body is not valid UTF-8: {e}")))?;
    let chain: GateChainToml = toml::from_str(text)
        .map_err(|e| ApiError::InvalidInput(format!("gate chain is not valid TOML: {e}")))?;

    // Full validation before any mutation (atomicity).
    let types = validate_chain(&chain)?;

    // The chain manages only PRE-MERGE gates. Post-merge recovery gates
    // (set_post_merge_gates) live in the same per-repo table but are
    // configured through their own endpoint — the chain TOML cannot
    // express a gate phase, so a chain PUT must neither delete them nor
    // re-phase them. A name collision would violate per-repo name
    // uniqueness (see create_gate), so it is rejected up front.
    let all_gates = state.quality_gates.list_by_repo_id(&repo_id).await?;
    let post_merge_names: std::collections::HashSet<&str> = all_gates
        .iter()
        .filter(|g| g.gate_phase == GatePhase::PostMerge)
        .map(|g| g.name.as_str())
        .collect();
    if let Some(entry) = chain
        .gates
        .iter()
        .find(|e| post_merge_names.contains(e.name.as_str()))
    {
        return Err(ApiError::InvalidInput(format!(
            "gate '{}' is configured as a post-merge gate for repo {repo_id}; \
             chain entries cannot reuse post-merge gate names",
            entry.name
        )));
    }
    let mut by_name: std::collections::HashMap<String, QualityGate> = all_gates
        .into_iter()
        .filter(|g| g.gate_phase == GatePhase::PreMerge)
        .map(|g| (g.name.clone(), g))
        .collect();

    let mut saved = Vec::with_capacity(chain.gates.len());
    for (i, entry) in chain.gates.iter().enumerate() {
        let gate_type = types[i].clone();
        let mut gate = by_name.remove(&entry.name).unwrap_or_else(|| QualityGate {
            id: new_id(),
            repo_id: Id::new(repo_id.clone()),
            name: entry.name.clone(),
            gate_type: gate_type.clone(),
            command: None,
            required_approvals: None,
            persona: None,
            validation_type: None,
            required: true,
            gate_phase: Default::default(),
            timeout_secs: None,
            position: 0,
            created_at: now_secs(),
        });
        // Update config in place (persisting gates keep their id and
        // created_at; only the chain-controlled fields change).
        gate.gate_type = gate_type;
        gate.command = entry.command.clone();
        gate.required_approvals = entry.min_approvals;
        gate.persona = entry.persona.clone();
        gate.validation_type = entry.validation_type.clone();
        gate.required = entry.required.unwrap_or(true);
        gate.timeout_secs = entry.timeout_secs;
        gate.position = i as u32;
        state.quality_gates.save(&gate).await?;
        saved.push(gate);
    }

    // Pre-merge gates absent from the new chain are removed from it.
    // (Post-merge gates were never in by_name — they survive the PUT.)
    for (_, removed) in by_name {
        state.quality_gates.delete(removed.id.as_str()).await?;
    }

    Ok((
        StatusCode::OK,
        Json(saved.into_iter().map(GateResponse::from).collect()),
    ))
}

/// GET /api/v1/repos/:id/gate-chain — the repo's current gate chain as
/// TOML (the same `[[gates]]` format `PUT` accepts, entries in chain
/// order). Only pre-merge gates are part of the chain; post-merge gates
/// are listed by `GET /repos/:id/post-merge-gates` instead (the chain
/// format cannot express a gate phase, so including them here would make
/// a GET→PUT roundtrip silently re-phase or drop them).
pub async fn get_gate_chain(
    State(state): State<Arc<AppState>>,
    Path(repo_id): Path<String>,
) -> Result<axum::response::Response, ApiError> {
    // Validate the repo exists.
    state
        .repos
        .find_by_id(&Id::new(&repo_id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("repo {repo_id} not found")))?;

    let gates = state
        .quality_gates
        .list_by_repo_id_and_phase(&repo_id, GatePhase::PreMerge)
        .await?;
    let chain = GateChainToml {
        gates: gates.iter().map(GateChainEntry::from).collect(),
    };
    let text = toml::to_string_pretty(&chain)
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("serialize gate chain: {e}")))?;

    Ok(axum::response::Response::builder()
        .header(axum::http::header::CONTENT_TYPE, "application/toml")
        .body(axum::body::Body::from(text))
        .unwrap())
}

/// GET /api/v1/merge-requests/:id/gates — list gate results for an MR.
///
/// Enriches each result with gate definition fields (name, type, required,
/// command) so the UI can display meaningful information without a separate
/// gate definitions fetch.
pub async fn list_mr_gate_results(
    State(state): State<Arc<AppState>>,
    Path(mr_id): Path<String>,
) -> Result<Json<Vec<GateResultResponse>>, ApiError> {
    let results = state.gate_results.list_by_mr_id(&mr_id).await?;

    // Collect unique gate_ids to look up definitions
    let gate_ids: Vec<String> = results
        .iter()
        .map(|r| r.gate_id.to_string())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();

    // Build a map of gate_id → definition (best-effort)
    let mut gate_defs = std::collections::HashMap::new();
    for gid in &gate_ids {
        if let Ok(Some(def)) = state.quality_gates.find_by_id(gid).await {
            gate_defs.insert(gid.clone(), def);
        }
    }

    let mut out: Vec<GateResultResponse> = results
        .into_iter()
        .map(|r| {
            let mut resp = GateResultResponse::from(r);
            if let Some(def) = gate_defs.get(&resp.gate_id) {
                resp.gate_name = Some(def.name.clone());
                resp.gate_type = Some(gate_type_str(&def.gate_type));
                resp.required = Some(def.required);
                resp.command = def.command.clone();
            }
            resp
        })
        .collect();
    out.sort_by_key(|r| r.started_at.unwrap_or(0));
    Ok(Json(out))
}

// ---------------------------------------------------------------------------
// Handlers — Spec Approval Ledger
// ---------------------------------------------------------------------------

/// GET /api/v1/specs/approvals — list all spec approvals (optionally filter by ?path=).
pub async fn list_spec_approvals(
    State(state): State<Arc<AppState>>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Result<Json<Vec<SpecApprovalResponse>>, ApiError> {
    let path_filter = params.get("path").map(|s| s.as_str());
    let all = if let Some(path) = path_filter {
        state.spec_approvals.list_by_path(path).await?
    } else {
        state.spec_approvals.list_all().await?
    };
    let mut result: Vec<SpecApprovalResponse> =
        all.into_iter().map(SpecApprovalResponse::from).collect();
    result.sort_by_key(|a| a.approved_at.unwrap_or(0));
    Ok(Json(result))
}

/// Check that a spec_ref ("path@sha") has an active approval in the ledger.
/// Returns Ok(()) if approved, Err(msg) if not.
pub async fn verify_spec_ref(state: &AppState, spec_ref: &str) -> Result<(), String> {
    // Parse "path@sha" format.
    let (path, sha) = spec_ref
        .rsplit_once('@')
        .ok_or_else(|| format!("invalid spec_ref format '{spec_ref}': expected 'path@sha'"))?;

    // Validate SHA is 40-char hex.
    if sha.len() != 40 || !sha.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!(
            "spec_ref SHA '{sha}' is not a valid 40-char hex SHA"
        ));
    }

    // Approval status per SHA (agent-gates.md §Spec Approval Ledger): query
    // the exact (spec_path, spec_sha) version — the forge's step-8 check —
    // rather than loading every approval for the path. An entry is active
    // only when it is Approved (approved_at set) and not since revoked or
    // rejected; Pending/Revoked/Rejected rows never authorize a merge.
    let for_sha = state
        .spec_approvals
        .find_by_spec_sha(path, sha)
        .await
        .unwrap_or_default();
    let has_active_approval = for_sha.iter().any(|a| a.is_active());

    if has_active_approval {
        Ok(())
    } else {
        Err(format!(
            "spec '{path}' at SHA '{sha}' has no active approval in the ledger"
        ))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::verify_spec_ref;
    use crate::mem::test_state;
    use axum::{body::Body, Router};
    use gyre_domain::{Repository, SpecApproval};
    use http::{Request, StatusCode};
    use tower::ServiceExt;

    use super::GateChainToml;

    fn app() -> Router {
        crate::api::api_router().with_state(test_state())
    }

    async fn body_json(resp: axum::response::Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    async fn create_repo(state: std::sync::Arc<crate::AppState>) -> String {
        let repo = Repository::new(
            gyre_common::Id::new("repo-1"),
            gyre_common::Id::new("proj-1"),
            "test-repo",
            "/tmp/test-repo",
            0,
        );
        state.repos.create(&repo).await.unwrap();
        "repo-1".to_string()
    }

    #[tokio::test]
    async fn create_and_list_gate() {
        let state = test_state();
        create_repo(state.clone()).await;
        let app = crate::api::api_router().with_state(state);

        let body = serde_json::json!({
            "name": "unit-tests",
            "gate_type": "test_command",
            "command": "cargo test"
        });
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/repos/repo-1/gates")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let json = body_json(resp).await;
        assert_eq!(json["name"], "unit-tests");
        assert_eq!(json["gate_type"], "test_command");

        // list
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/repos/repo-1/gates")
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
    async fn create_gate_rejects_duplicate_name_per_repo() {
        // Re-running `gyre bootstrap` re-POSTs the same default gate set;
        // a second gate with the same name for the same repo must be a
        // 409, not a silent duplicate row that runs once per bootstrap.
        let state = test_state();
        create_repo(state.clone()).await;
        let app = crate::api::api_router().with_state(state);

        let body_json_val = serde_json::json!({
            "name": "cargo-test",
            "gate_type": "test_command",
            "command": "cargo test"
        });
        let post = |app: &Router| {
            let body = Body::from(serde_json::to_vec(&body_json_val).unwrap());
            app.clone().oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/repos/repo-1/gates")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(body)
                    .unwrap(),
            )
        };
        let resp = post(&app).await.unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let resp = post(&app).await.unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::CONFLICT,
            "duplicate gate name for the same repo must be a 409 conflict"
        );

        // The list still holds exactly one row — no duplicate was written.
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/repos/repo-1/gates")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let json = body_json(resp).await;
        assert_eq!(
            json.as_array()
                .unwrap()
                .iter()
                .filter(|g| g["name"] == "cargo-test")
                .count(),
            1
        );
    }

    #[tokio::test]
    async fn create_gate_missing_command_returns_bad_request() {
        let state = test_state();
        create_repo(state.clone()).await;
        let app = crate::api::api_router().with_state(state);

        let body = serde_json::json!({
            "name": "lint",
            "gate_type": "lint_command"
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/repos/repo-1/gates")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn create_agent_review_gate() {
        let state = test_state();
        create_repo(state.clone()).await;
        let app = crate::api::api_router().with_state(state);

        let body = serde_json::json!({
            "name": "security-review",
            "gate_type": "agent_review",
            "persona": "personas/security.md"
        });
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/repos/repo-1/gates")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let json = body_json(resp).await;
        assert_eq!(json["gate_type"], "agent_review");
        assert_eq!(json["persona"], "personas/security.md");
    }

    #[tokio::test]
    async fn create_agent_validation_gate() {
        let state = test_state();
        create_repo(state.clone()).await;
        let app = crate::api::api_router().with_state(state);

        let body = serde_json::json!({
            "name": "domain-validation",
            "gate_type": "agent_validation",
            "persona": "personas/accountability.md",
            "validation_type": "license-scan"
        });
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/repos/repo-1/gates")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let json = body_json(resp).await;
        assert_eq!(json["gate_type"], "agent_validation");
        assert_eq!(json["validation_type"], "license-scan");

        // Round-trip through list: the stored gate must carry the config.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/repos/repo-1/gates")
                    .header("authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        let stored = json
            .as_array()
            .unwrap()
            .iter()
            .find(|g| g["gate_type"] == "agent_validation")
            .expect("agent_validation gate in list");
        assert_eq!(stored["validation_type"], "license-scan");
        assert_eq!(stored["persona"], "personas/accountability.md");
    }

    // ── Gate chain configuration (agent-gates.md §Gate Chains) ──────────

    /// The spec's exact §Gate Chains example: an ordered chain mixing all
    /// gate types.
    const SPEC_CHAIN_TOML: &str = r#"
[[gates]]
name = "tests"
type = "TestCommand"
command = "cargo test"
required = true

[[gates]]
name = "architecture-lint"
type = "LintCommand"
command = "scripts/check-arch.sh"
required = true

[[gates]]
name = "security-review"
type = "AgentReview"
persona = "personas/security.md"
required = true

[[gates]]
name = "spec-alignment-review"
type = "AgentReview"
persona = "personas/accountability.md"
required = true

[[gates]]
name = "peer-approval"
type = "RequiredApprovals"
min_approvals = 1
required = true
"#;

    async fn put_chain(
        app: &Router,
        toml_text: &str,
        content_type: &str,
    ) -> axum::response::Response {
        app.clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/v1/repos/repo-1/gate-chain")
                    .header("content-type", content_type)
                    .header("authorization", "Bearer test-token")
                    .body(Body::from(toml_text.as_bytes().to_vec()))
                    .unwrap(),
            )
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn gate_chain_put_parses_spec_toml_and_orders_chain() {
        let state = test_state();
        create_repo(state.clone()).await;
        let app = crate::api::api_router().with_state(state.clone());

        let resp = put_chain(&app, SPEC_CHAIN_TOML, "application/toml").await;
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        let gates = json.as_array().unwrap();
        assert_eq!(gates.len(), 5, "all five chain entries applied");

        // The chain is ordered: position = array index, mixed types kept.
        let names: Vec<&str> = gates.iter().map(|g| g["name"].as_str().unwrap()).collect();
        assert_eq!(
            names,
            vec![
                "tests",
                "architecture-lint",
                "security-review",
                "spec-alignment-review",
                "peer-approval"
            ]
        );
        for (i, g) in gates.iter().enumerate() {
            assert_eq!(g["position"], i as u64, "chain order = array index");
        }
        // Field mapping: spec TOML names → internal gate fields.
        assert_eq!(gates[0]["gate_type"], "test_command");
        assert_eq!(gates[0]["command"], "cargo test");
        assert_eq!(gates[2]["gate_type"], "agent_review");
        assert_eq!(gates[2]["persona"], "personas/security.md");
        assert_eq!(gates[4]["gate_type"], "required_approvals");
        assert_eq!(gates[4]["required_approvals"], 1);

        // The plain gates listing is chain-ordered too.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/repos/repo-1/gates")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let json = body_json(resp).await;
        let listed: Vec<&str> = json
            .as_array()
            .unwrap()
            .iter()
            .map(|g| g["name"].as_str().unwrap())
            .collect();
        assert_eq!(
            listed,
            vec![
                "tests",
                "architecture-lint",
                "security-review",
                "spec-alignment-review",
                "peer-approval"
            ]
        );

        // GET returns the chain as TOML in the same format PUT accepts.
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/repos/repo-1/gate-chain")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .unwrap_or(""),
            "application/toml"
        );
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let toml_text = String::from_utf8(bytes.to_vec()).unwrap();
        // Round-trip: the GET'd TOML parses back into the same chain, in order.
        let round: GateChainToml = toml::from_str(&toml_text).unwrap();
        let rt_names: Vec<&str> = round.gates.iter().map(|g| g.name.as_str()).collect();
        assert_eq!(
            rt_names,
            vec![
                "tests",
                "architecture-lint",
                "security-review",
                "spec-alignment-review",
                "peer-approval"
            ]
        );
        assert_eq!(round.gates[0].command.as_deref(), Some("cargo test"));
        assert_eq!(round.gates[4].min_approvals, Some(1));
        assert_eq!(
            round.gates[2].persona.as_deref(),
            Some("personas/security.md")
        );
    }

    #[tokio::test]
    async fn gate_chain_replace_reorders_keeps_ids_deletes_removed() {
        let state = test_state();
        create_repo(state.clone()).await;
        let app = crate::api::api_router().with_state(state.clone());

        let resp = put_chain(&app, SPEC_CHAIN_TOML, "text/plain").await;
        assert_eq!(resp.status(), StatusCode::OK);
        let first = body_json(resp).await;
        let id_of = |name: &str| {
            first
                .as_array()
                .unwrap()
                .iter()
                .find(|g| g["name"] == name)
                .unwrap()["id"]
                .as_str()
                .unwrap()
                .to_string()
        };
        let security_id = id_of("security-review");
        let tests_id = id_of("tests");

        // Replacement chain: security-review moved to the front, tests gone,
        // a brand-new advisory gate appended.
        let replacement = r#"
[[gates]]
name = "security-review"
type = "AgentReview"
persona = "personas/security.md"
required = true

[[gates]]
name = "peer-approval"
type = "RequiredApprovals"
min_approvals = 2
required = true

[[gates]]
name = "advisory-lint"
type = "LintCommand"
command = "cargo clippy -- -D warnings"
required = false
"#;
        let resp = put_chain(&app, replacement, "application/toml").await;
        assert_eq!(resp.status(), StatusCode::OK);
        let second = body_json(resp).await;
        let gates = second.as_array().unwrap();
        assert_eq!(gates.len(), 3);

        // Reordered: security-review now position 0.
        assert_eq!(gates[0]["name"], "security-review");
        assert_eq!(gates[0]["position"], 0);
        // Persisting gates keep their ids — prior GateResults stay linked.
        assert_eq!(gates[0]["id"].as_str().unwrap(), security_id);
        assert_eq!(gates[1]["id"].as_str().unwrap(), id_of("peer-approval"));
        // Updated config in place: min_approvals changed to 2.
        assert_eq!(gates[1]["required_approvals"], 2);
        // New gate gets a fresh id and the right position.
        assert_eq!(gates[2]["name"], "advisory-lint");
        assert_eq!(gates[2]["position"], 2);
        assert_eq!(gates[2]["required"], false);
        assert_ne!(gates[2]["id"].as_str().unwrap(), tests_id);

        // The removed gate is really gone from the repo's chain.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/repos/repo-1/gates")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let json = body_json(resp).await;
        let names: Vec<&str> = json
            .as_array()
            .unwrap()
            .iter()
            .map(|g| g["name"].as_str().unwrap())
            .collect();
        assert_eq!(
            names,
            vec!["security-review", "peer-approval", "advisory-lint"]
        );
    }

    #[tokio::test]
    async fn gate_chain_put_preserves_post_merge_gates() {
        // Post-merge recovery gates (PUT /repos/:id/post-merge-gates) share
        // the per-repo quality_gates table but are NOT chain entries — the
        // chain TOML cannot express a gate phase. A chain PUT must neither
        // delete them (they're absent from the chain) nor re-phase them,
        // and GET gate-chain must not emit them as chain entries (a
        // GET→PUT roundtrip would otherwise silently drop or re-phase
        // them).
        let state = test_state();
        create_repo(state.clone()).await;
        let app = crate::api::api_router().with_state(state.clone());

        // Seed a post-merge gate the way set_post_merge_gates stores them.
        let post_merge = gyre_domain::QualityGate {
            id: gyre_common::Id::new("pm-gate-1"),
            repo_id: gyre_common::Id::new("repo-1"),
            name: "post-merge-smoke".to_string(),
            gate_type: gyre_domain::GateType::TestCommand,
            command: Some("smoke.sh".to_string()),
            required_approvals: None,
            persona: None,
            validation_type: None,
            required: true,
            gate_phase: gyre_domain::GatePhase::PostMerge,
            timeout_secs: None,
            position: 0,
            created_at: 0,
        };
        state.quality_gates.save(&post_merge).await.unwrap();

        // Replace the pre-merge chain — "post-merge-smoke" is absent from it.
        let resp = put_chain(&app, SPEC_CHAIN_TOML, "application/toml").await;
        assert_eq!(resp.status(), StatusCode::OK);

        // The post-merge gate survived the replacement, still PostMerge.
        let pm = state
            .quality_gates
            .find_by_id("pm-gate-1")
            .await
            .unwrap()
            .expect("post-merge gate must survive a chain PUT");
        assert_eq!(pm.gate_phase, gyre_domain::GatePhase::PostMerge);
        assert_eq!(pm.name, "post-merge-smoke");
        assert_eq!(pm.command.as_deref(), Some("smoke.sh"));

        // GET gate-chain emits only the pre-merge chain.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/repos/repo-1/gate-chain")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let text = String::from_utf8(bytes.to_vec()).unwrap();
        assert!(
            !text.contains("post-merge-smoke"),
            "post-merge gates are not chain entries; chain TOML: {text}"
        );
        assert!(text.contains("name = \"tests\""), "chain TOML: {text}");

        // A chain entry reusing a post-merge gate's name is rejected: it
        // would violate per-repo name uniqueness (create_gate enforces the
        // same for POST /gates) or ambiguously rewire the recovery gate.
        let colliding = r#"
[[gates]]
name = "post-merge-smoke"
type = "TestCommand"
command = "cargo test"
"#;
        let resp = put_chain(&app, colliding, "application/toml").await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        // The rejected PUT left everything untouched.
        let pm2 = state
            .quality_gates
            .find_by_id("pm-gate-1")
            .await
            .unwrap()
            .expect("post-merge gate intact after rejected PUT");
        assert_eq!(pm2.gate_phase, gyre_domain::GatePhase::PostMerge);
        let chain_len = state
            .quality_gates
            .list_by_repo_id_and_phase("repo-1", gyre_domain::GatePhase::PreMerge)
            .await
            .unwrap()
            .len();
        assert_eq!(
            chain_len, 5,
            "pre-merge chain unchanged by the rejected PUT"
        );
    }

    #[tokio::test]
    async fn gate_chain_invalid_entries_rejected_and_chain_unchanged() {
        let state = test_state();
        create_repo(state.clone()).await;
        let app = crate::api::api_router().with_state(state.clone());

        // Start from a valid chain.
        let resp = put_chain(&app, SPEC_CHAIN_TOML, "application/toml").await;
        assert_eq!(resp.status(), StatusCode::OK);

        let before = {
            let mut v: Vec<String> = state
                .quality_gates
                .list_by_repo_id("repo-1")
                .await
                .unwrap()
                .iter()
                .map(|g| g.id.to_string())
                .collect();
            v.sort();
            v
        };

        // Duplicate names in one chain.
        let dup = r#"
[[gates]]
name = "tests"
type = "TestCommand"
command = "cargo test"

[[gates]]
name = "tests"
type = "LintCommand"
command = "cargo clippy"
"#;
        let resp = put_chain(&app, dup, "application/toml").await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        // Missing command for a TestCommand gate.
        let no_cmd = r#"
[[gates]]
name = "tests"
type = "TestCommand"
"#;
        let resp = put_chain(&app, no_cmd, "application/toml").await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        // Missing min_approvals for a RequiredApprovals gate.
        let no_min = r#"
[[gates]]
name = "peer-approval"
type = "RequiredApprovals"
"#;
        let resp = put_chain(&app, no_min, "application/toml").await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        // Unknown gate type.
        let bad_type = r#"
[[gates]]
name = "mystery"
type = "PsychicReview"
"#;
        let resp = put_chain(&app, bad_type, "application/toml").await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        // Malformed TOML.
        let resp = put_chain(&app, "[[gates\nname =", "application/toml").await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        // Wrong content type (JSON is a different format — 415, not a gamble).
        let resp = put_chain(
            &app,
            serde_json::to_string(&serde_json::json!({"gates": []}))
                .unwrap()
                .as_str(),
            "application/json",
        )
        .await;
        assert_eq!(resp.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);

        // Atomicity: none of the rejected chains changed the stored gates.
        let after = {
            let mut v: Vec<String> = state
                .quality_gates
                .list_by_repo_id("repo-1")
                .await
                .unwrap()
                .iter()
                .map(|g| g.id.to_string())
                .collect();
            v.sort();
            v
        };
        assert_eq!(
            after, before,
            "invalid chain configurations must not partially apply"
        );
    }

    #[tokio::test]
    async fn gate_chain_unknown_repo_is_404() {
        let state = test_state();
        let app = crate::api::api_router().with_state(state);
        let resp = put_chain(&app, SPEC_CHAIN_TOML, "application/toml").await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn delete_gate() {
        let state = test_state();
        create_repo(state.clone()).await;
        let app = crate::api::api_router().with_state(state);

        let body = serde_json::json!({
            "name": "tests",
            "gate_type": "test_command",
            "command": "cargo test"
        });
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/repos/repo-1/gates")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let json = body_json(resp).await;
        let gate_id = json["id"].as_str().unwrap().to_string();

        let resp = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/api/v1/repos/repo-1/gates/{gate_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn list_mr_gates_initially_empty() {
        let resp = app()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/merge-requests/mr-1/gates")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json.as_array().unwrap().len(), 0);
    }

    // -----------------------------------------------------------------------
    // Spec Approval Ledger (agent-gates.md §Spec Approval Ledger)
    // -----------------------------------------------------------------------

    /// Seed a Pending ledger entry directly through the repository port.
    async fn seed_ledger_entry(
        state: &std::sync::Arc<crate::AppState>,
        id: &str,
        spec_path: &str,
        spec_sha: &str,
    ) -> gyre_common::Id {
        let approval_id = gyre_common::Id::new(id);
        state
            .spec_approvals
            .create(&gyre_domain::SpecApproval::new(
                approval_id.clone(),
                spec_path,
                spec_sha,
                "user:test",
            ))
            .await
            .unwrap();
        approval_id
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn approvals_list_returns_full_ledger_data() {
        let state = test_state();
        let sha1 = "a".repeat(40);
        let sha2 = "b".repeat(40);
        seed_ledger_entry(&state, "apr-1", "system/design.md", &sha1).await;
        let apr2 = seed_ledger_entry(&state, "apr-2", "system/design.md", &sha2).await;

        // Multiple approvals for the same path (different SHAs) are allowed.
        let all = state
            .spec_approvals
            .list_by_path("system/design.md")
            .await
            .unwrap();
        assert_eq!(
            all.len(),
            2,
            "multiple approvals per path must be supported"
        );

        // Transition the second entry: Pending → Approved.
        state
            .spec_approvals
            .approve(&apr2, 1700000100)
            .await
            .unwrap()
            .expect("entry exists");

        // GET /api/v1/specs/approvals returns full ledger data with derived status.
        // Build the router over the SAME state we seeded — a fresh `app()`
        // would have its own empty ledger.
        let app = crate::api::api_router().with_state(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/specs/approvals")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        let arr = json.as_array().unwrap();
        assert_eq!(arr.len(), 2);

        let approved = arr
            .iter()
            .find(|a| a["spec_sha"] == sha2.as_str())
            .expect("approved entry present");
        assert_eq!(approved["status"], "approved");
        assert_eq!(approved["approved_at"], 1700000100);
        assert_eq!(approved["rejected_at"], serde_json::Value::Null);
        assert_eq!(approved["rejected_reason"], serde_json::Value::Null);
        assert_eq!(approved["rejected_by"], serde_json::Value::Null);

        let pending = arr
            .iter()
            .find(|a| a["spec_sha"] == sha1.as_str())
            .expect("pending entry present");
        assert_eq!(pending["status"], "pending");
        assert_eq!(pending["approved_at"], serde_json::Value::Null);
        assert!(!pending["active"].as_bool().unwrap());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn list_spec_approvals_includes_rejection_fields() {
        // HSI §3 Evaluative lens: the approval ledger response must let the
        // Explorer distinguish a REJECTED approval (✗) from a revoked one —
        // both are inactive, but rejection is a reviewer decision while
        // revocation returns the spec to pending.
        let state = test_state();
        let mut approved = SpecApproval::new(
            gyre_common::Id::new("ap-1"),
            "specs/system/auth.md",
            "1".repeat(40),
            "user:alice",
        );
        approved.approve(100).unwrap();
        state.spec_approvals.create(&approved).await.unwrap();

        let mut rejected = SpecApproval::new(
            gyre_common::Id::new("ap-2"),
            "specs/system/billing.md",
            "2".repeat(40),
            "user:bob",
        );
        rejected.reject("user:carol", "out of scope", 120).unwrap();
        state.spec_approvals.create(&rejected).await.unwrap();

        let app = crate::api::api_router().with_state(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/specs/approvals")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        let arr = json.as_array().unwrap();
        assert_eq!(arr.len(), 2);

        let by_path: std::collections::HashMap<&str, &serde_json::Value> = arr
            .iter()
            .map(|a| (a["spec_path"].as_str().unwrap(), a))
            .collect();

        let active = by_path.get("specs/system/auth.md").unwrap();
        assert_eq!(active["active"], serde_json::json!(true));
        // Rejected fields are omitted for active approvals (skip_serializing_if)
        assert!(active.get("rejected_at").is_none());

        let rejected = by_path.get("specs/system/billing.md").unwrap();
        assert_eq!(rejected["active"], serde_json::json!(false));
        assert_eq!(rejected["rejected_at"], serde_json::json!(120));
        assert_eq!(
            rejected["rejected_reason"],
            serde_json::json!("out of scope")
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn revoked_approval_no_longer_verifies_spec_ref() {
        let state = test_state();
        let sha = "e".repeat(40);
        let apr = seed_ledger_entry(&state, "apr-3", "system/revoked.md", &sha).await;
        state
            .spec_approvals
            .approve(&apr, 1700000100)
            .await
            .unwrap()
            .expect("entry exists");

        // Before revocation the spec_ref verifies.
        let spec_ref = format!("system/revoked.md@{sha}");
        assert!(
            verify_spec_ref(&state, &spec_ref).await.is_ok(),
            "active approval must verify"
        );

        // Approved → Revoked (post-merge withdrawal). Revocation requires a reason.
        state
            .spec_approvals
            .revoke(&apr, "user:admin", "spec withdrawn", 1700000200)
            .await
            .unwrap()
            .expect("entry exists");

        // After revocation the same spec_ref must fail verification (forge
        // blocks the merge at require_approved_spec).
        assert!(
            verify_spec_ref(&state, &spec_ref).await.is_err(),
            "revoked approval must NOT verify — merge must be blocked"
        );

        // Mutual exclusivity: approved_at cleared, revoked_at set.
        let reloaded = state
            .spec_approvals
            .find_by_id(&apr)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(reloaded.approved_at, None);
        assert_eq!(reloaded.revoked_at, Some(1700000200));
        assert_eq!(reloaded.revoked_by.as_deref(), Some("user:admin"));
        assert_eq!(
            reloaded.revocation_reason.as_deref(),
            Some("spec withdrawn")
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn reject_transition_pending_to_rejected_and_audit() {
        let state = test_state();
        let sha = "f".repeat(40);
        let apr = seed_ledger_entry(&state, "apr-4", "system/rejected.md", &sha).await;

        // Approved entries cannot be rejected (only revoked).
        state
            .spec_approvals
            .approve(&apr, 1700000100)
            .await
            .unwrap()
            .expect("entry exists");
        assert!(
            state
                .spec_approvals
                .reject(&apr, "user:reviewer", "no", 1700000150)
                .await
                .is_err(),
            "Approved → Rejected must be an invalid transition"
        );

        // Pending entries reject cleanly and never verify.
        let apr2 = seed_ledger_entry(&state, "apr-5", "system/rejected2.md", &sha).await;
        state
            .spec_approvals
            .reject(&apr2, "user:reviewer", "not ready", 1700000200)
            .await
            .unwrap()
            .expect("entry exists");
        let spec_ref = format!("system/rejected2.md@{sha}");
        assert!(
            verify_spec_ref(&state, &spec_ref).await.is_err(),
            "rejected (never approved) entry must not verify"
        );
    }

    /// Approval status is queried per SHA (task plan step 6: "on query:
    /// return approval status per SHA"). The exact-(path, sha) lookup must
    /// return only that version's rows: an approval for a different SHA of
    /// the same path must never satisfy a merge check for this SHA, and
    /// verify_spec_ref must deny while the version is Pending.
    #[tokio::test(flavor = "multi_thread")]
    async fn find_by_spec_sha_scopes_to_exact_version() {
        let state = test_state();
        let sha1 = "1".repeat(40);
        let sha2 = "2".repeat(40);
        seed_ledger_entry(&state, "apr-6", "system/versioned.md", &sha1).await;
        let apr2 = seed_ledger_entry(&state, "apr-7", "system/versioned.md", &sha2).await;

        // Approve only sha2.
        state
            .spec_approvals
            .approve(&apr2, 1700000100)
            .await
            .unwrap()
            .expect("entry exists");

        // Per-SHA query returns only that version's rows.
        let for_sha1 = state
            .spec_approvals
            .find_by_spec_sha("system/versioned.md", &sha1)
            .await
            .unwrap();
        assert_eq!(for_sha1.len(), 1);
        assert_eq!(for_sha1[0].spec_sha, sha1);
        assert_eq!(
            for_sha1[0].status(),
            gyre_domain::spec_approval::ApprovalStatus::Pending
        );

        let for_sha2 = state
            .spec_approvals
            .find_by_spec_sha("system/versioned.md", &sha2)
            .await
            .unwrap();
        assert_eq!(for_sha2.len(), 1);
        assert_eq!(for_sha2[0].spec_sha, sha2);
        assert_eq!(
            for_sha2[0].status(),
            gyre_domain::spec_approval::ApprovalStatus::Approved
        );

        // Unknown SHA for the same path: no rows, no accidental path-wide match.
        let for_missing = state
            .spec_approvals
            .find_by_spec_sha("system/versioned.md", &"3".repeat(40))
            .await
            .unwrap();
        assert!(
            for_missing.is_empty(),
            "per-SHA lookup must not return other versions' rows"
        );

        // Forge check (step 8): sha2 verifies, sha1 (same path, Pending) must
        // not — approving one version never approves its siblings.
        assert!(
            verify_spec_ref(&state, &format!("system/versioned.md@{sha2}"))
                .await
                .is_ok()
        );
        assert!(
            verify_spec_ref(&state, &format!("system/versioned.md@{sha1}"))
                .await
                .is_err(),
            "Pending version must not verify even though a sibling SHA is approved"
        );
    }
}
