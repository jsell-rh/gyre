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
/// `pub(crate)` so the SLSA provenance endpoint (provenance.rs) validates its
/// `?ref=` parameter with the same rules.
pub(crate) fn is_valid_ref(s: &str) -> bool {
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
    /// Output format: omitted (default) → the working summary shape;
    /// `release` → the structured release artifact (supply-chain.md
    /// §AIBOM Contents) for packaging alongside release artifacts.
    pub format: Option<String>,
    /// Release identifier recorded in the release artifact (e.g. `v1.2.3`).
    /// Only meaningful with `format=release`.
    pub release: Option<String>,
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

// ── Release artifact format (supply-chain.md §AIBOM Contents) ─────────────────

/// Per-agent stack attestation block in the release artifact.
#[derive(Serialize)]
pub struct ReleaseStackAttestation {
    /// The agent's registered stack fingerprint (Level 2+), or `null` when
    /// the agent never registered a stack.
    pub fingerprint: Option<String>,
    /// Gyre attestation level: 3 = managed runtime, 2 = CLI self-report,
    /// 1 = raw push (supply-chain.md §Attestation Levels).
    pub level: u8,
    /// True when the agent's registered fingerprint matches the repo's
    /// committed `gyre-stack.lock` fingerprint.
    pub verified: bool,
}

#[derive(Serialize)]
pub struct ReleaseAgent {
    pub agent_id: String,
    /// Commit SHAs attributed to this agent in the release range.
    pub commits: Vec<String>,
    pub stack_attestation: ReleaseStackAttestation,
    pub model: Option<String>,
    /// MCP servers as `name:version`, from the agent's registered stack.
    pub mcp_servers: Vec<String>,
    /// User who spawned the agent (from commit provenance).
    pub spawned_by: Option<String>,
    /// Task IDs touched by this agent's commits in range.
    pub task_ids: Vec<String>,
    /// Rekor entry UUIDs for the agent's signed commits (Fulcio-signed
    /// `jj squash` commits recorded via the commit-signature repository).
    pub sigstore_entries: Vec<String>,
    /// Conversation blob SHAs on record for this agent (the platform's
    /// prompt reference — supply-chain.md §AIBOM Contents: the AIBOM
    /// captures "which AI models, prompts, and tools produced each
    /// commit"). Scoped to the repo workspace's tenant via the
    /// conversation repository; empty when the workspace (and therefore
    /// the tenant scope) cannot be resolved.
    pub conversation_shas: Vec<String>,
}

/// Per-commit entry in the release artifact: agent attribution plus the
/// full attestation chain data for the commit (§7.3 / supply-chain.md
/// §AIBOM Contents) — the same chain block the summary format carries,
/// so a compliance reviewer can reconstruct and verify the chain offline.
#[derive(Serialize)]
pub struct ReleaseCommit {
    pub sha: String,
    /// Attributed agent (`null` for human commits).
    pub agent_id: Option<String>,
    pub task_id: Option<String>,
    pub timestamp: Option<u64>,
    /// Full attestation chain for this commit — `attestation_id`,
    /// `chain_depth`, and the chain nodes root-to-leaf. Absent when no
    /// attestation is on record for the commit.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chain_attestation: Option<serde_json::Value>,
}

#[derive(Serialize)]
pub struct ReleaseStackPolicy {
    /// Fingerprint pinned by the repo's committed `gyre-stack.lock` at the
    /// release ref (`null` when the repo carries no lockfile).
    pub lockfile_hash: Option<String>,
    /// True when every commit in the release range is agent-attributed and
    /// attested at Level 2+.
    pub all_commits_attested: bool,
    /// Repo minimum attestation level (`null` when no policy is set).
    pub minimum_level: Option<u8>,
    /// Stack drift / policy violation events on record for this repo
    /// (`stack_drift_detected` audit events, full context).
    pub policy_violations: Vec<serde_json::Value>,
}

#[derive(Serialize)]
pub struct ReleaseSummary {
    pub total_commits: usize,
    pub attested_commits: usize,
    pub unattested_commits: usize,
    pub models_used: Vec<String>,
    pub agents_involved: usize,
    /// Commits in the release range with no agent attribution.
    pub human_commits: usize,
}

#[derive(Serialize)]
pub struct AibomReleaseResponse {
    pub aibom_version: String,
    /// Release identifier (`?release=`), e.g. "v1.2.3".
    pub release: Option<String>,
    /// ISO-8601 generation timestamp.
    pub generated_at: String,
    pub repo_id: String,
    pub agents: Vec<ReleaseAgent>,
    /// Every commit in the release range (newest-first) with its
    /// attribution and chain attestation data.
    pub commits: Vec<ReleaseCommit>,
    pub stack_policy: ReleaseStackPolicy,
    pub summary: ReleaseSummary,
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
///   ?format=release     — structured release artifact (supply-chain.md
///                        §AIBOM Contents) for packaging with a release
///   ?release={id}       — release identifier recorded in the artifact
pub async fn get_aibom(
    State(state): State<Arc<AppState>>,
    Path(repo_id): Path<String>,
    Query(params): Query<AibomQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    // Validate output format before doing any work.
    match params.format.as_deref() {
        None | Some("summary") | Some("release") => {}
        Some(other) => {
            return Err(ApiError::InvalidInput(format!(
                "invalid 'format' parameter '{other}': expected 'release'"
            )))
        }
    }

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

    // Release artifact format (supply-chain.md §AIBOM Contents): a
    // structured document with full agent attribution, stack attestation,
    // stack policy compliance, and drift/violation history.
    if params.format.as_deref() == Some("release") {
        let release = build_release_aibom(&state, &repo, &params).await?;
        let value = serde_json::to_value(release).map_err(|e| ApiError::Internal(e.into()))?;
        return Ok(Json(value));
    }

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

    // Build per-agent summary (named struct — TASK-048 F1: positional
    // tuple carriers are a field-swap hazard).
    #[derive(Default)]
    struct AgentSummary {
        model: Option<String>,
        commit_count: usize,
        any_server_verified: bool,
        any_self_reported: bool,
    }
    let mut agent_map: HashMap<String, AgentSummary> = HashMap::new();

    for ac in &commits {
        let agent_id_str = ac.agent_id.to_string();
        let entry = agent_map.entry(agent_id_str).or_default();
        entry.commit_count += 1;
        let lvl =
            resolve_attestation_level(ac.attestation_level.as_deref(), ac.model_context.as_deref());
        if lvl == "server-verified" {
            entry.any_server_verified = true;
        } else if lvl == "self-reported" {
            entry.any_self_reported = true;
        }
        // Prefer the model from the first commit that has it.
        if entry.model.is_none() {
            if let Some(ctx_str) = &ac.model_context {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(ctx_str) {
                    if let Some(model) = v.get("model").and_then(|m| m.as_str()) {
                        entry.model = Some(model.to_string());
                    }
                }
            }
        }
    }

    // Resolve agent names via the agent repository.
    let mut aibom_agents: Vec<AibomAgent> = Vec::new();
    for (agent_id, summary) in &agent_map {
        let name = match state.agents.find_by_id(&Id::new(agent_id)).await {
            Ok(Some(a)) => a.name,
            _ => agent_id.clone(),
        };
        let level = if summary.any_server_verified {
            "server-verified"
        } else if summary.any_self_reported {
            "self-reported"
        } else {
            "unattested"
        };
        aibom_agents.push(AibomAgent {
            id: agent_id.clone(),
            name,
            model: summary.model.clone(),
            commit_count: summary.commit_count,
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

    let response = AibomResponse {
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
    };
    let value = serde_json::to_value(response).map_err(|e| ApiError::Internal(e.into()))?;
    Ok(Json(value))
}

// ── Release artifact builder (supply-chain.md §AIBOM Contents) ───────────────

/// Run a git child, retrying OS-level execution failures. Returns the
/// captured output when git ran to a clean exit — success OR a nonzero
/// exit code, because a clean nonzero exit is git's *answer* (e.g. "unknown
/// revision"), not an execution failure. A child that could not be spawned
/// or was killed by a signal (fork failure, OOM killer) is transient: it is
/// retried a bounded number of times, and a persistent execution failure is
/// reported as an error so callers do not mistake "git did not run" for
/// "git ran and said no" — on the release-artifact and SLSA endpoints that
/// conflation turns an infrastructure hiccup into a bogus "ref not found".
async fn run_git_child(git_bin: &str, args: &[&str]) -> Result<std::process::Output, ApiError> {
    const GIT_EXEC_ATTEMPTS: usize = 3;
    let mut last_err = String::new();
    for attempt in 1..=GIT_EXEC_ATTEMPTS {
        match tokio::process::Command::new(git_bin)
            .args(args)
            .output()
            .await
        {
            // A clean exit (any code) is git's verdict — return it to the
            // caller to interpret.
            Ok(out) if out.status.success() || out.status.code().is_some() => return Ok(out),
            // No exit code → the child died to a signal.
            Ok(out) => {
                last_err = format!("git terminated by a signal: {}", out.status);
            }
            Err(e) => {
                last_err = format!("failed to execute git: {e}");
            }
        }
        if attempt < GIT_EXEC_ATTEMPTS {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    }
    Err(ApiError::Internal(anyhow::anyhow!(
        "{last_err} (after {GIT_EXEC_ATTEMPTS} attempts)"
    )))
}

/// Resolve a ref to a commit SHA (`git rev-parse --verify --end-of-options`).
/// `Ok(Some(sha))` — the ref resolves. `Ok(None)` — git ran and the ref does
/// not resolve (the caller's not-found case). `Err` — git could not be
/// executed at all (spawn failure / signal death after retries): an
/// internal error, not a missing ref.
pub(crate) async fn git_rev_parse(
    git_bin: &str,
    repo_path: &str,
    r: &str,
) -> Result<Option<String>, ApiError> {
    let out = run_git_child(
        git_bin,
        &[
            "-C",
            repo_path,
            "rev-parse",
            "--verify",
            "--end-of-options",
            r,
        ],
    )
    .await?;
    if out.status.success() {
        Ok(Some(
            String::from_utf8_lossy(&out.stdout).trim().to_string(),
        ))
    } else {
        tracing::warn!(
            repo = repo_path,
            requested_ref = r,
            stderr = %String::from_utf8_lossy(&out.stderr),
            "git rev-parse rejected ref"
        );
        Ok(None)
    }
}

/// List the commit SHAs in a rev range (`git rev-list`), newest-first,
/// de-duplicated. `pub(crate)` so the SLSA provenance endpoint
/// (provenance.rs) can scope its commit set to the subject ref's history.
/// Like [`git_rev_parse`]: `Ok(None)` means git ran and rejected the range;
/// `Err` means git could not be executed (internal error).
///
/// The range is a single argv element and cannot occupy an option
/// position: callers pass either a SHA resolved by [`git_rev_parse`]
/// (hex) or a `{from}..{sha}` range whose `from` was validated by
/// [`is_valid_ref`] (rejects a leading dash and any `--`), so no
/// `--` end-of-options separator is needed before it — placing one
/// there makes git treat the range as a pathspec and fail with usage
/// error 129 for every input.
pub(crate) async fn git_rev_list(
    git_bin: &str,
    repo_path: &str,
    range: &str,
) -> Result<Option<Vec<String>>, ApiError> {
    let out = run_git_child(git_bin, &["-C", repo_path, "rev-list", range]).await?;
    if !out.status.success() {
        tracing::warn!(
            repo = repo_path,
            range = range,
            stderr = %String::from_utf8_lossy(&out.stderr),
            "git rev-list rejected range"
        );
        return Ok(None);
    }
    let shas = String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    Ok(Some(shas))
}

/// Build the structured AIBOM release artifact (supply-chain.md §AIBOM
/// Contents) from real state: the git commit set of the release range,
/// per-commit agent attribution, registered agent stacks, the repo's
/// committed `gyre-stack.lock`, the repo stack policy, Rekor entries for
/// signed commits, and the stack-drift audit history.
///
/// Unlike the summary format (which bounds `from`/`to` by commit timestamp),
/// the release artifact interprets `from`/`to` as a git range: `from..to`
/// when both are given, else every commit reachable from `to` (defaulting
/// to the repo's default branch) — a release artifact must enumerate the
/// exact commit set it attests to, including human commits.
async fn build_release_aibom(
    state: &Arc<AppState>,
    repo: &gyre_domain::Repository,
    params: &AibomQuery,
) -> Result<AibomReleaseResponse, ApiError> {
    let git_bin = std::env::var("GYRE_GIT_PATH").unwrap_or_else(|_| "git".to_string());
    let repo_id = repo.id.to_string();

    // Resolve the release ref: `to` when given, else the default branch.
    let to_ref = params
        .to
        .clone()
        .unwrap_or_else(|| format!("refs/heads/{}", repo.default_branch));
    let to_sha = git_rev_parse(&git_bin, &repo.path, &to_ref)
        .await?
        .ok_or_else(|| {
            ApiError::NotFound(format!("cannot resolve ref '{to_ref}' in repo {repo_id}"))
        })?;

    // The release commit set.
    let range = match &params.from {
        Some(from) => format!("{from}..{to_sha}"),
        None => to_sha.clone(),
    };
    let release_shas = git_rev_list(&git_bin, &repo.path, &range)
        .await?
        .ok_or_else(|| {
            ApiError::NotFound(format!(
                "cannot enumerate commits for range '{range}' in repo {repo_id}"
            ))
        })?;
    let sha_set: std::collections::HashSet<String> = release_shas.iter().cloned().collect();

    // Agent attribution for commits in the release set. Multiple records
    // per SHA (re-pushes) collapse to the first.
    let all_commits = state
        .agent_commits
        .find_by_repo(&repo.id)
        .await
        .map_err(ApiError::Internal)?;
    let mut attributed: Vec<&gyre_domain::AgentCommit> = Vec::new();
    let mut attributed_by_sha: std::collections::HashMap<String, &gyre_domain::AgentCommit> =
        std::collections::HashMap::new();
    let mut seen_shas: std::collections::HashSet<String> = std::collections::HashSet::new();
    for ac in &all_commits {
        if sha_set.contains(&ac.commit_sha) && seen_shas.insert(ac.commit_sha.clone()) {
            attributed_by_sha.insert(ac.commit_sha.clone(), ac);
            attributed.push(ac);
        }
    }

    // Tenant scope for conversation provenance. The repo's workspace
    // carries the tenant; when it cannot be resolved the scope cannot be
    // determined, so conversation SHAs are omitted and the skip is logged
    // — never fabricated with a default tenant (which would re-target the
    // lookup and could leak another tenant's conversations).
    let tenant_id = state
        .workspaces
        .find_by_id(&repo.workspace_id)
        .await
        .map_err(ApiError::Internal)?
        .map(|ws| ws.tenant_id);
    if tenant_id.is_none() {
        tracing::warn!(
            repo = %repo_id,
            workspace = %repo.workspace_id,
            "release AIBOM: workspace not found; conversation SHAs omitted (tenant scope cannot be determined)"
        );
    }

    // The repo's committed lockfile pins the expected stack.
    let lockfile_hash =
        crate::spec_registry::read_git_file(&git_bin, &repo.path, &to_sha, "gyre-stack.lock")
            .await
            .and_then(|content| gyre_domain::stack::StackLockfile::parse(&content).ok())
            .map(|lock| lock.fingerprint);

    // Repo stack policy (minimum level).
    let policy = state
        .kv_store
        .kv_get("repo_stack_policies", &repo_id)
        .await
        .map_err(ApiError::Internal)?
        .map(|v| super::stack_attest::parse_stack_policy(&v));

    // Group attributed commits by agent.
    let mut by_agent: std::collections::HashMap<String, Vec<&gyre_domain::AgentCommit>> =
        std::collections::HashMap::new();
    for ac in &attributed {
        by_agent
            .entry(ac.agent_id.to_string())
            .or_default()
            .push(ac);
    }

    let mut release_agents: Vec<ReleaseAgent> = Vec::with_capacity(by_agent.len());
    let mut models_used: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut attested_commits = 0usize;
    let mut unattested_commits = 0usize;

    for (agent_id, commits) in &by_agent {
        // Registered stack (Level 2+): fingerprint, model, MCP servers.
        let registered_stack = state
            .kv_store
            .kv_get("agent_stacks", agent_id)
            .await
            .ok()
            .flatten()
            .and_then(|s| serde_json::from_str::<super::stack_attest::AgentStack>(&s).ok());

        // Level: the stored per-agent level (resolved at push time) when
        // present, else the strongest level across the agent's commits.
        let stored_level = state
            .agents
            .find_by_id(&Id::new(agent_id))
            .await
            .ok()
            .flatten()
            .and_then(|a| a.attestation_level);
        let level = stored_level.unwrap_or_else(|| {
            commits
                .iter()
                .map(|ac| {
                    gyre_domain::stack::attestation_level_from_label(
                        ac.attestation_level.as_deref(),
                    )
                })
                .max()
                .unwrap_or(1)
        });
        for ac in commits {
            if gyre_domain::stack::attestation_level_from_label(ac.attestation_level.as_deref())
                >= 2
            {
                attested_commits += 1;
            } else {
                unattested_commits += 1;
            }
        }

        let fingerprint = registered_stack.as_ref().map(|s| s.fingerprint());
        let verified = match (&fingerprint, &lockfile_hash) {
            (Some(fp), Some(lock)) => fp == lock,
            _ => false,
        };

        // Model: registered stack first, else commit model context.
        let model = registered_stack
            .as_ref()
            .map(|s| s.model.clone())
            .or_else(|| {
                commits.iter().find_map(|ac| {
                    ac.model_context
                        .as_deref()
                        .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
                        .and_then(|v| v.get("model").and_then(|m| m.as_str()).map(str::to_string))
                })
            });
        if let Some(m) = &model {
            models_used.insert(m.clone());
        }

        let mcp_servers = registered_stack
            .as_ref()
            .map(|s| {
                s.mcp_servers
                    .iter()
                    .map(|m| format!("{}:{}", m.name, m.version))
                    .collect()
            })
            .unwrap_or_default();

        let spawned_by = commits.iter().find_map(|ac| ac.spawned_by_user_id.clone());

        let mut task_ids: Vec<String> =
            commits.iter().filter_map(|ac| ac.task_id.clone()).collect();
        task_ids.sort();
        task_ids.dedup();

        // Rekor entries for this agent's signed commits.
        let mut sigstore_entries = Vec::new();
        for ac in commits {
            if let Ok(Some(sig)) = state.commit_signatures.find(&repo_id, &ac.commit_sha).await {
                if let Some(uuid) = sig.rekor_entry_id {
                    sigstore_entries.push(uuid);
                }
            }
        }
        sigstore_entries.sort();
        sigstore_entries.dedup();

        // Conversation SHAs on record for this agent (prompt provenance,
        // §AIBOM Contents). A lookup failure is surfaced, not swallowed:
        // silently dropping provenance from a compliance artifact would
        // understate the AI involvement in the release.
        let conversation_shas = match &tenant_id {
            Some(t) => state
                .conversations
                .list_by_agent(&Id::new(agent_id), t)
                .await
                .map_err(ApiError::Internal)?,
            None => Vec::new(),
        };

        release_agents.push(ReleaseAgent {
            agent_id: agent_id.clone(),
            commits: commits.iter().map(|ac| ac.commit_sha.clone()).collect(),
            stack_attestation: ReleaseStackAttestation {
                fingerprint,
                level,
                verified,
            },
            model,
            mcp_servers,
            spawned_by,
            task_ids,
            sigstore_entries,
            conversation_shas,
        });
    }
    // Most prolific contributor first for a stable, reviewable artifact.
    release_agents.sort_by(|a, b| {
        b.commits
            .len()
            .cmp(&a.commits.len())
            .then(a.agent_id.cmp(&b.agent_id))
    });

    // Per-commit chain attestation data (§7.3 / §AIBOM Contents): every
    // commit in the release range — agent-attributed and human alike —
    // with the full attestation chain where one is on record, so the
    // artifact supports offline chain verification. Lookup failures are
    // surfaced as errors: a compliance artifact must not silently omit
    // attestation data because storage errored.
    let mut release_commits: Vec<ReleaseCommit> = Vec::with_capacity(release_shas.len());
    for sha in &release_shas {
        let ac = attributed_by_sha.get(sha.as_str()).copied();
        let chain_attestation = match state.chain_attestations.find_by_commit(sha).await {
            Ok(Some(att)) => {
                let chain = state
                    .chain_attestations
                    .load_chain(&att.id)
                    .await
                    .map_err(ApiError::Internal)?;
                if chain.is_empty() {
                    None
                } else {
                    Some(serde_json::json!({
                        "attestation_id": att.id,
                        "chain_depth": att.metadata.chain_depth,
                        "chain": chain,
                    }))
                }
            }
            Ok(None) => None,
            Err(e) => return Err(ApiError::Internal(e)),
        };
        release_commits.push(ReleaseCommit {
            sha: sha.clone(),
            agent_id: ac.map(|a| a.agent_id.to_string()),
            task_id: ac.and_then(|a| a.task_id.clone()),
            timestamp: ac.map(|a| a.timestamp),
            chain_attestation,
        });
    }

    // Stack policy compliance + drift history.
    let drift_events = super::stack_attest::stack_drift_events_for_repo(state, repo).await?;
    let policy_violations = drift_events
        .iter()
        .filter_map(|e| serde_json::to_value(e).ok())
        .collect::<Vec<_>>();

    let total_commits = release_shas.len();
    let human_commits = total_commits.saturating_sub(
        attributed
            .iter()
            .map(|ac| ac.commit_sha.clone())
            .collect::<std::collections::HashSet<_>>()
            .len(),
    );
    let all_commits_attested = human_commits == 0 && unattested_commits == 0 && total_commits > 0;

    Ok(AibomReleaseResponse {
        aibom_version: "1.0".to_string(),
        release: params.release.clone(),
        generated_at: gyre_domain::decisions::format_iso8601_utc(super::now_secs()),
        repo_id,
        agents: release_agents,
        commits: release_commits,
        stack_policy: ReleaseStackPolicy {
            lockfile_hash,
            all_commits_attested,
            minimum_level: policy.as_ref().map(|p| p.min_attestation_level),
            policy_violations,
        },
        summary: ReleaseSummary {
            total_commits,
            attested_commits,
            unattested_commits,
            models_used: models_used.into_iter().collect(),
            agents_involved: by_agent.len(),
            human_commits,
        },
    })
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

    /// Assert a 200 and return the parsed body. On any other status the
    /// assertion message carries the response body, so a future failure
    /// names the exact server error (which not-found path fired, or the
    /// internal git-execution error) instead of a bare status code —
    /// verification round 2 lost exactly that distinction on
    /// `aibom_release_format_full_attribution` (404 with no body retained).
    async fn expect_ok(resp: axum::response::Response) -> serde_json::Value {
        let status = resp.status();
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let text = String::from_utf8_lossy(&bytes).to_string();
        assert_eq!(status, StatusCode::OK, "request failed, body: {text}");
        serde_json::from_str(&text).unwrap()
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

    // ── Release artifact format (supply-chain.md §AIBOM Contents) ─────────

    use gyre_domain::stack::StackDriftEvent;
    use std::sync::Arc;

    /// `create_repo` with an explicit repo name (unique per call so parallel
    /// tests never share a git dir under the shared `repos_root`).
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
        let j = body_json(resp).await;
        let repo_id = j["id"].as_str().unwrap().to_string();
        (app, repo_id)
    }

    /// Run `git -C <repo_path> <args>` and return trimmed stdout; panics on
    /// failure.
    fn run_git(repo_path: &str, args: &[&str]) -> String {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(repo_path)
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
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
    /// failure so a bad tree input cannot surface as a bare
    /// `commit-tree failed` one step later.
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

    /// Create a commit in the bare repo from `{path: content}` files
    /// (nested paths supported), optionally on top of `parent`, returning
    /// the new SHA (refs untouched).
    fn commit_files(repo_path: &str, parent: Option<&str>, files: &[(&str, String)]) -> String {
        let blobs: Vec<(&str, String)> = files
            .iter()
            .map(|(path, content)| (*path, write_blob(repo_path, content)))
            .collect();
        let tree = write_tree(repo_path, &blobs);

        let mut cmd = std::process::Command::new("git");
        cmd.arg("-C").arg(repo_path).args([
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "commit-tree",
            &tree,
            "-m",
            "test commit",
        ]);
        if let Some(p) = parent {
            cmd.args(["-p", p]);
        }
        let out = cmd.output().unwrap();
        assert!(
            out.status.success(),
            "commit-tree failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    /// Real-git setup for release-artifact tests: an API-created repo (record
    /// via the API; `test_state` wires NoopGitOps so the bare git dir +
    /// initial commit are created here), an agent with a registered stack, a
    /// committed gyre-stack.lock pinning that stack, and two agent-attributed
    /// commits on main. The repo name is unique per call — parallel tests in
    /// this binary share `repos_root` (keyed by pid), so same-named repos
    /// would collide on the same git dir.
    async fn release_repo_with_commits() -> (
        Router,
        Arc<crate::AppState>,
        String,
        String,
        String,
        String,
        String,
    ) {
        let state = test_state();
        let app = crate::api::api_router().with_state(state.clone());
        let repo_name = format!("rel-{}", uuid::Uuid::new_v4().simple());
        let (app, repo_id) = create_repo_named(app, &repo_name).await;
        let repo = state
            .repos
            .find_by_id(&Id::new(&repo_id))
            .await
            .unwrap()
            .expect("repo record");
        let repo_path = repo.path.clone();

        // Workspace record so the release artifact can resolve the repo's
        // tenant for conversation provenance (no fabricated scope).
        state
            .workspaces
            .create(&gyre_domain::Workspace::new(
                Id::new("ws-1"),
                Id::new("tenant-rel"),
                "Release WS",
                "ws-1",
                1_700_000_000,
            ))
            .await
            .unwrap();

        // Real bare repo + initial (human) commit on main.
        std::fs::create_dir_all(&repo_path).unwrap();
        let out = std::process::Command::new("git")
            .args(["init", "--bare", &repo_path])
            .output()
            .unwrap();
        assert!(out.status.success(), "git init --bare failed");
        let head = commit_files(&repo_path, None, &[]);

        // Agent's registered stack — the same stack the lockfile pins.
        let stack = gyre_domain::stack::AgentStack {
            agents_md_hash: "amd-hash".to_string(),
            hooks: vec![],
            mcp_servers: vec![gyre_domain::stack::McpServerEntry {
                name: "odis".to_string(),
                version: "1.0".to_string(),
                config_hash: "cfg".to_string(),
            }],
            model: "claude-sonnet-4-6".to_string(),
            cli_version: "0.4.2".to_string(),
            settings_hash: "settings-hash".to_string(),
            persona_hash: None,
        };
        state
            .kv_store
            .kv_set(
                "agent_stacks",
                "agent-rel",
                serde_json::to_string(&stack).unwrap(),
            )
            .await
            .unwrap();
        let lock = gyre_domain::stack::StackLockfile::from_stack(&stack, 1_700_000_000);

        // main: initial (human) commit → lockfile commit → feature commit.
        // `commit_files` builds each tree from just the given files (no
        // parent-tree merge), so the lockfile must be carried into c2:
        // the release artifact reads `gyre-stack.lock` at the release tip
        // (`to`/default-branch HEAD = c2), and a tree without it would
        // report no pinned stack.
        let lock_toml = lock.to_toml().unwrap();
        let c1 = commit_files(
            &repo_path,
            Some(&head),
            &[("gyre-stack.lock", lock_toml.clone())],
        );
        let c2 = commit_files(
            &repo_path,
            Some(&c1),
            &[
                ("gyre-stack.lock", lock_toml),
                ("src/lib.rs", "pub fn f() {}".to_string()),
            ],
        );
        run_git(&repo_path, &["update-ref", "refs/heads/main", &c2]);

        // Agent attribution: both commits server-verified (level 3).
        for (i, sha) in [&c1, &c2].iter().enumerate() {
            let ac = AgentCommit::new(
                Id::new(format!("ac-{i}")),
                Id::new("agent-rel"),
                Id::new(&repo_id),
                (*sha).clone(),
                "refs/heads/main",
                1_700_000_000 + i as u64,
            )
            .with_provenance(
                Some(format!("TASK-{i}")),
                Some("user:jsell".to_string()),
                None,
                Some(r#"{"model":"claude-sonnet-4-6"}"#.to_string()),
            )
            .with_attestation_level("server-verified");
            state.agent_commits.record(&ac).await.unwrap();
        }

        // A Fulcio-signed commit → one Rekor entry to surface.
        let sig = gyre_ports::commit_signature_repo::CommitSignature {
            repo_id: repo_id.clone(),
            commit_sha: c2.clone(),
            signer_id: "agent-rel".to_string(),
            task_id: "TASK-1".to_string(),
            spawned_by: "user:jsell".to_string(),
            algorithm: gyre_ports::commit_signature_repo::ALGORITHM_ECDSA_P256.to_string(),
            signature: "c2ln".to_string(),
            signing_key_id: String::new(),
            signed_at: 1_700_000_000,
            sigstore_mode: gyre_ports::commit_signature_repo::SigstoreMode::Fulcio,
            oidc_subject: "agent-rel".to_string(),
            oidc_issuer: "https://gyre.example.com".to_string(),
            certificate_pem: None,
            certificate_chain_pem: None,
            rekor_entry_id: Some("rekor-entry-uuid-1".to_string()),
        };
        state.commit_signatures.save(&sig).await.unwrap();

        // A conversation blob for the agent (zstd-compressed per the
        // ConversationRepository port contract) → per-agent conversation
        // SHAs in the release artifact.
        let conv_payload = br#"{"turns":[{"role":"user","content":"implement the feature"}]}"#;
        let conv_bytes = zstd::encode_all(&conv_payload[..], 3).unwrap();
        let conv_sha = state
            .conversations
            .store(
                &Id::new("agent-rel"),
                &Id::new("ws-1"),
                &Id::new("tenant-rel"),
                &conv_bytes,
            )
            .await
            .unwrap();

        // An attestation chain node for c2 → per-commit chain
        // attestation data in the release artifact.
        let att = make_attestation_for_commit(&repo_id, &c2);
        state.chain_attestations.save(&att).await.unwrap();

        // Repo stack policy: minimum level 3.
        state
            .kv_store
            .kv_set(
                "repo_stack_policies",
                &repo_id,
                serde_json::to_string(&serde_json::json!({
                    "fingerprint": lock.fingerprint.clone(),
                    "required_level": 3,
                    "min_attestation_level": 3,
                    "enforcement": "block"
                }))
                .unwrap(),
            )
            .await
            .unwrap();

        // One drift event on the audit trail → policy_violations.
        let drift = StackDriftEvent {
            id: "drift-1".to_string(),
            repo_id: repo_id.clone(),
            workspace_id: repo.workspace_id.to_string(),
            agent_id: "agent-rel".to_string(),
            branch: "main".to_string(),
            lockfile_fingerprint: Some(lock.fingerprint.clone()),
            attested_stack_fingerprint: "different".to_string(),
            integrity_verified: true,
            reason: "stack drift: model differs".to_string(),
            mode: "warn".to_string(),
            outcome: "accepted".to_string(),
            override_reason: Some("approved".to_string()),
            timestamp: 1_700_000_050,
        };
        let event = gyre_domain::AuditEvent::new(
            Id::new("audit-drift-1"),
            gyre_domain::AuditEventType::StackDriftDetected,
            Some(Id::new("agent-rel")),
            None,
            None,
            Some(repo.workspace_id.clone()),
            Some(Id::new(&repo_id)),
            "repo".to_string(),
            Some(repo_id.clone()),
            gyre_domain::AuditOutcome::Success,
            serde_json::to_value(&drift).unwrap(),
            None,
            None,
            1_700_000_050,
        );
        state.audit.record(&event).await.unwrap();

        (app, state, repo_id, repo_path, c1, c2, conv_sha)
    }

    /// Minimal SignedInput-rooted attestation chain node for a commit —
    /// the same shape the constraint pipeline stores (chain_depth 0, root
    /// Signed input; `load_chain` then returns the single-node chain).
    fn make_attestation_for_commit(repo_id: &str, commit_sha: &str) -> gyre_common::Attestation {
        use gyre_common::{
            AttestationInput, AttestationMetadata, AttestationOutput, InputContent, KeyBinding,
            PersonaRef, ScopeConstraint, SignedInput,
        };
        let signed_input = SignedInput {
            content: InputContent {
                spec_path: "specs/system/payments.md".to_string(),
                spec_sha: "abc12345".to_string(),
                workspace_id: "ws-1".to_string(),
                repo_id: repo_id.to_string(),
                persona_constraints: vec![PersonaRef {
                    name: "security".to_string(),
                }],
                meta_spec_set_sha: "def456".to_string(),
                scope: ScopeConstraint {
                    allowed_paths: vec!["src/**".to_string()],
                    forbidden_paths: vec![],
                },
            },
            output_constraints: vec![],
            valid_until: u64::MAX,
            expected_generation: None,
            signature: vec![30],
            key_binding: KeyBinding {
                public_key: vec![1; 32],
                user_identity: "user:jsell".to_string(),
                issuer: "https://keycloak.example.com".to_string(),
                trust_anchor_id: "tenant-keycloak".to_string(),
                issued_at: 1_700_000_000,
                expires_at: u64::MAX,
                user_signature: vec![10],
                platform_countersign: vec![20],
            },
        };
        gyre_common::Attestation {
            id: "att-rel-c2".to_string(),
            input: AttestationInput::Signed(signed_input),
            output: AttestationOutput {
                content_hash: vec![40],
                commit_sha: commit_sha.to_string(),
                agent_signature: None,
                gate_results: vec![],
            },
            metadata: AttestationMetadata {
                created_at: 1_700_000_100,
                workspace_id: "ws-1".to_string(),
                repo_id: repo_id.to_string(),
                task_id: "TASK-1".to_string(),
                agent_id: "agent-rel".to_string(),
                chain_depth: 0,
            },
        }
    }

    #[tokio::test]
    async fn aibom_release_format_full_attribution() {
        let (app, state, _repo_path, repo_id, c1, c2, conv_sha) = release_repo_with_commits().await;

        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/api/v1/repos/{repo_id}/aibom?format=release&release=v1.2.3"
                    ))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let json = expect_ok(resp).await;

        // Artifact envelope (supply-chain.md §AIBOM Contents).
        assert_eq!(json["aibom_version"], "1.0");
        assert_eq!(json["release"], "v1.2.3");
        assert!(json["generated_at"].as_str().unwrap().contains('T'));
        assert_eq!(json["repo_id"], repo_id);

        // Full agent attribution: commits, stack attestation (fingerprint,
        // level, verified against the committed lockfile), model, MCP
        // servers, spawned_by, task_ids, sigstore entries.
        let agents = json["agents"].as_array().unwrap();
        assert_eq!(agents.len(), 1);
        let agent = &agents[0];
        assert_eq!(agent["agent_id"], "agent-rel");
        let commits = agent["commits"].as_array().unwrap();
        assert!(commits.contains(&serde_json::json!(c1)));
        assert!(commits.contains(&serde_json::json!(c2)));
        assert_eq!(agent["stack_attestation"]["level"], 3);
        assert_eq!(agent["stack_attestation"]["verified"], true);
        let fingerprint = agent["stack_attestation"]["fingerprint"]
            .as_str()
            .unwrap()
            .to_string();
        assert_eq!(fingerprint.len(), 64, "SHA-256 hex fingerprint");
        assert_eq!(agent["model"], "claude-sonnet-4-6");
        assert_eq!(agent["mcp_servers"], serde_json::json!(["odis:1.0"]));
        assert_eq!(agent["spawned_by"], "user:jsell");
        let task_ids = agent["task_ids"].as_array().unwrap();
        assert!(task_ids.contains(&serde_json::json!("TASK-0")));
        assert!(task_ids.contains(&serde_json::json!("TASK-1")));
        assert_eq!(
            agent["sigstore_entries"],
            serde_json::json!(["rekor-entry-uuid-1"])
        );
        // Conversation SHAs: the agent's prompt provenance (§AIBOM
        // Contents — "which AI models, prompts, and tools produced each
        // commit"), scoped via the repo workspace's tenant.
        assert_eq!(
            agent["conversation_shas"],
            serde_json::json!([conv_sha]),
            "per-agent conversation SHAs must be in the release artifact"
        );

        // Per-commit chain attestation data: every commit in the release
        // range, with the full chain where one is on record. c2 carries
        // the attestation stored by the helper; c1 and the human commit
        // have none (field absent).
        let commits = json["commits"].as_array().unwrap();
        assert_eq!(commits.len(), 3, "initial + c1 + c2");
        let c2_entry = commits
            .iter()
            .find(|c| c["sha"] == serde_json::json!(c2))
            .unwrap();
        assert_eq!(c2_entry["agent_id"], "agent-rel");
        assert_eq!(c2_entry["task_id"], "TASK-1");
        assert_eq!(
            c2_entry["chain_attestation"]["attestation_id"],
            "att-rel-c2"
        );
        assert_eq!(c2_entry["chain_attestation"]["chain_depth"], 0);
        let chain_nodes = c2_entry["chain_attestation"]["chain"].as_array().unwrap();
        assert!(!chain_nodes.is_empty(), "chain nodes must be present");
        let c1_entry = commits
            .iter()
            .find(|c| c["sha"] == serde_json::json!(c1))
            .unwrap();
        assert_eq!(c1_entry["agent_id"], "agent-rel");
        assert!(
            c1_entry.get("chain_attestation").is_none(),
            "c1 has no attestation on record — field must be absent"
        );
        let human_entries: Vec<_> = commits.iter().filter(|c| c["agent_id"].is_null()).collect();
        assert_eq!(human_entries.len(), 1, "the initial commit is human");
        assert!(human_entries[0].get("chain_attestation").is_none());

        // Stack policy block: lockfile hash, attestation coverage, minimum
        // level, drift history.
        let sp = &json["stack_policy"];
        assert_eq!(sp["lockfile_hash"].as_str().unwrap(), fingerprint);
        // The initial (human) commit on main is unattributed → not all
        // commits attested.
        assert_eq!(sp["all_commits_attested"], false);
        assert_eq!(sp["minimum_level"], 3);
        let violations = sp["policy_violations"].as_array().unwrap();
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0]["outcome"], "accepted");
        assert_eq!(violations[0]["override_reason"], "approved");

        // Summary: 3 commits on main (initial + c1 + c2), 2 agent-attributed
        // and attested, 1 human.
        let summary = &json["summary"];
        assert_eq!(summary["total_commits"], 3);
        assert_eq!(summary["attested_commits"], 2);
        assert_eq!(summary["unattested_commits"], 0);
        assert_eq!(summary["human_commits"], 1);
        assert_eq!(summary["agents_involved"], 1);
        assert_eq!(
            summary["models_used"],
            serde_json::json!(["claude-sonnet-4-6"])
        );

        // The drift event really came from the audit store.
        let filter = gyre_ports::AuditQueryFilter {
            event_type: Some("stack_drift_detected".to_string()),
            limit: 10,
            ..Default::default()
        };
        assert_eq!(state.audit.query(&filter).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn aibom_release_format_range_and_unknown_format() {
        let (app, _state, repo_id, _repo_path, c1, _c2, _conv_sha) =
            release_repo_with_commits().await;

        // `from` bounds the release range: only commits after c1 (i.e. c2
        // + nothing else) are in the artifact.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/api/v1/repos/{repo_id}/aibom?format=release&from={c1}"
                    ))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let json = expect_ok(resp).await;
        assert_eq!(json["summary"]["total_commits"], 1);
        assert_eq!(json["summary"]["attested_commits"], 1);
        assert_eq!(json["summary"]["human_commits"], 0);
        assert_eq!(json["stack_policy"]["all_commits_attested"], true);

        // Unknown format → 400.
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/repos/{repo_id}/aibom?format=xml"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn aibom_release_format_unresolvable_ref() {
        let state = test_state();
        let app = crate::api::api_router().with_state(state.clone());
        let (app, repo_id) = create_repo(app).await;

        // A ref that does not exist in the repo → 404, not an empty artifact.
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/api/v1/repos/{repo_id}/aibom?format=release&to=refs/tags/nope"
                    ))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }
}
