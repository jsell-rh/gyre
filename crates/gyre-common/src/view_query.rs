//! View Query Grammar for the Explorer canvas.
//!
//! Defined in `specs/system/view-query-grammar.md`.
//! The LLM generates view queries; the renderer executes them deterministically.

use serde::{Deserialize, Serialize};

// ── Scope ────────────────────────────────────────────────────────────────────

/// What subgraph to show.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Scope {
    /// Show everything.
    All,
    /// BFS from a node along specified edges.
    Focus {
        /// Node name, qualified_name, or computed reference like "$clicked", "$selected".
        node: String,
        #[serde(default)]
        edges: Vec<String>,
        #[serde(default = "default_direction")]
        direction: String,
        #[serde(default = "default_depth")]
        depth: u32,
    },
    /// Show nodes matching node_types, name pattern, or a computed set.
    Filter {
        #[serde(default)]
        node_types: Vec<String>,
        /// Computed expression like "$intersect($where(complexity, '>', 20), $test_unreachable)".
        computed: Option<String>,
        /// Substring match on name or qualified_name (case-insensitive).
        name_pattern: Option<String>,
    },
    /// Nodes NOT reachable from any test function.
    TestGaps,
    /// Changes between two commits.
    Diff {
        from_commit: String,
        to_commit: String,
    },
    /// Cross-cutting concept from seed nodes expanded along edges.
    Concept {
        seed_nodes: Vec<String>,
        #[serde(default)]
        expand_edges: Vec<String>,
        #[serde(default = "default_concept_depth")]
        expand_depth: u32,
        /// Expansion direction: "outgoing", "incoming", or "both" (default: "both").
        #[serde(default = "default_concept_direction")]
        expand_direction: String,
    },
}

fn default_direction() -> String {
    // Default "incoming" for blast-radius style queries — the most common use case.
    // The LLM system prompt documents this default explicitly.
    "incoming".to_string()
}
fn default_depth() -> u32 {
    5
}
fn default_concept_depth() -> u32 {
    2
}
fn default_concept_direction() -> String {
    "both".to_string()
}

// ── Emphasis ─────────────────────────────────────────────────────────────────

/// How matched/unmatched nodes are styled.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HighlightStyle {
    pub color: Option<String>,
    pub label: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Highlight {
    pub matched: Option<HighlightStyle>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HeatConfig {
    /// Metric name: incoming_calls, complexity, test_fragility, churn, etc.
    pub metric: String,
    /// Color palette name: "blue-red", "green-yellow-red", etc.
    #[serde(default = "default_palette")]
    pub palette: String,
}

fn default_palette() -> String {
    "blue-red".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BadgeConfig {
    /// Template string, e.g. "{{count}} calls".
    pub template: Option<String>,
    pub metric: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Emphasis {
    pub highlight: Option<Highlight>,
    /// Opacity for non-matched nodes (0.0–1.0).
    pub dim_unmatched: Option<f64>,
    /// Array of colors by BFS depth.
    pub tiered_colors: Option<Vec<String>>,
    /// Color all nodes by metric.
    pub heat: Option<HeatConfig>,
    /// Attach text labels.
    pub badges: Option<BadgeConfig>,
}

// ── Edges ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EdgeFilter {
    /// Edge types to show (inclusion). When result set is active, edges restricted to connections BETWEEN result nodes.
    #[serde(default)]
    pub filter: Vec<String>,
    /// Edge types to exclude (exclusion). Applied after filter — if both set, filter takes precedence.
    #[serde(default)]
    pub exclude: Vec<String>,
}

// ── Zoom ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Zoom {
    Named(String),
    Level { level: f64 },
}

impl Default for Zoom {
    fn default() -> Self {
        Zoom::Named("current".to_string())
    }
}

// ── Annotation ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ViewAnnotation {
    pub title: Option<String>,
    pub description: Option<String>,
}

// ── Groups ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ViewGroup {
    pub name: String,
    /// Node names or patterns to include in this group.
    #[serde(default)]
    pub nodes: Vec<String>,
    pub color: Option<String>,
    pub label: Option<String>,
}

// ── Callouts ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ViewCallout {
    pub node: String,
    pub text: String,
    pub color: Option<String>,
}

// ── Narrative ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NarrativeStep {
    pub node: String,
    pub text: String,
    pub order: Option<u32>,
}

// ── Top-level ViewQuery ──────────────────────────────────────────────────────

