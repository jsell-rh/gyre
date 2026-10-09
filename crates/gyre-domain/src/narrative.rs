//! Narrative generation for architectural deltas (specs/system/realized-model.md §6).
//!
//! Two layers, per spec §6:
//! - **Template-based narratives** — deterministic prose grounded in the delta
//!   plus graph context (module membership, implemented traits, fields, spec
//!   governance). This module owns that logic entirely.
//! - **LLM-synthesized narratives** — this module builds the *grounded facts*
//!   (`build_narrative_facts` / `build_llm_user_prompt`) that the server feeds
//!   to the LLM port. The LLM call itself lives in `gyre-server` because
//!   `gyre-domain` MUST NOT depend on the LLM port (see gyre-ports llm.rs).
//!
//! Grounding contract: every clause rendered here is derived from either the
//! delta record's `delta_json` or the supplied [`NarrativeGrounding`] built
//! from live graph nodes/edges. Nothing is invented; unknown context is
//! omitted rather than guessed.

use gyre_common::graph::{ArchitecturalDelta, EdgeType, FieldChange, GraphEdge, GraphNode};
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// Added-node count above which per-node sentences collapse into grouped
/// "N types added to `module`" summaries (§6: "groups of related changes").
pub const GROUP_THRESHOLD: usize = 3;

/// Maximum field names listed before summarizing the remainder.
const MAX_LISTED_FIELDS: usize = 8;

/// Longest rendered field-change value before char-safe truncation.
const MAX_VALUE_CHARS: usize = 60;

/// Graph-derived facts used to ground delta narratives.
///
/// Built once per repo from the live graph (all keyed by qualified name) and
/// reused across deltas. Maps only contain entries that actually exist in the
/// graph — a missing key means the fact is unknown and the narrative omits it.
#[derive(Debug, Clone, Default)]
pub struct NarrativeGrounding {
    /// Child qualified name → qualified name of the module that contains it.
    pub parent: HashMap<String, String>,
    /// Implementing type qualified name → implemented trait/interface names.
    pub implements: HashMap<String, Vec<String>>,
    /// Code node qualified name → spec paths governing it.
    pub specs: HashMap<String, Vec<String>>,
    /// Struct/type qualified name → its field names.
    pub fields: HashMap<String, Vec<String>>,
}

impl NarrativeGrounding {
    /// Index live graph nodes/edges into narrative grounding facts.
    ///
    /// Soft-deleted nodes and edges are excluded. Edge directions follow the
    /// extractor contract: `Contains` parent→child, `Implements` impl→trait,
    /// `FieldOf` field→owner, `GovernedBy` code→spec.
    pub fn from_graph(nodes: &[GraphNode], edges: &[GraphEdge]) -> Self {
        let mut g = NarrativeGrounding::default();
        let live: HashMap<&str, &GraphNode> = nodes
            .iter()
            .filter(|n| n.deleted_at.is_none())
            .map(|n| (n.id.as_str(), n))
            .collect();

        // Node-level spec grounding: spec_path / spec_paths columns.
        for n in nodes.iter().filter(|n| n.deleted_at.is_none()) {
            let mut paths: BTreeSet<String> = BTreeSet::new();
            if let Some(p) = n.spec_path.as_deref() {
                paths.insert(p.to_string());
            }
            for p in &n.spec_paths {
                paths.insert(p.clone());
            }
            if !paths.is_empty() {
                g.specs
                    .entry(n.qualified_name.clone())
                    .or_default()
                    .extend(paths);
            }
        }

        for e in edges.iter().filter(|e| e.deleted_at.is_none()) {
            let (Some(src), Some(tgt)) = (
                live.get(e.source_id.as_str()).copied(),
                live.get(e.target_id.as_str()).copied(),
            ) else {
                continue;
            };
            match e.edge_type {
                EdgeType::Contains => {
                    // parent (module) → child
                    if src.node_type == gyre_common::graph::NodeType::Module {
                        g.parent
                            .insert(tgt.qualified_name.clone(), src.qualified_name.clone());
                    }
                }
                EdgeType::Implements => {
                    g.implements
                        .entry(src.qualified_name.clone())
                        .or_default()
                        .push(tgt.name.clone());
                }
                EdgeType::FieldOf => {
                    // field → owning type
                    g.fields
                        .entry(tgt.qualified_name.clone())
                        .or_default()
                        .push(src.name.clone());
                }
                EdgeType::GovernedBy => {
                    // code node → Spec node (target name is the spec path)
                    let spec = tgt.name.clone();
                    if !spec.is_empty() {
                        g.specs
                            .entry(src.qualified_name.clone())
                            .or_default()
                            .push(spec);
                    }
                }
                _ => {}
            }
        }

        // Deterministic, deduped lists.
        for v in g.implements.values_mut() {
            v.sort();
            v.dedup();
        }
        for v in g.specs.values_mut() {
            v.sort();
            v.dedup();
        }
        for v in g.fields.values_mut() {
            v.sort();
            v.dedup();
        }
        g
    }

