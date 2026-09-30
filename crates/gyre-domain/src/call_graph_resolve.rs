//! Pure resolution of Pass 2 call edges to graph edges.
//!
//! Pass 2 subprocess I/O lives in `gyre-adapters` behind the
//! `CallGraphExtractor` port. This module holds the *pure* half: detecting a
//! repository's languages (by reading marker files in the repo subtree) and
//! resolving the raw qualified-name [`CallEdge`]s a type checker reports into
//! [`GraphEdge`]s between the nodes extracted in Pass 1.
//!
//! No subprocess I/O happens here — this keeps the domain layer free of
//! infrastructure concerns (see `scripts/check-arch.sh`).

use gyre_common::call_graph::{CallEdge, Language};
use gyre_common::graph::{EdgeType, GraphEdge, GraphNode, NodeType};
use gyre_common::Id;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

// ── Language detection (marker-file reads only, no subprocess) ──────────────

fn has_rust_manifest(dir: &Path) -> bool {
    dir.join("Cargo.toml").is_file()
}

fn has_go_manifest(dir: &Path) -> bool {
    dir.join("go.mod").is_file()
}

fn has_python_manifest(dir: &Path) -> bool {
    dir.join("pyproject.toml").is_file()
        || dir.join("setup.py").is_file()
        || dir.join("requirements.txt").is_file()
}

fn has_typescript_manifest(dir: &Path) -> bool {
    dir.join("tsconfig.json").is_file() || dir.join("package.json").is_file()
}

/// Detect ALL languages present in a (possibly polyglot) repository.
///
/// Checks the repo root and its immediate subdirectories (depth=1) for
/// manifest files, so monorepos with language roots one level down
/// (e.g. `backend/Cargo.toml`, `frontend/package.json`) are handled.
pub fn detect_all_languages(repo_root: &Path) -> Vec<Language> {
    let mut has_rust = false;
    let mut has_go = false;
    let mut has_python = false;
    let mut has_ts = false;

    let mut dirs_to_check: Vec<PathBuf> = vec![repo_root.to_path_buf()];
    if let Ok(entries) = std::fs::read_dir(repo_root) {
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.is_dir() {
                dirs_to_check.push(path);
            }
        }
    }

    for dir in &dirs_to_check {
        has_rust = has_rust || has_rust_manifest(dir);
        has_go = has_go || has_go_manifest(dir);
        has_python = has_python || has_python_manifest(dir);
        has_ts = has_ts || has_typescript_manifest(dir);
    }

    let mut languages = Vec::new();
    if has_rust {
        languages.push(Language::Rust);
    }
    if has_go {
        languages.push(Language::Go);
    }
    if has_python {
        languages.push(Language::Python);
    }
    if has_ts {
        languages.push(Language::TypeScript);
    }
    languages
}

// ── Edge resolution ─────────────────────────────────────────────────────────

/// Resolve the raw call edges reported by a type checker into [`GraphEdge`]s
/// between the Pass 1 nodes, deduplicating against `existing_edges`.
///
/// `language` selects the name-resolution strategy: Go qualified names require
/// receiver/package-aware suffix matching (see [`resolve_go_node`]); other
/// languages emit names that match node `qualified_name`s directly.
///
/// Edges whose `from` or `to` name cannot be resolved to an existing node are
/// dropped — Pass 2 only adds edges between known Pass 1 nodes.
pub fn resolve_call_edges(
    language: Language,
    raw_edges: &[CallEdge],
    nodes: &[GraphNode],
    existing_edges: &[GraphEdge],
    repo_id: &Id,
) -> Vec<GraphEdge> {
    // Exact qualified_name lookup.
    let by_qname: HashMap<&str, &GraphNode> = nodes
        .iter()
        .filter(|n| n.deleted_at.is_none() && !n.qualified_name.is_empty())
        .map(|n| (n.qualified_name.as_str(), n))
        .collect();

    // Callable nodes grouped by qualified_name for suffix matching.
    let mut by_name: HashMap<&str, Vec<&GraphNode>> = HashMap::new();
    for n in nodes.iter().filter(|n| {
        n.deleted_at.is_none() && matches!(n.node_type, NodeType::Function | NodeType::Endpoint)
    }) {
        by_name
            .entry(n.qualified_name.as_str())
            .or_default()
            .push(n);
    }

    let existing_calls: HashSet<(String, String)> = existing_edges
        .iter()
        .filter(|e| e.deleted_at.is_none() && e.edge_type == EdgeType::Calls)
        .map(|e| (e.source_id.to_string(), e.target_id.to_string()))
        .collect();

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let mut new_edges = Vec::new();
    let mut seen: HashSet<(String, String)> = HashSet::new();

    for edge in raw_edges {
        let from = resolve_node(language, &edge.from, &by_qname, &by_name);
        let to = resolve_node(language, &edge.to, &by_qname, &by_name);
        let (Some(from), Some(to)) = (from, to) else {
            continue;
        };
        if from.id == to.id {
            continue;
        }
        let key = (from.id.to_string(), to.id.to_string());
        if existing_calls.contains(&key) || !seen.insert(key) {
            continue;
        }
        new_edges.push(GraphEdge {
            id: Id::new(Uuid::new_v4().to_string()),
            repo_id: repo_id.clone(),
            source_id: from.id.clone(),
            target_id: to.id.clone(),
            edge_type: EdgeType::Calls,
            metadata: None,
            first_seen_at: now,
            last_seen_at: now,
            deleted_at: None,
        });
    }

    new_edges
}

