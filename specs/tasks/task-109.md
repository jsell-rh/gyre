---
title: "Externalize spec lifecycle configuration"
spec_ref: "spec-lifecycle.md §Configuration"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "spec-lifecycle.md §Configuration"
commits: ["48a346a28d33f40d7d16899c36f20078c002f32c", "e667278c839308ee3fbb591930b2c4b717090f8b", "2fb3ca466628b677a612cad2b14706469c6eeb8b", "5b2b0684ed92530fceec3293d9472ae13120daca", "696c03f2df6a0459c3e3d1d2971e6a3340414cae", "0dbe7863b4677f170458f24b1f6d76565a26ce24", "c8936f051f24811d4708d5350ee3f0adc7335bb2", "caadfea4b4707d80b0217f8ebd8771e195180fb7", "a9b7d4945979728e7461906fc5532000013b07c9", "483559c07e9276be64746e96fa5fde1c005c0250", "8cf379a075e2ecb069fc3084f0edbcd298a7400b", "219b976d5634ab1a393c2a18b88d8b108eaf00d1", "8f512ea3ce235e80fd0e751b4ed625ce22ccaa16", "be0b113d055d0a2e690b1cb47c5fde53420bfb00", "8fce2a130fb6b2d75d840ab4445cf411186512db", "e74159d30c0ea5f639597e3c9e366335ebce2e28"]
---

## Spec Excerpt

From `spec-lifecycle.md` §Configuration:

```toml
# Per-repo spec lifecycle config
[spec_lifecycle]
enabled = true
watched_paths = ["specs/system/", "specs/development/"]
ignored_paths = ["specs/milestones/", "specs/prior-art/", "specs/personas/", "specs/prompts/"]
auto_invalidate_approvals = true
dedup_open_tasks = true
default_priority_new = "Medium"
default_priority_modified = "High"
default_priority_deleted = "High"
```

Current state: Watched paths are hardcoded as `["specs/system/", "specs/development/"]` in `git_http.rs:1270`. The spec requires per-repo configuration with all the options above.

## Implementation Plan

1. **Add spec_lifecycle config to repository settings:**
   - Extend the `Repository` domain entity (or a related config entity) with a `spec_lifecycle_config` field
   - Define `SpecLifecycleConfig` struct in `gyre-domain`:
     ```rust
     pub struct SpecLifecycleConfig {
         pub enabled: bool,
         pub watched_paths: Vec<String>,
         pub ignored_paths: Vec<String>,
         pub auto_invalidate_approvals: bool,
         pub dedup_open_tasks: bool,
         pub default_priority_new: String,
         pub default_priority_modified: String,
         pub default_priority_deleted: String,
     }
     ```
   - Provide sensible defaults matching current hardcoded values

2. **Database migration:**
   - Add `spec_lifecycle_config` JSON column to `repositories` table (or a separate `repo_configs` table)
   - Default value: JSON with the current hardcoded defaults

3. **API endpoint:**
   - `GET /api/v1/repos/:id/settings/spec-lifecycle` — get current config
   - `PUT /api/v1/repos/:id/settings/spec-lifecycle` — update config
   - Only Admins and Developers can modify

4. **Wire into git_http.rs:**
   - In `process_spec_lifecycle()`, load the repo's spec lifecycle config instead of using hardcoded paths
   - Use `config.watched_paths` and `config.ignored_paths` for filtering
   - Use `config.default_priority_*` for task creation
   - Respect `config.enabled` — skip spec lifecycle processing if disabled
   - Respect `config.auto_invalidate_approvals` flag
   - Respect `config.dedup_open_tasks` flag

5. **UI integration:**
   - Add spec lifecycle section to RepoSettings.svelte
   - Show current config with editable fields
   - Path inputs as tag-style list (add/remove watched/ignored paths)

## Acceptance Criteria

- [ ] `SpecLifecycleConfig` struct in gyre-domain with all spec fields
- [ ] Database column stores per-repo config
- [ ] GET/PUT API for spec lifecycle settings
- [ ] git_http.rs uses per-repo config instead of hardcoded values
- [ ] Config defaults match current behavior
- [ ] Disabling `enabled` skips spec lifecycle processing
- [ ] Priority overrides work for task creation
- [ ] UI shows and edits spec lifecycle settings
- [ ] `cargo test --all` passes

## Agent Instructions

Read `specs/system/spec-lifecycle.md` §Configuration for the full config spec. The current hardcoded paths are in `gyre-server/src/git_http.rs` — search for `specs/system/` or `specs/development/` near `process_spec_lifecycle`. The Repository domain model is in `gyre-domain/src/repository.rs`. Repo settings UI is in `web/src/components/RepoSettings.svelte`. API route registration is in `gyre-server/src/api/mod.rs`. Check migration numbering — currently at 000038.

