//! In-memory knowledge graph adapter.

use anyhow::Result;
use async_trait::async_trait;
use gyre_common::{
    graph::{ArchitecturalDelta, EdgeType, GraphEdge, GraphNode, NodeType, SpecConfidence},
    Id,
};
use gyre_ports::GraphPort;
use std::sync::RwLock;

/// Thread-safe in-memory store for the knowledge graph.
#[derive(Default)]
pub struct MemGraphStore {
    nodes: RwLock<Vec<GraphNode>>,
    edges: RwLock<Vec<GraphEdge>>,
    deltas: RwLock<Vec<ArchitecturalDelta>>,
}

impl MemGraphStore {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl GraphPort for MemGraphStore {
    async fn create_node(&self, node: GraphNode) -> Result<GraphNode> {
        self.nodes.write().unwrap().push(node.clone());
        Ok(node)
    }

    async fn get_node(&self, id: &Id) -> Result<Option<GraphNode>> {
        let nodes = self.nodes.read().unwrap();
        Ok(nodes.iter().find(|n| &n.id == id).cloned())
    }

    async fn list_nodes(
        &self,
        repo_id: &Id,
        node_type: Option<NodeType>,
    ) -> Result<Vec<GraphNode>> {
        let nodes = self.nodes.read().unwrap();
        Ok(nodes
            .iter()
            .filter(|n| &n.repo_id == repo_id)
            .filter(|n| node_type.as_ref().is_none_or(|nt| &n.node_type == nt))
            .cloned()
            .collect())
    }

    async fn create_edge(&self, edge: GraphEdge) -> Result<GraphEdge> {
        self.edges.write().unwrap().push(edge.clone());
        Ok(edge)
    }

    async fn list_edges(
        &self,
        repo_id: &Id,
        edge_type: Option<EdgeType>,
    ) -> Result<Vec<GraphEdge>> {
        let edges = self.edges.read().unwrap();
        Ok(edges
            .iter()
            .filter(|e| &e.repo_id == repo_id)
            .filter(|e| edge_type.as_ref().is_none_or(|et| &e.edge_type == et))
            .cloned()
            .collect())
    }

    async fn delete_nodes_by_repo(&self, repo_id: &Id) -> Result<u64> {
        let mut nodes = self.nodes.write().unwrap();
        let before = nodes.len();
        nodes.retain(|n| &n.repo_id != repo_id);
        Ok((before - nodes.len()) as u64)
    }

    async fn delete_edges_by_repo(&self, repo_id: &Id) -> Result<u64> {
        let mut edges = self.edges.write().unwrap();
        let before = edges.len();
        edges.retain(|e| &e.repo_id != repo_id);
        Ok((before - edges.len()) as u64)
    }

    async fn record_delta(&self, delta: ArchitecturalDelta) -> Result<ArchitecturalDelta> {
        self.deltas.write().unwrap().push(delta.clone());
        Ok(delta)
    }

    async fn list_deltas(
        &self,
        repo_id: &Id,
        since: Option<u64>,
        until: Option<u64>,
    ) -> Result<Vec<ArchitecturalDelta>> {
        let deltas = self.deltas.read().unwrap();
        Ok(deltas
            .iter()
            .filter(|d| &d.repo_id == repo_id)
            .filter(|d| since.is_none_or(|s| d.timestamp >= s))
            .filter(|d| until.is_none_or(|u| d.timestamp <= u))
            .cloned()
            .collect())
    }

    async fn get_nodes_by_spec(&self, repo_id: &Id, spec_path: &str) -> Result<Vec<GraphNode>> {
        let nodes = self.nodes.read().unwrap();
        Ok(nodes
            .iter()
            .filter(|n| &n.repo_id == repo_id)
            .filter(|n| n.spec_path.as_deref() == Some(spec_path))
            .cloned()
            .collect())
    }

    async fn link_node_to_spec(
        &self,
        node_id: &Id,
        spec_path: &str,
        confidence: SpecConfidence,
    ) -> Result<()> {
        let mut nodes = self.nodes.write().unwrap();
        if let Some(node) = nodes.iter_mut().find(|n| &n.id == node_id) {
            node.spec_path = Some(spec_path.to_string());
            node.spec_confidence = confidence;
        }
        Ok(())
    }

    async fn list_edges_for_node(&self, node_id: &Id) -> Result<Vec<GraphEdge>> {
        let edges = self.edges.read().unwrap();
        Ok(edges
            .iter()
            .filter(|e| &e.source_id == node_id || &e.target_id == node_id)
            .cloned()
            .collect())
    }
}
