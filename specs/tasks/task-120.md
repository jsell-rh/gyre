---
title: "Enhance User entity with profile fields and preferences"
spec_ref: "user-management.md §User Entity"
depends_on: [task-216]
progress: ready-for-review
coverage_sections:
  - "user-management.md §User Entity"
  - "user-management.md §Username vs Display Name"
  - "user-management.md §User Preferences"
commits: ["7752dd2e6f2a9f8922f8ca0dfde5ed0364bc97fb", "7d53b6d2baaaa980439b1b5da704567ac271758b", "7c52fffda67c3550bf6972774fca12bda819d916", "a3f95bbf83705f915ff76cd89fc2e7aad060f285", "cf32398a7e6284b5c41fe87a41710a34db0350e4", "3ba2ed859cc8139968ce6ee9978fe19dd94f29e5", "9391340519d34cfd5c45a711fc4c1fd98cea99b8", "c76b224d984ddcfe253dcc0b58f0de21093e5238", "7f00472ce2ac51518fae404a0783cd511b679b7e"]
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
(username/display_name/avatar_url/timezone/locale/preferences/last_login_at/
updated_at), `UserPreferences` with Theme/UiDensity/DiffView/FeedScope,
per-tenant unique immutable URL-safe username derived from SSO
`preferred_username` on first login, server-side JSON preferences via
`GET`/`PUT /api/v1/users/me`, migration 000056 — is complete and unchanged
this round: six product commits plus the Round-1/2 repairs, the rustfmt fix
`7d53b6d2`, the clippy derive-scoping fix `7752dd2e`, and the 000056
`INTEGER` to `BIGINT` fix `93913405`, as reviewed in
`specs/reviews/task-120.md` Round 2 (PASS).

**This round — rebase repair (finding `8884e159`, evidence under
/tmp/stage/review-evidence/task-120-rebase-8884e159/):** resolved the
staged merge of assignment base `3c39ab1f` (merge commit `ab7a779b`). The
only conflict was the `specs/coverage/SUMMARY.md` TOTAL row: each parent's
TOTAL was consistent with its own table but wrong for the merged table
(main landed task-172, moving ui-layout 38 assigned/0 implemented to 37/1;
this branch landed task-120, moving user-management 26/0 to 23/3).
Resolved by regenerating `SUMMARY.md` with the repo's canonical generator
`scripts/update-coverage-summary.sh` from the merged per-spec files —
TOTAL 802/219/0/307/113/163/47%.

### Verification

- All 40 merged per-spec rows match their coverage files (programmatic
  status-count comparison); the recomputed TOTAL equals both the row sums
  and the generator's own formula (276/583 to 47%).
- Generator idempotency: re-running `update-coverage-summary.sh` on
  `ab7a779b` produces a zero diff.
- Merge integrity: `git diff 3c39ab1f ab7a779b` over all merge-staged
  paths is empty except SUMMARY.md's 3 expected lines; no `crates/` files
  were touched, so the task-120 product surface is bit-identical to the
  reviewed `4bd6c4c1`.
- `bash scripts/check-task-commit-attribution.sh` returns OK (merge
  commits are out of its scope by design).

Transport note (recorded per assignment): this sandbox's listener probe
(`accept`) is not supported (errno 95, `capabilities.json`) — server-boot
smoke over HTTP cannot run here. Host-side checks: `cargo test --all` +
GitHub CI on this exact head.