## Shipped

Per-repo spec lifecycle configuration (spec-lifecycle.md §Configuration)
replaces the previously hardcoded watched/ignored path prefixes and task
priorities in the post-receive hook.

### Behavior

- **Domain** (`gyre-domain/src/spec_lifecycle_config.rs`):
  `SpecLifecycleConfig` with all eight spec fields — `enabled`,
  `watched_paths`, `ignored_paths`, `auto_invalidate_approvals`,
  `dedup_open_tasks`, and `default_priority_new/modified/deleted` as typed
  `TaskPriority` (invalid values rejected at deserialization). Defaults match
  the prior hardcoded behavior exactly: watched
  `["specs/system/", "specs/development/"]`, ignored
  `["specs/milestones/", "specs/prior-art/", "specs/personas/",
  "specs/prompts/"]`, priorities Medium/High/High, all flags true. Serde
  field-level defaults keep a partial JSON payload from zeroing unspecified
  fields; `is_watched()` applies ignored-over-watched precedence.
- **Persistence** (migration `2026-10-08-000059_spec_lifecycle_configs`):
  separate `spec_lifecycle_configs` table (`repo_id` PK, `config` JSON TEXT),
  portable SQL for both SQLite and PostgreSQL through the shared embedded
  migrations dir. Diesel adapters (SQLite + Postgres) implement the
  `SpecLifecycleRepository` port with upsert-on-conflict; absent rows mean
  defaults. Wired through the `store!` macro in `AppState`; in-memory mode
  uses `MemSpecLifecycleRepository` (port-backed per the durable-state
  invariant).
- **API** (`GET/PUT /api/v1/repos/:id/settings/spec-lifecycle`): PUT is
  Admin/Developer-only — the Agent role gets 403 (an agent must not be able
  to disable the drift detector it is subject to); GET returns defaults for
  unconfigured repos; unknown repo → 404. Route registered in `api/mod.rs`
  with an ABAC `RouteResourceMapping` entry; documented in
  `docs/api-reference.md`.
- **Hook wiring** (`git_http.rs process_spec_lifecycle`): loads per-repo
  config through the same port the API writes; `enabled=false` returns
  before any diff/task/approval work; filtering via `config.is_watched`
  (ignored wins); `classify_spec_change` takes priorities from config
  (A/R→new, M→modified, D→deleted); `auto_invalidate_approvals=false` skips
  approval revocation; `dedup_open_tasks=false` skips the open-task dedup
  gate. No hardcoded path literals remain in production code. Workspace
  scoping for emitted events: the repo is re-resolved once per push and,
  when the row is unresolvable, workspace-scoped events are skipped rather
  than emitted under a fabricated scope (task-097 F3 class); approval
  invalidation is independent of the repo row and still runs (see the
  current rebase round below).
- **UI** (`RepoSettings.svelte`): "Spec Lifecycle" tab with enabled toggle,
  tag-style add/remove lists for watched/ignored paths, both flag toggles,
  and three priority selects; loads via `api.repoSpecLifecycle`, saves via
  `api.setRepoSpecLifecycle`; locale strings in `en.json`.

### Repair history (condensed; full detail in earlier review rounds)

- **Contract (569e4bd2):** an earlier candidate marked the
  acceptance-criteria checkboxes as checked, which the verifier counts as a
  change to the assigned requirements; the contract was restored — the task
  file keeps its original open checkboxes and carries status via
  `progress:` plus this section.
- **Migration numbering:** renumbered three times (`000056`→`000057`→
  `000058`→`000059`, findings 7ac79a6a, d9ebdcd9, 7a1761da) because
  successive assignment bases added colliding migrations on the same
  date-sequence; Diesel runs only one migration per version, so a collision
  silently drops the `spec_lifecycle_configs` table on fresh databases.
  Current version `2026-10-08-000059` does not collide with any merged-base
  migration; `check-migration-versions.sh` passes at the current head.
- **Verification repair (d9ebdcd9, commit 696c03f2):** rustfmt drift,
  a missing `#[allow(clippy::double_must_use)]` on the port trait, and a
  discarded must-use `Result` in a test — all fixed; clippy-diff clean.
- **Review repair (d2598c14, commit 2fb3ca466):** added
  `get_put_traverse_sqlite_migration_end_to_end` (real SQLite-backed
  GET/PUT traverse of the migration-created table — the exact failure mode
  behind the renumber rounds); coverage row updated to `implemented`.
  `web/dist` is NOT rebuilt or committed on this branch — the executor
  restores dist from the assignment base and final integration rebuilds and
  publishes it remotely; the task-109 UI source in `web/src` is intact.
