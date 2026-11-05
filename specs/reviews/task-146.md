# Review — task-146 (Complete analytics event schema and auto-emitted events coverage)

Spec: `specs/system/analytics.md` §Event Schema, §Auto-Emitted Events, §Query API, §Query Parameters.
Candidate: `51dfabe81828a47fa7614046191d645df30eaa73` (branch `pipeline/task-146/cf44fa5f35e94256b5b16dc9594da833-10`), diff from assigned base `75ada1c4`.
Task commits in scope: `38c2c5e7`, `dc13ede3` (plus controller process/docs commits).
Verdict: **not approved** — real, working implementations for the large majority of the contract, but trigger-point coverage for 3 of the 12 auto-emitted events is partial while the shipped notes and the coverage matrix claim all 12 are complete.

## What was verified as real and working

- **Event schema**: `AnalyticsEvent` (gyre-domain/src/analytics.rs) carries all 9 spec fields; `with_scope` populates user/session/workspace/repo; serde round-trip test pins the shape (`analytics_event_matches_spec_schema` passes).
- **Storage**: migration `2026-10-08-000056_analytics_scope_columns` adds the four scope columns + indexes; portable SQL (runs on both SQLite and PostgreSQL via the shared embedded dir); diesel schema updated in lockstep; `SqliteStorage::new` runs pending migrations so the adapter tests exercise the real schema.
- **Query API**: `GET /api/v1/analytics/events` supports the full spec filter set — `event_name` (exact or trailing-`*` prefix via LIKE on SQLite and Postgres), `agent_id`, `user_id`, `workspace_id`, `repo_id`, `since`/`until` (ISO8601 or unix secs), `limit` (default 100, cap 10_000), `group_by` (`event_name`/`agent_id`/`workspace_id`/`day`, unsupported rejected with 400). `/count` and `/daily` (incl. `?days=`) present. Mem, SQLite, and Postgres adapters all implement `query_filtered`.
- **Emission sites verified in source** for all 12 event names, with all spec-required properties, at: tasks.rs:347 + mcp.rs:1064 (`task.status_changed`), merge_requests.rs:666 + merge_processor.rs `emit_mr_merged` (`mr.merged`), merge_requests.rs/repos.rs:320/specs.rs:941 (`mr.closed`), spawn.rs:1111 + orchestrator.rs:146 (`agent.spawned`), spawn.rs:1343 + mcp.rs:1454 (`agent.completed`), spawn.rs fail/stop + admin.rs:349 + stale_agents.rs:43 (`agent.failed`), merge_processor.rs:758/1580/1872 (`merge_queue.processed`), gate_executor.rs:104/114 (`gate.failed`/`gate.passed`), specs.rs:702 (`spec.approved`), budget.rs:270 (`budget.warning`), search.rs:83 (`search.query`).
- **Focused test runs** (this checkout, evidence in `/tmp/stage/review-evidence/`): gyre-domain analytics (3 passed), gyre-server `-- analytics` (42 passed, every per-event test), gyre-server `-- budget` (10 passed, incl. 80%-threshold boundary both sides), gyre-adapters `-- analytics` (13 passed, incl. `analytics_query_filtered_all_params`).
- **Mutation probes** (source restored and verified clean after each):
  - Disabling the `run_gate` analytics record call (gate_executor.rs:143) → both `passing_gate_emits_gate_passed_analytics_event` and `failing_gate_emits_gate_failed_analytics_event` FAIL with "0 events". Tests are anchored to production emission, not self-confirming.
  - Disabling the `workspace_id` conjunct in the mem adapter's `query_filtered` → `query_events_filters_by_scope_ids` FAILS (3 events vs 1). Scope filters are genuinely enforced through the full HTTP → filter → adapter chain.
- **Repair round (dc13ede3) claims verified in source**: orchestrator spawn handlers load the workspace and compare `tenant_id` (403 on mismatch); `evaluate_attestation_abac` Deny fails the queue entry before the merge commit (merge_processor.rs:1526-1538); recovery.rs `resolve_ref` sites fail closed; exemption files only shrank (entries removed after real fixes, `FROZEN_EXEMPTION_COUNT` lowered 5→3); no new exemptions.
- **Static gates** all pass in this checkout: arch, mcp-write-tools, dead-message-kinds, abac-route-registry, abac-exempt-handlers, mem-port-contracts, scope-literal-defaults, fabricated-scope-defaults, fail-open-ref-resolution, unbounded-external-http, forwarded-header-trust, forged-scope-fields, in-memory-state-stores, relative-path-defaults, byte-slice-truncation, lossy-secret-conversion, inert-enforcement, migration-versions, migration-sql-portability, task-commit-attribution.

