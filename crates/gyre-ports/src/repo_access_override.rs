use anyhow::Result;
use async_trait::async_trait;
use gyre_common::Id;
use gyre_domain::{RepoAccessOverride, WorkspaceRole};

/// Per-repo access restrictions (user-management.md §Repo Access).
///
/// An override restricts a member's effective role on one repo to the more
/// restrictive of their workspace role and the override role. Overrides can
/// never expand access beyond the workspace role; the domain layer clamps at
/// evaluation time (`effective_repo_role`) and rejects expansion attempts at
/// creation time (`RepoAccessOverride::validate_restriction`).
#[async_trait]
#[allow(clippy::double_must_use)] // async_trait generates a must_use future for each method.
pub trait RepoAccessOverrideRepository: Send + Sync {
    /// Persist a new override.
    ///
    /// Fails if an override for the same `(repo_id, user_id)` pair already
    /// exists — use `update_role` to change it.
    async fn create(&self, override_: &RepoAccessOverride) -> Result<()>;
    async fn find_by_id(&self, id: &Id) -> Result<Option<RepoAccessOverride>>;
    async fn find_by_repo_and_user(
        &self,
        repo_id: &Id,
        user_id: &Id,
    ) -> Result<Option<RepoAccessOverride>>;
    async fn list_by_repo(&self, repo_id: &Id) -> Result<Vec<RepoAccessOverride>>;
    /// Change the restriction role of an existing override.
    async fn update_role(&self, id: &Id, role: WorkspaceRole) -> Result<()>;
    async fn delete(&self, id: &Id) -> Result<()>;
}
