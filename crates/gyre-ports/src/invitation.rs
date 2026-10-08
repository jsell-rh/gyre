//! Repository ports for tenant and workspace invitations
//! (user-management.md §Tenant-Level User Onboarding, §Workspace
//! Invitation Flow, §Invitation Expiry).

use anyhow::Result;
use async_trait::async_trait;
use gyre_common::Id;
use gyre_domain::{InvitationStatus, TenantInvitation, WorkspaceInvitation};

/// Persistence for tenant-level invitations.
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
    /// Transition an invitation to a terminal status, recording the
    /// acceptance timestamp when accepting.
    async fn update_status(
        &self,
        id: &Id,
        status: InvitationStatus,
        accepted_at: Option<u64>,
    ) -> Result<()>;
    /// Delete an invitation. Only used for administrative cleanup; normal
    /// lifecycle transitions go through `update_status`.
    async fn delete(&self, id: &Id) -> Result<()>;
}

/// Persistence for workspace-level invitations.
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
    /// Transition an invitation to a terminal status, recording the
    /// acceptance timestamp when accepting.
    async fn update_status(
        &self,
        id: &Id,
        status: InvitationStatus,
        accepted_at: Option<u64>,
    ) -> Result<()>;
    /// Delete an invitation. Only used for administrative cleanup; normal
    /// lifecycle transitions go through `update_status`.
    async fn delete(&self, id: &Id) -> Result<()>;
}
