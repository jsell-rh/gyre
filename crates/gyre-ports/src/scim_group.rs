//! Repository ports for SCIM group provisioning and the per-workspace
//! IdP-group-to-role mapping (identity-security.md §SCIM Provisioning /
//! §Group-to-Workspace Mapping).
//!
//! Both ports are durable: the production wiring is the SQLite/PostgreSQL
//! adapter pair over migration `000068_scim_groups_and_mappings`. An
//! in-memory `AppState` store is prohibited for this state (restart
//! orphaning — see the standing invariants in AGENTS.md).

use anyhow::Result;
use async_trait::async_trait;
use gyre_common::Id;
use gyre_domain::{ScimGroup, ScimGroupMember, ScimWorkspaceMapping};

/// Persistence for SCIM Group resources and their membership sets.
// clippy 1.99 + async-trait: the macro's desugared `#[must_use]` wrapper
// on methods returning `Result` trips `double_must_use` at the trait
// declaration. It is an expansion artifact affecting every
// `#[async_trait]` trait on this toolchain, not a defect in this trait.
#[allow(clippy::double_must_use)]
#[async_trait]
pub trait ScimGroupRepository: Send + Sync {
    /// Store a new group. Fails if a group with the same non-empty
    /// `external_id` already exists (the IdP join key is unique).
    async fn create(&self, group: &ScimGroup) -> Result<()>;
    async fn find_by_id(&self, id: &Id) -> Result<Option<ScimGroup>>;
    /// Look up a group by its IdP join key (RFC 7644 externalId).
    async fn find_by_external_id(&self, external_id: &str) -> Result<Option<ScimGroup>>;
    /// All groups, oldest first (stable order for startIndex/count
    /// pagination — RFC 7644 §3.4.2.4).
    async fn list(&self) -> Result<Vec<ScimGroup>>;
    /// Update the mutable group attributes (displayName, updated_at).
    /// The externalId join key is immutable after creation, like the
    /// Users surface.
    async fn update(&self, group: &ScimGroup) -> Result<()>;
    /// Delete the group AND its membership rows.
    async fn delete(&self, id: &Id) -> Result<()>;
    /// All members of a group.
    async fn list_members(&self, group_id: &Id) -> Result<Vec<ScimGroupMember>>;
    /// Add one member. Idempotent: adding an existing member is `Ok(())`
    /// (re-pushing the same membership must not duplicate rows —
    /// task-266).
    async fn add_member(&self, member: &ScimGroupMember) -> Result<()>;
    /// Remove one member. Removing a non-member is `Ok(())`.
    async fn remove_member(&self, group_id: &Id, user_id: &Id) -> Result<()>;
    /// Replace the group's membership set with exactly `members`
    /// (RFC 7644 PUT semantics). Atomic: readers never observe a
    /// half-replaced set.
    async fn replace_members(&self, group_id: &Id, members: &[ScimGroupMember]) -> Result<()>;
}

/// Persistence for the admin-configured per-workspace
/// IdP-group → workspace-role mappings.
// Same async-trait expansion artifact as above.
#[allow(clippy::double_must_use)]
#[async_trait]
pub trait ScimMappingRepository: Send + Sync {
    /// Replace the workspace's whole mapping set atomically (PUT
    /// semantics — the request body is the authoritative set).
    async fn replace_for_workspace(
        &self,
        workspace_id: &Id,
        mappings: &[ScimWorkspaceMapping],
    ) -> Result<()>;
    async fn list_by_workspace(&self, workspace_id: &Id) -> Result<Vec<ScimWorkspaceMapping>>;
    /// Every mapping (across workspaces) whose `idp_group` equals the
    /// given key — the lookup the group-membership effect consults.
    async fn list_by_idp_group(&self, idp_group: &str) -> Result<Vec<ScimWorkspaceMapping>>;
}
