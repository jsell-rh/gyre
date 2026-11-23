//! Shared deferred-edge resolution for Pass 1 syntax extractors.
//!
//! The Pass 1 extractors walk files one at a time, but a deferred edge's
//! target may live in a file that has not been extracted yet (e.g.
//! `class Admin(User)` where `User` sits in a later file). Extractors
//! therefore collect `(source_id, target_name)` pairs during the pass and
//! resolve them after every file has been extracted, mirroring the Rust
//! extractor's `deferred_returns`/`resolve_returns` pattern
//! (rust_extractor.rs).
//!
//! Resolution contract (same for every extractor):
//! 1. try an exact qualified-name match in the run-scoped index;
//! 2. else try a suffix match — any node whose qualified name ends with
//!    `.{name}` (Python/TypeScript use `.`; Go qnames also use `.`);
//! 3. **no fabricated targets** — a name that resolves to no known node
//!    (stdlib/external imports, primitives, generic markers like `Generic`)
//!    yields no edge.
//!
//! Determinism: HashMap iteration order is randomized per process, so when
//! several nodes share a name suffix the resolver must not take "whatever
//! was hashed first". Candidates that are type-like nodes (Type / Interface /
//! Enum / Trait) win over incidental name collisions, and remaining ties are
//! broken by the lexicographically smallest qualified name — the same input
//! always produces the same edge.

use gyre_common::graph::NodeType;
use gyre_common::Id;
use std::collections::HashMap;

/// Node types preferred as deferred-edge targets when several candidates
/// share a name suffix. `Implements` (class inheritance) and `Returns`
/// (function signatures) semantically point at type declarations, so a
/// same-named function or module must not win the collision.
const PREFERRED_TARGET_TYPES: &[NodeType] = &[
    NodeType::Type,
    NodeType::Interface,
    NodeType::Enum,
    NodeType::Trait,
];

/// Build an `Id → NodeType` lookup from an extractor's node list, for
/// [`resolve_deferred_target`]'s type-preference pass.
pub(crate) fn node_type_index(nodes: &[gyre_common::graph::GraphNode]) -> HashMap<Id, NodeType> {
    nodes
        .iter()
        .map(|n| (n.id.clone(), n.node_type.clone()))
        .collect()
}

