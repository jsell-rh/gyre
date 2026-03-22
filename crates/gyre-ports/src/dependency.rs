//! Port trait for the cross-repo dependency graph.

use anyhow::Result;
use async_trait::async_trait;
use gyre_common::Id;
use gyre_domain::DependencyEdge;

/// Port for persisting and querying cross-repo dependency edges.
#[async_trait]
pub trait DependencyRepository: Send + Sync {
    /// Insert or replace a dependency edge.
    async fn save(&self, edge: &DependencyEdge) -> Result<()>;

    /// Fetch a single edge by ID.
    async fn find_by_id(&self, id: &Id) -> Result<Option<DependencyEdge>>;

    /// Outgoing dependencies: all edges where source_repo_id == repo_id.
    async fn list_by_repo(&self, repo_id: &Id) -> Result<Vec<DependencyEdge>>;

    /// Incoming dependencies: all edges where target_repo_id == repo_id.
    async fn list_dependents(&self, repo_id: &Id) -> Result<Vec<DependencyEdge>>;

    /// All edges in the graph (tenant-wide).
    async fn list_all(&self) -> Result<Vec<DependencyEdge>>;

    /// Remove an edge by ID. Returns true if an edge was deleted.
    async fn delete(&self, id: &Id) -> Result<bool>;
}