fn resolve_node<'a>(
    language: Language,
    qualified: &str,
    by_qname: &HashMap<&str, &'a GraphNode>,
    by_name: &HashMap<&str, Vec<&'a GraphNode>>,
) -> Option<&'a GraphNode> {
    match language {
        Language::Go => resolve_go_node(qualified, by_qname, by_name),
        _ => by_qname
            .get(qualified)
            .copied()
            .or_else(|| by_name.get(qualified).map(|c| c[0])),
    }
}

/// Resolve a Go qualified name (e.g. `pkg/path.TypeName.MethodName`) to a graph
/// node.
///
/// Resolution strategy:
/// 1. Exact qualified-name match (most reliable).
/// 2. For methods (`Receiver.Method`), require the full `Receiver.Method`
///    suffix to match — not just the bare method name, which causes false
///    positives like `FooService.Handle` matching `BarService.Handle`.
/// 3. When multiple candidates match, prefer one in the same package directory
///    inferred from the caller's package path in the qualified name.
pub fn resolve_go_node<'a>(
    qualified: &str,
    by_qname: &HashMap<&str, &'a GraphNode>,
    by_name: &HashMap<&str, Vec<&'a GraphNode>>,
) -> Option<&'a GraphNode> {
    if let Some(n) = by_qname.get(qualified) {
        return Some(n);
    }
    if let Some(candidates) = by_name.get(qualified) {
        return Some(candidates[0]);
    }

    let pkg_prefix = qualified.rsplit_once('.').map(|(prefix, _)| prefix);

    // Try TypeName.MethodName pattern first (more specific than bare name).
    let parts: Vec<&str> = qualified.rsplitn(3, '.').collect();
    if parts.len() >= 2 {
        let method = parts[0];
        let type_name = parts[1];
        let combined = format!("{}.{}", type_name, method);
        let suffix = format!(".{}", combined);
        let candidates: Vec<&GraphNode> = by_name
            .iter()
            .filter(|(qn, _)| **qn == combined.as_str() || qn.ends_with(&suffix))
            .flat_map(|(_, nodes)| nodes.iter().copied())
            .collect();
        if candidates.len() == 1 {
            return Some(candidates[0]);
        }
        if candidates.len() > 1 {
            if let Some(pkg) = pkg_prefix {
                if let Some(best) = candidates
                    .iter()
                    .find(|n| n.qualified_name.starts_with(pkg) || n.file_path.contains(pkg))
                {
                    return Some(best);
                }
            }
            return Some(candidates[0]);
        }
    }

    // Plain functions (no receiver): match by full qualified_name suffix, but
    // only when there is a single unambiguous match.
    if let Some(short) = qualified.rsplit('.').next() {
        let exact_suffix = format!(".{}", short);
        let candidates: Vec<&GraphNode> = by_name
            .iter()
            .filter(|(qn, _)| **qn == short || qn.ends_with(&exact_suffix))
            .flat_map(|(_, nodes)| nodes.iter().copied())
            .collect();
        if candidates.len() == 1 {
            return Some(candidates[0]);
        }
        if candidates.len() > 1 {
            if let Some(pkg) = pkg_prefix {
                if let Some(best) = candidates
                    .iter()
                    .find(|n| n.qualified_name.starts_with(pkg) || n.file_path.contains(pkg))
                {
                    return Some(best);
                }
            }
            // Ambiguous with no package hint → do not guess (wrong edges are
            // worse than missing ones).
            return None;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use gyre_common::graph::{SpecConfidence, Visibility};

    fn func_node(id: &str, qname: &str, file: &str) -> GraphNode {
        GraphNode {
            id: Id::new(id),
            repo_id: Id::new("repo1"),
            node_type: NodeType::Function,
            name: qname.rsplit('.').next().unwrap_or(qname).to_string(),
            qualified_name: qname.to_string(),
            file_path: file.to_string(),
            line_start: 1,
            line_end: 10,
            visibility: Visibility::Public,
            doc_comment: None,
            spec_path: None,
            spec_paths: vec![],
            spec_confidence: SpecConfidence::None,
            last_modified_sha: "abc".to_string(),
            last_modified_by: None,
            last_modified_at: 0,
            created_sha: "abc".to_string(),
            created_at: 0,
            complexity: None,
            churn_count_30d: 0,
            test_coverage: None,
            first_seen_at: 0,
            last_seen_at: 0,
            deleted_at: None,
            test_node: false,
            spec_approved_at: None,
            milestone_completed_at: None,
        }
    }

    #[test]
    fn detect_all_languages_polyglot() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join("go.mod"), "module x\ngo 1.21").unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[package]").unwrap();
        let langs = detect_all_languages(dir.path());
        assert!(langs.contains(&Language::Go));
        assert!(langs.contains(&Language::Rust));
        assert_eq!(langs.len(), 2);
    }

    #[test]
    fn detect_all_languages_empty_dir_is_empty() {
        let dir = tempfile::TempDir::new().unwrap();
        assert!(detect_all_languages(dir.path()).is_empty());
    }

    #[test]
    fn resolve_go_cross_package_edge() {
        let nodes = vec![
            func_node("h", "example.com/x/api.Handler.Handle", "api/handler.go"),
            func_node(
                "p",
                "example.com/x/service.ProcessRequest",
                "service/svc.go",
            ),
        ];
        let raw = vec![CallEdge {
            from: "example.com/x/api.Handler.Handle".to_string(),
            to: "example.com/x/service.ProcessRequest".to_string(),
        }];
        let edges = resolve_call_edges(Language::Go, &raw, &nodes, &[], &Id::new("repo1"));
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].source_id, Id::new("h"));
        assert_eq!(edges[0].target_id, Id::new("p"));
        assert_eq!(edges[0].edge_type, EdgeType::Calls);
    }

    #[test]
    fn resolve_dedups_against_existing_calls() {
        let nodes = vec![
            func_node("a", "pkg.a", "a.go"),
            func_node("b", "pkg.b", "b.go"),
        ];
        let existing = vec![GraphEdge {
            id: Id::new("e1"),
            repo_id: Id::new("repo1"),
            source_id: Id::new("a"),
            target_id: Id::new("b"),
            edge_type: EdgeType::Calls,
            metadata: None,
            first_seen_at: 0,
            last_seen_at: 0,
            deleted_at: None,
        }];
        let raw = vec![CallEdge {
            from: "pkg.a".to_string(),
            to: "pkg.b".to_string(),
        }];
        let edges = resolve_call_edges(Language::Go, &raw, &nodes, &existing, &Id::new("repo1"));
        assert!(
            edges.is_empty(),
            "edge already present in Pass 1 must not be re-added"
        );
    }

    #[test]
    fn resolve_dedups_within_batch() {
        let nodes = vec![
            func_node("a", "pkg.a", "a.go"),
            func_node("b", "pkg.b", "b.go"),
        ];
        let raw = vec![
            CallEdge {
                from: "pkg.a".to_string(),
                to: "pkg.b".to_string(),
            },
            CallEdge {
                from: "pkg.a".to_string(),
                to: "pkg.b".to_string(),
            },
        ];
        let edges = resolve_call_edges(Language::Go, &raw, &nodes, &[], &Id::new("repo1"));
        assert_eq!(edges.len(), 1, "duplicate raw edges collapse to one");
    }

    #[test]
    fn resolve_skips_unknown_endpoints() {
        let nodes = vec![func_node("a", "pkg.a", "a.go")];
        let raw = vec![CallEdge {
            from: "pkg.a".to_string(),
            to: "pkg.unknown".to_string(),
        }];
        let edges = resolve_call_edges(Language::Go, &raw, &nodes, &[], &Id::new("repo1"));
        assert!(edges.is_empty(), "edge to an unknown node is dropped");
    }

    #[test]
    fn resolve_non_go_uses_exact_match() {
        let nodes = vec![
            func_node("a", "mod.Class.method", "a.py"),
            func_node("b", "other.func", "b.py"),
        ];
        let raw = vec![CallEdge {
            from: "mod.Class.method".to_string(),
            to: "other.func".to_string(),
        }];
        let edges = resolve_call_edges(Language::Python, &raw, &nodes, &[], &Id::new("repo1"));
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].source_id, Id::new("a"));
        assert_eq!(edges[0].target_id, Id::new("b"));
    }
}
