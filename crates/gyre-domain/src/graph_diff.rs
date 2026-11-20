//! Pure two-commit node change-set computation — the shared core consumed by
//! every surface that must answer "which graph nodes changed between commit X
//! and commit Y" (view-query `Scope::Diff`, realized-model.md's
//! `GET /repos/{id}/graph/diff`).
//!
//! Boundary semantics: the change set is computed by comparing the graph
//! state extracted at each boundary commit — a node changed and reverted
//! between the boundaries is NOT reported changed, because its content is
//! identical at both boundaries. Identity is the node's `qualified_name`.
//!
//! A node counts as *modified* only when a substantive field differs
//! (signature/doc comment, structural location, visibility, metrics such as
//! complexity, spec linkage). Bookkeeping fields that differ by construction
//! between two independent extractions — `id`, `repo_id`, SHA stamps
//! (`created_sha`/`last_modified_sha`), timestamps (`created_at`,
//! `last_modified_at`, `first_seen_at`, `last_seen_at`), `deleted_at`, and
//! approval bookkeeping — never count as a content difference.
//!
//! This module is pure domain logic: no git, filesystem, or infrastructure
//! knowledge (hexagonal boundary). Callers resolve and snapshot the boundary
//! commits themselves and hand the two extracted node lists here.

use gyre_common::graph::{GraphNode, NodeType, SpecConfidence, Visibility};

/// Node-level change set between two boundary commits, keyed by
/// `qualified_name`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NodeChangeSet {
    /// Nodes present at `to` but not at `from`.
    pub added: Vec<String>,
    /// Nodes present at `from` but not at `to`.
    pub removed: Vec<String>,
    /// Nodes present at both boundaries whose substantive content differs.
    pub modified: Vec<String>,
}

impl NodeChangeSet {
    /// Qualified names of nodes that a view-query diff scope can match
    /// (added + modified). Removed nodes cannot be rendered.
    pub fn matched(&self) -> impl Iterator<Item = &str> {
        self.added
            .iter()
            .chain(self.modified.iter())
            .map(|s| s.as_str())
    }

    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty() && self.modified.is_empty()
    }
}

/// Compute the node-level change set between the graph state extracted at
/// the `from` boundary commit and the state extracted at the `to` boundary
/// commit.
///
/// Both node lists are compared by `qualified_name`; duplicate qualified
/// names within one list are collapsed to the first occurrence.
pub fn compute_node_change_set(from_nodes: &[GraphNode], to_nodes: &[GraphNode]) -> NodeChangeSet {
    let mut from_map: std::collections::HashMap<&str, &GraphNode> =
        std::collections::HashMap::with_capacity(from_nodes.len());
    for n in from_nodes {
        from_map.entry(n.qualified_name.as_str()).or_insert(n);
    }
    let mut to_map: std::collections::HashMap<&str, &GraphNode> =
        std::collections::HashMap::with_capacity(to_nodes.len());
    for n in to_nodes {
        to_map.entry(n.qualified_name.as_str()).or_insert(n);
    }

    let mut change_set = NodeChangeSet::default();
    // Stable, deterministic ordering: iterate the input order.
    for n in to_nodes {
        let qn = n.qualified_name.as_str();
        match from_map.get(qn) {
            None => {
                if !change_set.added.contains(&n.qualified_name) {
                    change_set.added.push(n.qualified_name.clone());
                }
            }
            Some(from) => {
                if nodes_differ_substantively(from, n)
                    && !change_set.modified.contains(&n.qualified_name)
                {
                    change_set.modified.push(n.qualified_name.clone());
                }
            }
        }
    }
    for n in from_nodes {
        if !to_map.contains_key(n.qualified_name.as_str())
            && !change_set.removed.contains(&n.qualified_name)
        {
            change_set.removed.push(n.qualified_name.clone());
        }
    }
    change_set
}

/// Whether two versions of the same node (same `qualified_name`) differ in
/// any substantive field.
///
/// Substantive: node type, name, structural location (file path, line
/// range), visibility, doc/signature comment, spec linkage, and metrics.
/// Everything else — ids, repo id, SHA stamps, timestamps, soft-delete
/// markers, approval bookkeeping — differs by construction between two
/// independent extractions and is excluded.
pub(crate) fn nodes_differ_substantively(a: &GraphNode, b: &GraphNode) -> bool {
    normalize_node_type(&a.node_type) != normalize_node_type(&b.node_type)
        || a.name != b.name
        || a.file_path != b.file_path
        || a.line_start != b.line_start
        || a.line_end != b.line_end
        || normalize_visibility(&a.visibility) != normalize_visibility(&b.visibility)
        || a.doc_comment != b.doc_comment
        || a.spec_path != b.spec_path
        || a.spec_paths != b.spec_paths
        || normalize_spec_confidence(&a.spec_confidence)
            != normalize_spec_confidence(&b.spec_confidence)
        || a.complexity != b.complexity
        || a.churn_count_30d != b.churn_count_30d
        || a.test_coverage != b.test_coverage
        || a.test_node != b.test_node
}

fn normalize_node_type(nt: &NodeType) -> String {
    format!("{:?}", nt).to_lowercase()
}

fn normalize_visibility(v: &Visibility) -> String {
    format!("{:?}", v).to_lowercase()
}

