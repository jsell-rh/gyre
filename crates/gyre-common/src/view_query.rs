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
        /// Expansion direction: "outgoing", "incoming", or "both" (default: "outgoing").
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
    "outgoing".to_string()
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
                            "Unknown edge type '{}' in Focus scope — known types: calls, contains, implements, depends_on, field_of, returns, routes_to, renders, persists_to, governed_by, produced_by",
                            e
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
                            "Unknown edge type '{}' in Concept scope — known types: calls, contains, implements, depends_on, field_of, returns, routes_to, renders, persists_to, governed_by, produced_by",
                            e
                        ));
                    }
                }
            }
            _ => {}
        }

        // Validate edge filter strings
        for e in &self.edges.filter {
            if !Self::KNOWN_EDGE_TYPES.contains(&e.to_lowercase().as_str()) {
                errors.push(format!(
                    "Unknown edge type '{}' in edge filter — known types: calls, contains, implements, depends_on, field_of, returns, routes_to, renders, persists_to, governed_by, produced_by",
                    e
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

        // Validate tiered_colors is non-empty when present
        if let Some(ref colors) = self.emphasis.tiered_colors {
            if colors.is_empty() {
                errors.push("tiered_colors array must not be empty when provided".to_string());
            }
        }

        // Validate heat metric is recognized
        if let Some(ref heat) = self.emphasis.heat {
            if !heat.metric.is_empty() && !Self::KNOWN_HEAT_METRICS.contains(&heat.metric.as_str())
            {
                errors.push(format!(
                    "Unknown heat metric '{}' — known metrics: complexity, churn, incoming_calls, outgoing_calls, test_coverage, field_count, test_fragility",
                    heat.metric
                ));
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
                let known_prefixes = [
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
                if !known_prefixes.iter().any(|p| normalized.starts_with(p)) {
                    errors.push(format!("Unknown computed expression: '{}'. Known: $where, $callers, $callees, $implementors, $fields, $descendants, $ancestors, $governed_by, $test_fragility, $reachable, $intersect, $union, $diff", normalized));
                }
                // Check balanced parens
                if normalized.contains('(') {
                    let open = normalized.chars().filter(|c| *c == '(').count();
                    let close = normalized.chars().filter(|c| *c == ')').count();
                    if open != close {
                        errors.push(format!(
                            "Unbalanced parentheses in computed expression: {} open, {} close",
                            open, close
                        ));
                    }
                }
                // Validate $where arguments: $where(metric, op, value)
                if normalized.starts_with("$where(") && normalized.ends_with(')') {
                    let inner = &normalized[7..normalized.len() - 1];
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
                        let known_where_metrics = [
                            "complexity",
                            "churn",
                            "churn_count_30d",
                            "incoming_calls",
                            "outgoing_calls",
                            "test_coverage",
                            "field_count",
                            "test_fragility",
                            "risk_score",
                        ];
                        if !known_where_metrics.contains(&parts[0]) {
                            errors.push(format!("Unknown $where metric '{}'", parts[0]));
                        }
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

        errors
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
}
