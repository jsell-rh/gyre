//! Shared two-commit graph-diff core.
//!
//! Computes the node-level change set between two boundary commits from real
//! per-commit repository state: each boundary ref is resolved against the
//! repository's real git (>=7-hex SHA prefixes, branch names, tags, HEAD),
//! each boundary commit's tree is snapshotted via the `git archive` machinery
//! (shared with the push-triggered extraction pipeline,
//! [`crate::graph_extraction::snapshot_commit_tree`]), the extraction
//! pipeline runs on each snapshot, and the change set is computed by
//! qualified-name identity with the pure domain function
//! [`gyre_domain::graph_diff::compute_node_change_set`].
//!
//! Both contract surfaces consume THIS module — never a private copy:
//! - `GET /repos/{id}/graph/diff` (realized-model.md "Graph Diff", shared
//!   with task graph-diff-two-commits of plan
//!   b26f707c1c6ea89a961ab82e824808bf1625a3e2b3a0c2312981a90a99f3fef3),
//! - the view-query `Scope::Diff` (view-query-grammar.md §2 `diff`).
//!
//! Recorded-delta point-in-time reconstruction (`list_nodes_at`/
//! `list_edges_at`) cannot serve these surfaces because callers reference
//! arbitrary commit pairs — including boundary commits at which no
//! extraction ever ran.
//!
//! Per-(repo, resolved commit) extraction results are cached; a cache entry
//! is keyed by the FULL resolved SHA, whose tree content is immutable, so
//! the cache can never alter results.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use gyre_common::graph::GraphNode;
use gyre_common::view_query::{Scope, ViewQuery};
use gyre_common::Id;
use gyre_domain::graph_diff::{compute_node_change_set, NodeChangeSet};
use gyre_domain::view_query_resolver::InjectedDiffChangeSet;

/// Maximum number of per-commit extraction results kept in the process-wide
/// cache. Bounded so repeated diff queries cannot grow memory without limit;
/// eviction removes the oldest entries first and never changes results
/// (entries are keyed by immutable (repo, full SHA) pairs).
const EXTRACTION_CACHE_MAX_ENTRIES: usize = 32;

type ExtractionCache = HashMap<(String, String), Arc<Vec<GraphNode>>>;

fn extraction_cache() -> &'static Mutex<ExtractionCache> {
    static CACHE: OnceLock<Mutex<ExtractionCache>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn git_bin() -> String {
    std::env::var("GYRE_GIT_PATH").unwrap_or_else(|_| "git".to_string())
}

/// The resolved two-commit diff for a repo.
#[derive(Debug, Clone)]
pub struct CommitRangeDiff {
    /// Full SHA the from ref resolved to.
    pub from_sha: String,
    /// Full SHA the to ref resolved to.
    pub to_sha: String,
    /// Node-level change set (qualified-name identity) between the two
    /// boundary commits.
    pub change_set: NodeChangeSet,
}

/// Outcome of a commit-range diff computation: either the resolved diff, or
/// explicit warnings explaining why no diff could be computed. Never a
/// fabricated/fail-open default (a ref that does not resolve means the state
/// cannot be determined — it does not mean "empty diff is fine").
#[derive(Debug, Clone, Default)]
pub struct CommitRangeDiffOutcome {
    pub diff: Option<CommitRangeDiff>,
    pub warnings: Vec<String>,
}

/// Resolve `refname` in `repo_path` to its full SHA, or `None` when it does
/// not resolve.
///
/// Uses the repo's real git (`git rev-parse --verify`), so >=7-hex SHA
/// prefixes (when unambiguous), branch names, tags, and HEAD all resolve.
/// `None` is surfaced by callers as an explicit warning — never defaulted.
async fn resolve_commit_ref(repo_path: &str, refname: &str) -> Option<String> {
    crate::git_refs::resolve_ref(repo_path, refname).await
}

