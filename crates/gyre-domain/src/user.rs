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

/// UI spacing preference (user-management.md §User Preferences).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum UiDensity {
    Compact,
    #[default]
    Comfortable,
    Spacious,
}

/// Code diff rendering preference.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum DiffView {
    #[default]
    SideBySide,
    Unified,
}

/// Default scope of the activity feed.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum FeedScope {
    #[default]
    MyActivity,
    Workspace,
    All,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UserPreferences {
    pub default_workspace_id: Option<Id>,
    pub theme: Theme,
    pub notification_channels: NotificationChannels,
    pub ui_density: UiDensity,
    /// Code viewer font size in pixels.
    pub code_font_size: u32,
    pub diff_view: DiffView,
    pub activity_feed_scope: FeedScope,
}

impl Default for UserPreferences {
    fn default() -> Self {
        Self {
            default_workspace_id: None,
            theme: Theme::default(),
            notification_channels: NotificationChannels::default(),
            ui_density: UiDensity::default(),
            code_font_size: 14,
            diff_view: DiffView::default(),
            activity_feed_scope: FeedScope::default(),
        }
    }
}

/// Partial view of [`UserPreferences`] used by `PUT /api/v1/users/me`:
/// every field is optional; omitted fields keep their current value.
///
/// This mirrors the handler's partial-update contract for the top-level
/// profile fields (display_name/timezone/locale are each `Option`), so a
/// preferences payload can update just `theme` without re-sending
/// `notification_channels` and friends.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct UserPreferencesPatch {
    pub default_workspace_id: Option<Id>,
    pub theme: Option<Theme>,
    pub notification_channels: Option<NotificationChannels>,
    pub ui_density: Option<UiDensity>,
    pub code_font_size: Option<u32>,
    pub diff_view: Option<DiffView>,
    pub activity_feed_scope: Option<FeedScope>,
}

