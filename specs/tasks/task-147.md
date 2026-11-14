---
title: "Implement analytics decision API with built-in decision types"
spec_ref: "analytics.md §Decision API for Agents"
depends_on:
  - task-146
progress: not-started
coverage_sections:
  - "analytics.md §Decision API for Agents"
  - "analytics.md §GET /api/v1/analytics/decide"
  - "analytics.md §POST /api/v1/analytics/decide/custom"
commits: []
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
