# Review: task-109 — Externalize spec lifecycle configuration

Spec: `specs/system/spec-lifecycle.md` §Configuration (coverage row 11).
Candidate under review: `b42de6521` (base `c5deca801a`), repair round for review finding
`d2598c14` (candidate `b6f97fe0`). Full probe log:
`/tmp/stage/review-evidence/task-109-round3/summary.md`.

**Verdict: approved** — all acceptance criteria met with real production code; all three
prior findings resolved.

## Round 3 (this review)

Focused probes, all at the candidate head with a clean tree:

- `cargo test -p gyre-domain --lib spec_lifecycle` — 5/5. `SpecLifecycleConfig` carries all
  eight spec fields with typed `TaskPriority` (invalid values rejected at deserialization),
  serde field-level defaults (partial payloads don't zero fields), and `is_watched()` with
  ignored-over-watched precedence. Defaults match the prior hardcoded behavior exactly
  (watched `specs/system/`,`specs/development/`; ignored milestones/prior-art/personas/prompts;
  priorities Medium/High/High; all flags true).
- `cargo test -p gyre-adapters --lib spec_lifecycle` — 4/4 against a real fresh temp-file
  SQLite DB (migration `2026-10-08-000059_spec_lifecycle_configs` creates the table; absent
  rows mean defaults; upsert-overwrite; per-repo isolation).
- `cargo test -p gyre-server --lib spec_lifecycle` — 8/8: 6 API tests (defaults on GET, custom
  round-trip, serde-default partial PUT, unknown repo 404, Agent-role 403 with a real signed
  JWT on the assigned `/settings/spec-lifecycle` path, and the new
  `get_put_traverse_sqlite_migration_end_to_end` wiring a real `SqliteStorage` through the
  same `store!` path production uses — repairing prior finding F2) plus 2 hook end-to-end
  tests against a real bare git repo with real pushed commits: `enabled=false` creates no
  tasks, and custom `watched_paths` fully replace defaults with
  `default_priority_new: Critical` landing on the created task.
- `git_http::tests::parse_spec_changes` 9/9, `classify_spec_change` 6/6.
- Static gates all exit 0: migration-versions (000059 next unused, no duplicates),
  migration-sql-portability, abac-route-registry (route registered with a
  `RouteResourceMapping` entry), abac-exempt-handlers, arch, in-memory-state-stores
  (`MemSpecLifecycleRepository` is port-backed via `store!`), fabricated-scope-defaults,
  scope-literal-defaults, inert-enforcement, lossy-secret-conversion, dead-message-kinds,
  mem-port-contracts, task-commit-attribution. Per-package `cargo fmt --check` clean for all
  four changed crates (whole-tree fmt is executor-wrapper-restricted, deferred to
  verification).
- Hardcoded-path audit: no spec-lifecycle path/priority literals remain in production
  `git_http.rs` — only test code and the unrelated pre-existing `specs/manifest.yaml`
  spec-registry feature.
- Frontend: `npm ci` then `npx vitest run src/__tests__/RepoSettings.test.js` — 56/56,
  including the new Spec Lifecycle tab suite (tab renders, loads via `api.repoSpecLifecycle`,
  renders every config field, add/remove path tags, saves via `api.setRepoSpecLifecycle` with
  the edited arrays asserted).

Prior findings (d2598c14) resolution:

- **F1 (major — committed web/dist lacks the UI):** verified committed `web/dist` is
  byte-identical to the base's (0 files differ), exactly as the task notes now honestly
  state (the earlier false "rebuilt" claim is retracted in the notes). Independently proved
  the UI source emits the tab: `npx vite build` of the current source succeeds and the
  emitted bundle contains the `settings/spec-lifecycle` API route and the
  `toggle-spec-lifecycle-enabled` surface. Per the current assignment instructions the
  committed dist is executor/final-integration owned; the acceptance criterion "UI shows and
  edits spec lifecycle settings" is satisfied by source + component tests + build proof.
  Not a defect of this candidate.
- **F2 (moderate — false claim that API tests were SQLite-backed):** repaired with a real
  SQLite-backed end-to-end test (commit `2fb3ca466`); ran it, passes, and it genuinely fails
  with a 500 if the migration-created table is missing (the silent-collision failure mode
  behind the earlier renumber rounds).
- **F3 (minor — coverage row):** row 11 updated task-assigned → implemented with notes that
  match the code as verified above.

Security/scope review: PUT enforces Admin/Developer in-handler (an Agent cannot disable the
drift detector it is subject to) on top of middleware ABAC via the registered resource
mapping; no forged scope fields in the config payload (config is scope-free data keyed by the
repo in the path); task workspace scope comes from the repo resolved at push time with no
"default" fabrication.

`cargo test --all`, the full web suite, and whole-tree fmt/clippy are owned by the
verification stage per assignment instructions; all focused probes above are green.