/// Resolve a deferred target name against the run-scoped name index.
///
/// `sep` is the qualified-name separator used by the extractor's naming
/// scheme (`.` for Python/TypeScript/Go qualified names).
///
/// Returns `None` when the name resolves to no known node — callers must
/// skip the edge entirely (no fabricated targets).
pub(crate) fn resolve_deferred_target(
    name: &str,
    name_to_id: &HashMap<String, Id>,
    id_to_type: &HashMap<Id, NodeType>,
    sep: char,
) -> Option<Id> {
    if name.is_empty() {
        return None;
    }

    // 1. Exact qualified-name match.
    if let Some(id) = name_to_id.get(name) {
        return Some(id.clone());
    }

    // 2. Suffix match: any node whose qualified name ends with `{sep}{name}`.
    let suffix = format!("{sep}{name}");
    let mut candidates: Vec<(&String, &Id)> = name_to_id
        .iter()
        .filter(|(qname, _)| qname.ends_with(&suffix))
        .collect();
    if candidates.is_empty() {
        return None;
    }

    // Prefer type-like targets (class/interface/enum/trait declarations) over
    // incidental name collisions with functions, modules, or fields.
    let type_like: Vec<(&String, &Id)> = candidates
        .iter()
        .filter(|(_, id)| {
            id_to_type
                .get(*id)
                .is_some_and(|t| PREFERRED_TARGET_TYPES.contains(t))
        })
        .cloned()
        .collect();
    if !type_like.is_empty() {
        candidates = type_like;
    }

    // Deterministic tie-break: lexicographically smallest qualified name.
    candidates.sort_by(|a, b| a.0.cmp(b.0));
    candidates.first().map(|(_, id)| (*id).clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use gyre_common::graph::{GraphNode, NodeType, SpecConfidence, Visibility};

    fn node(id: &str, qname: &str, node_type: NodeType) -> GraphNode {
        GraphNode {
            id: Id::new(id.to_string()),
            repo_id: Id::new(String::new()),
            node_type,
            name: qname.rsplit('.').next().unwrap_or(qname).to_string(),
            qualified_name: qname.to_string(),
            file_path: String::new(),
            line_start: 0,
            line_end: 0,
            visibility: Visibility::Public,
            doc_comment: None,
            spec_path: None,
            spec_paths: vec![],
            spec_confidence: SpecConfidence::None,
            last_modified_sha: String::new(),
            last_modified_by: None,
            last_modified_at: 0,
            created_sha: String::new(),
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

    fn index(nodes: &[GraphNode]) -> (HashMap<String, Id>, HashMap<Id, NodeType>) {
        let name_to_id = nodes
            .iter()
            .map(|n| (n.qualified_name.clone(), n.id.clone()))
            .collect();
        let id_to_type = node_type_index(nodes);
        (name_to_id, id_to_type)
    }

    #[test]
    fn empty_name_resolves_to_nothing() {
        let (name_to_id, id_to_type) = index(&[]);
        assert!(resolve_deferred_target("", &name_to_id, &id_to_type, '.').is_none());
    }

    #[test]
    fn exact_match_wins_before_suffix() {
        let nodes = [node("a", "pkg.User", NodeType::Type)];
        let (name_to_id, id_to_type) = index(&nodes);
        assert_eq!(
            resolve_deferred_target("pkg.User", &name_to_id, &id_to_type, '.'),
            Some(Id::new("a".to_string()))
        );
    }

    #[test]
    fn suffix_match_resolves_name() {
        let nodes = [node("a", "src.models.User", NodeType::Type)];
        let (name_to_id, id_to_type) = index(&nodes);
        assert_eq!(
            resolve_deferred_target("User", &name_to_id, &id_to_type, '.'),
            Some(Id::new("a".to_string()))
        );
    }

    #[test]
    fn unknown_name_resolves_to_nothing() {
        let nodes = [node("a", "src.models.User", NodeType::Type)];
        let (name_to_id, id_to_type) = index(&nodes);
        assert!(resolve_deferred_target("ExternalBase", &name_to_id, &id_to_type, '.').is_none());
    }

    #[test]
    fn type_like_candidate_preferred_over_same_named_function() {
        // A `User` function and a `User` class share the `.User` suffix; the
        // class must win regardless of hash order.
        let nodes = [
            node("fn", "src.handlers.User", NodeType::Function),
            node("ty", "src.models.User", NodeType::Type),
        ];
        let (name_to_id, id_to_type) = index(&nodes);
        assert_eq!(
            resolve_deferred_target("User", &name_to_id, &id_to_type, '.'),
            Some(Id::new("ty".to_string()))
        );
    }

    #[test]
    fn suffix_match_is_deterministic_under_ambiguity() {
        // Two type-like candidates: the lexicographically smallest qualified
        // name must win every run, not whichever HashMap slot came first.
        let nodes = [
            node("b", "src.zeta.User", NodeType::Type),
            node("a", "src.alpha.User", NodeType::Type),
        ];
        let (name_to_id, id_to_type) = index(&nodes);
        assert_eq!(
            resolve_deferred_target("User", &name_to_id, &id_to_type, '.'),
            Some(Id::new("a".to_string()))
        );
    }

    #[test]
    fn partial_suffix_does_not_match() {
        // `UserManager` must not resolve a deferred `User` target.
        let nodes = [node("a", "src.models.UserManager", NodeType::Type)];
        let (name_to_id, id_to_type) = index(&nodes);
        assert!(resolve_deferred_target("User", &name_to_id, &id_to_type, '.').is_none());
    }
}
