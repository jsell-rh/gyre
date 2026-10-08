//! Executable spec assertion parser and evaluator (system-explorer spec S9).
//!
//! Specs can contain structural assertions embedded as HTML comments, in two
//! accepted forms.
//!
//! Attribute form (system-explorer.md §9 — `type`/`from`/`to`, `subject`/`trait`,
//! `node_type`/`property` parameters):
//!
//! ```markdown
//! <!-- gyre:assert type="no_dependency" from="gyre-domain" to="gyre-adapters" -->
//! <!-- gyre:assert type="implements" subject="SearchService" trait="FullTextPort" -->
//! <!-- gyre:assert type="all_have" node_type="Endpoint" property="auth_middleware" -->
//! ```
//!
//! Predicate form (the richer call syntax used by earlier slices):
//!
//! ```markdown
//! <!-- gyre:assert module("gyre-domain") NOT depends_on("gyre-adapters") -->
//! <!-- gyre:assert type("TaskPort") has_implementors >= 1 -->
//! <!-- gyre:assert endpoint("/api/v1/specs") governed_by("specs/system/spec-lifecycle.md") -->
//! ```
//!
//! This module parses those comments from markdown content and evaluates them
//! against a knowledge graph (Vec<GraphNode> + Vec<GraphEdge>).

use gyre_common::graph::{EdgeType, GraphEdge, GraphNode, NodeType};
use serde::{Deserialize, Serialize};

// ── Public types ────────────────────────────────────────────────────────────

/// A parsed assertion extracted from a spec's markdown content.
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedAssertion {
    /// 1-based line number where the assertion was found.
    pub line: usize,
    /// The raw assertion text (everything between `gyre:assert` and `-->`).
    pub assertion_text: String,
    /// The parsed subject of the assertion.
    pub subject: Subject,
    /// The predicate to evaluate.
    pub predicate: Predicate,
}

/// The subject of an assertion -- the entity being tested.
#[derive(Debug, Clone, PartialEq)]
pub enum Subject {
    /// `module("name")` -- matches NodeType::Module by name.
    Module(String),
    /// `type("name")` -- matches NodeType::Type or NodeType::Interface by name.
    Type(String),
    /// `endpoint("path")` -- matches NodeType::Endpoint by name.
    Endpoint(String),
    /// `function("name")` -- matches NodeType::Function by name.
    Function(String),
    /// `all_have node_type="X"` -- every node of the given type (by
    /// `NodeType` string name, e.g. "Endpoint") is the subject.
    NodesOfType(NodeType),
}

/// The predicate portion of an assertion.
#[derive(Debug, Clone, PartialEq)]
pub enum Predicate {
    /// `depends_on("target")` -- subject has a DependsOn edge to target.
    DependsOn(String),
    /// `NOT depends_on("target")` -- subject must NOT have a DependsOn edge to target.
    NotDependsOn(String),
    /// `has_implementors >= N` -- subject must have at least N Implements edges targeting it.
    HasImplementors(Comparison, usize),
    /// `governed_by("path")` -- subject's spec_path matches path.
    GovernedBy(String),
    /// `calls("target")` -- subject has a Calls edge to target.
    Calls(String),
    /// `NOT calls("target")` -- subject must NOT have a Calls edge to target.
    NotCalls(String),
    /// `test_coverage >= 0.8` -- subject's test_coverage meets threshold.
    TestCoverage(Comparison, f64),
    /// `complexity <= 20` -- subject's complexity meets threshold.
    Complexity(Comparison, f64),
    /// `churn <= 10` -- subject's churn_count_30d meets threshold.
    Churn(Comparison, f64),
    /// `field_count <= 15` -- number of FieldOf edges meets threshold.
    FieldCount(Comparison, usize),
    /// `implements "Trait"` (attribute form `type="implements" subject="S"
    /// trait="T"`) -- the subject node has an Implements edge to the named
    /// trait.
    Implements(String),
    /// `all_have property="p"` (attribute form `type="all_have"
    /// node_type="X" property="p"`) -- every subject node has a non-null
    /// value for the named property.
    HasProperty(String),
}

/// Comparison operator for numeric predicates.
#[derive(Debug, Clone, PartialEq)]
pub enum Comparison {
    Gte, // >=
    Gt,  // >
    Lte, // <=
    Lt,  // <
    Eq,  // ==
}

/// Result of evaluating a single assertion against the knowledge graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssertionResult {
    /// 1-based line number where the assertion was found.
    pub line: usize,
    /// The raw assertion text.
    pub assertion_text: String,
    /// Whether the assertion passed.
    pub passed: bool,
    /// Human-readable explanation of the result.
    pub explanation: String,
}

impl ParsedAssertion {
    /// The assertion type name as used by the attribute form
    /// (`no_dependency` / `implements` / `all_have`) or the predicate-form
    /// predicate keyword (`not_depends_on`, `has_implementors`, ...).
    pub fn type_name(&self) -> &'static str {
        self.predicate.type_name()
    }

    /// The assertion parameters as a JSON object (`from`/`to`,
    /// `subject`/`trait`, `node_type`/`property`, or the predicate-form
    /// equivalents), for persistence and API responses.
    pub fn params_json(&self) -> serde_json::Value {
        let mut params = match &self.subject {
            Subject::Module(name) => serde_json::json!({ "module": name }),
            Subject::Type(name) => serde_json::json!({ "type": name }),
            Subject::Endpoint(path) => serde_json::json!({ "endpoint": path }),
            Subject::Function(name) => serde_json::json!({ "function": name }),
            Subject::NodesOfType(node_type) => {
                serde_json::json!({ "node_type": node_type_str(node_type) })
            }
        };
        if let (Some(k), Some(v)) = self.predicate.params() {
            params[k] = v;
        }
        params
    }
}

impl Predicate {
    /// The canonical assertion type name for this predicate.
    pub fn type_name(&self) -> &'static str {
        match self {
            Predicate::DependsOn(_) => "depends_on",
            Predicate::NotDependsOn(_) => "no_dependency",
            Predicate::HasImplementors(_, _) => "has_implementors",
            Predicate::GovernedBy(_) => "governed_by",
            Predicate::Calls(_) => "calls",
            Predicate::NotCalls(_) => "not_calls",
            Predicate::TestCoverage(_, _) => "test_coverage",
            Predicate::Complexity(_, _) => "complexity",
            Predicate::Churn(_, _) => "churn",
            Predicate::FieldCount(_, _) => "field_count",
            Predicate::Implements(_) => "implements",
            Predicate::HasProperty(_) => "all_have",
        }
    }

    /// The predicate-specific parameter, as a (key, value) pair merged over
    /// the subject parameters by [`ParsedAssertion::params_json`].
    fn params(&self) -> (Option<&'static str>, Option<serde_json::Value>) {
        match self {
            Predicate::DependsOn(t) | Predicate::NotDependsOn(t) => {
                (Some("to"), Some(serde_json::json!(t)))
            }
            Predicate::HasImplementors(cmp, n) => (
                Some("count"),
                Some(serde_json::json!(format!("{} {}", comparison_str(cmp), n))),
            ),
            Predicate::GovernedBy(p) => (Some("path"), Some(serde_json::json!(p))),
            Predicate::Calls(t) | Predicate::NotCalls(t) => {
                (Some("target"), Some(serde_json::json!(t)))
            }
            Predicate::TestCoverage(cmp, t)
            | Predicate::Complexity(cmp, t)
            | Predicate::Churn(cmp, t) => (
                Some("threshold"),
                Some(serde_json::json!(format!("{} {}", comparison_str(cmp), t))),
            ),
            Predicate::FieldCount(cmp, n) => (
                Some("count"),
                Some(serde_json::json!(format!("{} {}", comparison_str(cmp), n))),
            ),
            Predicate::Implements(t) => (Some("trait"), Some(serde_json::json!(t))),
            Predicate::HasProperty(p) => (Some("property"), Some(serde_json::json!(p))),
        }
    }
}

