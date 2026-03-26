/// View specification grammar for the Explorer canvas.
///
/// Defined in `specs/system/ui-layout.md` §4.
/// Used by: Explorer CRUD saved views, LLM-generated views, and built-in views.
use serde::{Deserialize, Serialize};

// ── Layout ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum LayoutType {
    Graph,
    Hierarchical,
    Layered,
    List,
    Timeline,
    SideBySide,
    Diff,
    Flow,
}

// ── Data layer ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataFilter {
    pub min_churn: Option<u32>,
    pub spec_path: Option<String>,
    pub visibility: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceSource {
    pub mr_id: Option<String>,
    pub gate_run_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataLayer {
    /// Substring search on node name / qualified_name.
    pub concept: Option<String>,
    #[serde(default)]
    pub node_types: Vec<String>,
    #[serde(default)]
    pub edge_types: Vec<String>,
    #[serde(default)]
    pub depth: u32,
    pub repo_id: Option<String>,
    pub filter: Option<DataFilter>,
    /// Required when `layout == "flow"`.
    pub trace_source: Option<TraceSource>,
}

// ── Encoding layer ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncodingLayer {
    pub color: Option<serde_json::Value>,
    pub size: Option<serde_json::Value>,
    pub border: Option<serde_json::Value>,
    pub opacity: Option<serde_json::Value>,
    pub label: Option<String>,
    pub group_by: Option<String>,
    pub edge_color: Option<serde_json::Value>,
    pub edge_style: Option<serde_json::Value>,
    pub particle_color: Option<serde_json::Value>,
    pub particle_speed: Option<serde_json::Value>,
    pub node_badge: Option<serde_json::Value>,
}

// ── Highlight layer ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HighlightLayer {
    pub spec_path: Option<String>,
    pub node_ids: Option<Vec<String>>,
    pub edge_types: Option<Vec<String>>,
}

// ── Annotations ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Annotation {
    pub node_name: String,
    pub text: String,
}

// ── Sub-view (for side-by-side) ───────────────────────────────────────────────

/// Reduced view spec for use within a `side-by-side` layout.
/// Only `data`, `layout`, and `encoding` are permitted — no nesting.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubViewSpec {
    pub data: DataLayer,
    pub layout: LayoutType,
    pub encoding: Option<EncodingLayer>,
}

// ── Top-level ViewSpec ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ViewSpec {
    pub name: String,
    pub description: Option<String>,
    pub data: DataLayer,
    pub layout: LayoutType,
    pub encoding: Option<EncodingLayer>,
    pub annotations: Option<Vec<Annotation>>,
    pub highlight: Option<HighlightLayer>,
    pub explanation: Option<String>,
    /// Left sub-view for `side-by-side` layout.
    pub left: Option<Box<SubViewSpec>>,
    /// Right sub-view for `side-by-side` layout.
    pub right: Option<Box<SubViewSpec>>,
}

// ── Validation ────────────────────────────────────────────────────────────────

/// Validate a ViewSpec against the grammar constraints from ui-layout.md §4.
///
/// Returns `Err(message)` if invalid.
pub fn validate_view_spec(spec: &ViewSpec) -> Result<(), String> {
    // Flow layout requires trace_source.
    if spec.layout == LayoutType::Flow && spec.data.trace_source.is_none() {
        return Err("layout 'flow' requires data.trace_source".to_string());
    }

    // filter.spec_path requires repo_id.
    if let Some(filter) = &spec.data.filter {
        if filter.spec_path.is_some() && spec.data.repo_id.is_none() {
            return Err("data.filter.spec_path requires data.repo_id".to_string());
        }
    }

    // side-by-side: sub-views cannot themselves be side-by-side (depth=1 max).
    if spec.layout == LayoutType::SideBySide {
        if let Some(left) = &spec.left {
            if left.layout == LayoutType::SideBySide {
                return Err(
                    "side-by-side sub-views cannot contain side-by-side layouts".to_string()
                );
            }
            validate_sub_view(left)?;
        }
        if let Some(right) = &spec.right {
            if right.layout == LayoutType::SideBySide {
                return Err(
                    "side-by-side sub-views cannot contain side-by-side layouts".to_string()
                );
            }
            validate_sub_view(right)?;
        }
    }

    Ok(())
}

fn validate_sub_view(sub: &SubViewSpec) -> Result<(), String> {
    // flow sub-views also require trace_source.
    if sub.layout == LayoutType::Flow && sub.data.trace_source.is_none() {
        return Err("flow sub-view requires data.trace_source".to_string());
    }
    // filter.spec_path requires repo_id.
    if let Some(filter) = &sub.data.filter {
        if filter.spec_path.is_some() && sub.data.repo_id.is_none() {
            return Err("sub-view data.filter.spec_path requires data.repo_id".to_string());
        }
    }
    Ok(())
}