impl UserPreferences {
    /// Merge a patch over `self`: `Some` fields replace, `None` fields keep
    /// the current value (PUT /api/v1/users/me partial-update semantics).
    pub fn apply_patch(&mut self, patch: UserPreferencesPatch) {
        if let Some(v) = patch.default_workspace_id {
            self.default_workspace_id = Some(v);
        }
        if let Some(v) = patch.theme {
            self.theme = v;
        }
        if let Some(v) = patch.notification_channels {
            self.notification_channels = v;
        }
        if let Some(v) = patch.ui_density {
            self.ui_density = v;
        }
        if let Some(v) = patch.code_font_size {
            self.code_font_size = v;
        }
        if let Some(v) = patch.diff_view {
            self.diff_view = v;
        }
        if let Some(v) = patch.activity_feed_scope {
            self.activity_feed_scope = v;
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

    /// Create a user with distinct username and display name (SSO provisioning).
    ///
    /// `username` is the immutable, unique, URL-safe handle (derived from the
    /// SSO `preferred_username` claim); `display_name` is the editable
    /// human-readable name (user-management.md §Username vs Display Name).
    pub fn new_sso(
        id: Id,
        external_id: impl Into<String>,
        username: impl Into<String>,
        display_name: impl Into<String>,
        now: u64,
    ) -> Self {
        let mut u = Self::new(id, external_id, display_name, now);
        u.username = username.into();
        u
    }

    /// Validate a username for URL-safety (user-management.md §Username vs
    /// Display Name: unique, URL-safe, immutable after creation).
    ///
    /// A username is URL-safe iff it is 1..=64 chars of ASCII lowercase
    /// letters, digits, `-` or `_`, does not start or end with `-`/`_`, and
    /// contains no consecutive `-`/`_`. Uppercase is rejected: usernames
    /// appear in URLs (`/@{username}`) and mentions (`@{username}`), which
    /// are case-sensitive in path segments.
    pub fn validate_username(username: &str) -> Result<(), String> {
        if username.is_empty() || username.len() > 64 {
            return Err("username must be 1-64 characters".to_string());
        }
        if !username
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
        {
            return Err(
                "username must be URL-safe: lowercase letters, digits, '-' or '_'".to_string(),
            );
        }
        let edge = |c: char| c == '-' || c == '_';
        let first = username.chars().next().unwrap();
        let last = username.chars().last().unwrap();
        if edge(first) || edge(last) {
            return Err("username must not start or end with '-' or '_'".to_string());
        }
        if username.contains("--") || username.contains("__") {
            return Err("username must not contain consecutive '-' or '_'".to_string());
        }
        Ok(())
    }

    /// Normalize an SSO-provided name into a URL-safe username.
    ///
    /// Lowercases, maps unsupported chars to `-`, collapses separators, and
    /// trims separator edges. Returns `None` when nothing URL-safe remains
    /// (caller must fall back to the SSO subject).
    pub fn sanitize_username(raw: &str) -> Option<String> {
        let mut out = String::with_capacity(raw.len());
        for c in raw.chars() {
            if c.is_ascii_lowercase() || c.is_ascii_digit() {
                out.push(c);
            } else if c.is_ascii_uppercase() {
                out.push(c.to_ascii_lowercase());
            } else if c == '-' || c == '_' || c == '.' || c == ' ' || c == '@' {
                out.push('-');
            }
            // All other characters are dropped.
        }
        // Collapse runs of separators and trim edges.
        let mut collapsed = String::with_capacity(out.len());
        let mut prev_sep = false;
        for c in out.chars() {
            let is_sep = c == '-';
            if is_sep && prev_sep {
                continue;
            }
            collapsed.push(c);
            prev_sep = is_sep;
        }
        let trimmed = collapsed.trim_matches('-');
        if trimmed.is_empty() || trimmed.len() > 64 {
            return None;
        }
        Some(trimmed.to_string())
    }

    /// Record a login: stamp `last_login_at` and `updated_at` (spec §User
    /// Entity: last_login_at is set on each authentication).
    pub fn record_login(&mut self, now: u64) {
        self.last_login_at = Some(now);
        self.updated_at = now;
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
        assert_eq!(u.preferences.code_font_size, 14);
        assert_eq!(u.preferences.theme, Theme::System);
        assert_eq!(u.preferences.ui_density, UiDensity::Comfortable);
        assert_eq!(u.preferences.diff_view, DiffView::SideBySide);
        assert_eq!(u.preferences.activity_feed_scope, FeedScope::MyActivity);
        assert!(u.preferences.notification_channels.in_app);
    }

    #[test]
    fn new_sso_splits_username_from_display_name() {
        let u = User::new_sso(Id::new("u1"), "ext-1", "jsell", "Jordan Sell", 1000);
        assert_eq!(u.username, "jsell");
        assert_eq!(u.display_name, "Jordan Sell");
        assert_eq!(u.name(), "Jordan Sell");
    }

    #[test]
    fn username_validation_accepts_url_safe() {
        for ok in ["jsell", "asmith", "a1-b2_c3", "x"] {
            assert!(User::validate_username(ok).is_ok(), "{ok} should be valid");
        }
    }

    #[test]
    fn username_validation_rejects_unsafe() {
        for bad in [
            "",
            "-jsell",
            "jsell-",
            "_jsell",
            "jsell_",
            "js--ell",
            "js__ell",
            "J SELL",
            "jsell!",
            "j/sell",
            "jörg",
            &"x".repeat(65),
        ] {
            assert!(User::validate_username(bad).is_err(), "{bad} should be invalid");
        }
    }

    #[test]
    fn sanitize_username_normalizes() {
        assert_eq!(User::sanitize_username("Jordan Sell").as_deref(), Some("jordan-sell"));
        assert_eq!(User::sanitize_username("Alice_Smith").as_deref(), Some("alice-smith"));
        assert_eq!(User::sanitize_username("bob@example.com").as_deref(), Some("bob-example-com"));
        assert_eq!(User::sanitize_username("  --Carol--  ").as_deref(), Some("carol"));
        assert_eq!(User::sanitize_username("!!!"), None);
        assert_eq!(User::sanitize_username(""), None);
    }

    #[test]
    fn record_login_stamps_timestamps() {
        let mut u = User::new(Id::new("u1"), "ext-1", "alice", 1000);
        assert_eq!(u.last_login_at, None);
        u.record_login(2000);
        assert_eq!(u.last_login_at, Some(2000));
        assert_eq!(u.updated_at, 2000);
    }

    #[test]
    fn preferences_json_roundtrip_includes_new_fields() {
        let mut prefs = UserPreferences::default();
        prefs.ui_density = UiDensity::Compact;
        prefs.code_font_size = 16;
        prefs.diff_view = DiffView::Unified;
        prefs.activity_feed_scope = FeedScope::All;
        let json = serde_json::to_value(&prefs).unwrap();
        assert_eq!(json["ui_density"], "Compact");
        assert_eq!(json["code_font_size"], 16);
        assert_eq!(json["diff_view"], "Unified");
        assert_eq!(json["activity_feed_scope"], "All");
        let back: UserPreferences = serde_json::from_value(json).unwrap();
        assert_eq!(back, prefs);
    }
}