fn node_type_str(node_type: &NodeType) -> &'static str {
    match node_type {
        NodeType::Package => "Package",
        NodeType::Module => "Module",
        NodeType::Type => "Type",
        NodeType::Trait => "Trait",
        NodeType::Interface => "Interface",
        NodeType::Function => "Function",
        NodeType::Method => "Method",
        NodeType::Class => "Class",
        NodeType::Enum => "Enum",
        NodeType::EnumVariant => "EnumVariant",
        NodeType::Endpoint => "Endpoint",
        NodeType::Component => "Component",
        NodeType::Table => "Table",
        NodeType::Constant => "Constant",
        NodeType::Field => "Field",
        NodeType::Spec => "Spec",
    }
}

/// A persisted assertion-check result for one spec, produced by the post-push
/// knowledge-graph check (system-explorer.md §9).
///
/// One record per assertion, keyed by (repo_id, spec_path, line) — the latest
/// push's result replaces any earlier one for that assertion.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpecAssertionResult {
    pub id: String,
    pub repo_id: String,
    /// Repo-relative spec path, e.g. `specs/system/architecture.md`.
    pub spec_path: String,
    /// 1-based line of the `<!-- gyre:assert ... -->` comment in the spec.
    pub line: usize,
    /// Canonical assertion type (`no_dependency`, `implements`, `all_have`, ...).
    pub assertion_type: String,
    /// Raw assertion text (between `gyre:assert` and `-->`).
    pub assertion_text: String,
    /// Assertion parameters as a JSON object string.
    pub params_json: String,
    pub passed: bool,
    pub explanation: String,
    /// Commit SHA the knowledge graph was extracted from when checked.
    pub commit_sha: String,
    /// Unix epoch seconds when the check ran.
    pub checked_at: u64,
}

// ── Parsing ─────────────────────────────────────────────────────────────────

/// Parse all `<!-- gyre:assert ... -->` comments from markdown content.
pub fn parse_assertions(content: &str) -> Vec<ParsedAssertion> {
    let mut results = Vec::new();

    for (line_idx, line) in content.lines().enumerate() {
        let trimmed = line.trim();

        // Look for <!-- gyre:assert ... -->
        if let Some(rest) = strip_assertion_comment(trimmed) {
            if let Some(assertion) = parse_single_assertion(rest, line_idx + 1) {
                results.push(assertion);
            }
        }
    }

    results
}

/// Strip the `<!-- gyre:assert` prefix and `-->` suffix, returning the inner text.
fn strip_assertion_comment(s: &str) -> Option<&str> {
    let s = s.strip_prefix("<!--")?;
    let s = s.strip_suffix("-->")?;
    let s = s.trim();
    let s = s.strip_prefix("gyre:assert")?;
    Some(s.trim())
}

/// Parse a single assertion body.
///
/// Two forms are accepted:
/// - Attribute form: `type="no_dependency" from="gyre-domain" to="gyre-adapters"`
///   (system-explorer.md §9).
/// - Predicate form: `module("gyre-domain") NOT depends_on("gyre-adapters")`.
fn parse_single_assertion(text: &str, line: usize) -> Option<ParsedAssertion> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }

    let assertion_text = text.to_string();

    // Attribute form starts with `type="..."` — try it first.
    if text.starts_with("type") {
        if let Some(parsed) = parse_attribute_assertion(text, line) {
            return Some(parsed);
        }
        // A predicate-form subject can never be named `type="..."` (the
        // predicate form uses `type("Name")` with parens), so a `type="`
        // prefix that fails attribute parsing is a malformed assertion.
        if text.starts_with("type=\"") {
            return None;
        }
    }

    // Predicate form: subject_kind("name") PREDICATE...
    let (subject, rest) = parse_subject(text)?;

    let rest = rest.trim();

    // Parse predicate
    let predicate = parse_predicate(rest)?;

    Some(ParsedAssertion {
        line,
        assertion_text,
        subject,
        predicate,
    })
}

/// Parse an attribute-form assertion body:
/// `type="no_dependency" from="A" to="B"`,
/// `type="implements" subject="S" trait="T"`,
/// `type="all_have" node_type="X" property="P"`.
fn parse_attribute_assertion(text: &str, line: usize) -> Option<ParsedAssertion> {
    let attrs = parse_attributes(text)?;

    let assertion_type = attrs.get("type")?;
    let (subject, predicate) = match assertion_type.as_str() {
        "no_dependency" => {
            let from = attrs.get("from")?;
            let to = attrs.get("to")?;
            // `from` names a module/package crate; `to` the dependency that
            // must not exist.
            (Subject::Module(from.clone()), Predicate::NotDependsOn(to.clone()))
        }
        "implements" => {
            let subject_name = attrs.get("subject")?;
            let trait_name = attrs.get("trait")?;
            (Subject::Type(subject_name.clone()), Predicate::Implements(trait_name.clone()))
        }
        "all_have" => {
            let node_type = attrs.get("node_type")?;
            let property = attrs.get("property")?;
            let node_type = NodeType::from_str_name(&node_type)?;
            (Subject::NodesOfType(node_type), Predicate::HasProperty(property.clone()))
        }
        _ => return None,
    };

    Some(ParsedAssertion {
        line,
        assertion_text: text.to_string(),
        subject,
        predicate,
    })
}

/// Parse whitespace-separated `key="value"` attribute pairs.
/// Returns None on malformed input (missing quote, dangling key).
fn parse_attributes(text: &str) -> Option<std::collections::HashMap<String, String>> {
    let mut attrs = std::collections::HashMap::new();
    let mut rest = text.trim();

    while !rest.is_empty() {
        // key
        let eq = rest.find('=')?;
        let key = rest[..eq].trim();
        if key.is_empty() || !key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return None;
        }
        rest = rest[eq + 1..].trim_start();

        // "value"
        rest = rest.strip_prefix('"')?;
        let end_quote = rest.find('"')?;
        // No byte-slice truncation risk: positions come from `find` on ASCII
        // delimiters, so they are char boundaries.
        let value = rest[..end_quote].to_string();
        rest = rest[end_quote + 1..].trim_start();

        attrs.insert(key.to_string(), value);
    }

    Some(attrs)
}

/// Parse a subject like `module("gyre-domain")` and return the subject + remaining text.
fn parse_subject(text: &str) -> Option<(Subject, &str)> {
    // Try each subject type — must match exact keyword followed by `(`, not a longer word.
    for (prefix, constructor) in &[
        ("module", Subject::Module as fn(String) -> Subject),
        ("type", Subject::Type as fn(String) -> Subject),
        ("endpoint", Subject::Endpoint as fn(String) -> Subject),
        ("function", Subject::Function as fn(String) -> Subject),
    ] {
        if let Some(rest) = text.strip_prefix(prefix) {
            // Ensure the prefix is a complete word: next char must be '(' or whitespace
            // (not an alphanumeric continuation like "typeForTask").
            let first_non_ws = rest.trim_start();
            if !first_non_ws.starts_with('(') {
                continue;
            }
            let rest = rest.trim();
            if let Some((arg, remainder)) = parse_quoted_arg(rest) {
                return Some((constructor(arg), remainder));
            }
        }
    }
    None
}

/// Parse `("value") rest...` and return (value, rest).
fn parse_quoted_arg(text: &str) -> Option<(String, &str)> {
    let text = text.strip_prefix('(')?.trim();
    let text = text.strip_prefix('"')?;

    let end_quote = text.find('"')?;
    let value = text[..end_quote].to_string();
    let rest = text[end_quote + 1..].trim();
    let rest = rest.strip_prefix(')')?.trim();
    Some((value, rest))
}

