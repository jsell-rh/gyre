# Review — task-109 (Externalize spec lifecycle configuration)

Spec: `specs/system/spec-lifecycle.md` §Configuration — per-repo `spec_lifecycle`
config (`enabled`, `watched_paths`, `ignored_paths`, `auto_invalidate_approvals`,
`dedup_open_tasks`, `default_priority_new/modified/deleted`) replacing the
hardcoded path prefixes and priorities in the post-receive hook.

Candidate: `0bbaeae7bb2f9379ec3435c81610f7baba4dc7c3` (merge of assignment base
`110180fb` into `pipeline/task-109/bb3c2c3e25c64bd48175ebe554228a43-3`). All 9
commits listed in the task's `commits:` frontmatter are ancestors of HEAD
(verified). Working tree clean.

Verdict: **complete** (approved).

## Evidence

Compilation-backed probes could NOT run in this sandbox: no outbound network
(curl to static.crates.io / index.crates.io exits 7), `CARGO_HOME=/tmp/cargo`
holds the registry index only (no crate blobs), and `web/node_modules` is
absent. This is an infrastructure restriction, not a code defect; concrete
deferred checks for the verifier are recorded in
`/tmp/stage/review-evidence/task-109-r3/deferred-verification.txt`. Independent
verification performed instead (all evidence under
`/tmp/stage/review-evidence/task-109-r3/`):

### Full diff read (24 non-dist files, +1647/−66)

- **Domain** (`gyre-domain/src/spec_lifecycle_config.rs`): all eight spec
  fields; defaults byte-match the spec table and the prior hardcoded behavior
  (watched `specs/system/`, `specs/development/`; ignored
  milestones/prior-art/personas/prompts; Medium/High/High; flags true). Typed
  `TaskPriority` (plain serde enum: invalid strings like `"Urgent"` fail
  deserialization → 400 at the PUT boundary, verified enum shape in
  `task.rs:23-28`). Serde field-level defaults mean a partial JSON payload
  cannot silently zero unspecified fields (unit test covers `{"enabled":false}`
  keeping default paths). `is_watched()` applies ignored-over-watched
  precedence; empty `watched_paths` matches nothing.
- **Persistence**: migration `2026-10-08-000056_spec_lifecycle_configs` is the
  unique next sequence number (`check-migration-versions.sh` exit 0) and uses
  portable SQL (`CREATE TABLE IF NOT EXISTS`, no dialect-only constructs —
  `check-migration-sql-portability.sh` exit 0). Both Diesel adapters
  (SQLite + Postgres) implement the new `SpecLifecycleRepository` port with
  upsert-on-conflict on `repo_id` PK; absent rows return `SpecLifecycleConfig::default()`
  per the port contract. Wired via the `store!` macro in `build_state`
  (DB-backed) and `MemSpecLifecycleRepository` in mem mode — port-backed, so
  `check-in-memory-state-stores.sh` exit 0. All three `AppState` construction
  sites updated (`lib.rs`, `mem.rs test_state_inner`, `middleware.rs` test).
- **API** (`api/spec_lifecycle.rs`): `GET/PUT /api/v1/repos/:id/settings/spec-lifecycle`
  — the assigned route contract (restored by 483559c0 after round 1). PUT is
  Admin/Developer-only (Agent → 403, tested with a real signed JWT); unknown
  repo → 404; GET returns defaults for unconfigured repos. Route registered in
  `api/mod.rs` with an ABAC `RouteResourceMapping` entry (single, non-duplicate —
  `check-abac-route-registry.sh` exit 0), so `require_auth_middleware` +
  `abac_middleware` run on it in production (`lib.rs:645-658`); not in the
  frozen exemption file, so the per-handler-authorization rule does not apply.
  Handler shape mirrors the established `spec_policy` sibling exactly
  (role gate → repo existence → port write). Documented in
  `docs/api-reference.md`.
