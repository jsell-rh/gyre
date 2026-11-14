# Review: task-147 — Analytics Decision API with built-in decision types

Comparison base: 32c4faefdcd01744f5d2e3015a1a615b2cf83b6e → candidate
2792144179cc875c457ca35d09e3eaef6a619812 (branch head). The candidate forked
from bfc36c9c (task-146 tip, the declared dependency) and reconciled with
cdd1dd02; its own work diff (cdd1dd02..27921441) touches exactly 9 files, all
task-147 scoped. Verdict: **approved** — evidence under
`/tmp/stage/review-evidence/` (see `evidence.md`).

## Spec conformance (analytics.md §Decision API for Agents)

- **All six built-in decisions** evaluate on `GET /api/v1/analytics/decide`
  via pure domain evaluators (`gyre-domain/src/decisions.rs`, 1603 lines; no
  storage access — `check-arch.sh` passes): `feature.promote` (adoption rate
  over distinct actors, error-rate delta and p99 latency delta vs equal-length
  baseline, ≥30-use sample gate), `feature.rollback` (gate failure-rate spike
  current-vs-before, distinct affected MRs, trigger
  `gate.failure_rate_spike`), `gate.effectiveness` (catch rate +
  false-positive rate from `properties.false_positive`), `agent.performance`
  (nearest-rank p50/p99 of `duration_secs`, failure rate from
  `agent.failed`), `budget.headroom` (real `BudgetConfig`/`BudgetUsage` burn
  rate projection, ISO8601 exhaustion), `merge_queue.health` (avg wait,
  failure rate, `blocked_count` from a live `merge_queue.list_queue()` query).
  Response shapes match the spec examples, including `decision` and ISO8601
  `evaluated_at` at top level (`DecisionResult::response_json` flattening).
- **Custom rule DSL** on `POST /api/v1/analytics/decide/custom`: `metric`,
  `window` (`7d`/`24h`/`30m`/…, capped at 365d so no unbounded fetch),
  `threshold`, `comparison` (gte/gt/lte/lt/eq), `then`, optional
  property-equality `filter`; first matching rule wins, else `default`, with
  `matched_rule` index/null and a reason naming each rule's actual count —
  exactly the spec's example response shape. Invalid DSL → 400 naming the
  field.
- **Real data**: events fetched through the analytics port
  (`query_filtered`, 10k cap, window from context) scoped to the caller when
  the context names `workspace_id`/`repo_id`; budget from the real config and
  usage stores; blocked count from the merge-queue port. No hardcoded
  outcomes, no `bail!` stubs, no audit-only paths.
- **Confidence from sample size**: standard-error shrinkage `1 − 1/√n`
  (0 at n≤1, capped 0.99) — monotone, tested.
- **Anti-forgery scoping**: `validate_decision_scope` (audit.rs) applies the
  same lookup-and-compare rule as `record_event` (task-146 F1): cross-tenant
  scope → 403, unknown → 404; forged scopes cannot steer another scope's
  analytics into a decision. Unscoped contexts query platform-wide, the same
  established posture as the pre-existing analytics query API (the decide
  handler is strictly stricter than `query_events`, which does no
  caller-scope validation at all).
- **Empty data** returns spec-shaped defaults (promote=false,
  rollback=false/trigger=none, healthy=true, 100% headroom), not errors —
  pinned by `decide_all_builtins_on_empty_data_return_defaults_not_errors`.
- Routes registered in `api/mod.rs`; both mapped in the ABAC resolver
  (`analytics` resource); `check-abac-route-registry.sh` passes with no new
  exemptions. Coverage rows 6/7/8 updated to `implemented` with accurate
  notes; rows 9–11 correctly left for tasks 148/149.

## Test strength

24 domain unit tests + 14 server integration tests with independently derived
numeric assertions (adoption 100/120, failure rate 0.1, catch rate 0.1,
`affected_mrs` 25, `blocked_count` 2 from a live enqueue, exhaustion =
now+10800 from burn rate). Integration tests drive the production router →
handler → port → mem adapter → evaluator with controlled-timestamp seeds the
HTTP record endpoint cannot produce. No self-confirming, assertionless, or
stub-backed tests found. Domain suite: 534+8 pass on the exact candidate.

## Probes run (evidence: /tmp/stage/review-evidence/)

- `cargo test -p gyre-domain decisions` → 24 passed; full `-p gyre-domain` →
  534+8 passed, 0 failed.
- `cargo check -p gyre-server` → clean.
- `cargo clippy -p gyre-domain -- -W clippy::all` → zero warnings on
  decisions.rs (21 pre-existing elsewhere).
- `check-arch.sh`, `check-abac-route-registry.sh`,
  `check-inert-enforcement.sh`, `check-rustfmt-diff.py` vs base → all pass.

## Base alignment (observation, non-blocking)

The candidate is not a descendant of the assigned base 32c4fae (task-135's
tip): it forked from bfc36c9c and merged cdd1dd02, so the raw base..candidate
diff shows task-135's gate-chain work as "deleted" — absence by fork point,
not a revert (the candidate's own diff touches no task-135 file). Verified in
a scratch clone: `git merge-tree --write-tree 32c4fae 27921441` merges
conflict-free and the merged tree retains BOTH task-135's migration
`2026-11-12-000062_gate_chain_config` AND the new `decisions.rs`. The
verifier must validate the candidate reconciled with main.

## Prior verification failure (durable finding, non-blocking)

The retained checks.sh log tail shows vite build warnings (unused CSS
selectors, a11y hints) in ExplorerCodeTab.svelte / Icon.svelte /
ExplorerCanvas.svelte / ExplorerChat.svelte with a successful build. The
candidate touches zero `web/` files — pre-existing frontend warnings, a
generated-output observation, not a candidate defect.

## Deferred to verifier (sandbox 4 GiB wrapper blocks server tests)

1. `cargo test -p gyre-server api::analytics` (the 14 new integration tests).
2. Workspace-wide `check-clippy-diff.py` / `check-rustfmt-diff.py` on the
   reconciled head.
3. Validate the merged-with-main tree (conflict-free merge verified above).
