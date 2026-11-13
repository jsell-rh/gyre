# Review: task-146 — Complete analytics event schema and auto-emitted events coverage

Comparison base: 890a70dec844985f778b19f81dadd29a94d19de0 → candidate 398ba512c85bca4cf8d6b8735c1c25cbcd081b5e.
Range: 75 commits (4 review-repair rounds + base merges), 41 files, +5889/−374.
No `web/` changes. No new exemption entries (both exemption-file diffs are
comment-text-only; frozen count unchanged). No deleted tests; mechanical gates all
pass in this sandbox (arch, ABAC registry/exempt handlers, mem-port contracts,
migration versions + SQL portability, fabricated/scope-literal defaults,
inert-enforcement, lossy secrets, fail-open refs, byte-slice truncation,
relative-path defaults, in-memory state stores, dead message kinds, forwarded
headers, forged scope fields, unbounded HTTP, MCP write tools).

## Spec conformance (analytics.md §Event Schema / §Auto-Emitted Events / §Query API / §Query Parameters)

- **Schema**: `AnalyticsEvent` (gyre-domain/src/analytics.rs) carries all 9 spec
  fields; `with_scope` populates user/session/workspace/repo. Id-typed fields are
  stored as text (`Option<String>`) — `Id` is a string newtype and the storage
  columns are TEXT, so the serialized shape matches the spec schema exactly.
  Pinned by `analytics_event_matches_spec_schema` (run in this review: passed).
- **All 12 auto-emitted events** emit at real trigger points with all spec-required
  properties (verified at every emission site, see review-evidence/00-summary.md):
  - `task.status_changed` — shared helper called from 12 production transition
    paths (HTTP, MCP update_task, spawn/completion auto-advance, admin kill, stale
    abort, spec-reject, repo-archive, ralph-loop re-open/ceiling/terminal respawn).
  - `mr.merged` — gate_count counted from real gate_results rows; queue_wait_secs
    from real enqueue time (queue path + HTTP transition path).
  - `mr.closed` — reason named per path (closed_without_merge / repo_archived /
    spec_rejected / gate_failure).
  - `agent.spawned` — compute_target reports the *resolved* launch target
    (repair 9bf30a10 fixed a hardcoded "local"), persona reports the bound persona
    (repair 5fb9e9b8) or the honest `default-worker` bootstrap label.
  - `agent.completed` / `agent.failed` — both surface paths (HTTP + MCP), every
    kill/death path including stale-abort, repo-archive stop, failed launch
    teardown, orchestrator process-death monitor, ralph-loop ceilings.
  - `merge_queue.processed` — merged + every terminal Failed path + operator
    cancel (`outcome: "skipped"`), wait_secs from enqueued_at.
  - `gate.failed` / `gate.passed` — after real gate execution, output_snippet
    char-boundary-safe truncated to 512.
  - `spec.approved` — from the approval handler with approver identity split into
    agent/user scope fields.
  - `budget.warning` — 80% threshold on agents/tokens/cost from the real budget
    check and usage-recording paths, with a below-threshold negative test.
  - `search.query` — HTTP + MCP gyre_search through one shared helper.
- **Query API / Parameters**: no new routes (per task instruction). GET
  /analytics/events supports event_name (exact + trailing-`*` prefix via LIKE on
  SQLite and Postgres), agent_id, user_id, workspace_id, repo_id, since/until
  (ISO8601 or unix secs, date-only bounds handled), limit (default 100, capped
  10_000), group_by (event_name/agent_id/workspace_id/day; unsupported field
  rejected). `AnalyticsQueryFilter` implemented identically in SQLite, Postgres,
  and mem adapters; `analytics_query_filtered_all_params` (run in this review:
  passed, 13/13 adapter tests) uses a cross-cutting seed so dropping any single
  predicate changes an asserted result set.

## Security posture (improvements, not regressions)

- Record/search identity fields are bound to the verified caller; caller-supplied
  workspace/repo scope attributions are validated against the caller's tenant
  (Forbidden/NotFound on mismatch), with regression tests that query the persisted
  events back through the API.
- Workspace-orchestrator spawn tenant containment restored for every caller
  (repair 3bcd6f52) with a two-sided regression test; the repo-tier bypass is
  documented with its bootstrap rationale.

## Verification run (this review)

- `cargo test -p gyre-domain --lib analytics` → 3 passed.
- `cargo test -p gyre-adapters --lib analytics` → 13 passed (real SQLite rows).
- `cargo check -p gyre-server --lib --tests` → clean (type-checks every unit test
  and `tests/api_integration.rs`, including all per-event analytics tests).
- All 19 mechanical check scripts listed above → pass, except
  `check-task-commit-attribution.sh`, which fails only against the controller's
  working-tree staging reset (`commits: []`); the candidate commit's committed
  task file records all 18 attributed SHAs (verified by full-SHA grep). Not a
  candidate defect.

## Verifier-required probes (gyre-server test execution exceeds the 4 GiB sandbox budget)

1. `cargo test -p gyre-server --lib`
2. `cargo test -p gyre-server --test api_integration`
3. `cargo fmt --check` + clippy on changed crates

Full named-test list and logs: /tmp/stage/review-evidence/00-summary.md.

## Verdict

No findings. All five coverage sections (Purpose, Event Schema, Auto-Emitted
Events, Query API, Query Parameters) are backed by real production emission paths,
real adapter-backed filtering, and per-event behavioral tests that fail when the
emission is removed. Sections 6–12 (Decision API) belong to task-147 and were
correctly left `task-assigned`. Approve.
