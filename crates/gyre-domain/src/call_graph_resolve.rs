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
            id: calls_edge_id(repo_id, &from.id, &to.id),
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

/// Deterministic content-derived id for a Pass 2 `Calls` edge.
///
/// Rationale (specs/reviews/task-072.md F8): Pass 2 runs as a background
/// `tokio::spawn` after Pass 1's transaction commits. With random UUIDs, a
/// re-derived edge gets a NEW id every run, so the upsert never fires and rows
/// accumulate; worse, Pass 1's soft-delete sweep can land mid-flight and
/// delete an edge Pass 2 is about to (re-)insert, leaving a torn state. A
/// content-derived id — SHA-256 over `repo|source|target|calls` — makes
/// re-derivation upsert in place (the adapters' `create_edge` conflicts on
/// `id` and preserves `first_seen_at`), and gives Pass 2 ownership of its edge
/// type so the foreground sweep skips it. Include `repo_id` so the same
/// (source, target) pair in two repos never collides on the shared TEXT pk.
fn calls_edge_id(repo_id: &Id, source_id: &Id, target_id: &Id) -> Id {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(repo_id.as_str().as_bytes());
    hasher.update(b"|");
    hasher.update(source_id.as_str().as_bytes());
    hasher.update(b"|");
    hasher.update(target_id.as_str().as_bytes());
    hasher.update(b"|calls");
    let digest = hasher.finalize();
    let hex: String = digest.iter().map(|b| format!("{:02x}", b)).collect();
    // Shape as 8-4-4-4-12 for consistency with other edge ids. Assembled from
    // chars (never constant byte-index slicing) — see
    // scripts/check-byte-slice-truncation.sh.
    let mut out = String::with_capacity(36);
    for (i, c) in hex.chars().take(32).enumerate() {
        if i == 8 || i == 12 || i == 16 || i == 20 {
            out.push('-');
        }
        out.push(c);
    }
    Id::new(out)
}

fn resolve_node<'a>(
    language: Language,
    qualified: &str,
    by_qname: &HashMap<&str, &'a GraphNode>,
    by_name: &HashMap<&str, Vec<&'a GraphNode>>,
) -> Option<&'a GraphNode> {
    match language {
        Language::Go => resolve_go_node(qualified, by_qname, by_name),
        // `by_name` is keyed by `qualified_name`, so any name findable there
        // was just probed in `by_qname` (which indexes every node). A
        // `by_name` fallback here could never fire — the `candidates[0]` it
        // used to carry was dead code containing an arbitrary tiebreak, which
        // this module forbids outright (specs/prompts/implementation.md #146).
        _ => by_qname.get(qualified).copied(),
    }
}

/// Resolve a Go qualified name (e.g. `pkg/path.TypeName.MethodName`) to a graph
/// node.
///
/// One policy, applied to every branch: resolve exactly or refuse.
/// 1. Exact qualified-name match (most reliable — both Pass 1 and the
///    type checker build import-path names, so this is the normal case).
/// 2. For methods (`Receiver.Method`), require the full `Receiver.Method`
///    suffix to match — not just the bare method name, which causes false
///    positives like `FooService.Handle` matching `BarService.Handle`.
/// 3. A suffix match is only accepted when the package path embedded in the
///    raw qualified name corroborates it: [`node_in_pkg`] must match exactly
///    one candidate. Single candidates are checked too — a lone
///    `…/svc10.Process` suffix hit does NOT license `…/svc1.Process`
///    resolving to it; prefix-similar packages look unambiguous but are the
///    wrong callee.
/// 4. Anything left ambiguous or uncorroborated → `None`. Guessing
///    `candidates[0]` would silently link the caller to the wrong callee: a
///    wrong edge is worse than a missing one. This holds for methods and
///    plain functions alike — there is exactly one ambiguity policy in this
///    module, the `select` closure below.
///
/// [`node_in_pkg`]: Go qualified names use the import path (module +
/// directory, see `go_extractor` module docs); the hint is corroborated when
/// the node's qualified name nests inside the package or its file path
/// contains the package path.
pub fn resolve_go_node<'a>(
    qualified: &str,
    by_qname: &HashMap<&str, &'a GraphNode>,
    by_name: &HashMap<&str, Vec<&'a GraphNode>>,
) -> Option<&'a GraphNode> {
    if let Some(n) = by_qname.get(qualified) {
        return Some(n);
    }

    let pkg_prefix = qualified.rsplit_once('.').map(|(prefix, _)| prefix);

    // The single ambiguity policy for this module: a suffix match survives
    // only if exactly one candidate lies inside the raw name's package hint.
    // No ordering is ever consulted — HashMap iteration order cannot
    // influence which node (if any) is returned.
    let select = |candidates: Vec<&'a GraphNode>| -> Option<&'a GraphNode> {
        let pkg = pkg_prefix?;
        let mut hits = candidates.into_iter().filter(|n| node_in_pkg(n, pkg));
        let hit = hits.next()?;
        hits.next().is_none().then_some(hit)
    };

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
        if !candidates.is_empty() {
            return select(candidates);
        }
    }

    // Plain functions (no receiver): match by full qualified_name suffix,
    // subject to the same corroboration policy.
    if let Some(short) = qualified.rsplit('.').next() {
        let exact_suffix = format!(".{}", short);
        let candidates: Vec<&GraphNode> = by_name
            .iter()
            .filter(|(qn, _)| **qn == short || qn.ends_with(&exact_suffix))
            .flat_map(|(_, nodes)| nodes.iter().copied())
            .collect();
        if !candidates.is_empty() {
            return select(candidates);
        }
    }
    None
}

