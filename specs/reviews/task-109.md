# Review — task-109 (Externalize spec lifecycle configuration)

Spec: `specs/system/spec-lifecycle.md` §Configuration.
Assignment base `b5a872087`, candidate `ae52a6be` (merge head resolving the
base into the task branch; the one source conflict — the git_http.rs dedup
block — resolved as a union of the config gate and the base's task-104
`SpecChanged` audit emission, with a dedicated test on the exact hunk).
Verdict: **approved** — no findings. Independent evidence below.

## Probes run (candidate `ae52a6be` checked out; working tree clean before and after)

All Rust probes via `/tmp/stage/bin/cargo`; frontend via `npm ci`-seeded
`web/node_modules` (build.rs ran `npm ci` during the server build) and
`/tmp/stage/bin/npx vitest`.

- `cargo test -p gyre-domain --lib spec_lifecycle` → **5/5 ok** (defaults match
  spec; partial-JSON serde defaults; string priorities; ignored-over-watched).
- `cargo test -p gyre-adapters --lib spec_lifecycle` → **4/4 ok** against a
  real fresh temp-file SQLite DB (absent row → defaults; round-trip; upsert
  overwrites, not duplicates; per-repo isolation).
- `cargo test -p gyre-server --lib spec_lifecycle` → **9/9 ok**, including
  `get_put_traverse_sqlite_migration_end_to_end` (API handlers against a real
  SQLite storage wired exactly like production `store!` — proves the
  migration-created table is genuinely traversed), `put_agent_role_forbidden`
  (JWT-signed agent-role token → 403 on PUT),
  `spec_lifecycle_disabled_skips_task_creation` and
  `spec_lifecycle_per_repo_config_drives_filtering_and_priority` (both drive
  `process_spec_lifecycle` end-to-end over real `git init --bare` repos with
  real pushed commits), and
  `spec_lifecycle_dedup_skip_still_records_spec_changed_audit` (the resolved
  rebase hunk).
- `cargo test -p gyre-server --lib parse_` → **61/61 ok**;
  `cargo test -p gyre-server --lib persona` → **39/39 ok** — both rebase-union
  consumers (config-filtered spec parsing, unfiltered persona parsing) keep
  their semantics.
- `cd web && npx vitest run src/__tests__/RepoSettings.test.js` → **57/57
  passed** (Spec Lifecycle tab: load, toggle, add/remove watched/ignored path
  tags, priority selects, save round-trip, failure toast).
- Mechanical gates, all exit 0: `check-arch`, `check-abac-route-registry`
  (route registered with a `RouteResourceMapping`), `check-migration-versions`
  (`2026-10-08-000059` collides with nothing), `check-migration-sql-portability`
  (plain `CREATE TABLE`, portable to SQLite and PG),
  `check-in-memory-state-stores` (durable state goes through the
  `SpecLifecycleRepository` port + adapters, not a bare AppState map),
  `check-mem-port-contracts`, `check-task-commit-attribution`.
- **Mutation probe** (restored immediately; `git status` clean after):
  removing the `if !config.enabled { return; }` gate from
  `process_spec_lifecycle` makes `spec_lifecycle_disabled_skips_task_creation`
  **FAIL** (task created for the specs/system push). The test is
  load-bearing, not self-confirming. By construction the same fixture drives
  `spec_lifecycle_per_repo_config_drives_filtering_and_priority` (custom
  watched path + Critical priority; the same push contains a `specs/system/`
  file that must NOT produce a task) — hardcoded paths/priorities would fail
  it.

## Verified against acceptance criteria

- **Struct**: `gyre-domain/src/spec_lifecycle_config.rs` — all eight spec
  fields; priorities typed `TaskPriority` (invalid values rejected at
  deserialization — stronger than the plan's `String`). Defaults identical to
  the prior hardcoded behavior (watched system/development, ignored
  milestones/prior-art/personas/prompts, Medium/High/High, all flags true).
- **Persistence**: migration `2026-10-08-000059_spec_lifecycle_configs`
  (repo_id PK, config JSON TEXT); SQLite and Postgres Diesel adapters are real
  (spawn_blocking + pool, upsert `on_conflict(repo_id).do_update()`); absent
  rows mean defaults per the port contract, and all three adapters (sqlite,
  postgres, mem) implement that contract identically.
- **API**: `GET/PUT /api/v1/repos/:id/settings/spec-lifecycle` registered in
  `api/mod.rs` with an ABAC mapping; PUT enforces Admin/Developer in the
  handler body (JWT-backed agent 403 test); unknown repo → 404; GET is the
  same middleware-authenticated pattern as sibling settings endpoints.
  Documented in `docs/api-reference.md`.
- **Hook wiring**: `process_spec_lifecycle` loads config through the same port
  the API writes; `enabled=false` returns before any diff/task/approval work
  (mutation-proven); filtering via `is_watched` (ignored wins, both old and
  new rename paths checked); all three priorities flow from config into
  `classify_spec_change`; `auto_invalidate_approvals` and `dedup_open_tasks`
  gates honored. `SPEC_WATCHED_PATHS` and the hardcoded
  `parse_spec_changes` wrapper are gone; no `specs/system/` literals remain in
  production server code (only in the domain defaults and tests).
- **UI**: full Spec Lifecycle tab in `RepoSettings.svelte` (enabled toggle,
  tag-style add/remove for both path lists with Enter-to-add and dedup guard,
  both flag toggles, three priority selects, save with saved/error states),
  wired through `api.repoSpecLifecycle`/`api.setRepoSpecLifecycle`; locale
  strings in `en.json`.
- **Contract hygiene**: task file acceptance-criteria checkboxes remain open
  (`- [ ]`), status carried via `progress:` + Shipped section; commits
  frontmatter lists all task-labeled commits (attribution gate exit 0).
- Config-load failure in the hook degrades to defaults via
  `unwrap_or_default()` — fail-open toward the *enabled* default, i.e. drift
  detection stays on; the safe direction.

## Notes for verification (not defects)

- `cargo test --all`, all-target clippy, full vitest, and CI at the exact
  merge head remain remote-verification scope per the standing instructions;
  the focused suites above cover every file in the candidate diff.
- `web/dist` at the candidate is identical to the assignment base
  (`git diff b5a872087 -- web/dist` empty); the final integration rebuilds
  and publishes the bundle remotely.
