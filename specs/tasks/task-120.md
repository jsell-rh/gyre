---
title: "Enhance User entity with profile fields and preferences"
spec_ref: "user-management.md §User Entity"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "user-management.md §User Entity"
  - "user-management.md §Username vs Display Name"
  - "user-management.md §User Preferences"
commits: ["0dfba43fb64499f158b96276a982c51d3f6ce71b", "0e15d87d862a91bbd9585300cdb844cc7b5c7f11", "bbeadf6a3f06728801a0f98cf62da56971bf33d0", "04fde7df77b884b20e4108dff917a5324174f034", "0170f283e0289a10468b4683efa64f7b266ad83b", "e9a63c7d50520dcedbeb832dab9e213c74e91340"]
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

This is a resume of retained source `c0dd943c` (attempt-12 branch): the full
task-120 implementation — all six product commits, the Round-1 F1/F2/F3 repairs,
and the Round-2 integration-rejection bookkeeping repair — is preserved on this
tree byte-identically (merge `be1223b9` with the newer base `4b9d61c4` touched
no task-120 product surface; verified by empty `git diff c0dd943 HEAD --
crates/gyre-domain/src/user.rs crates/gyre-adapters/ ...` over all six commit
paths). This round's work: re-verification on the merged tree plus truthful
progress/bookkeeping restoration for fresh independent review.

- `User` entity (`gyre-domain/src/user.rs`) carries every spec profile field
  (username, display_name, avatar_url, timezone, locale, preferences,
  last_login_at, updated_at) with `UserPreferences` (Theme/UiDensity/DiffView/
  FeedScope) stored server-side as JSON; migration 000056 adds the columns,
  backfills defaults (UTC/en-US), and sanitizes legacy rows to unique URL-safe
  handles via 38 depth-1 portable-SQL UPDATEs (no parser overflow on SQLite).
- Username contract enforced across the chain: unique (index + create checks),
  URL-safe (`validate_username`/`sanitize_username`), immutable after creation
  (adapter guards in sqlite/postgres/mem; SCIM update ignores rename attempts;
  SCIM create sanitizes with external-id fallback and 400/409 on unusable/
  duplicate handles). First login derives the handle from SSO
  `preferred_username` (sub fallback) and stamps `last_login_at`.
- `PUT /api/v1/users/me` implements genuine partial-update semantics
  (`UserPreferencesPatch` merge — omitted fields keep stored values), returned
  via `GET /users/me`; malformed preferences → 400 with nothing applied.
- Port `find_by_username` implemented in sqlite/postgres/mem with parity
  contract guards.

### Resume verification (2026-10-09, merged tree `be1223b9`)

Product surface identity: `git diff c0dd943..be1223b9 -- crates/gyre-domain/src/user.rs
crates/gyre-adapters/src/sqlite/ crates/gyre-adapters/src/postgres/user.rs
crates/gyre-adapters/migrations/2026-10-08-000056_user_entity_profile_fields/
crates/gyre-server/src/api/users.rs crates/gyre-server/src/api/scim.rs
crates/gyre-server/src/auth.rs` → empty (byte-identical). Focused probes
re-run on the merged tree, evidence persisted under
`/tmp/stage/review-evidence/task-120-resume/`:

- `cargo test -p gyre-adapters --lib` → 349 passed, 0 failed (includes the
  SQLite-boot path the F1 parser overflow broke and
  `migration_000056_backfills_unique_url_safe_usernames`).
- `cargo test -p gyre-server --lib api::scim` → 9 passed; `api::users` → 14
  passed; `auth::` → 39 passed (username derivation, last_login_at stamping,
  SCIM cutover, partial-update preferences).
- `cargo test -p gyre-domain --lib user` → 11 passed.
- Mechanical gates re-run on the merged tree: migration versions, SQL
  portability, mem-port contracts, arch, ABAC route registry, commit
  attribution (with the six product commits restored in frontmatter), scope
  guards — all OK.

Transport note (recorded per assignment): this sandbox's listener probe
(`accept`) is not supported (errno 95) — server-boot smoke over HTTP cannot run
here; the SQLite migration-boot path is instead proven by every
`SqliteStorage::new`-constructing adapter test (349/349). Host-side checks:
`cargo test --all` + GitHub CI on this exact head.
