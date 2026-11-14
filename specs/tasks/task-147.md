---
title: "Implement analytics decision API with built-in decision types"
spec_ref: "analytics.md §Decision API for Agents"
depends_on:
  - task-146
progress: ready-for-review
coverage_sections:
  - "analytics.md §Decision API for Agents"
  - "analytics.md §GET /api/v1/analytics/decide"
  - "analytics.md §POST /api/v1/analytics/decide/custom"
commits: ["41b239140ee2ae658d64cde4cc266fe786cf6767"]
---

## Spec Excerpt

From `analytics.md` §Decision API for Agents:

Agents use analytics to make autonomous data-driven decisions — should this feature flag be promoted? Did this change improve the merge success rate? Is this agent persona performing well?

### `GET /api/v1/analytics/decide`

Evaluate a named decision based on current analytics data.

**Built-in decisions:**

| Decision Name | Question | Returns |
|---|---|---|
| `feature.promote` | Should this feature flag be promoted? | `{promote: bool, confidence: float, reason: string, metrics: {...}}` |
| `feature.rollback` | Should this feature be rolled back? | `{rollback: bool, confidence: float, reason: string, trigger: string}` |
| `gate.effectiveness` | Is this quality gate catching real bugs? | `{effective: bool, false_positive_rate: float, catch_rate: float}` |
| `agent.performance` | Is this agent persona performing within norms? | `{healthy: bool, p50_duration_secs, p99_duration_secs, failure_rate: float}` |
| `budget.headroom` | How much budget headroom does this workspace have? | `{tokens_remaining_pct: float, cost_remaining_pct: float, projected_exhaustion_at: string}` |
| `merge_queue.health` | Is the merge queue healthy or degraded? | `{healthy: bool, avg_wait_secs, failure_rate: float, blocked_count: int}` |

### `POST /api/v1/analytics/decide/custom`

Agents can define and evaluate custom decisions using a simple rule DSL with `metric`, `window`, `threshold`, `comparison`, and `then` fields.

## Implementation Plan

1. **Define decision types** in `gyre-domain`:
   ```rust
   pub struct DecisionRequest {
       pub decision: String,
       pub context: serde_json::Value,
   }

   pub struct DecisionResult {
       pub decision: String,
       pub outcome: serde_json::Value,  // decision-specific fields
       pub reason: String,
       pub evaluated_at: u64,
   }
   ```

2. **Implement built-in decision evaluators** in `gyre-domain`:
   - `FeaturePromoteDecision`: queries `feature.used` events, computes adoption rate, error rate delta, latency delta
   - `FeatureRollbackDecision`: queries error/failure events in a window, detects spikes
   - `GateEffectivenessDecision`: queries gate.passed/gate.failed, computes false positive rate
   - `AgentPerformanceDecision`: queries agent.completed/agent.failed, computes p50/p99 duration, failure rate
   - `BudgetHeadroomDecision`: reads budget usage, projects exhaustion
   - `MergeQueueHealthDecision`: queries merge_queue.processed, computes avg wait and failure rate

3. **Implement custom decision evaluator:**
   - Parse the rule DSL (metric, window, threshold, comparison, then)
   - For each rule: query the analytics store for the metric in the window, compare against threshold
   - Return the first matching rule's `then` value, or the `default`

4. **Add REST endpoints** in `gyre-server`:
   - `GET /api/v1/analytics/decide?decision=<name>&context=<json>` — evaluate a built-in decision
   - `POST /api/v1/analytics/decide/custom` — evaluate a custom decision DSL

5. **Register routes** in `crates/gyre-server/src/api/mod.rs`.

6. **Tests:**
   - Unit test for each built-in decision evaluator with known analytics data
   - Unit test for custom decision rule DSL parsing and evaluation
   - Integration test: record events, call decide endpoint, verify correct decision
   - Edge case: empty analytics data returns reasonable defaults (not errors)

## Acceptance Criteria

- [ ] `GET /api/v1/analytics/decide` evaluates all 6 built-in decisions
- [ ] `POST /api/v1/analytics/decide/custom` evaluates custom rule DSL
- [ ] Each built-in decision returns the spec-defined response shape
- [ ] Custom decisions support `metric`, `window`, `threshold`, `comparison`, `then` rules
- [ ] Decisions are based on real analytics data from the event store
- [ ] Confidence scores are computed from sample size and statistical significance
- [ ] Tests pass

## Agent Instructions

- Read `crates/gyre-server/src/api/analytics.rs` for the existing analytics query infrastructure
- Read `crates/gyre-server/src/api/mod.rs` for route registration patterns
- Read `analytics.md` for the full JSON examples of request/response shapes
- The decision evaluators should be pure domain functions that accept analytics data as input — no direct DB access from domain
- Use the existing analytics query port to fetch event data for decision evaluation
- Register new routes in mod.rs alongside existing analytics routes (lines 430-438)

