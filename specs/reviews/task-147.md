# Review: task-147 — Analytics decision API with built-in decision types

Comparison base: cdd1dd02d252e039545818860fe3376e0eca5b53 → candidate
e7e9efbfbc7482ff7df742b9b3c8104f2066ca8e.
Diff: 9 files, +2563/−7. Product surface: `gyre-domain/src/decisions.rs` (new, 1605
lines incl. tests), `gyre-server/src/api/analytics.rs` (+814: two handlers + 12 router
tests), `api/mod.rs` (2 routes), `abac_middleware.rs` (2 resolver entries),
`api/audit.rs` (`validate_decision_scope`), `docs/api-reference.md`, coverage matrix.
No `scripts/` changes, no exemption-file changes, no deleted tests, no gate weakening.
The other range commits (d673937a, 890ba53e, 0e5c4ff1, a943e49b, e7e9efbf) touch only
the task file (verified per-commit); the two product commits (41b23914, 4201065e) are
recorded in the task frontmatter.

## R1 Findings

No blocking findings. Evidence reviewed:

### Spec conformance (analytics.md §Decision API for Agents)

- **All 6 built-in decisions** evaluate on `GET /api/v1/analytics/decide` with the
  spec response shapes: `feature.promote` `{promote, confidence, reason, metrics}`,
  `feature.rollback` `{rollback, confidence, reason, trigger}` (+`metrics`, matching
  the spec's own rollback example), `gate.effectiveness`
  `{effective, false_positive_rate, catch_rate}`, `agent.performance`
  `{healthy, p50_duration_secs, p99_duration_secs, failure_rate}`,
  `budget.headroom` `{tokens_remaining_pct, cost_remaining_pct,
  projected_exhaustion_at}`, `merge_queue.health` `{healthy, avg_wait_secs,
  failure_rate, blocked_count}`. Verified against the spec's JSON examples
  (analytics.md lines 75–215); `DecisionResult::response_json` flattens the outcome
  with `decision` + ISO8601 `evaluated_at`, exactly the spec's example envelope.
- **Custom rule DSL** on `POST /api/v1/analytics/decide/custom`: `metric`, `window`
  ("7d"/"24h"/"30m" suffix forms), `threshold`, `comparison` (gte/gt/lte/lt/eq),
  `then`, optional property-equality `filter`; first matching rule wins, `default`
  otherwise, `matched_rule` index/null, and a `reason` naming each rule's actual
  count (mirrors the spec example's reason text). The spec's exact example decision
  (`spec-approval-pace`) is exercised end-to-end in
  `decide_custom_rule_dsl_first_match_and_default`. Invalid DSL → 400 naming the
  field (`CustomDecisionSpec::parse` validates name, window, comparison, filter
  shape per rule with the rule index).
- **Pure-domain evaluators** (`gyre-domain/src/decisions.rs`): no storage access —
  the hexagonal lint (`scripts/check-arch.sh`) passes and the module imports only
  `crate`, serde, and std. The server handler fetches events via the analytics port
  (`query_filtered`, limit 10k, window derived from context), budget config/usage
  via the budget stores, and blocked entries via the merge-queue port, then
  delegates. Exactly the plan's layering.
- **Real data, not constants**: adoption/error/latency deltas from
  `feature.used`/`feature.error` events with controlled timestamps seeded straight
  into the store (the HTTP record path stamps `now`, so the tests deliberately use
  `state.analytics.record` to exercise historical windows); gate pass/fail counts;
  p50/p99 from `properties.duration_secs`; budget numbers from a real
  `BudgetConfig`+`BudgetUsage` pair; blocked_count from live non-terminal queue
  entries (`list_queue` excludes Merged/Failed/Cancelled — sqlite adapter line 157,
  asserted by its own `list_queue_excludes_terminal` test).
- **Confidence from sample size** (acceptance criterion):
  `significance_confidence(n) = min(1 − 1/√n, 0.99)` — 0 at n≤1, ≥0.9 at n=100,
  capped, monotone; unit test `significance_confidence_grows_with_sample` pins the
  curve, and every built-in outcome carries it.
- **Edge case**: `decide_all_builtins_on_empty_data_return_defaults_not_errors`
  asserts 200 + spec-shaped defaults (promote=false, healthy=true, 100% headroom,
  etc.) for all six on an empty store — the plan's empty-data requirement.
- **Unknown decision name** → 400 listing the built-ins.

### Security / scope handling

- **Anti-forgery on decision context** (`validate_decision_scope`, audit.rs):
  `context.workspace_id`/`repo_id` are validated against the caller's tenant
  (lookup + compare → 403 cross-tenant, 404 unknown) before they scope any query —
  the same rule task-146's review imposed on `record_event`. Tested both ways
  (`decide_rejects_cross_tenant_scope_in_context` covers GET decide and POST custom,
  asserting FORBIDDEN, not silently ignored scope).
- Absent scope fields → unscoped (platform-wide) aggregation, byte-for-byte
  consistent with the pre-existing `GET /api/v1/analytics/events` query semantics
  (`query_events` applies `workspace_id`/`repo_id` only when supplied) and gated by
  the same ABAC `analytics` resource mapping. Not a new exposure introduced by this
  task.
- `budget.headroom` requires `workspace_id` in context (400 otherwise) and the id is
  tenant-validated before the budget stores are read.
- Routes registered in `mod.rs` **and** in the ABAC resolver
  (`abac_middleware.rs` +2 entries, no exemption file growth) —
  `scripts/check-abac-route-registry.sh` passes.
- `parse_window_secs` caps at 365d, so a custom rule can never force an unbounded
  fetch; the trailing-unit byte slice only happens on an ASCII-unit last byte
  (multibyte UTF-8 falls through to the numeric-parse failure path), and
  `check-byte-slice-truncation.sh` passes.

### Test quality

The 12 router tests run through `crate::build_router(test_state())` — real auth +
ABAC middleware stack — and assert on status codes and computed values, not on
mirrored logic. Killing any evaluator input (removing the error-spike events,
flipping a cross-tenant scope, corrupting the DSL) flips a concrete assertion. The
24 domain unit tests independently pin each evaluator's math (adoption rate
100/120, error delta 0.3, catch rate 0.1, p50 range, budget 75%/80% and the exact
projected-exhaustion instant, percentile nearest-rank, window parsing, first-match
rule semantics, window/filter narrowing).

### Verification run (this review)

- `/tmp/stage/bin/cargo test -p gyre-domain decisions` → **24 passed, 0 failed**
  (evidence: review-evidence/domain-decisions-tests.txt).
- `/tmp/stage/bin/cargo check -p gyre-server` → clean in 8m10s
  (review-evidence/server-cargo-check.txt) — handlers, ABAC entries, and audit
  helper type-check against the real AppState.
- `scripts/check-abac-route-registry.sh` → OK (both new routes resolve).
- `scripts/check-arch.sh` → OK (no forbidden deps/I/O in domain).
- `scripts/check-byte-slice-truncation.sh` → OK.

### Verifier must run (deferred from this sandbox: gyre-server test-binary
compilation exceeds the 4 GiB budget here)

