# Review — task-109 (Externalize spec lifecycle configuration)

Spec: `specs/system/spec-lifecycle.md` §Configuration — per-repo config with
`enabled`, `watched_paths`, `ignored_paths`, `auto_invalidate_approvals`,
`dedup_open_tasks`, `default_priority_new/modified/deleted`, replacing the
hardcoded watched-path prefixes and task priorities in the post-receive hook.

Candidate: `61e92ab3169894094d36f7ca2b2d81af05ffb0cd` on
`pipeline/task-109/08b6b15fd69348d8b2e548d532df1096-1`, assigned base
`ed57c723631e7ad99b8983cf127fdca3eefb1a9a` (merged into the branch as
`2bb4d127`; `git diff c7658cc 2bb4d127 -- <task-109 surface>` is empty — the
merge only rebuilt `web/dist` artifacts). Cumulative diff
`git diff ed57c723 61e92ab3`: +1731/−150 across domain/ports/adapters/server
spec-lifecycle files, migration 000056, `api/mod.rs` + `abac_middleware.rs`
registrations, `RepoSettings.svelte` + `api.js` + `en.json`, docs, task file,
and rebuilt `web/dist`.

Verdict: **complete** (approved).

## Probes (evidence under `/tmp/stage/review-evidence/task-109/`)

All run at the exact candidate HEAD with the working tree clean (`git status`
pristine before and after; mutation experiments fully restored and re-verified
green):

- `SKIP_WEB_BUILD=1 cargo test -p gyre-domain --lib spec_lifecycle` — **5/5**
  (defaults match spec exactly, watched/ignored filtering, ignored-over-watched
  precedence, partial-JSON serde defaults, string priorities).
- `SKIP_WEB_BUILD=1 cargo test -p gyre-adapters --lib spec_lifecycle` — **4/4**
  (unconfigured defaults, round-trip, upsert-overwrite, per-repo isolation)
  against real SQLite storage.
- `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib spec_lifecycle` — **7/7**:
  API 5 (defaults for unconfigured repo, PUT round-trip through the same port
  the hook reads, partial-body serde defaults, unknown repo 404, Agent-role
  403 with a real signed JWT on the assigned route) and hook 2 — end-to-end
  against a real bare git repo with real pushed commits to main
  (`enabled=false` creates no tasks; custom `watched_paths` fully replace the
  defaults with `default_priority_new: Critical` landing on the created task
  while the `specs/system/` file in the same diff produces no task).
- `cargo test -p gyre-server --lib parse_spec_changes` — **9/9**;
  `classify_spec_change` — **6/6** (config-driven filtering, custom paths,
  ignored-override, configured priorities, rename→new priority).
- `cd web && npm ci && npx vitest run src/__tests__/RepoSettings.test.js` —
  **56/56**.
- **Mutation probe 1:** removed the `if !config.enabled { return; }` gate from
  `process_spec_lifecycle` — `spec_lifecycle_disabled_skips_task_creation`
  **FAILED** (tasks created). Test kills the disabled behavior. Restored.
- **Mutation probe 2:** removed the Admin/Developer role check from the PUT
  handler — `put_agent_role_forbidden` **FAILED with left: 200, right: 403**.
  This proves the 403 is enforced by the handler's own role check (the ABAC
  middleware alone permits the request), i.e. the acceptance criterion
  "Agent may not modify" is real enforcement, not an incidental middleware
  artifact. Restored; full 7/7 suite re-run green post-restore.
- Mechanical gates on the tree, all exit 0: `check-arch.sh`,
  `check-abac-route-registry.sh`, `check-migration-versions.sh`,
  `check-mem-port-contracts.sh`, `check-in-memory-state-stores.sh`,
  `check-task-commit-attribution.sh`, `check-api-auth.sh`,
  `check-forged-scope-fields.sh`, `check-migration-sql-portability.sh`.
- Task contract unmodified: `scripts/dev-contract.py:requirement_parts`
  against the assigned base text from `/tmp/stage/review-before.json` —
  frontmatter **match: True**, prose **match: True** (acceptance-criteria
  checkboxes remain `[ ]`; status carried via `progress:` + Shipped, per the
  contract-repair round for finding 569e4bd2).

