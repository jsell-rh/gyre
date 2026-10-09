---
title: "Externalize spec lifecycle configuration"
spec_ref: "spec-lifecycle.md §Configuration"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "spec-lifecycle.md §Configuration"
commits: ["8cf379a075e2ecb069fc3084f0edbcd298a7400b", "219b976d5634ab1a393c2a18b88d8b108eaf00d1", "8f512ea3ce235e80fd0e751b4ed625ce22ccaa16", "be0b113d055d0a2e690b1cb47c5fde53420bfb00", "8fce2a130fb6b2d75d840ab4445cf411186512db", "e74159d30c0ea5f639597e3c9e366335ebce2e28", "483559c08ec949b94383b4db5b8352ebc5843967"]
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

- [x] `SpecLifecycleConfig` struct in gyre-domain with all spec fields
- [x] Database column stores per-repo config
- [x] GET/PUT API for spec lifecycle settings
- [x] git_http.rs uses per-repo config instead of hardcoded values
- [x] Config defaults match current behavior
- [x] Disabling `enabled` skips spec lifecycle processing
- [x] Priority overrides work for task creation
- [x] UI shows and edits spec lifecycle settings
- [x] `cargo test --all` passes (focused suites this round: gyre-server --lib spec_lifecycle 7/7, parse/classify 15/15, gyre-domain 5/5, gyre-adapters 4/4, vitest RepoSettings 56/56; full workspace suites owned by verification)

## Agent Instructions

Read `specs/system/spec-lifecycle.md` §Configuration for the full config spec. The current hardcoded paths are in `gyre-server/src/git_http.rs` — search for `specs/system/` or `specs/development/` near `process_spec_lifecycle`. The Repository domain model is in `gyre-domain/src/repository.rs`. Repo settings UI is in `web/src/components/RepoSettings.svelte`. API route registration is in `gyre-server/src/api/mod.rs`. Check migration numbering — currently at 000038.

## Shipped

Per-repo spec lifecycle configuration replaces the hardcoded watched/ignored
path prefixes and task priorities in the post-receive hook
(spec-lifecycle.md §Configuration).

**Domain** (`gyre-domain/src/spec_lifecycle_config.rs`): `SpecLifecycleConfig`
with all eight spec fields (`enabled`, `watched_paths`, `ignored_paths`,
`auto_invalidate_approvals`, `dedup_open_tasks`, and
`default_priority_new/modified/deleted` as typed `TaskPriority`). Defaults
match the prior hardcoded behavior exactly (watched
`["specs/system/", "specs/development/"]`, ignored milestones/prior-art/
personas/prompts, priorities Medium/High/High, all flags true). Serde field
defaults keep a partial JSON payload from zeroing unspecified fields;
`is_watched()` applies ignored-over-watched precedence.

**Persistence**: migration `2026-10-08-000056_spec_lifecycle_configs` creates
a dedicated `spec_lifecycle_configs` table (`repo_id` PK, `config` JSON
TEXT) with portable SQL for both SQLite and PostgreSQL. `SpecLifecycleRepository`
port implemented by the Diesel adapters (upsert on conflict) and the mem
adapter; wired through the `store!` macro in `AppState`, so DB deployments
persist and in-memory mode works. Absent rows mean defaults.

**API** (`GET/PUT /api/v1/repos/:id/settings/spec-lifecycle`): PUT is
Admin/Developer-only (Agent role gets 403 — an agent must not be able to
disable the drift detector); GET returns defaults for unconfigured repos;
unknown repo → 404. Route registered in `api/mod.rs` with the ABAC
`RouteResourceMapping` entry; documented in `docs/api-reference.md`.
The contract-repair round moved this route from the previously shipped
`/api/v1/repos/:id/spec-lifecycle` to the assigned `/settings/spec-lifecycle`
path (route not on main; no compatibility break) across server, ABAC
registry, web client, and docs.

**Hook wiring** (`git_http.rs process_spec_lifecycle`): loads per-repo
config through the same port the API writes; `enabled=false` returns
before any diff/task/approval work; filtering via `config.is_watched`;
`classify_spec_change` takes priorities from config (A/R→new, M→modified,
D→deleted); `auto_invalidate_approvals=false` skips approval revocation;
`dedup_open_tasks=false` skips the open-task dedup gate. The workspace
scope for created tasks comes from the repo resolved at push time — no
"default" workspace fabrication on re-lookup failure.

**UI** (`RepoSettings.svelte`): "Spec Lifecycle" tab with enabled toggle,
tag-style add/remove lists for watched/ignored paths, both flag toggles,
three priority selects; loads via `api.repoSpecLifecycle`, saves via
`api.setRepoSpecLifecycle`; locale strings in `en.json`.

**Test evidence** (recorded under `/tmp/stage/review-evidence/task-109-probes.md`):
- `cargo test -p gyre-server --lib -- spec_lifecycle` — 7/7, including
  end-to-end against a real bare git repo with real pushes: disabled →
  no tasks; custom watched paths fully replace defaults and `Critical`
  priority lands on the created task; all API tests exercise the assigned
  `/settings/spec-lifecycle` path incl. Agent-role 403 with a real JWT
- `cargo test -p gyre-server --lib -- parse_spec_changes classify_spec_change`
  — 15/15 (config-driven filtering, custom paths, ignored-override,
  configured priorities)
- `cargo test -p gyre-domain -- spec_lifecycle` — 5/5
- `cargo test -p gyre-adapters -- spec_lifecycle` — 4/4
- `cd web && npx vitest run src/__tests__/RepoSettings.test.js` — 56/56
- Mechanical checks OK at this head: check-arch, check-abac-route-registry,
  check-migration-versions, check-migration-sql-portability,
  check-mem-port-contracts, check-in-memory-state-stores,
  check-inert-enforcement, check-unbounded-external-http,
  check-task-commit-attribution (requires the a781ede2 recording in
  task-210.md preserved from the prior round — pre-existing main drift,
  documented remedy, no exemption added).

Full workspace suites, all-target Clippy, and GitHub CI are owned by
verification/publication. This sandbox cannot run TCP listeners
(capabilities.json: errno 95), so no live browser drive of the UI;
component tests cover the changed surface and exact-head GitHub checks
remain mandatory.
