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

/// A resolved prompt template with its provenance (ui-layout.md §2).
///
/// `sha` is `Some(branch-head SHA)` only when the template was loaded from
/// the repo's git tree (`specs/prompts/<function_key>.md`); DB overrides and
/// hardcoded defaults have no git provenance and carry `None`.
#[derive(Debug, Clone)]
pub struct ResolvedPrompt {
    pub content: String,
    pub sha: Option<String>,
}

/// View spec grammar schema injected into the explorer-generate prompt
/// (ui-layout.md §2 Available Data / Output Format: "The server sends the
/// LLM: the question, the list of available node types and counts in the
/// workspace, and the view spec grammar schema").
///
/// Mirrors the grammar in ui-layout.md §4 that `parse_and_validate` enforces
/// (ViewQuery first, legacy ViewSpec accepted).
pub const VIEW_SPEC_GRAMMAR: &str = "\
{
  \"name\": string,
  \"description\": string,
  \"data\": {
    \"concept\": string|null,
    \"node_types\": string[],     // node type names present in the graph
    \"edge_types\": string[],
    \"depth\": integer 0-5,
    \"filter\": {\"min_churn\": integer|null, \"spec_path\": string|null, \"visibility\": \"public\"|\"private\"|null},
    \"repo_id\": string|null
  },
  \"layout\": \"graph\"|\"hierarchical\"|\"layered\"|\"list\"|\"timeline\"|\"side-by-side\"|\"diff\"|\"flow\",
  \"encoding\": {
    \"color\": {\"field\": string, \"scale\": string},
    \"size\": {\"field\": string, \"scale\": \"linear\", \"range\": [number, number]},
    \"label\": string,
    \"group_by\": string
  },
  \"annotations\": [{\"node_name\": string, \"text\": string}],
  \"highlight\": {\"spec_path\": string}|null,
  \"explanation\": string
}";

/// Substitute `{{variable}}` placeholders in a prompt template
/// (ui-layout.md §2: "Variables enclosed in `{{...}}` are substituted at
/// runtime by the server. The template itself is static text committed to
/// git.").
///
/// Placeholders with no matching variable are left verbatim so unknown
/// template variables remain visible in the prompt (and testable) rather
/// than being silently dropped.
pub fn substitute_template(template: &str, vars: &[(&str, &str)]) -> String {
    let mut out = template.to_string();
    for (name, value) in vars {
        out = out.replace(&format!("{{{{{name}}}}}"), value);
    }
    out
}
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
    (model, max_tokens_for_function(function_key, |k| std::env::var(k).ok()))
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
    let (summary, _count) = workspace_graph_summary_parts(state, workspace_id, repo_filter).await;
    summary
}