fn normalize_spec_confidence(c: &SpecConfidence) -> String {
    format!("{:?}", c).to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;
    use gyre_common::Id;

    fn node(qn: &str) -> GraphNode {
        let (name, _) = qn.rsplit_once("::").unwrap_or((qn, ""));
        GraphNode {
            id: Id::new(uuid::Uuid::new_v4().to_string()),
            repo_id: Id::new("repo-1"),
            node_type: NodeType::Function,
            name: name.to_string(),
            qualified_name: qn.to_string(),
            file_path: "src/lib.rs".to_string(),
            line_start: 0,
            line_end: 0,
            visibility: Visibility::Public,
            doc_comment: None,
            spec_path: None,
            spec_paths: vec![],
            spec_confidence: SpecConfidence::None,
            last_modified_sha: format!("{:0>40}", "a"),
            last_modified_by: None,
            last_modified_at: 1,
            created_sha: format!("{:0>40}", "a"),
            created_at: 1,
            complexity: None,
            churn_count_30d: 0,
            test_coverage: None,
            first_seen_at: 1,
            last_seen_at: 1,
            deleted_at: None,
            test_node: false,
            spec_approved_at: None,
            milestone_completed_at: None,
        }
    }

    #[test]
    fn added_removed_modified_classification() {
        let from = vec![node("c::keep"), node("c::gone"), node("c::changed")];
        let mut changed = node("c::changed");
        changed.doc_comment = Some("fn changed(x: u64)".to_string());
        let to = vec![node("c::keep"), changed, node("c::new")];

        let cs = compute_node_change_set(&from, &to);
        assert_eq!(cs.added, vec!["c::new".to_string()]);
        assert_eq!(cs.removed, vec!["c::gone".to_string()]);
        assert_eq!(cs.modified, vec!["c::changed".to_string()]);
        // matched = added + modified; removed is not matchable.
        let matched: Vec<&str> = cs.matched().collect();
        assert_eq!(matched, vec!["c::new", "c::changed"]);
    }

    #[test]
    fn changed_and_reverted_between_boundaries_is_not_modified() {
        // Boundary-to-boundary semantics: if the node's content is identical
        // at from and to, intermediate commits are invisible — the node is
        // NOT reported changed.
        let from = vec![node("c::f")];
        let to = vec![node("c::f")];
        let cs = compute_node_change_set(&from, &to);
        assert!(cs.is_empty(), "identical boundaries must be empty: {cs:?}");
    }

    #[test]
    fn bookkeeping_only_differences_are_not_modified() {
        // SHA stamps, timestamps, first/last-seen, ids differ by construction
        // between two independent extractions — none of these may count as a
        // content difference.
        let a = node("c::f");
        let mut b = node("c::f");
        b.id = Id::new(uuid::Uuid::new_v4().to_string());
        b.repo_id = Id::new("repo-2");
        b.created_sha = format!("{:0>40}", "b");
        b.last_modified_sha = format!("{:0>40}", "c");
        b.created_at = 999;
        b.last_modified_at = 998;
        b.first_seen_at = 997;
        b.last_seen_at = 996;
        b.last_modified_by = Some(Id::new("agent-1"));
        b.deleted_at = None;

        assert!(!nodes_differ_substantively(&a, &b));
        let cs = compute_node_change_set(&[a], &[b]);
        assert!(
            cs.modified.is_empty(),
            "bookkeeping-only diff must not be modified: {cs:?}"
        );
    }

    #[test]
    fn substantive_field_differences_count_as_modified() {
        // Clippy: a bare fn-pointer pair reads as a complex type — name it.
        type Mutator = fn(&mut GraphNode);
        let cases: Vec<(Mutator, &str)> = vec![
            (
                |n: &mut GraphNode| n.doc_comment = Some("fn f(x: u64)".into()),
                "doc/signature",
            ),
            (|n: &mut GraphNode| n.complexity = Some(7), "complexity"),
            (
                |n: &mut GraphNode| n.test_coverage = Some(0.5),
                "test_coverage",
            ),
            (
                |n: &mut GraphNode| n.spec_path = Some("specs/x.md".into()),
                "spec_path",
            ),
            (
                |n: &mut GraphNode| n.spec_paths = vec!["specs/x.md".into()],
                "spec_paths",
            ),
            (
                |n: &mut GraphNode| n.file_path = "src/other.rs".into(),
                "file_path",
            ),
            (|n: &mut GraphNode| n.line_start = 12, "line_start"),
            (|n: &mut GraphNode| n.line_end = 34, "line_end"),
            (|n: &mut GraphNode| n.churn_count_30d = 3, "churn"),
            (|n: &mut GraphNode| n.test_node = true, "test_node"),
        ];
        for (mutate, label) in cases {
            let mut a = node("c::f");
            let mut b = node("c::f");
            mutate(&mut b);
            assert!(nodes_differ_substantively(&a, &b), "case {label}");
            // Sanity: the same mutation on both sides is not a difference.
            mutate(&mut a);
            assert!(
                !nodes_differ_substantively(&a, &b),
                "case {label} (both mutated)"
            );
        }
    }

    #[test]
    fn duplicate_qualified_names_collapse() {
        let from = vec![node("c::f"), node("c::f")];
        let to = vec![node("c::f")];
        let cs = compute_node_change_set(&from, &to);
        assert!(cs.is_empty());
    }
}