/// A complete view query that the renderer executes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ViewQuery {
    pub scope: Scope,
    #[serde(default)]
    pub emphasis: Emphasis,
    #[serde(default)]
    pub edges: EdgeFilter,
    #[serde(default)]
    pub zoom: Zoom,
    #[serde(default)]
    pub annotation: ViewAnnotation,
    #[serde(default)]
    pub groups: Vec<ViewGroup>,
    #[serde(default)]
    pub callouts: Vec<ViewCallout>,
    #[serde(default)]
    pub narrative: Vec<NarrativeStep>,
}

// ── Validation ──────────────────────────────────────────────────────────────

const MAX_DEPTH: u32 = 100;

impl ViewQuery {
    /// Validate a ViewQuery and return a list of errors (empty = valid).
    /// Known edge type strings.
    const KNOWN_EDGE_TYPES: &'static [&'static str] = &[
        "calls",
        "contains",
        "implements",
        "depends_on",
        "dependson",
        "field_of",
        "fieldof",
        "returns",
        "routes_to",
        "routesto",
        "renders",
        "persists_to",
        "persiststo",
        "governed_by",
        "governedby",
        "produced_by",
        "producedby",
    ];

    /// Known heat palette names.
    const KNOWN_PALETTES: &'static [&'static str] =
        &["blue-red", "green-yellow-red", "purple-orange"];

    /// Known heat metric strings.
    const KNOWN_HEAT_METRICS: &'static [&'static str] = &[
        "complexity",
        "churn",
        "churn_count_30d",
        "incoming_calls",
        "outgoing_calls",
        "test_coverage",
        "field_count",
        "test_fragility",
        "risk_score",
        // Evaluative lens metrics (from OTLP trace spans)
        "span_duration",
        "span_count",
        "error_rate",
    ];

    /// Validate a ViewQuery and return a list of errors (empty = valid).
    pub fn validate(&self) -> Vec<String> {
        let mut errors = Vec::new();

        // Bound depth fields to prevent DoS
        match &self.scope {
            Scope::Focus {
                depth,
                node,
                edges,
                direction,
            } => {
                if *depth > MAX_DEPTH {
                    errors.push(format!(
                        "Focus depth {} exceeds maximum of {MAX_DEPTH}",
                        depth
                    ));
                }
                if node.is_empty() {
                    errors.push("Focus scope 'node' field must not be empty".to_string());
                }
                // Validate computed expressions in Focus.node (e.g. "$callers(Foo)")
                if node.starts_with('$') && node != "$clicked" && node != "$selected" {
                    Self::validate_computed_expression(node, &mut errors);
                }
                // Validate edge type strings
                for e in edges {
                    if !Self::KNOWN_EDGE_TYPES.contains(&e.to_lowercase().as_str()) {
                        errors.push(format!(
                            "Unknown edge type '{}' in Focus scope — known types: {}",
                            e,
                            Self::KNOWN_EDGE_TYPES.join(", ")
                        ));
                    }
                }
                // Validate direction
                let valid_dirs = ["incoming", "outgoing", "both"];
                if !valid_dirs.contains(&direction.as_str()) {
                    errors.push(format!(
                        "Invalid direction '{}' in Focus scope — must be one of: incoming, outgoing, both",
                        direction
                    ));
                }
            }
            Scope::Concept {
                expand_depth,
                seed_nodes,
                expand_edges,
                expand_direction,
            } => {
                if *expand_depth > MAX_DEPTH {
                    errors.push(format!(
                        "Concept expand_depth {} exceeds maximum of {MAX_DEPTH}",
                        expand_depth
                    ));
                }
                if seed_nodes.is_empty() {
                    errors.push("Concept scope 'seed_nodes' must not be empty".to_string());
                }
                for e in expand_edges {
                    if !Self::KNOWN_EDGE_TYPES.contains(&e.to_lowercase().as_str()) {
                        errors.push(format!(
                            "Unknown edge type '{}' in Concept scope — known types: {}",
                            e,
                            Self::KNOWN_EDGE_TYPES.join(", ")
                        ));
                    }
                }
                let valid_dirs = ["incoming", "outgoing", "both"];
                if !valid_dirs.contains(&expand_direction.as_str()) {
                    errors.push(format!(
                        "Invalid expand_direction '{}' in Concept scope — must be one of: incoming, outgoing, both",
                        expand_direction
                    ));
                }
            }
            Scope::Diff {
                from_commit,
                to_commit,
            } => {
                if from_commit.is_empty() {
                    errors.push("Diff scope 'from_commit' must not be empty".to_string());
                }
                if to_commit.is_empty() {
                    errors.push("Diff scope 'to_commit' must not be empty".to_string());
                }
                // Validate commit references: accept SHA prefixes (4+ hex chars),
                // ~epoch temporal references, branch names (alphanumeric + -_./),
                // and special refs like HEAD
                fn validate_commit_ref(s: &str, field: &str, errors: &mut Vec<String>) {
                    if s.starts_with('~') {
                        return; // Temporal reference
                    }
                    if s.is_empty() {
                        return; // Caught above
                    }
                    // Accept: SHA hex prefixes (7+ hex chars), branch names, HEAD, tags
                    let is_hex = s.chars().all(|c| c.is_ascii_hexdigit());
                    if is_hex && s.len() < 7 {
                        errors.push(format!(
                            "Diff {} '{}' too short — need at least 7 hex characters for SHA prefix matching",
                            field, s
                        ));
                        return;
                    }
                    // Accept branch names, tags, HEAD — must be alphanumeric + -_./~^
                    let is_valid_ref = s
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || "-_./~^".contains(c));
                    if !is_valid_ref {
                        errors.push(format!(
                            "Diff {} '{}' contains invalid characters — use SHA prefix, branch name, tag, HEAD, or ~epoch",
                            field, s
                        ));
                    }
                }
                validate_commit_ref(from_commit, "from_commit", &mut errors);
                validate_commit_ref(to_commit, "to_commit", &mut errors);
                if from_commit == to_commit && !from_commit.is_empty() {
                    errors.push(format!(
                        "Diff from_commit and to_commit are identical ('{}') — diff will be empty",
                        from_commit
                    ));
                }
            }
            _ => {}
        }

        // Validate edge filter strings
        for e in &self.edges.filter {
            if !Self::KNOWN_EDGE_TYPES.contains(&e.to_lowercase().as_str()) {
                errors.push(format!(
                    "Unknown edge type '{}' in edge filter — known types: {}",
                    e,
                    Self::KNOWN_EDGE_TYPES.join(", ")
                ));
            }
        }

        // Validate dim_unmatched range (0.0-1.0)
        if let Some(dim) = self.emphasis.dim_unmatched {
            if !(0.0..=1.0).contains(&dim) {
                errors.push(format!(
                    "dim_unmatched {dim} is out of range — must be between 0.0 and 1.0"
                ));
            }
        }

        // Validate tiered_colors is non-empty when present + validate color format
        if let Some(ref colors) = self.emphasis.tiered_colors {
            if colors.is_empty() {
                errors.push("tiered_colors array must not be empty when provided".to_string());
            }
            for c in colors {
                if !Self::is_valid_color(c) {
                    errors.push(format!(
                        "Invalid color '{}' in tiered_colors — expected hex (#rgb, #rrggbb, #rrggbbaa), named CSS color, or hsl/rgb function",
                        c
                    ));
                }
            }
        }

        // Validate badge template length
        if let Some(ref badges) = self.emphasis.badges {
            if let Some(ref tmpl) = badges.template {
                if tmpl.len() > 500 {
                    errors.push(format!(
                        "Badge template length {} exceeds maximum of 500 characters",
                        tmpl.len()
                    ));
                }
            }
        }

        // Validate callout color fields
        for callout in &self.callouts {
            if let Some(ref c) = callout.color {
                if !Self::is_valid_color(c) {
                    errors.push(format!(
                        "Invalid color '{}' in callout for node '{}' — expected hex, named CSS color, or hsl/rgb function",
                        c, callout.node
                    ));
                }
            }
        }

        // Validate group color fields
        for group in &self.groups {
            if let Some(ref c) = group.color {
                if !Self::is_valid_color(c) {
                    errors.push(format!(
                        "Invalid color '{}' in group '{}' — expected hex, named CSS color, or hsl/rgb function",
                        c, group.name
                    ));
                }
            }
        }

        // Validate narrative step order uniqueness
        if !self.narrative.is_empty() {
            let mut seen_orders: std::collections::HashSet<u32> = std::collections::HashSet::new();
            for step in &self.narrative {
                if let Some(order) = step.order {
                    if !seen_orders.insert(order) {
                        errors.push(format!(
                            "Duplicate narrative step order {} for node '{}' — each step must have a unique order",
                            order, step.node
                        ));
                    }
                }
            }
        }

        // Validate heat metric is recognized
        if let Some(ref heat) = self.emphasis.heat {
            if !heat.metric.is_empty() && !Self::KNOWN_HEAT_METRICS.contains(&heat.metric.as_str())
            {
                errors.push(format!(
                    "Unknown heat metric '{}' — known metrics: {}",
                    heat.metric,
                    Self::KNOWN_HEAT_METRICS.join(", ")
                ));
            }
        }

        // Validate highlight.matched.color
        if let Some(ref highlight) = self.emphasis.highlight {
            if let Some(ref matched) = highlight.matched {
                if let Some(ref c) = matched.color {
                    if !Self::is_valid_color(c) {
                        errors.push(format!(
                            "Invalid color '{}' in highlight.matched — expected hex, named CSS color, or hsl/rgb function",
                            c
                        ));
                    }
                }
            }
        }

        // Validate Zoom field
        match &self.zoom {
            Zoom::Named(name) => {
                // Detect when serde(untagged) parsed a number string as Named
                if name.parse::<f64>().is_ok() {
                    errors.push(format!(
                        "Zoom value '{}' looks like a number — use {{\"level\": {}}} instead of a plain string",
                        name, name
                    ));
                } else {
                    let valid_names = ["fit", "current"];
                    if !valid_names.contains(&name.as_str()) {
                        errors.push(format!(
                            "Unknown zoom value '{}' — must be \"fit\", \"current\", or {{\"level\": N}}",
                            name
                        ));
                    }
                }
            }
            Zoom::Level { level } => {
                if *level < 0.05 || *level > 20.0 {
                    errors.push(format!(
                        "Zoom level {} is out of range — must be between 0.05 and 20.0",
                        level
                    ));
                }
            }
        }

        // Validate computed expression syntax in Filter scope
        if let Scope::Filter {
            computed: Some(ref expr),
            ..
        } = &self.scope
        {
            let normalized = expr.trim();
            if !normalized.is_empty() {
                Self::validate_computed_expression(normalized, &mut errors);
            }
        }

        errors
    }

    /// Return non-fatal warnings for the query. These do not block execution
    /// but inform the caller that defaults will be substituted.
    pub fn warnings(&self) -> Vec<String> {
        let mut warnings = Vec::new();

        // Warn on unrecognized heat palette (query still works with default)
        if let Some(ref heat) = self.emphasis.heat {
            if !heat.palette.is_empty() && !Self::KNOWN_PALETTES.contains(&heat.palette.as_str()) {
                warnings.push(format!(
                    "Unknown heat palette '{}' — will use default 'blue-red'. Known palettes: {}",
                    heat.palette,
                    Self::KNOWN_PALETTES.join(", ")
                ));
            }
        }

        warnings
    }

    /// Known computed expression prefixes.
    const KNOWN_COMPUTED_PREFIXES: &'static [&'static str] = &[
        "$clicked",
        "$selected",
        "$test_unreachable",
        "$test_reachable",
        "$ungoverned",
        "$where(",
        "$intersect(",
        "$union(",
        "$diff(",
        "$callers(",
        "$callees(",
        "$implementors(",
        "$fields(",
        "$descendants(",
        "$ancestors(",
        "$governed_by(",
        "$test_fragility(",
        "$reachable(",
    ];

    /// Maximum recursion depth for nested computed expressions.
    const MAX_COMPUTED_DEPTH: u32 = 20;

    /// Recursively validate a computed expression, including inner expressions
    /// within $intersect, $union, $diff.
    fn validate_computed_expression(expr: &str, errors: &mut Vec<String>) {
        Self::validate_computed_expression_inner(expr, errors, 0);
    }

    fn validate_computed_expression_inner(expr: &str, errors: &mut Vec<String>, depth: u32) {
        let expr = expr.trim();
        if expr.is_empty() {
            return;
        }

        if depth > Self::MAX_COMPUTED_DEPTH {
            errors.push(format!(
                "Computed expression nesting depth exceeds maximum of {}",
                Self::MAX_COMPUTED_DEPTH
            ));
            return;
        }

        // Standalone keywords must match exactly; parenthesized expressions match as prefix
        let is_known = Self::KNOWN_COMPUTED_PREFIXES.iter().any(|p| {
            if p.ends_with('(') {
                // Prefix match for function-style expressions like "$where("
                expr.starts_with(p)
            } else {
                // Exact match for standalone keywords like "$ungoverned"
                expr == *p
            }
        });
        if !is_known {
            errors.push(format!("Unknown computed expression: '{}'. Known: $where, $callers, $callees, $implementors, $fields, $descendants, $ancestors, $governed_by, $test_fragility, $reachable, $ungoverned, $intersect, $union, $diff", expr));
            return;
        }

        // Structural parentheses validation: check nesting is well-formed
        if expr.contains('(') {
            let mut depth_paren: i32 = 0;
            for ch in expr.chars() {
                match ch {
                    '(' => depth_paren += 1,
                    ')' => {
                        depth_paren -= 1;
                        if depth_paren < 0 {
                            errors.push(format!(
                                "Malformed parentheses in computed expression: closing ')' before matching '(' in '{}'",
                                expr
                            ));
                            return;
                        }
                    }
                    _ => {}
                }
            }
            if depth_paren != 0 {
                errors.push(format!(
                    "Unbalanced parentheses in computed expression: {} unclosed '(' in '{}'",
                    depth_paren, expr
                ));
                return;
            }
        }

        // Recursively validate inner expressions in set operations
        for set_op in &["$intersect(", "$union(", "$diff("] {
            if expr.starts_with(set_op) {
                // Extract inner content between outermost parens
                if let Some(inner) = Self::extract_inner_parens(expr, set_op.len() - 1) {
                    // Split on top-level commas (not inside nested parens)
                    let parts = Self::split_top_level_commas(inner);
                    for part in &parts {
                        let trimmed = part.trim();
                        if !trimmed.is_empty() {
                            Self::validate_computed_expression_inner(trimmed, errors, depth + 1);
                        }
                    }
                }
                return;
            }
        }

        // Validate $where arguments: $where(metric, op, value)
        if expr.starts_with("$where(") && expr.ends_with(')') {
            let inner = &expr[7..expr.len() - 1];
            let parts: Vec<&str> = inner
                .splitn(3, ',')
                .map(|s| s.trim().trim_matches('\'').trim_matches('"'))
                .collect();
            if parts.len() != 3 {
                errors.push(format!(
                    "$where requires 3 arguments (metric, operator, value), got {}",
                    parts.len()
                ));
            } else {
                // String-valued properties that support = and != with string values
                let string_props = ["node_type", "visibility", "spec_confidence"];
                let is_string_prop = string_props.contains(&parts[0]);

                if !is_string_prop && !Self::KNOWN_HEAT_METRICS.contains(&parts[0]) {
                    errors.push(format!("Unknown $where metric '{}'", parts[0]));
                }
                if is_string_prop {
                    let known_str_ops = ["=", "==", "!="];
                    if !known_str_ops.contains(&parts[1]) {
                        errors.push(format!(
                            "$where operator '{}' not valid for string property '{}' — use = or !=",
                            parts[1], parts[0]
                        ));
                    }
                    // Validate known enum values for specific string properties
                    if parts[0] == "node_type" {
                        let known_types = [
                            "function",
                            "type",
                            "interface",
                            "trait",
                            "module",
                            "package",
                            "endpoint",
                            "table",
                            "field",
                            "constant",
                            "component",
                            "class",
                            "enum",
                            "enum_variant",
                            "method",
                            "spec",
                            "test",
                        ];
                        if !known_types.contains(&parts[2]) {
                            errors.push(format!(
                                "$where node_type value '{}' is not recognized — known types: {}",
                                parts[2],
                                known_types.join(", ")
                            ));
                        }
                    } else if parts[0] == "visibility" {
                        let known_vis = ["public", "private", "internal", "protected"];
                        if !known_vis.contains(&parts[2]) {
                            errors.push(format!(
                                "$where visibility value '{}' is not recognized — known values: {}",
                                parts[2],
                                known_vis.join(", ")
                            ));
                        }
                    } else if parts[0] == "spec_confidence" {
                        let known_conf = ["high", "medium", "low", "none"];
                        if !known_conf.contains(&parts[2]) {
                            errors.push(format!(
                                "$where spec_confidence value '{}' is not recognized — known values: {}",
                                parts[2],
                                known_conf.join(", ")
                            ));
                        }
                    }
                } else {
                    let known_ops = [">", ">=", "<", "<=", "==", "="];
                    if !known_ops.contains(&parts[1]) {
                        errors.push(format!("Unknown $where operator '{}'", parts[1]));
                    }
                    if parts[2].parse::<f64>().is_err() {
                        errors.push(format!("$where value '{}' is not a number", parts[2]));
                    }
                }
            }
        }

        // Validate single-argument functions: $callers, $callees, $implementors, etc.
        let single_arg_fns = [
            "$callers(",
            "$callees(",
            "$implementors(",
            "$fields(",
            "$descendants(",
            "$ancestors(",
            "$governed_by(",
            "$test_fragility(",
        ];
        for fn_prefix in &single_arg_fns {
            if expr.starts_with(fn_prefix) && expr.ends_with(')') {
                let inner = expr[fn_prefix.len()..expr.len() - 1].trim();
                let fn_name = &fn_prefix[..fn_prefix.len() - 1];
                if inner.is_empty() {
                    errors.push(format!("{} requires exactly 1 argument, got 0", fn_name));
                } else {
                    // Check that the argument is non-empty after stripping quotes
                    let unquoted = inner.trim_matches('\'').trim_matches('"').trim();
                    if unquoted.is_empty() {
                        errors.push(format!("{} argument must not be empty", fn_name));
                    }
                }
                return;
            }
        }

        // Validate $reachable(node, [edge_types], direction, depth) — needs at least 2 args
        if expr.starts_with("$reachable(") && expr.ends_with(')') {
            let inner = &expr[11..expr.len() - 1].trim();
            if inner.is_empty() {
                errors.push(
                    "$reachable requires at least 2 arguments (node, edge_types)".to_string(),
                );
            }
        }
    }

    /// Extract the content between the outermost parentheses starting at `paren_pos`.
    fn extract_inner_parens(s: &str, paren_pos: usize) -> Option<&str> {
        let bytes = s.as_bytes();
        if paren_pos >= bytes.len() || bytes[paren_pos] != b'(' {
            return None;
        }
        // The content starts after the opening paren and ends before the matching close
        let start = paren_pos + 1;
        let mut depth = 1;
        let mut end = start;
        for &b in &bytes[start..] {
            if b == b'(' {
                depth += 1;
            } else if b == b')' {
                depth -= 1;
                if depth == 0 {
                    return Some(&s[start..end]);
                }
            }
            end += 1;
        }
        None
    }

    /// Split a string on commas that are not inside parentheses.
    fn split_top_level_commas(s: &str) -> Vec<&str> {
        let mut parts = Vec::new();
        let mut depth = 0;
        let mut start = 0;
        for (i, ch) in s.char_indices() {
            match ch {
                '(' => depth += 1,
                ')' => {
                    if depth > 0 {
                        depth -= 1;
                    }
                }
                ',' if depth == 0 => {
                    parts.push(&s[start..i]);
                    start = i + 1;
                }
                _ => {}
            }
        }
        parts.push(&s[start..]);
        parts
    }

    /// Standard CSS named colors (CSS Color Level 4).
    const CSS_NAMED_COLORS: &'static [&'static str] = &[
        "aliceblue",
        "antiquewhite",
        "aqua",
        "aquamarine",
        "azure",
        "beige",
        "bisque",
        "black",
        "blanchedalmond",
        "blue",
        "blueviolet",
        "brown",
        "burlywood",
        "cadetblue",
        "chartreuse",
        "chocolate",
        "coral",
        "cornflowerblue",
        "cornsilk",
        "crimson",
        "cyan",
        "darkblue",
        "darkcyan",
        "darkgoldenrod",
        "darkgray",
        "darkgreen",
        "darkgrey",
        "darkkhaki",
        "darkmagenta",
        "darkolivegreen",
        "darkorange",
        "darkorchid",
        "darkred",
        "darksalmon",
        "darkseagreen",
        "darkslateblue",
        "darkslategray",
        "darkslategrey",
        "darkturquoise",
        "darkviolet",
        "deeppink",
        "deepskyblue",
        "dimgray",
        "dimgrey",
        "dodgerblue",
        "firebrick",
        "floralwhite",
        "forestgreen",
        "fuchsia",
        "gainsboro",
        "ghostwhite",
        "gold",
        "goldenrod",
        "gray",
        "green",
        "greenyellow",
        "grey",
        "honeydew",
        "hotpink",
        "indianred",
        "indigo",
        "ivory",
        "khaki",
        "lavender",
        "lavenderblush",
        "lawngreen",
        "lemonchiffon",
        "lightblue",
        "lightcoral",
        "lightcyan",
        "lightgoldenrodyellow",
        "lightgray",
        "lightgreen",
        "lightgrey",
        "lightpink",
        "lightsalmon",
        "lightseagreen",
        "lightskyblue",
        "lightslategray",
        "lightslategrey",
        "lightsteelblue",
        "lightyellow",
        "lime",
        "limegreen",
        "linen",
        "magenta",
        "maroon",
        "mediumaquamarine",
        "mediumblue",
        "mediumorchid",
        "mediumpurple",
        "mediumseagreen",
        "mediumslateblue",
        "mediumspringgreen",
        "mediumturquoise",
        "mediumvioletred",
        "midnightblue",
        "mintcream",
        "mistyrose",
        "moccasin",
        "navajowhite",
        "navy",
        "oldlace",
        "olive",
        "olivedrab",
        "orange",
        "orangered",
        "orchid",
        "palegoldenrod",
        "palegreen",
        "paleturquoise",
        "palevioletred",
        "papayawhip",
        "peachpuff",
        "peru",
        "pink",
        "plum",
        "powderblue",
        "purple",
        "red",
        "rosybrown",
        "royalblue",
        "saddlebrown",
        "salmon",
        "sandybrown",
        "seagreen",
        "seashell",
        "sienna",
        "silver",
        "skyblue",
        "slateblue",
        "slategray",
        "slategrey",
        "snow",
        "springgreen",
        "steelblue",
        "tan",
        "teal",
        "thistle",
        "tomato",
        "transparent",
        "turquoise",
        "violet",
        "wheat",
        "white",
        "whitesmoke",
        "yellow",
        "yellowgreen",
    ];

    /// Check if a string is a valid CSS color value.
    /// Accepts: hex (#rgb, #rrggbb, #rrggbbaa), named CSS colors,
    /// hsl/rgb/rgba/hsla functions.
    fn is_valid_color(s: &str) -> bool {
        let s = s.trim();
        if s.is_empty() {
            return false;
        }
        // Hex colors
        if s.starts_with('#') {
            let hex = &s[1..];
            return matches!(hex.len(), 3 | 4 | 6 | 8)
                && hex.chars().all(|c| c.is_ascii_hexdigit());
        }
        // CSS functions — validate that arguments are numeric/comma/space/percent/period only
        if s.starts_with("rgb(")
            || s.starts_with("rgba(")
            || s.starts_with("hsl(")
            || s.starts_with("hsla(")
        {
            if !s.ends_with(')') {
                return false;
            }
            let paren_start = s.find('(').unwrap_or(0) + 1;
            let paren_end = s.len() - 1;
            if paren_start >= paren_end {
                return false;
            }
            let args = &s[paren_start..paren_end];
            return args.chars().all(|c| {
                c.is_ascii_digit()
                    || c == ','
                    || c == ' '
                    || c == '.'
                    || c == '%'
                    || c == '-'
                    || c == '/'
            });
        }
        // Named CSS colors — only accept known names, not arbitrary strings
        let lower = s.to_ascii_lowercase();
        Self::CSS_NAMED_COLORS.contains(&lower.as_str())
    }
}

