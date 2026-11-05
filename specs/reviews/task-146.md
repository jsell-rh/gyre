# Review — task-146 (Complete analytics event schema and auto-emitted events coverage)

Spec: `specs/system/analytics.md` §Event Schema / §Auto-Emitted Events / §Query API / §Query Parameters.
Candidate under review: `f57a199e` (base `0cb71015`, branch `pipeline/task-146/56cbc391d74c4685bbbe47a48126a074-1`).
This is the third review round; both prior rounds' findings are verified fixed below.
Verdict: **approved** (`/tmp/stage/verdict.json`, evidence under `/tmp/stage/review-evidence`).

## Prior-round findings — all fixed with real code

1. **Duplicate migration version 20261008000056 (round 2)** — the analytics
   scope-columns migration is renumbered to
   `2026-10-08-000057_analytics_scope_columns` (up/down content otherwise
   unchanged). `bash scripts/check-migration-versions.sh` exits 0; adapter
   analytics tests pass on real storage, including the previously failing
   `analytics_query_filtered_all_params` ("no such column:
   analytics_events.user_id" is gone): `cargo test -p gyre-adapters --lib
   analytics` → 13 passed, 0 failed.
2. **Stale line-pinned exemption (round 2)** —
   `scripts/relative-path-defaults-exemptions.txt` pin updated
   `spawn.rs:597` → `:605` (canonicalize reference `:606-609`);
   `bash scripts/check-relative-path-defaults.sh` exits 0. No new exemption
   entries anywhere in the branch: every exemption-file diff in
   `0cb71015..f57a199e` is a deletion or comment-only rewrite
   (abac-exempt-handlers −2 entries, byte-slice −1, fabricated-scope −2 with
   `FROZEN_EXEMPTION_COUNT` lowered 5→3, inert-enforcement −3).
3. **merge_queue.processed only on merge-concluding paths (round 1)** — a
   `fail_queue_entry()` helper now routes every terminal-Failure site through
   one place (gate failure in candidate selection :1027, MR not found
   :1048/:1125, repo not found :584, spec-policy blocks :1170/:1178/:1209,
   breaking-change policy :1275, spec-link gates :1326/:1352, constraint
   enforcement :1429, attestation-ABAC denial :1519, dependency-closed
   :240), and circuit-breaker `Cancelled` entries emit the spec outcome
   `skipped` via `emit_queue_processed_terminal` (:87). When the MR no longer
   resolves, the event records unscoped with the entry's MR id — no
   fabricated scope.
4. **task.status_changed only on HTTP/MCP paths (round 1)** — shared
   `record_task_status_changed()` helper (api/analytics.rs:57) is called by
   all nine production transition paths: HTTP status endpoint (tasks.rs:345),
   MCP update_task (mcp.rs:1062), spawn auto-advance (spawn.rs:562),
   completion auto-advance (spawn.rs:1283), admin force-kill (admin.rs:319),
   stale-abort (stale_agents.rs:89), spec-rejection cancel (specs.rs:979),
   repo-archive cancel (repos.rs:288).
5. **agent.failed missing on spec-rejection kill path (round 1)** — now
   emitted on spec-rejection stop (specs.rs, after the agent Stopped
   transition) and repo-archive stop (repos.rs), with the same
   agent_id/task_id/reason/duration_secs properties as the
   fail/admin-kill/stale-abort paths.
6. **Coverage matrix overclaim (round 1)** — `specs/coverage/system/analytics.md`
   §3 now enumerates the actual multi-path trigger coverage per event;
   sections 6–12 remain honestly `task-assigned` to tasks 147–149; SUMMARY
   totals recomputed consistently (5 implemented / 7 assigned).

## Acceptance criteria — independently verified

- **Struct matches spec schema:** all 9 fields present on `AnalyticsEvent`
  (gyre-domain/src/analytics.rs), pinned by
  `analytics_event_matches_spec_schema` (asserts every field + serde
  round-trip). Id-typed fields are `Option<String>` at rest — documented in
  the coverage matrix, matches the storage layer's TEXT columns, no
  functional impact.
- **All 12 events emit at trigger points with spec-required properties:**
  each emission site inspected — task.status_changed (helper above),
  mr.merged (mr_id/repo_id/gate_count/queue_wait_secs, both HTTP transition
  and queue paths), mr.closed (mr_id/repo_id/reason; transition, repo-archive,
  spec-reject), agent.spawned (agent_id/task_id/compute_target/persona),
  agent.completed (agent_id/task_id/duration_secs), agent.failed (4 kill
  paths), merge_queue.processed (mr_id/outcome/wait_secs, all terminal
  outcomes), gate.failed (gate_id/gate_type/mr_id/output_snippet),
  gate.passed (…/duration_secs), spec.approved
  (spec_path/approver_type/approval_mode), budget.warning
  (workspace_id/metric/threshold_pct, in the real enforcement path that
  returns Err over-limit), search.query (query_length/entity_types/
  result_count/duration_ms). The `"persona": "default-worker"` constant on
  the worker spawn path is spec-accurate, not a stand-in: agent-runtime.md
  §Bootstrap defines `default-worker` as the seeded global worker persona,
  `SpawnAgentRequest` has no persona field, and the orchestrator spawn path
  reports its real tier persona.
- **Query API filter parameters:** `event_name` (exact or trailing-`*`
  prefix via `event_name_like`), `agent_id`, `user_id`, `workspace_id`,
  `repo_id`, `since`, `until` (ISO8601 or unix-sec), `limit` (default 100,
  cap 10_000), `group_by` — present in both the HTTP params struct and the
  port `AnalyticsQueryFilter`; adapter-tested on real storage.
- **Tests pass:** full `cargo test -p gyre-server --lib` → 1298 passed,
  0 failed; `cargo test -p gyre-adapters --lib analytics` → 13 passed;
  `cargo test -p gyre-server --test api_integration --
  orchestrator_spawn_denied_across_tenants` → passed.

## Mutation probes (each restored; `git status --short` clean after)

- Early-return in `emit_queue_processed_terminal` (merge_processor.rs) →
  all 4 queue-processed regression tests **FAIL** (0 passed / 4 failed) —
  the tests are anchored to the production emission, not incidental
  behavior.
- Early-return in `record_task_status_changed` (api/analytics.rs) → all 5
  internal-transition regression tests **FAIL** (0/5).
- Renaming the spec-rejection `agent.failed` event string to
  `agent.failed_MUTATED` → `reject_spec_cancels_in_flight_tasks` **FAILS**
  (control flow untouched — pure event-name assertion).

## Static gates (all exit 0, outputs under /tmp/stage/review-evidence)

check-migration-versions, check-relative-path-defaults,
check-fabricated-scope-defaults, check-inert-enforcement,
check-abac-exempt-handlers (88 handlers), check-byte-slice-truncation,
check-migration-sql-portability, check-task-commit-attribution
(all task-labeled commits in `0cb71015..f57a199e` recorded in frontmatter;
the task-200.md diff in range is a benign commit-list reorder from a merge).

## Non-blocking notes

- `git checkout -- web/dist` was needed after test runs: build.rs rebuilds
  the Svelte bundle as a side effect of `cargo test -p gyre-server`. Not a
  candidate defect; tree restored clean.
- Full workspace suites, all-target Clippy, and the web test suite remain
  the verifier's scope per the review protocol.