/// Extract the graph nodes for `sha` (with per-(repo, sha) caching).
///
/// Snapshots the commit tree and runs the full extraction pipeline on it —
/// the same production extractors the push flow uses.
async fn extract_commit_nodes(
    repo_path: &str,
    repo_id: &Id,
    sha: &str,
) -> anyhow::Result<Arc<Vec<GraphNode>>> {
    let cache_key = (repo_id.to_string(), sha.to_string());
    if let Some(hit) = extraction_cache()
        .lock()
        .ok()
        .and_then(|c| c.get(&cache_key).cloned())
    {
        return Ok(hit);
    }

    let tmp = crate::graph_extraction::snapshot_commit_tree(repo_path, sha, &git_bin()).await?;
    let tmp_path: PathBuf = tmp.path().to_path_buf();
    let sha_owned = sha.to_string();
    let repo_id_str = repo_id.to_string();
    let (nodes, _edges) = tokio::task::spawn_blocking(move || {
        crate::graph_extraction::run_all_extractors(&tmp_path, &sha_owned, &repo_id_str)
    })
    .await?;

    let nodes = Arc::new(nodes);
    if let Ok(mut cache) = extraction_cache().lock() {
        // Bound the cache: drop the oldest entries (HashMap has no order, so
        // evict until under the limit; correctness is unaffected either way
        // because entries are immutable per (repo, full SHA)).
        while cache.len() >= EXTRACTION_CACHE_MAX_ENTRIES {
            let victim = cache.keys().next().cloned();
            match victim {
                Some(k) => {
                    cache.remove(&k);
                }
                None => break,
            }
        }
        cache.insert(cache_key, Arc::clone(&nodes));
    }
    Ok(nodes)
}

/// Compute the node-level change set between two boundary commits of a repo,
/// from real per-commit repository state.
///
/// Boundary refs are resolved against the repo's real git; an unresolvable
/// ref yields an explicit warning and `diff: None` (empty matched set at the
/// caller) — never a fabricated default. Refs that resolve to the same
/// commit yield an empty change set.
pub async fn compute_commit_range_diff(
    repo_path: &str,
    repo_id: &Id,
    from_ref: &str,
    to_ref: &str,
) -> CommitRangeDiffOutcome {
    let mut outcome = CommitRangeDiffOutcome::default();

    let from_sha = match resolve_commit_ref(repo_path, from_ref).await {
        Some(sha) => sha,
        None => {
            outcome.warnings.push(format!(
                "Diff from_commit '{}' could not be resolved in the repository's git \
                 (expected a >=7-hex SHA prefix, branch name, tag, or HEAD); \
                 the diff matched set is empty.",
                from_ref
            ));
            return outcome;
        }
    };
    let to_sha = match resolve_commit_ref(repo_path, to_ref).await {
        Some(sha) => sha,
        None => {
            outcome.warnings.push(format!(
                "Diff to_commit '{}' could not be resolved in the repository's git \
                 (expected a >=7-hex SHA prefix, branch name, tag, or HEAD); \
                 the diff matched set is empty.",
                to_ref
            ));
            return outcome;
        }
    };

    if from_sha == to_sha {
        outcome.diff = Some(CommitRangeDiff {
            from_sha,
            to_sha,
            change_set: NodeChangeSet::default(),
        });
        return outcome;
    }

    let (from_nodes, to_nodes) = match (
        extract_commit_nodes(repo_path, repo_id, &from_sha).await,
        extract_commit_nodes(repo_path, repo_id, &to_sha).await,
    ) {
        (Ok(f), Ok(t)) => (f, t),
        (Err(e), _) | (_, Err(e)) => {
            outcome.warnings.push(format!(
                "Failed to compute the commit-range diff between '{}' and '{}' \
                 (boundary-commit extraction failed: {e}); the diff matched set is empty.",
                from_ref, to_ref
            ));
            return outcome;
        }
    };

    let change_set = compute_node_change_set(&from_nodes, &to_nodes);
    outcome.diff = Some(CommitRangeDiff {
        from_sha,
        to_sha,
        change_set,
    });
    outcome
}

/// Whether a view query carries a SHA/ref-based (non-temporal) Diff scope —
/// the query form whose change set must come from this core.
pub fn query_needs_diff_change_set(query: &ViewQuery) -> bool {
    diff_scope_refs(query).is_some()
}

/// The (from, to) refs of a non-temporal Diff scope, if the query has one.
fn diff_scope_refs(query: &ViewQuery) -> Option<(&str, &str)> {
    match &query.scope {
        Scope::Diff {
            from_commit,
            to_commit,
        } => {
            let temporal = |s: &str| {
                s.strip_prefix('~').is_some_and(|rest| {
                    !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit())
                })
            };
            // The domain resolver's temporal path requires BOTH boundaries to
            // be "~<epoch>" refs; any other combination is a SHA/ref-based
            // query form and needs the server-computed change set.
            if temporal(from_commit) && temporal(to_commit) {
                None
            } else {
                Some((from_commit.as_str(), to_commit.as_str()))
            }
        }
        _ => None,
    }
}