/// `workspace_graph_summary` split into its two template variables:
/// `(node_type_summary, node_count)` — the explorer-generate template header
/// (ui-layout.md §2) uses `{{node_type_summary}}` and `{{node_count}}`.
///
/// Format: one line per node type present, `"<type>: <count>"`. Repo-scoped
/// when `repo_id` is given.
pub async fn workspace_graph_summary_parts(
    state: &AppState,
    workspace_id: &Id,
    repo_filter: Option<&Id>,
) -> (String, u64) {
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

    let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for rid in repos {
        let repo_id = Id::new(&rid);
        let nodes = state.graph_store.list_nodes(&repo_id, None).await;
        for node in nodes.unwrap_or_default() {
            *counts.entry(serde_plain_name(&node.node_type)).or_insert(0) += 1;
        }
    }

    let total: u64 = counts.values().map(|c| *c as u64).sum();
    if counts.is_empty() {
        return ("No graph nodes indexed for this workspace yet.".to_string(), 0);
    }

    let mut lines: Vec<String> = counts
        .into_iter()
        .map(|(name, count)| format!("{name}: {count}"))
        .collect();
    lines.sort();
    (lines.join("\n"), total)
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

#[cfg(test)]
mod tests {
    use super::*;
    use gyre_ports::GitOpsPort as _;
    use std::sync::Arc;

    // ── substitute_template ──────────────────────────────────────────────

    #[test]
    fn substitute_replaces_known_variables() {
        let out = substitute_template(
            "workspace {{workspace_name}} has {{node_count}} nodes",
            &[("workspace_name", "acme"), ("node_count", "42")],
        );
        assert_eq!(out, "workspace acme has 42 nodes");
    }

    #[test]
    fn substitute_leaves_unknown_variables_verbatim() {
        // Unknown variables stay visible in the prompt rather than being
        // silently dropped — a typo'd variable name must be diagnosable.
        let out = substitute_template("x: {{known}} y: {{unknown}}", &[("known", "v")]);
        assert_eq!(out, "x: v y: {{unknown}}");
    }

    #[test]
    fn substitute_does_not_partial_match_different_names() {
        // {{node_count}} must not clobber {{node_count_extra}}-style names:
        // replacement is exact-brace, not substring.
        let out = substitute_template(
            "{{a}} {{ab}}",
            &[("a", "1"), ("ab", "2")],
        );
        assert_eq!(out, "1 2");
    }

    #[test]
    fn substitute_escapes_nothing_special_in_values() {
        // Values containing braces must not recursively expand.
        let out = substitute_template("{{x}}", &[("x", "{{x}}")]);
        assert_eq!(out, "{{x}}");
    }

    // ── max_tokens_for_function ──────────────────────────────────────────

    #[test]
    fn max_tokens_defaults_match_spec() {
        // ui-layout.md §2: generate=2000, ask=4000, assist=4000.
        let none = |_: &str| None::<String>;
        assert_eq!(
            max_tokens_for_function("explorer-generate", none),
            Some(2000)
        );
        assert_eq!(max_tokens_for_function("briefing-ask", none), Some(4000));
        assert_eq!(max_tokens_for_function("specs-assist", none), Some(4000));
    }

    #[test]
    fn max_tokens_env_override_wins() {
        let env = |name: &str| match name {
            "GYRE_LLM_MAX_TOKENS_GENERATE" => Some("500".to_string()),
            _ => None::<String>,
        };
        assert_eq!(
            max_tokens_for_function("explorer-generate", env),
            Some(500)
        );
        // Other endpoints untouched by that env var.
        let none = |_: &str| None::<String>;
        assert_eq!(max_tokens_for_function("briefing-ask", none), Some(4000));
    }

    /// Build a test state with real git ops, a repo whose default branch
    /// contains `specs/prompts/briefing-ask.md`, and register it in a
    /// workspace.
    async fn state_with_prompt_in_git(content: &str) -> (Arc<AppState>, Id) {
        let dir = tempfile::TempDir::new().unwrap();
        let repo_path = dir.path().join("repo.git");
        let git = gyre_adapters::Git2OpsAdapter::new();
        git.init_bare(repo_path.to_str().unwrap()).await.unwrap();
        git.create_initial_commit(repo_path.to_str().unwrap(), "main")
            .await
            .unwrap();
        git.write_file(
            repo_path.to_str().unwrap(),
            "main",
            "specs/prompts/briefing-ask.md",
            content.as_bytes(),
            "add prompt template",
        )
        .await
        .unwrap();

        let mut state = (*crate::mem::test_state()).clone();
        state.git_ops = Arc::new(git);
        let state = Arc::new(state);

        // Workspace + repo entities pointing at the real git path.
        let ws_id = Id::new("ws-tpl-test");
        state
            .workspaces
            .create(&gyre_domain::Workspace::new(
                ws_id.clone(),
                Id::new("tenant-1"),
                "tpl-ws",
                "tpl-ws",
                0,
            ))
            .await
            .unwrap();
        let repo = gyre_domain::Repository::new(
            Id::new("repo-tpl-test"),
            ws_id.clone(),
            "tpl-repo",
            repo_path.to_str().unwrap().to_string(),
            0,
        );
        state.repos.create(&repo).await.unwrap();

        // Leak the tempdir: the repo must stay on disk for the duration of
        // the test (the state holds only the path string).
        std::mem::forget(dir);
        (state, ws_id)
    }

    #[tokio::test]
    async fn prompt_template_loads_from_repo_git_tree_with_sha() {
        let (state, ws_id) =
            state_with_prompt_in_git("# Git template\nvars: {{node_count}}").await;

        let resolved =
            resolve_prompt_template(&state, &ws_id, None, "briefing-ask", "FALLBACK").await;

        assert_eq!(resolved.content, "# Git template\nvars: {{node_count}}");
        // Provenance: the default-branch head SHA, a real 40-hex git SHA —
        // not None (DB/hardcoded) and not a fake.
        let sha = resolved.sha.expect("git-tree template must carry a SHA");
        assert_eq!(sha.len(), 40);
        assert!(sha.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[tokio::test]
    async fn prompt_template_falls_back_to_hardcoded_when_absent() {
        // No repo carries the file → hardcoded fallback, no SHA.
        let state = crate::mem::test_state();
        let ws_id = Id::new("ws-no-template");

        let resolved =
            resolve_prompt_template(&state, &ws_id, None, "briefing-ask", "FALLBACK").await;

        assert_eq!(resolved.content, "FALLBACK");
        assert!(resolved.sha.is_none());
    }

    #[tokio::test]
    async fn db_override_beats_git_tree() {
        let (state, ws_id) = state_with_prompt_in_git("from git").await;

        state
            .prompt_templates
            .upsert_workspace(
                &ws_id,
                "briefing-ask",
                "from db",
                &Id::new("user-1"),
            )
            .await
            .unwrap();

        let resolved =
            resolve_prompt_template(&state, &ws_id, None, "briefing-ask", "FALLBACK").await;
        assert_eq!(resolved.content, "from db");
        // DB override has no git provenance.
        assert!(resolved.sha.is_none());
    }
}
