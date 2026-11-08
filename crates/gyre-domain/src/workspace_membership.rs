use gyre_common::Id;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorkspaceRole {
    Owner,
    Admin,
    Developer,
    Viewer,
}

impl WorkspaceRole {
    pub fn as_str(&self) -> &'static str {
        match self {
            WorkspaceRole::Owner => "Owner",
            WorkspaceRole::Admin => "Admin",
            WorkspaceRole::Developer => "Developer",
            WorkspaceRole::Viewer => "Viewer",
        }
    }

    pub fn parse_role(s: &str) -> Option<Self> {
        match s {
            "Owner" | "owner" => Some(WorkspaceRole::Owner),
            "Admin" | "admin" => Some(WorkspaceRole::Admin),
            "Developer" | "developer" => Some(WorkspaceRole::Developer),
            "Viewer" | "viewer" => Some(WorkspaceRole::Viewer),
            _ => None,
        }
    }

    /// Privilege rank used to compare roles. Higher rank = more privilege.
    /// Owner(3) > Admin(2) > Developer(1) > Viewer(0).
    pub fn rank(&self) -> u8 {
        match self {
            WorkspaceRole::Owner => 3,
            WorkspaceRole::Admin => 2,
            WorkspaceRole::Developer => 1,
            WorkspaceRole::Viewer => 0,
        }
    }

    /// Parse a role, rejecting rank ordering surprises defensively.
    pub fn from_rank(rank: u8) -> Option<Self> {
        match rank {
            3 => Some(WorkspaceRole::Owner),
            2 => Some(WorkspaceRole::Admin),
            1 => Some(WorkspaceRole::Developer),
            0 => Some(WorkspaceRole::Viewer),
            _ => None,
        }
    }

    /// The repo access permission set granted by this workspace role
    /// (user-management.md §Repo Access).
    ///
    /// | Role        | Permissions                                              |
    /// |-------------|----------------------------------------------------------|
    /// | Owner       | read, write, admin, manage_gates, manage_budget           |
    /// | Admin       | read, write, admin, manage_gates                          |
    /// | Developer   | read, write, create_mr, create_task, spawn_agent          |
    /// | Viewer      | read                                                     |
    pub fn repo_permissions(&self) -> &'static [RepoPermission] {
        use RepoPermission::*;
        match self {
            WorkspaceRole::Owner => &[
                Read,
                Write,
                Admin,
                ManageGates,
                ManageBudget,
                CreateMr,
                CreateTask,
                SpawnAgent,
            ],
            WorkspaceRole::Admin => &[
                Read,
                Write,
                Admin,
                ManageGates,
                CreateMr,
                CreateTask,
                SpawnAgent,
            ],
            WorkspaceRole::Developer => &[Read, Write, CreateMr, CreateTask, SpawnAgent],
            WorkspaceRole::Viewer => &[Read],
        }
    }

    /// Whether this role grants the given repo permission.
    pub fn allows_repo_permission(&self, perm: RepoPermission) -> bool {
        self.repo_permissions().contains(&perm)
    }
}

/// The effective role a member has on a repo: the MORE restrictive of their
/// workspace role and any per-repo override. Overrides can restrict but never
/// expand access beyond the workspace role (user-management.md §Repo Access),
/// so the lower-rank (less privileged) role always wins.
pub fn effective_repo_role(
    workspace_role: WorkspaceRole,
    override_role: WorkspaceRole,
) -> WorkspaceRole {
    if workspace_role.rank() <= override_role.rank() {
        workspace_role
    } else {
        override_role
    }
}

/// A repo access permission, derived from workspace role per
/// user-management.md §Repo Access.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RepoPermission {
    /// Read repo data (files, commits, MRs, status).
    Read,
    /// Write repo content (branches, commits, repo settings).
    Write,
    /// Administer the repo (archive, delete, configure, manage overrides).
    Admin,
    /// Manage quality gates on the repo.
    ManageGates,
    /// Manage the workspace budget that funds repo agents.
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceMembership {
    pub id: Id,
    pub user_id: Id,
    pub workspace_id: Id,
    pub role: WorkspaceRole,
    pub invited_by: Id,
    pub accepted: bool,
    pub accepted_at: Option<u64>,
    pub created_at: u64,
}

impl WorkspaceMembership {
    pub fn new(
        id: Id,
        user_id: Id,
        workspace_id: Id,
        role: WorkspaceRole,
        invited_by: Id,
        now: u64,
    ) -> Self {
        Self {
            id,
            user_id,
            workspace_id,
            role,
            invited_by,
            accepted: false,
            accepted_at: None,
            created_at: now,
        }
    }

    pub fn accept(&mut self, now: u64) {
        self.accepted = true;
        self.accepted_at = Some(now);
    }

    /// Whether this membership grants access to workspace resources. A
    /// membership is active once accepted (user-management.md §Workspace
    /// Invitation Flow: "On accept: membership is active, user gains access
    /// to workspace repos").
    pub fn is_active(&self) -> bool {
        self.accepted
    }
}

