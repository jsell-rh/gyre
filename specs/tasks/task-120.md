---
title: "Enhance User entity with profile fields and preferences"
spec_ref: "user-management.md §User Entity"
depends_on: []
progress: needs-revision
coverage_sections:
  - "user-management.md §User Entity"
  - "user-management.md §Username vs Display Name"
  - "user-management.md §User Preferences"
commits: ["374ad4608489ef269dbac19c2c5733ea03f87699", "a47efb87ded7fe2dbf16c368d277341a7167bb46", "a1dcb09d809ba8277f791f3aadf142724918f626", "7e26a8bbc8f50c10db42c7145c3a0857f05d723c", "48b890530068612984f076d4ad35371170fd26e0", "40b7f7c4905ec9bd9f96ad74680e7e7c119fcc62"]
review: specs/reviews/task-120.md
---

## Revision Round 1 (2026-10-09)

All three review findings repaired:

- **F1 (critical, migration parser overflow):** the 39-deep nested
  `REPLACE(...)` predicate in `000056/up.sql` is replaced by 38 sequential
  depth-1 `UPDATE` statements over a `tmp_username_scratch` column (dropped
  after); the sanitize predicate reads the scratch remainder. Portable SQL
  on both backends (no GLOB/regexp/JSON1). Regression test
  `migration_000056_backfills_unique_url_safe_usernames` builds a
  pre-000056 DB and applies 000056 alone, asserting sanitize/dedup/pass-2
  collision semantics.
- **F2 (SCIM cutover):** `scim_create_user` derives the handle via
  `User::sanitize_username` with external-id fallback and 400 when neither
  yields a usable handle; duplicate → precise 409. `scim_update_user`
  leaves `username`/`external_id` untouched (immutable after creation) and
  replaces only `displayName`/`emails`. Four focused SCIM tests.
- **F3 (dedup suffix collision):** pass-2 dedup renames any row still
  sharing a handle to `<handle>-<row id>` so `CREATE UNIQUE INDEX` cannot
  fail; covered by the `jsell`/`jsell-2` pre-existing-handle case in the
  migration test.

Focused verification on this tree (2026-10-09): `gyre-adapters --lib` →
**349 passed, 0 failed** (pre-repair: 84/264 — every SqliteStorage test
died at migration time); `gyre-server --lib api::scim` → 9 passed
(incl. `scim_update_user`, previously 500); `api::users` → 14 passed;
`auth::` → 39 passed; `gyre-domain --lib user` → 11 passed. Mechanical
gates OK: migration versions, SQL portability, mem-port contracts, arch,
ABAC route registry, commit attribution, fabricated/scope-literal
defaults, inert enforcement, lossy secret conversion, forged scope
fields, unbounded external HTTP.

Test-harness note: the migration regression test initially failed with
`no such table: __diesel_schema_migrations` — raw
`MigrationHarness::run_migrations` (unlike `run_pending_migrations`) does
not set up the bookkeeping table; fixed by calling `conn.setup()` first.

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

## Review

### Review changed source code

- specs/coverage/SUMMARY.md

Preserved these edits for implementation. Review cannot approve its own source or verifier edits. Repair them within task scope and request a fresh independent review.