// ── Canvas State (sent by frontend with each user message) ───────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SelectedNodeInfo {
    pub id: String,
    pub name: String,
    pub node_type: String,
    pub qualified_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CanvasState {
    pub selected_node: Option<SelectedNodeInfo>,
    #[serde(default)]
    pub zoom_level: f64,
    #[serde(default)]
    pub visible_tree_groups: Vec<String>,
    pub active_filter: Option<String>,
    pub active_lens: Option<String>,
    pub active_query: Option<serde_json::Value>,
    /// Recent user interactions (clicks, zooms, query presets) for conversational context.
    /// Accepts both structured objects (preferred) and plain strings (legacy).
    #[serde(default)]
    pub recent_interactions: Vec<InteractionRecord>,
}

/// Structured record of a user interaction on the canvas.
/// Accepts both structured objects `{"action":"click","node":"Foo"}` and
/// plain strings `"click:Foo(function)"` for backwards compatibility.
#[derive(Debug, Clone, Serialize)]
pub struct InteractionRecord {
    pub action: String,
    #[serde(default)]
    pub node: Option<String>,
    #[serde(default)]
    pub detail: Option<String>,
}

impl<'de> serde::Deserialize<'de> for InteractionRecord {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de;

        struct InteractionVisitor;
        impl<'de> de::Visitor<'de> for InteractionVisitor {
            type Value = InteractionRecord;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a string or an InteractionRecord object")
            }

