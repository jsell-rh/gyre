# Review — task-146 (Complete analytics event schema and auto-emitted events coverage)

Spec: `specs/system/analytics.md` §Event Schema, §Auto-Emitted Events, §Query API, §Query Parameters (plus §Purpose as context).
Candidate: `2c8b6558166beff10ff31e083b0b0e09c78c6995` (branch `pipeline/task-146/7c6fd2e3ec4d42b4abfa1aa7b069176b-1`), base `fefde2946`. Frontmatter commits `8f0ee1903` and `ed7ac6d36` verified as ancestors of the candidate.
Verdict: **approved** (independent review round; findings: none blocking).

## What was reviewed

The diff from base spans the analytics surface (gyre-domain struct, gyre-ports
`AnalyticsQueryFilter`/`query_filtered`, SQLite/Postgres/mem adapters,
migration `2026-10-08-000059_analytics_scope_columns`) plus emission wiring
across fifteen server files, and — from earlier repair rounds merged forward —
real fixes to pre-existing debt (orchestrator spawn tenant containment,
gate_executor "default" ws_id fabrication, void `validate_persona` renamed
`warn_unapproved_persona`, attestation-ABAC enforcement on merge). Exemption
files only shrank (entries removed, `FROZEN_EXEMPTION_COUNT` lowered 3→1) —
no grandfathering.

## Independent evidence

All commands and outputs retained under `/tmp/stage/review-evidence/`.

- **Struct**: `AnalyticsEvent` carries all 9 spec fields; `with_scope`
  populates user/session/workspace/repo. Id-typed fields are `Option<String>`
  (Id is a String newtype; identical serde wire format) — documented in the
  coverage row, not a spec contradiction.
- **All 12 auto-emitted events** have production emission sites with all
  spec-required properties (verified in source, per-event list in
  notes.md): `task.status_changed` (HTTP + MCP + auto-advance + admin kill +
  stale-abort + spec-reject + repo-archive), `mr.merged` (HTTP transition +
  queue merge, `gate_count`/`queue_wait_secs`), `mr.closed`,
  `agent.spawned` (`compute_target`/`persona`), `agent.completed` (HTTP +
  MCP), `agent.failed` (fail/kill/admin/stale-abort/spec-reject/repo-archive/
  compose-teardown), `merge_queue.processed` (merged + every terminal Failed
  path via `fail_queue_entry` + circuit-breaker and operator-cancel
  `skipped`), `gate.failed`/`gate.passed` (gate_executor `run_gate`),
  `spec.approved`, `budget.warning` (80% threshold, spawn check + usage
  recording), `search.query`.
- **Query API**: full spec filter set including `group_by`
  (event_name/agent_id/workspace_id/day, unsupported → 400), trailing-`*`
  prefix match, ISO8601-or-unix `since`/`until`, limit default 100/cap
  10_000; implemented with Diesel bound parameters on SQLite and Postgres and
  in mem.
- **Mutation probes** (test-strength proof, each restored afterwards, tree
  clean): renaming the `search.query` event, renaming `gate.passed`, and
  mutating the cancel-entry `outcome` value each made the corresponding
  per-event test FAIL — the tests assert real emissions with specific
  property values, not self-confirming mirrors.
- **Focused suites**: `--lib analytics` 42/0, `merge_processor::tests` 71/0,
  `api::analytics` 20/0, `"emits"` 24/0, `"records"` 18/0, adapters
  `analytics` 13/0, `--test api_integration` 67/0, and each of the 10 named
  task-146 regression tests individually green.
- **Invariant gates**: all 18 branch `check-*.sh` scripts exit 0 (incl.
  task-commit-attribution, migration versions/portability,
  fabricated-scope, abac registry/exempt handlers, mcp-write-tools, arch).
  Migration SQL is dialect-portable; `web/dist` byte-identical to base.
- **Coverage matrix**: the 5 assigned sections are `implemented` with
  accurate per-section notes; the 7 remaining analytics sections stay
  `task-assigned` to task-147 — correct scoping, no side-effect closure.

## Minor observations (recorded, not blocking)

- `Option<String>` vs the spec schema's `Option<Id>` for scope fields —
  same wire representation, documented; acceptable.
- `event_name` prefix LIKE does not escape `%`/`_` — benign (filter-only
  surface; the caller can query unfiltered).
- Stale-agent Pause stop emits no `agent.failed` — documented design reading
  (pause is a configured graceful stop, not "fails or is killed");
  consistent with the spec trigger wording.
