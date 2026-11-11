---
title: "task-146 review — analytics event schema and auto-emitted events"
task: task-146
candidate: 0006649fac2c1d51d824323e25ed185118c81144
base: cba416b46a73d3568f745d7b4eae49a0700a90a6
verdict: needs-revision
reviewed: 2026-11-14
---

# Review: task-146 (analytics.md §Event Schema / §Auto-Emitted Events / §Query API / §Query Parameters)

## What was verified

Focused probes (evidence in pipeline artifacts, `/tmp/stage/review-evidence`
during review): `cargo test -p gyre-domain analytics` (3 passed, incl. the
spec-schema round-trip test), `cargo test -p gyre-adapters --lib analytics`
(13 passed, incl. `analytics_query_filtered_all_params` with cross-cutting
seed data), `cargo check -p gyre-server` clean, and all relevant shell gates
(arch, migration-versions, migration-portability, mem-port-contracts,
fabricated-scope-defaults, scope-literal-defaults, forged-scope-fields,
inert-enforcement, byte-slice-truncation, forwarded-header-trust,
lossy-secret-conversion, dead-message-kinds) exit 0.

Functional verification by source inspection of every emission site:

- **Event Schema** — all 9 spec fields on `AnalyticsEvent`; `with_scope`
  populates user/session/workspace/repo; Id-typed fields carried as
  `Option<String>` matching the storage layer's TEXT columns (pre-existing
  representation). ✔
- **Auto-Emitted Events** — all 12 events emit at real trigger points with
  all spec-required properties, including multi-path coverage the task plan
  did not explicitly demand: `merge_queue.processed` on every terminal queue
  path (gate failure, missing MR/repo, spec/breaking-change/spec-link/
  constraint/attestation-ABAC blocks, git errors, circuit-breaker "skipped"),
  `task.status_changed` on HTTP + MCP + internal auto-advance/kill/abort/
  reject/archive paths, `agent.failed` on fail/kill/stale-abort/stop paths,
  `mr.merged` on both the queue and HTTP transition paths, `agent.spawned`
  on worker/orchestrator/ralph-loop re-spawn paths. ✔
- **Query API / Query Parameters** — full spec filter set (event_name exact
  + trailing-`*` prefix, agent_id, user_id, workspace_id, repo_id,
  since/until ISO8601-or-unix inclusive bounds, limit default 100 cap
  10_000, group_by event_name/agent_id/workspace_id/day with unsupported
  fields rejected) implemented on sqlite, postgres, and mem adapters with
  cross-adapter semantics tests; `daily` supports the spec's `?days=` form. ✔
- **Tests** — per-event tests trigger real production paths (HTTP router,
  `run_gate`, `check_spawn_budget`, queue processing) and assert persisted
  events and properties, with meaningful negatives (no event below the
  budget threshold, failed gate emits no `gate.passed`, empty search query
  emits nothing, cross-tenant orchestrator spawn 403 with positive control).
  Not self-confirming. ✔
- **Migration** — scope columns added as 000061 (the branch caught and fixed
  a duplicate-000060 collision induced by the rebase); SQL is portable. ✔

## F1 (blocking) — record_event persists caller-supplied scope fields unvalidated

`POST /api/v1/analytics/events` (`record_event`,
crates/gyre-server/src/api/analytics.rs) was extended by this diff to accept
`user_id`, `session_id`, `workspace_id`, `repo_id` in the request body and
persist them verbatim (`.with_scope(req.user_id…, req.session_id,
req.workspace_id…, req.repo_id…)`) with no derivation from the authenticated
caller and no validation against the caller's scope. The route is
authenticated (`require_auth_middleware` layers every /api/v1/ route), so the
middleware knows the caller — the handler binds nothing server-side.

Any authenticated caller permitted to POST analytics events can attribute
events to an arbitrary user/workspace/repo, and those fields are exactly what
the new query filters select on: forged events appear inside another
workspace's `GET /analytics/events?workspace_id=…` result set and feed the
spec's agent-consumable decision loop. This is the standing repo constraint
("never persist caller-supplied provenance or scoping fields unvalidated in
an authenticated handler; derive from the caller or validate against the
caller's scope") and the same class repaired for audit events in task-102 F2
(`validate_workspace_scope`/`validate_repo_scope` in api/audit.rs — directly
reusable here). `scripts/check-forged-scope-fields.sh` passes only because
its heuristic requires the handler to extract `AuthenticatedAgent`; this
handler extracts no auth at all, which is a checker blind spot, not
compliance.

Repro: authenticate as any caller with analytics access; POST
`{"event_name":"feature.used","workspace_id":"<victim-ws>","user_id":"<victim-user>","properties":{}}`
→ 201; then `GET /api/v1/analytics/events?workspace_id=<victim-ws>` returns
the forged event inside the victim scope.

Fix: extract the authenticated identity in the handler, derive
agent_id/user_id from it, and validate workspace_id/repo_id against the
caller's scope (audit.rs pattern) — or drop the scope fields from the request
body. A regression test should assert that a cross-scope attribution attempt
is rejected (403) or re-scoped to the caller.

## Non-blocking observations

- Read-side tenant blindness of the analytics query endpoints (any
  authenticated caller can query any scope, or all events unfiltered) is
  pre-existing at the base and outside this task's assigned sections; worth
  a follow-up task.
- `group_by` aggregates over the limit-truncated result set (default 100
  events); acceptable reading of the spec, but callers aggregating high-
  volume events must pass an explicit large `limit`.
- Exemption-file / check-script edits in the diff are comment-only; frozen
  counts unchanged, no new entries. Fine.

## Verifier-owned checks (server test compilation deferred for the 4 GiB budget)

- `cargo test -p gyre-server --lib -- analytics`
- `cargo test -p gyre-server --lib ralph_loop`
- `cargo test -p gyre-server --lib spawn`
- `cargo test -p gyre-server --lib search`
- `cargo test -p gyre-server --test api_integration`
- Full workspace suite, all-target clippy, fmt, CI.

## Verdict

needs-revision — the functional contract (schema, 12 events, query filters,
tests, migration) is genuinely implemented and verified, but F1 must be
repaired before approval.
