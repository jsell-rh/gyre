//! Repo access model (user-management.md §Repo Access).
//!
//! Repo access is inherited from workspace membership by default:
//!
//! | Workspace Role | Repo Access                                           |
//! |---------------|-------------------------------------------------------|
//! | Owner         | Full (read, write, admin, manage gates, manage budget)|
//! | Admin         | Full (read, write, admin, manage gates)               |
//! | Developer     | Read, write, create MRs, create tasks, spawn agents   |
//! | Viewer        | Read-only                                             |
//!
//! Per-repo overrides can restrict (but not expand) access beyond the
//! workspace role: a Developer can be restricted to Viewer on a specific
//! repo, but a Viewer cannot be elevated to Developer on a specific repo
//! without changing their workspace role.

use gyre_common::Id;
use serde::{Deserialize, Serialize};

use crate::workspace_membership::WorkspaceRole;

/// A single repo-level capability a workspace role can carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RepoPermission {
    /// Read repo contents (clone, fetch, browse).
    Read,
    /// Write repo contents (push).
    Write,
    /// Administer the repo (settings, deletion, override management).
    Admin,
    /// Manage quality gates on the repo.
    ManageGates,
    /// Manage the workspace/repo budget.
    ManageBudget,
    /// Create merge requests.
    CreateMr,
    /// Create tasks.
    CreateTask,
    /// Spawn agents.
    SpawnAgent,
}

impl RepoPermission {
    pub fn as_str(&self) -> &'static str {
        match self {
            RepoPermission::Read => "read",
            RepoPermission::Write => "write",
            RepoPermission::Admin => "admin",
            RepoPermission::ManageGates => "manage_gates",
            RepoPermission::ManageBudget => "manage_budget",
            RepoPermission::CreateMr => "create_mr",
            RepoPermission::CreateTask => "create_task",
            RepoPermission::SpawnAgent => "spawn_agent",
        }
    }
}

impl WorkspaceRole {
    /// The repo permissions this workspace role carries, per the
    /// user-management.md §Repo Access table.
    pub fn repo_permissions(&self) -> &'static [RepoPermission] {
        match self {
            // Owner: Full (read, write, admin, manage gates, manage budget)
            WorkspaceRole::Owner => &[
                RepoPermission::Read,
                RepoPermission::Write,
                RepoPermission::Admin,
                RepoPermission::ManageGates,
                RepoPermission::ManageBudget,
                RepoPermission::CreateMr,
                RepoPermission::CreateTask,
                RepoPermission::SpawnAgent,
            ],
            // Admin: Full (read, write, admin, manage gates)
            WorkspaceRole::Admin => &[
                RepoPermission::Read,
                RepoPermission::Write,
                RepoPermission::Admin,
                RepoPermission::ManageGates,
                RepoPermission::CreateMr,
                RepoPermission::CreateTask,
                RepoPermission::SpawnAgent,
            ],
            // Developer: Read, write, create MRs, create tasks, spawn agents
            WorkspaceRole::Developer => &[
                RepoPermission::Read,
                RepoPermission::Write,
                RepoPermission::CreateMr,
                RepoPermission::CreateTask,
                RepoPermission::SpawnAgent,
            ],
            // Viewer: Read-only
            WorkspaceRole::Viewer => &[RepoPermission::Read],
        }
    }

    /// Whether this role carries the given repo permission.
    pub fn has_repo_permission(&self, perm: RepoPermission) -> bool {
        self.repo_permissions().contains(&perm)
    }
}

/// A per-repo, per-user access restriction (user-management.md §Repo Access).
///
/// The override caps the user's effective role on one repo at
/// `restricted_role`. Because the effective role is
/// `WorkspaceRole::most_restrictive(workspace_role, restricted_role)`, an
/// override can only restrict — never expand — access beyond the workspace
/// role.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoAccessOverride {
    pub id: Id,
    pub repo_id: Id,
    pub user_id: Id,
    /// The role cap this override imposes. MUST NOT be more privileged than
    /// the user's workspace role (see [`RepoAccessOverride::validate`]).
    pub restricted_role: WorkspaceRole,
    pub created_by: Id,
    pub created_at: u64,
}

impl RepoAccessOverride {
    pub fn new(
        id: Id,
        repo_id: Id,
        user_id: Id,
        restricted_role: WorkspaceRole,
        created_by: Id,
        created_at: u64,
    ) -> Self {
        Self {
            id,
            repo_id,
            user_id,
            restricted_role,
            created_by,
            created_at,
        }
    }

    /// Validate the restrict-only invariant against the user's workspace
    /// membership role: an override that would grant capabilities beyond the
    /// workspace role is rejected (user-management.md §Repo Access — a
    /// Viewer cannot be elevated to Developer on a specific repo).
    pub fn validate(&self, workspace_role: WorkspaceRole) -> Result<(), String> {
        if self.restricted_role.rank() > workspace_role.rank() {
            return Err(format!(
                "repo access override for user {} restricts to {}, which is more privileged than their workspace role {}; overrides can restrict but not expand access",
                self.user_id,
                self.restricted_role.as_str(),
                workspace_role.as_str()
            ));
        }
        Ok(())
    }

