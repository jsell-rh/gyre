use gyre_common::Id;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum UserRole {
    Admin,
    Developer,
    Agent,
    ReadOnly,
}

impl UserRole {
    pub fn as_str(&self) -> &'static str {
        match self {
            UserRole::Admin => "Admin",
            UserRole::Developer => "Developer",
            UserRole::Agent => "Agent",
            UserRole::ReadOnly => "ReadOnly",
        }
    }

    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "Admin" | "admin" => Some(UserRole::Admin),
            "Developer" | "developer" => Some(UserRole::Developer),
            "Agent" | "agent" => Some(UserRole::Agent),
            "ReadOnly" | "readonly" | "read_only" => Some(UserRole::ReadOnly),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: Id,
    /// Keycloak subject ID (JWT `sub` claim).
    pub external_id: String,
    pub name: String,
    pub email: Option<String>,
    pub roles: Vec<UserRole>,
    pub created_at: u64,
    pub updated_at: u64,
}

impl User {
    pub fn new(id: Id, external_id: impl Into<String>, name: impl Into<String>, now: u64) -> Self {
        Self {
            id,
            external_id: external_id.into(),
            name: name.into(),
            email: None,
            roles: vec![UserRole::ReadOnly],
            created_at: now,
            updated_at: now,
        }
    }

    pub fn is_admin(&self) -> bool {
        self.roles.contains(&UserRole::Admin)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_user_defaults_to_readonly() {
        let u = User::new(Id::new("u1"), "ext-1", "alice", 1000);
        assert_eq!(u.roles, vec![UserRole::ReadOnly]);
        assert!(!u.is_admin());
    }

    #[test]
    fn admin_role_detection() {
        let mut u = User::new(Id::new("u1"), "ext-1", "admin", 1000);
        u.roles = vec![UserRole::Admin];
        assert!(u.is_admin());
    }

    #[test]
    fn role_roundtrip() {
        for role in [
            UserRole::Admin,
            UserRole::Developer,
            UserRole::Agent,
            UserRole::ReadOnly,
        ] {
            assert_eq!(UserRole::from_str(role.as_str()), Some(role));
        }
    }
}
