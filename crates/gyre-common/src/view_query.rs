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
    "outgoing".to_string()
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
    /// Edge types to show. When result set is active, edges restricted to connections BETWEEN result nodes.
    #[serde(default)]
    pub filter: Vec<String>,
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
                depth, node, edges, ..
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
            }
            Scope::Concept {
                expand_depth,
                seed_nodes,
                expand_edges,
                ..
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
                // Non-temporal SHAs need at least 4 hex chars
                if !from_commit.starts_with('~') && !from_commit.is_empty() {
                    if from_commit.len() < 4 {
                        errors.push(format!(
                            "Diff from_commit '{}' too short — need at least 4 hex characters for SHA prefix matching",
                            from_commit
                        ));
                    } else if !from_commit.chars().all(|c| c.is_ascii_hexdigit()) {
                        errors.push(format!(
                            "Diff from_commit '{}' contains non-hex characters — use hex SHA prefix or ~epoch for temporal diff",
                            from_commit
                        ));
                    }
                }
                if !to_commit.starts_with('~') && !to_commit.is_empty() {
                    if to_commit.len() < 4 {
                        errors.push(format!(
                            "Diff to_commit '{}' too short — need at least 4 hex characters for SHA prefix matching",
                            to_commit
                        ));
                    } else if !to_commit.chars().all(|c| c.is_ascii_hexdigit()) {
                        errors.push(format!(
                            "Diff to_commit '{}' contains non-hex characters — use hex SHA prefix or ~epoch for temporal diff",
                            to_commit
                        ));
                    }
                }
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

    /// Known computed expression prefixes.
    const KNOWN_COMPUTED_PREFIXES: &'static [&'static str] = &[
        "$clicked",
        "$selected",
        "$test_unreachable",
        "$test_reachable",
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

        if !Self::KNOWN_COMPUTED_PREFIXES
            .iter()
            .any(|p| expr.starts_with(p))
        {
            errors.push(format!("Unknown computed expression: '{}'. Known: $where, $callers, $callees, $implementors, $fields, $descendants, $ancestors, $governed_by, $test_fragility, $reachable, $intersect, $union, $diff", expr));
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

    /// Check if a string is a valid CSS color value.
    /// Accepts: hex (#rgb, #rrggbb, #rrggbbaa), named CSS colors,
    /// hsl/rgb/rgba/hsla functions, and common color keywords.
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
        // CSS functions
        if s.starts_with("rgb(")
            || s.starts_with("rgba(")
            || s.starts_with("hsl(")
            || s.starts_with("hsla(")
        {
            return s.ends_with(')');
        }
        // Named colors (subset of common ones + allow any alpha-only string <= 30 chars)
        s.len() <= 30 && s.chars().all(|c| c.is_ascii_alphabetic())
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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InteractionRecord {
    pub action: String,
    #[serde(default)]
    pub node: Option<String>,
    #[serde(default)]
    pub detail: Option<String>,
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
    SaveView {
        name: String,
        description: Option<String>,
        query: serde_json::Value,
    },
    /// Load a saved view.
    LoadView { view_id: String },
    /// Delete a saved view.
    DeleteView { view_id: String },
    /// List saved views for this repo.
    ListViews,
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
    /// Status update.
    Status { status: String },
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
