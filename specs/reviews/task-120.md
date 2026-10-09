# Review — task-120 (Enhance User entity with profile fields and preferences)

Spec: `user-management.md` §User Entity, §Username vs Display Name, §User Preferences.
Commits under review: `bdce1cb` (domain + migration), `d5bd0d7` (ports/adapters/auth/API/mem), `4bcffaa` (partial-update semantics for PUT /users/me). `d69ef40`/`ba2d968` touch only this task file (process commits, not product surface; attribution check passes).
Verdict: **needs-revision**.

## Round 1

Test runs (shared `target/`):

- `cargo test -p gyre-domain --lib user` → **11 passed, 0 failed**.
- `cargo test -p gyre-server --lib api::users` → **14 passed, 0 failed** (mem-backed).
- `cargo test -p gyre-server --lib auth::` login tests (`first_login_derives_url_safe_username_from_preferred`, `second_login_stamps_last_login_and_keeps_username`, `preferred_username_unsanitizable_falls_back_to_subject`, `jwt_auto_creates_user_on_first_login`) → **4 passed, 0 failed**.
- `cargo test -p gyre-adapters --lib` → **84 passed, 264 FAILED** — every test that constructs `SqliteStorage` dies at migration time: `Diesel migration failed: Failed to run 2026-10-08-000056_user_entity_profile_fields with: parser stack overflow`. Includes all 15 `sqlite::user` tests, so none of the SQLite adapter round-trip/immutability/uniqueness tests actually ran.
- `cargo test -p gyre-server --lib api::scim` → **5 passed, 1 FAILED** (`scim_update_user`: 500 vs 200).
- `bash scripts/check-migration-versions.sh` → OK. `check-migration-sql-portability.sh` → OK (checks for dialect-only SQL text, not parse depth — blind to F1). `check-mem-port-contracts.sh` → OK. `check-arch.sh` → OK. `check-abac-route-registry.sh` → OK. `check-task-commit-attribution.sh` → OK. `check-fabricated-scope-defaults.sh` / `check-scope-literal-defaults.sh` / `check-inert-enforcement.sh` / `check-lossy-secret-conversion.sh` → OK.
- Standalone reproduction of the migration against system SQLite 3.50 (python `sqlite3`, minimal pre-000056 `users` table): statement 8 of `up.sql` → `parser stack overflow`; all other 14 statements parse and run; backfill/dedup/defaults produce correct values (`Alice Smith`×2 → `alice-smith`/`alice-smith-2`, `jörg`/`!!!` correctly *flagged* for sanitization by the predicate's semantics).
- Accepted criterion "cargo test --all passes" is currently false.

Verified working (no findings):

- **§User Entity conformance: PASS.** `gyre-domain/src/user.rs` `User` carries every spec field (`username`, `display_name`, `avatar_url`, `timezone`, `locale`, `tenant_id`, `global_role`, `preferences`, `last_login_at`, `created_at`, `updated_at`, plus legacy `roles`/`email`). `UserPreferences` has all 7 spec fields with `Theme`/`UiDensity`/`DiffView`/`FeedScope` enums matching the spec variants exactly. Round-trip and default tests are real (`preferences_json_roundtrip_includes_new_fields`, `default_preferences`).
- **§Username vs Display Name: PASS on the paths that run.** `validate_username` enforces URL-safety (1–64 chars, `[a-z0-9_-]`, no edge/consecutive separators — solid rejection test list incl. `jörg`, 65-char, `js--ell`); `sanitize_username` normalizes (`Jordan.Sell@example.com` → `jordan-sell-example-com`, proven end-to-end by `first_login_derives_url_safe_username_from_preferred`). Auth flow (`auth.rs:806-848`): first login derives the handle from `preferred_username` (sub fallback), stamps `last_login_at`; re-login never renames when the IdP changes `preferred_username` (proven by `second_login_stamps_last_login_and_keeps_username`); the immutability is enforced at the adapter layer in all three implementations (sqlite `user.rs:250-263`, postgres `user.rs:244-257`, mem `mem.rs:1007-1020`) with tests that would fail without the guards. Uniqueness enforced by explicit create-contract checks + `idx_users_username` (global rather than per-tenant — documented in the migration as strictly stronger under the spec's no-cross-tenant-identity model; acceptable, but note the deviation from the task plan's "unique per tenant").
- **§User Preferences: PASS.** Stored server-side as JSON in `users.preferences` (migration backfills the exact `UserPreferences::default()` JSON), returned by `GET /users/me`, updated by `PUT /users/me` with genuine partial-update semantics: `UserPreferencesPatch` + `apply_patch` merge omitted fields over stored values (`put_me_updates_display_name_and_preferences` asserts omitted `notification_channels`/`default_workspace_id` keep stored values — kills the replace-with-defaults bug class), and `put_me_rejects_malformed_preferences` asserts invalid theme → 400 with nothing applied.
- **Port/adapter parity:** `find_by_username` added to the port and implemented in sqlite, postgres, and mem; mem adapter enforces the same duplicate/immutability contracts in code (mem-port-contract check OK). Postgres `user.rs` mirrors the SQLite mapping field-for-field.
- **Mechanical gates** all pass (see runs above).

Findings:

- [open] **F1 (critical): migration 000056 aborts on SQLite — the default deployment cannot start, and 264 adapter tests fail.** `crates/gyre-adapters/migrations/2026-10-08-000056_user_entity_profile_fields/up.sql:59-76` expresses the "username contains a character outside `[a-z0-9_-]`" predicate as a single 39-deep nested `REPLACE(...)` chain. SQLite's parser overflows at that nesting depth — reproduced against both the bundled `libsqlite3-sys 0.28` (Cargo.toml:50, used by `SqliteStorage`) and system SQLite 3.50: `parser stack overflow` on exactly that statement. `SqliteStorage::new` runs `run_pending_migrations(MIGRATIONS)` on every construction (`sqlite/mod.rs:121-126`), so every SQLite-backed server boot and every `SqliteStorage`-constructing test dies. The existing `sqlite::mod` migration tests (`migrations_pending_zero_after_startup`, `migrations_create_tables`) would have caught this — the suite was not run. The predicate's *semantics* are correct (standalone run confirms `jörg`/`!!!` are flagged, `alice-smith` is not); only the expression depth is broken. **Repair:** flatten the chain — e.g. add a scratch column (or temp table), run ~39 sequential single-`REPLACE` UPDATE statements (each depth-1) over it, then apply the final predicate (`scratch <> '' OR length(username) > 64 OR edge separators OR '--'/'__'`) and drop the scratch column. Must remain portable SQL for both backends (`check-migration-sql-portability.sh`; no GLOB/regexp/JSON1). Acceptance: `cargo test -p gyre-adapters --lib` → 0 failures, `cargo test --all` green.
- [open] **F2 (major): username immutability cutover missed the SCIM caller — `PUT /scim/v2/Users/{id}` now 500s — and the SCIM create path can insert non-URL-safe usernames.** `api/scim.rs:319` sets `user.username = req.user_name` and `:326-328` may rewrite `user.external_id`; the new adapter-level immutability guard (`sqlite/user.rs:250-263`, `mem.rs:1007-1020`) rejects the update, so the handler returns 500: `api::scim::tests::scim_update_user` FAILS (500 vs 200) — a regression introduced by this branch (pre-branch, mem `update` was a blind insert). Separately, `scim_create_user` (`api/scim.rs:275`) stores `req.user_name` verbatim with no `validate_username`/`sanitize_username`, so "Bad Handle!" can be persisted as a username — reopening the exact URL-safety hole this task closes everywhere else (the migration sanitizes legacy rows; `POST /api/v1/users` validates; SCIM does not). **Repair:** in `scim_update_user`, treat `userName` as the immutable handle — ignore a changed value or return 409 (spec: immutable after creation), and never rewrite `external_id`; in `scim_create_user`, run the same sanitize-or-reject the local bootstrap path uses. Update `scim_update_user` test to assert the chosen behavior, and add a SCIM create test that a non-URL-safe `userName` is rejected or sanitized.
- [open] **F3 (minor): dedup suffix can collide with a pre-existing distinct handle, aborting the migration at `CREATE UNIQUE INDEX`.** `up.sql:84-93`: two legacy rows both named `jsell` dedup to `jsell` / `jsell-2`; if a *third, distinct* row was already literally named `jsell-2`, the index creation fails and the migration aborts. Rare on real data, but the F1 repair touches this statement's neighborhood — cheap to harden while there (e.g. re-run the dedup pass until no further renames, or suffix with a value guaranteed unused such as `-u<rowid>`).

Non-findings (checked, judged out of scope or acceptable):

- Global `UNIQUE(username)` instead of per-tenant — documented in the migration, strictly stronger, consistent with the spec's cross-tenant isolation stance. Task plan says per-tenant; deviation is defensible and recorded.
- `get_me` fallback for principals with no stored user (`api/users.rs:100-122`) fabricates `username: agent_id` — synthetic response for agent-token callers, nothing persisted; cosmetic, pre-existing shape (was `name: agent_id`).
- Every JWT-authenticated request now performs a `users.update()` write (`validate_jwt` → `find_or_create_user` → `record_login` + update; extractor runs per request). Literal spec reading ("last_login_at is set on each authentication") is satisfied and tested; note the SQLite write-amplification cost — a follow-up could stamp on a coarser cadence if it shows up in practice. Not a revision blocker.
- `check-migration-sql-portability.sh` passing despite F1 — the script audits for dialect-only SQL text, which this migration genuinely avoids; parse depth is a different failure class already covered by the `sqlite::mod` migration tests once they are actually run. No new script needed; run the suite.
- Username backfill degradation for legacy non-ASCII names (`jörg` → `jrg`) — documented in the migration; legacy-data trade-off, new users go through the Rust sanitizer.

Summary: the domain model, port contracts, adapter mappings, auth derivation flow, and the `PUT /users/me` partial-update semantics are real and well-tested — the sections' substance is in place, and the mem-backed tests prove the behaviors. But the shipped migration cannot run on SQLite (F1: default-deployment startup failure masked by mem-backed green tests — exactly the failure class the repo's storage-portability rules exist to prevent), and the immutability cutover broke the SCIM update endpoint while leaving SCIM create able to violate the URL-safe contract (F2). Revision required.