    /// The user's effective role on this repo: the more restrictive of their
    /// workspace role and this override.
    pub fn effective_role(&self, workspace_role: WorkspaceRole) -> WorkspaceRole {
        WorkspaceRole::most_restrictive(workspace_role, self.restricted_role)
    }
}

/// The user's effective repo role given their workspace membership role and
/// an optional per-repo override.
pub fn effective_repo_role(
    workspace_role: WorkspaceRole,
    override_role: Option<WorkspaceRole>,
) -> WorkspaceRole {
    match override_role {
        Some(o) => WorkspaceRole::most_restrictive(workspace_role, o),
        None => workspace_role,
    }
}

/// The user's effective repo permissions given their workspace membership
/// role and an optional per-repo override (derived from the effective role).
pub fn effective_repo_permissions(
    workspace_role: WorkspaceRole,
    override_role: Option<WorkspaceRole>,
) -> Vec<RepoPermission> {
    effective_repo_role(workspace_role, override_role)
        .repo_permissions()
        .to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_table_owner_full_access() {
        let perms = WorkspaceRole::Owner.repo_permissions();
        assert!(perms.contains(&RepoPermission::ManageBudget));
        assert!(perms.contains(&RepoPermission::ManageGates));
        assert!(perms.contains(&RepoPermission::Admin));
        assert!(perms.contains(&RepoPermission::Write));
        assert!(perms.contains(&RepoPermission::Read));
    }

    #[test]
    fn spec_table_admin_no_budget() {
        let perms = WorkspaceRole::Admin.repo_permissions();
        assert!(perms.contains(&RepoPermission::ManageGates));
        assert!(perms.contains(&RepoPermission::Admin));
        assert!(!perms.contains(&RepoPermission::ManageBudget));
    }

    #[test]
    fn spec_table_developer() {
        let perms = WorkspaceRole::Developer.repo_permissions();
        assert!(perms.contains(&RepoPermission::Read));
        assert!(perms.contains(&RepoPermission::Write));
        assert!(perms.contains(&RepoPermission::CreateMr));
        assert!(perms.contains(&RepoPermission::CreateTask));
        assert!(perms.contains(&RepoPermission::SpawnAgent));
        assert!(!perms.contains(&RepoPermission::Admin));
        assert!(!perms.contains(&RepoPermission::ManageGates));
        assert!(!perms.contains(&RepoPermission::ManageBudget));
    }

    #[test]
    fn spec_table_viewer_read_only() {
        assert_eq!(
            WorkspaceRole::Viewer.repo_permissions(),
            &[RepoPermission::Read]
        );
    }

    #[test]
    fn override_restricts_developer_to_viewer() {
        let o = RepoAccessOverride::new(
            Id::new("o1"),
            Id::new("repo1"),
            Id::new("u1"),
            WorkspaceRole::Viewer,
            Id::new("admin"),
            0,
        );
        o.validate(WorkspaceRole::Developer).unwrap();
        assert_eq!(
            o.effective_role(WorkspaceRole::Developer),
            WorkspaceRole::Viewer
        );
        // The restricted developer loses write on this repo.
        assert!(
            !effective_repo_permissions(WorkspaceRole::Developer, Some(WorkspaceRole::Viewer))
                .contains(&RepoPermission::Write)
        );
    }

    #[test]
    fn override_cannot_expand_viewer_to_developer() {
        let o = RepoAccessOverride::new(
            Id::new("o1"),
            Id::new("repo1"),
            Id::new("u1"),
            WorkspaceRole::Developer,
            Id::new("admin"),
            0,
        );
        let err = o.validate(WorkspaceRole::Viewer).unwrap_err();
        assert!(err.contains("restrict but not expand"));
        // Even if an expansion override were stored, the effective-role
        // computation still cannot grant more than the workspace role.
        assert_eq!(
            o.effective_role(WorkspaceRole::Viewer),
            WorkspaceRole::Viewer
        );
    }

    #[test]
    fn no_override_keeps_workspace_role() {
        assert_eq!(
            effective_repo_role(WorkspaceRole::Developer, None),
            WorkspaceRole::Developer
        );
        assert_eq!(
            effective_repo_permissions(WorkspaceRole::Developer, None),
            WorkspaceRole::Developer.repo_permissions().to_vec()
        );
    }

    #[test]
    fn override_to_same_role_is_no_op() {
        let o = RepoAccessOverride::new(
            Id::new("o1"),
            Id::new("repo1"),
            Id::new("u1"),
            WorkspaceRole::Developer,
            Id::new("admin"),
            0,
        );
        o.validate(WorkspaceRole::Developer).unwrap();
        assert_eq!(
            o.effective_role(WorkspaceRole::Developer),
            WorkspaceRole::Developer
        );
    }
}
