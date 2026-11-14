# Review — task-146 (Complete analytics event schema and auto-emitted events coverage)

Spec: `specs/system/analytics.md` §Event Schema, §Auto-Emitted Events, §Query API, §Query Parameters, §Purpose.
Candidate: `2a9dedaa` (base `b6892403`); all 28 commits listed in the task frontmatter attributed.
Verdict: **approved** — no findings.

## Round (verification-repair confirmation)

This round's candidate adds only the test-setup repair `95177f7b` (5 lines,
`stale_agents.rs` pause test: the agent is assigned to the ws-2 workspace the
test creates and asserts, since `record_agent_failed` scopes the event to
`agent.workspace_id`; the previous run compared a bootstrap-"default" scope
against ws-2 and could never pass) plus two bookkeeping commits touching only
`specs/tasks/task-146.md`. Production code is byte-identical to the tree the
independent verifier executed in run ad0c26e5 (2109 passed / 1 failed → the
test repaired here).

Probes run (evidence in `/tmp/stage/review-evidence/task-146-review.md` and
alongside files):

- All 19 repository static invariant checks → exit 0 (architecture, ABAC
  registry, MCP write gates, migration versions + SQL portability, dead
  message kinds, byte-slice truncation, relative path defaults, fail-open ref
  resolution, task commit attribution, mem port contracts, fabricated scope
  defaults, lossy secret conversion, scope literal defaults, inert
  enforcement, forged scope fields, forwarded header trust, in-memory state
  stores, unbounded external HTTP).
- `/tmp/stage/bin/cargo test -p gyre-domain` → 508+8 passed, 0 failed,
  including `analytics_event_matches_spec_schema` (all 9 spec fields,
  `with_scope`, serde round-trip).
- `/tmp/stage/bin/cargo test -p gyre-adapters analytics` → 16 passed, 0
  failed against real SQLite, including
  `analytics_query_filtered_all_params` (each filter dimension is the only
  discriminator for one seeded event, so dropping any predicate fails it),
  `analytics_event_name_prefix_is_literal_case_sensitive`
  (`MR.*`/`mr.m_rged*` match nothing), and
  `analytics_count_grouped_over_full_filtered_set` (grouped totals count the
  full filtered set, not the limit-truncated page).
- `/tmp/stage/bin/cargo check -p gyre-server --lib` → clean.

Verified working (no findings):

- **Event Schema**: `AnalyticsEvent` (gyre-domain/src/analytics.rs) carries
  all 9 spec fields; `with_scope` populates user/session/workspace/repo.
  Id-typed fields stored as `Option<String>` (Id is a transparent String
  newtype; documented in the coverage matrix) — behaviorally equivalent,
  filtering unaffected.
- **All 12 auto-emitted events** fire at real trigger points with all
  spec-required properties (per-event file:line evidence in the review-evidence
  note). Multi-path coverage is real, not single-path: `task.status_changed`
  has 20 production call sites; `merge_queue.processed` covers every terminal
  queue path including circuit-breaker "skipped"; `gate.failed`/`gate.passed`
  cover both the pre-merge `run_gate` and post-merge `run_post_merge_gates`
  loops; `mr.closed` covers HTTP transition, repo archive, spec reject, and
  ralph-loop gate-failure close; `agent.completed` covers both the HTTP
  endpoint and the MCP `gyre_agent_complete` path; `agent.failed` covers
  fail/stop endpoints, budget kill, stale abort/pause, ralph-loop ceilings,
  and the orchestrator process-death monitor; `search.query` covers both the
  HTTP search endpoint and the MCP `gyre_search` tool.
- **No hardcoded stand-ins**: `gate_count` is a real `gate_results` count,
  `queue_wait_secs` derived from queue entry `enqueued_at`,
  `compute_target` reports the label of the target the process actually
  launched on (orchestrator path repaired in an earlier round), `persona`
  reports the bound persona or the honest `default-worker` bootstrap label,
  durations derive from persisted spawn times. `budget.warning`'s 80%
  threshold is the platform-documented Warn constant (BudgetConfig).
- **Query Parameters**: event_name exact/trailing-`*` literal prefix
  (case-sensitive on all three adapters — mem `starts_with`, SQLite
  `substr(...)=` under BINARY collation, Postgres escaped case-sensitive
  LIKE), agent/user/workspace/repo scope, since/until (ISO8601 or unix
  secs), limit (default 100, max 10_000 per spec), group_by
  (event_name/agent_id/workspace_id/day) aggregated server-side over the
  full filtered set. `/daily` supports the spec's `?event_name=&days=30`
  form.
- **Anti-forgery**: `record_event` ignores request-body agent_id/user_id
  (binds to the verified caller, impersonation attributes to the
  impersonator) and validates workspace/repo attribution against the
  caller's tenant; `search.query`'s workspace scope is validated before it
  becomes event attribution. Pinned by dedicated regression tests.
- **Migration**: `2026-10-08-000063_analytics_scope_columns` uses the next
  unused sequence number and portable SQL (both check scripts pass).
- **Coverage matrix honesty**: exactly the five task sections marked
  implemented with landed evidence; Decision API sections (task-147) left
  task-assigned.

Test-inflation check: none found. The filter-parity test fails if any
predicate is dropped; the prefix-literalness tests fail on LIKE-style
case-insensitive or wildcard matching; the grouped-count test fails if
`limit` truncates the aggregation input; the scope-validation tests fail if
caller-forged attribution is persisted; the schema test fails if any spec
field is removed.

Server tests the verifier must execute (4 GiB budget — not run in this
sandbox): `cargo test --all` at the candidate; focused:
`cargo test -p gyre-server --lib stale_agents`, `--lib api::analytics`,
`post_merge_gates_run_against_head_sha_tree`, `gate_executor`,
`orchestrator`, `ralph_loop`, `mcp`.
