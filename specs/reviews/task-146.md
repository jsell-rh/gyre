# Review — task-146 (Complete analytics event schema and auto-emitted events coverage)

Spec: `specs/system/analytics.md` §Event Schema, §Auto-Emitted Events, §Query API, §Query Parameters (§Purpose is context).
Candidate: `7b0b91f145f77944bf6042b1508be3604db29b61` (base `6f5a631c9fc09664a3e766cd10536f8b56d9ce2f`; the branch carries 26 recorded task-146 commits plus rebase-merge commits onto the moving main base).
Verdict: **needs-revision** — one blocking process finding; the analytics implementation itself is real and complete.

## Round 1 (this review)

Evidence directory: `/tmp/stage/review-evidence` (probe commands, logs, base-vs-candidate comparisons).

Focused probes run here (server test compilation deferred to the verifier per the 4 GiB budget rule):

- `cargo check -p gyre-server -p gyre-domain -p gyre-ports -p gyre-adapters` → clean, no warnings (8m36s).
- `cargo test -p gyre-domain analytics` → 3 passed, incl. `analytics_event_matches_spec_schema` (all 9 spec fields + serde round-trip of the scope fields).
- `cargo test -p gyre-adapters analytics` → 16 passed (filter parity, literal case-sensitive prefix, grouped counts over the full filtered set, limit/since, day aggregation, retention purge).
- Mutation probe: disabled the `workspace_id` predicate in `SqliteStorage::query_filtered` → `analytics_query_filtered_all_params` FAILED (test kills a disabled filter); source restored, suite green again, tree clean.
- Every AGENTS.md-frozen mechanical check passes on the candidate (arch, mem-port-contracts, migration-versions, migration portability, byte-slice truncation, fail-open ref resolution, fabricated/lookup scope defaults, lossy secrets, scope literal defaults, inert enforcement, relative path defaults, forged scope fields, forwarded-header trust, unbounded HTTP, dead message kinds, MCP write tools, ABAC exempt handlers + route registry).
- Base-vs-candidate check comparison in a worktree of the base commit: the only check that flips 0→1 on the candidate is `check-task-commit-attribution` (finding F1 below). All other failing scripts (dead-parameters, silent-result-discard, hardcoded-defaults, dependent-scope-resolution, etc.) fail identically at base — pre-existing debt or mawk environment artifacts, not candidate regressions.

Verified working (no findings):

- **Event schema**: `gyre_domain::AnalyticsEvent` carries all 9 spec fields; `with_scope` populates user/session/workspace/repo; stored on new TEXT columns via portable migration `2026-10-08-000063_analytics_scope_columns` (next free sequence 63; version-uniqueness and SQL-portability checks pass).
- **All 12 auto-emitted events emit at real trigger points with the spec-required properties**, far beyond the two-paths-per-event minimum: `task.status_changed` (HTTP endpoint, MCP `update_task`, spawn/completion auto-advance, stale-abort, admin kill, spec-reject, repo-archive, ralph-loop re-open/ceiling/terminal blocks, signal-chain claims); `mr.merged` (HTTP transition + queue merge, gate_count from real gate results, queue_wait from the queue entry); `mr.closed` (transition, spec-reject, repo-archive, ralph-loop gate-failure close); `agent.spawned` (worker spawn with the *bound* persona slug and the *resolved* compute-target label — both pinned by regression tests, orchestrator spawn, ralph-loop re-spawn); `agent.completed`/`agent.failed` (every death path: fail/stop endpoints, admin kill, status endpoint, stale abort/pause, budget kill, compose teardown, ralph-loop ceilings, process-death monitor); `merge_queue.processed` (merged plus every terminal Failed path and both Cancelled paths, outcome "skipped"); `gate.passed`/`gate.failed` (pre-merge and post-merge gate runs, char-boundary-safe `output_snippet` truncation); `spec.approved` (approver identity derived from the actual approver, not the request body); `budget.warning` (80% threshold with a below-threshold negative test); `search.query` (HTTP + MCP `gyre_search`, identity bound to the verified caller, `query_length` in chars, deduped `entity_types`, measured `duration_ms`).
- **Query API completeness without new routes** (per the task instruction, `api/mod.rs` unchanged): `event_name` exact or trailing-`*` prefix — literal and case-sensitive on all three adapters (mem `starts_with`, SQLite `substr(...) = ...` under BINARY collation, Postgres escaped LIKE), scope filters (agent/user/workspace/repo), ISO8601-or-unix `since`/`until`, capped `limit`, and server-side `group_by` over the full filtered set with `limit` bounding groups, not input (review-round F1 regression tests pin this on mem and SQLite).
- **Anti-forgery**: `POST /analytics/events` ignores body `agent_id`/`user_id` and binds identity to the verified caller; workspace/repo attribution is validated against the caller's tenant via the audit.rs scope validators (shared helpers, cross-checked with task-102's pattern); the search endpoint's `workspace_id` param is validated for the same reason (it attributes the auto-emitted `search.query` event).
- **Tests are kill-bug style with negative controls** (per-event property assertions, absence assertions for the complementary event, below-threshold and empty-query negative tests, persona/compute-target regression tests), and the adapter filter-parity suite demonstrably fails when a predicate is disabled (mutation probe above).

Findings:

- [x] **F1 (blocking, process): `check-task-commit-attribution` regresses 0→1 on the candidate — commit `e5ee853a` is invisible to task-195's review scoping.** `e5ee853a` ("fix(task-146): pass None constraints in analytics gate tests after task-195 merge", 2-line `run_gate` arity fix in `crates/gyre-server/src/gate_executor.rs`) mentions `task-195` in its subject and touches product surface, so the check requires its short SHA in *both* task-146's and task-195's `commits:` frontmatter. It is recorded in task-146's list only. Reproduce: with the candidate's own `specs/tasks/task-146.md` (the HEAD version — note the review working tree was reset to the task's original definition by the executor, which makes the check fail more broadly and is not candidate content), `bash scripts/check-task-commit-attribution.sh` exits 1 with `e5ee853a task-195`; in a worktree of the base commit `6f5a631c` the same script exits 0. The check is a static gate in `scripts/dev-check.sh` (the deterministic integration gate the verifier runs) and in `.pre-commit-config.yaml`, so the candidate cannot pass verification as-is. Remedy: record `e5ee853a` in `specs/tasks/task-195.md`'s `commits:` frontmatter (defensible — the commit adapts task-195's `run_gate` surface), or drop the `task-195` token from the commit subject via the branch's next process commit. Do not touch `scripts/task-c....txt`.

Minor (not blocking, recorded for completeness):

- The candidate adds more best-effort `let _ = state.analytics.record(&ev).await;` sites; `check-silent-result-discard` already fails at base with the same pattern for the pre-existing emit sites, so this is legacy-debt continuation, not a new gate regression (the check is not in the dev-check gate list). If a future round hardens that check, the analytics emit path should propagate errors like the audit emitters do.

Verifier must run (server test compilation deferred from review):

- `cargo nextest run --all` (CI), or at minimum `cargo test -p gyre-server --lib analytics`, `--lib emits`, `--lib records`, and `cargo test -p gyre-server --test api_integration` (`orchestrator_spawn_denied_across_tenants`).
- `scripts/dev-check.sh` static gates, including `check-task-commit-attribution` (expected to fail on this candidate until F1 is fixed).
- `cargo fmt --check` / clippy on the changed files (rustfmt/clippy-diff gates could not be compiled within the review budget; the branch carries dedicated rustfmt/clippy fix commits, but the verifier must confirm).