## Findings

### F1 (code): `merge_queue.processed` is not recorded on most failure paths — spec outcome "failed" only partially wired, "skipped" never

`crates/gyre-server/src/merge_processor.rs`

The spec trigger is "Queue entry processed" with `outcome` (merged/failed/skipped). The candidate emits the event only where the merge *attempt* concludes: merged (single-entry path :1580 and atomic-group path :758), conflict (:1838), and git error (:1850). A queue entry that reaches the terminal `Failed` state through any of the other processing paths records **no** event:

- quality-gate failure during candidate selection (:1026 — the primary failure mode for queue entries),
- MR not found (:1047), repo not found (:576, :1124),
- spec-policy blocks (:1167, :1180, :1211),
- breaking-change policy (:1281),
- spec-link merge gates (:1334, :1363),
- constraint enforcement (:1442),
- attestation-ABAC denial (:1535).

The spec's `skipped` outcome value is never emitted anywhere: circuit-breaker cancellation (:80) permanently removes entries with no event, and dequeue/cancel paths emit nothing.

Reproduce: enqueue an MR whose required gate fails; run `process_next`; the entry status becomes `Failed`; `state.analytics.query(Some("merge_queue.processed"), ...)` returns no event for it. Evidence: `review-evidence/merge-queue-emission-sites.txt` — 2 `emit_queue_processed_failed` call sites vs 15 `MergeQueueEntryStatus::Failed` transition sites.

Impact: merge-queue analytics systematically undercount failed outcomes — material for the spec's agent-consumable decisions (`merge_queue.health` failure rate, §Decision API). The shipped claim "All 12 spec-required auto-emitted events record at their real trigger points" and coverage section 3 (depth 3) marked `implemented` overstate this event's coverage.

### F2 (code): `task.status_changed` not recorded on internal (non-endpoint) task transitions

`crates/gyre-server/src/api/spawn.rs`, `crates/gyre-server/src/api/admin.rs`, `crates/gyre-server/src/api/specs.rs`

The spec trigger is "Task transitions status". The event is emitted only from the HTTP status endpoint (tasks.rs:347) and the MCP `update_task` tool (mcp.rs:1064). These production transition sites persist status changes with no event:

- agent spawn auto-advance Backlog→InProgress (spawn.rs:552),
- agent completion auto-advance →InProgress/→Review (spawn.rs:1261-1266),
- admin force-kill blocking the agent's task →Blocked (admin.rs:311),
- spec-rejection cancelling all in-flight tasks for the spec →Cancelled (specs.rs:972).

Reproduce: spawn an agent for a Backlog task (task becomes InProgress in storage); query analytics for `task.status_changed` → no event. Evidence: `review-evidence/task-transition-sites.txt`.

### F3 (code, minor): `agent.failed` not recorded on the spec-rejection agent-stop path

`crates/gyre-server/src/api/specs.rs:974-977`

Spec-rejection stops active agents working on the cancelled tasks (`transition_status(AgentStatus::Stopped)` + update) with no `agent.failed` event. The candidate itself maps operator-initiated stops to `agent.failed` with reason "killed" (spawn.rs `stop_agent`), and documents kill coverage as "fail/admin-kill/stale-abort" — this fourth kill path is silent, inconsistent with the candidate's own semantics.

### F4 (coverage): coverage matrix and shipped notes overstate trigger-point completeness

`specs/coverage/system/analytics.md` §3 (Auto-Emitted Events, depth 3) is marked `implemented` with the note "All 12 events emit at real trigger points", and `specs/tasks/task-146.md` Shipped makes the same claim. Per F1-F3 this is true for 9 of 12 events and partial for `merge_queue.processed`, `task.status_changed`, and `agent.failed`. Under the no-fakery regime the matrix must reflect reality; either the remaining trigger points get wired or the section note must scope the claim.

## Notes for the fix

All three gaps are small and localized: route the remaining terminal `MergeQueueEntryStatus::Failed`/`Cancelled` transitions through the existing `emit_queue_processed_failed` helper (or an outcome="skipped" sibling for cancellations), and add a shared `emit_task_status_changed(old, new, task)` helper called from the four internal task-transition sites plus the spec-reject agent stop. The per-event test pattern already in the repo (trigger the production path, query the repository, assert properties) extends naturally.

— Reviewer, 2026-11-04 (evidence: `/tmp/stage/review-evidence/`)
