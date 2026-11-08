# Review — task-146 (Complete analytics event schema and auto-emitted events coverage)

Spec: `specs/system/analytics.md` §Event Schema, §Auto-Emitted Events, §Query API, §Query Parameters (coverage sections 1-5 of `specs/coverage/system/analytics.md`).
Candidate under review: `a6e810ef4730d0903fafca8cf564162adb7e8ba5` on `pipeline/task-146/48dde8be360e4ee482bbcc034777432a-1`, diffed against assigned base `c5deca801a331e049f35b181f42d898df24e7e2b` (5 commits, +4247/−258).
Verdict: **needs-revision** (one gate-integrity finding; the task's five acceptance criteria are otherwise met with strong evidence).

## Round 1

All probes run with `/tmp/stage/bin/cargo` from the candidate checkout (tree verified clean at HEAD == candidate before and after every temporary mutation; full log: `/tmp/stage/review-evidence/review-summary.md`).

Test runs:

- `cargo test -p gyre-domain --lib analytics` → 3 passed, 0 failed (schema round-trip test).
- `cargo test -p gyre-adapters --lib analytics` → 13 passed, 0 failed (`analytics_query_filtered_all_params` et al. on real SQLite storage, incl. the duplicate-migration-version guard).
- `cargo test -p gyre-server --lib -- analytics budget_warning search_emits queue_processed status_changed agent_spawned agent_completed agent_failed gate_failed gate_passed mr_closed mr_merged spec_approved` → 54 passed, 0 failed (per-event coverage across all 12 events).
- `cargo test -p gyre-server --lib` → 1468 passed, 0 failed. `cargo test -p gyre-server --test api_integration` → 67 passed, 0 failed.
- `cargo fmt --check` → exit 0. `web/` byte-identical to base.
- Every `scripts/check-*.sh` run on both the candidate and the base (base via a temporary git worktree, removed after): pass=55 / fail=34 on BOTH — the 34 failures are pre-existing environment conditions (JS checks without node_modules, awk-dialect errors), so the branch introduces zero new gate failures. All gates the task names (migration versions/portability, fabricated-scope, relative-path, inert-enforcement, abac-exempt-handlers, abac-route-registry, task-commit-attribution) pass on the candidate.

Mutation probes (tests are load-bearing, not self-confirming):

- Disabling `emit_queue_processed_terminal` (early `return;`) → all 4 `queue_processed` regression tests FAIL.
- Replacing `state.analytics.record(&ev)` with `let _ = &ev;` at `gate_executor.rs:143` and `budget.rs:289` → `failing_gate_emits_gate_failed_analytics_event`, `passing_gate_emits_gate_passed_analytics_event`, and `budget_warning_emitted_when_threshold_crossed` all FAIL (the negative-direction budget test correctly still passes).

Verified working (no findings):

- **Schema**: `AnalyticsEvent` (gyre-domain/src/analytics.rs) carries all 9 spec fields; `with_scope` populates user/session/workspace/repo. Id-typed fields are `Option<String>` (storage TEXT representation, documented in the coverage row; serde shape identical to the spec's `Option<Id>`).
- **All 12 auto-emitted events** fire at real production trigger points with all spec-required properties — every emit site read and matched against the spec table: `task.status_changed` (shared helper called by HTTP status endpoint, MCP `update_task`, spawn/completion auto-advance, admin force-kill, stale-abort, spec-reject and repo-archive cancellations), `mr.merged` (HTTP transition + queue merge, both with `gate_count`/`queue_wait_secs`), `mr.closed` (HTTP transition, repo-archive, spec-reject, each with `reason`), `agent.spawned` (worker + orchestrator paths, `compute_target`/`persona`), `agent.completed` (HTTP + MCP `gyre_agent_complete`, `duration_secs`), `agent.failed` (fail/kill/admin-kill/stale-abort/spec-reject/repo-archive, `reason`), `merge_queue.processed` (merged + every terminal-`Failed` path via `fail_queue_entry` + circuit-breaker `Cancelled` as outcome `skipped`, `outcome`/`wait_secs`), `gate.failed`/`gate.passed` (gate_executor, `output_snippet`/`duration_secs`), `spec.approved` (`spec_path`/`approver_type`/`approval_mode`), `budget.warning` (`workspace_id`/`metric`/`threshold_pct`, 4 call sites in the budget check paths), `search.query` (`query_length`/`entity_types`/`result_count`/`duration_ms`).
- **Query API**: full spec filter set — `event_name` (exact or trailing-`*` prefix), `agent_id`, `user_id`, `workspace_id`, `repo_id`, `since`/`until` (ISO8601 or unix secs, date-only upper bound extends to end-of-day), `limit` (default 100, cap 10_000), `group_by` (event_name/agent_id/workspace_id/day; unsupported fields rejected) — implemented as real SQL in the SQLite and Postgres adapters and in the mem adapter, with adapter tests on real storage.
- **Migration** `2026-10-08-000059_analytics_scope_columns`: unique sequence number, portable SQL, runs on both backends (portability gate passes).
- **Carry-over repairs verified real**: orchestrator spawn tenant containment (handlers load the scoped entity and compare tenant; `orchestrator_spawn_denied_across_tenants` proves 403/no-agent + 201 control), attestation-ABAC Deny fails the queue entry through `fail_queue_entry`, `warn_unapproved_persona` rename, gate-executor/specs fail-closed scope handling, recovery fail-closed `resolve_ref`.

Findings:

- [ ] **F1 (gate integrity, the revision blocker): a NEW dead exemption entry was added vs base — `crates/gyre-server/src/api/traces.rs:150` in `scripts/abac-exempt-handlers-exemptions.txt`.** The entry does not exist in base `c5deca801`; it was re-added from the pre-rebase lineage by merge `935a15413` and kept by the conflict resolution. It describes `get_span_payload` as an unfixed F1b/F1c hazard ("resolution deferred to a follow-up", "claim unbacked") — but `traces.rs` is byte-identical to base and the handler is fixed (resolves span → trace → MR → workspace, returns Forbidden on tenant mismatch), so the entry suppresses nothing: `bash scripts/check-abac-exempt-handlers.sh` prints the identical `OK (88 handler(s) checked)` with the entry present and with it removed (verified empirically, file restored after). Why it still blocks: (a) AGENTS.md freezes exemption files at "never add new entries", and this branch was already repaired once (`ab8c41a2`) precisely because the verification gate rejects NEW exemption entries vs base by design — this candidate would hit that same gate; (b) the task's Shipped notes claim exemption diffs vs base "contain only deletions/comment updates, never new entries", which is false for this file; (c) a line-pinned dead entry is a silent-grandfather vector — a future regression at `traces.rs:150` (reintroduced deferral comment, weakened containment) would keep the check green while the file documents it as a known open hazard. Remedy: delete the single line (zero behavior change, verified); everything else in the exemption diffs is legitimate deletions (orchestrator ×2, `gate_executor.rs:111`, `validate_persona` ×2 — all fixed for real).
- [ ] **F2 (minor, process): unrelated task file touched — `specs/tasks/task-200.md`** commits list reordered vs base (last element moved to first; same set, no semantic change; `check-task-commit-attribution.sh` passes). task-146 should not modify task-200's record. Remedy: revert the reordering, or confirm the controller's attribution tooling owns it and ignore as noise.

Everything else in the candidate is sound: real implementations, real storage, real enforcement, and per-event tests that demonstrably fail when the production emission is disabled. Once F1's one-line deletion lands (and F2 is reverted or acknowledged), this review supports approval.