## Verified working (no findings)

- **Domain** (`gyre-domain/src/spec_lifecycle_config.rs`): all eight spec
  fields present; defaults byte-match the spec block (watched
  `specs/system/`+`specs/development/`, ignored milestones/prior-art/personas/
  prompts, priorities Medium/High/High, all flags true). Priorities are typed
  `TaskPriority` (plain serde enum → invalid strings rejected at
  deserialization, verified against `task.rs:22-27`). Serde field-level
  defaults prevent partial JSON payloads from zeroing unspecified fields
  (tested). `is_watched` applies ignored-over-watched precedence (tested).
- **Persistence**: separate `spec_lifecycle_configs` table (`repo_id` PK,
  `config` JSON TEXT), migration `2026-10-08-000056_spec_lifecycle_configs` —
  next unused sequence number after 000055, no duplicates
  (`check-migration-versions.sh`), portable SQL for both backends
  (`check-migration-sql-portability.sh`; shared embedded `migrations/` dir is
  run by both `sqlite/mod.rs` and `postgres/mod.rs`). Both Diesel adapters
  implement `SpecLifecycleRepository` with upsert-on-conflict; absent rows →
  domain defaults (port contract documented and honored by mem adapter too).
  Wired through `store!` in `AppState` (production adapters) and
  `MemSpecLifecycleRepository` in mem mode — port-backed, satisfying the
  durable-state invariant (`check-in-memory-state-stores.sh` clean).
- **API**: GET/PUT `/api/v1/repos/:id/settings/spec-lifecycle` — the assigned
  route contract, registered in `api/mod.rs:188-192` with an ABAC
  `RouteResourceMapping` entry (`abac_middleware.rs:140`, resource `repo`).
  GET mirrors the sibling settings handlers (`spec-policy`, `stack-policy` —
  no per-handler auth extractor, middleware ABAC governs; consistent
  pre-existing pattern). PUT enforces Admin/Developer in the handler body
  (mutation-proved), unknown repo → 404 before any write. Documented in
  `docs/api-reference.md`.
- **Hook wiring** (`git_http.rs process_spec_lifecycle`): config loaded
  through the same port the API writes; `enabled=false` returns before any
  diff/task/approval work (mutation-proved); filtering via
  `parse_spec_changes_with_config` + `config.is_watched` (ignored wins);
  `classify_spec_change` takes priorities from config (A/R→new, M→modified,
  D→deleted); `auto_invalidate_approvals=false` skips `revoke_all_for_path`;
  `dedup_open_tasks=false` skips the open-task dedup gate. Task workspace
  scope comes from the repo resolved at push time — no "default" workspace
  fabrication. No `specs/system/` / `specs/development/` path literals remain
  in production code (grep: only tests and the domain defaults constant).
- **UI**: "Spec Lifecycle" tab in `RepoSettings.svelte` with enabled toggle,
  tag-style add/remove for watched/ignored paths, both flag toggles, and three
  priority selects restricted to Low/Medium/High/Critical; loads via
  `api.repoSpecLifecycle`, saves via `api.setRepoSpecLifecycle`; locale
  strings complete in `en.json`; 56/56 vitest including the new tab tests.
- **Diff scope**: only task-109 surface plus the rebuilt `web/dist` artifacts
  (self-consistent set from the merged source) — no unrelated production,
  script, or spec changes.

## Deferred to verification (infrastructure, not code defects)

This sandbox's crates.io route was intermittent; the first cargo invocation
failed with a connection error and succeeded on retry (recorded in
`01-domain-tests.txt`). Owned by the verification stage per assignment:
`cargo test --all`, all-target Clippy, `git_clone_empty_repo_via_smart_http`
(needs a TCP listener), live-UI browser drive against a rebuilt web bundle,
and exact-head GitHub checks.

Minor (not blocking, recorded for completeness):

- `auto_invalidate_approvals=false` and `dedup_open_tasks=false` have no
  dedicated disabling tests; both are single-line conditionals around
  behavior covered elsewhere, and the two acceptance-criteria behaviors that
  the task names explicitly (enabled gate, priority overrides) are
  mutation-proved. Noted so verification can extend coverage if desired.
