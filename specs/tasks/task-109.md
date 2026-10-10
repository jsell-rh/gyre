---
title: "Externalize spec lifecycle configuration"
spec_ref: "spec-lifecycle.md §Configuration"
depends_on: []
progress: needs-revision
coverage_sections:
  - "spec-lifecycle.md §Configuration"
commits: ["caadfea4b4707d80b0217f8ebd8771e195180fb7", "a9b7d4945979728e7461906fc5532000013b07c9", "483559c07e9276be64746e96fa5fde1c005c0250", "8cf379a075e2ecb069fc3084f0edbcd298a7400b", "219b976d5634ab1a393c2a18b88d8b108eaf00d1", "8f512ea3ce235e80fd0e751b4ed625ce22ccaa16", "be0b113d055d0a2e690b1cb47c5fde53420bfb00", "8fce2a130fb6b2d75d840ab4445cf411186512db", "e74159d30c0ea5f639597e3c9e366335ebce2e28"]
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

**Domain** (`gyre-domain/src/spec_lifecycle_config.rs`): `SpecLifecycleConfig`
with all eight spec fields — `enabled`, `watched_paths`, `ignored_paths`,
`auto_invalidate_approvals`, `dedup_open_tasks`, and
`default_priority_new/modified/deleted` as typed `TaskPriority` (invalid
values rejected at deserialization). Defaults match the prior hardcoded
behavior exactly: watched `["specs/system/", "specs/development/"]`, ignored
`["specs/milestones/", "specs/prior-art/", "specs/personas/",
"specs/prompts/"]`, priorities Medium/High/High, all flags true. Serde
field-level defaults keep a partial JSON payload from zeroing unspecified
fields; `is_watched()` applies ignored-over-watched precedence.

**Persistence** (migration `2026-10-08-000056_spec_lifecycle_configs`):
separate `spec_lifecycle_configs` table (`repo_id` PK, `config` JSON TEXT),
portable SQL for both SQLite and PostgreSQL through the shared embedded
migrations dir. Diesel adapters (SQLite + Postgres) implement the
`SpecLifecycleRepository` port with upsert-on-conflict; absent rows mean
defaults. Wired through the `store!` macro in `AppState`; in-memory mode uses
`MemSpecLifecycleRepository` (port-backed per the durable-state invariant).

**API** (`GET/PUT /api/v1/repos/:id/settings/spec-lifecycle`, the assigned
route contract restored by 483559c0 after the first review round): PUT is
Admin/Developer-only — the Agent role gets 403 (an agent must not be able to
disable the drift detector it is subject to); GET returns defaults for
unconfigured repos; unknown repo → 404. Route registered in `api/mod.rs`
with an ABAC `RouteResourceMapping` entry; documented in
`docs/api-reference.md`.

**Hook wiring** (`git_http.rs process_spec_lifecycle`): loads per-repo
config through the same port the API writes; `enabled=false` returns before
any diff/task/approval work; filtering via `config.is_watched` (ignored
wins); `classify_spec_change` takes priorities from config (A/R→new,
M→modified, D→deleted); `auto_invalidate_approvals=false` skips approval
revocation; `dedup_open_tasks=false` skips the open-task dedup gate. The
workspace scope for created tasks comes from the repo resolved at push
time — no "default" workspace fabrication on re-lookup failure. No
hardcoded path literals remain in production code.

**UI** (`RepoSettings.svelte`): "Spec Lifecycle" tab with enabled toggle,
tag-style add/remove lists for watched/ignored paths, both flag toggles, and
three priority selects; loads via `api.repoSpecLifecycle`, saves via
`api.setRepoSpecLifecycle`; locale strings in `en.json`.

**Contract-repair round (finding 569e4bd2):** the prior candidate
`b1526c71` closed the acceptance-criteria checkboxes as `[x]`, which
`scripts/dev-contract.py:requirement_parts` counts as a change to the
assigned requirements (checkboxes are contract prose; only `Shipped` /
`Review` are operational sections excluded from the contract hash). Same
failure mode as task-170's documented repair. This round restored the
assigned contract — the task file keeps its original `[ ]` checkboxes and
carries status via `progress:` plus this section, matching completed tasks
task-189/task-212 on main. Implementation tree is byte-identical to the
published candidate apart from this file (`git diff b1526c71 HEAD --
crates/ web/src/ docs/` empty); the assignment base merge 918f16bf is
specs-only (task-216.md), so no code re-verification gap. Contract-hash
equivalence proven with `dev-contract.py.requirement_parts`: base ==
this file's final form (True), base == checkbox-flipped form (False).

**Test evidence** (focused probes at HEAD `9f5e5e81` on this branch,
recorded under `/tmp/stage/review-evidence/task-109/`):
- `SKIP_WEB_BUILD=1 cargo test -p gyre-domain --lib spec_lifecycle` — 5/5
- `SKIP_WEB_BUILD=1 cargo test -p gyre-adapters --lib spec_lifecycle` — 4/4
  (unconfigured defaults, round-trip, upsert overwrite, per-repo isolation)
- `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib spec_lifecycle` — 7/7
  (API 5 incl. Agent-role 403 with a real JWT on the assigned
  `/settings/spec-lifecycle` path; hook 2: `enabled=false` creates no tasks
  and custom `watched_paths` fully replace defaults with
  `default_priority_new: Critical` landing on the created task — end-to-end
  against a real bare git repo with real pushed commits)
- `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib
  'git_http::tests::parse_spec_changes'` — 9/9 and
  `'git_http::tests::classify_spec_change'` — 6/6 (config-driven filtering,
  custom paths, ignored-override, configured priorities)
- `cd web && npm ci && npx vitest run
  src/__tests__/RepoSettings.test.js` — 56/56
- All 17 mechanical invariant checks exit 0 (arch, ABAC route registry,
  migration versions, migration SQL portability, mem port contracts,
  in-memory state stores, forged scope fields, relative path defaults,
  scope literal defaults, inert enforcement, task-commit attribution, byte
  slice truncation, fail-open ref resolution, fabricated scope defaults,
  lossy secret conversion, unbounded external HTTP).

`cargo test --all` and exact-head GitHub checks are owned by the
verification stage per assignment instructions; this sandbox cannot run TCP
listeners (capability probe errno 95, `capabilities.json`), so the
`git_clone_empty_repo_via_smart_http` test and live-UI browser drive
require host verification.

## Review

### Review changed source code

- crates/gyre-server/src/api/spec_lifecycle.rs

Preserved these edits for implementation. Review cannot approve its own source or verifier edits. Repair them within task scope and request a fresh independent review.