/// Parse the predicate portion of an assertion.
fn parse_predicate(text: &str) -> Option<Predicate> {
    let text = text.trim();

    // Check for NOT prefix
    if let Some(rest) = text.strip_prefix("NOT").map(|s| s.trim_start()) {
        // NOT depends_on("target")
        if let Some(rest2) = rest.strip_prefix("depends_on") {
            let rest2 = rest2.trim();
            let (target, _) = parse_quoted_arg(rest2)?;
            return Some(Predicate::NotDependsOn(target));
        }
        // NOT calls("target")
        if let Some(rest2) = rest.strip_prefix("calls") {
            let rest2 = rest2.trim();
            let (target, _) = parse_quoted_arg(rest2)?;
            return Some(Predicate::NotCalls(target));
        }
        return None;
    }

    // depends_on("target")
    if let Some(rest) = text.strip_prefix("depends_on") {
        let rest = rest.trim();
        let (target, _) = parse_quoted_arg(rest)?;
        return Some(Predicate::DependsOn(target));
    }

    // has_implementors >= N
    if let Some(rest) = text.strip_prefix("has_implementors") {
        let rest = rest.trim();
        let (cmp, rest) = parse_comparison(rest)?;
        let n: usize = rest.trim().parse().ok()?;
        return Some(Predicate::HasImplementors(cmp, n));
    }

    // governed_by("path")
    if let Some(rest) = text.strip_prefix("governed_by") {
        let rest = rest.trim();
        let (path, _) = parse_quoted_arg(rest)?;
        return Some(Predicate::GovernedBy(path));
    }

    // calls("target")
    if let Some(rest) = text.strip_prefix("calls") {
        let rest = rest.trim();
        let (target, _) = parse_quoted_arg(rest)?;
        return Some(Predicate::Calls(target));
    }

    // test_coverage >= 0.8
    if let Some(rest) = text.strip_prefix("test_coverage") {
        let rest = rest.trim();
        let (cmp, rest) = parse_comparison(rest)?;
        let n: f64 = rest.trim().parse().ok()?;
        return Some(Predicate::TestCoverage(cmp, n));
    }

    // complexity <= 20
    if let Some(rest) = text.strip_prefix("complexity") {
        let rest = rest.trim();
        let (cmp, rest) = parse_comparison(rest)?;
        let n: f64 = rest.trim().parse().ok()?;
        return Some(Predicate::Complexity(cmp, n));
    }

    // churn <= 10
    if let Some(rest) = text.strip_prefix("churn") {
        let rest = rest.trim();
        let (cmp, rest) = parse_comparison(rest)?;
        let n: f64 = rest.trim().parse().ok()?;
        return Some(Predicate::Churn(cmp, n));
    }

    // field_count <= 15
    if let Some(rest) = text.strip_prefix("field_count") {
        let rest = rest.trim();
        let (cmp, rest) = parse_comparison(rest)?;
        let n: usize = rest.trim().parse().ok()?;
        return Some(Predicate::FieldCount(cmp, n));
    }

    None
}

/// Parse a comparison operator like `>=`, `>`, `<=`, `<`, `==` and return it + remaining text.
fn parse_comparison(text: &str) -> Option<(Comparison, &str)> {
    let text = text.trim();
    if let Some(rest) = text.strip_prefix(">=") {
        Some((Comparison::Gte, rest))
    } else if let Some(rest) = text.strip_prefix('>') {
        Some((Comparison::Gt, rest))
    } else if let Some(rest) = text.strip_prefix("<=") {
        Some((Comparison::Lte, rest))
    } else if let Some(rest) = text.strip_prefix('<') {
        Some((Comparison::Lt, rest))
    } else if let Some(rest) = text.strip_prefix("==") {
        Some((Comparison::Eq, rest))
    } else {
        None
    }
}

// ── Evaluation ──────────────────────────────────────────────────────────────

/// Evaluate a list of parsed assertions against the knowledge graph.
pub fn evaluate_assertions(
    assertions: &[ParsedAssertion],
    nodes: &[GraphNode],
    edges: &[GraphEdge],
) -> Vec<AssertionResult> {
    assertions
        .iter()
        .map(|a| evaluate_one(a, nodes, edges))
        .collect()
}

fn evaluate_one(
    assertion: &ParsedAssertion,
    nodes: &[GraphNode],
    edges: &[GraphEdge],
) -> AssertionResult {
    let base = AssertionResult {
        line: assertion.line,
        assertion_text: assertion.assertion_text.clone(),
        passed: false,
        explanation: String::new(),
    };

    // Find matching subject nodes
    let subject_nodes = find_subject_nodes(&assertion.subject, nodes);
    if subject_nodes.is_empty() {
        let subject_desc = describe_subject(&assertion.subject);
        return AssertionResult {
            explanation: format!("Subject not found in knowledge graph: {subject_desc}"),
            ..base
        };
    }
    match &assertion.predicate {
        Predicate::Implements(trait_name) => {
            eval_implements(&subject_nodes, trait_name, nodes, edges, base)
        }
        Predicate::HasProperty(property) => {
            eval_has_property(&subject_nodes, property, base)
        }
        Predicate::DependsOn(target) => {
            eval_depends_on(&subject_nodes, target, nodes, edges, base, false)
        }
        Predicate::NotDependsOn(target) => {
            eval_depends_on(&subject_nodes, target, nodes, edges, base, true)
        }
        Predicate::HasImplementors(cmp, n) => {
            eval_has_implementors(&subject_nodes, cmp, *n, edges, base)
        }
        Predicate::GovernedBy(path) => eval_governed_by(&subject_nodes, path, nodes, edges, base),
        Predicate::Calls(target) => eval_calls(&subject_nodes, target, nodes, edges, base, false),
        Predicate::NotCalls(target) => eval_calls(&subject_nodes, target, nodes, edges, base, true),
        Predicate::TestCoverage(cmp, threshold) => eval_numeric_predicate(
            &subject_nodes,
            cmp,
            *threshold,
            "test_coverage",
            base,
            |n| n.test_coverage,
        ),
        Predicate::Complexity(cmp, threshold) => {
            eval_numeric_predicate(&subject_nodes, cmp, *threshold, "complexity", base, |n| {
                n.complexity.map(|c| c as f64)
            })
        }
        Predicate::Churn(cmp, threshold) => {
            eval_numeric_predicate(&subject_nodes, cmp, *threshold, "churn", base, |n| {
                Some(n.churn_count_30d as f64)
            })
        }
        Predicate::FieldCount(cmp, threshold) => {
            let subject_ids: Vec<&str> = subject_nodes.iter().map(|n| n.id.as_str()).collect();
            let count = edges
                .iter()
                .filter(|e| {
                    e.edge_type == EdgeType::FieldOf && subject_ids.contains(&e.target_id.as_str())
                })
                .count();
            let passed = compare_usize(count, cmp, *threshold);
            let cmp_str = comparison_str(cmp);
            AssertionResult {
                passed,
                explanation: format!("field_count is {count} ({cmp_str} {threshold} = {passed})"),
                ..base
            }
        }
    }
}

fn find_subject_nodes<'a>(subject: &Subject, nodes: &'a [GraphNode]) -> Vec<&'a GraphNode> {
    match subject {
        Subject::Module(name) => nodes
            .iter()
            .filter(|n| {
                // Crates surface as Package nodes ("gyre-domain"), file
                // modules as Module nodes — a module subject matches either,
                // by name or qualified name.
                (n.node_type == NodeType::Module || n.node_type == NodeType::Package)
                    && (n.name == *name || n.qualified_name == *name)
            })
            .collect(),
        Subject::Type(name) => nodes
            .iter()
            .filter(|n| {
                (n.node_type == NodeType::Type
                    || n.node_type == NodeType::Trait
                    || n.node_type == NodeType::Interface
                    || n.node_type == NodeType::Class
                    || n.node_type == NodeType::Enum)
                    && (n.name == *name || n.qualified_name == *name)
            })
            .collect(),
        Subject::Endpoint(name) => nodes
            .iter()
            .filter(|n| {
                n.node_type == NodeType::Endpoint && (n.name == *name || n.qualified_name == *name)
            })
            .collect(),
        Subject::Function(name) => nodes
            .iter()
            .filter(|n| {
                n.node_type == NodeType::Function && (n.name == *name || n.qualified_name == *name)
            })
            .collect(),
        Subject::NodesOfType(node_type) => nodes
            .iter()
            .filter(|n| n.node_type == *node_type)
            .collect(),
    }
}

