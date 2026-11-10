# Review: task-109 — Externalize spec lifecycle configuration

- **Candidate:** `4a665909259cdb1a2b6eac8b34e4270897bbab12`
- **Base:** `402142b1c8d214b03977d689742baae6a1af7910`
- **Spec:** `specs/system/spec-lifecycle.md` §Configuration
- **Verdict:** **Approved** — independent evidence supports the task contract.

## What was reviewed

Diff `402142b1..4a6659092` (25 files: domain struct, port, SQLite/Postgres
adapters, migration 000059, API handlers + route/ABAC registration,
`git_http.rs` hook wiring, RepoSettings UI + locale strings, coverage row,
task record). The candidate head is the merge of assignment base 402142b1
(task-106/116/141) plus a process commit recording the rebase repair; the
one source conflict (diff-parser region) was resolved as a correct union:
base's unfiltered `parse_name_status` kept for `parse_persona_changes`,
config-driven `parse_spec_changes_with_config` for the lifecycle hook,
hardcoded `SPEC_WATCHED_PATHS` const deleted.

## Independent evidence (all at candidate head, this sandbox, cold build)

Focused Rust probes via `/tmp/stage/bin/cargo` — logs under
`/tmp/stage/review-evidence/task-109/`:

- `cargo test -p gyre-domain --lib spec_lifecycle` — **5/5** (defaults match
  spec exactly, ignored-over-watched precedence, partial-JSON serde defaults,
  string priorities).
- `cargo test -p gyre-adapters --lib spec_lifecycle` — **4/4** against a real
  fresh SQLite DB (unconfigured→defaults, round-trip, upsert-overwrite,
  per-repo isolation).
- `cargo test -p gyre-server --lib spec_lifecycle` — **8/8**, including the
  SQLite-migration-traverse API test (real `SqliteStorage`, migration-created
  table), the JWT-backed Agent-role 403 test through the real auth/ABAC
  middleware, and two end-to-end hook tests against a real bare git repo.
- `cargo test -p gyre-server --lib parse_` — **59/59**; `push_modifying_spec_revokes_ledger_approvals` — ok.
- **Mutation probe:** inverted the `if !config.enabled` guard in
  `process_spec_lifecycle` → `spec_lifecycle_disabled_skips_task_creation`
  **FAILED** (exit 101), then restored and re-verified 8/8. The test is
  load-bearing, not self-confirming.
- Frontend: `npm ci` + `npx vitest run RepoSettings.test.js` — **57/57**,
  verbose run confirms the 4 Spec Lifecycle tab tests (panel, API load,
  field rendering, save with add/remove paths).
- Mechanical gates exit 0: arch, migration versions, migration SQL
  portability, ABAC route registry, ABAC exempt handlers, in-memory state
  stores, mem port contracts, forged scope fields, inert enforcement,
  unbounded external HTTP, task commit attribution. Per-package
  `cargo fmt --check` clean (domain/ports/adapters/server). `en.json` parses.

## Findings

None blocking. Two non-blocking observations recorded for the verifier:

1. The `false` branches of `auto_invalidate_approvals` and
   `dedup_open_tasks` at the hook level are covered only by inspection and
   persistence round-trip, not a dedicated end-to-end test (their `true`
   branches are covered: `push_modifying_spec_revokes_ledger_approvals`,
   dedup path). The guards are one-line `if` wrappers around the correct
   blocks; not fake implementation.
2. `get_for_repo(...).unwrap_or_default()` in the hook degrades an infra
   error to config defaults (= prior hardcoded behavior), matching the
   port's documented "degrades to default behavior" contract. Acceptable.

## Integrity

After the mutation probe, the working tree was restored and verified
byte-identical to the pre-review snapshot (1555/1555 files, including
regenerated `web/dist` build artifacts reverted via git).