— Reviewer, 2026-10-08

## Round 2 (2026-10-09) — repairs re-verified, verdict PASS

Comparison base `66422bd4`… HEAD `7b922a3` (product tree identical since `0dfba43`; only process/docs/spec commits after). Round-1 product history was rebased into checkpoint commits (`e9a63c7`→`0dfba43`); attribution check passes mechanically, and the post-round-1 repair commits touched only migration + SCIM + `sqlite/mod.rs` (`bbeadf6`: up.sql + scim.rs; `0e15d87`: up.sql + sqlite/mod.rs; `0dfba43`: sqlite/mod.rs), so the round-1 PASS findings on auth/users/domain/mem/postgres apply unchanged to identical bytes.

### F1 (critical — migration parser overflow): REPAIRED

`000056/up.sql:70-120`: the 39-deep nested `REPLACE(...)` predicate is gone; the delete-every-allowed-character pass now runs as 38 sequential depth-1 `UPDATE` statements over a `tmp_username_scratch` column, dropped after. The final predicate (`:110-119`) reads the scratch remainder plus length/edge/consecutive-separator checks, matching the Rust contract. Portable SQL only (REPLACE/substr/length/IN — no GLOB/regexp/JSON1); portability gate OK.

Evidence (persisted, `/tmp/stage/review-evidence/task-120-round2/`):
- `adapters-lib.txt` — `cargo test -p gyre-adapters --lib` at source `034f8fd` (product tree byte-identical to HEAD, verified via `git diff 034f8fd..HEAD -- crates/` → empty): **349 passed, 0 failed** (12 ignored). Pre-repair: 84/264.
- `migration-000056-regression.txt` — `migration_000056_backfills_unique_url_safe_usernames` at `7b922a3`: **ok** (1 passed, exit 0).
- Re-ran the full adapters suite on this tree via the prebuilt binary (built from this tree; verified no `.rs` under `crates/` newer than the binary): **349 passed, 0 failed** — independently confirms the persisted evidence.

