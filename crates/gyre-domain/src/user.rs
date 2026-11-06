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
    pub email: EmailConfig,
    pub webhook: Option<WebhookConfig>,
    pub slack: Option<SlackConfig>,
}

impl Default for NotificationChannels {
    fn default() -> Self {
        Self {
            in_app: true, // Always true (can't disable) — per user-management.md §Delivery Channels.
            email: EmailConfig::default(),
            webhook: None,
            slack: None,
        }
    }
}

/// Email channel configuration (user-management.md §Delivery Channels).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmailConfig {
    pub enabled: bool,
    pub digest: DigestFrequency,
    /// Only email notifications at or above this priority.
    pub min_priority: NotificationPriority,
}

impl Default for EmailConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            digest: DigestFrequency::Off,
            min_priority: NotificationPriority::Medium,
        }
    }
}

/// Outbound webhook channel configuration. The secret signs payloads with HMAC-SHA256.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebhookConfig {
    pub url: String,
    pub secret: String,
    /// Only deliver webhook notifications at or above this priority.
    pub min_priority: NotificationPriority,
}

/// Slack incoming-webhook channel configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SlackConfig {
    pub webhook_url: String,
    /// Override channel (default: DM).
    pub channel: Option<String>,
    /// Only deliver Slack notifications at or above this priority.
    pub min_priority: NotificationPriority,
}

/// Delivery priority bands used for per-channel threshold filtering
/// (user-management.md §Notification Entity).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum NotificationPriority {
    Low,
    Medium,
    High,
    Urgent,
}

impl NotificationPriority {
    /// Parses the wire representation (case-insensitive).
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "low" => Some(Self::Low),
            "medium" => Some(Self::Medium),
            "high" => Some(Self::High),
            "urgent" => Some(Self::Urgent),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Urgent => "urgent",
        }
    }

    /// Maps a 1–10 HSI §8 notification priority onto the 4-band scale of
    /// user-management.md §Notification Entity:
    /// 1–3 Urgent, 4–6 High, 7–8 Medium, 9–10 Low.
    pub fn from_band(p: u8) -> Self {
        match p {
            1..=3 => Self::Urgent,
            4..=6 => Self::High,
            7..=8 => Self::Medium,
            _ => Self::Low,
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