            fn visit_str<E: de::Error>(self, v: &str) -> Result<InteractionRecord, E> {
                // Parse "action:node(detail)" or just "action:node" or plain "action"
                if let Some((action, rest)) = v.split_once(':') {
                    let (node, detail) = if let Some(paren) = rest.find('(') {
                        let node = &rest[..paren];
                        let detail = rest[paren + 1..].trim_end_matches(')');
                        (Some(node.to_string()), Some(detail.to_string()))
                    } else {
                        (Some(rest.to_string()), None)
                    };
                    Ok(InteractionRecord {
                        action: action.to_string(),
                        node,
                        detail,
                    })
                } else {
                    Ok(InteractionRecord {
                        action: v.to_string(),
                        node: None,
                        detail: None,
                    })
                }
            }

            fn visit_map<A: de::MapAccess<'de>>(
                self,
                map: A,
            ) -> Result<InteractionRecord, A::Error> {
                #[derive(serde::Deserialize)]
                struct Inner {
                    action: String,
                    #[serde(default)]
                    node: Option<String>,
                    #[serde(default)]
                    detail: Option<String>,
                }
                let inner = Inner::deserialize(de::value::MapAccessDeserializer::new(map))?;
                Ok(InteractionRecord {
                    action: inner.action,
                    node: inner.node,
                    detail: inner.detail,
                })
            }
        }

        deserializer.deserialize_any(InteractionVisitor)
    }
}

