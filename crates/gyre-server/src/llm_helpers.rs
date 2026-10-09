use std::collections::BTreeMap;

use gyre_common::graph::NodeType;
use gyre_common::Id;

use crate::AppState;

/// Default LLM model used when no config is set and no env override is present.
pub const DEFAULT_LLM_MODEL: &str = "gemini-2.0-flash-001";

/// Default max output tokens for explorer-views/generate (ui-layout.md §2:
/// "Token Limits" — 2000).
pub const DEFAULT_MAX_TOKENS_GENERATE: u32 = 2000;

/// Default max output tokens for briefing/ask (ui-layout.md §2: 4000).
pub const DEFAULT_MAX_TOKENS_ASK: u32 = 4000;

/// Default max output tokens for specs/assist (ui-layout.md §2: 4000).
pub const DEFAULT_MAX_TOKENS_ASSIST: u32 = 4000;

/// Pure token-limit resolution for an LLM function key.
///
/// Order: per-function env override (`GYRE_LLM_MAX_TOKENS_GENERATE` /
/// `_ASK` / `_ASSIST`) → hardcoded default per ui-layout.md §2. Function
/// keys without a specced limit (e.g. graph-predict) resolve to `None`
/// (no cap).
///
/// Kept pure (env lookup injected) so tests never mutate process-global
/// environment state (env-mutating integration tests race under cargo
/// test's multithreaded runner).
pub fn max_tokens_for_function(function_key: &str, env_lookup: impl Fn(&str) -> Option<String>) -> Option<u32> {
    let (env_name, default) = match function_key {
        "explorer-generate" => ("GYRE_LLM_MAX_TOKENS_GENERATE", Some(DEFAULT_MAX_TOKENS_GENERATE)),
        "briefing-ask" => ("GYRE_LLM_MAX_TOKENS_ASK", Some(DEFAULT_MAX_TOKENS_ASK)),
        "specs-assist" => ("GYRE_LLM_MAX_TOKENS_ASSIST", Some(DEFAULT_MAX_TOKENS_ASSIST)),
        _ => return None,
    };
    match env_lookup(env_name).and_then(|v| v.parse::<u32>().ok()) {
        Some(n) => Some(n),
        None => default,
    }
}

/// Resolve the model name and max_tokens for an LLM function in a workspace.
///
/// Model resolution order:
///   1. LLM config override (workspace_id + function_key)
///   2. Workspace entity `llm_model` (set via PUT /workspaces/{id})
///   3. `GYRE_LLM_MODEL` environment variable
///   4. Hardcoded default ("gemini-2.0-flash-001")
///
/// Token limit resolution order:
///   1. LLM config override `max_tokens`
///   2. `GYRE_LLM_MAX_TOKENS_{GENERATE|ASK|ASSIST}` env → hardcoded default
///      (2000/4000/4000 per ui-layout.md §2); other function keys → None.
pub async fn resolve_llm_model(
    state: &AppState,
    workspace_id: &Id,
    function_key: &str,
) -> (String, Option<u32>) {
    if let Ok(Some(cfg)) = state
        .llm_configs
        .get_effective(workspace_id, function_key)
        .await
    {
        return (cfg.model_name, cfg.max_tokens);
    }
    let model = match state.workspaces.find_by_id(workspace_id).await {
        Ok(Some(ws)) if ws.llm_model.is_some() => ws.llm_model.unwrap_or_default(),
        _ => std::env::var("GYRE_LLM_MODEL").unwrap_or_else(|_| DEFAULT_LLM_MODEL.to_string()),
    };
    (model, max_tokens_for_function(function_key, std::env::var))
}

/// Resolved prompt template plus its provenance SHA (ui-layout.md §2:
/// "prompt template version (git SHA) recorded in cost entries").
///
/// `sha` is `Some(branch-head SHA)` only when the template was loaded from
/// the workspace's git tree; DB overrides and hardcoded defaults carry no
/// git provenance.
pub async fn resolve_prompt_template(
    state: &AppState,
    workspace_id: &Id,
    repo_hint: Option<&Id>,
    function_key: &str,
    hardcoded_fallback: &str,
) -> ResolvedPrompt {
    // 1. DB workspace/tenant override.
    if let Ok(Some(tpl)) = state
        .prompt_templates
        .get_effective(workspace_id, function_key)
        .await
    {
        return ResolvedPrompt {
            content: tpl.content,
            sha: None,
        };
    }

    // 2. Git tree: specs/prompts/<function_key>.md on the default branch.
    // Candidates: the request's own repo when known, else every repo in the
    // workspace (first whose default branch carries the file wins).
    let path = format!("specs/prompts/{function_key}.md");
    let candidates: Vec<gyre_domain::Repository> = match repo_hint {
        Some(rid) => state.repos.find_by_id(rid).await.ok().flatten().into_iter().collect(),
        None => state
            .repos
            .list_by_workspace(workspace_id)
            .await
            .unwrap_or_default(),
    };
    for repo in candidates {
        if let Ok(Some(bytes)) = state
            .git_ops
            .read_file(&repo.path, &repo.default_branch, &path)
            .await
        {
            if let Ok(content) = String::from_utf8(bytes) {
                // Provenance: the branch-head commit SHA at load time.
                let sha = state
                    .git_ops
                    .commit_log(&repo.path, &repo.default_branch, 1)
                    .await
                    .ok()
                    .and_then(|log| log.into_iter().next())
                    .map(|c| c.sha);
                return ResolvedPrompt { content, sha };
            }
        }
    }

    // 3. Hardcoded fallback.
    ResolvedPrompt {
        content: hardcoded_fallback.to_string(),
        sha: None,
    }
}

/// Compact workspace graph summary for LLM grounding (node types and counts,
/// per ui-layout.md §2 "Available Data": explorer-generate and briefing-ask
/// prompts must carry the workspace's node types/counts).
///
/// Format: one line per node type present, `"<type>: <count>"`, plus a total
/// line. Repo-scoped when `repo_id` is given (explorer with a repo filter).
pub async fn workspace_graph_summary(
    state: &AppState,
    workspace_id: &Id,
    repo_filter: Option<&Id>,
) -> String {
    let repos: Vec<String> = match repo_filter {
        Some(rid) => vec![rid.to_string()],
        None => state
            .repos
            .list_by_workspace(workspace_id)
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|r| r.id.to_string())
            .collect(),
    };

    let mut counts: BTreeMap<NodeType, usize> = BTreeMap::new();
    for rid in repos {
        let repo_id = Id::new(&rid);
        let nodes = state.graph_store.list_nodes(&repo_id, None).await;
        for node in nodes.unwrap_or_default() {
            *counts.entry(node.node_type).or_insert(0) += 1;
        }
    }

    if counts.is_empty() {
        return "No graph nodes indexed for this workspace yet.".to_string();
    }

    let mut lines = Vec::with_capacity(counts.len() + 1);
    let total: usize = counts.values().sum();
    for (ty, count) in &counts {
        lines.push(format!("{}: {}", serde_plain_name(ty), count));
    }
    lines.push(format!("total: {total}"));
    lines.join("\n")
}

/// Lower-case snake name for a NodeType (serde rename format), without
/// serializing.
fn serde_plain_name(ty: &NodeType) -> String {
    // NodeType serializes snake_case; use serde_plain if available via
    // serde_json round-trip as the boring fallback.
    serde_json::to_value(ty)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_else(|| format!("{ty:?}").to_lowercase())
}
