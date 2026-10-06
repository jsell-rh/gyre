//! In-memory knowledge graph adapter.

use anyhow::Result;
use async_trait::async_trait;
use gyre_common::{
    graph::{ArchitecturalDelta, EdgeType, GraphEdge, GraphNode, NodeType, SpecConfidence},
    Id,
};
use gyre_ports::GraphPort;
use parking_lot::RwLock;

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
        let mut nodes = self.nodes.write();
        // Upsert on id, matching the SQLite adapter's ON CONFLICT(`id`)
        // semantics (sqlite/graph.rs create_node): re-persisting a node
        // updates the mutable fields in place while the creation-identity
        // fields (created_sha, created_at, first_seen_at) are immutable —
        // never updated on conflict. Without this, do_extract()'s
        // upsert-all-nodes loop (graph_extraction.rs step 4) would
        // duplicate every node on each push in the mem store.
        if let Some(existing) = nodes.iter_mut().find(|n| n.id == node.id) {
            existing.node_type = node.node_type.clone();
            existing.name = node.name.clone();
            existing.qualified_name = node.qualified_name.clone();
            existing.file_path = node.file_path.clone();
            existing.line_start = node.line_start;
            existing.line_end = node.line_end;
            existing.visibility = node.visibility.clone();
            existing.doc_comment = node.doc_comment.clone();
            existing.spec_path = node.spec_path.clone();
            existing.spec_confidence = node.spec_confidence.clone();
            existing.last_modified_sha = node.last_modified_sha.clone();
            existing.last_modified_by = node.last_modified_by.clone();
            existing.last_modified_at = node.last_modified_at;
            existing.complexity = node.complexity;
            existing.churn_count_30d = node.churn_count_30d;
            existing.test_coverage = node.test_coverage;
            existing.last_seen_at = node.last_seen_at;
            // Clear deleted_at when a node reappears after removal, and
            // preserve created_*/first_seen_at — same as SQLite.
            existing.deleted_at = node.deleted_at;
            existing.test_node = node.test_node;
        } else {
            nodes.push(node.clone());
        }
        Ok(node)
    }

    async fn get_node(&self, id: &Id) -> Result<Option<GraphNode>> {
        let nodes = self.nodes.read();
        Ok(nodes.iter().find(|n| &n.id == id).cloned())
    }

    async fn list_nodes(
        &self,
        repo_id: &Id,
        node_type: Option<NodeType>,
    ) -> Result<Vec<GraphNode>> {
        let nodes = self.nodes.read();
        Ok(nodes
            .iter()
            .filter(|n| &n.repo_id == repo_id)
            .filter(|n| n.deleted_at.is_none())
            .filter(|n| node_type.as_ref().is_none_or(|nt| &n.node_type == nt))
            .cloned()
            .collect())
    }

    async fn create_edge(&self, edge: GraphEdge) -> Result<GraphEdge> {
        let mut edges = self.edges.write();
        // Upsert on id, matching the SQLite adapter's ON CONFLICT(`id`)
        // semantics: re-persisting a content-derived edge id updates the
        // mutable fields in place (edge_type, metadata, last_seen_at,
        // deleted_at) while first_seen_at — the identity of when the edge
        // first appeared — is immutable, exactly as the SQLite adapter
        // never updates it on conflict.
        if let Some(existing) = edges.iter_mut().find(|e| e.id == edge.id) {
            existing.edge_type = edge.edge_type.clone();
            existing.metadata = edge.metadata.clone();
            existing.last_seen_at = edge.last_seen_at;
            existing.deleted_at = edge.deleted_at;
        } else {
            edges.push(edge.clone());
        }
        Ok(edge)
    }

    async fn list_edges(
        &self,
        repo_id: &Id,
        edge_type: Option<EdgeType>,
    ) -> Result<Vec<GraphEdge>> {
        let edges = self.edges.read();
        Ok(edges
            .iter()
            .filter(|e| &e.repo_id == repo_id)
            .filter(|e| e.deleted_at.is_none())
            .filter(|e| edge_type.as_ref().is_none_or(|et| &e.edge_type == et))
            .cloned()
            .collect())
    }

    async fn delete_node(&self, id: &Id) -> Result<()> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let mut nodes = self.nodes.write();
        if let Some(node) = nodes.iter_mut().find(|n| &n.id == id) {
            node.deleted_at = Some(now);
        }
        Ok(())
    }

    async fn delete_edge(&self, id: &Id) -> Result<()> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let mut edges = self.edges.write();
        if let Some(edge) = edges.iter_mut().find(|e| &e.id == id) {
            edge.deleted_at = Some(now);
        }
        Ok(())
    }

    async fn delete_nodes_by_repo(&self, repo_id: &Id) -> Result<u64> {
        let mut nodes = self.nodes.write();
        let before = nodes.len();
        nodes.retain(|n| &n.repo_id != repo_id);
        Ok((before - nodes.len()) as u64)
    }

    async fn delete_edges_by_repo(&self, repo_id: &Id) -> Result<u64> {
        let mut edges = self.edges.write();
        let before = edges.len();
        edges.retain(|e| &e.repo_id != repo_id);
        Ok((before - edges.len()) as u64)
    }

    async fn record_delta(&self, delta: ArchitecturalDelta) -> Result<ArchitecturalDelta> {
        self.deltas.write().push(delta.clone());
        Ok(delta)
    }

    async fn list_deltas(
        &self,
        repo_id: &Id,
        since: Option<u64>,
        until: Option<u64>,
    ) -> Result<Vec<ArchitecturalDelta>> {
        let deltas = self.deltas.read();
        Ok(deltas
            .iter()
            .filter(|d| &d.repo_id == repo_id)
            .filter(|d| since.is_none_or(|s| d.timestamp >= s))
            .filter(|d| until.is_none_or(|u| d.timestamp <= u))
            .cloned()
            .collect())
    }

    async fn get_nodes_by_spec(&self, repo_id: &Id, spec_path: &str) -> Result<Vec<GraphNode>> {
        let nodes = self.nodes.read();
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
        let mut nodes = self.nodes.write();
        if let Some(node) = nodes.iter_mut().find(|n| &n.id == node_id) {
            node.spec_path = Some(spec_path.to_string());
            node.spec_confidence = confidence;
        }
        Ok(())
    }

    async fn list_edges_for_node(&self, node_id: &Id) -> Result<Vec<GraphEdge>> {
        let edges = self.edges.read();
        Ok(edges
            .iter()
            .filter(|e| &e.source_id == node_id || &e.target_id == node_id)
            .cloned()
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gyre_common::graph::Visibility;

    fn node(id: &str, qname: &str) -> GraphNode {
        GraphNode {
            id: Id::new(id),
            repo_id: Id::new("repo1"),
            node_type: NodeType::Function,
            name: qname.rsplit('.').next().unwrap_or(qname).to_string(),
            qualified_name: qname.to_string(),
            file_path: "a.go".to_string(),
            line_start: 1,
            line_end: 2,
            visibility: Visibility::Public,
            doc_comment: None,
            spec_path: None,
            spec_paths: vec![],
            spec_confidence: SpecConfidence::None,
            last_modified_sha: "s1".to_string(),
            last_modified_by: None,
            last_modified_at: 0,
            created_sha: "s1".to_string(),
            created_at: 10,
            complexity: None,
            churn_count_30d: 0,
            test_coverage: None,
            first_seen_at: 10,
            last_seen_at: 10,
            deleted_at: None,
            test_node: false,
            spec_approved_at: None,
            milestone_completed_at: None,
        }
    }

    #[tokio::test]
    async fn create_node_upserts_on_id_like_sqlite() {
        // GraphPort contract: create_node upserts on `id` — re-persisting
        // an existing node updates mutable fields but preserves the
        // creation-identity fields (created_*, first_seen_at) and never
        // duplicates the row. The SQLite adapter's ON CONFLICT(`id`) has
        // always done this; the mem adapter must mirror it or
        // do_extract()'s upsert-all-nodes loop silently duplicates every
        // node per push in mem-backed (test) deployments.
        let store = MemGraphStore::new();
        store.create_node(node("n1", "pkg.A")).await.unwrap();

        let mut again = node("n1", "pkg.A");
        again.name = "renamed".to_string();
        again.last_seen_at = 99;
        again.first_seen_at = 99; // caller must not be able to move it
        again.created_at = 99; // ditto
        store.create_node(again).await.unwrap();

        let listed = store.list_nodes(&Id::new("repo1"), None).await.unwrap();
        assert_eq!(listed.len(), 1, "upsert must not duplicate the node row");
        let fetched = store.get_node(&Id::new("n1")).await.unwrap().unwrap();
        assert_eq!(fetched.name, "renamed", "mutable fields update in place");
        assert_eq!(fetched.last_seen_at, 99);
        assert_eq!(
            fetched.first_seen_at, 10,
            "first_seen_at is immutable on conflict (sqlite parity)"
        );
        assert_eq!(
            fetched.created_at, 10,
            "created_at is immutable on conflict (sqlite parity)"
        );

        // Soft-delete, then revive with a fresh upsert carrying deleted_at:
        // None — both adapters write the caller's value; the upsert path
        // (not the store) clears the tombstone, exactly like SQLite's
        // `deleted_at.eq(row.deleted_at)` on conflict.
        store.delete_node(&Id::new("n1")).await.unwrap();
        let revived = store.get_node(&Id::new("n1")).await.unwrap().unwrap();
        assert!(revived.deleted_at.is_some(), "delete_node tombstones");
        let mut third = node("n1", "pkg.A");
        third.deleted_at = None; // do_extract clears the tombstone pre-upsert
        store.create_node(third).await.unwrap();
        let fetched = store.get_node(&Id::new("n1")).await.unwrap().unwrap();
        assert!(
            fetched.deleted_at.is_none(),
            "revive via upsert with deleted_at: None clears the tombstone"
        );
        let listed = store.list_nodes(&Id::new("repo1"), None).await.unwrap();
        assert_eq!(listed.len(), 1, "revive must not duplicate the row");
    }
}