fn describe_subject(subject: &Subject) -> String {
    match subject {
        Subject::Module(n) => format!("module(\"{n}\")"),
        Subject::Type(n) => format!("type(\"{n}\")"),
        Subject::Endpoint(n) => format!("endpoint(\"{n}\")"),
        Subject::Function(n) => format!("function(\"{n}\")"),
        Subject::NodesOfType(t) => format!("nodes of type \"{t:?}\""),
    }
}

/// Find target nodes by name (any node type).
fn find_target_nodes<'a>(target: &str, nodes: &'a [GraphNode]) -> Vec<&'a GraphNode> {
    nodes
        .iter()
        .filter(|n| n.name == target || n.qualified_name == target)
        .collect()
}

/// `implements "Trait"` — the subject node must have an Implements edge to a
/// node named `trait` (attribute form `type="implements"`).
fn eval_implements(
    subject_nodes: &[&GraphNode],
    trait_name: &str,
    nodes: &[GraphNode],
    edges: &[GraphEdge],
    base: AssertionResult,
) -> AssertionResult {
    let trait_nodes = find_target_nodes(trait_name, nodes);
    if trait_nodes.is_empty() {
        return AssertionResult {
            passed: false,
            explanation: format!("Trait \"{trait_name}\" not found in knowledge graph"),
            ..base
        };
    }

    let subject_ids: Vec<&str> = subject_nodes.iter().map(|n| n.id.as_str()).collect();
    let trait_ids: Vec<&str> = trait_nodes.iter().map(|n| n.id.as_str()).collect();

    let has_edge = edges.iter().any(|e| {
        e.edge_type == EdgeType::Implements
            && subject_ids.contains(&e.source_id.as_str())
            && trait_ids.contains(&e.target_id.as_str())
    });

    AssertionResult {
        passed: has_edge,
        explanation: if has_edge {
            format!("Implements edge to \"{trait_name}\" exists")
        } else {
            format!("No Implements edge to \"{trait_name}\" found")
        },
        ..base
    }
}

/// `all_have property="p"` — every subject node must carry a non-null value
/// for the named property (attribute form `type="all_have"`).
fn eval_has_property(
    subject_nodes: &[&GraphNode],
    property: &str,
    base: AssertionResult,
) -> AssertionResult {
    let extract: fn(&GraphNode) -> Option<String> = match property {
        "spec_path" => |n: &GraphNode| n.spec_path.clone(),
        "doc_comment" => |n: &GraphNode| n.doc_comment.clone(),
        "complexity" => |n: &GraphNode| n.complexity.map(|c| c.to_string()),
        "test_coverage" => |n: &GraphNode| n.test_coverage.map(|c| c.to_string()),
        "last_modified_by" => |n: &GraphNode| n.last_modified_by.as_ref().map(|i| i.to_string()),
        "spec_approved_at" => |n: &GraphNode| n.spec_approved_at.map(|c| c.to_string()),
        _ => {
            return AssertionResult {
                passed: false,
                explanation: format!("Unknown property \"{property}\" for all_have assertion"),
                ..base
            };
        }
    };

    let missing: Vec<&str> = subject_nodes
        .iter()
        .filter(|n| extract(n).is_none())
        .map(|n| n.name.as_str())
        .collect();

    AssertionResult {
        passed: missing.is_empty(),
        explanation: if missing.is_empty() {
            format!(
                "All {} node(s) have property \"{property}\"",
                subject_nodes.len()
            )
        } else if missing.len() <= 5 {
            format!(
                "{} of {} node(s) missing property \"{}\": {}",
                missing.len(),
                subject_nodes.len(),
                property,
                missing.join(", ")
            )
        } else {
            format!(
                "{} of {} node(s) missing property \"{}\" (e.g. {})",
                missing.len(),
                subject_nodes.len(),
                property,
                missing[..5].join(", ")
            )
        },
        ..base
    }
}

fn eval_depends_on(
    subject_nodes: &[&GraphNode],
    target: &str,
    nodes: &[GraphNode],
    edges: &[GraphEdge],
    base: AssertionResult,
    negate: bool,
) -> AssertionResult {
    let target_nodes = find_target_nodes(target, nodes);
    if target_nodes.is_empty() && !negate {
        return AssertionResult {
            passed: false,
            explanation: format!("Target \"{target}\" not found in knowledge graph"),
            ..base
        };
    }

    let target_ids: Vec<&str> = target_nodes.iter().map(|n| n.id.as_str()).collect();
    let subject_ids: Vec<&str> = subject_nodes.iter().map(|n| n.id.as_str()).collect();

    let has_edge = edges.iter().any(|e| {
        e.edge_type == EdgeType::DependsOn
            && subject_ids.contains(&e.source_id.as_str())
            && target_ids.contains(&e.target_id.as_str())
    });

    if negate {
        AssertionResult {
            passed: !has_edge,
            explanation: if has_edge {
                format!("Found unwanted DependsOn edge to \"{target}\"")
            } else {
                format!("No DependsOn edge to \"{target}\" (correct)")
            },
            ..base
        }
    } else {
        AssertionResult {
            passed: has_edge,
            explanation: if has_edge {
                format!("DependsOn edge to \"{target}\" exists")
            } else {
                format!("No DependsOn edge to \"{target}\" found")
            },
            ..base
        }
    }
}

fn eval_has_implementors(
    subject_nodes: &[&GraphNode],
    cmp: &Comparison,
    n: usize,
    edges: &[GraphEdge],
    base: AssertionResult,
) -> AssertionResult {
    let subject_ids: Vec<&str> = subject_nodes.iter().map(|n| n.id.as_str()).collect();

    // Count Implements edges where subject is the target (things implement the subject)
    let count = edges
        .iter()
        .filter(|e| {
            e.edge_type == EdgeType::Implements && subject_ids.contains(&e.target_id.as_str())
        })
        .count();

    let passed = match cmp {
        Comparison::Gte => count >= n,
        Comparison::Gt => count > n,
        Comparison::Lte => count <= n,
        Comparison::Lt => count < n,
        Comparison::Eq => count == n,
    };

    let cmp_str = match cmp {
        Comparison::Gte => ">=",
        Comparison::Gt => ">",
        Comparison::Lte => "<=",
        Comparison::Lt => "<",
        Comparison::Eq => "==",
    };

    AssertionResult {
        passed,
        explanation: format!("Found {count} implementors (expected {cmp_str} {n})"),
        ..base
    }
}

fn eval_governed_by(
    subject_nodes: &[&GraphNode],
    path: &str,
    all_nodes: &[GraphNode],
    edges: &[GraphEdge],
    base: AssertionResult,
) -> AssertionResult {
    let lower_path = path.to_lowercase();

    // Check 1: spec_path match (case-insensitive substring, consistent with view_query_resolver)
    let any_governed_by_path = subject_nodes.iter().any(|n| {
        n.spec_path
            .as_deref()
            .map(|sp| sp.to_lowercase().contains(&lower_path))
            .unwrap_or(false)
    });

    // Check 2: GovernedBy edge match — find spec nodes matching the path, then check edges
    let spec_node_ids: std::collections::HashSet<String> = all_nodes
        .iter()
        .filter(|n| {
            n.deleted_at.is_none()
                && n.spec_path
                    .as_ref()
                    .map_or(false, |sp| sp.to_lowercase().contains(&lower_path))
        })
        .map(|n| n.id.to_string())
        .collect();
    let subject_ids: std::collections::HashSet<String> =
        subject_nodes.iter().map(|n| n.id.to_string()).collect();
    let any_governed_by_edge = edges.iter().any(|e| {
        e.deleted_at.is_none()
            && e.edge_type == gyre_common::graph::EdgeType::GovernedBy
            && subject_ids.contains(&e.source_id.to_string())
            && spec_node_ids.contains(&e.target_id.to_string())
    });

    let any_governed = any_governed_by_path || any_governed_by_edge;

    AssertionResult {
        passed: any_governed,
        explanation: if any_governed {
            format!("Subject is governed by \"{path}\"")
        } else {
            let actual: Vec<String> = subject_nodes
                .iter()
                .filter_map(|n| n.spec_path.as_deref().map(|s| format!("\"{s}\"")))
                .collect();
            if actual.is_empty() {
                format!("Subject has no spec_path and no GovernedBy edge (expected \"{path}\")")
            } else {
                format!(
                    "Subject governed by {} (expected \"{path}\")",
                    actual.join(", ")
                )
            }
        },
        ..base
    }
}

