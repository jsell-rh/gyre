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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum GlobalRole {
    TenantAdmin,
    Member,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Theme {
    Light,
    Dark,
    #[default]
    System,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserPreferences {
    pub default_workspace_id: Option<Id>,
    pub theme: Theme,
    pub notification_channels: NotificationChannels,
    pub editor_font_size: u32,
}

impl Default for UserPreferences {
    fn default() -> Self {
        Self {
            default_workspace_id: None,
            theme: Theme::default(),
            notification_channels: NotificationChannels::default(),
            editor_font_size: 14,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationChannels {
    pub in_app: bool,
    pub email_enabled: bool,
    pub email_digest: DigestFrequency,
}

impl Default for NotificationChannels {
    fn default() -> Self {
        Self {
            in_app: true,
            email_enabled: false,
            email_digest: DigestFrequency::Off,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DigestFrequency {
    Immediate,
    Hourly,
    Daily,
    Off,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: Id,
    /// Keycloak subject ID (JWT `sub` claim).
    pub external_id: String,
    /// Unique, URL-safe username (derived from SSO preferred_username).
    pub username: String,
    /// Human-readable display name, editable by user.
    pub display_name: String,
    pub email: Option<String>,
    pub avatar_url: Option<String>,
    pub timezone: String,
    pub locale: String,
    pub tenant_id: Option<Id>,
    pub global_role: GlobalRole,
    pub preferences: UserPreferences,
    pub roles: Vec<UserRole>,
    pub last_login_at: Option<u64>,
    pub created_at: u64,
    pub updated_at: u64,
}

impl User {
    pub fn new(id: Id, external_id: impl Into<String>, name: impl Into<String>, now: u64) -> Self {
        let name = name.into();
        Self {
            id,
            external_id: external_id.into(),
            username: name.clone(),
            display_name: name,
            email: None,
            avatar_url: None,
            timezone: "UTC".to_string(),
            locale: "en".to_string(),
            tenant_id: None,
            global_role: GlobalRole::Member,
            preferences: UserPreferences::default(),
            roles: vec![UserRole::ReadOnly],
            last_login_at: None,
            created_at: now,
            updated_at: now,
        }
    }

    /// Backwards-compatible name accessor.
    pub fn name(&self) -> &str {
        &self.display_name
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

    #[test]
    fn user_name_backwards_compat() {
        let u = User::new(Id::new("u1"), "ext-1", "Jordan Sell", 1000);
        assert_eq!(u.name(), "Jordan Sell");
        assert_eq!(u.display_name, "Jordan Sell");
        assert_eq!(u.username, "Jordan Sell");
    }

    #[test]
    fn default_preferences() {
        let u = User::new(Id::new("u1"), "ext-1", "alice", 1000);
        assert_eq!(u.preferences.editor_font_size, 14);
        assert_eq!(u.preferences.theme, Theme::System);
        assert!(u.preferences.notification_channels.in_app);
    }
}
