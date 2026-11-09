# Review — task-109 (spec-lifecycle.md §Configuration — Externalize spec lifecycle configuration)

Spec: `specs/system/spec-lifecycle.md` §Configuration (per-repo config: enabled, watched_paths, ignored_paths, auto_invalidate_approvals, dedup_open_tasks, default_priority_new/modified/deleted).
Candidate under review: `5cc62b9531d26dbb91399d5c74161cec6deeabef` (base `fefde2946`). Verdict: **approved**.

This candidate is the end of a long repair chain (migration renumbers 000056→000059 after three base-side collisions, contract repair 569e4bd2, verification repair d9ebdcd9, review repair d2598c14, rebase merges of two assignment bases, CI repair bc54e343). This round independently re-verified the full task contract at the exact head.

## Probes (all at candidate HEAD; full transcripts under `/tmp/stage/review-evidence/task-109-final/`)

- `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib spec_lifecycle` → 8/8. Includes `get_put_traverse_sqlite_migration_end_to_end` (real temp-file SQLite, real GET/PUT traverse of the migration-created table), `put_agent_role_forbidden` (real JWT), and two end-to-end hook tests against a real bare git repo with real pushed commits.
- `SKIP_WEB_BUILD=1 cargo test -p gyre-adapters --lib spec_lifecycle` → 4/4 (real fresh SQLite DBs).
- `SKIP_WEB_BUILD=1 cargo test -p gyre-domain --lib spec_lifecycle` → 5/5; `parse_spec_changes` 9/9; `classify_spec_change` 6/6.
- `npm ci` + `npx vitest run src/__tests__/RepoSettings.test.js` → 57/57 (10-tab merged bar asserting both `Spec Lifecycle` and `Secrets`; spec-lifecycle form tests).
- Mechanical checks all exit 0: check-migration-versions, check-migration-sql-portability, check-abac-route-registry, check-arch, check-task-commit-attribution, check-mem-port-contracts, check-in-memory-state-stores.

## Mutation probes (temporary edits, restored; tree verified clean after each)

1. Removed the `if !config.enabled` early return in `process_spec_lifecycle` → `spec_lifecycle_disabled_skips_task_creation` FAILS. The enabled gate is real and enforced.
2. Forced `config = SpecLifecycleConfig::default()` after the port load (ignore per-repo config) → `spec_lifecycle_per_repo_config_drives_filtering_and_priority` FAILS (`specs/system/a.md` vs `docs/guides/b.md`). Custom watched paths fully replace defaults, and the created task's spec_path and `Critical` priority come from config.
3. Removed the Admin/Developer role check in the PUT handler → `put_agent_role_forbidden` FAILS. Notably, the builtin ABAC policy `builtin-agent-scoped-access` (priority 700) *allows* Agent-role `write` on all resource types, so middleware ABAC would not deny an agent PUT here — the in-handler role check is the actual containment preventing an agent from disabling the drift detector it is subject to, and the test is the only enforcement of it.

## Verified against the acceptance criteria

- **Struct + fields**: `SpecLifecycleConfig` in gyre-domain with all eight spec fields; priorities typed as `TaskPriority` (invalid values rejected at deserialization); serde field-level defaults prevent partial payloads zeroing fields; `is_watched()` applies ignored-over-watched precedence.
- **Persistence**: migration `2026-10-08-000059_spec_lifecycle_configs` (next unused sequence; no version collision — `check-migration-versions.sh` clean), portable SQL (IF NOT EXISTS upcast works on both SQLite and PG), `repo_id` PK with upsert-on-conflict in both Diesel adapters, absent rows = defaults per the port contract. Wired through the `store!` macro exactly like sibling stores; mem mode uses a port-backed `MemSpecLifecycleRepository` (durable-state invariant respected).
- **API**: GET/PUT `/api/v1/repos/:id/settings/spec-lifecycle` registered in `api/mod.rs` with an ABAC `RouteResourceMapping` (registry check clean); PUT role-gated in-handler (mutation-killed probe above); unknown repo → 404; documented in `docs/api-reference.md`.
- **Hook wiring**: `process_spec_lifecycle` loads config through the same port the API writes; workspace scope comes from the repo resolved at push time (no "default" fabrication); no hardcoded spec path literals remain in production code (grep confirms only tests/doc-comments).
- **Defaults**: match the prior hardcoded behavior exactly (unit + adapter tests assert the full default set).
- **UI**: full spec-lifecycle tab in RepoSettings.svelte (enabled toggle, tag-style watched/ignored path lists, both flags, three priority selects), api.js client methods, en.json strings; 57/57 vitest.
- **CI repair (durable finding bc54e343)**: e2e job pinned to `ubuntu-24.04` with a comment naming the Playwright 1.58.2 lockfile constraint; valid YAML, 12 steps intact; workflow-only change (no Rust/web source), matching the identical repair on sibling task branches. The underlying failure (locked playwright-core@1.58.2 has no ubuntu26 registry entries) was previously verified in this sandbox against the locked package source; the live exact-head re-run belongs to remote verification.
- **Contract integrity**: task file acceptance checkboxes remain `[ ]` (569e4bd2 repair holds); coverage row 11 accurately updated to `implemented` with "Awaiting independent verification"; `web/dist` is byte-identical to the assignment base (0 diff lines — matches the retracted-dist-claim disclosure; no false claim).

## Minor observations (non-blocking, recorded for the record)

- `process_spec_lifecycle` loads config with `.unwrap_or_default()`, so a storage error silently degrades to default config. The failure direction is benign (defaults = enabled + prior hardcoded behavior; drift detection cannot be disabled by an outage) and matches the sibling `spec_policies`/`repo_push_gates` degrade-to-prior pattern in the same file. A `warn!` on the error path would be tidier.
- The `auto_invalidate_approvals=false` and `dedup_open_tasks=false` skip paths have no dedicated hook tests (the true paths are pre-existing, coverage-row-9-verified behavior; the guards are simple `if config.X` inversions). The critical flags (`enabled`, watched paths, priorities) all have mutation-verified killing tests.

`cargo test --all`, the full web suite, and exact-head GitHub checks are owned by the verification stage. Nothing in this review contradicts the task contract; the implementation is real, enforced, and tested with killing tests.
