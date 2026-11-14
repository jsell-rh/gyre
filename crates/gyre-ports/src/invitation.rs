//! Repository ports for tenant and workspace invitations
//! (user-management.md §Tenant-Level User Onboarding, §Workspace
//! Invitation Flow, §Invitation Expiry).

use anyhow::Result;
use async_trait::async_trait;
use gyre_common::Id;
use gyre_domain::{InvitationStatus, TenantInvitation, WorkspaceInvitation};

/// Persistence for tenant-level invitations.
// clippy 1.99 + async-trait: the macro's desugared `#[must_use]` wrapper
// on methods returning `Result` trips `double_must_use` at the trait
// declaration. It is an expansion artifact affecting every
// `#[async_trait]` trait on this toolchain, not a defect in this trait.
#[allow(clippy::double_must_use)]
#[async_trait]
pub trait TenantInvitationRepository: Send + Sync {
    /// Store a new invitation. Fails if a pending invitation for the same
    /// `(tenant_id, email)` already exists — re-invite requires revoking or
    /// waiting for expiry first (spec §Invitation Expiry: re-inviting after
    /// expiry creates a new invitation with a new token).
    async fn create(&self, invitation: &TenantInvitation) -> Result<()>;
    async fn find_by_id(&self, id: &Id) -> Result<Option<TenantInvitation>>;
    /// Look up an invitation by the SHA-256 hash of its token. Used by the
    /// magic-link accept endpoint.
    async fn find_by_token_hash(&self, token_hash: &str) -> Result<Option<TenantInvitation>>;
    /// All invitations for a tenant, newest first.
    async fn list_by_tenant(&self, tenant_id: &Id) -> Result<Vec<TenantInvitation>>;
    /// All invitations across tenants with the given status (used by the
    /// expiry job).
    async fn list_by_status(&self, status: InvitationStatus) -> Result<Vec<TenantInvitation>>;
    /// Atomically transition the invitation from `from` to `to`.
    ///
    /// Returns `Ok(false)` — not an error — when the invitation is missing
    /// or no longer in `from` (it was accepted, declined, revoked, expired,
    /// or claimed concurrently). This is the single-use guard for
    /// acceptance: callers perform the transition FIRST and only proceed
    /// with user/membership creation when it returns `true`, so two
    /// concurrent accepts of the same token cannot both win. Adapters must
    /// enforce the `from` precondition in the write itself (a conditional
    /// UPDATE / a check-and-set under the store lock), never as a separate
    /// read-then-write.
    async fn transition_status(
        &self,
        id: &Id,
        from: InvitationStatus,
        to: InvitationStatus,
        accepted_at: Option<u64>,
    ) -> Result<bool>;
    /// Delete an invitation. Only used for administrative cleanup; normal
    /// lifecycle transitions go through [`Self::transition_status`].
    async fn delete(&self, id: &Id) -> Result<()>;
}

/// Persistence for workspace-level invitations.
// Same async-trait expansion artifact as above — see the note on
// `TenantInvitationRepository`.
#[allow(clippy::double_must_use)]
#[async_trait]
pub trait WorkspaceInvitationRepository: Send + Sync {
    /// Store a new invitation. Fails if a pending invitation for the same
    /// `(workspace_id, user_id)` already exists.
    async fn create(&self, invitation: &WorkspaceInvitation) -> Result<()>;
    async fn find_by_id(&self, id: &Id) -> Result<Option<WorkspaceInvitation>>;
    /// Look up an invitation by the SHA-256 hash of its token.
    async fn find_by_token_hash(&self, token_hash: &str) -> Result<Option<WorkspaceInvitation>>;
    /// All invitations for a workspace (all statuses — the settings page
    /// shows pending, the admin panel shows expired/accepted too).
    async fn list_by_workspace(&self, workspace_id: &Id) -> Result<Vec<WorkspaceInvitation>>;
    /// All invitations for a user across workspaces (the invitee's
    /// "pending invitations" view).
    async fn list_by_user(&self, user_id: &Id) -> Result<Vec<WorkspaceInvitation>>;
    /// All invitations across workspaces with the given status (used by the
    /// expiry job).
    async fn list_by_status(&self, status: InvitationStatus) -> Result<Vec<WorkspaceInvitation>>;
    /// Atomically transition the invitation from `from` to `to`.
    ///
    /// Same contract as
    /// [`TenantInvitationRepository::transition_status`]: `Ok(false)` when
    /// the invitation is not (or no longer) in `from`; the conditional
    /// write is the single-use acceptance guard.
    async fn transition_status(
        &self,
        id: &Id,
        from: InvitationStatus,
        to: InvitationStatus,
        accepted_at: Option<u64>,
    ) -> Result<bool>;
    /// Delete an invitation. Only used for administrative cleanup; normal
    /// lifecycle transitions go through [`Self::transition_status`].
    async fn delete(&self, id: &Id) -> Result<()>;
}
