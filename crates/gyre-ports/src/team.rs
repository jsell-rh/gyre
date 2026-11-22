use anyhow::Result;
use async_trait::async_trait;
use gyre_common::Id;
use gyre_domain::{Team, TeamMembership};

/// Team storage (user-management.md §Team Management).
///
/// Teams are organizational, not authorization: callers enforce that team
/// members are workspace members; this port persists teams and memberships.
///
/// Per-handler auth — callers MUST verify the team's `workspace_id` matches
/// the caller's scope before invoking mutating methods.
#[allow(clippy::double_must_use)] // async_trait generates a must_use future for each method.
#[async_trait]
pub trait TeamRepository: Send + Sync {
    /// Create a team.
    /// Fails if a team with the same `slug` already exists in the workspace.
    async fn create(&self, team: &Team) -> Result<()>;

    async fn find_by_id(&self, id: &Id) -> Result<Option<Team>>;

    /// Find a team by its workspace-unique slug (the `team:{slug}`
    /// reference form used by notification routing and spec ownership).
    async fn find_by_slug(&self, workspace_id: &Id, slug: &str) -> Result<Option<Team>>;

    async fn list_by_workspace(&self, workspace_id: &Id) -> Result<Vec<Team>>;

    /// Update a team.
    /// Fails if the new `slug` collides with another team's slug in the
    /// same workspace.
    async fn update(&self, team: &Team) -> Result<()>;

    /// Delete a team and its memberships.
    async fn delete(&self, id: &Id) -> Result<()>;

    /// Add a member to a team.
    /// Fails if the user is already a member of the team.
    async fn add_member(&self, membership: &TeamMembership) -> Result<()>;

    /// Remove a member from a team. Ok when the membership does not exist
    /// (idempotent per docs/api-reference.md M22.8).
    async fn remove_member(&self, team_id: &Id, user_id: &Id) -> Result<()>;

    /// List a team's memberships, ordered by `added_at` ascending.
    async fn list_members(&self, team_id: &Id) -> Result<Vec<TeamMembership>>;

    /// List the teams in a workspace that count `user_id` as a member
    /// (team-scoped budget reporting and routing fan-in).
    async fn list_for_user(&self, workspace_id: &Id, user_id: &Id) -> Result<Vec<Team>>;
}