fn eval_calls(
    subject_nodes: &[&GraphNode],
    target: &str,
    nodes: &[GraphNode],
    edges: &[GraphEdge],
    base: AssertionResult,
    negate: bool,
) -> AssertionResult {
    let target_nodes = find_target_nodes(target, nodes);
    if target_nodes.is_empty() && !negate {
        return AssertionResult {
            passed: false,
            explanation: format!("Target \"{target}\" not found in knowledge graph"),
            ..base
        };
    }

    let target_ids: Vec<&str> = target_nodes.iter().map(|n| n.id.as_str()).collect();
    let subject_ids: Vec<&str> = subject_nodes.iter().map(|n| n.id.as_str()).collect();

    let has_edge = edges.iter().any(|e| {
        e.edge_type == EdgeType::Calls
            && subject_ids.contains(&e.source_id.as_str())
            && target_ids.contains(&e.target_id.as_str())
    });

    if negate {
        AssertionResult {
            passed: !has_edge,
            explanation: if has_edge {
                format!("Found unwanted Calls edge to \"{target}\"")
            } else {
                format!("No Calls edge to \"{target}\" (correct)")
            },
            ..base
        }
    } else {
        AssertionResult {
            passed: has_edge,
            explanation: if has_edge {
                format!("Calls edge to \"{target}\" exists")
            } else {
                format!("No Calls edge to \"{target}\" found")
            },
            ..base
        }
    }
}

fn comparison_str(cmp: &Comparison) -> &'static str {
    match cmp {
        Comparison::Gte => ">=",
        Comparison::Gt => ">",
        Comparison::Lte => "<=",
        Comparison::Lt => "<",
        Comparison::Eq => "==",
    }
}

fn compare_f64(actual: f64, cmp: &Comparison, threshold: f64) -> bool {
    match cmp {
        Comparison::Gte => actual >= threshold,
        Comparison::Gt => actual > threshold,
        Comparison::Lte => actual <= threshold,
        Comparison::Lt => actual < threshold,
        Comparison::Eq => (actual - threshold).abs() < f64::EPSILON,
    }
}

fn compare_usize(actual: usize, cmp: &Comparison, threshold: usize) -> bool {
    match cmp {
        Comparison::Gte => actual >= threshold,
        Comparison::Gt => actual > threshold,
        Comparison::Lte => actual <= threshold,
        Comparison::Lt => actual < threshold,
        Comparison::Eq => actual == threshold,
    }
}

