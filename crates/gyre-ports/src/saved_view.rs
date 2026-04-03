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
    async fn update(&self, view: SavedView) -> Result<SavedView>;
    async fn delete(&self, id: &Id) -> Result<()>;
}