// ── WebSocket Protocol Messages ──────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ExplorerClientMessage {
    /// User sends a question with current canvas state.
    Message {
        text: String,
        #[serde(default)]
        canvas_state: CanvasState,
    },
    /// Save current view query.
    /// When `scope` is `Some("workspace")`, the view is saved with
    /// `repo_id = "__workspace__"` so it appears in workspace-level listings.
    SaveView {
        name: String,
        description: Option<String>,
        query: serde_json::Value,
        #[serde(default)]
        scope: Option<String>,
    },
    /// Load a saved view.
    LoadView { view_id: String },
    /// Delete a saved view.
    DeleteView { view_id: String },
    /// List saved views for this repo.
    ListViews,
    /// Cancel the currently running agent query.
    Cancel,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ExplorerServerMessage {
    /// Streamed text response from LLM.
    Text { content: String, done: bool },
    /// View query to apply to canvas.
    ViewQuery {
        query: serde_json::Value,
        explanation: Option<String>,
    },
    /// List of saved views.
    Views { views: Vec<SavedViewSummary> },
    /// Status update (with optional agent_path: "sdk" or "native").
    Status {
        status: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        agent_path: Option<String>,
        /// Seconds since the graph data cache was last loaded/refreshed.
        /// Included so the UI can show "Graph data from Xs ago".
        #[serde(skip_serializing_if = "Option::is_none")]
        graph_data_age_secs: Option<u64>,
    },
    /// Non-fatal warning (e.g. deprecation notices).
    Warning { message: String },
    /// Error.
    Error { message: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedViewSummary {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub created_at: u64,
    /// Whether this is a system-provided (non-deletable) view.
    #[serde(default)]
    pub is_system: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minimal_query_with_heat(palette: &str) -> ViewQuery {
        ViewQuery {
            scope: Scope::All,
            emphasis: Emphasis {
                heat: Some(HeatConfig {
                    metric: "complexity".to_string(),
                    palette: palette.to_string(),
                }),
                ..Default::default()
            },
            edges: EdgeFilter::default(),
            zoom: Zoom::default(),
            annotation: ViewAnnotation::default(),
            groups: vec![],
            callouts: vec![],
            narrative: vec![],
        }
    }

    #[test]
    fn test_valid_palette_no_warnings() {
        for palette in ViewQuery::KNOWN_PALETTES {
            let q = minimal_query_with_heat(palette);
            assert!(
                q.validate().is_empty(),
                "valid palette should have no errors"
            );
            assert!(
                q.warnings().is_empty(),
                "known palette '{}' should produce no warnings",
                palette
            );
        }
    }

    #[test]
    fn test_unknown_palette_produces_warning() {
        let q = minimal_query_with_heat("viridis");
        // Should NOT be an error (query still works)
        assert!(
            q.validate().is_empty(),
            "unknown palette should not be an error"
        );
        // Should produce a warning
        let warnings = q.warnings();
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("viridis"));
        assert!(warnings[0].contains("blue-red"));
    }

    #[test]
    fn test_default_palette_no_warning() {
        let q = minimal_query_with_heat("blue-red");
        assert!(q.warnings().is_empty());
    }

    #[test]
    fn test_save_view_scope_field_deserialization() {
        let json = r#"{"type":"save_view","name":"test","query":{},"scope":"workspace"}"#;
        let msg: ExplorerClientMessage = serde_json::from_str(json).unwrap();
        match msg {
            ExplorerClientMessage::SaveView { scope, .. } => {
                assert_eq!(scope.as_deref(), Some("workspace"));
            }
            _ => panic!("Expected SaveView"),
        }
    }

    #[test]
    fn test_save_view_scope_field_optional() {
        let json = r#"{"type":"save_view","name":"test","query":{}}"#;
        let msg: ExplorerClientMessage = serde_json::from_str(json).unwrap();
        match msg {
            ExplorerClientMessage::SaveView { scope, .. } => {
                assert!(scope.is_none());
            }
            _ => panic!("Expected SaveView"),
        }
    }
}
