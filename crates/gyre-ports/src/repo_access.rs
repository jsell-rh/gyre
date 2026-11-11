use anyhow::Result;
use async_trait::async_trait;
use gyre_common::Id;
use gyre_domain::RepoAccessOverride;

/// Per-repo access overrides (user-management.md §Repo Access).
///
/// An override caps one user's effective role on one repo. Overrides can
/// restrict but never expand access beyond the workspace role — that
/// invariant is validated against the caller's membership at the service
/// layer (`RepoAccessOverride::validate`); this repository persists whatever
/// the service layer has already validated.
#[async_trait]
#[allow(clippy::double_must_use)] // async_trait generates a must_use future for each method.
pub trait RepoAccessOverrideRepository: Send + Sync {
    /// Persist a new override.
    ///
    /// Fails if an override for the same `(repo_id, user_id)` pair already
    /// exists — updating an override goes through [`delete`] + create so the
    /// restriction change is always an explicit operation (audit surface),
    /// never a silent in-place rewrite.
    async fn create(&self, o: &RepoAccessOverride) -> Result<()>;
    /// The override for `(repo_id, user_id)`, if any.
    async fn find_by_repo_and_user(
        &self,
        repo_id: &Id,
        user_id: &Id,
    ) -> Result<Option<RepoAccessOverride>>;
    /// All overrides for a repo.
    async fn list_by_repo(&self, repo_id: &Id) -> Result<Vec<RepoAccessOverride>>;
    /// Delete the override for `(repo_id, user_id)`. Returns whether a row
    /// was removed.
    async fn delete(&self, repo_id: &Id, user_id: &Id) -> Result<bool>;
}