/// A per-repo restriction on a member's access
/// (user-management.md §Repo Access).
///
/// Overrides can restrict but never expand: the member's effective role on
/// the repo is the more restrictive of their workspace role and the override
/// role. Setting an override whose rank exceeds the member's workspace role
/// is rejected at creation time, and the evaluation-time clamp
/// (`effective_repo_role`) enforces restrict-only even if the workspace role
/// changes later.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoAccessOverride {
    pub id: Id,
    pub repo_id: Id,
    pub user_id: Id,
    /// The role this member is restricted to on this repo.
    pub role: WorkspaceRole,
    pub created_by: Id,
    pub created_at: u64,
    pub updated_at: u64,
}

impl RepoAccessOverride {
    pub fn new(
        id: Id,
        repo_id: Id,
        user_id: Id,
        role: WorkspaceRole,
        created_by: Id,
        now: u64,
    ) -> Self {
        Self {
            id,
            repo_id,
            user_id,
            role,
            created_by,
            created_at: now,
            updated_at: now,
        }
    }

    /// Validate a restrict-only override: the override role must not grant
    /// more than the member's current workspace role. Returns the override
    /// role when valid (the caller stores it), or the expansion attempt.
    pub fn validate_restriction(
        override_role: WorkspaceRole,
        workspace_role: WorkspaceRole,
    ) -> Result<WorkspaceRole, WorkspaceRole> {
        if override_role.rank() > workspace_role.rank() {
            Err(override_role)
        } else {
            Ok(override_role)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn membership_starts_pending() {
        let m = WorkspaceMembership::new(
            Id::new("m1"),
            Id::new("u1"),
            Id::new("ws1"),
            WorkspaceRole::Developer,
            Id::new("admin"),
            1000,
        );
        assert!(!m.accepted);
        assert!(m.accepted_at.is_none());
    }

    #[test]
    fn accept_membership() {
        let mut m = WorkspaceMembership::new(
            Id::new("m1"),
            Id::new("u1"),
            Id::new("ws1"),
            WorkspaceRole::Developer,
            Id::new("admin"),
            1000,
        );
        m.accept(2000);
        assert!(m.accepted);
        assert_eq!(m.accepted_at, Some(2000));
    }

    #[test]
    fn role_roundtrip() {
        for role in [
            WorkspaceRole::Owner,
            WorkspaceRole::Admin,
            WorkspaceRole::Developer,
            WorkspaceRole::Viewer,
        ] {
            assert_eq!(WorkspaceRole::parse_role(role.as_str()), Some(role));
        }
    }

    #[test]
    fn repo_permissions_match_spec_table() {
        use RepoPermission::*;
        // Owner: full — read, write, admin, manage gates, manage budget.
        for p in [Read, Write, Admin, ManageGates, ManageBudget] {
            assert!(WorkspaceRole::Owner.allows_repo_permission(p));
        }
        // Admin: read, write, admin, manage gates — but NOT budget.
        for p in [Read, Write, Admin, ManageGates] {
            assert!(WorkspaceRole::Admin.allows_repo_permission(p));
        }
        assert!(!WorkspaceRole::Admin.allows_repo_permission(ManageBudget));
        // Developer: read, write, create MRs/tasks, spawn agents — no admin.
        for p in [Read, Write, CreateMr, CreateTask, SpawnAgent] {
            assert!(WorkspaceRole::Developer.allows_repo_permission(p));
        }
        assert!(!WorkspaceRole::Developer.allows_repo_permission(Admin));
        assert!(!WorkspaceRole::Developer.allows_repo_permission(ManageGates));
        // Viewer: read-only.
        assert!(WorkspaceRole::Viewer.allows_repo_permission(Read));
        for p in [
            Write,
            Admin,
            ManageGates,
            ManageBudget,
            CreateMr,
            CreateTask,
            SpawnAgent,
        ] {
            assert!(!WorkspaceRole::Viewer.allows_repo_permission(p));
        }
    }

    #[test]
    fn effective_role_is_always_the_more_restrictive() {
        use WorkspaceRole::*;
        // Override below workspace role: restricts.
        assert_eq!(effective_repo_role(Developer, Viewer), Viewer);
        // Override equal to workspace role: no change.
        assert_eq!(effective_repo_role(Developer, Developer), Developer);
        // Override above workspace role: clamped — can never expand.
        assert_eq!(effective_repo_role(Viewer, Owner), Viewer);
        assert_eq!(effective_repo_role(Developer, Owner), Developer);
        assert_eq!(effective_repo_role(Admin, Owner), Admin);
    }

    #[test]
    fn override_validation_rejects_expansion() {
        use WorkspaceRole::*;
        // A Viewer cannot be elevated to Developer on a specific repo.
        assert!(
            RepoAccessOverride::validate_restriction(Developer, Viewer).is_err(),
            "override above workspace role must be rejected"
        );
        // A Developer restricted to Viewer on a specific repo is allowed.
        assert!(RepoAccessOverride::validate_restriction(Viewer, Developer).is_ok());
        // Equal rank is a no-op restriction, allowed.
        assert!(RepoAccessOverride::validate_restriction(Developer, Developer).is_ok());
    }
}