fn eval_numeric_predicate<F>(
    subject_nodes: &[&GraphNode],
    cmp: &Comparison,
    threshold: f64,
    metric_name: &str,
    base: AssertionResult,
    extract: F,
) -> AssertionResult
where
    F: Fn(&GraphNode) -> Option<f64>,
{
    // Evaluate against first subject node that has the metric
    for n in subject_nodes {
        if let Some(val) = extract(n) {
            let passed = compare_f64(val, cmp, threshold);
            let cmp_str = comparison_str(cmp);
            return AssertionResult {
                passed,
                explanation: format!("{metric_name} is {val} ({cmp_str} {threshold} = {passed})"),
                ..base
            };
        }
    }
    AssertionResult {
        passed: false,
        explanation: format!("No {metric_name} data available for subject"),
        ..base
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use gyre_common::graph::{SpecConfidence, Visibility};
    use gyre_common::Id;

    // ── Parser tests ────────────────────────────────────────────────────────

    #[test]
    fn parse_not_depends_on() {
        let content = r#"# Architecture
Some text here.
<!-- gyre:assert module("gyre-domain") NOT depends_on("gyre-adapters") -->
More text.
"#;
        let assertions = parse_assertions(content);
        assert_eq!(assertions.len(), 1);
        let a = &assertions[0];
        assert_eq!(a.line, 3);
        assert_eq!(a.subject, Subject::Module("gyre-domain".to_string()));
        assert_eq!(
            a.predicate,
            Predicate::NotDependsOn("gyre-adapters".to_string())
        );
    }

    #[test]
    fn parse_has_implementors() {
        let content = r#"<!-- gyre:assert type("TaskPort") has_implementors >= 1 -->"#;
        let assertions = parse_assertions(content);
        assert_eq!(assertions.len(), 1);
        let a = &assertions[0];
        assert_eq!(a.line, 1);
        assert_eq!(a.subject, Subject::Type("TaskPort".to_string()));
        assert_eq!(a.predicate, Predicate::HasImplementors(Comparison::Gte, 1));
    }

    #[test]
    fn parse_governed_by() {
        let content = r#"<!-- gyre:assert endpoint("/api/v1/specs") governed_by("specs/system/spec-lifecycle.md") -->"#;
        let assertions = parse_assertions(content);
        assert_eq!(assertions.len(), 1);
        assert_eq!(
            assertions[0].subject,
            Subject::Endpoint("/api/v1/specs".to_string())
        );
        assert_eq!(
            assertions[0].predicate,
            Predicate::GovernedBy("specs/system/spec-lifecycle.md".to_string())
        );
    }

    #[test]
    fn parse_calls() {
        let content = r#"<!-- gyre:assert function("main") calls("init_server") -->"#;
        let assertions = parse_assertions(content);
        assert_eq!(assertions.len(), 1);
        assert_eq!(
            assertions[0].predicate,
            Predicate::Calls("init_server".to_string())
        );
    }

    #[test]
    fn parse_not_calls() {
        let content = r#"<!-- gyre:assert function("handler") NOT calls("unsafe_fn") -->"#;
        let assertions = parse_assertions(content);
        assert_eq!(assertions.len(), 1);
        assert_eq!(
            assertions[0].predicate,
            Predicate::NotCalls("unsafe_fn".to_string())
        );
    }

    #[test]
    fn parse_depends_on() {
        let content = r#"<!-- gyre:assert module("gyre-server") depends_on("gyre-domain") -->"#;
        let assertions = parse_assertions(content);
        assert_eq!(assertions.len(), 1);
        assert_eq!(
            assertions[0].predicate,
            Predicate::DependsOn("gyre-domain".to_string())
        );
    }

    #[test]
    fn parse_multiple_assertions() {
        let content = r#"# Spec
<!-- gyre:assert module("a") NOT depends_on("b") -->
Some explanation.
<!-- gyre:assert type("Foo") has_implementors >= 2 -->
<!-- gyre:assert endpoint("/health") governed_by("specs/health.md") -->
"#;
        let assertions = parse_assertions(content);
        assert_eq!(assertions.len(), 3);
        assert_eq!(assertions[0].line, 2);
        assert_eq!(assertions[1].line, 4);
        assert_eq!(assertions[2].line, 5);
    }

    #[test]
    fn parse_with_extra_whitespace() {
        let content = r#"<!--   gyre:assert   module( "gyre-domain" )   NOT   depends_on( "gyre-adapters" )   -->"#;
        let assertions = parse_assertions(content);
        assert_eq!(assertions.len(), 1);
        assert_eq!(
            assertions[0].subject,
            Subject::Module("gyre-domain".to_string())
        );
    }

    #[test]
    fn parse_ignores_non_assertion_comments() {
        let content = r#"
<!-- This is a normal comment -->
<!-- gyre:something_else -->
<!-- gyre:assert module("a") NOT depends_on("b") -->
"#;
        let assertions = parse_assertions(content);
        assert_eq!(assertions.len(), 1);
    }

    #[test]
    fn parse_comparison_operators() {
        for (op, expected) in [
            (">=", Comparison::Gte),
            (">", Comparison::Gt),
            ("<=", Comparison::Lte),
            ("<", Comparison::Lt),
            ("==", Comparison::Eq),
        ] {
            let content = format!(r#"<!-- gyre:assert type("X") has_implementors {op} 3 -->"#);
            let assertions = parse_assertions(&content);
            assert_eq!(assertions.len(), 1, "failed for {op}");
            assert_eq!(
                assertions[0].predicate,
                Predicate::HasImplementors(expected, 3),
                "failed for {op}"
            );
        }
    }

    #[test]
    fn parse_empty_content() {
        assert!(parse_assertions("").is_empty());
        assert!(parse_assertions("just text\nno assertions").is_empty());
    }

    // ── Evaluator tests ─────────────────────────────────────────────────────

    fn make_node(id: &str, name: &str, node_type: NodeType) -> GraphNode {
        GraphNode {
            id: Id::new(id),
            repo_id: Id::new("repo-1"),
            node_type,
            name: name.to_string(),
            qualified_name: name.to_string(),
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

    fn make_edge(source_id: &str, target_id: &str, edge_type: EdgeType) -> GraphEdge {
        GraphEdge {
            id: Id::new(&format!("{source_id}-{target_id}")),
            repo_id: Id::new("repo-1"),
            source_id: Id::new(source_id),
            target_id: Id::new(target_id),
            edge_type,
            metadata: None,
            first_seen_at: 0,
            last_seen_at: 0,
            deleted_at: None,
        }
    }

    #[test]
    fn eval_not_depends_on_passes_when_no_edge() {
        let nodes = vec![
            make_node("n1", "gyre-domain", NodeType::Module),
            make_node("n2", "gyre-adapters", NodeType::Module),
        ];
        let edges = vec![]; // No dependency

        let content =
            r#"<!-- gyre:assert module("gyre-domain") NOT depends_on("gyre-adapters") -->"#;
        let assertions = parse_assertions(content);
        let results = evaluate_assertions(&assertions, &nodes, &edges);

        assert_eq!(results.len(), 1);
        assert!(results[0].passed);
        assert!(results[0].explanation.contains("correct"));
    }

    #[test]
    fn eval_not_depends_on_fails_when_edge_exists() {
        let nodes = vec![
            make_node("n1", "gyre-domain", NodeType::Module),
            make_node("n2", "gyre-adapters", NodeType::Module),
        ];
        let edges = vec![make_edge("n1", "n2", EdgeType::DependsOn)];

        let content =
            r#"<!-- gyre:assert module("gyre-domain") NOT depends_on("gyre-adapters") -->"#;
        let assertions = parse_assertions(content);
        let results = evaluate_assertions(&assertions, &nodes, &edges);

        assert_eq!(results.len(), 1);
        assert!(!results[0].passed);
        assert!(results[0].explanation.contains("unwanted"));
    }

    #[test]
    fn eval_depends_on_passes() {
        let nodes = vec![
            make_node("n1", "gyre-server", NodeType::Module),
            make_node("n2", "gyre-domain", NodeType::Module),
        ];
        let edges = vec![make_edge("n1", "n2", EdgeType::DependsOn)];

        let content = r#"<!-- gyre:assert module("gyre-server") depends_on("gyre-domain") -->"#;
        let assertions = parse_assertions(content);
        let results = evaluate_assertions(&assertions, &nodes, &edges);

        assert_eq!(results.len(), 1);
        assert!(results[0].passed);
    }

    #[test]
    fn eval_has_implementors_gte() {
        let nodes = vec![
            make_node("t1", "TaskPort", NodeType::Interface),
            make_node("t2", "SqliteTaskRepo", NodeType::Type),
            make_node("t3", "MemTaskRepo", NodeType::Type),
        ];
        let edges = vec![
            make_edge("t2", "t1", EdgeType::Implements),
            make_edge("t3", "t1", EdgeType::Implements),
        ];

        let content = r#"<!-- gyre:assert type("TaskPort") has_implementors >= 1 -->"#;
        let assertions = parse_assertions(content);
        let results = evaluate_assertions(&assertions, &nodes, &edges);

        assert_eq!(results.len(), 1);
        assert!(results[0].passed);
        assert!(results[0].explanation.contains("2 implementors"));
    }

    #[test]
    fn eval_has_implementors_fails_when_zero() {
        let nodes = vec![make_node("t1", "TaskPort", NodeType::Interface)];
        let edges = vec![];

        let content = r#"<!-- gyre:assert type("TaskPort") has_implementors >= 1 -->"#;
        let assertions = parse_assertions(content);
        let results = evaluate_assertions(&assertions, &nodes, &edges);

        assert_eq!(results.len(), 1);
        assert!(!results[0].passed);
        assert!(results[0].explanation.contains("0 implementors"));
    }

    #[test]
    fn eval_governed_by_passes() {
        let mut node = make_node("e1", "/api/v1/specs", NodeType::Endpoint);
        node.spec_path = Some("specs/system/spec-lifecycle.md".to_string());
        let nodes = vec![node];

        let content = r#"<!-- gyre:assert endpoint("/api/v1/specs") governed_by("specs/system/spec-lifecycle.md") -->"#;
        let assertions = parse_assertions(content);
        let results = evaluate_assertions(&assertions, &nodes, &[]);

        assert_eq!(results.len(), 1);
        assert!(results[0].passed);
    }

    #[test]
    fn eval_governed_by_fails_wrong_spec() {
        let mut node = make_node("e1", "/api/v1/specs", NodeType::Endpoint);
        node.spec_path = Some("specs/other.md".to_string());
        let nodes = vec![node];

        let content = r#"<!-- gyre:assert endpoint("/api/v1/specs") governed_by("specs/system/spec-lifecycle.md") -->"#;
        let assertions = parse_assertions(content);
        let results = evaluate_assertions(&assertions, &nodes, &[]);

        assert_eq!(results.len(), 1);
        assert!(!results[0].passed);
        assert!(results[0].explanation.contains("specs/other.md"));
    }

    #[test]
    fn eval_calls_passes() {
        let nodes = vec![
            make_node("f1", "main", NodeType::Function),
            make_node("f2", "init_server", NodeType::Function),
        ];
        let edges = vec![make_edge("f1", "f2", EdgeType::Calls)];

        let content = r#"<!-- gyre:assert function("main") calls("init_server") -->"#;
        let assertions = parse_assertions(content);
        let results = evaluate_assertions(&assertions, &nodes, &edges);

        assert_eq!(results.len(), 1);
        assert!(results[0].passed);
    }

    #[test]
    fn eval_not_calls_passes() {
        let nodes = vec![
            make_node("f1", "handler", NodeType::Function),
            make_node("f2", "unsafe_fn", NodeType::Function),
        ];
        let edges = vec![]; // No calls edge

        let content = r#"<!-- gyre:assert function("handler") NOT calls("unsafe_fn") -->"#;
        let assertions = parse_assertions(content);
        let results = evaluate_assertions(&assertions, &nodes, &edges);

        assert_eq!(results.len(), 1);
        assert!(results[0].passed);
    }

    #[test]
    fn eval_subject_not_found() {
        let nodes = vec![];
        let edges = vec![];

        let content = r#"<!-- gyre:assert module("nonexistent") NOT depends_on("x") -->"#;
        let assertions = parse_assertions(content);
        let results = evaluate_assertions(&assertions, &nodes, &edges);

        assert_eq!(results.len(), 1);
        assert!(!results[0].passed);
        assert!(results[0].explanation.contains("not found"));
    }

    #[test]
    fn eval_multiple_assertions_mixed_results() {
        let nodes = vec![
            make_node("n1", "gyre-domain", NodeType::Module),
            make_node("n2", "gyre-adapters", NodeType::Module),
            make_node("t1", "TaskPort", NodeType::Interface),
        ];
        let edges = vec![]; // No edges at all

        let content = r#"
<!-- gyre:assert module("gyre-domain") NOT depends_on("gyre-adapters") -->
<!-- gyre:assert type("TaskPort") has_implementors >= 1 -->
"#;
        let assertions = parse_assertions(content);
        let results = evaluate_assertions(&assertions, &nodes, &edges);

        assert_eq!(results.len(), 2);
        assert!(results[0].passed); // NOT depends_on passes (no edge)
        assert!(!results[1].passed); // has_implementors >= 1 fails (0 implementors)
    }

    #[test]
    fn eval_not_depends_on_target_missing_passes() {
        // If the target doesn't exist, NOT depends_on should pass
        let nodes = vec![make_node("n1", "gyre-domain", NodeType::Module)];
        let edges = vec![];

        let content = r#"<!-- gyre:assert module("gyre-domain") NOT depends_on("nonexistent") -->"#;
        let assertions = parse_assertions(content);
        let results = evaluate_assertions(&assertions, &nodes, &edges);

        assert_eq!(results.len(), 1);
        assert!(results[0].passed);
    }

    // ── New predicate tests ────────────────────────────────────────────

    #[test]
    fn parse_test_coverage() {
        let content = r#"<!-- gyre:assert function("handler") test_coverage >= 0.8 -->"#;
        let assertions = parse_assertions(content);
        assert_eq!(assertions.len(), 1);
        assert_eq!(
            assertions[0].predicate,
            Predicate::TestCoverage(Comparison::Gte, 0.8)
        );
    }

    #[test]
    fn parse_complexity() {
        let content = r#"<!-- gyre:assert function("process") complexity <= 20 -->"#;
        let assertions = parse_assertions(content);
        assert_eq!(assertions.len(), 1);
        assert_eq!(
            assertions[0].predicate,
            Predicate::Complexity(Comparison::Lte, 20.0)
        );
    }

    #[test]
    fn parse_churn() {
        let content = r#"<!-- gyre:assert type("Config") churn <= 5 -->"#;
        let assertions = parse_assertions(content);
        assert_eq!(assertions.len(), 1);
        assert_eq!(
            assertions[0].predicate,
            Predicate::Churn(Comparison::Lte, 5.0)
        );
    }

    #[test]
    fn parse_field_count() {
        let content = r#"<!-- gyre:assert type("BigStruct") field_count <= 15 -->"#;
        let assertions = parse_assertions(content);
        assert_eq!(assertions.len(), 1);
        assert_eq!(
            assertions[0].predicate,
            Predicate::FieldCount(Comparison::Lte, 15)
        );
    }

    #[test]
    fn eval_test_coverage_passes() {
        let mut node = make_node("n1", "handler", NodeType::Function);
        node.test_coverage = Some(0.9);
        let nodes = vec![node];
        let edges = vec![];

        let content = r#"<!-- gyre:assert function("handler") test_coverage >= 0.8 -->"#;
        let assertions = parse_assertions(content);
        let results = evaluate_assertions(&assertions, &nodes, &edges);

        assert_eq!(results.len(), 1);
        assert!(
            results[0].passed,
            "Expected pass: {}",
            results[0].explanation
        );
    }

    #[test]
    fn eval_test_coverage_fails() {
        let mut node = make_node("n1", "handler", NodeType::Function);
        node.test_coverage = Some(0.3);
        let nodes = vec![node];
        let edges = vec![];

        let content = r#"<!-- gyre:assert function("handler") test_coverage >= 0.8 -->"#;
        let assertions = parse_assertions(content);
        let results = evaluate_assertions(&assertions, &nodes, &edges);

        assert_eq!(results.len(), 1);
        assert!(!results[0].passed);
    }

    #[test]
    fn eval_complexity_passes() {
        let mut node = make_node("n1", "simple_fn", NodeType::Function);
        node.complexity = Some(5);
        let nodes = vec![node];
        let edges = vec![];

        let content = r#"<!-- gyre:assert function("simple_fn") complexity <= 20 -->"#;
        let assertions = parse_assertions(content);
        let results = evaluate_assertions(&assertions, &nodes, &edges);

        assert_eq!(results.len(), 1);
        assert!(
            results[0].passed,
            "Expected pass: {}",
            results[0].explanation
        );
    }

    #[test]
    fn eval_complexity_fails() {
        let mut node = make_node("n1", "complex_fn", NodeType::Function);
        node.complexity = Some(30);
        let nodes = vec![node];
        let edges = vec![];

        let content = r#"<!-- gyre:assert function("complex_fn") complexity <= 20 -->"#;
        let assertions = parse_assertions(content);
        let results = evaluate_assertions(&assertions, &nodes, &edges);

        assert_eq!(results.len(), 1);
        assert!(!results[0].passed);
    }

    #[test]
    fn eval_churn_passes() {
        let mut node = make_node("n1", "Config", NodeType::Type);
        node.churn_count_30d = 3;
        let nodes = vec![node];
        let edges = vec![];

        let content = r#"<!-- gyre:assert type("Config") churn <= 5 -->"#;
        let assertions = parse_assertions(content);
        let results = evaluate_assertions(&assertions, &nodes, &edges);

        assert_eq!(results.len(), 1);
        assert!(
            results[0].passed,
            "Expected pass: {}",
            results[0].explanation
        );
    }

    #[test]
    fn eval_field_count_passes() {
        let nodes = vec![
            make_node("n1", "SmallStruct", NodeType::Type),
            make_node("f1", "field_a", NodeType::Field),
            make_node("f2", "field_b", NodeType::Field),
        ];
        let edges = vec![
            make_edge("f1", "n1", EdgeType::FieldOf),
            make_edge("f2", "n1", EdgeType::FieldOf),
        ];

        let content = r#"<!-- gyre:assert type("SmallStruct") field_count <= 15 -->"#;
        let assertions = parse_assertions(content);
        let results = evaluate_assertions(&assertions, &nodes, &edges);

        assert_eq!(results.len(), 1);
        assert!(
            results[0].passed,
            "Expected pass: {}",
            results[0].explanation
        );
        assert!(results[0].explanation.contains("2"));
    }

    #[test]
    fn eval_field_count_fails() {
        let mut nodes = vec![make_node("n1", "BigStruct", NodeType::Type)];
        let mut edges = vec![];
        // Add 5 fields, assert field_count <= 3
        for i in 0..5 {
            let fid = format!("f{i}");
            let fname = format!("field_{i}");
            nodes.push(make_node(&fid, &fname, NodeType::Field));
            edges.push(make_edge(&fid, "n1", EdgeType::FieldOf));
        }

        let content = r#"<!-- gyre:assert type("BigStruct") field_count <= 3 -->"#;
        let assertions = parse_assertions(content);
        let results = evaluate_assertions(&assertions, &nodes, &edges);

        assert_eq!(results.len(), 1);
        assert!(!results[0].passed);
    }

    // ── Attribute form (system-explorer.md §9) ──────────────────────────

    #[test]
    fn parse_attribute_no_dependency() {
        let content =
            r#"<!-- gyre:assert type="no_dependency" from="gyre-domain" to="gyre-adapters" -->"#;
        let assertions = parse_assertions(content);
        assert_eq!(assertions.len(), 1);
        let a = &assertions[0];
        assert_eq!(a.line, 1);
        assert_eq!(a.subject, Subject::Module("gyre-domain".to_string()));
        assert_eq!(
            a.predicate,
            Predicate::NotDependsOn("gyre-adapters".to_string())
        );
    }

    #[test]
    fn parse_attribute_implements() {
        let content =
            r#"<!-- gyre:assert type="implements" subject="SearchService" trait="FullTextPort" -->"#;
        let assertions = parse_assertions(content);
        assert_eq!(assertions.len(), 1);
        let a = &assertions[0];
        assert_eq!(a.subject, Subject::Type("SearchService".to_string()));
        assert_eq!(a.predicate, Predicate::Implements("FullTextPort".to_string()));
    }

    #[test]
    fn parse_attribute_all_have() {
        let content =
            r#"<!-- gyre:assert type="all_have" node_type="Endpoint" property="spec_path" -->"#;
        let assertions = parse_assertions(content);
        assert_eq!(assertions.len(), 1);
        let a = &assertions[0];
        assert_eq!(a.subject, Subject::NodesOfType(NodeType::Endpoint));
        assert_eq!(a.predicate, Predicate::HasProperty("spec_path".to_string()));
    }

    #[test]
    fn parse_attribute_all_have_snake_case_node_type() {
        // The snake_case wire name is accepted too (NodeType::from_str_name).
        let content =
            r#"<!-- gyre:assert type="all_have" node_type="endpoint" property="spec_path" -->"#;
        let assertions = parse_assertions(content);
        assert_eq!(assertions.len(), 1);
        assert_eq!(assertions[0].subject, Subject::NodesOfType(NodeType::Endpoint));
    }

    #[test]
    fn parse_attribute_unknown_type_is_skipped() {
        let content = r#"<!-- gyre:assert type="bogus" from="a" to="b" -->"#;
        assert!(parse_assertions(content).is_empty());
    }

    #[test]
    fn parse_attribute_all_have_unknown_node_type_is_skipped() {
        let content =
            r#"<!-- gyre:assert type="all_have" node_type="NotAType" property="p" -->"#;
        assert!(parse_assertions(content).is_empty());
    }

    #[test]
    fn parse_attribute_missing_required_params_is_skipped() {
        // no_dependency requires both from and to.
        assert!(parse_assertions(r#"<!-- gyre:assert type="no_dependency" from="a" -->"#)
            .is_empty());
        // implements requires both subject and trait.
        assert!(parse_assertions(r#"<!-- gyre:assert type="implements" subject="S" -->"#)
            .is_empty());
        // all_have requires both node_type and property.
        assert!(parse_assertions(r#"<!-- gyre:assert type="all_have" node_type="Endpoint" -->"#)
            .is_empty());
    }

    #[test]
    fn parse_attribute_spec_example_block() {
        // The exact §9 example block, embedded in a spec.
        let content = r#"## Invariants

<!-- gyre:assert type="no_dependency" from="gyre-domain" to="gyre-adapters" -->
- `gyre-domain` MUST NOT depend on `gyre-adapters`

<!-- gyre:assert type="implements" subject="SearchService" trait="FullTextPort" -->
- `SearchService` MUST implement `FullTextPort`

<!-- gyre:assert type="all_have" node_type="Endpoint" property="auth_middleware" -->
- All API endpoints MUST have auth middleware
"#;
        let assertions = parse_assertions(content);
        assert_eq!(assertions.len(), 3);
        assert_eq!(assertions[0].line, 3);
        assert_eq!(
            assertions[0].subject,
            Subject::Module("gyre-domain".to_string())
        );
        assert_eq!(
            assertions[1].subject,
            Subject::Type("SearchService".to_string())
        );
        assert_eq!(assertions[2].subject, Subject::NodesOfType(NodeType::Endpoint));
    }

    #[test]
    fn eval_attribute_no_dependency_passes_and_fails() {
        let nodes = vec![
            make_node("n1", "gyre-domain", NodeType::Module),
            make_node("n2", "gyre-adapters", NodeType::Module),
        ];
        let content =
            r#"<!-- gyre:assert type="no_dependency" from="gyre-domain" to="gyre-adapters" -->"#;
        let assertions = parse_assertions(content);
        assert_eq!(assertions.len(), 1);

        // No edge: passes.
        let results = evaluate_assertions(&assertions, &nodes, &[]);
        assert_eq!(results.len(), 1);
        assert!(results[0].passed);

        // Edge exists: fails.
        let edges = vec![make_edge("n1", "n2", EdgeType::DependsOn)];
        let results = evaluate_assertions(&assertions, &nodes, &edges);
        assert_eq!(results.len(), 1);
        assert!(!results[0].passed);
    }

    #[test]
    fn eval_attribute_implements_passes_and_fails() {
        let nodes = vec![
            make_node("s1", "SearchService", NodeType::Type),
            make_node("t1", "FullTextPort", NodeType::Interface),
        ];
        let content =
            r#"<!-- gyre:assert type="implements" subject="SearchService" trait="FullTextPort" -->"#;
        let assertions = parse_assertions(content);
        assert_eq!(assertions.len(), 1);

        // Implements edge present: passes.
        let edges = vec![make_edge("s1", "t1", EdgeType::Implements)];
        let results = evaluate_assertions(&assertions, &nodes, &edges);
        assert_eq!(results.len(), 1);
        assert!(results[0].passed, "should pass: {}", results[0].explanation);

        // No Implements edge: fails.
        let results = evaluate_assertions(&assertions, &nodes, &[]);
        assert_eq!(results.len(), 1);
        assert!(!results[0].passed);
    }

    #[test]
    fn eval_attribute_all_have_passes_and_fails() {
        let mut good = make_node("e1", "/api/v1/specs", NodeType::Endpoint);
        good.spec_path = Some("specs/system/spec-lifecycle.md".to_string());
        let mut bad = make_node("e2", "/api/v1/repos", NodeType::Endpoint);
        bad.spec_path = None;
        let unrelated = make_node("f1", "handler", NodeType::Function);

        let content =
            r#"<!-- gyre:assert type="all_have" node_type="Endpoint" property="spec_path" -->"#;
        let assertions = parse_assertions(content);
        assert_eq!(assertions.len(), 1);

        // All endpoints carry the property: passes (unrelated Function node ignored).
        let nodes = vec![good.clone(), unrelated.clone()];
        let results = evaluate_assertions(&assertions, &nodes, &[]);
        assert_eq!(results.len(), 1);
        assert!(results[0].passed, "should pass: {}", results[0].explanation);

        // One endpoint lacks it: fails, naming the offender.
        let nodes = vec![good, bad, unrelated];
        let results = evaluate_assertions(&assertions, &nodes, &[]);
        assert_eq!(results.len(), 1);
        assert!(!results[0].passed);
        assert!(results[0].explanation.contains("/api/v1/repos"));
    }

    #[test]
    fn eval_attribute_all_have_no_subject_nodes_fails() {
        // Zero nodes of the requested type — the assertion cannot hold
        // vacuously; the subject is not found.
        let nodes = vec![make_node("f1", "handler", NodeType::Function)];
        let content =
            r#"<!-- gyre:assert type="all_have" node_type="Endpoint" property="spec_path" -->"#;
        let assertions = parse_assertions(content);
        let results = evaluate_assertions(&assertions, &nodes, &[]);
        assert_eq!(results.len(), 1);
        assert!(!results[0].passed);
        assert!(results[0].explanation.contains("not found"));
    }

    #[test]
    fn eval_attribute_all_have_unknown_property_fails() {
        let nodes = vec![make_node("e1", "/api/v1/specs", NodeType::Endpoint)];
        let content =
            r#"<!-- gyre:assert type="all_have" node_type="Endpoint" property="auth_middleware" -->"#;
        let assertions = parse_assertions(content);
        let results = evaluate_assertions(&assertions, &nodes, &[]);
        assert_eq!(results.len(), 1);
        assert!(!results[0].passed);
        assert!(results[0].explanation.contains("Unknown property"));
    }

    #[test]
    fn parse_attribute_mixed_with_predicate_form() {
        let content = r#"# Spec
<!-- gyre:assert type="no_dependency" from="a" to="b" -->
<!-- gyre:assert module("a") NOT depends_on("b") -->
"#;
        let assertions = parse_assertions(content);
        assert_eq!(assertions.len(), 2);
        assert_eq!(
            assertions[0].subject,
            Subject::Module("a".to_string())
        );
        assert_eq!(
            assertions[1].predicate,
            Predicate::NotDependsOn("b".to_string())
        );
    }
}
