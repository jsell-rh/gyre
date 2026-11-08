# Review — task-109 (Externalize spec lifecycle configuration)

Spec: `specs/system/spec-lifecycle.md` §Configuration. Assigned route contract
`GET/PUT /api/v1/repos/:id/settings/spec-lifecycle`.
Candidate under review: `ec6568e7d76c0e556df6713b999c005c79c42f98` (base `72ad2a128`,
merge-base verified equal to the base). All 12 frontmatter commits exist on the branch.
Verdict: **approved**.

## Round 1 — probes

All evidence under `/tmp/stage/review-evidence/task-109/` (commands in `COMMANDS.txt`).

- `SKIP_WEB_BUILD=1 cargo test -p gyre-domain --lib spec_lifecycle` → **5/5** (defaults match
  spec exactly, watched filtering, ignored-over-watched precedence, partial-JSON serde
  defaults, string priorities).
- `SKIP_WEB_BUILD=1 cargo test -p gyre-adapters --lib spec_lifecycle` → **4/4** against a
  real temp-file SQLite DB. `SqliteStorage::new` runs pending migrations, so these prove
  migration `2026-10-08-000058_spec_lifecycle_configs` creates the table and that
  round-trip / upsert-overwrite / per-repo isolation work on real storage.
- `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib spec_lifecycle` → **7/7**: 5 API tests
  (defaults for unconfigured repo, PUT round-trip through the same port the hook reads,
  partial-body serde defaults, unknown repo 404, Agent-role 403 with a real RS256 JWT on
  the assigned path) + 2 end-to-end hook tests against a real bare git repo with real
  pushed commits (`enabled=false` creates no tasks — the fixture's default-watched
  `specs/system/` file would otherwise produce one, so the test kills the bug; custom
  `watched_paths: ["docs/guides/"]` fully replaces defaults so the `specs/system/` file in
  the same diff produces no task, and `default_priority_new: Critical` lands on the created
  task).
- `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib 'git_http::tests::parse_spec_changes'`
  → **9/9**; `'git_http::tests::classify_spec_change'` → **6/6**; full
  `'git_http::tests'` → **43/43** (no regressions in the shared hook module).
- `cd web && npm ci && npx vitest run src/__tests__/RepoSettings.test.js` → **56/56**
  (tab renders, load via `api.repoSpecLifecycle`, field rendering, add/remove path tags,
  toggles, priority selects, save via `api.setRepoSpecLifecycle` with edited payload).
- Static gates all exit 0: `check-migration-versions`, `check-migration-sql-portability`,
  `check-abac-route-registry`, `check-mem-port-contracts`, `check-arch`,
  `check-in-memory-state-stores`, `check-scope-literal-defaults`,
  `check-fabricated-scope-defaults`, `check-dead-message-kinds`,
  `check-task-commit-attribution`.
- Package-scoped `cargo fmt -p {gyre-domain,gyre-ports,gyre-adapters,gyre-server} --check`
  → clean. Workspace-wide `cargo fmt --all --check` and `check-clippy-diff.py` are rejected
  by the executor's workspace-wide wrapper in this sandbox — deferred to verification
  (transport restriction, not a code defect).

## Verified working (no findings)

- **Domain**: `SpecLifecycleConfig` with all eight spec fields; priorities typed
  `TaskPriority` (invalid values rejected at deserialization, i.e. a bad PUT payload is a
  4xx, not a silently stored junk value); serde field-level defaults so partial payloads
  can't zero unspecified fields; `is_watched()` applies ignored-over-watched precedence.
  Defaults match the prior hardcoded behavior exactly (watched
  `specs/system/`,`specs/development/`; ignored milestones/prior-art/personas/prompts;
  Medium/High/High; all flags true).
- **Persistence**: `spec_lifecycle_configs` table at the next unused sequence 000058 (56/57
  are taken by the base's `spec_assertion_results` / `spec_approval_attestation` — the
  renumber history checks out), portable SQL (plain `CREATE TABLE IF NOT EXISTS`, TEXT
  columns), down.sql present, schema.rs matches. SQLite and Postgres adapters both
  implement `SpecLifecycleRepository` with upsert-on-conflict; absent rows mean defaults
  (never an error, so the hook degrades to prior behavior). Wired through `store!` in
  `AppState`: DB-backed deployments get the Diesel adapter, mem mode gets
  `MemSpecLifecycleRepository` (port-backed, per the durable-state invariant).
- **API**: route registered in `api/mod.rs` with an ABAC `RouteResourceMapping` entry;
  PUT enforces Admin/Developer in the handler **before** any mutation (Agent 403 proven
  with a real signed JWT — an agent cannot disable the drift detector it is subject to);
  unknown repo → 404; GET matches the repo-first-then-config shape used by sibling routes.
- **Hook wiring**: the `SPEC_WATCHED_PATHS` constant is gone (grep: no production path
  literals remain outside the domain defaults, which is where they belong);
  `process_spec_lifecycle` loads per-repo config through the same port the PUT writes;
  `enabled=false` returns before any diff/task/approval work; filtering via
  `config.is_watched` on both old and new rename paths; `classify_spec_change` takes all
  three priorities from config (R→new, matching the pre-change Medium mapping); the
  `auto_invalidate_approvals` guard wraps the entire approval-revocation block and the
  `dedup_open_tasks` guard wraps the entire dedup gate (read in full, not just the diff
  hunks). Workspace scope for created tasks comes from the repo resolved at push time
  (`resolved.workspace_id`, git_http.rs:267) — the prior failure-prone re-lookup with its
  fabricated-identity risk class is gone, and `check-fabricated-scope-defaults` passes.
- **UI**: full Spec Lifecycle tab (enabled toggle, tag-style add/remove for watched and
  ignored paths, both flag toggles, three priority selects, save with toast feedback);
  `api.js` endpoints; `en.json` strings; vitest covers load/render/edit/save.

## Non-blocking observations (recorded for the verifier, no revision required)

1. **Shipped-notes evidence attribution is inaccurate on one sentence.** The task file's
   test-evidence section claims the gyre-server API tests "construct a fresh SQLite-backed
   server, so GET/PUT now exercise the 000058 migration end-to-end". They do not:
   `mem::test_state()` and `make_test_state_with_jwt()` are in-memory. The migration *is*
   exercised end-to-end by the gyre-adapters suite (`SqliteStorage::new` runs pending
   migrations on a real temp-file DB), which the notes also cite correctly and which this
   review re-ran independently. No acceptance criterion depends on the inaccurate
   sentence; the behavior it claims is proven by a different, verifiable probe.
2. **The `auto_invalidate_approvals=false` and `dedup_open_tasks=false` hook effects have
   no dedicated hook-level test** (only persistence round-trip through the API). Both
   guards were verified by direct code reading to wrap their complete blocks, and neither
   flag is an acceptance-criteria checkbox; flagging as a coverage observation only.

`cargo test --all`, the full web suite, exact-head GitHub checks, and workspace-wide
fmt/clippy are owned by the verification stage per assignment instructions.