The regression test itself is real: builds a pre-000056 DB by running all migrations except 000056, inserts legacy rows (`Alice Smith`×2, `jörg`, `jsell`×2 + pre-existing `jsell-2`, empty name, 65-char name), applies 000056 alone, and asserts exact handles including `jsell-2-u-jsell-3` for the pass-2 collision plus `validate_username` on every result (`sqlite/mod.rs:326-424`). Panics on the original overflow version and on a dedup abort — cannot pass without the repaired migration.

### F2 (major — SCIM cutover): REPAIRED

- `scim_create_user` (`api/scim.rs:268-318`): derives the handle via `User::sanitize_username(&req.user_name)`, falls back to `external_id`, returns 400 when neither yields a usable handle, and 409 (via `find_by_username`) when the handle is taken — precise conflicts instead of raw storage 500s, matching the `api::users::create_user` pattern.
- `scim_update_user` (`api/scim.rs:345-370`): no longer touches `username` or `external_id`; replaces only `displayName`/`emails` and stamps `updated_at`. The documented choice (ignore a changed `userName` rather than 409) is sound — IdPs re-send their original pre-sanitized `userName` on every sync PUT, so 409 would break sync loops; immutability is still enforced.

Test quality checked (`api/scim.rs:512-626`): assertions genuinely pin the behavior — `scim_update_user` PUTs `bob-renamed` + swapped `externalId` and asserts the response (and a follow-up GET) still shows `bob`/null externalId; `scim_create_sanitizes_user_name` asserts `"Bad Handle!"` → `bad-handle` (display name keeps raw); `scim_create_sanitizes_falls_back_to_external_id` asserts the ext-id fallback AND the 400 when no fallback exists; `scim_create_conflict_on_duplicate_username` asserts distinct external ids colliding on the sanitized handle → 409.

