---
title: "Enhance User entity with profile fields and preferences"
spec_ref: "user-management.md §User Entity"
depends_on: [task-216]
progress: ready-for-review
coverage_sections:
  - "user-management.md §User Entity"
  - "user-management.md §Username vs Display Name"
  - "user-management.md §User Preferences"
commits: ["50c487140b20a7b7346ff30d4463648e31283107", "7752dd2e6f2a9f8922f8ca0dfde5ed0364bc97fb", "7d53b6d2baaaa980439b1b5da704567ac271758b", "7c52fffda67c3550bf6972774fca12bda819d916", "a3f95bbf83705f915ff76cd89fc2e7aad060f285", "cf32398a7e6284b5c41fe87a41710a34db0350e4", "89febe77672670ffec5c16dd66af7d584c3f011e", "b10b4a367094f5752d70511629b3df556936fda7", "3ba2ed859cc8139968ce6ee9978fe19dd94f29e5", "9391340519d34cfd5c45a711fc4c1fd98cea99b8", "c76b224d984ddcfe253dcc0b58f0de21093e5238", "7f00472ce2ac51518fae404a0783cd511b679b7e"]
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
`GET`/`PUT /api/v1/users/me` — is complete: six product commits plus the
Round-1/2 repairs (rustfmt `7d53b6d2`, clippy derive-scoping `7752dd2e`,
INTEGER→BIGINT `93913405`), the rebase repair (`c76b224d`/`7f00472c`),
and the first version-collision renumber `000056 → 000057` (`50c48714`),
all as reviewed in `specs/reviews/task-120.md` Round 2 (PASS).

**This round — merge repair (`75d49d4c8`):** merging main (`124a852be`)
raised two source issues, both fixed:

1. **`sqlite/mod.rs` content conflict** — the conflicting hunk held two
   independent regression tests (task-120's username-backfill guard and
   main's task-102 `migration_000054_folds_legacy_path_pid_into_detail`).
   Both are kept; neither semantics changed.
2. **Second migration version collision** — main landed task-193's
   `2026-10-08-000057_spec_approval_attestation` under Diesel version
   `20261008000057`, colliding with the previously renumbered
   `..._user_entity_profile_fields` (Diesel runs only one migration per
   version, alphabetically first — ours would again be silently
   dropped). Renumbered to the next unused sequence:
   `2026-10-08-000058_user_entity_profile_fields` (SQL unchanged).
   Updated the regression test's partition key to `20261008000058`
   (renamed to `migration_000058_backfills_unique_url_safe_usernames`;
   its pre-migration set now correctly includes main's 000056 and 000057)
   and the 000057→000058 comments in `sqlite/mod.rs`, `schema.rs`, and
   `auth.rs`.

### Verification

- `bash scripts/check-migration-versions.sh` → OK (no duplicate versions).
- `bash scripts/check-migration-sql-portability.sh` → OK.
- `cargo test -p gyre-adapters --lib` → **370 passed, 0 failed** (12
  ignored), including the renumbered
  `migration_000058_backfills_unique_url_safe_usernames`, main's
  `migration_000054_folds_legacy_path_pid_into_detail`,
  `no_duplicate_migration_version_prefixes`,
  `migrations_create_previously_shadowed_columns`, and
  `migration_is_idempotent`.
- `cargo test -p gyre-server --lib` → **1387 passed, 0 failed** (the
  merge auto-merged many server files; this confirms they compile and
  pass together with the user-entity surface).
- `cargo check --workspace --all-targets` → clean.
- `python3 scripts/check-clippy-diff.py HEAD^1` → changed lines clean;
  `python3 scripts/check-rustfmt-diff.py HEAD^1` → clean.
- `bash scripts/check-arch.sh`, `check-abac-route-registry.sh`,
  `check-abac-exempt-handlers.sh`, `check-mcp-write-tools.sh`,
  `check-dead-message-kinds.sh`, `check-byte-slice-truncation.sh`,
  `check-relative-path-defaults.sh`, `check-fail-open-ref-resolution.sh`,
  `check-mem-port-contracts.sh`, `check-fabricated-scope-defaults.sh`,
  `check-lossy-secret-conversion.sh`, `check-scope-literal-defaults.sh`,
  `check-inert-enforcement.sh`, `check-forwarded-header-trust.sh`,
  `check-in-memory-state-stores.sh`, `check-unbounded-external-http.sh`,
  `check-non-atomic-creation.sh`, `check-forged-scope-fields.sh`, and
  `check-task-commit-attribution.sh` → all OK on the merged tree.

Transport note (recorded per assignment): the prior round's listener
probe was unsupported; this sandbox's `capabilities.json` now reports
`tcp_listener_probe: supported`. No server-boot HTTP smoke was needed
for this round's repair — the changes are storage-layer only and covered
by the adapter test suite above. Host-side checks: `cargo test --all` +
GitHub CI on this exact head.
