---
title: "Enhance User entity with profile fields and preferences"
spec_ref: "user-management.md §User Entity"
depends_on: [task-216]
progress: ready-for-review
coverage_sections:
  - "user-management.md §User Entity"
  - "user-management.md §Username vs Display Name"
  - "user-management.md §User Preferences"
commits: ["c76b224d984ddcfe253dcc0b58f0de21093e5238", "7f00472ce2ac51518fae404a0783cd511b679b7e"]
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

Merge round (assignment base `6bf777a6` merged as `c817a5f6`; last code-touching
commit `bad88ded`):
the task-120 product surface is unchanged by the merge — `git diff
0ceae938..HEAD -- crates/` touches only the merged task-200 files
(`gyre-common/src/message.rs`, `api/messages.rs`, `mcp.rs`), all ten task-120
paths byte-identical — and the full implementation (six product commits,
Round-1 F1/F2/F3 repairs, Round-2 bookkeeping repair) remains as reviewed in
`specs/reviews/task-120.md` Round 2 (PASS). This round's work: fresh
post-merge evidence, two bookkeeping repairs, and removal of stray probe
artifacts. Earlier rounds' details below.

### Merge-round verification (2026-10-10, evidence under
/tmp/stage/review-evidence/task-120-merge-c817a5f6/)

- Product surface identity: `git diff 0ceae938..HEAD -- crates/` → only the
  three merged task-200 files; all ten task-120 paths byte-identical to the
  reviewed Round-2 tree.
- `cargo test -p gyre-domain --lib user` → **11 passed, 0 failed**.
- `cargo test -p gyre-server --lib auth::` → **39 passed, 0 failed**
  (`auth-suite.txt`).
- `cargo test -p gyre-server --lib api::users` → **14 passed**;
  `api::scim` → **9 passed** (`users-api-suite.txt`, `scim-suite.txt`).
- `cargo test -p gyre-adapters --lib` → **349 passed, 0 failed** (12 ignored;
  includes `migration_000056_backfills_unique_url_safe_usernames` and every
  `SqliteStorage::new` boot path) — `adapters-lib.txt`.
- Mechanical gates: migration versions, SQL portability, mem-port contracts,
  arch — OK.
- Bookkeeping repairs this round:
  - `SUMMARY.md` merge conflict resolved by recomputing TOTAL from the merged
  table rows (802/219/0/308/112/163/47%, row formula verified against
  per-spec rows); merge committed as `c817a5f6`.
  - task-200 publication commit `6bf777a6` recorded in
  `specs/tasks/task-200.md` frontmatter (commit `1616755f`) — the merge made
  it reachable here while its SHA was missing there, the same drift class
  repaired for task-210 in `4dd13430`. Attribution gate now OK.
  - Stray `.rlib`/`.rmeta` probe artifacts tracked by the `c76b224d`
  checkpoint commit removed (commit `bad88ded`) — dead build outputs, no
  source changes.
- `web/dist` churn from local test-builds (no `SKIP_WEB_BUILD`) was discarded;
  committed dist untouched (`git diff 0ceae938..HEAD -- web/` → empty).

Checkpoint round (recovered assignment, tree `7f00472c` → this head): the full
task-120 implementation — all six product commits, the Round-1 F1/F2/F3 repairs,
and the Round-2 integration-rejection bookkeeping repair — was preserved by the
interrupted-assignment checkpoint and is byte-identical on this tree (verified:
`git diff 0dfba43..HEAD -- <all task-120 crate paths>` → empty; `0dfba43` is the
newest of the six frontmatter product commits, the others are its ancestors via
`0170f28`). This round's work: re-verification of the checkpointed product
surface with fresh evidence, plus one pre-existing attribution-drift repair.

### Resume verification (2026-10-09, checkpoint tree, evidence under
`/tmp/stage/review-evidence/task-120-checkpoint/`)

- Product surface identity: `git diff 0dfba43..HEAD -- crates/gyre-domain/src/user.rs
  crates/gyre-ports/src/user.rs crates/gyre-adapters/src/sqlite/ crates/gyre-adapters/src/postgres/user.rs
  crates/gyre-adapters/src/schema.rs crates/gyre-adapters/src/migrations/
  crates/gyre-server/src/api/users.rs crates/gyre-server/src/api/scim.rs
  crates/gyre-server/src/auth.rs crates/gyre-server/src/mem.rs` → empty.
- `cargo test -p gyre-adapters --lib` → **349 passed, 0 failed** (12 ignored;
  includes the SQLite-boot path the F1 parser overflow broke and
  `migration_000056_backfills_unique_url_safe_usernames`) —
  `adapters-lib.txt`.
- `cargo test -p gyre-server --lib api::scim` → **9 passed**; `api::users` →
  **14 passed** — `scim-users.txt`.
- `cargo test -p gyre-server --lib auth::` → **39 passed** (username
  derivation, last_login_at stamping, SCIM cutover, partial-update
  preferences).
- `cargo test -p gyre-domain --lib user` → **11 passed**.
- Mechanical gates re-run on this tree: migration versions, SQL portability,
  mem-port contracts, arch, ABAC route registry, ABAC exempt handlers,
  forwarded-header trust, in-memory-state stores, unbounded external HTTP,
  fail-open ref resolution, relative path defaults, byte-slice truncation,
  MCP write tools, dead message kinds, fabricated/scope-literal defaults,
  inert enforcement, lossy secret conversion, forged scope fields — all OK.
  Commit attribution initially failed on **pre-existing drift from upstream
  main** (task-210 commit `a781ede2` absent from `specs/tasks/task-210.md`
  frontmatter though it is an ancestor of the assignment base `8c2d1775`);
  repaired on this branch by recording the SHA in the frontmatter
  (commit `4dd13430`), matching the same fix already shipped on sibling
  pipeline branches (`63d66b46`). Gate now OK.

Transport note (recorded per assignment): this sandbox's listener probe
(`accept`) is not supported (errno 95, `capabilities.json`) — server-boot smoke
over HTTP cannot run here; the SQLite migration-boot path is instead proven by
every `SqliteStorage::new`-constructing adapter test (349/349). Host-side
checks: `cargo test --all` + GitHub CI on this exact head.