## Shipped

The analytics Decision API (analytics.md §Decision API for Agents): all
six built-in decisions plus the custom rule DSL, evaluated over real
platform data.

### Behavior

- **`GET /api/v1/analytics/decide?decision=<name>&context=<json>`** —
  evaluates the six built-in decisions. The evaluators are pure domain
  functions in `gyre-domain/src/decisions.rs` (hexagonal boundary kept:
  no storage access from domain); the handler in
  `crates/gyre-server/src/api/analytics.rs` fetches each decision's
  events through the analytics port (`query_filtered`, 10k cap, window
  derived from the context) and delegates evaluation:
  - `feature.promote` — adoption rate (distinct actors on the flag vs all
    `feature.used` actors), error-rate delta and p99 latency delta
    (current window vs equal-length baseline), sample size; promotes only
    with ≥30 uses, ≥50% adoption, ≤+1pt error delta, ≤+100 ms latency
    delta; spec response shape `{promote, confidence, reason, metrics}`.
  - `feature.rollback` — gate failure-rate spike detection (current vs
    before window), distinct affected MRs, trigger
    `gate.failure_rate_spike`; shape `{rollback, confidence, reason,
    trigger, metrics}`.
  - `gate.effectiveness` — catch rate (`gate.failed` ÷ all gate runs) and
    false-positive rate (failures marked `properties.false_positive`);
    shape `{effective, false_positive_rate, catch_rate}`.
  - `agent.performance` — p50/p99 of `agent.completed` durations,
    failure rate from `agent.failed`; shape `{healthy,
    p50_duration_secs, p99_duration_secs, failure_rate}`.
  - `budget.headroom` — tokens/cost remaining pct from the workspace's
    real `BudgetConfig` limits and `BudgetUsage` snapshot, plus a
    projected exhaustion timestamp extrapolated from the burn rate
    since `period_start` ("none"/"unknown" when not projectable);
    requires `workspace_id` in the context.
  - `merge_queue.health` — avg wait and failure rate from
    `merge_queue.processed` events, `blocked_count` from a live
    `merge_queue.list_queue()` query (non-terminal entries); shape
    `{healthy, avg_wait_secs, failure_rate, blocked_count}`.
- **`POST /api/v1/analytics/decide/custom`** — parses and validates the
  rule DSL (`metric`, `window` `"7d"`/`"24h"`/`"30m"`/…, `threshold`,
  `comparison` gte/gt/lte/lt/eq, `then`, optional property-equality
  `filter`), fetches each distinct metric's events once over the widest
  rule window, and evaluates: first matching rule's `then`, else the
  decision's `default`, with `matched_rule` (index or null) and a reason
  naming each rule's actual count. Invalid DSL → 400 naming the field.
- Confidence scores are computed from sample size via the
  standard-error shrinkage `1 − 1/√n` (0 at n≤1, capped 0.99).
- Anti-forgery scoping: a context naming `workspace_id`/`repo_id` is
  validated against the caller's tenant (shared
  `validate_decision_scope` in audit.rs, same rule as `record_event`);
  cross-tenant scope → 403, unknown → 404 — a forged scope cannot steer
  another scope's analytics into a decision outcome.
- Empty analytics data returns spec-shaped defaults (promote=false,
  rollback=false/trigger=none, healthy=true, 100% headroom), not errors.
- Routes registered in `api/mod.rs`; both mapped in the ABAC route
  registry (`analytics` resource).

### Test evidence

- `cargo test -p gyre-domain --lib decisions` — 24 unit tests pass in
  this sandbox: one per built-in evaluator (healthy + degraded +
  empty-data cases), ISO8601 formatting, window parsing, nearest-rank
  percentiles, significance confidence, DSL parse/validation, custom
  evaluation (first-match, default, window + filter narrowing), and the
  `DecisionResult` response shape.
- `cargo check -p gyre-server --tests` — the server integration tests
  (14 new tests in `api/analytics.rs`: end-to-end decide for all six
  builtins against seeded mem-store events with controlled timestamps,
  budget headroom from real config/usage stores, merge-queue health with
  live blocked entries, custom DSL match/default/invalid, unknown
  decision 400, cross-tenant scope 403) type-check; the sandbox build
  wrapper defers server test execution to the independent verifier
  (evidence in `/tmp/stage/review-evidence`).
- `scripts/check-arch.sh`, `check-abac-route-registry.sh`,
  `check-inert-enforcement.sh`, `check-forged-scope-fields.sh`,
  `check-scope-literal-defaults.sh`, `check-mem-port-contracts.sh`,
  `check-byte-slice-truncation.sh`, `check-unbounded-external-http.sh`,
  `check-lossy-secret-conversion.sh`, `check-rustfmt-diff.py` — all
  pass on the assignment head.
