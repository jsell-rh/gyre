//! Knowledge graph domain types for the realized model (specs/system/realized-model.md).

use crate::Id;
use serde::{Deserialize, Serialize};

/// Universal node types in the knowledge graph.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum NodeType {
    Package,
    Module,
    Type,
    Interface,
    Function,
    Endpoint,
    Component,
    Table,
    Constant,
}

/// Typed relationship between two graph nodes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum EdgeType {
    Contains,
    Implements,
    DependsOn,
    Calls,
    FieldOf,
    Returns,
    RoutesTo,
    Renders,
    PersistsTo,
    GovernedBy,
    ProducedBy,
}

/// Symbol visibility level.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Visibility {
    Public,
    PubCrate,
    Private,
}

/// Confidence that a node is governed by a specific spec.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SpecConfidence {
    High,
    Medium,
    Low,
    None,
}

/// A node in the knowledge graph representing a code symbol or architectural entity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphNode {
    pub id: Id,
    pub repo_id: Id,
    pub node_type: NodeType,
    pub name: String,
    pub qualified_name: String,
    pub file_path: String,
    pub line_start: u32,
    pub line_end: u32,
    pub visibility: Visibility,
    pub doc_comment: Option<String>,
    pub spec_path: Option<String>,
    pub spec_confidence: SpecConfidence,
    pub last_modified_sha: String,
    pub last_modified_by: Option<Id>,
    pub last_modified_at: u64,
    pub created_sha: String,
    pub created_at: u64,
    pub complexity: Option<u32>,
    pub churn_count_30d: u32,
}

/// A directed edge between two graph nodes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphEdge {
    pub id: Id,
    pub repo_id: Id,
    pub source_id: Id,
    pub target_id: Id,
    pub edge_type: EdgeType,
    /// Optional JSON metadata for the edge.
    pub metadata: Option<String>,
}

/// A recorded architectural change associated with a commit.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchitecturalDelta {
    pub id: Id,
    pub repo_id: Id,
    pub commit_sha: String,
    pub timestamp: u64,
    pub agent_id: Option<Id>,
    pub spec_ref: Option<String>,
    /// Serialized delta details (JSON).
    pub delta_json: String,
}

/// Risk metrics for a module derived from graph analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskMetrics {
    pub module_name: String,
    pub churn_rate: f32,
    pub coupling_score: f32,
    pub spec_coverage: f32,
    pub complexity: f32,
    pub fan_in: u32,
    pub fan_out: u32,
    pub agent_contention: u32,
    pub staleness_days: u32,
}

/// A named concept view that groups related graph nodes for display.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConceptView {
    pub name: String,
    pub description: String,
    /// Glob patterns matching node qualified names to include.
    pub include_types: Vec<String>,
    pub include_traits: Vec<String>,
    pub include_modules: Vec<String>,
    pub include_endpoints: Vec<String>,
    pub include_specs: Vec<String>,
}
