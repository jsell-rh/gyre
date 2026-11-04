---
title: "Enhance User entity with profile fields and preferences"
spec_ref: "user-management.md §User Entity"
depends_on: [task-216]
progress: ready-for-review
coverage_sections:
  - "user-management.md §User Entity"
  - "user-management.md §Username vs Display Name"
  - "user-management.md §User Preferences"
commits: ["7c52fffda67c3550bf6972774fca12bda819d916", "a3f95bbf83705f915ff76cd89fc2e7aad060f285", "cf32398a7e6284b5c41fe87a41710a34db0350e4", "3ba2ed859cc8139968ce6ee9978fe19dd94f29e5", "9391340519d34cfd5c45a711fc4c1fd98cea99b8", "c76b224d984ddcfe253dcc0b58f0de21093e5238", "7f00472ce2ac51518fae404a0783cd511b679b7e", "7d53b6d2baaaa980439b1b5da704567ac271758b", "7752dd2e6f2a9f8922f8ca0dfde5ed0364bc97fb"]
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

**Verification-repair round (2026-10-10, evidence under
/tmp/stage/review-evidence/task-120-rustfmt-clippy-repair/):** repaired
durable verification finding `09ee56647f38444fa90254ebb9cad8ba` (`checks.sh`
exit 1 on candidate `f745dafe`) — two changed-line gate failures, both
repaired:

- **rustfmt (`auth.rs`):** three violations on task-120 lines — the
  `resolve_unique_username` signature (fits on one line), the
  `while !User::validate_username(...)` condition in `suffixed_username`,
  and a mis-indented `Request::builder()` in the tests. Fixed by applying
  the gate's own formatter invocation (`rustfmt --edition 2021 --config
  skip_children=true`) to the file (commit `7d53b6d2`) — behavior-neutral,
  and the whole-file violation set was exactly those three lines before
  the fix.
- **clippy::redundant_field_names (`sqlite/mod.rs`):** the struct-level
  `#[allow]`s added by checkpoint `c76b224d` on the migration-000056 test's
  `Row`/`Defaults` row carriers never suppressed the lint —
  `QueryableByName`'s derive (diesel_derives 2.3.7) expands to
  `Self { field: field, … }` inside a dummy module that inherits neither
  struct- nor field-level `#[allow]` (the same mechanism the merged
  `sqlite/search.rs` documents and works around). Fixed by wrapping both
  row carriers in a fn-local module carrying a module-level inner
  `#![allow(clippy::redundant_field_names)]` (commit `7752dd2e`) — the
  scoping `search.rs` proved effective on the same diesel version.

Also resolved the staged merge of assignment base `75ada1c4` (merge commit
`2ff4dc5b`): the only conflict, the `SUMMARY.md` TOTAL row, was resolved by
recomputing the total from the merged table rows
(802/219/0/308/112/163/47%; sum-of-rows and the coverage formula verified
programmatically against the 40 per-spec rows).

### Repair-round verification (evidence in the directory above)

- `python3 scripts/check-rustfmt-diff.py 75ada1c4` → "changed lines clean
  (11 Rust files checked)".
- `python3 scripts/check-clippy-diff.py 75ada1c4` → "changed lines clean
  (11 Rust files, 1119 existing warnings outside changes)" — the full
  workspace `cargo clippy --all-targets --all-features` behind it also
  proves both edited files compile.
- `cargo test -p gyre-adapters --lib migration_000056` → **1 passed, 0
  failed** — the restructured row carriers behave identically (backfill
  expectations and column-default assertions unchanged).
- The task-120 product surface is otherwise unchanged this round. The full
  implementation (six product commits, Round-1 F1/F2/F3 repairs, Round-2
  bookkeeping repair, and the 000056 `INTEGER`→`BIGINT` fix `93913405`)
  remains as reviewed in `specs/reviews/task-120.md` Round 2 (PASS).

Transport note (recorded per assignment): this sandbox's listener probe
(`accept`) is not supported (errno 95, `capabilities.json`) — server-boot
smoke over HTTP cannot run here. Host-side checks: `cargo test --all` +
GitHub CI on this exact head.