Evidence (persisted, `scim-suite.txt`): `api::scim` at `7b922a3` via prebuilt binary from this tree: **9 passed, 0 failed** — including `scim_update_user` (the round-1 regression, previously 500) and the three new sanitize/fallback/409 tests.

### F3 (minor — dedup suffix collision): REPAIRED

`up.sql:149-158`: pass-2 dedup re-ranks the post-pass-1 table and renames any row still sharing a handle to `<handle>-<row id>` — unique by primary key, so `CREATE UNIQUE INDEX idx_users_username` (`:168`) cannot fail on dedup collisions. Covered by the `jsell`/`jsell-2` pre-existing-handle case in the migration test (`u-jsell-3` → `jsell-2-u-jsell-3`). Deterministic tie-break: earliest-created row keeps the shorter handle.

### Round-2 bookkeeping repair (integration rejection): VERIFIED

The rejected integration's only preserved item was `specs/coverage/SUMMARY.md`. Repaired by flipping the bookkeeping to implementation ownership: `user-management.md` rows 2 (User Entity), 3 (Username vs Display Name), 11 (User Preferences) → `implemented` with evidence notes citing the code paths; header counts updated (26/3 → 23/3); SUMMARY regenerated. Verified: `bash scripts/update-coverage-summary.sh` reproduces `SUMMARY.md` **byte-identically**; `grep -c -F '| not-started |' specs/coverage/system/user-management.md` → **0**; all other rows untouched. The business-continuity/HSI count changes in SUMMARY are accumulated sync drift from accepted audit commits — mechanical regeneration output, not hand-edited numbers.

### Fresh verification on this tree (2026-10-09)

- `api::scim` → 9 passed, 0 failed. `api::users` → 14 passed. `auth::` → 39 passed. `health` (SQLite-backed `SqliteStorage::new` construction) → 15 passed — the default-deployment boot path F1 broke is exercised and green.
- `gyre-adapters --lib` → 349 passed, 0 failed (persisted at `034f8fd` + re-run at `7b922a3` via prebuilt binary).
- Mechanical gates re-run: migration versions, SQL portability, mem-port contracts, arch, commit attribution — all OK.
- Round-1 verified-PASS surfaces (domain `user` 11/11, users API 14/14, auth 39/39) re-run green on this tree.

### Verdict

All three findings repaired with real implementations and regression tests that fail on the original failure classes. No new exemptions, no gate weakening, no deleted tests, no unrelated changes — the round-2 diff beyond the F1/F2/F3 product repairs is spec/coverage bookkeeping and this task file. **progress: complete.**

— Reviewer, 2026-10-09
