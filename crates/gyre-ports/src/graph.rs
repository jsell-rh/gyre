//! Port trait for the knowledge graph store.

use anyhow::Result;
use async_trait::async_trait;
use gyre_common::{
    graph::{ArchitecturalDelta, EdgeType, GraphEdge, GraphNode, NodeType, SpecConfidence},
    Id,
};

/// Storage port for the knowledge graph (realized model).
#[async_trait]
pub trait GraphPort: Send + Sync {
    /// Persist a new graph node.
    async fn create_node(&self, node: GraphNode) -> Result<GraphNode>;

    /// Retrieve a node by ID.
    async fn get_node(&self, id: &Id) -> Result<Option<GraphNode>>;

    /// List nodes for a repo, optionally filtered by node type.
    async fn list_nodes(&self, repo_id: &Id, node_type: Option<NodeType>)
        -> Result<Vec<GraphNode>>;

    /// Persist a new graph edge.
    async fn create_edge(&self, edge: GraphEdge) -> Result<GraphEdge>;

    /// List edges for a repo, optionally filtered by edge type.
    async fn list_edges(&self, repo_id: &Id, edge_type: Option<EdgeType>)
        -> Result<Vec<GraphEdge>>;

    /// Delete a single node by ID (used during incremental graph updates).
    async fn delete_node(&self, id: &Id) -> Result<()>;

    /// Delete a single edge by ID (used during incremental graph updates).
    async fn delete_edge(&self, id: &Id) -> Result<()>;

    /// Delete all nodes for a repo (e.g. before a full re-index).
    async fn delete_nodes_by_repo(&self, repo_id: &Id) -> Result<u64>;

    /// Delete all edges for a repo.
    async fn delete_edges_by_repo(&self, repo_id: &Id) -> Result<u64>;

    /// Record an architectural delta tied to a commit.
    async fn record_delta(&self, delta: ArchitecturalDelta) -> Result<ArchitecturalDelta>;

    /// List deltas for a repo, optionally bounded by unix-epoch timestamp range.
    async fn list_deltas(
        &self,
        repo_id: &Id,
        since: Option<u64>,
        until: Option<u64>,
    ) -> Result<Vec<ArchitecturalDelta>>;

    /// Return all nodes linked to a particular spec path.
    async fn get_nodes_by_spec(&self, repo_id: &Id, spec_path: &str) -> Result<Vec<GraphNode>>;

    /// Update the spec link and confidence on an existing node.
    async fn link_node_to_spec(
        &self,
        node_id: &Id,
        spec_path: &str,
        confidence: SpecConfidence,
    ) -> Result<()>;

    /// Return all edges where the given node is the source or target.
    async fn list_edges_for_node(&self, node_id: &Id) -> Result<Vec<GraphEdge>>;
}
