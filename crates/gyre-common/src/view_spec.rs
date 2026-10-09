/// View specification grammar for the Explorer canvas.
///
/// Defined in `specs/system/ui-layout.md` §4.
/// Used by: Explorer CRUD saved views, LLM-generated views, and built-in views.
use serde::{Deserialize, Serialize};

// ── Layout ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
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
#[serde(deny_unknown_fields)]
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
    // `left`/`right` sub-views are meaningful only for `side-by-side`. On any
    // other layout they are smuggled content that no renderer consumes —
    // reject rather than silently store (the nesting-depth rule above would
    // otherwise never see them).
    if spec.layout != LayoutType::SideBySide && (spec.left.is_some() || spec.right.is_some()) {
        return Err(
            "'left'/'right' sub-views are only allowed with layout 'side-by-side'".to_string()
        );
    }
    if spec.layout == LayoutType::SideBySide {
        let (left, right) = match (&spec.left, &spec.right) {
            (Some(l), Some(r)) => (l, r),
            _ => {
                return Err(
                    "layout 'side-by-side' requires both 'left' and 'right' sub-views".to_string()
                )
            }
        };
        for sub in [left, right] {
            if sub.layout == LayoutType::SideBySide {
                return Err(
                    "side-by-side sub-views cannot contain side-by-side layouts".to_string()
                );
            }
            validate_sub_view(sub)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn valid_spec() -> serde_json::Value {
        json!({
            "name": "How authentication works",
            "description": "Authentication flow from request to identity resolution",
            "data": {
                "concept": "auth",
                "node_types": ["Module", "Function", "Type", "Endpoint"],
                "edge_types": ["Contains", "Implements", "RoutesTo"],
                "depth": 2,
                "filter": {"min_churn": 0, "spec_path": null, "visibility": null},
                "repo_id": null
            },
            "layout": "hierarchical",
            "encoding": {
                "color": {"field": "node_type", "scale": "categorical"},
                "size": {"field": "churn_count_30d", "scale": "linear", "range": [24, 64]},
                "label": "qualified_name",
                "group_by": "file_path"
            },
            "annotations": [
                {"node_name": "require_auth_middleware", "text": "Entry point"}
            ],
            "highlight": {"spec_path": "specs/system/identity-security.md"},
            "explanation": "Authentication flows through require_auth_middleware"
        })
    }

    fn parse(v: serde_json::Value) -> ViewSpec {
        serde_json::from_value(v).expect("spec should parse")
    }

    #[test]
    fn spec_example_from_ui_layout_parses_and_validates() {
        let spec = parse(valid_spec());
        assert_eq!(spec.layout, LayoutType::Hierarchical);
        assert_eq!(spec.data.concept.as_deref(), Some("auth"));
        assert_eq!(spec.data.depth, 2);
        assert_eq!(
            spec.encoding.as_ref().unwrap().label.as_deref(),
            Some("qualified_name")
        );
        assert!(validate_view_spec(&spec).is_ok());
    }

    #[test]
    fn layout_names_serialize_as_kebab_case_per_spec() {
        // ui-layout.md §4 uses kebab-case layout names in JSON ("side-by-side").
        assert_eq!(
            serde_json::to_value(LayoutType::SideBySide).unwrap(),
            json!("side-by-side")
        );
        for (variant, expected) in [
            (LayoutType::Graph, "graph"),
            (LayoutType::Hierarchical, "hierarchical"),
            (LayoutType::Layered, "layered"),
            (LayoutType::List, "list"),
            (LayoutType::Timeline, "timeline"),
            (LayoutType::Diff, "diff"),
            (LayoutType::Flow, "flow"),
        ] {
            assert_eq!(
                serde_json::to_value(variant.clone()).unwrap(),
                json!(expected)
            );
            let back: LayoutType =
                serde_json::from_value(json!(expected)).expect("roundtrip layout");
            assert_eq!(back, variant);
        }
    }

    #[test]
    fn unknown_layout_name_is_rejected() {
        let mut v = valid_spec();
        v["layout"] = json!("sankey");
        let err = serde_json::from_value::<ViewSpec>(v).unwrap_err();
        assert!(err.to_string().contains("unknown variant"));
    }

    #[test]
    fn flow_layout_requires_trace_source() {
        let mut v = valid_spec();
        v["layout"] = json!("flow");
        let spec = parse(v);
        let err = validate_view_spec(&spec).unwrap_err();
        assert!(err.contains("trace_source"), "got: {err}");
    }

    #[test]
    fn flow_layout_with_trace_source_is_valid() {
        let mut v = valid_spec();
        v["layout"] = json!("flow");
        v["data"]["trace_source"] = json!({"mr_id": "mr-47"});
        let spec = parse(v);
        assert!(validate_view_spec(&spec).is_ok());
    }

    #[test]
    fn spec_path_filter_requires_repo_id() {
        let mut v = valid_spec();
        v["data"]["filter"]["spec_path"] = json!("system/payment-retry.md");
        v["data"]["repo_id"] = json!(null);
        let spec = parse(v);
        let err = validate_view_spec(&spec).unwrap_err();
        assert!(err.contains("repo_id"), "got: {err}");
    }

    #[test]
    fn side_by_side_requires_both_sub_views() {
        let mut v = valid_spec();
        v["layout"] = json!("side-by-side");
        v["left"] = json!({
            "data": {"spec_path": null},
            "layout": "list"
        });
        let spec = parse(v);
        let err = validate_view_spec(&spec).unwrap_err();
        assert!(err.contains("left"), "got: {err}");
        assert!(err.contains("right"), "got: {err}");
    }

    #[test]
    fn side_by_side_sub_view_cannot_contain_side_by_side() {
        let mut v = valid_spec();
        v["layout"] = json!("side-by-side");
        v["left"] = json!({
            "data": {},
            "layout": "side-by-side"
        });
        v["right"] = json!({
            "data": {},
            "layout": "list"
        });
        let spec = parse(v);
        let err = validate_view_spec(&spec).unwrap_err();
        assert!(err.contains("cannot contain side-by-side"), "got: {err}");
    }

    #[test]
    fn side_by_side_sub_view_rejects_top_level_only_fields() {
        let mut v = valid_spec();
        v["layout"] = json!("side-by-side");
        v["left"] = json!({
            "data": {},
            "layout": "list",
            "name": "sub-view name",
            "annotations": [{"node_name": "a", "text": "b"}],
            "explanation": "leaked field"
        });
        v["right"] = json!({
            "data": {},
            "layout": "list"
        });
        let err = serde_json::from_value::<ViewSpec>(v).unwrap_err();
        assert!(err.to_string().contains("unknown field"), "got: {err}");
    }

    #[test]
    fn side_by_side_sub_views_do_not_inherit_parent_repo_id() {
        // Spec: "No field inheritance: sub-views do not inherit data fields
        // from the parent. Each sub-view must declare its own repo_id if needed."
        // A sub-view filter.spec_path with no sub-view repo_id must fail even
        // when the parent declares one.
        let mut v = valid_spec();
        v["layout"] = json!("side-by-side");
        v["data"]["repo_id"] = json!("repo-1");
        v["left"] = json!({
            "data": {"filter": {"spec_path": "system/payment-retry.md"}},
            "layout": "list"
        });
        v["right"] = json!({
            "data": {},
            "layout": "list"
        });
        let spec = parse(v);
        let err = validate_view_spec(&spec).unwrap_err();
        assert!(err.contains("repo_id"), "got: {err}");
    }

    #[test]
    fn orphan_sub_views_rejected_on_non_side_by_side_layout() {
        // `left`/`right` are meaningful only for 'side-by-side'. On any other
        // layout they are smuggled content no renderer consumes — and the
        // nesting-depth rule would never see them.
        let mut v = valid_spec();
        v["layout"] = json!("list");
        v["left"] = json!({
            "data": {},
            "layout": "side-by-side"
        });
        v["right"] = json!({
            "data": {},
            "layout": "list"
        });
        let spec = parse(v);
        let err = validate_view_spec(&spec).unwrap_err();
        assert!(
            err.contains("only allowed with layout 'side-by-side'"),
            "got: {err}"
        );
    }

    #[test]
    fn flow_sub_view_requires_trace_source() {
        let mut v = valid_spec();
        v["layout"] = json!("side-by-side");
        v["left"] = json!({"data": {}, "layout": "flow"});
        v["right"] = json!({"data": {}, "layout": "list"});
        let spec = parse(v);
        let err = validate_view_spec(&spec).unwrap_err();
        assert!(err.contains("trace_source"), "got: {err}");
    }

    #[test]
    fn valid_side_by_side_composition_from_spec_example() {
        let spec = parse(json!({
            "name": "Spec realization",
            "data": {"node_types": [], "edge_types": [], "depth": 1},
            "layout": "side-by-side",
            "left": {
                "data": {"repo_id": "repo-1", "filter": {"spec_path": "system/payment-retry.md"}},
                "layout": "list",
                "encoding": {"label": "name", "color": {"field": "node_type"}}
            },
            "right": {
                "data": {"repo_id": "repo-1", "node_types": ["Type", "Function"]},
                "layout": "hierarchical"
            }
        }));
        assert!(validate_view_spec(&spec).is_ok());
    }
}
