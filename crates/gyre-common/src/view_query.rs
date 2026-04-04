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
    pub fn validate(&self) -> Vec<String> {
        let mut errors = Vec::new();

        // Bound depth fields to prevent DoS
        match &self.scope {
            Scope::Focus { depth, .. } => {
                if *depth > MAX_DEPTH {
                    errors.push(format!(
                        "Focus depth {} exceeds maximum of {MAX_DEPTH}",
                        depth
                    ));
                }
            }
            Scope::Concept { expand_depth, .. } => {
                if *expand_depth > MAX_DEPTH {
                    errors.push(format!(
                        "Concept expand_depth {} exceeds maximum of {MAX_DEPTH}",
                        expand_depth
                    ));
                }
            }
            _ => {}
        }

        // Validate dim_unmatched range (0.0-1.0)
        if let Some(dim) = self.emphasis.dim_unmatched {
            if !(0.0..=1.0).contains(&dim) {
                errors.push(format!(
                    "dim_unmatched {dim} is out of range — must be between 0.0 and 1.0"
                ));
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
    /// List saved views for this repo.
    ListViews,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ExplorerServerMessage {
    /// Streamed text response from LLM.
    Text { content: String, done: bool },
    /// View query to apply to canvas.
    ViewQuery { query: serde_json::Value },
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