/// Resolve the injected change set for a view query's SHA/ref-based Diff
/// scope against the repo's real git, via the shared core.
///
/// Returns `None` when the query has no non-temporal Diff scope (nothing to
/// inject). Otherwise returns `Some(...)` — an unresolvable ref or extraction
/// failure yields an empty change set plus explicit warnings (surfaced in
/// `DryRunResult.warnings` by the domain resolver), never a fabricated
/// default.
pub async fn resolve_view_query_diff_changes(
    repo_path: &str,
    repo_id: &Id,
    query: &ViewQuery,
) -> Option<InjectedDiffChangeSet> {
    let (from_ref, to_ref) = diff_scope_refs(query)?;
    let outcome = compute_commit_range_diff(repo_path, repo_id, from_ref, to_ref).await;
    match outcome.diff {
        Some(d) => Some(InjectedDiffChangeSet {
            added: d.change_set.added,
            removed: d.change_set.removed,
            modified: d.change_set.modified,
            warnings: outcome.warnings,
        }),
        None => Some(InjectedDiffChangeSet {
            warnings: outcome.warnings,
            ..Default::default()
        }),
    }
}

/// [`resolve_view_query_diff_changes`] for the API/tool surfaces: loads the
/// repo record to find its git path, then resolves via the shared core.
/// A missing repo record or an unusable repo path is surfaced as an explicit
/// warning on an empty change set — never a silent default.
pub async fn resolve_view_query_diff_changes_for_state(
    state: &crate::AppState,
    repo_id: &str,
    query: &ViewQuery,
) -> Option<InjectedDiffChangeSet> {
    if !query_needs_diff_change_set(query) {
        return None;
    }
    match state.repos.find_by_id(&Id::new(repo_id)).await {
        Ok(Some(repo)) if !repo.path.is_empty() => {
            resolve_view_query_diff_changes(&repo.path, &Id::new(repo_id), query).await
        }
        Ok(_) => Some(InjectedDiffChangeSet {
            warnings: vec![format!(
                "Diff scope could not be resolved: repo '{repo_id}' has no git storage path; \
                 the diff matched set is empty."
            )],
            ..Default::default()
        }),
        Err(e) => Some(InjectedDiffChangeSet {
            warnings: vec![format!(
                "Diff scope could not be resolved: failed to load repo '{repo_id}' ({e}); \
                 the diff matched set is empty."
            )],
            ..Default::default()
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temporal_diff_scopes_do_not_need_the_core() {
        let q: ViewQuery = serde_json::from_value(serde_json::json!({
            "scope": { "type": "diff", "from_commit": "~1000", "to_commit": "~2000" }
        }))
        .unwrap();
        assert!(!query_needs_diff_change_set(&q));
        assert!(diff_scope_refs(&q).is_none());
    }

    #[test]
    fn sha_and_ref_diff_scopes_need_the_core() {
        for (from, to) in [
            ("abcdef1234567", "1234567abcdef"),
            ("main", "HEAD"),
            ("v1.0", "feature/x"),
            ("no-such-branch", "HEAD"),
        ] {
            let q: ViewQuery = serde_json::from_value(serde_json::json!({
                "scope": { "type": "diff", "from_commit": from, "to_commit": to }
            }))
            .unwrap();
            assert!(query_needs_diff_change_set(&q), "{from}..{to}");
            assert_eq!(diff_scope_refs(&q), Some((from, to)));
        }
    }

    #[test]
    fn non_diff_scopes_do_not_need_the_core() {
        let q: ViewQuery = serde_json::from_value(serde_json::json!({
            "scope": { "type": "all" }
        }))
        .unwrap();
        assert!(!query_needs_diff_change_set(&q));
    }

    #[test]
    fn mixed_temporal_sha_refs_need_the_core() {
        // Only one boundary temporal → not a temporal query form; the SHA
        // side still needs real git resolution.
        let q: ViewQuery = serde_json::from_value(serde_json::json!({
            "scope": { "type": "diff", "from_commit": "~1000", "to_commit": "main" }
        }))
        .unwrap();
        assert!(query_needs_diff_change_set(&q));
    }
}
