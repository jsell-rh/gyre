//! S3.3: Spec Editing Backend — assist/save/prompts
//!
//! POST /api/v1/repos/:id/specs/assist   — LLM-assisted editing (SSE stream)
//! POST /api/v1/repos/:id/specs/save     — commit spec to feature branch + create MR
//! POST /api/v1/repos/:id/prompts/save   — direct commit to default branch

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::sse::{Event, Sse},
    response::{IntoResponse, Response},
    Json,
};
use futures_util::{stream, StreamExt as _};
use gyre_common::{Id, Notification, NotificationType};
use gyre_domain::{CostEntry, MergeRequest, MrStatus};
use serde::{Deserialize, Serialize};
use std::{sync::Arc, time::Duration};

use crate::{
    auth::AuthenticatedAgent,
    llm_rate_limit::{check_rate_limit, LLM_RATE_LIMIT, LLM_WINDOW_SECS},
    AppState,
};

use super::error::ApiError;
use super::{new_id, now_secs};

// ── Request / Response types ──────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct SpecAssistRequest {
    pub spec_path: String,
    pub instruction: String,
    pub draft_content: Option<String>,
}

/// Request body for POST /api/v1/workspaces/:id/meta-specs/assist — the
/// workspace-scoped LLM assist for the §9 meta-spec preview loop's State 1
/// chat (ui-layout.md §9). No repo or spec file is involved; the effective
/// content is the caller's in-editor draft (or the stored registry prompt
/// when only `meta_spec_id` is given).
#[derive(Deserialize)]
pub struct MetaSpecAssistRequest {
    pub instruction: String,
    /// The in-editor draft of the meta-spec prompt; overrides the stored
    /// registry prompt when present.
    #[serde(default)]
    pub draft_content: Option<String>,
    /// Registry id of the meta-spec being edited. Used to load the stored
    /// prompt (when no draft is given) and to scope-check the target.
    #[serde(default)]
    pub meta_spec_id: Option<String>,
}

#[derive(Deserialize)]
pub struct SpecSaveRequest {
    pub spec_path: String,
    pub content: String,
    pub message: String,
    /// Optimistic-concurrency token: the spec's `current_sha` (git blob SHA)
    /// at the time the client loaded it for editing (HSI §7). When present and
    /// it no longer matches the ledger's `current_sha`, the save is rejected
    /// with 409 Conflict unless `overwrite` is set.
    #[serde(default)]
    pub base_sha: Option<String>,
    /// Force the save even if `base_sha` is stale ("Overwrite" in the conflict
    /// dialog). Skips the concurrency check.
    #[serde(default)]
    pub overwrite: bool,
}

#[derive(Serialize)]
pub struct SpecSaveResponse {
    pub branch: String,
    pub mr_id: String,
}

#[derive(Deserialize)]
pub struct PromptSaveRequest {
    pub prompt_path: String,
    pub content: String,
    pub message: String,
}