    /// Module containing `qualified_name`: grounded module node if known,
    /// otherwise the qualified-name prefix (no prefix → `None`).
    fn module_of<'a>(&'a self, qualified_name: &'a str) -> Option<&'a str> {
        if let Some(m) = self.parent.get(qualified_name) {
            return Some(m.as_str());
        }
        qualified_name.rsplit_once("::").map(|(prefix, _)| prefix)
    }
}

/// Parsed, tolerant view of `delta_json`.
///
/// The extractor stores `nodes_added`/`nodes_removed`/`nodes_modified` either
/// as detail arrays (current format) or as bare counts (legacy/compact
/// format). `None` detail with `Some(count)` means count-only.
#[derive(Debug, Default)]
struct DeltaFacts {
    added: Vec<AddedFact>,
    removed: Vec<String>,
    modified: Vec<ModifiedFact>,
    added_count_only: Option<u64>,
    removed_count_only: Option<u64>,
    modified_count_only: Option<u64>,
    edges_added: u64,
    edges_removed: u64,
}

#[derive(Debug)]
struct AddedFact {
    name: String,
    node_type: String,
    qualified_name: String,
}

#[derive(Debug)]
struct ModifiedFact {
    qualified_name: String,
    field_changes: Vec<FieldChange>,
}

fn parse_delta_facts(delta_json: &str) -> DeltaFacts {
    let Ok(root) = serde_json::from_str::<serde_json::Value>(delta_json) else {
        return DeltaFacts::default();
    };
    let mut f = DeltaFacts::default();

    if let Some(arr) = root.get("nodes_added").and_then(|v| v.as_array()) {
        for entry in arr {
            let name = entry
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let qualified_name = entry
                .get("qualified_name")
                .and_then(|v| v.as_str())
                .unwrap_or(&name)
                .to_string();
            f.added.push(AddedFact {
                name,
                node_type: entry
                    .get("node_type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                qualified_name,
            });
        }
    } else if let Some(n) = root.get("nodes_added").and_then(|v| v.as_u64()) {
        f.added_count_only = Some(n);
    }

    if let Some(arr) = root.get("nodes_removed").and_then(|v| v.as_array()) {
        f.removed = arr
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect();
    } else if let Some(n) = root.get("nodes_removed").and_then(|v| v.as_u64()) {
        f.removed_count_only = Some(n);
    }

    if let Some(arr) = root.get("nodes_modified").and_then(|v| v.as_array()) {
        for entry in arr {
            let qualified_name = entry
                .get("qualified_name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let field_changes = entry
                .get("field_changes")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|fc| {
                            let field = fc.get("field").and_then(|v| v.as_str())?;
                            Some(FieldChange {
                                field: field.to_string(),
                                old_value: fc
                                    .get("old_value")
                                    .and_then(|v| v.as_str())
                                    .map(|s| s.to_string()),
                                new_value: fc
                                    .get("new_value")
                                    .and_then(|v| v.as_str())
                                    .map(|s| s.to_string()),
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();
            if !qualified_name.is_empty() {
                f.modified.push(ModifiedFact {
                    qualified_name,
                    field_changes,
                });
            }
        }
    } else if let Some(n) = root.get("nodes_modified").and_then(|v| v.as_u64()) {
        f.modified_count_only = Some(n);
    }

    f.edges_added = root
        .get("edges_added")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    f.edges_removed = root
        .get("edges_removed")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    f
}

fn name_of(qualified_name: &str) -> &str {
    qualified_name.rsplit_once("::").map(|(_, n)| n).unwrap_or(qualified_name)
}

fn plural(type_name: &str) -> String {
    if let Some(stem) = type_name.strip_suffix('y') {
        format!("{stem}ies")
    } else if type_name.ends_with('s') || type_name.is_empty() {
        format!("{type_name}es")
    } else {
        format!("{type_name}s")
    }
}

fn join_list(items: &[String], word: &str) -> String {
    match items.len() {
        0 => String::new(),
        1 => items[0].clone(),
        2 => format!("{} {word} {}", items[0], items[1]),
        _ => format!(
            "{}, {word} {}",
            items[..items.len() - 1].join(", "),
            items[items.len() - 1]
        ),
    }
}

fn truncate_value(v: &str) -> String {
    let flat: String = v.chars().filter(|c| !c.is_ascii_control()).collect();
    let flat = flat.trim();
    if flat.chars().count() <= MAX_VALUE_CHARS {
        return flat.to_string();
    }
    let cut: String = flat.chars().take(MAX_VALUE_CHARS).collect();
    format!("{cut}…")
}

/// Structured, grounded facts for one delta — the substrate for both the
/// template renderer and the LLM prompt.
fn facts_json(
    delta: &ArchitecturalDelta,
    grounding: &NarrativeGrounding,
    attribution: Option<&str>,
) -> serde_json::Value {
    let f = parse_delta_facts(&delta.delta_json);
    let spec_ref = delta.spec_ref.as_deref().map(|s| s.split('@').next().unwrap_or(s));

    let added: Vec<serde_json::Value> = f
        .added
        .iter()
        .map(|a| {
            let mut o = serde_json::Map::new();
            o.insert("name".into(), a.name.clone().into());
            if !a.node_type.is_empty() {
                o.insert("node_type".into(), a.node_type.clone().into());
            }
            if let Some(m) = grounding.module_of(&a.qualified_name) {
                o.insert("module".into(), m.to_string().into());
            }
            if let Some(t) = grounding.implements.get(&a.qualified_name) {
                if !t.is_empty() {
                    o.insert("implements".into(), t.clone().into());
                }
            }
            if let Some(fl) = grounding.fields.get(&a.qualified_name) {
                if !fl.is_empty() {
                    o.insert("fields".into(), fl.clone().into());
                }
            }
            if let Some(sp) = grounding.specs.get(&a.qualified_name) {
                if !sp.is_empty() {
                    o.insert("specs".into(), sp.clone().into());
                }
            }
            serde_json::Value::Object(o)
        })
        .collect();

    let removed: Vec<serde_json::Value> = f
        .removed
        .iter()
        .map(|qn| {
            let mut o = serde_json::Map::new();
            o.insert("qualified_name".into(), qn.as_str().into());
            if let Some(m) = grounding.module_of(qn) {
                o.insert("module".into(), m.to_string().into());
            }
            serde_json::Value::Object(o)
        })
        .collect();

    let modified: Vec<serde_json::Value> = f
        .modified
        .iter()
        .map(|m| {
            serde_json::json!({
                "qualified_name": m.qualified_name,
                "field_changes": m.field_changes.iter().map(|fc| serde_json::json!({
                    "field": fc.field,
                    "old_value": fc.old_value,
                    "new_value": fc.new_value,
                })).collect::<Vec<_>>(),
            })
        })
        .collect();

    let mut root = serde_json::Map::new();
    root.insert("commit".into(), delta.commit_sha.clone().into());
    if let Some(s) = spec_ref {
        root.insert("spec_ref".into(), s.into());
    }
    if let Some(a) = attribution {
        root.insert("produced_by".into(), a.into());
    }
    if !added.is_empty() {
        root.insert("nodes_added".into(), added.into());
    }
    if let Some(n) = f.added_count_only {
        root.insert("nodes_added_count".into(), n.into());
    }
    if !removed.is_empty() {
        root.insert("nodes_removed".into(), removed.into());
    }
    if let Some(n) = f.removed_count_only {
        root.insert("nodes_removed_count".into(), n.into());
    }
    if !modified.is_empty() {
        root.insert("nodes_modified".into(), modified.into());
    }
    if let Some(n) = f.modified_count_only {
        root.insert("nodes_modified_count".into(), n.into());
    }
    if f.edges_added > 0 {
        root.insert("edges_added".into(), f.edges_added.into());
    }
    if f.edges_removed > 0 {
        root.insert("edges_removed".into(), f.edges_removed.into());
    }
    serde_json::Value::Object(root)
}

/// Render a deterministic, fully-grounded narrative for one architectural
/// delta.
///
/// - `attribution` is the caller-resolved provenance label (e.g.
///   `"agent worker-12 under persona backend-dev"`); `None` omits the
///   attribution sentence.
///
/// Returns `""` for deltas with no renderable facts.
pub fn generate_template_narrative(
    delta: &ArchitecturalDelta,
    grounding: &NarrativeGrounding,
    attribution: Option<&str>,
) -> String {
    let f = parse_delta_facts(&delta.delta_json);
    let mut sentences: Vec<String> = Vec::new();
    let mut specs_mentioned: BTreeSet<String> = BTreeSet::new();

    // ── Additions ────────────────────────────────────────────────────────
    if f.added.len() > GROUP_THRESHOLD {
        // Group by (module, node_type); many related changes read as summaries.
        let mut groups: BTreeMap<(Option<String>, String), usize> = BTreeMap::new();
        for a in &f.added {
            *groups
                .entry((
                    grounding.module_of(&a.qualified_name).map(str::to_string),
                    a.node_type.clone(),
                ))
                .or_default() += 1;
        }
        for ((module, node_type), n) in groups {
            let type_word = plural(if node_type.is_empty() { "node" } else { &node_type });
            match module {
                Some(m) => sentences.push(format!("{} {type_word} added to module `{m}`.", n)),
                None => sentences.push(format!("{} {type_word} added.", n)),
            }
        }
    } else {
        for a in &f.added {
            let type_word = if a.node_type.is_empty() {
                "node"
            } else {
                &a.node_type
            };
            let mut s = match grounding.module_of(&a.qualified_name) {
                Some(m) => format!("New {type_word} `{}` added to module `{m}`.", a.name),
                None => format!("New {type_word} `{}` added.", a.name),
            };
            if let Some(traits) = grounding.implements.get(&a.qualified_name) {
                if !traits.is_empty() {
                    let quoted: Vec<String> = traits.iter().map(|t| format!("`{t}`")).collect();
                    let word = if traits.len() == 1 { "trait" } else { "traits" };
                    s.push_str(&format!(" Implements {word} {}.", join_list(&quoted, "and")));
                }
            }
            if let Some(fields) = grounding.fields.get(&a.qualified_name) {
                if !fields.is_empty() {
                    let listed: Vec<&String> = fields.iter().take(MAX_LISTED_FIELDS).collect();
                    let names: Vec<String> = listed.iter().map(|f| f.to_string()).collect();
                    let rest = fields.len() - listed.len();
                    let tail = if rest > 0 {
                        format!(", and {rest} more")
                    } else {
                        String::new()
                    };
                    s.push_str(&format!(
                        " {} fields: {}{}.",
                        fields.len(),
                        names.join(", "),
                        tail
                    ));
                }
            }
            if let Some(specs) = grounding.specs.get(&a.qualified_name) {
                for sp in specs {
                    specs_mentioned.insert(sp.clone());
                }
            }
            sentences.push(s);
        }
    }
    if let Some(n) = f.added_count_only {
        sentences.push(format!("{n} nodes added."));
    }

    // ── Removals ─────────────────────────────────────────────────────────
    for qn in &f.removed {
        let nm = name_of(qn);
        match grounding.module_of(qn) {
            Some(m) if m != qn.as_str() => {
                sentences.push(format!("`{nm}` removed from module `{m}`."))
            }
            _ => sentences.push(format!("`{qn}` removed.")),
        }
    }
    if let Some(n) = f.removed_count_only {
        sentences.push(format!("{n} nodes removed."));
    }

    // ── Modifications ────────────────────────────────────────────────────
    for m in &f.modified {
        let nm = name_of(&m.qualified_name);
        let module = grounding.module_of(&m.qualified_name);
        let head = match module {
            Some(modl) => format!("`{nm}` in `{modl}` modified"),
            None => format!("`{nm}` modified"),
        };
        if m.field_changes.is_empty() {
            sentences.push(format!("{head}."));
            continue;
        }
        let changes: Vec<String> = m
            .field_changes
            .iter()
            .map(|fc| {
                let old = fc.old_value.as_deref().unwrap_or("unset");
                let new = fc.new_value.as_deref().unwrap_or("unset");
                format!("{}: {} → {}", fc.field, truncate_value(old), truncate_value(new))
            })
            .collect();
        sentences.push(format!("{head} — {}.", changes.join("; ")));
    }
    if let Some(n) = f.modified_count_only {
        sentences.push(format!("{n} nodes modified."));
    }

    // ── Relationships ────────────────────────────────────────────────────
    if f.edges_added > 0 {
        sentences.push(format!(
            "{} new relationship{} established.",
            f.edges_added,
            if f.edges_added == 1 { "" } else { "s" }
        ));
    }
    if f.edges_removed > 0 {
        sentences.push(format!(
            "{} relationship{} removed.",
            f.edges_removed,
            if f.edges_removed == 1 { "" } else { "s" }
        ));
    }

    // ── Commit-level grounding ───────────────────────────────────────────
    if let Some(spec) = delta
        .spec_ref
        .as_deref()
        .map(|s| s.split('@').next().unwrap_or(s))
    {
        specs_mentioned.insert(spec.to_string());
    }
    if !specs_mentioned.is_empty() {
        let list: Vec<String> = specs_mentioned.iter().cloned().collect();
        sentences.push(format!("Governed by spec: {}.", list.join(", ")));
    }
    if let Some(a) = attribution {
        sentences.push(format!("Produced by {a}."));
    }

    sentences.join(" ")
}

/// Structured grounded facts for one delta, exposed for LLM synthesis.
pub fn build_narrative_facts(
    delta: &ArchitecturalDelta,
    grounding: &NarrativeGrounding,
    attribution: Option<&str>,
) -> serde_json::Value {
    facts_json(delta, grounding, attribution)
}

/// User prompt for the LLM-synthesized narrative. The grounded facts JSON is
/// injected into the system template's `{{facts}}` placeholder; the user
/// prompt carries only the generation instruction so echo-based test mocks
/// produce stable output.
pub const LLM_NARRATIVE_USER_PROMPT: &str = "Summarize the architectural delta described in the system prompt as a short plain-prose narrative for a human briefing.";

#[cfg(test)]
mod tests {
    use super::*;
    use gyre_common::graph::{NodeType, SpecConfidence, Visibility};
    use gyre_common::Id;

    fn node(id: &str, ty: NodeType, name: &str, qn: &str) -> GraphNode {
        GraphNode {
            id: Id::new(id),
            repo_id: Id::new("repo-1"),
            node_type: ty,
            name: name.to_string(),
            qualified_name: qn.to_string(),
            file_path: "src/lib.rs".to_string(),
            line_start: 1,
            line_end: 10,
            visibility: Visibility::Public,
            doc_comment: None,
            spec_path: None,
            spec_paths: vec![],
            spec_confidence: SpecConfidence::None,
            last_modified_sha: "abc".to_string(),
            last_modified_by: None,
            last_modified_at: 100,
            created_sha: "abc".to_string(),
            created_at: 100,
            complexity: None,
            churn_count_30d: 0,
            test_coverage: None,
            first_seen_at: 100,
            last_seen_at: 100,
            deleted_at: None,
            test_node: false,
            spec_approved_at: None,
            milestone_completed_at: None,
        }
    }

    fn edge(id: &str, et: EdgeType, src: &str, tgt: &str) -> GraphEdge {
        GraphEdge {
            id: Id::new(id),
            repo_id: Id::new("repo-1"),
            source_id: Id::new(src),
            target_id: Id::new(tgt),
            edge_type: et,
            metadata: None,
            first_seen_at: 100,
            last_seen_at: 100,
            deleted_at: None,
        }
    }

    fn delta(delta_json: &str) -> ArchitecturalDelta {
        ArchitecturalDelta {
            id: Id::new("delta-1"),
            repo_id: Id::new("repo-1"),
            commit_sha: "abc123def".to_string(),
            timestamp: 1000,
            agent_id: None,
            spec_ref: None,
            delta_json: delta_json.to_string(),
        }
    }

    fn grounding_vector_index() -> NarrativeGrounding {
        // Module gyre_domain::search contains type VectorIndex implementing
        // FullTextPort, with 3 fields, governed by search.md.
        let module = node("n-module", NodeType::Module, "search", "gyre_domain::search");
        let mut ty = node(
            "n-type",
            NodeType::Type,
            "VectorIndex",
            "gyre_domain::search::VectorIndex",
        );
        ty.spec_path = Some("specs/system/search.md".to_string());
        let trt = node(
            "n-trait",
            NodeType::Trait,
            "FullTextPort",
            "gyre_domain::search::FullTextPort",
        );
        let f1 = node(
            "n-f1",
            NodeType::Field,
            "embedding_model",
            "gyre_domain::search::VectorIndex::embedding_model",
        );
        let f2 = node(
            "n-f2",
            NodeType::Field,
            "dimension",
            "gyre_domain::search::VectorIndex::dimension",
        );
        let f3 = node(
            "n-f3",
            NodeType::Field,
            "index_path",
            "gyre_domain::search::VectorIndex::index_path",
        );
        let edges = vec![
            edge("e1", EdgeType::Contains, "n-module", "n-type"),
            edge("e2", EdgeType::Implements, "n-type", "n-trait"),
            edge("e3", EdgeType::FieldOf, "n-f1", "n-type"),
            edge("e4", EdgeType::FieldOf, "n-f2", "n-type"),
            edge("e5", EdgeType::FieldOf, "n-f3", "n-type"),
        ];
        NarrativeGrounding::from_graph(
            &[module, ty, trt, f1, f2, f3],
            &edges,
        )
    }

    #[test]
    fn single_addition_renders_grounded_spec_example_shape() {
        let g = grounding_vector_index();
        let d = delta(
            r#"{"nodes_added":[{"name":"VectorIndex","node_type":"type","qualified_name":"gyre_domain::search::VectorIndex"}]}"#,
        );
        let n = generate_template_narrative(&d, &g, None);
        assert!(n.contains("New type `VectorIndex` added to module `gyre_domain::search`."), "{n}");
        assert!(n.contains("Implements trait `FullTextPort`."), "{n}");
        assert!(
            n.contains("3 fields: dimension, embedding_model, index_path."),
            "{n}"
        );
        assert!(n.contains("Governed by spec: specs/system/search.md."), "{n}");
    }

    #[test]
    fn agent_attribution_appended() {
        let g = NarrativeGrounding::default();
        let d = delta(
            r#"{"nodes_added":[{"name":"Foo","node_type":"type","qualified_name":"crate::Foo"}]}"#,
        );
        let n = generate_template_narrative(
            &d,
            &g,
            Some("agent worker-12 under persona backend-dev"),
        );
        assert!(n.contains("Produced by agent worker-12 under persona backend-dev."), "{n}");
    }

    #[test]
    fn many_additions_group_by_module_and_type() {
        let g = NarrativeGrounding::default();
        let entries: Vec<String> = (0..5)
            .map(|i| {
                format!(r#"{{"name":"T{i}","node_type":"type","qualified_name":"crate::search::T{i}"}}"#)
            })
            .collect();
        let d = delta(&format!(r#"{{"nodes_added":[{}]}}"#, entries.join(",")));
        let n = generate_template_narrative(&d, &g, None);
        assert!(n.contains("5 types added to module `crate::search`."), "{n}");
        assert!(!n.contains("New type"), "grouped output omits per-node detail: {n}");
    }

    #[test]
    fn removals_and_modifications_render() {
        let g = NarrativeGrounding::default();
        let d = delta(
            r#"{"nodes_removed":["crate::search::Legacy"],"nodes_modified":[{"qualified_name":"crate::search::VectorIndex","field_changes":[{"field":"complexity","old_value":"3","new_value":"9"}]}]}"#,
        );
        let n = generate_template_narrative(&d, &g, None);
        assert!(n.contains("`Legacy` removed from module `crate::search`."), "{n}");
        assert!(
            n.contains("`VectorIndex` in `crate::search` modified — complexity: 3 → 9."),
            "{n}"
        );
    }

    #[test]
    fn compact_count_only_delta_renders_counts() {
        let g = NarrativeGrounding::default();
        let d = delta(
            r#"{"nodes_extracted":40,"edges_extracted":80,"nodes_added":2,"nodes_removed":1,"nodes_modified":3,"edges_added":5,"edges_removed":2}"#,
        );
        let n = generate_template_narrative(&d, &g, None);
        assert!(n.contains("2 nodes added."), "{n}");
        assert!(n.contains("1 nodes removed."), "{n}");
        assert!(n.contains("3 nodes modified."), "{n}");
        assert!(n.contains("5 new relationships established."), "{n}");
        assert!(n.contains("2 relationships removed."), "{n}");
    }

    #[test]
    fn empty_or_malformed_delta_renders_empty() {
        let g = NarrativeGrounding::default();
        assert_eq!(generate_template_narrative(&delta("{}"), &g, None), "");
        assert_eq!(generate_template_narrative(&delta("not json"), &g, None), "");
        assert_eq!(generate_template_narrative(&delta(""), &g, None), "");
    }

    #[test]
    fn spec_ref_renders_governance_sentence_without_at() {
        let g = NarrativeGrounding::default();
        let mut d = delta(
            r#"{"nodes_added":[{"name":"Foo","node_type":"type","qualified_name":"crate::Foo"}]}"#,
        );
        d.spec_ref = Some("specs/system/search.md@deadbeef".to_string());
        let n = generate_template_narrative(&d, &g, None);
        assert!(n.contains("Governed by spec: specs/system/search.md."), "{n}");
        assert!(!n.contains('@'), "sha suffix stripped: {n}");
    }

    #[test]
    fn deleted_nodes_and_edges_excluded_from_grounding() {
        let mut module = node("n-module", NodeType::Module, "search", "crate::search");
        module.deleted_at = Some(1);
        let ty = node("n-type", NodeType::Type, "Foo", "crate::search::Foo");
        let edges = vec![
            edge("e1", EdgeType::Contains, "n-module", "n-type"),
            edge("e2", EdgeType::Implements, "n-type", "n-missing"),
        ];
        let g = NarrativeGrounding::from_graph(&[module, ty], &edges);
        // Deleted module must not ground the parent…
        assert!(g.parent.is_empty());
        // …and edges to missing nodes must not create facts.
        assert!(g.implements.is_empty());
    }

    #[test]
    fn governed_by_edge_adds_spec_fact() {
        let ty = node("n-type", NodeType::Type, "Foo", "crate::Foo");
        let spec = node("n-spec", NodeType::Spec, "specs/system/foo.md", "specs/system/foo.md");
        let g = NarrativeGrounding::from_graph(
            &[ty, spec],
            &[edge("e1", EdgeType::GovernedBy, "n-type", "n-spec")],
        );
        assert_eq!(
            g.specs.get("crate::Foo").map(|v| v.as_slice()),
            Some(["specs/system/foo.md".to_string()].as_slice())
        );
    }

    #[test]
    fn facts_json_carries_grounding() {
        let g = grounding_vector_index();
        let d = delta(
            r#"{"nodes_added":[{"name":"VectorIndex","node_type":"type","qualified_name":"gyre_domain::search::VectorIndex"}],"edges_added":1}"#,
        );
        let facts = build_narrative_facts(&d, &g, Some("agent a-1"));
        assert_eq!(facts["commit"], "abc123def");
        assert_eq!(facts["produced_by"], "agent a-1");
        assert_eq!(facts["edges_added"], 1);
        assert_eq!(facts["nodes_added"][0]["module"], "gyre_domain::search");
        assert_eq!(facts["nodes_added"][0]["implements"][0], "FullTextPort");
        assert_eq!(facts["nodes_added"][0]["specs"][0], "specs/system/search.md");
        assert_eq!(facts["nodes_added"][0]["fields"].as_array().unwrap().len(), 3);
    }

    #[test]
    fn long_field_change_values_truncated_at_char_boundary() {
        let g = NarrativeGrounding::default();
        let long = "é".repeat(120);
        let d = delta(&format!(
            r#"{{"nodes_modified":[{{"qualified_name":"crate::Foo","field_changes":[{{"field":"doc_comment","old_value":null,"new_value":"{long}"}}]}}]}}"#
        ));
        let n = generate_template_narrative(&d, &g, None);
        assert!(n.contains('…'), "truncated with ellipsis: {n}");
        assert!(!n.contains(&long), "full value not embedded: {n}");
    }
}
