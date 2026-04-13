---
title: "Enhance User entity with profile fields and preferences"
spec_ref: "user-management.md §User Entity"
depends_on: []
progress: not-started
coverage_sections:
  - "user-management.md §User Entity"
  - "user-management.md §Username vs Display Name"
  - "user-management.md §User Preferences"
commits: []
---

## Spec Excerpt

From `user-management.md` §User Entity:

```rust
pub struct User {
    pub id: Id,
    pub external_id: String,        // Keycloak subject (JWT sub claim)
    pub username: String,            // Unique, URL-safe, immutable after creation
    pub display_name: String,        // Human-readable, editable
    pub email: String,               // Derived from SSO, not user-editable
    pub avatar_url: Option<String>,  // From SSO or uploaded
    pub timezone: String,            // IANA timezone (e.g., "America/New_York")
    pub locale: String,              // i18n locale (e.g., "en-US")
    pub tenant_id: Id,
    pub global_role: GlobalRole,
    pub preferences: UserPreferences,
    pub last_login_at: Option<u64>,
    pub created_at: u64,
    pub updated_at: u64,
}
```

From §Username vs Display Name:
- **Username:** unique, URL-safe, immutable after creation. Derived from SSO `preferred_username` on first login.
- **Display Name:** human-readable, editable. Used in UI, activity feeds.
- **Email:** derived from SSO. Not user-editable.

From §User Preferences:
```rust
pub struct UserPreferences {
    pub theme: Theme,                       // Light, Dark, System
    pub default_workspace_id: Option<Id>,
    pub notification_channels: NotificationChannels,
    pub ui_density: UiDensity,              // Compact, Comfortable, Spacious
    pub code_font_size: u32,
    pub diff_view: DiffView,               // SideBySide, Unified
    pub activity_feed_scope: FeedScope,     // MyActivity, Workspace, All
}
```

Preferences stored server-side (not localStorage). Persist across devices and sessions.

## Implementation Plan

1. **Audit existing User model** in `gyre-domain/src/user.rs`:
   - Check which fields from the spec already exist
   - Add missing fields: `username`, `display_name`, `avatar_url`, `timezone`, `locale`, `preferences`, `last_login_at`, `updated_at`
   - Existing fields like `external_id`, `email`, `tenant_id`, `global_role` likely already present

2. **Add UserPreferences domain type:**
   - `Theme` enum (Light, Dark, System)
   - `UiDensity` enum (Compact, Comfortable, Spacious)
   - `DiffView` enum (SideBySide, Unified)
   - `FeedScope` enum (MyActivity, Workspace, All)
   - Serialize as JSON for storage

3. **Migration** (check current number — currently at 000049):
   - Add columns to `users` table: `username`, `display_name`, `avatar_url`, `timezone`, `locale`, `preferences`, `last_login_at`, `updated_at`
   - Add unique index on `username` within tenant
   - Default `timezone` to "UTC", `locale` to "en-US"

4. **Update UserRepository port** in `gyre-ports`:
   - Add `find_by_username` method
   - Update existing methods to handle new fields

5. **Update SQLite adapter** to map new fields

6. **Update auth flow** in `gyre-server/src/auth.rs`:
   - On first login, derive `username` from SSO `preferred_username` claim
   - Set `last_login_at` on each authentication

7. **Update `PUT /api/v1/users/me`** to accept preferences update

## Acceptance Criteria

- [ ] User entity has all spec'd fields (username, display_name, avatar_url, timezone, locale, preferences, last_login_at, updated_at)
- [ ] UserPreferences struct with Theme, UiDensity, DiffView, FeedScope enums
- [ ] Username is unique per tenant, URL-safe, immutable after creation
- [ ] Username derived from SSO preferred_username on first login
- [ ] Preferences stored server-side as JSON, returned via `GET /api/v1/users/me`
- [ ] `PUT /api/v1/users/me` accepts display_name, timezone, locale, and preferences updates
- [ ] Migration adds columns with sensible defaults
- [ ] `cargo test --all` passes

## Agent Instructions

Read `specs/system/user-management.md` §User Entity through §User Preferences for full requirements. Existing User model: `gyre-domain/src/user.rs`. User port: `gyre-ports/src/user.rs` (or grep for `UserRepository`). SQLite adapter: grep for `impl UserRepository` in `gyre-adapters/`. Auth flow: `gyre-server/src/auth.rs`. User API: `gyre-server/src/api/users.rs`. Profile adapter: `gyre-adapters/src/sqlite/user_profile.rs`. Check migration numbering: `ls crates/gyre-adapters/migrations/ | tail -5` — currently at 000049.
