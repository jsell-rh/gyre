---
title: "Enhance User entity with profile fields and preferences"
spec_ref: "user-management.md §User Entity"
depends_on: [task-216]
progress: complete
coverage_sections:
  - "user-management.md §User Entity"
  - "user-management.md §Username vs Display Name"
  - "user-management.md §User Preferences"
commits: ["50c487140b20a7b7346ff30d4463648e31283107", "7752dd2e6f2a9f8922f8ca0dfde5ed0364bc97fb", "7d53b6d2baaaa980439b1b5da704567ac271758b", "7c52fffda67c3550bf6972774fca12bda819d916", "a3f95bbf83705f915ff76cd89fc2e7aad060f285", "cf32398a7e6284b5c41fe87a41710a34db0350e4", "89febe77672670ffec5c16dd66af7d584c3f011e", "b10b4a367094f5752d70511629b3df556936fda7", "3ba2ed859cc8139968ce6ee9978fe19dd94f29e5", "9391340519d34cfd5c45a711fc4c1fd98cea99b8", "c76b224d984ddcfe253dcc0b58f0de21093e5238"]
review: specs/reviews/task-120.md
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

## Shipped

The task-120 product surface — User entity profile fields
(username/display_name/avatar_url/timezone/locale/preferences/
last_login_at/updated_at), `UserPreferences` with
Theme/UiDensity/DiffView/FeedScope, per-tenant unique immutable URL-safe
username derived from SSO `preferred_username` on first login,
server-side JSON preferences via `GET`/`PUT /api/v1/users/me` — is
complete on the current base, as reviewed in `specs/reviews/task-120.md`
Round 2 (PASS): the twelve product commits in `commits:` (six feature
commits plus the rustfmt/clippy/INTEGER→BIGINT/rebase/renumber repairs)
and the merge repair `75d49d4c8` are all ancestors of this branch head;
main integrated the task-120 candidate and then moved forward.

**This round — requirements-changed repair (base `cd752300c`):** the
assignment base advanced past the previous merge-repair round (task-208
amended `user-management.md` §"My Stuff" Views and removed
`GET /api/v1/users/me/{agents,tasks,mrs}` end-to-end; migrations 000056
through `2026-11-08-000058` landed). I verified the current task-120
contract is genuinely satisfied by the retained source on this base —
no re-implementation was needed because the retained product surface is
fully contained in the base tree:

- Spec sections §User Entity / §Username vs Display Name / §User
  Preferences are unchanged by the task-208 amendment (verified by
  reading the current spec text against the task excerpt).
- `gyre-domain/src/user.rs` carries every spec field and the exact
  `UserPreferences` shape (Theme/default_workspace_id/
  notification_channels/ui_density/code_font_size/diff_view/
  activity_feed_scope) plus the `UserPreferencesPatch` partial-update
  merge.
- Migration `2026-10-08-000058_user_entity_profile_fields` is the
  unique holder of Diesel version `20261008000058` on the current tree
  (no second collision with main's later migrations).
- `find_or_create_user` (auth.rs) derives the username from SSO
  `preferred_username`, stamps `last_login_at`, never renames;
  `find_by_username` port + SQLite adapter with
  `username_immutable_on_update` rejection test are present.

Concrete fixes this round (docs accuracy only — no product source
changed):

1. `specs/coverage/system/user-management.md` row 2 cited the
   pre-second-renumber migration (000057) and the old regression-test
   name; updated to 000058, the two-step renumber story (000056→000057
   after task-180, →000058 after task-193), and
   `migration_000058_backfills_unique_url_safe_usernames`, and replaced
   the stale "Pending independent review" with the Round-2 PASS +
   landed-on-main status. Row 3's stale `auth.rs:806-848` line
   reference replaced with the `find_or_create_user` function
   reference. Row 11's "Pending independent review" likewise updated.

### Verification

Focused probes on this exact head (evidence retained under
`/tmp/stage/review-evidence`):

- `bash scripts/check-migration-versions.sh` → OK (no duplicate Diesel
  versions on the merged migration set through 2026-11-08).
- `bash scripts/check-migration-sql-portability.sh` → OK.
- `cargo test -p gyre-adapters --lib
  migration_000058_backfills_unique_url_safe_usernames` → 1 passed.
- `cargo test -p gyre-adapters --lib user` → 21 passed, 0 failed
  (includes `username_immutable_on_update`, `find_by_username`).
- `cargo test -p gyre-server --lib api::users` → 18 passed, 0 failed
  (includes `put_me_updates_display_name_and_preferences`,
  `put_me_rejects_malformed_preferences`,
  `my_stuff_endpoints_are_removed`).
- `cargo test -p gyre-server --lib auth` → 71 passed, 0 failed
  (find_or_create_user / username-derivation coverage).
- `bash scripts/check-task-commit-attribution.sh` → OK.

Host-side checks owned by verification: `cargo test --all`,
architecture/Clippy gates, GitHub CI on this exact head.
