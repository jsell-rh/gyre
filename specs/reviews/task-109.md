# Review — task-109 (Externalize spec lifecycle configuration)

Spec: `specs/system/spec-lifecycle.md` §Configuration — per-repo `[spec_lifecycle]`
config (enabled, watched_paths, ignored_paths, auto_invalidate_approvals,
dedup_open_tasks, default_priority_new/modified/deleted) replacing the hardcoded
path prefixes and priorities in the post-receive hook.

Assigned: base `918f16bfbf7f85cc9d65b1316a3af35a5e6d5ecb`, candidate
`c7658cc4eee26eba8e8b340e7b3e751685e80b68`. Cumulative diff
`git diff 918f16bf..c7658cc4` (+1723/−150 across domain/ports/adapters/server/web,
migration `2026-10-08-000056_spec_lifecycle_configs`). The final commit
`c7658cc4` touches only `specs/tasks/task-109.md` (contract repair of finding
569e4bd2: the checkbox-flipping `b1526c71` was reverted to the original `[ ]`
prose; verified independently with `scripts/dev-contract.py:requirement_parts`:
base == candidate final form → True). Working tree at HEAD verified pristine
before and after all probes.

Verdict: **complete** (approved).

## Independent probes (evidence under /tmp/stage/review-evidence/task-109/)

- `SKIP_WEB_BUILD=1 cargo test -p gyre-domain --lib spec_lifecycle` — **5/5**
  (defaults match spec byte-for-byte, ignored-over-watched precedence, partial
  JSON deserialization keeps spec defaults, string priorities typed).
- `SKIP_WEB_BUILD=1 cargo test -p gyre-adapters --lib spec_lifecycle` — **4/4**
  (unconfigured → defaults, custom round-trip, upsert overwrite, per-repo
  isolation) against a real SQLite file via `SqliteStorage`.
- `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib spec_lifecycle` — **7/7**:
  API 5 (defaults on GET, PUT round-trip through the same port the hook reads,
  partial-body serde defaults, unknown repo 404, **Agent-role 403 on PUT with a
  real signed JWT through the full router with seeded builtin policies**) and
  hook 2 end-to-end against a real bare git repo with real pushed commits
  (`spec_lifecycle_disabled_skips_task_creation`,
  `spec_lifecycle_per_repo_config_drives_filtering_and_priority`).
- `git_http::tests::parse_spec_changes` — **9/9**; `classify_spec_change` —
  **6/6** (custom watched paths fully replace defaults, ignored-override,
  configured priorities incl. rename→new).
- **Mutation probe:** `process_spec_lifecycle` was temporarily mutated to ignore
  the stored per-repo config (`let config = SpecLifecycleConfig::default()` after
  the port load). Both end-to-end tests **FAILED** exactly as a real regression
  would: the disabled test created a task for `specs/system/a.md`, and the
  per-repo test got `Some("specs/system/a.md")` where `Some("docs/guides/b.md")`
  was required. File restored via `git checkout`; tree re-verified pristine
  (`git diff HEAD` empty) and the suite re-ran **7/7**. The tests are not
  self-confirming — they kill a config-wiring break.
- `cd web && npm ci && npx vitest run src/__tests__/RepoSettings.test.js` —
  **56/56** (tab renders, loads via `api.repoSpecLifecycle`, field rendering
  from config, add/remove path tags, save payload through
  `api.setRepoSpecLifecycle` with the edited lists).
- Mechanical gates all exit 0: arch, ABAC route registry + exempt handlers,
  migration versions (000056 is the next unused after 000055 at base — the
  task text's "currently at 000038" is stale prose, not the contract), migration
  SQL portability, mem port contracts, in-memory state stores, forged scope
  fields, relative path defaults, scope literal defaults, inert enforcement,
  task-commit attribution (all 9 frontmatter commits present on branch),
  byte-slice truncation, fail-open ref resolution, fabricated scope defaults,
  lossy secret conversion, unbounded external HTTP, forwarded header trust.

## Verified working (no findings)

- **Domain** (`gyre-domain/src/spec_lifecycle_config.rs`): all eight spec
  fields; priorities typed as `TaskPriority` (invalid strings rejected at
  deserialization by serde enum error). `Default` matches the spec table
  exactly and the prior hardcoded behavior. `is_watched` applies
  ignored-over-watched precedence.
- **Persistence**: separate `spec_lifecycle_configs` table (repo_id PK, config
  JSON TEXT) in the shared embedded migrations dir; `CREATE TABLE IF NOT
  EXISTS`/`DROP TABLE IF EXISTS` are portable to both SQLite and PostgreSQL.
  Both Diesel adapters (SQLite + Postgres) implement
  `SpecLifecycleRepository` with on-conflict upsert; absent rows mean defaults
  (in the adapter, not just the port doc). Mem adapter is port-backed via the
  `store!` wiring in `AppState` — durable-state invariant respected.
- **API**: `GET/PUT /api/v1/repos/:id/settings/spec-lifecycle` — the assigned
  route contract (restored by 483559c0). PUT enforces Admin/Developer in the
  handler body; the Agent 403 was proven with a real JWT through the full
  router, not a unit-level role check. Route registered in `api/mod.rs` with an
  ABAC `RouteResourceMapping` entry (`abac_middleware.rs:140`); not in any
  exemption file (registry + exempt-handler checks pass). GET mirrors the
  sibling `get_spec_policy` pattern (middleware auth + ABAC scope). Unknown
  repo → 404. Documented in `docs/api-reference.md`.
- **Hook wiring** (`git_http.rs process_spec_lifecycle`): loads config through
  the same port the API writes; `enabled=false` returns before any
  diff/task/approval work; filtering and classification are config-driven;
  `auto_invalidate_approvals=false` skips approval revocation;
  `dedup_open_tasks=false` skips the dedup gate. The workspace scope for
  created tasks now comes from the repo resolved at push time
  (`push_workspace_id`), removing the prior `unwrap_or(Id::new("default"))`
  fabrication. The only remaining `specs/system/` literals in production code
  are the documented defaults in the domain config — correct per the spec.
- **UI**: real Spec Lifecycle tab in `RepoSettings.svelte` (enabled toggle,
  tag-style add/remove for watched/ignored paths, both flag toggles, three
  priority selects), wired to `api.repoSpecLifecycle` /
  `api.setRepoSpecLifecycle`; locale strings in `en.json`; committed
  `web/dist` contains the spec-lifecycle bundle.

## Notes (non-blocking)

- A partial PUT replaces unspecified fields with spec defaults (serde
  field-level defaults) rather than merging with the stored row. The spec
  excerpt defines no PATCH-like merge semantics, PUT-as-full-replacement is a
  defensible reading, the behavior is disclosed in the task notes, and the UI
  always sends the complete config. Recorded so it is a conscious contract.
- `cargo test --all` and exact-head GitHub checks are verification-stage owned;
  this sandbox cannot run TCP listeners (errno 95, `capabilities.json`), so the
  `git_clone_empty_repo_via_smart_http` pre-existing failure and live browser
  drive require host verification. Not a code defect.