- `cargo test -p gyre-server --lib analytics::tests::decide` — the 12 Decision API
  router tests (decide_unknown_decision_is_rejected,
  decide_all_builtins_on_empty_data_return_defaults_not_errors,
  decide_feature_promote_from_real_event_data,
  decide_feature_promote_rejects_error_spike,
  decide_feature_rollback_triggers_on_gate_failure_spike,
  decide_gate_effectiveness_and_agent_performance,
  decide_budget_headroom_from_real_usage,
  decide_budget_headroom_requires_workspace,
  decide_merge_queue_health_from_real_events_and_queue,
  decide_custom_rule_dsl_first_match_and_default,
  decide_custom_invalid_dsl_is_rejected,
  decide_rejects_cross_tenant_scope_in_context).
- `cargo test -p gyre-adapters` (query_filtered filter semantics backing the
  scoped decide queries) and workspace clippy/fmt gates as usual.

### Notes (non-blocking)

- N1: `merge_queue.health`'s `blocked_count` comes from `list_queue()`, which is
  platform-wide (the port takes no scope and `MergeQueueEntry` carries no
  workspace field). This is the identical semantics as the pre-existing
  `GET /api/v1/merge-queue` endpoint, so it is a pre-existing port limitation, not
  a regression from this task; it exposes only an aggregate count.
- N2: The 10k event cap per metric silently truncates decisions over very high
  volume windows (kept newest-first, the right side for window comparisons).
  Acceptable; documented via the filter constant.
- N3 (generated-output observation, non-blocking per review rules): this task
  touches no web source; nothing to observe for `web/dist/`.

## Verdict

R1: approved — task meets analytics.md §Decision API for Agents, §GET
/api/v1/analytics/decide, and §POST /api/v1/analytics/decide/custom with real
implementations and substantive tests.