#[derive(Serialize)]
pub struct PromptSaveResponse {
    pub commit_sha: String,
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Derive a URL-safe slug from a spec path for branch naming.
///
/// e.g. "specs/system/payment-retry.md" -> "specs-system-payment-retry-a1b2"
///
/// The 4-char hex suffix is a deterministic hash of the full original path,
/// preventing collisions between paths that produce the same slug segment
/// (e.g. "system/payment-retry.md" and "system/payment-retry-v2.md").
fn spec_path_slug(spec_path: &str) -> String {
    let without_ext = spec_path.trim_end_matches(".md");
    let slug = without_ext.replace('/', "-").to_lowercase();
    let hash: u32 = spec_path
        .bytes()
        .fold(0u32, |h, b| h.wrapping_mul(31).wrapping_add(b as u32));
    format!("{}-{:04x}", slug, hash & 0xffff)
}

/// A single line in an LCS-based text diff between two spec versions.
#[derive(Serialize)]
pub struct SpecDiffLine {
    /// `"context"` (unchanged), `"add"` (only in the submitted version), or
    /// `"remove"` (only in the current server version).
    pub op: String,
    pub text: String,
}

/// Compute a line-level diff between `current` (the server's version) and
/// `submitted` (the client's version) using longest-common-subsequence.
///
/// Lines only in `current` are `remove` (the concurrent edit dropped them),
/// lines only in `submitted` are `add` (the client's local changes), and
/// shared lines are `context`. Used to build the 409 conflict diff (HSI §7).
fn line_diff(current: &str, submitted: &str) -> Vec<SpecDiffLine> {
    let a: Vec<&str> = current.lines().collect();
    let b: Vec<&str> = submitted.lines().collect();
    let n = a.len();
    let m = b.len();

    // LCS length table.
    let mut lcs = vec![vec![0usize; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[i][j] = if a[i] == b[j] {
                lcs[i + 1][j + 1] + 1
            } else {
                lcs[i + 1][j].max(lcs[i][j + 1])
            };
        }
    }

    let mut out = Vec::new();
    let (mut i, mut j) = (0usize, 0usize);
    while i < n && j < m {
        if a[i] == b[j] {
            out.push(SpecDiffLine {
                op: "context".to_string(),
                text: a[i].to_string(),
            });
            i += 1;
            j += 1;
        } else if lcs[i + 1][j] >= lcs[i][j + 1] {
            out.push(SpecDiffLine {
                op: "remove".to_string(),
                text: a[i].to_string(),
            });
            i += 1;
        } else {
            out.push(SpecDiffLine {
                op: "add".to_string(),
                text: b[j].to_string(),
            });
            j += 1;
        }
    }
    while i < n {
        out.push(SpecDiffLine {
            op: "remove".to_string(),
            text: a[i].to_string(),
        });
        i += 1;
    }
    while j < m {
        out.push(SpecDiffLine {
            op: "add".to_string(),
            text: b[j].to_string(),
        });
        j += 1;
    }
    out
}

/// Read the current default-branch content of a spec file (best-effort).
///
/// Mirrors `get_spec`'s read path: specs live under `specs/<path>` in the repo,
/// but tolerates a `spec_path` that already carries the `specs/` prefix.
async fn read_current_spec_content(repo_path: &str, spec_path: &str) -> Option<String> {
    let git_bin = std::env::var("GYRE_GIT_PATH").unwrap_or_else(|_| "git".to_string());
    let stripped = spec_path.strip_prefix("specs/").unwrap_or(spec_path);
    let candidate = format!("specs/{stripped}");
    if let Some(c) =
        crate::spec_registry::read_git_file(&git_bin, repo_path, "HEAD", &candidate).await
    {
        return Some(c);
    }
    // Fall back to the raw path in case the file is not under specs/.
    crate::spec_registry::read_git_file(&git_bin, repo_path, "HEAD", spec_path).await
}

/// Build the 409 Conflict response for a stale spec save and create
/// `SpecConflict` notifications for both editors (HSI §7 Conflict Prevention).
///
/// Recipients are: the caller (the second editor, if the connection carries a
/// verified user identity) plus every *other* user currently editing the same
/// `spec:<path>` entity per the presence map. The response body carries both
/// versions and a line-level diff so the client can render a side-by-side view.
async fn spec_conflict_response(
    state: &Arc<AppState>,
    repo: &gyre_domain::Repository,
    caller: &AuthenticatedAgent,
    spec_path: &str,
    submitted_content: &str,
    base_sha: &str,
    current_sha: &str,
) -> Response {
    let current_content = read_current_spec_content(&repo.path, spec_path)
        .await
        .unwrap_or_default();
    let diff = line_diff(&current_content, submitted_content);
    let added = diff.iter().filter(|d| d.op == "add").count();
    let removed = diff.iter().filter(|d| d.op == "remove").count();
    let diff_summary = format!("+{added} / -{removed} lines");

    // Resolve tenant for the notifications.
    let tenant_id = match state.workspaces.find_by_id(&repo.workspace_id).await {
        Ok(Some(ws)) => ws.tenant_id.to_string(),
        _ => "unknown".to_string(),
    };

    // Determine notification recipients: the caller + other concurrent editors
    // of the same spec entity (from the presence map).
    let editing_entity = format!("spec:{spec_path}");
    let mut recipients: Vec<String> = Vec::new();
    if let Some(uid) = &caller.user_id {
        recipients.push(uid.to_string());
    }
    {
        let map = state.presence.read().await;
        for ((uid, _sid), entry) in map.iter() {
            if entry.editing_entity.as_deref() == Some(editing_entity.as_str())
                && !recipients.contains(uid)
            {
                recipients.push(uid.clone());
            }
        }
    }

    let now = now_secs();
    // Persist the full line diff (and summary) in the notification body so BOTH
    // editors get a diff view in their Inbox (HSI §7 item 3) — not just the
    // second editor who receives the transient 409 response. `to_value` borrows
    // `diff` so it remains available for the HTTP response below.
    let diff_value =
        serde_json::to_value(&diff).unwrap_or_else(|_| serde_json::Value::Array(vec![]));
    let body = serde_json::json!({
        "spec_path": spec_path,
        "base_sha": base_sha,
        "current_sha": current_sha,
        "diff_summary": diff_summary,
        "diff": diff_value,
    })
    .to_string();

    for uid in &recipients {
        let mut notif = Notification::new(
            new_id(),
            repo.workspace_id.clone(),
            Id::new(uid),
            NotificationType::SpecConflict,
            format!("Spec edit conflict: {spec_path}"),
            &tenant_id,
            now as i64,
        );
        notif.body = Some(body.clone());
        notif.source.entity_type = "spec".to_string();
        notif.source.entity_id = spec_path.to_string();
        notif.source.repo_id = Some(repo.id.clone());
        if let Err(e) = state.notifications.create(&notif).await {
            tracing::warn!(spec_path = %spec_path, recipient = %uid, "Failed to create spec-conflict notification: {e}");
        }
    }

    (
        StatusCode::CONFLICT,
        Json(serde_json::json!({
            "error": "Spec was modified since you loaded it. Reload to see the latest version, or overwrite.",
            "spec_path": spec_path,
            "base_sha": base_sha,
            "current_sha": current_sha,
            "current_content": current_content,
            "submitted_content": submitted_content,
            "diff": diff,
        })),
    )
        .into_response()
}

// ── Handlers ──────────────────────────────────────────────────────────────────

/// POST /api/v1/repos/:id/specs/assist
///
/// LLM-assisted spec editing. Returns an SSE stream with partial explanation
/// chunks and a final complete event containing `{diff, explanation}`.
///
/// The LLM reads the current spec content (from the repo's default branch or
/// `draft_content` if provided) and knowledge graph context (entities governed
/// by this spec). It produces a JSON response with a `diff` array of edit
/// operations and an `explanation` string.
///
/// If `spec_path` does not exist and no `draft_content` is provided, returns 404.
/// If the LLM returns invalid JSON, an `event: error` SSE event is emitted.
///
/// ABAC: resource_type "spec", action "generate".
pub async fn assist_spec(
    State(state): State<Arc<AppState>>,
    Path(repo_id): Path<String>,
    caller: AuthenticatedAgent,
    Json(req): Json<SpecAssistRequest>,
) -> Result<Sse<impl futures_util::Stream<Item = Result<Event, std::convert::Infallible>>>, ApiError>
{
    let repo_id_typed = Id::new(&repo_id);
    let repo = state
        .repos
        .find_by_id(&repo_id_typed)
        .await
        .map_err(ApiError::Internal)?
        .ok_or_else(|| ApiError::NotFound(format!("repo {} not found", repo_id)))?;

    // Per-user/workspace sliding-window rate limit (HSI §6): 10 req/60 s.
    {
        let workspace_id = repo.workspace_id.to_string();
        let mut limiter = state.llm_rate_limiter.lock().await;
        if let Err(retry_after) = check_rate_limit(
            &mut limiter,
            &caller.agent_id,
            &workspace_id,
            LLM_RATE_LIMIT,
            LLM_WINDOW_SECS,
        ) {
            return Err(ApiError::RateLimited(retry_after));
        }
    }

    // Require LLM to be configured.
    let factory = state
        .llm
        .as_ref()
        .ok_or(super::error::ApiError::LlmUnavailable)?;

    // Determine effective spec content: draft_content overrides committed content.
    let spec_content = if let Some(ref draft) = req.draft_content {
        draft.clone()
    } else {
        // Read spec from the repo's default branch.
        match state
            .git_ops
            .read_file(&repo.path, &repo.default_branch, &req.spec_path)
            .await
        {
            Ok(Some(bytes)) => String::from_utf8_lossy(&bytes).to_string(),
            Ok(None) => {
                // Spec does not exist and no draft_content — 404.
                return Err(ApiError::NotFound(format!(
                    "spec {} not found in repo {}",
                    req.spec_path, repo_id
                )));
            }
            Err(e) => {
                tracing::warn!(
                    repo_id = %repo_id,
                    spec_path = %req.spec_path,
                    "Failed to read spec from repo: {e}"
                );
                // Fall back to empty content so the LLM can still assist.
                String::new()
            }
        }
    };

    // Build knowledge graph context: query nodes governed by this spec.
    let graph_context = match state
        .graph_store
        .get_nodes_by_spec(&repo_id_typed, &req.spec_path)
        .await
    {
        Ok(nodes) if !nodes.is_empty() => {
            let summaries: Vec<String> = nodes
                .iter()
                .take(50) // Limit context size
                .map(|n| {
                    format!(
                        "- {} ({:?}): {} [{}:{}–{}]",
                        n.name,
                        n.node_type,
                        n.qualified_name,
                        n.file_path,
                        n.line_start,
                        n.line_end
                    )
                })
                .collect();
            summaries.join("\n")
        }
        Ok(_) => "No graph nodes are currently linked to this spec.".to_string(),
        Err(e) => {
            tracing::warn!(
                repo_id = %repo_id,
                spec_path = %req.spec_path,
                "Failed to load graph context: {e}"
            );
            "Graph context unavailable.".to_string()
        }
    };

    // Load effective prompt; fall back to hardcoded default.
    let template_content = state
        .prompt_templates
        .get_effective(&repo.workspace_id, "specs-assist")
        .await
        .map_err(ApiError::Internal)?
        .map(|t| t.content)
        .unwrap_or_else(|| crate::llm_defaults::PROMPT_SPECS_ASSIST.to_string());

    let system_prompt = template_content
        .replace("{{spec_path}}", &req.spec_path)
        .replace("{{spec_content}}", &spec_content)
        .replace("{{graph_context}}", &graph_context)
        .replace("{{instruction}}", &req.instruction)
        // Backward compat: old templates may use {{draft_content}}
        .replace(
            "{{draft_content}}",
            req.draft_content.as_deref().unwrap_or(&spec_content),
        );
    let user_prompt = format!("Instruction: {}", req.instruction);

    // Resolve model and call streaming LLM.
    let (model, max_tokens) =
        crate::llm_helpers::resolve_llm_model(&state, &repo.workspace_id, "specs-assist").await;
    let llm_stream = factory
        .for_model(&model)
        .stream_complete(&system_prompt, &user_prompt, max_tokens)
        .await
        .map_err(ApiError::Internal)?;

    let chunks: Vec<String> = llm_stream.filter_map(|r| async { r.ok() }).collect().await;
    let full_text = chunks.join("");

    // Budget tracking: charge workspace for LLM usage (ui-layout.md §3 line 158,
    // platform-model.md §Budget Tracking). The LlmPort does not return actual
    // usage, so keep the existing estimate — ~4 chars per token for the
    // prompts plus response overhead and a 3x reasoning multiplier — and
    // split it into input/output for the per-call budget record so that
    // input + output equals the estimate.
    let estimated_input = (user_prompt.len() + system_prompt.len()) / 4;
    let base_estimate = (estimated_input as f64 + 500.0) * 3.0;
    let cost_entry = CostEntry::new(
        new_id(),
        Id::new(caller.agent_id.clone()),
        None,
        "llm_query",
        base_estimate,
        "tokens",
        now_secs(),
    );
    if let Err(e) = state.costs.record(&cost_entry).await {
        tracing::warn!("Failed to record specs/assist cost entry: {e}");
    }

    // Per-call budget record + workspace/tenant counter increment
    // (platform-model.md §Budget Tracking).
    let ws_for_budget = state
        .workspaces
        .find_by_id(&repo.workspace_id)
        .await
        .ok()
        .flatten();
    if let Some(ws) = ws_for_budget {
        super::budget::record_llm_call_usage(
            &state,
            &super::budget::LlmCallUsage {
                tenant_id: ws.tenant_id.clone(),
                workspace_id: ws.id.clone(),
                repo_id: Some(repo.id.clone()),
                agent_id: None,
                task_id: None,
                usage_type: "llm_query".to_string(),
                input_tokens: estimated_input as u64,
                output_tokens: (base_estimate - estimated_input as f64) as u64,
                cost_usd: 0.0,
                model: model.clone(),
            },
        )
        .await;
    } else {
        tracing::warn!(
            workspace_id = %repo.workspace_id,
            "specs/assist budget recording skipped: workspace not found"
        );
    }

    Ok(assist_sse_response(assist_sse_events(&full_text)))
}

/// POST /api/v1/workspaces/:id/meta-specs/assist
///
/// Workspace-scoped LLM assist for the §9 meta-spec preview loop's State 1
/// chat (ui-layout.md §9): the persona/principle editor's chat input calls
/// this endpoint with the in-editor draft, and the response uses the same
/// SSE `{diff, explanation}` protocol as the repo-scoped specs/assist so the
/// editor renders the same inline Accept/Edit/Dismiss suggestion blocks.
///
/// Unlike the repo-scoped route there is no repo or spec file: the effective
/// content is the caller's `draft_content`, falling back to the stored
/// registry prompt when only `meta_spec_id` is given. Rate limiting and
/// budget accounting are keyed to the workspace.
///
/// ABAC: resource_type "meta_spec", action "generate" (same action as the
/// repo-scoped specs/assist).
pub async fn assist_meta_spec(
    State(state): State<Arc<AppState>>,
    Path(workspace_id): Path<String>,
    caller: AuthenticatedAgent,
    Json(req): Json<MetaSpecAssistRequest>,
) -> Result<Sse<impl futures_util::Stream<Item = Result<Event, std::convert::Infallible>>>, ApiError>
{
    let ws_id = Id::new(&workspace_id);
    let ws = state
        .workspaces
        .find_by_id(&ws_id)
        .await
        .map_err(ApiError::Internal)?
        .ok_or_else(|| ApiError::NotFound(format!("workspace {workspace_id} not found")))?;

    // Per-user/workspace sliding-window rate limit (HSI §6): 10 req/60 s —
    // same limits as the repo-scoped assist, keyed to the workspace.
    {
        let mut limiter = state.llm_rate_limiter.lock().await;
        if let Err(retry_after) = check_rate_limit(
            &mut limiter,
            &caller.agent_id,
            &workspace_id,
            LLM_RATE_LIMIT,
            LLM_WINDOW_SECS,
        ) {
            return Err(ApiError::RateLimited(retry_after));
        }
    }

    // Require LLM to be configured.
    let factory = state
        .llm
        .as_ref()
        .ok_or(super::error::ApiError::LlmUnavailable)?;

    // Resolve the meta-spec target: registry entry (when an id is given) plus
    // the effective content (draft overrides the stored prompt). A
    // Workspace-scoped meta-spec from another workspace is a cross-scope
    // access, not a miss — reject it as Forbidden rather than leaking its
    // existence via a different error shape.
    let (meta_kind, meta_name, published_prompt) = match req.meta_spec_id.as_deref() {
        Some(id) => {
            let ms = state
                .meta_specs
                .get_by_id(&Id::new(id))
                .await
                .map_err(ApiError::Internal)?
                .ok_or_else(|| ApiError::NotFound(format!("meta-spec {id} not found")))?;
            if ms.scope == gyre_domain::MetaSpecScope::Workspace
                && ms.scope_id.as_deref() != Some(workspace_id.as_str())
            {
                return Err(ApiError::Forbidden(format!(
                    "meta-spec {id} does not belong to workspace {workspace_id}"
                )));
            }
            (ms.kind.as_str().to_string(), ms.name.clone(), Some(ms.prompt))
        }
        None => ("meta-spec".to_string(), "draft".to_string(), None),
    };

    let spec_content = req
        .draft_content
        .clone()
        .or(published_prompt)
        .ok_or_else(|| {
            ApiError::BadRequest(
                "draft_content or meta_spec_id is required for meta-spec assist".to_string(),
            )
        })?;

    // Load effective prompt; fall back to hardcoded default.
    let template_content = state
        .prompt_templates
        .get_effective(&ws_id, "meta-specs-assist")
        .await
        .map_err(ApiError::Internal)?
        .map(|t| t.content)
        .unwrap_or_else(|| crate::llm_defaults::PROMPT_META_SPECS_ASSIST.to_string());

    let system_prompt = template_content
        .replace("{{meta_spec_kind}}", &meta_kind)
        .replace("{{meta_spec_name}}", &meta_name)
        .replace("{{meta_spec_content}}", &spec_content)
        .replace("{{instruction}}", &req.instruction);
    let user_prompt = format!("Instruction: {}", req.instruction);

    // Resolve model and call streaming LLM.
    let (model, max_tokens) =
        crate::llm_helpers::resolve_llm_model(&state, &ws_id, "meta-specs-assist").await;
    let llm_stream = factory
        .for_model(&model)
        .stream_complete(&system_prompt, &user_prompt, max_tokens)
        .await
        .map_err(ApiError::Internal)?;

    let chunks: Vec<String> = llm_stream.filter_map(|r| async { r.ok() }).collect().await;
    let full_text = chunks.join("");

    // Budget tracking: charge the workspace for LLM usage (same estimate as
    // the repo-scoped assist — ~4 chars per token plus response overhead and
    // a 3x reasoning multiplier), split into input/output for the per-call
    // budget record.
    let estimated_input = (user_prompt.len() + system_prompt.len()) / 4;
    let base_estimate = (estimated_input as f64 + 500.0) * 3.0;
    let cost_entry = CostEntry::new(
        new_id(),
        Id::new(caller.agent_id.clone()),
        None,
        "llm_query",
        base_estimate,
        "tokens",
        now_secs(),
    );
    if let Err(e) = state.costs.record(&cost_entry).await {
        tracing::warn!("Failed to record meta-specs/assist cost entry: {e}");
    }

    super::budget::record_llm_call_usage(
        &state,
        &super::budget::LlmCallUsage {
            tenant_id: ws.tenant_id.clone(),
            workspace_id: ws.id.clone(),
            repo_id: None,
            agent_id: None,
            task_id: None,
            usage_type: "llm_query".to_string(),
            input_tokens: estimated_input as u64,
            output_tokens: (base_estimate - estimated_input as f64) as u64,
            cost_usd: 0.0,
            model: model.clone(),
        },
    )
    .await;

    Ok(assist_sse_response(assist_sse_events(&full_text)))
}

// ── Shared SSE protocol ───────────────────────────────────────────────────────

/// Build the SSE event list for an LLM assist response.
///
/// Shared by the repo-scoped spec assist (`POST /repos/:id/specs/assist`) and
/// the workspace-scoped meta-spec assist (`POST /workspaces/:id/meta-specs/assist`)
/// so both surfaces emit the identical wire protocol the editor's SSE parser
/// consumes: `partial` events streaming the explanation progressively, a final
/// `complete` event carrying `{diff, explanation}`, or an `error` event when the
/// LLM output is not valid `{diff, explanation}` JSON with well-formed ops.
fn assist_sse_events(full_text: &str) -> Vec<Result<Event, std::convert::Infallible>> {
    let mut events: Vec<Result<Event, std::convert::Infallible>> = Vec::new();

    // Try to parse the LLM response as the spec-required {diff, explanation} JSON.
    match serde_json::from_str::<serde_json::Value>(full_text) {
        Ok(parsed) if parsed.get("diff").is_some() && parsed.get("explanation").is_some() => {
            // Valid response — stream explanation as partial events.
            let explanation = parsed["explanation"].as_str().unwrap_or("");
            if !explanation.is_empty() {
                // Emit explanation in chunks for progressive rendering.
                let exp_chars: Vec<char> = explanation.chars().collect();
                let chunk_size = exp_chars.len().max(1).div_ceil(3);
                for chunk in exp_chars.chunks(chunk_size) {
                    let text: String = chunk.iter().collect();
                    let data = serde_json::to_string(&serde_json::json!({"text": text}))
                        .unwrap_or_default();
                    events.push(Ok(Event::default().event("partial").data(data)));
                }
            }

            // Validate diff ops: each must have op, path, content.
            let diff = parsed["diff"].as_array();
            let valid_diff = diff
                .map(|ops| {
                    ops.iter().all(|op| {
                        let op_str = op.get("op").and_then(|v| v.as_str()).unwrap_or("");
                        let has_path = op.get("path").and_then(|v| v.as_str()).is_some();
                        matches!(op_str, "add" | "remove" | "replace") && has_path
                    })
                })
                .unwrap_or(false);

            if valid_diff {
                events.push(Ok(Event::default().event("complete").data(
                    serde_json::to_string(&serde_json::json!({
                        "diff": parsed["diff"],
                        "explanation": parsed["explanation"],
                    }))
                    .unwrap_or_default(),
                )));
            } else {
                // diff ops are invalid — send error event
                let error_data = serde_json::to_string(&serde_json::json!({
                    "error": "LLM produced invalid diff operations. Each operation must have op (add/remove/replace), path, and content fields.",
                    "explanation": explanation,
                }))
                .unwrap_or_default();
                events.push(Ok(Event::default().event("error").data(error_data)));
            }
        }
        Ok(_) => {
            // JSON parsed but missing required fields — send error event.
            let error_data = serde_json::to_string(&serde_json::json!({
                "error": "LLM response is valid JSON but missing required 'diff' and/or 'explanation' fields.",
                "raw_response": full_text,
            }))
            .unwrap_or_default();
            events.push(Ok(Event::default().event("error").data(error_data)));
        }
        Err(_) => {
            // LLM returned invalid JSON — send error event.
            let error_data = serde_json::to_string(&serde_json::json!({
                "error": "LLM returned invalid JSON. Please try rephrasing your instruction.",
                "raw_response": full_text,
            }))
            .unwrap_or_default();
            events.push(Ok(Event::default().event("error").data(error_data)));
        }
    }

    events
}

/// Wrap pre-built assist SSE events in a keep-alive SSE response.
fn assist_sse_response(
    events: Vec<Result<Event, std::convert::Infallible>>,
) -> Sse<impl futures_util::Stream<Item = Result<Event, std::convert::Infallible>>> {
    let s = stream::iter(events);
    Sse::new(s).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keep-alive"),
    )
}

