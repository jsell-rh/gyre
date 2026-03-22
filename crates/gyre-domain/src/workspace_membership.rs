use gyre_common::Id;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
}