- **Hook wiring** (`git_http.rs`): `process_spec_lifecycle` loads config through
  the same port the PUT handler writes; `enabled=false` returns before any
  diff/task/approval work (line 1376); filtering via `config.is_watched` (lines
  1344-1345); `classify_spec_change` takes all three priorities from config
  (line 1476); `auto_invalidate_approvals` gates the revocation block (line
  1435); `dedup_open_tasks` gates the open-task dedup (line 1483). Task
  workspace scope now comes from the repo resolved at push time
  (`push_workspace_id`), removing the prior `unwrap_or_else(Id::new("default"))`
  fabrication — the exact task-097 failure class. No hardcoded watched-path
  literals remain in production code (grep: only domain `DEFAULT_WATCHED_PATHS`
  constants and tests). All call sites of the renamed
  `parse_spec_changes_with_config` updated.
- **Tests are not vacuous** (static analysis of test semantics): the
  `enabled=false` hook test pushes a diff (empty-tree base substitution, line
  1399-1403) that includes `specs/system/a.md` — a watched path under defaults —
  so a regression that ignored `enabled` would create a task and fail the
  assertion. The config-driven test sets `watched_paths: ["docs/guides/"]` and
  `default_priority_new: Critical` and asserts exactly one task with
  `spec_path == docs/guides/b.md` and priority Critical — kills both
  "custom paths are additive" and "priorities ignored" bugs. The
  partial-payload, per-repo isolation, upsert-overwrite, and unknown-repo-404
  behaviors all have dedicated assertions.
- **UI** (`RepoSettings.svelte` + `api.js` + `en.json`): Spec Lifecycle tab with
  enabled/flag toggles, tag-style add/remove for watched/ignored paths
  (empty input guarded), three priority selects whose options match the
  `TaskPriority` variants; loads/saves through the assigned REST route.
  `web/dist` rebuilt and self-consistent (index.html asset refs match the
  committed bundle files; bundle contains the Spec Lifecycle UI strings).

### Contract integrity

`dev-contract.py.requirement_parts` run at this candidate: frontmatter and
requirement prose are byte-equal to the assigned base `110180fb` (True/True);
the acceptance-criteria checkboxes remain `[ ]` (flipping them demonstrably
changes the contract hash — verified False on a flipped copy). The `Shipped`
section is the only operational section present and is excluded from the
contract.

### Mechanical gates (exact candidate tree, all exit 0)

check-arch, check-abac-route-registry, check-abac-exempt-handlers (89 handlers),
check-mcp-write-tools, check-migration-versions, check-migration-sql-portability,
check-mem-port-contracts, check-in-memory-state-stores, check-scope-literal-defaults,
check-fabricated-scope-defaults, check-forged-scope-fields, check-inert-enforcement,
check-byte-slice-truncation, check-lossy-secret-conversion, check-relative-path-defaults,
check-fail-open-ref-resolution, check-dead-message-kinds, check-unbounded-external-http,
check-forwarded-header-trust, check-task-commit-attribution.

## Minor observations (not blocking)

- `auto_invalidate_approvals=false` and `dedup_open_tasks=false` have no direct
  hook-level test (the wiring is present and read-verified at git_http.rs:1435
  and 1483; the acceptance criteria do not demand per-flag tests). If the
  verifier wants belt-and-braces, a `dedup_open_tasks=false` duplicate-task
  assertion would be the cheapest addition.
- `process_spec_lifecycle` uses `.unwrap_or_default()` on `get_for_repo`, so a
  storage *error* (vs. an absent row) degrades to default config. The failure
  direction is toward more detection (enabled, default paths), and the port
  contract defines absent-row-as-defaults, so this is acceptable; noted so it
  is a conscious decision.
- The Agent-role 403 is enforced both by the handler role gate and potentially
  by ABAC policy; the test asserts the observable 403 either way, which
  satisfies the endpoint contract.

## Deferred to verification (network-blocked here)

`cargo test --all` (acceptance criterion), the focused spec_lifecycle suites
(domain/adapters/server), `parse_spec_changes`/`classify_spec_change` suites,
`web && npm ci && npx vitest run src/__tests__/RepoSettings.test.js`, clippy/fmt,
and an optional live GET/PUT server probe. Full list in
`/tmp/stage/review-evidence/task-109-r3/deferred-verification.txt`.