/// True when `node` lies inside the package path `pkg`:
/// - the node's qualified name nests inside `pkg` at a `.` boundary
///   (`<pkg>.Type.Method` / `<pkg>.Func`), or
/// - the file path contains the package path as a whole path segment
///   (vendored trees store `vendor/<pkg>/...`).
///
/// The boundary is load-bearing in both branches: without it the hint
/// `…/svc1` would also claim `…/svc10.Func` and the vendored file
/// `vendor/…/svc10/a.go` — a prefix-similar package, and exactly the
/// wrong-edge harm the refuse-on-ambiguity policy exists to prevent.
fn node_in_pkg(node: &GraphNode, pkg: &str) -> bool {
    node.qualified_name
        .strip_prefix(pkg)
        .map_or(false, |rest| rest.starts_with('.'))
        || path_contains_segment(&node.file_path, pkg)
}

/// True when `haystack` (a `/`-separated path) contains `pkg` (itself a
/// `/`-separated path, e.g. a Go import path) as a consecutive whole-segment
/// run. `vendor/example.com/svc1/a.go` contains `example.com/svc1`;
/// `vendor/example.com/svc10/a.go` does not contain `example.com/svc1`.
fn path_contains_segment(haystack: &str, pkg: &str) -> bool {
    let needle = format!("/{}/", pkg);
    let padded = format!("/{}", haystack.trim_matches('/'));
    padded.contains(&needle)
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

    /// Build a raw Pass 2 qualified name the way `scripts/go-callgraph`
    /// (`golang.org/x/tools/go/callgraph/cha`) does: import path
    /// (module + directory) + `.` + `Type.Method` or `func`.
    ///
    /// Written from the rule in specs/system/lsp-call-graph.md §1 — NOT by
    /// copying Pass 1's node strings. The `*_divergence_*` tests below prove
    /// this construction is a genuinely separate input: a name built from the
    /// package clause instead of the directory must NOT resolve.
    fn callgraph_name(module: &str, dir: &str, item: &str) -> String {
        format!("{module}/{dir}.{item}")
    }

    #[test]
    fn resolve_go_cross_package_edge() {
        // Pass 1 nodes: import-path qnames (module + directory).
        let nodes = vec![
            func_node("h", "example.com/x/api.Handler.Handle", "api/handler.go"),
            func_node(
                "p",
                "example.com/x/service.ProcessRequest",
                "service/svc.go",
            ),
        ];
        // Pass 2 raw edges: built by the go-callgraph rule above.
        let raw = vec![CallEdge {
            from: callgraph_name("example.com/x", "api", "Handler.Handle"),
            to: callgraph_name("example.com/x", "service", "ProcessRequest"),
        }];
        let edges = resolve_call_edges(Language::Go, &raw, &nodes, &[], &Id::new("repo1"));
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].source_id, Id::new("h"));
        assert_eq!(edges[0].target_id, Id::new("p"));
        assert_eq!(edges[0].edge_type, EdgeType::Calls);
    }

    #[test]
    fn resolve_go_import_path_when_package_clause_differs_from_dir() {
        // Directory `svc1/` whose files declare `package handlers` (legal Go:
        // clause and directory name may differ). Pass 1 qnames follow the
        // import path (module + directory); go-callgraph's Pkg.Path() is also
        // the import path, so the edge resolves.
        let nodes = vec![
            func_node(
                "h",
                "example.com/app/svc1.Handler.Handle",
                "svc1/handler.go",
            ),
            func_node("p", "example.com/app/svc1.Process", "svc1/proc.go"),
        ];
        let raw = vec![CallEdge {
            from: callgraph_name("example.com/app", "svc1", "Handler.Handle"),
            to: callgraph_name("example.com/app", "svc1", "Process"),
        }];
        let edges = resolve_call_edges(Language::Go, &raw, &nodes, &[], &Id::new("repo1"));
        assert_eq!(
            edges.len(),
            1,
            "import-path names resolve even when the package clause differs from the directory"
        );
    }

    #[test]
    fn resolve_go_divergence_package_clause_name_does_not_resolve() {
        // Divergence case (checklist #137): two directories both declaring
        // `package handlers`. A raw name built from the package CLAUSE
        // (…/handlers.Handler.Handle) is ambiguous between them and its hint
        // matches neither node — it must be dropped, not guessed. The
        // import-path names (module + directory) disambiguate cleanly,
        // proving the two fixtures are built from different rules.
        let nodes = vec![
            func_node(
                "h1",
                "example.com/app/svc1.Handler.Handle",
                "svc1/handler.go",
            ),
            func_node(
                "h2",
                "example.com/app/svc2.Handler.Handle",
                "svc2/handler.go",
            ),
        ];
        let raw = vec![CallEdge {
            from: callgraph_name("example.com/app", "svc1", "Handler.Handle"),
            to: "example.com/app/handlers.Handler.Handle".to_string(),
        }];
        let edges = resolve_call_edges(Language::Go, &raw, &nodes, &[], &Id::new("repo1"));
        assert!(
            edges.is_empty(),
            "a clause-derived name must not resolve to a directory-derived node"
        );

        let raw_ok = vec![CallEdge {
            from: callgraph_name("example.com/app", "svc1", "Handler.Handle"),
            to: callgraph_name("example.com/app", "svc2", "Handler.Handle"),
        }];
        let edges = resolve_call_edges(Language::Go, &raw_ok, &nodes, &[], &Id::new("repo1"));
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].target_id, Id::new("h2"));
    }
    #[test]
    fn resolve_go_refuses_ambiguous_method_candidates() {
        // F7 (specs/reviews/task-072.md): two `Handler.Handle` methods in
        // different packages, and a raw name whose package hint matches
        // neither. The method branch must refuse — returning candidates[0]
        // would link the caller to an arbitrary wrong callee.
        let nodes = vec![
            func_node("a", "example.com/x/pkgA.Handler.Handle", "pkgA/handler.go"),
            func_node("b", "example.com/x/pkgB.Handler.Handle", "pkgB/handler.go"),
            func_node("c", "example.com/x/pkgC.Caller.Run", "pkgC/caller.go"),
        ];
        let raw = vec![CallEdge {
            from: callgraph_name("example.com/x", "pkgC", "Caller.Run"),
            to: callgraph_name("example.com/x", "unknown", "Handler.Handle"),
        }];
        let edges = resolve_call_edges(Language::Go, &raw, &nodes, &[], &Id::new("repo1"));
        assert!(
            edges.is_empty(),
            "ambiguous method match with unusable hint must be dropped, not guessed"
        );
    }

    #[test]
    fn resolve_go_disambiguates_method_with_package_hint() {
        // The hint path still works when the raw name carries the package:
        // `pkgA.Handler.Handle`'s prefix before the method is
        // `example.com/x/pkgA.Handler`, which the pkgA node's qualified name
        // starts with.
        let nodes = vec![
            func_node("a", "example.com/x/pkgA.Handler.Handle", "pkgA/handler.go"),
            func_node("b", "example.com/x/pkgB.Handler.Handle", "pkgB/handler.go"),
        ];
        let raw = vec![CallEdge {
            from: callgraph_name("example.com/x", "pkgA", "Handler.Handle"),
            to: callgraph_name("example.com/x", "pkgB", "Handler.Handle"),
        }];
        let edges = resolve_call_edges(Language::Go, &raw, &nodes, &[], &Id::new("repo1"));
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].source_id, Id::new("a"));
        assert_eq!(edges[0].target_id, Id::new("b"));
    }

    #[test]
    fn resolve_dedups_against_existing_calls() {
        let nodes = vec![
            func_node("a", "example.com/x/svc.A", "svc/a.go"),
            func_node("b", "example.com/x/svc.B", "svc/b.go"),
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
            from: callgraph_name("example.com/x", "svc", "A"),
            to: callgraph_name("example.com/x", "svc", "B"),
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
            func_node("a", "example.com/x/svc.A", "svc/a.go"),
            func_node("b", "example.com/x/svc.B", "svc/b.go"),
        ];
        let raw = vec![
            CallEdge {
                from: callgraph_name("example.com/x", "svc", "A"),
                to: callgraph_name("example.com/x", "svc", "B"),
            },
            CallEdge {
                from: callgraph_name("example.com/x", "svc", "A"),
                to: callgraph_name("example.com/x", "svc", "B"),
            },
        ];
        let edges = resolve_call_edges(Language::Go, &raw, &nodes, &[], &Id::new("repo1"));
        assert_eq!(edges.len(), 1, "duplicate raw edges collapse to one");
    }

    #[test]
    fn resolve_skips_unknown_endpoints() {
        let nodes = vec![func_node("a", "example.com/x/svc.A", "svc/a.go")];
        let raw = vec![CallEdge {
            from: callgraph_name("example.com/x", "svc", "A"),
            to: callgraph_name("example.com/x", "svc", "Missing"),
        }];
        let edges = resolve_call_edges(Language::Go, &raw, &nodes, &[], &Id::new("repo1"));
        assert!(edges.is_empty(), "edge to an unknown node is dropped");
    }

    #[test]
    fn calls_edge_ids_are_content_derived() {
        // F8 (specs/reviews/task-072.md): re-deriving the same raw edge must
        // produce the same id so the adapters' create_edge upserts in place
        // instead of accumulating rows; different endpoints must not collide.
        let nodes = vec![
            func_node("a", "example.com/x/svc.A", "svc/a.go"),
            func_node("b", "example.com/x/svc.B", "svc/b.go"),
            func_node("c", "example.com/x/svc.C", "svc/c.go"),
        ];
        let raw = vec![CallEdge {
            from: callgraph_name("example.com/x", "svc", "A"),
            to: callgraph_name("example.com/x", "svc", "B"),
        }];
        let first = resolve_call_edges(Language::Go, &raw, &nodes, &[], &Id::new("repo1"));
        let second = resolve_call_edges(Language::Go, &raw, &nodes, &[], &Id::new("repo1"));
        assert_eq!(first.len(), 1);
        assert_eq!(
            first[0].id, second[0].id,
            "same endpoints re-derive the same edge id"
        );

        let raw_other = vec![CallEdge {
            from: callgraph_name("example.com/x", "svc", "A"),
            to: callgraph_name("example.com/x", "svc", "C"),
        }];
        let other = resolve_call_edges(Language::Go, &raw_other, &nodes, &[], &Id::new("repo1"));
        assert_eq!(other.len(), 1);
        assert_ne!(
            first[0].id, other[0].id,
            "different endpoints derive different edge ids"
        );

        // Repo scoping: the same endpoints in a different repo are a
        // different edge (sqlite pk is the bare id string).
        let other_repo = resolve_call_edges(Language::Go, &raw, &nodes, &[], &Id::new("repo2"));
        assert_ne!(
            first[0].id, other_repo[0].id,
            "same endpoints in another repo derive a different edge id"
        );
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

    #[test]
    fn resolve_go_prefix_similar_package_is_not_guessed() {
        // Raw names target package `svc1`; the graph only contains the
        // prefix-similar `svc10` package. A lone suffix hit looks
        // unambiguous, but the old branches accepted it: a single candidate
        // was returned without any package corroboration, emitting a wrong
        // edge into `svc10`. Both branches (method and plain function) must
        // now demand that the hint corroborate even the only candidate.
        let nodes = vec![
            func_node("c", "example.com/app/caller.Call", "caller/c.go"),
            func_node(
                "wrong-fn",
                "example.com/app/svc10.Process",
                "svc10/process.go",
            ),
            func_node(
                "wrong-m",
                "example.com/app/svc10.Handler.Handle",
                "svc10/handler.go",
            ),
        ];
        let raw = vec![
            CallEdge {
                from: callgraph_name("example.com/app", "caller", "Call"),
                to: callgraph_name("example.com/app", "svc1", "Process"),
            },
            CallEdge {
                from: callgraph_name("example.com/app", "caller", "Call"),
                to: callgraph_name("example.com/app", "svc1", "Handler.Handle"),
            },
        ];
        let edges = resolve_call_edges(Language::Go, &raw, &nodes, &[], &Id::new("repo1"));
        assert!(
            edges.is_empty(),
            "names for package svc1 must not resolve into prefix-similar svc10, \
             got edges to {:?}",
            edges
                .iter()
                .map(|e| e.target_id.as_str())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn resolve_go_function_hint_boundary_picks_the_real_package() {
        // Suffix path with the true package present (exact qualified-name
        // match forced to miss by an empty by_qname): the naive
        // `starts_with(pkg)` hint matched BOTH `…/svc1.Process` and
        // `…/svc10.Process` — HashMap iteration order then decided which
        // node got the edge. The boundary-aware hint leaves exactly one hit.
        let nodes = vec![
            func_node("p1", "example.com/app/svc1.Process", "svc1/process.go"),
            func_node("p10", "example.com/app/svc10.Process", "svc10/process.go"),
        ];
        let mut by_name: HashMap<&str, Vec<&GraphNode>> = HashMap::new();
        for n in &nodes {
            by_name
                .entry(n.qualified_name.as_str())
                .or_default()
                .push(n);
        }
        let empty_qnames: HashMap<&str, &GraphNode> = HashMap::new();
        let hit = resolve_go_node("example.com/app/svc1.Process", &empty_qnames, &by_name);
        assert_eq!(
            hit.map(|n| n.id.as_str()),
            Some("p1"),
            "hint …/svc1 must claim only the svc1 node, never the svc10 row"
        );
    }
    #[test]
    fn resolve_go_vendored_prefix_similar_path_is_not_guessed() {
        // The file-path corroboration branch of `node_in_pkg`: a vendored
        // node whose FILE PATH is prefix-similar to the hint must not be
        // claimed. Hint `example.com/app/svc1` used to substring-match the
        // vendored file `vendor/example.com/app/svc10/a.go`, resolving the
        // raw name into the wrong (svc10) callee.
        let nodes = vec![
            func_node("c", "example.com/app/caller.Call", "caller/c.go"),
            func_node(
                "wrong-fn",
                "vendor.example.com/app/svc10.Process",
                "vendor/example.com/app/svc10/a.go",
            ),
        ];
        let raw = vec![CallEdge {
            from: callgraph_name("example.com/app", "caller", "Call"),
            to: callgraph_name("example.com/app", "svc1", "Process"),
        }];
        let edges = resolve_call_edges(Language::Go, &raw, &nodes, &[], &Id::new("repo1"));
        assert!(
            edges.is_empty(),
            "hint …/svc1 must not claim vendored …/svc10 file path, got {:?}",
            edges
                .iter()
                .map(|e| e.target_id.as_str())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn node_in_pkg_matches_vendored_file_path_at_segment_boundary() {
        // Positive control for the same branch: the vendored node whose
        // path genuinely contains the hint as a whole-segment run IS
        // corroborated (single candidate, unambiguous).
        let nodes = vec![
            func_node("c", "example.com/app/caller.Call", "caller/c.go"),
            func_node(
                "vend",
                "vendored.svc1.Process",
                "vendor/example.com/app/svc1/a.go",
            ),
        ];
        let raw = vec![CallEdge {
            from: callgraph_name("example.com/app", "caller", "Call"),
            to: callgraph_name("example.com/app", "svc1", "Process"),
        }];
        let edges = resolve_call_edges(Language::Go, &raw, &nodes, &[], &Id::new("repo1"));
        assert_eq!(edges.len(), 1, "genuine vendored package is corroborated");
        assert_eq!(edges[0].target_id, Id::new("vend"));
    }
}