/// Git blob SHA of `content` as stored at `spec_path` on `branch`.
///
/// Primary path: `git rev-parse <branch>:<path>` via the spec registry
/// (handles any object format). Fallback when git is unavailable: derive the
/// SHA-1 git blob id directly from the content just written to the branch —
/// a blob id is sha1("blob <len>\0<content>") over the exact bytes
/// `write_file` stored, so it matches what rev-parse returns for the same
/// content (the same value `git hash-object` prints for those bytes).
///
/// Never returns "": an empty spec_sha is a fail-open that pushes the Inbox
/// approve action onto a legacy fallback which approves the pre-edit ledger
/// version — a no-op masked by an "Approved" success message.
async fn spec_blob_sha(
    git_bin: &str,
    repo_path: &str,
    branch: &str,
    spec_path: &str,
    content: &str,
) -> String {
    match crate::spec_registry::get_blob_sha(git_bin, repo_path, branch, spec_path).await {
        Some(sha) => sha,
        None => {
            use sha1::{Digest, Sha1};
            let mut hasher = Sha1::new();
            hasher.update(format!("blob {}\0", content.len()).as_bytes());
            hasher.update(content.as_bytes());
            hex::encode(hasher.finalize())
        }
    }
}

/// POST /api/v1/repos/:id/specs/save
///
/// Commits a spec change to a `spec-edit/<slug>-<uuid>` feature branch and
/// auto-creates an MR targeting the default branch. If an existing open MR
/// for the same spec_path exists (matched by branch prefix and the MR author
/// is the current user), appends a commit to the existing branch.
///
/// Creates a priority-2 (High) "Spec pending approval" notification. The
/// notification's entity_id is the MR ID so the Inbox "Approve" action can
/// enqueue the MR.
///
/// ABAC: resource_type "spec", action "write".
pub async fn save_spec(
    State(state): State<Arc<AppState>>,
    Path(repo_id): Path<String>,
    caller: AuthenticatedAgent,
    Json(req): Json<SpecSaveRequest>,
) -> Result<Response, ApiError> {
    let repo_id_typed = Id::new(&repo_id);
    let repo = state
        .repos
        .find_by_id(&repo_id_typed)
        .await
        .map_err(ApiError::Internal)?
        .ok_or_else(|| ApiError::NotFound(format!("repo {} not found", repo_id)))?;

    // Optimistic concurrency (HSI §7 Conflict Prevention): if the client sent
    // the `base_sha` it loaded and the ledger's `current_sha` has since advanced
    // (another editor's change was merged), reject with 409 Conflict and notify
    // both editors — unless the client explicitly chose to overwrite.
    if !req.overwrite {
        if let Some(base_sha) = req.base_sha.as_deref() {
            let stripped = req
                .spec_path
                .strip_prefix("specs/")
                .unwrap_or(&req.spec_path)
                .to_string();
            let candidates: Vec<String> = if stripped == req.spec_path {
                vec![req.spec_path.clone()]
            } else {
                vec![req.spec_path.clone(), stripped]
            };
            let mut current_sha_opt: Option<String> = None;
            for c in &candidates {
                if let Ok(Some(entry)) = state.spec_ledger.find_by_path(c).await {
                    current_sha_opt = Some(entry.current_sha);
                    break;
                }
            }
            if let Some(current_sha) = current_sha_opt {
                if !current_sha.is_empty() && current_sha != base_sha {
                    return Ok(spec_conflict_response(
                        &state,
                        &repo,
                        &caller,
                        &req.spec_path,
                        &req.content,
                        base_sha,
                        &current_sha,
                    )
                    .await);
                }
            }
        }
    }

    let slug = spec_path_slug(&req.spec_path);
    let branch_prefix = format!("spec-edit/{}", slug);

    // Check for an existing open MR for this spec_path (matched by branch prefix).
    let all_mrs = state
        .merge_requests
        .list_by_repo(&repo_id_typed)
        .await
        .unwrap_or_default();
    let caller_id = Id::new(&caller.agent_id);
    let existing_mr = all_mrs.into_iter().find(|mr| {
        mr.status == MrStatus::Open
            && mr.source_branch.starts_with(&branch_prefix)
            && mr.author_agent_id.as_ref() == Some(&caller_id)
    });

    if let Some(mr) = existing_mr {
        // Existing open MR — append a commit to the existing branch.
        state
            .git_ops
            .write_file(
                &repo.path,
                &mr.source_branch,
                &req.spec_path,
                req.content.as_bytes(),
                &req.message,
            )
            .await
            .map_err(ApiError::Internal)?;

        // task-175 review F1: the SpecPendingApproval cards created by the
        // first save carry that save's blob SHA in their body, and the Inbox
        // approve action submits body.spec_sha — approve_spec only records
        // the approval when it matches the ledger's post-merge current_sha.
        // Leaving the first save's SHA in place would make every Approve
        // click on this card record an approval for content that never
        // becomes current: the card shows "Approved" while the spec stays
        // Pending, permanently. Refresh the unresolved cards' body with the
        // blob SHA of the content just appended (resolved cards keep their
        // historical body — an audit record of what was actually acted on).
        let git_bin = std::env::var("GYRE_GIT_PATH").unwrap_or_else(|_| "git".to_string());
        let blob_sha = spec_blob_sha(
            &git_bin,
            &repo.path,
            &mr.source_branch,
            &req.spec_path,
            &req.content,
        )
        .await;
        let refreshed_body = serde_json::json!({
            "spec_path": req.spec_path,
            "spec_sha": blob_sha,
            "mr_id": mr.id.to_string(),
            "mr_title": mr.title.clone(),
            "branch": mr.source_branch.clone(),
            "repo_id": repo.id.to_string(),
        })
        .to_string();
        if let Err(e) = state
            .notifications
            .update_body_by_entity_ref(
                mr.id.as_str(),
                NotificationType::SpecApprovalRequested.as_str(),
                &refreshed_body,
            )
            .await
        {
            // Non-fatal — the commit is already on the MR branch; the card
            // body going stale is logged, not silently swallowed.
            tracing::warn!(
                mr_id = %mr.id,
                "failed to refresh spec-pending-approval notification body: {e}"
            );
        }

        return Ok((
            StatusCode::OK,
            Json(SpecSaveResponse {
                branch: mr.source_branch,
                mr_id: mr.id.to_string(),
            }),
        )
            .into_response());
    }

    // Generate new branch: spec-edit/<slug>-<short_uuid>
    let short_uuid = &uuid::Uuid::new_v4().to_string().replace('-', "")[..8];
    let branch_name = format!("{}-{}", branch_prefix, short_uuid);

    // Create the feature branch from the default branch.
    state
        .git_ops
        .create_branch(&repo.path, &branch_name, &repo.default_branch)
        .await
        .map_err(ApiError::Internal)?;

    // Write the spec content to the new branch.
    state
        .git_ops
        .write_file(
            &repo.path,
            &branch_name,
            &req.spec_path,
            req.content.as_bytes(),
            &req.message,
        )
        .await
        .map_err(ApiError::Internal)?;

    let now = now_secs();
    let mr_id = new_id();
    let mut mr = MergeRequest::new(
        mr_id.clone(),
        repo_id_typed.clone(),
        format!("Spec edit: {}", req.spec_path),
        branch_name.clone(),
        repo.default_branch.clone(),
        now,
    );
    mr.workspace_id = repo.workspace_id.clone();
    mr.author_agent_id = Some(caller_id.clone());

    state
        .merge_requests
        .create(&mr)
        .await
        .map_err(ApiError::Internal)?;

    // Resolve tenant_id from workspace for the notification.
    let tenant_id = match state.workspaces.find_by_id(&repo.workspace_id).await {
        Ok(Some(ws)) => ws.tenant_id.to_string(),
        _ => "unknown".to_string(),
    };

    // Priority-2 "Spec pending approval" notification (HSI §2 + §8 P2,
    // user-management.md §Notification Entity).
    //
    // Fan out to every workspace Admin/Developer/Owner member — these are the
    // roles allowed to approve (reject_spec allows Admin/Developer). When the
    // workspace has no eligible members, fall back to "system" so the global
    // admin token's Inbox still shows the pending approval.
    //
    // The body carries everything the Inbox inline actions need:
    //   spec_path — for GET /specs/:path and the reject endpoint
    //   spec_sha  — the git blob SHA of the spec content on the MR branch
    //               (the version the human is approving; matches the ledger's
    //               current_sha semantics from sync_spec_ledger)
    //   mr_id / branch / repo_id — for the MR quick links
    //
    // The structured source (entity_type "mr", entity_id = MR id) carries the
    // same source entity the legacy body fields describe, for the
    // notification-entity deep-link surface.
    //
    // See spec_blob_sha for how the SHA is derived (git rev-parse with a
    // content-derived fallback — never "", which is a fail-open).
    let git_bin = std::env::var("GYRE_GIT_PATH").unwrap_or_else(|_| "git".to_string());
    let blob_sha = spec_blob_sha(
        &git_bin,
        &repo.path,
        &branch_name,
        &req.spec_path,
        &req.content,
    )
    .await;
    let notif_body = serde_json::json!({
        "spec_path": req.spec_path,
        "spec_sha": blob_sha,
        "mr_id": mr_id.to_string(),
        "mr_title": mr.title.clone(),
        "branch": branch_name.clone(),
        "repo_id": repo.id.to_string(),
    })
    .to_string();

    let members = state
        .workspace_memberships
        .list_by_workspace(&repo.workspace_id)
        .await
        .unwrap_or_default();
    let mut recipients: Vec<Id> = members
        .iter()
        .filter(|m| {
            use gyre_domain::WorkspaceRole;
            matches!(
                m.role,
                WorkspaceRole::Admin | WorkspaceRole::Developer | WorkspaceRole::Owner
            )
        })
        .map(|m| m.user_id.clone())
        .collect();
    if recipients.is_empty() {
        // No eligible members — deliver to the system account so the
        // global-admin Inbox still surfaces the pending approval.
        recipients.push(Id::new("system"));
    }

    for uid in recipients {
        let notif_id = new_id();
        let mut notif = Notification::new(
            notif_id,
            repo.workspace_id.clone(),
            uid.clone(),
            NotificationType::SpecApprovalRequested,
            format!("Spec pending approval: {}", req.spec_path),
            &tenant_id,
            now as i64,
        );
        notif.body = Some(notif_body.clone());
        notif.source.entity_type = "mr".to_string();
        notif.source.entity_id = mr_id.to_string();
        notif.source.repo_id = Some(repo.id.clone());
        // Non-fatal — MR is created even if notification fails.
        if let Err(e) = state.notifications.create(&notif).await {
            tracing::warn!(mr_id = %mr_id, user_id = %uid, "Failed to create spec-pending-approval notification: {e}");
        }
    }

    Ok((
        StatusCode::CREATED,
        Json(SpecSaveResponse {
            branch: branch_name,
            mr_id: mr_id.to_string(),
        }),
    )
        .into_response())
}