- **CI repair (bc54e343):** the `E2E Tests` job failed in
  "Install Playwright browsers" — `ubuntu-latest` now resolves to ubuntu
  26.04 and the locked `playwright-core@1.58.2` has no chromium registry
  entries for ubuntu ≥ 25. Fixed by pinning the `e2e` job to
  `runs-on: ubuntu-24.04` in `.github/workflows/e2e.yml`. Not a candidate
  defect; the identical pin is now also on main (present in the current
  assignment base), so the merged workflow carries it on both sides.

### Rebase repair rounds

Three assignment bases have been merged into this branch. The first two
(`ed57c723` task-201 persistent search, as `2bb4d127`; `fefde2946` task-089
HSI Explorer, as `8e37d4561`) touched no task-109 source semantics — the
former conflicted only in `web/dist` artifacts (taken from the base
wholesale), the latter additively in `AppState` wiring and the
`RepoSettings.svelte` tab bar (both sides kept; the merged bar renders 10
tabs and the tab-count test was updated accordingly with all labels still
asserted).

**Current round (finding 28f33736, merge commit `e135f37de`):** merged
assignment base `774737638` (task-177 rendering technology, plus task-099
bootstrap fallback and spec-approval ledger path normalization). One source
conflict, in `process_spec_lifecycle`: the candidate had added a
`workspace_id: &Id` parameter and emitted events unconditionally under the
push-time resolved workspace; the base hoisted the workspace resolution to
once-per-push returning `Option<Id>`, skips only the workspace-scoped
events when the repo row is unresolvable, and keeps approval invalidation
and cross-workspace notification running (its test
`push_modifying_spec_revokes_ledger_approvals` relies on that decoupling —
it drives the hook with a repo id that has no repo row). Resolved by
adopting the base's shape: the conflict hunk takes the base side, the
candidate's now-redundant `workspace_id` parameter is removed, and the
production call site plus the two task-109 hook tests drop the argument
(their fixture registers the real repo row, so the per-push re-lookup
resolves the same workspace the tests previously passed explicitly). Both
sides' invariants hold: no fabricated scope on lookup failure (events are
skipped), and the task-097 F3 class rule is satisfied. The diff vs the base
parent now shows only the task-109 config machinery. All auto-merged
overlap files (`schema.rs`, `lib.rs`s, `abac_middleware.rs`, `api/mod.rs`,
`mem.rs`, `middleware.rs`, `docs/api-reference.md`, `en.json`, `e2e.yml`)
verified to carry both sides' symbols. Evidence:
`/tmp/stage/review-evidence/task-109/rebase-repair-7747376.md`.

### Test evidence

Earlier rounds (before the current merge) recorded, at their then-heads:
`cargo test -p gyre-server --lib spec_lifecycle` — 8/8 (6 API incl. the
SQLite-backed migration traverse, plus 2 end-to-end hook tests against a
real bare git repo); `cargo test -p gyre-adapters --lib spec_lifecycle` —
4/4 against a real fresh SQLite DB; `cargo test -p gyre-domain --lib
spec_lifecycle` — 5/5; `parse_spec_changes` — 9/9 and
`classify_spec_change` — 6/6; `npx vitest run RepoSettings.test.js` — 57/57
and `SecretsPanel.test.js` — 9/9; clippy-diff and fmt clean.

At the current merge head (`e135f37de`) this sandbox instance has NO
outbound network (direct TCP refused) and no prewarmed cargo registry,
target dir, or `web/node_modules`, so Rust and vitest suites could not be
executed here — recorded as a transport restriction for remote
verification, not a code defect. Probes that did run here, all exit 0:
`cargo fmt --all --check` (rustfmt parses every workspace crate — syntax
validation of the merged tree), `check-migration-versions.sh`,
`check-migration-sql-portability.sh`, `check-arch.sh`,
`check-abac-route-registry.sh`, `check-task-commit-attribution.sh`, and
the remaining mechanical gates (byte-slice, relative-path defaults,
fail-open refs, fabricated/lossy/scope-literal defaults, inert
enforcement, dead message kinds, mem port contracts, forwarded headers,
in-memory state stores, unbounded HTTP, forged scope fields, MCP write
tools, ABAC exempt handlers); `e2e.yml` parses as YAML and `en.json` as
JSON. Full suites at the exact merge head belong to remote verification;
the merge-conflict resolution itself is a three-way-diff-verified union
described above.
