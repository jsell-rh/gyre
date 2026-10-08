//! Tenant and workspace invitation entities (user-management.md
//! §Tenant-Level User Onboarding, §Workspace Invitation Flow,
//! §Invitation Expiry).
//!
//! Two invitation kinds exist:
//!
//! - [`TenantInvitation`]: onboards a *new* user into the tenant by email.
//!   Accepting creates the user (local mode) or links an existing SSO
//!   account, then activates any pre-assigned workspace memberships.
//! - [`WorkspaceInvitation`]: grants an *existing* tenant user access to a
//!   workspace. Accepting activates the stored [`WorkspaceMembership`].
//!
//! Both kinds share the [`InvitationStatus`] lifecycle and expire via the
//! background job (expired rows are marked, never deleted, for audit).

use crate::workspace_membership::WorkspaceRole;
use crate::user::GlobalRole;
use gyre_common::Id;
use serde::{Deserialize, Serialize};

/// Lifecycle state shared by tenant and workspace invitations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum InvitationStatus {
    Pending,
    Accepted,
    Declined,
    Expired,
    Revoked,
}

impl InvitationStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            InvitationStatus::Pending => "Pending",
            InvitationStatus::Accepted => "Accepted",
            InvitationStatus::Declined => "Declined",
            InvitationStatus::Expired => "Expired",
            InvitationStatus::Revoked => "Revoked",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "Pending" => Some(InvitationStatus::Pending),
            "Accepted" => Some(InvitationStatus::Accepted),
            "Declined" => Some(InvitationStatus::Declined),
            "Expired" => Some(InvitationStatus::Expired),
            "Revoked" => Some(InvitationStatus::Revoked),
            _ => None,
        }
    }
}

/// Invitation to join a tenant (user-management.md §Tenant-Level User
/// Onboarding).
///
/// The invitation token is delivered to the invitee out of band (magic
/// link); only its SHA-256 hash is stored.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TenantInvitation {
    pub id: Id,
    pub tenant_id: Id,
    pub email: String,
    pub invited_by: Id,
    pub role: GlobalRole,
    /// Optionally pre-assigned workspaces, parallel to `workspace_roles`.
    pub workspace_ids: Vec<Id>,
    /// Role per pre-assigned workspace, parallel to `workspace_ids`.
    pub workspace_roles: Vec<WorkspaceRole>,
    pub status: InvitationStatus,
    /// SHA-256 hex of the invitation token (magic link).
    pub token_hash: String,
    pub expires_at: u64,
    pub created_at: u64,
    pub accepted_at: Option<u64>,
}

impl TenantInvitation {
    pub fn is_expired(&self, now: u64) -> bool {
        self.status == InvitationStatus::Pending && now >= self.expires_at
    }
}

/// Invitation for an existing tenant user to join a workspace
/// (user-management.md §Workspace Invitation Flow).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceInvitation {
    pub id: Id,
    pub tenant_id: Id,
    pub workspace_id: Id,
    pub user_id: Id,
    pub invited_by: Id,
    pub role: WorkspaceRole,
    pub status: InvitationStatus,
    /// SHA-256 hex of the invitation token.
    pub token_hash: String,
    pub expires_at: u64,
    pub created_at: u64,
    pub accepted_at: Option<u64>,
}

impl WorkspaceInvitation {
    pub fn is_expired(&self, now: u64) -> bool {
        self.status == InvitationStatus::Pending && now >= self.expires_at
    }
}

/// Configurable invitation policy (user-management.md §Invitation Expiry).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvitationPolicy {
    /// Tenant invitation expiry in days (default: 7).
    pub tenant_invite_expiry_days: u32,
    /// Workspace invitation expiry in days (default: 7).
    pub workspace_invite_expiry_days: u32,
    /// Max pending workspace invitations per workspace (default: 50).
    pub max_pending_invitations: u32,
    /// Re-invite after expiry (default: true).
    pub allow_re_invite: bool,
}

impl Default for InvitationPolicy {
    fn default() -> Self {
        Self {
            tenant_invite_expiry_days: 7,
            workspace_invite_expiry_days: 7,
            max_pending_invitations: 50,
            allow_re_invite: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_roundtrip() {
        for s in [
            InvitationStatus::Pending,
            InvitationStatus::Accepted,
            InvitationStatus::Declined,
            InvitationStatus::Expired,
            InvitationStatus::Revoked,
        ] {
            assert_eq!(InvitationStatus::parse(s.as_str()), Some(s));
        }
        assert_eq!(InvitationStatus::parse("nonsense"), None);
    }

    #[test]
    fn tenant_invitation_expiry_only_when_pending() {
        let mut inv = TenantInvitation {
            id: Id::new("inv1"),
            tenant_id: Id::new("t1"),
            email: "a@example.com".to_string(),
            invited_by: Id::new("u1"),
            role: GlobalRole::Member,
            workspace_ids: vec![],
            workspace_roles: vec![],
            status: InvitationStatus::Pending,
            token_hash: "h".to_string(),
            expires_at: 1000,
            created_at: 0,
            accepted_at: None,
        };
        // Pending + past expiry → expired.
        assert!(inv.is_expired(1500));
        // Not yet past expiry → not expired.
        assert!(!inv.is_expired(999));
        // Non-pending statuses never report expired (the expiry job only
        // transitions Pending rows).
        inv.status = InvitationStatus::Accepted;
        assert!(!inv.is_expired(1500));
        inv.status = InvitationStatus::Revoked;
        assert!(!inv.is_expired(1500));
    }

    #[test]
    fn workspace_invitation_expiry_only_when_pending() {
        let inv = WorkspaceInvitation {
            id: Id::new("wi1"),
            tenant_id: Id::new("t1"),
            workspace_id: Id::new("ws1"),
            user_id: Id::new("u1"),
            invited_by: Id::new("u2"),
            role: WorkspaceRole::Developer,
            status: InvitationStatus::Pending,
            token_hash: "h".to_string(),
            expires_at: 100,
            created_at: 0,
            accepted_at: None,
        };
        assert!(inv.is_expired(100));
        assert!(!inv.is_expired(99));
    }

    #[test]
    fn policy_defaults() {
        let p = InvitationPolicy::default();
        assert_eq!(p.tenant_invite_expiry_days, 7);
        assert_eq!(p.workspace_invite_expiry_days, 7);
        assert_eq!(p.max_pending_invitations, 50);
        assert!(p.allow_re_invite);
    }
}
