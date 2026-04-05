//! Port trait for saved explorer views.

use anyhow::Result;
use async_trait::async_trait;
use gyre_common::Id;
use serde::{Deserialize, Serialize};

/// A saved explorer view query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedView {
    pub id: Id,
    pub repo_id: Id,
    pub workspace_id: Id,
    pub tenant_id: Id,
    pub name: String,
    pub description: Option<String>,
    pub query_json: String,
    pub created_by: String,
    pub created_at: u64,
    pub updated_at: u64,
    pub is_system: bool,
}

/// Storage port for saved explorer views.
#[async_trait]
pub trait SavedViewRepository: Send + Sync {
    async fn create(&self, view: SavedView) -> Result<SavedView>;
    async fn get(&self, id: &Id) -> Result<Option<SavedView>>;
    async fn list_by_repo(&self, repo_id: &Id) -> Result<Vec<SavedView>>;
    async fn list_by_workspace(&self, workspace_id: &Id) -> Result<Vec<SavedView>>;
    /// List views for a repo, filtered by tenant_id at the SQL level.
    /// Prefer this over list_by_repo + post-filter for defense-in-depth.
    async fn list_by_repo_and_tenant(
        &self,
        repo_id: &Id,
        tenant_id: &Id,
    ) -> Result<Vec<SavedView>> {
        // Default: fall back to list_by_repo + filter (backwards compat)
        let views = self.list_by_repo(repo_id).await?;
        Ok(views
            .into_iter()
            .filter(|v| v.tenant_id == *tenant_id)
            .collect())
    }
    async fn update(&self, view: SavedView) -> Result<SavedView>;
    /// Delete a view. Requires tenant_id for defense-in-depth: the WHERE clause
    /// includes tenant_id to prevent cross-tenant deletion even with a valid ID.
    async fn delete(&self, id: &Id, tenant_id: &Id) -> Result<()>;
    /// Delete a view with tenant_id guard for defense-in-depth.
    /// Returns true if a row was deleted, false if no matching row found.
    async fn delete_scoped(&self, id: &Id, tenant_id: &Id) -> Result<bool> {
        // Default: verify tenant before deleting (backwards compat)
        if let Some(view) = self.get(id).await? {
            if view.tenant_id != *tenant_id {
                return Ok(false);
            }
        }
        self.delete(id, tenant_id).await?;
        Ok(true)
    }
}
