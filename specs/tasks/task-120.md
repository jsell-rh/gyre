---
title: "Enhance User entity with profile fields and preferences"
spec_ref: "user-management.md §User Entity"
depends_on: [task-216]
progress: ready-for-review
coverage_sections:
  - "user-management.md §User Entity"
  - "user-management.md §Username vs Display Name"
  - "user-management.md §User Preferences"
commits: ["3ba2ed859cc8139968ce6ee9978fe19dd94f29e5", "9391340519d34cfd5c45a711fc4c1fd98cea99b8", "c76b224d984ddcfe253dcc0b58f0de21093e5238", "7f00472ce2ac51518fae404a0783cd511b679b7e", "cf32398a7e6284b5c41fe87a41710a34db0350e4"]
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

**Contract-restore round (2026-10-10, evidence under
/tmp/stage/review-evidence/task-120-contract-restore/):** repaired durable
finding `bb62e23198f543909f6016d9d26ba3b9` (rejected candidate `da2671df`).
The finding's "changed the assigned requirements" is confined to this task
file: the prior branch tip's process commits had accumulated pipeline
narrative rounds atop `## Shipped` and a rewritten commits list, drifting the
file from its original assignment contract. This round restores the original
contract text (spec excerpt, implementation plan, acceptance criteria,
agent instructions — all verbatim from the assignment) and replaces the
narrative with this summary of the actual shipped behavior. No spec document
or coverage matrix row was modified by the restore; the product code is
unchanged this round.

**Product surface (unchanged since the reviewed repairs):** the task-120
implementation is the six product commits plus the review repairs, reviewed
PASS in `specs/reviews/task-120.md` Round 2, and the two later durable-finding
repairs — `93913405` (finding `15814362...`: migration 000056
`users.last_login_at` INTEGER → BIGINT for PostgreSQL) and `cf32398a`
(finding `f6b29ff8...`: username-collision suffix walk in
`find_or_create_user`, restoring the `base_username` binding the interrupted
checkpoint had dropped). Relative to `93913405` the only changed task-120
product file is `crates/gyre-server/src/auth.rs` (+265/−3, the collision
walk: `resolve_unique_username` probing base, base-2, base-3, …;
`suffixed_username` truncation/stem repair; `u-<id>` escape hatch on suffix
exhaustion; TOCTOU note that a racing create self-heals on next login).
BIGINT fix confirmed present (`up.sql:23`). `web/` is byte-identical to
upstream content (`git diff 05709c24..HEAD -- web/` → empty; zero tracked
`web/dist/` files changed since `93913405`) — no checkpoint-build churn on
this tree.

**Verification (2026-10-10, tree `2acc0dc8`, evidence under
/tmp/stage/review-evidence/task-120-contract-restore/):** domain user
**11/11**; full auth suite **42 passed, 0 failed** (39 reviewed baseline +
3 collision tests, including both lockout scenarios `jordan-sell-2` and
`-3`-when-`-2`-squatted); users API **14/14**; SCIM **9/9**; adapters
**349/349** (12 ignored, reviewed baseline — includes
`migration_000056_backfills_unique_url_safe_usernames` and every
`SqliteStorage::new` boot path). Gates: migration versions, SQL portability,
arch, mem-port contracts — all OK. Attribution gate initially failed on the
base merge making `27bd585c` (upstream task-155, which lands its own task
file without listing itself — same pre-existing drift class as task-210
`a781ede2` / task-200 `6bf777a6`, repaired in `4dd13430` / `1616755f`)
reachable; repaired this round by recording the SHA in
`specs/tasks/task-155.md` frontmatter.

Transport note: this sandbox's listener probe (`accept`) is not supported
(errno 95, `capabilities.json`) — server-boot smoke over HTTP cannot run
here; the SQLite migration-boot path is proven by every
`SqliteStorage::new`-constructing adapter test. Host-side checks:
`cargo test --all` + GitHub CI on the published head; live-PG round-trip
(no PG server in this sandbox).