/// POST /api/v1/repos/:id/prompts/save
///
/// Direct commit of a prompt template to the default branch — no MR, no
/// approval notification. Prompt templates in `specs/prompts/` are in
/// spec-lifecycle's `ignored_paths`, enabling fast iteration without formal
/// spec approval.
///
/// ABAC: resource_type "spec", action "generate" (not "write") so that
/// Supervised trust's `trust:require-human-mr-review` policy does not block
/// prompt iteration.
pub async fn save_prompt(
    State(state): State<Arc<AppState>>,
    Path(repo_id): Path<String>,
    Json(req): Json<PromptSaveRequest>,
) -> Result<(StatusCode, Json<PromptSaveResponse>), ApiError> {
    let repo_id_typed = Id::new(&repo_id);
    let repo = state
        .repos
        .find_by_id(&repo_id_typed)
        .await
        .map_err(ApiError::Internal)?
        .ok_or_else(|| ApiError::NotFound(format!("repo {} not found", repo_id)))?;

    // Commit directly to the default branch (no feature branch, no MR).
    let commit_sha = state
        .git_ops
        .write_file(
            &repo.path,
            &repo.default_branch,
            &req.prompt_path,
            req.content.as_bytes(),
            &req.message,
        )
        .await
        .map_err(ApiError::Internal)?;

    Ok((StatusCode::CREATED, Json(PromptSaveResponse { commit_sha })))
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
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

    fn auth() -> &'static str {
        "Bearer test-token"
    }

    #[tokio::test]
    async fn assist_spec_not_found_returns_404() {
        let app = app();
        let body = serde_json::json!({
            "spec_path": "specs/system/vision.md",
            "instruction": "Add error handling section",
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/repos/no-such-repo/specs/assist")
                    .header("Authorization", auth())
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn assist_spec_rate_limited_after_10_requests() {
        let app = app();
        let repo_id = create_repo(&app, "rate-limit-test-repo", "ws-rate-test").await;

        let assist_body = serde_json::json!({
            "spec_path": "specs/system/vision.md",
            "instruction": "Add a summary",
            "draft_content": "# Vision\n\nExisting content.",
        });

        // First 10 requests must succeed.
        for i in 0..10 {
            let resp = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri(format!("/api/v1/repos/{}/specs/assist", repo_id))
                        .header("Authorization", auth())
                        .header("content-type", "application/json")
                        .body(Body::from(serde_json::to_vec(&assist_body).unwrap()))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::OK, "request {i} should succeed");
        }

        // 11th request must be rate-limited.
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/repos/{}/specs/assist", repo_id))
                    .header("Authorization", auth())
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&assist_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
        let retry_after = resp
            .headers()
            .get("Retry-After")
            .expect("Retry-After header");
        let secs: u64 = retry_after.to_str().unwrap().parse().unwrap();
        assert!(secs >= 1, "Retry-After must be at least 1 second");
    }

    #[tokio::test]
    async fn save_spec_not_found_returns_404() {
        let app = app();
        let body = serde_json::json!({
            "spec_path": "specs/system/vision.md",
            "content": "# Vision\n\nContent here.",
            "message": "Add vision spec",
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/repos/no-such-repo/specs/save")
                    .header("Authorization", auth())
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn save_prompt_not_found_returns_404() {
        let app = app();
        let body = serde_json::json!({
            "prompt_path": "specs/prompts/specs-assist.md",
            "content": "# Updated prompt",
            "message": "Tweak specs-assist prompt",
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/repos/no-such-repo/prompts/save")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn save_spec_creates_mr_for_existing_repo() {
        let state = test_state();
        let app = crate::api::api_router().with_state(state.clone());
        // Create a repo first.
        let create_body = serde_json::json!({
            "name": "spec-edit-test",
            "workspace_id": "ws-1",
            "tenant_id": "tenant-1",
        });
        let repo_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/repos")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&create_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(repo_resp.status(), StatusCode::CREATED);
        let repo_json = body_json(repo_resp).await;
        let repo_id = repo_json["id"].as_str().unwrap().to_string();

        // Save a spec — should create an MR.
        let save_body = serde_json::json!({
            "spec_path": "specs/system/vision.md",
            "content": "# Vision\n\nContent here.",
            "message": "Add vision spec",
        });
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/repos/{}/specs/save", repo_id))
                    .header("Authorization", auth())
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&save_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let json = body_json(resp).await;
        assert!(json["branch"].as_str().unwrap().starts_with("spec-edit/"));
        let mr_id = json["mr_id"].as_str().unwrap();
        assert!(!mr_id.is_empty());

        // Verify notification was created (F4: must verify side effects, not just response).
        let notifs = state.notifications.list_recent(10).await.unwrap();
        let spec_notif = notifs
            .iter()
            .find(|n| n.notification_type == NotificationType::SpecApprovalRequested)
            .expect("SpecApprovalRequested notification must be created");
        assert_eq!(spec_notif.priority_rank, 2);
        assert_eq!(spec_notif.source.entity_id.as_str(), mr_id);
        // HSI §8 P2: the body must carry the structured fields the Inbox
        // inline actions consume (approve SHA, MR diff fetch, quick links).
        let body: serde_json::Value =
            serde_json::from_str(spec_notif.body.as_deref().unwrap()).unwrap();
        assert_eq!(body["spec_path"], "specs/system/vision.md");
        assert_eq!(body["mr_id"], mr_id);
        assert_eq!(body["repo_id"], repo_id);
        // HSI §8 P2 / task-175 F3: spec_sha must be the blob SHA of the exact
        // content the human is approving — never "" (the old fail-open wrote
        // an empty SHA when git was unavailable, pushing the Inbox approve
        // action onto a legacy fallback that approves the pre-edit version).
        // The expected value was produced by `git hash-object` over the exact
        // saved content — an external anchor, not mirrored logic: a blob id
        // is sha1("blob <len>\0<content>").
        assert_eq!(body["spec_sha"], "8035a51635f85a1fa9452e01964fbbd9f01179d4");
        // No workspace members in the test state — the system fallback
        // recipient keeps the approval visible in the global-admin Inbox.
        assert_eq!(spec_notif.user_id, Id::new("system"));
    }

    #[tokio::test]
    async fn save_spec_fans_out_approval_notification_to_eligible_members() {
        use gyre_domain::{Repository, WorkspaceMembership, WorkspaceRole};

        let state = test_state();
        // Repo record only — the notification fan-out runs after the MR is
        // persisted but before any git-dependent assertion; git writes go
        // through NoopGitOps so the branch/blob lookups are inert.
        let ws_id = Id::new("ws-fanout");
        let repo = Repository::new(
            Id::new("repo-fanout"),
            ws_id.clone(),
            "fanout",
            "/tmp/gyre-nonexistent-fanout-repo",
            0,
        );
        state.repos.create(&repo).await.unwrap();
        // Two eligible members (Admin, Developer) and one ineligible (Viewer).
        for (uid, role) in [
            ("user-admin", WorkspaceRole::Admin),
            ("user-dev", WorkspaceRole::Developer),
            ("user-viewer", WorkspaceRole::Viewer),
        ] {
            state
                .workspace_memberships
                .create(&WorkspaceMembership::new(
                    Id::new(format!("membership-{uid}")),
                    Id::new(uid),
                    ws_id.clone(),
                    role,
                    Id::new("inviter"),
                    0,
                ))
                .await
                .unwrap();
        }

        let caller = AuthenticatedAgent {
            agent_id: "user-admin".to_string(),
            user_id: Some(Id::new("user-admin")),
            roles: vec![],
            tenant_id: "default".to_string(),
            jwt_claims: None,
            bearer_token: None,
            deprecated_token_auth: false,
            impersonation: None,
        };
        let req = SpecSaveRequest {
            spec_path: "specs/system/fanout.md".to_string(),
            content: "# Fanout\n".to_string(),
            message: "edit".to_string(),
            base_sha: None,
            overwrite: false,
        };
        let resp = save_spec(
            axum::extract::State(state.clone()),
            axum::extract::Path("repo-fanout".to_string()),
            caller,
            Json(req),
        )
        .await
        .expect("save_spec should succeed");
        assert_eq!(resp.status(), StatusCode::CREATED);

        // Eligible members each get exactly one notification; the Viewer
        // gets none; the system fallback is not used when members exist.
        for uid in ["user-admin", "user-dev"] {
            let notifs = state
                .notifications
                .list_for_user(&Id::new(uid), Some(&ws_id), None, None, None, 100, 0)
                .await
                .unwrap();
            let approvals: Vec<_> = notifs
                .iter()
                .filter(|n| n.notification_type == NotificationType::SpecApprovalRequested)
                .collect();
            assert_eq!(
                approvals.len(),
                1,
                "user {uid} must get exactly one SpecApprovalRequested notification"
            );
            let body: serde_json::Value =
                serde_json::from_str(approvals[0].body.as_deref().unwrap()).unwrap();
            assert_eq!(body["spec_path"], "specs/system/fanout.md");
            assert_eq!(body["repo_id"], "repo-fanout");
        }
        let viewer_notifs = state
            .notifications
            .list_for_user(
                &Id::new("user-viewer"),
                Some(&ws_id),
                None,
                None,
                None,
                100,
                0,
            )
            .await
            .unwrap();
        assert!(
            viewer_notifs.is_empty(),
            "Viewer must not receive approval notifications"
        );
        let system_notifs = state
            .notifications
            .list_for_user(&Id::new("system"), Some(&ws_id), None, None, None, 100, 0)
            .await
            .unwrap();
        assert!(
            system_notifs.is_empty(),
            "system fallback must not be used when eligible members exist"
        );
    }

    #[tokio::test]
    async fn save_spec_existing_mr_returns_same_mr() {
        let app = app();
        // Create a repo.
        let create_body = serde_json::json!({
            "name": "spec-edit-existing-mr",
            "workspace_id": "ws-1",
            "tenant_id": "tenant-1",
        });
        let repo_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/repos")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&create_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(repo_resp.status(), StatusCode::CREATED);
        let repo_json = body_json(repo_resp).await;
        let repo_id = repo_json["id"].as_str().unwrap().to_string();

        let save_body = serde_json::json!({
            "spec_path": "specs/system/vision.md",
            "content": "# Vision v1",
            "message": "First edit",
        });

        // First save — creates MR.
        let resp1 = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/repos/{}/specs/save", repo_id))
                    .header("Authorization", auth())
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&save_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp1.status(), StatusCode::CREATED);
        let json1 = body_json(resp1).await;
        let branch1 = json1["branch"].as_str().unwrap().to_string();
        let mr_id1 = json1["mr_id"].as_str().unwrap().to_string();

        // Second save to same spec_path — should return the existing MR.
        let save_body2 = serde_json::json!({
            "spec_path": "specs/system/vision.md",
            "content": "# Vision v2",
            "message": "Second edit",
        });
        let resp2 = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/repos/{}/specs/save", repo_id))
                    .header("Authorization", auth())
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&save_body2).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        // Existing MR case returns 200, not 201.
        assert_eq!(resp2.status(), StatusCode::OK);
        let json2 = body_json(resp2).await;
        assert_eq!(json2["branch"].as_str().unwrap(), branch1);
        assert_eq!(json2["mr_id"].as_str().unwrap(), mr_id1);
    }

    #[tokio::test]
    async fn save_spec_existing_mr_refreshes_notification_spec_sha() {
        // task-175 review F1: the append-to-existing-MR path used to return
        // early without refreshing the SpecPendingApproval card's body, so it
        // kept the FIRST save's blob SHA. The Inbox approve action submits
        // body.spec_sha and approve_spec only records the approval when it
        // matches the ledger's post-merge current_sha — a stale SHA makes
        // every Approve click record an approval for content that never
        // becomes current (card shows "Approved", spec stays Pending).
        let state = test_state();
        let app = crate::api::api_router().with_state(state.clone());
        let create_body = serde_json::json!({
            "name": "spec-edit-stale-sha",
            "workspace_id": "ws-1",
            "tenant_id": "tenant-1",
        });
        let repo_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/repos")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&create_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(repo_resp.status(), StatusCode::CREATED);
        let repo_id = body_json(repo_resp).await["id"]
            .as_str()
            .unwrap()
            .to_string();

        let do_save = |content: &'static str| {
            let body = serde_json::json!({
                "spec_path": "specs/system/vision.md",
                "content": content,
                "message": "edit",
            });
            app.clone().oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/repos/{}/specs/save", repo_id))
                    .header("Authorization", auth())
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
        };

        let resp1 = do_save("# Vision v1").await.unwrap();
        assert_eq!(resp1.status(), StatusCode::CREATED);
        let json1 = body_json(resp1).await;
        let mr_id = json1["mr_id"].as_str().unwrap().to_string();

        // First save's card carries the v1 blob SHA
        // (`git hash-object` over "# Vision v1").
        let notifs = state.notifications.list_recent(10).await.unwrap();
        let card = notifs
            .iter()
            .find(|n| {
                n.notification_type == NotificationType::SpecApprovalRequested
                    && n.source.entity_id == mr_id.as_str()
            })
            .expect("SpecApprovalRequested card after first save");
        let body: serde_json::Value = serde_json::from_str(card.body.as_deref().unwrap()).unwrap();
        assert_eq!(body["spec_sha"], "98537aa01f28c720793d8f199d221ebd97c3bff3");

        // Second save appends to the same MR — the card must be refreshed in
        // place (still exactly one card for this MR), carrying the v2 blob SHA
        // (`git hash-object` over "# Vision v2"), not the stale v1 SHA.
        let resp2 = do_save("# Vision v2").await.unwrap();
        assert_eq!(resp2.status(), StatusCode::OK);
        let json2 = body_json(resp2).await;
        assert_eq!(json2["mr_id"].as_str().unwrap(), mr_id);

        let notifs = state.notifications.list_recent(10).await.unwrap();
        let cards: Vec<&gyre_common::Notification> = notifs
            .iter()
            .filter(|n| {
                n.notification_type == NotificationType::SpecApprovalRequested
                    && n.source.entity_id == mr_id.as_str()
            })
            .collect();
        assert_eq!(
            cards.len(),
            1,
            "append must refresh the existing card, not create a duplicate"
        );
        let body: serde_json::Value =
            serde_json::from_str(cards[0].body.as_deref().unwrap()).unwrap();
        assert_eq!(body["spec_sha"], "1a862bdc192b340de10c4e0cd259c1c60c4cca11");
        assert_eq!(body["mr_id"], mr_id);
    }

    #[tokio::test]
    async fn save_prompt_returns_commit_sha() {
        let app = app();
        // Create a repo.
        let create_body = serde_json::json!({
            "name": "prompt-save-test",
            "workspace_id": "ws-1",
            "tenant_id": "tenant-1",
        });
        let repo_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/repos")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&create_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(repo_resp.status(), StatusCode::CREATED);
        let repo_json = body_json(repo_resp).await;
        let repo_id = repo_json["id"].as_str().unwrap().to_string();

        let save_body = serde_json::json!({
            "prompt_path": "specs/prompts/specs-assist.md",
            "content": "# Updated prompt template",
            "message": "Update specs-assist prompt",
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/repos/{}/prompts/save", repo_id))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&save_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let json = body_json(resp).await;
        let sha = json["commit_sha"].as_str().unwrap();
        // SHA should be a valid 40-char hex string
        assert_eq!(sha.len(), 40);
        assert!(sha.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn spec_path_slug_no_collision() {
        let s1 = spec_path_slug("specs/system/payment-retry.md");
        let s2 = spec_path_slug("specs/system/payment-retry-v2.md");
        assert_ne!(s1, s2, "different paths must produce different slugs");
        assert!(s1.starts_with("specs-system-payment-retry-"));
    }

    #[test]
    fn spec_path_slug_format() {
        let slug = spec_path_slug("specs/system/vision.md");
        // Must match: lowercase, no slashes, ends with 4-hex-char suffix
        assert!(slug.starts_with("specs-system-vision-"));
        let parts: Vec<&str> = slug.rsplitn(2, '-').collect();
        assert_eq!(parts[0].len(), 4);
        assert!(parts[0].chars().all(|c| c.is_ascii_hexdigit()));
    }

    // ── LLM endpoint tests ───────────────────────────────────────────────────

    fn app_no_llm() -> Router {
        let mut s = (*crate::mem::test_state()).clone();
        s.llm = None;
        crate::api::api_router().with_state(std::sync::Arc::new(s))
    }

    /// Create a test app with a mock LLM that returns a fixed response string.
    fn app_with_llm_response(response: &str) -> Router {
        let mut s = (*crate::mem::test_state()).clone();
        s.llm = Some(Arc::new(gyre_adapters::MockLlmPortFactory {
            inner: Arc::new(gyre_adapters::MockLlmAdapter::new(response)),
        }));
        crate::api::api_router().with_state(Arc::new(s))
    }

    /// Parse SSE body text into (event_type, data) pairs.
    fn parse_sse_events(body: &str) -> Vec<(String, String)> {
        let mut events = Vec::new();
        let mut current_event = String::new();
        let mut current_data = String::new();
        for line in body.lines() {
            if let Some(evt) = line.strip_prefix("event:") {
                current_event = evt.trim().to_string();
            } else if let Some(data) = line.strip_prefix("data:") {
                current_data = data.trim().to_string();
            } else if line.is_empty() && !current_event.is_empty() {
                events.push((current_event.clone(), current_data.clone()));
                current_event.clear();
                current_data.clear();
            }
        }
        // Capture last event if body doesn't end with empty line.
        if !current_event.is_empty() {
            events.push((current_event, current_data));
        }
        events
    }

    async fn body_text(resp: axum::response::Response) -> String {
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    async fn create_repo(app: &Router, name: &str, ws: &str) -> String {
        let create_body = serde_json::json!({
            "name": name,
            "workspace_id": ws,
            "tenant_id": "tenant-1",
        });
        let repo_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/repos")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&create_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(repo_resp.status(), StatusCode::CREATED);
        let json = body_json(repo_resp).await;
        json["id"].as_str().unwrap().to_string()
    }

    #[tokio::test]
    async fn assist_spec_returns_503_when_llm_unavailable() {
        let app = app_no_llm();
        let repo_id = create_repo(&app, "assist-503-repo", "ws-assist-503").await;

        let assist_body = serde_json::json!({
            "spec_path": "specs/system/vision.md",
            "instruction": "Add a summary",
            "draft_content": "# Vision\n\nExisting content.",
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/repos/{repo_id}/specs/assist"))
                    .header("Authorization", auth())
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&assist_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn assist_spec_nonexistent_spec_no_draft_returns_404() {
        // NoopGitOps.read_file returns None, so spec doesn't exist.
        // Without draft_content, the handler should return 404.
        let app = app();
        let repo_id = create_repo(&app, "assist-404-repo", "ws-assist-404").await;

        let assist_body = serde_json::json!({
            "spec_path": "specs/nonexistent.md",
            "instruction": "Add a section",
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/repos/{repo_id}/specs/assist"))
                    .header("Authorization", auth())
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&assist_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn assist_spec_with_valid_diff_response_streams_sse() {
        let valid_response = serde_json::json!({
            "diff": [
                {"op": "add", "path": "## Error Handling", "content": "When retries exceed max..."},
                {"op": "replace", "path": "## Overview", "content": "Updated overview text."}
            ],
            "explanation": "Added error handling section and updated overview."
        });
        let app = app_with_llm_response(&valid_response.to_string());
        let repo_id = create_repo(&app, "assist-valid-repo", "ws-assist-valid").await;

        let assist_body = serde_json::json!({
            "spec_path": "specs/system/vision.md",
            "instruction": "Add error handling",
            "draft_content": "# Vision\n\n## Overview\nOld overview.",
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/repos/{repo_id}/specs/assist"))
                    .header("Authorization", auth())
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&assist_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let ct = resp.headers().get("content-type").unwrap();
        assert!(ct.to_str().unwrap().contains("text/event-stream"));

        let body = body_text(resp).await;
        let events = parse_sse_events(&body);

        // Should have partial events (explanation chunks) + one complete event.
        let partial_events: Vec<_> = events.iter().filter(|(t, _)| t == "partial").collect();
        let complete_events: Vec<_> = events.iter().filter(|(t, _)| t == "complete").collect();
        assert!(
            !partial_events.is_empty(),
            "should have partial events for explanation"
        );
        assert_eq!(
            complete_events.len(),
            1,
            "should have exactly one complete event"
        );

        // Verify the complete event contains {diff, explanation}.
        let complete_data: serde_json::Value = serde_json::from_str(&complete_events[0].1).unwrap();
        assert!(complete_data.get("diff").unwrap().is_array());
        assert!(complete_data.get("explanation").unwrap().is_string());
        let diff = complete_data["diff"].as_array().unwrap();
        assert_eq!(diff.len(), 2);
        assert_eq!(diff[0]["op"], "add");
        assert_eq!(diff[1]["op"], "replace");
    }

    #[tokio::test]
    async fn assist_spec_invalid_json_from_llm_sends_error_event() {
        // Mock LLM returns non-JSON text.
        let app = app_with_llm_response("This is not valid JSON at all");
        let repo_id = create_repo(&app, "assist-badjson-repo", "ws-assist-badjson").await;

        let assist_body = serde_json::json!({
            "spec_path": "specs/system/vision.md",
            "instruction": "Add something",
            "draft_content": "# Vision",
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/repos/{repo_id}/specs/assist"))
                    .header("Authorization", auth())
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&assist_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body = body_text(resp).await;
        let events = parse_sse_events(&body);

        // Should have an error event, no complete event.
        let error_events: Vec<_> = events.iter().filter(|(t, _)| t == "error").collect();
        let complete_events: Vec<_> = events.iter().filter(|(t, _)| t == "complete").collect();
        assert_eq!(error_events.len(), 1, "should have one error event");
        assert_eq!(complete_events.len(), 0, "should have no complete event");

        let error_data: serde_json::Value = serde_json::from_str(&error_events[0].1).unwrap();
        assert!(error_data["error"]
            .as_str()
            .unwrap()
            .contains("invalid JSON"));
    }

    #[tokio::test]
    async fn assist_spec_json_missing_fields_sends_error_event() {
        // Mock LLM returns valid JSON but missing required fields.
        let app = app_with_llm_response(r#"{"some_field": "value"}"#);
        let repo_id = create_repo(&app, "assist-missingfields-repo", "ws-assist-missing").await;

        let assist_body = serde_json::json!({
            "spec_path": "specs/system/vision.md",
            "instruction": "Add something",
            "draft_content": "# Vision",
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/repos/{repo_id}/specs/assist"))
                    .header("Authorization", auth())
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&assist_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body = body_text(resp).await;
        let events = parse_sse_events(&body);

        let error_events: Vec<_> = events.iter().filter(|(t, _)| t == "error").collect();
        assert_eq!(error_events.len(), 1);
        let error_data: serde_json::Value = serde_json::from_str(&error_events[0].1).unwrap();
        assert!(error_data["error"]
            .as_str()
            .unwrap()
            .contains("missing required"));
    }

    #[tokio::test]
    async fn assist_spec_draft_content_overrides_repo_content() {
        // When draft_content is provided, the handler uses it instead of reading
        // from the repo (NoopGitOps returns None for read_file). This test verifies
        // that providing draft_content avoids the 404 that would occur without it.
        let valid_response = serde_json::json!({
            "diff": [{"op": "add", "path": "## New Section", "content": "New content."}],
            "explanation": "Added new section."
        });
        let app = app_with_llm_response(&valid_response.to_string());
        let repo_id = create_repo(&app, "assist-draft-repo", "ws-assist-draft").await;

        let assist_body = serde_json::json!({
            "spec_path": "specs/nonexistent.md",
            "instruction": "Add a new section",
            "draft_content": "# My Draft Spec\n\nDraft content here.",
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/repos/{repo_id}/specs/assist"))
                    .header("Authorization", auth())
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&assist_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        // Should succeed (200) because draft_content was provided, not 404.
        assert_eq!(resp.status(), StatusCode::OK);

        let body = body_text(resp).await;
        let events = parse_sse_events(&body);
        let complete_events: Vec<_> = events.iter().filter(|(t, _)| t == "complete").collect();
        assert_eq!(complete_events.len(), 1, "should have a complete event");
    }

    #[tokio::test]
    async fn assist_spec_new_spec_creation_with_draft() {
        // New spec creation: spec_path doesn't exist + draft_content provided.
        // The LLM should produce only "add" operations.
        let valid_response = serde_json::json!({
            "diff": [
                {"op": "add", "path": "# New Spec", "content": "# Payment Retry\n\nSpec content."}
            ],
            "explanation": "Created new payment retry spec."
        });
        let app = app_with_llm_response(&valid_response.to_string());
        let repo_id = create_repo(&app, "assist-newspec-repo", "ws-assist-newspec").await;

        let assist_body = serde_json::json!({
            "spec_path": "specs/system/payment-retry.md",
            "instruction": "Create a payment retry spec",
            "draft_content": "",
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/repos/{repo_id}/specs/assist"))
                    .header("Authorization", auth())
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&assist_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body = body_text(resp).await;
        let events = parse_sse_events(&body);
        let complete_events: Vec<_> = events.iter().filter(|(t, _)| t == "complete").collect();
        assert_eq!(complete_events.len(), 1);

        let complete_data: serde_json::Value = serde_json::from_str(&complete_events[0].1).unwrap();
        let diff = complete_data["diff"].as_array().unwrap();
        assert_eq!(diff.len(), 1);
        assert_eq!(diff[0]["op"], "add");
    }

    #[tokio::test]
    async fn assist_spec_invalid_diff_ops_sends_error() {
        // Valid JSON with diff/explanation but invalid op values.
        let bad_ops_response = serde_json::json!({
            "diff": [
                {"op": "invalid_op", "path": "## Section", "content": "text"}
            ],
            "explanation": "Some explanation."
        });
        let app = app_with_llm_response(&bad_ops_response.to_string());
        let repo_id = create_repo(&app, "assist-badops-repo", "ws-assist-badops").await;

        let assist_body = serde_json::json!({
            "spec_path": "specs/system/vision.md",
            "instruction": "Do something",
            "draft_content": "# Vision",
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/repos/{repo_id}/specs/assist"))
                    .header("Authorization", auth())
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&assist_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body = body_text(resp).await;
        let events = parse_sse_events(&body);
        let error_events: Vec<_> = events.iter().filter(|(t, _)| t == "error").collect();
        assert_eq!(error_events.len(), 1);
        let error_data: serde_json::Value = serde_json::from_str(&error_events[0].1).unwrap();
        assert!(error_data["error"]
            .as_str()
            .unwrap()
            .contains("invalid diff"));
    }

    // ── HSI §7 Conflict Prevention: optimistic concurrency on save ──────────

    use gyre_domain::spec_ledger::{ApprovalStatus, SpecLedgerEntry};

    fn ledger_entry(path: &str, sha: &str, workspace_id: &str) -> SpecLedgerEntry {
        SpecLedgerEntry {
            path: path.to_string(),
            title: format!("Spec {path}"),
            owner: "system".to_string(),
            kind: None,
            current_sha: sha.to_string(),
            approval_mode: "human_only".to_string(),
            approval_status: ApprovalStatus::Approved,
            linked_tasks: vec![],
            linked_mrs: vec![],
            drift_status: "clean".to_string(),
            created_at: 0,
            updated_at: 0,
            repo_id: None,
            workspace_id: Some(workspace_id.to_string()),
        }
    }

    #[tokio::test]
    async fn save_spec_stale_base_sha_returns_409_with_diff() {
        let state = test_state();
        let app = crate::api::api_router().with_state(state.clone());
        let repo_id = create_repo(&app, "conflict-repo", "ws-conf").await;

        // Ledger records the spec at a NEWER sha than what the client loaded.
        state
            .spec_ledger
            .save(&ledger_entry(
                "system/conflict.md",
                "server-sha-2",
                "ws-conf",
            ))
            .await
            .unwrap();

        let save_body = serde_json::json!({
            "spec_path": "specs/system/conflict.md",
            "content": "# Conflict\n\nMy local edit.\n",
            "message": "edit",
            "base_sha": "client-sha-1",
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/repos/{}/specs/save", repo_id))
                    .header("Authorization", auth())
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&save_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CONFLICT);
        let json = body_json(resp).await;
        assert_eq!(json["current_sha"].as_str().unwrap(), "server-sha-2");
        assert_eq!(json["base_sha"].as_str().unwrap(), "client-sha-1");
        let diff = json["diff"].as_array().expect("diff array");
        assert!(!diff.is_empty(), "diff must not be empty");
        // The submitted lines appear as `add` entries.
        assert!(
            diff.iter()
                .any(|d| d["op"] == "add" && d["text"] == "My local edit."),
            "diff should include the submitted line as an add: {diff:?}"
        );
    }

    #[tokio::test]
    async fn save_spec_matching_base_sha_creates_mr() {
        let state = test_state();
        let app = crate::api::api_router().with_state(state.clone());
        let repo_id = create_repo(&app, "match-repo", "ws-match").await;
        state
            .spec_ledger
            .save(&ledger_entry("system/ok.md", "sha-x", "ws-match"))
            .await
            .unwrap();

        let save_body = serde_json::json!({
            "spec_path": "specs/system/ok.md",
            "content": "# OK\n",
            "message": "edit",
            "base_sha": "sha-x",
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/repos/{}/specs/save", repo_id))
                    .header("Authorization", auth())
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&save_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
    }

    #[tokio::test]
    async fn save_spec_overwrite_bypasses_stale_base_sha() {
        let state = test_state();
        let app = crate::api::api_router().with_state(state.clone());
        let repo_id = create_repo(&app, "overwrite-repo", "ws-ovr").await;
        state
            .spec_ledger
            .save(&ledger_entry("system/ovr.md", "server-sha-2", "ws-ovr"))
            .await
            .unwrap();

        let save_body = serde_json::json!({
            "spec_path": "specs/system/ovr.md",
            "content": "# Overwrite\n",
            "message": "force",
            "base_sha": "client-sha-1",
            "overwrite": true,
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/repos/{}/specs/save", repo_id))
                    .header("Authorization", auth())
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&save_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        // Overwrite skips the concurrency check → creates the MR (201).
        assert_eq!(resp.status(), StatusCode::CREATED);
    }

    #[tokio::test]
    async fn save_spec_conflict_notifies_caller_and_concurrent_editor() {
        use crate::PresenceEntry;
        use gyre_domain::Repository;

        let state = test_state();
        // Repo record only — the 409 path returns before any git write.
        let repo = Repository::new(
            Id::new("repo-conf"),
            Id::new("ws-conf"),
            "conf",
            "/tmp/gyre-nonexistent-conf-repo",
            0,
        );
        state.repos.create(&repo).await.unwrap();
        state
            .spec_ledger
            .save(&ledger_entry(
                "system/conflict.md",
                "server-sha-2",
                "ws-conf",
            ))
            .await
            .unwrap();

        // Another human is concurrently editing the same spec (presence map).
        state.presence.write().await.insert(
            ("user-b".to_string(), "sess-b".to_string()),
            PresenceEntry {
                workspace_id: "ws-conf".to_string(),
                view: "specs".to_string(),
                editing_entity: Some("spec:specs/system/conflict.md".to_string()),
                timestamp: 0,
                server_last_seen: 0,
                connection_id: 1,
            },
        );

        let caller = AuthenticatedAgent {
            agent_id: "user-a".to_string(),
            user_id: Some(Id::new("user-a")),
            roles: vec![],
            tenant_id: "default".to_string(),
            jwt_claims: None,
            bearer_token: None,
            deprecated_token_auth: false,
            impersonation: None,
        };
        let req = SpecSaveRequest {
            spec_path: "specs/system/conflict.md".to_string(),
            content: "# Conflict\n\nSecond editor's changes.\n".to_string(),
            message: "edit".to_string(),
            base_sha: Some("client-sha-1".to_string()),
            overwrite: false,
        };
        let resp = match save_spec(
            axum::extract::State(state.clone()),
            axum::extract::Path("repo-conf".to_string()),
            caller,
            Json(req),
        )
        .await
        {
            Ok(r) => r,
            Err(_) => panic!("save_spec should return Ok(409 response), not Err"),
        };
        assert_eq!(resp.status(), StatusCode::CONFLICT);

        // Both editors receive a SpecConflict notification with the spec path.
        let ws = Id::new("ws-conf");
        for uid in ["user-a", "user-b"] {
            let notifs = state
                .notifications
                .list_for_user(&Id::new(uid), Some(&ws), None, None, None, 100, 0)
                .await
                .unwrap();
            let conflict: Vec<_> = notifs
                .iter()
                .filter(|n| n.notification_type == NotificationType::SpecConflict)
                .collect();
            assert_eq!(
                conflict.len(),
                1,
                "user {uid} must get exactly one SpecConflict notification, got {notifs:?}"
            );
            assert_eq!(conflict[0].priority_rank, 2);
            assert_eq!(
                conflict[0].source.entity_id.as_str(),
                "specs/system/conflict.md"
            );
            let body: serde_json::Value =
                serde_json::from_str(conflict[0].body.as_deref().unwrap()).unwrap();
            assert_eq!(body["current_sha"], "server-sha-2");
            // HSI §7 item 3: the diff must be persisted so BOTH editors get a
            // diff view in their Inbox — not only the second editor's 409 dialog.
            let diff = body["diff"]
                .as_array()
                .expect("diff array persisted in notification body");
            assert!(
                !diff.is_empty(),
                "conflict notification body must carry the line diff for the Inbox diff view"
            );
            assert!(
                diff.iter().any(|d| d["op"] == "add"),
                "submitted lines should appear as add ops in the persisted diff"
            );
        }
    }

    // ── POST /workspaces/:id/meta-specs/assist (§9 State 1 chat) ──────────

    /// App with a fixed-response mock LLM, returning the state so tests can
    /// seed meta-specs directly.
    fn app_with_llm_response_and_state(
        response: &str,
    ) -> (Router, std::sync::Arc<crate::AppState>) {
        let mut s = (*crate::mem::test_state()).clone();
        s.llm = Some(Arc::new(gyre_adapters::MockLlmPortFactory {
            inner: Arc::new(gyre_adapters::MockLlmAdapter::new(response)),
        }));
        let state = Arc::new(s);
        (crate::api::api_router().with_state(state.clone()), state)
    }

    async fn create_workspace(app: &Router, slug: &str) -> String {
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/workspaces")
                    .header("Authorization", auth())
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_vec(&serde_json::json!({
                            "name": slug,
                            "slug": slug,
                        }))
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let json = body_json(resp).await;
        json["id"].as_str().unwrap().to_string()
    }

    async fn seed_meta_spec(
        state: &std::sync::Arc<crate::AppState>,
        id: &str,
        name: &str,
        scope: gyre_domain::MetaSpecScope,
        scope_id: Option<String>,
        prompt: &str,
    ) {
        let ms = gyre_domain::MetaSpec {
            id: Id::new(id),
            kind: gyre_domain::MetaSpecKind::Persona,
            name: name.to_string(),
            scope,
            scope_id,
            prompt: prompt.to_string(),
            version: 1,
            content_hash: "h1".to_string(),
            required: false,
            approval_status: gyre_domain::MetaSpecApprovalStatus::Approved,
            approved_by: None,
            approved_content_hash: None,
            approved_at: None,
            created_by: "system".to_string(),
            created_at: 1000,
            updated_at: 1000,
        };
        state.meta_specs.create(&ms).await.expect("seed meta-spec");
    }

    async fn post_meta_spec_assist(
        app: &Router,
        workspace_id: &str,
        body: &serde_json::Value,
    ) -> axum::response::Response {
        app.clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/workspaces/{workspace_id}/meta-specs/assist"))
                    .header("Authorization", auth())
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn assist_meta_spec_unknown_workspace_returns_404() {
        let app = app();
        let resp = post_meta_spec_assist(
            &app,
            "no-such-ws",
            &serde_json::json!({
                "instruction": "Make error handling more defensive",
                "draft_content": "You are a backend developer…",
            }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn assist_meta_spec_requires_draft_or_meta_spec_id() {
        let app = app();
        let ws = create_workspace(&app, "ms-assist-req").await;
        let resp = post_meta_spec_assist(
            &app,
            &ws,
            &serde_json::json!({ "instruction": "improve the prompt" }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn assist_meta_spec_unknown_meta_spec_returns_404() {
        let app = app();
        let ws = create_workspace(&app, "ms-assist-unknown-ms").await;
        let resp = post_meta_spec_assist(
            &app,
            &ws,
            &serde_json::json!({
                "instruction": "improve",
                "meta_spec_id": "no-such-ms",
            }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn assist_meta_spec_rejects_meta_spec_from_another_workspace() {
        let (app, state) =
            app_with_llm_response_and_state("{}");
        let ws_a = create_workspace(&app, "ms-assist-ws-a").await;
        let ws_b = create_workspace(&app, "ms-assist-ws-b").await;
        seed_meta_spec(
            &state,
            "ms-foreign",
            "foreign-persona",
            gyre_domain::MetaSpecScope::Workspace,
            Some(ws_a),
            "You are a backend developer.",
        )
        .await;
        let resp = post_meta_spec_assist(
            &app,
            &ws_b,
            &serde_json::json!({
                "instruction": "improve",
                "meta_spec_id": "ms-foreign",
            }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn assist_meta_spec_with_draft_streams_sse_complete() {
        let valid_response = serde_json::json!({
            "diff": [
                {"op": "add", "path": "## Error Handling", "content": "Handle all errors with Result<T, E>."}
            ],
            "explanation": "Added defensive error handling to the persona."
        });
        let (app, _state) =
            app_with_llm_response_and_state(&valid_response.to_string());
        let ws = create_workspace(&app, "ms-assist-draft").await;

        let resp = post_meta_spec_assist(
            &app,
            &ws,
            &serde_json::json!({
                "instruction": "Make error handling more defensive",
                "draft_content": "You are a backend developer…",
            }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        let ct = resp.headers().get("content-type").unwrap();
        assert!(ct.to_str().unwrap().contains("text/event-stream"));

        let body = body_text(resp).await;
        let events = parse_sse_events(&body);
        let partial_events: Vec<_> = events.iter().filter(|(t, _)| t == "partial").collect();
        let complete_events: Vec<_> = events.iter().filter(|(t, _)| t == "complete").collect();
        assert!(
            !partial_events.is_empty(),
            "should have partial events for explanation"
        );
        assert_eq!(complete_events.len(), 1, "exactly one complete event");
        let complete_data: serde_json::Value =
            serde_json::from_str(&complete_events[0].1).unwrap();
        let diff = complete_data["diff"].as_array().unwrap();
        assert_eq!(diff[0]["op"], "add");
        assert_eq!(
            complete_data["explanation"].as_str().unwrap(),
            "Added defensive error handling to the persona."
        );
    }

    /// Test LLM factory that records every `stream_complete` system prompt
    /// and returns a fixed valid `{diff, explanation}` response — lets tests
    /// assert WHAT the LLM was actually given (stored prompt vs draft), not
    /// just that the call succeeded.
    struct RecordingLlmFactory {
        prompts: Arc<std::sync::Mutex<Vec<String>>>,
        response: String,
    }

    struct RecordingLlmPort {
        prompts: Arc<std::sync::Mutex<Vec<String>>>,
        response: String,
    }

    #[async_trait::async_trait]
    impl gyre_ports::LlmPort for RecordingLlmPort {
        async fn complete(
            &self,
            _system_prompt: &str,
            _user_prompt: &str,
            _max_tokens: Option<u32>,
        ) -> anyhow::Result<String> {
            Ok(self.response.clone())
        }
        async fn predict_json(
            &self,
            _system_prompt: &str,
            _user_prompt: &str,
        ) -> anyhow::Result<serde_json::Value> {
            serde_json::from_str(&self.response)
                .map_err(|e| anyhow::anyhow!("invalid JSON: {e}"))
        }
        async fn stream_complete(
            &self,
            system_prompt: &str,
            _user_prompt: &str,
            _max_tokens: Option<u32>,
        ) -> anyhow::Result<
            std::pin::Pin<
                Box<dyn futures_util::Stream<Item = anyhow::Result<String>> + Send>,
            >,
        > {
            self.prompts.lock().unwrap().push(system_prompt.to_string());
            let text = self.response.clone();
            Ok(Box::pin(futures_util::stream::iter(vec![Ok(text)])))
        }
        async fn complete_with_tools(
            &self,
            _system_prompt: &str,
            _messages: &[gyre_ports::ConversationMessage],
            _tools: &[gyre_ports::ToolDefinition],
            _max_tokens: Option<u32>,
        ) -> anyhow::Result<gyre_ports::ToolCallingResponse> {
            Ok(gyre_ports::ToolCallingResponse {
                text: self.response.clone(),
                tool_calls: vec![],
                stop_reason: "end_turn".to_string(),
            })
        }
    }

    impl gyre_ports::LlmPortFactory for RecordingLlmFactory {
        fn for_model(&self, _model_name: &str) -> Arc<dyn gyre_ports::LlmPort> {
            Arc::new(RecordingLlmPort {
                prompts: self.prompts.clone(),
                response: self.response.clone(),
            })
        }
    }

    fn app_with_recording_llm(
        response: &str,
    ) -> (Router, std::sync::Arc<crate::AppState>, Arc<std::sync::Mutex<Vec<String>>>) {
        let mut s = (*crate::mem::test_state()).clone();
        let prompts = Arc::new(std::sync::Mutex::new(Vec::new()));
        s.llm = Some(Arc::new(RecordingLlmFactory {
            prompts: prompts.clone(),
            response: response.to_string(),
        }));
        let state = Arc::new(s);
        (
            crate::api::api_router().with_state(state.clone()),
            state,
            prompts,
        )
    }

    #[tokio::test]
    async fn assist_meta_spec_loads_published_prompt_when_no_draft() {
        // With only meta_spec_id, the LLM must receive the STORED registry
        // prompt — asserted from the recorded system prompt, so a regression
        // that drops the lookup (empty content) or swaps draft/published
        // precedence fails here.
        let valid_response = serde_json::json!({
            "diff": [
                {"op": "replace", "path": "## Overview", "content": "Tightened persona."}
            ],
            "explanation": "Tightened the persona."
        });
        let (app, state, prompts) =
            app_with_recording_llm(&valid_response.to_string());
        let ws = create_workspace(&app, "ms-assist-published").await;
        seed_meta_spec(
            &state,
            "ms-global-1",
            "backend-dev",
            gyre_domain::MetaSpecScope::Global,
            None,
            "You are a backend developer who writes defensive code.",
        )
        .await;

        let resp = post_meta_spec_assist(
            &app,
            &ws,
            &serde_json::json!({
                "instruction": "Tighten the persona",
                "meta_spec_id": "ms-global-1",
            }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_text(resp).await;
        let events = parse_sse_events(&body);
        assert!(
            events.iter().any(|(t, _)| t == "complete"),
            "stored-prompt assist should stream a complete event"
        );

        let prompts = prompts.lock().unwrap();
        assert_eq!(prompts.len(), 1, "exactly one LLM call");
        let sys = &prompts[0];
        assert!(
            sys.contains("You are a backend developer who writes defensive code."),
            "system prompt must carry the stored registry prompt"
        );
        assert!(
            sys.contains("backend-dev"),
            "system prompt must name the meta-spec"
        );
        assert!(
            sys.contains("meta:persona"),
            "system prompt must carry the meta-spec kind"
        );
    }

    #[tokio::test]
    async fn assist_meta_spec_draft_overrides_published_prompt() {
        // draft_content must take precedence over the stored prompt — the
        // human is editing a draft in the §9 State 1 editor.
        let valid_response = serde_json::json!({
            "diff": [],
            "explanation": "no-op"
        });
        let (app, state, prompts) =
            app_with_recording_llm(&valid_response.to_string());
        let ws = create_workspace(&app, "ms-assist-draft-override").await;
        seed_meta_spec(
            &state,
            "ms-global-2",
            "backend-dev",
            gyre_domain::MetaSpecScope::Global,
            None,
            "STORED PROMPT MARKER",
        )
        .await;

        let resp = post_meta_spec_assist(
            &app,
            &ws,
            &serde_json::json!({
                "instruction": "improve",
                "meta_spec_id": "ms-global-2",
                "draft_content": "DRAFT PROMPT MARKER",
            }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        let prompts = prompts.lock().unwrap();
        assert_eq!(prompts.len(), 1);
        assert!(
            prompts[0].contains("DRAFT PROMPT MARKER"),
            "draft_content must be the effective content"
        );
        assert!(
            !prompts[0].contains("STORED PROMPT MARKER"),
            "stored prompt must not leak into the prompt when a draft is given"
        );
    }

    #[tokio::test]
    async fn assist_meta_spec_returns_503_when_llm_unavailable() {
        let app = app_no_llm();
        let ws = create_workspace(&app, "ms-assist-503").await;
        let resp = post_meta_spec_assist(
            &app,
            &ws,
            &serde_json::json!({
                "instruction": "improve",
                "draft_content": "You are a backend developer.",
            }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    }
}
