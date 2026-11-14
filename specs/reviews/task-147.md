# Review — task-147 (Implement analytics decision API with built-in decision types)

Spec: `specs/system/analytics.md` §Decision API for Agents (§GET /api/v1/analytics/decide, §POST /api/v1/analytics/decide/custom).
Base `bfc36c9c`, candidate `890ba53e` (implementation commit `41b23914`, listed in `commits:` frontmatter; `d673937a`/`890ba53e` are task-file-only — no unlisted product-surface drift).
Verdict: **needs-revision** — the functional implementation is real and verified clean, but the candidate fails the repo's diff-scoped rustfmt and clippy CI gates on its own new lines (F1/F2, mechanical fixes).

## Round 1

### Probes run (evidence in /tmp/stage/review-evidence)

- `cargo test -p gyre-domain --lib decisions` → **24/24 passed** (EXIT=0) — `domain-decisions-tests.log`.
- `cargo check -p gyre-server --tests` → **EXIT=0** (9m51s) — `server-check-tests.log`.
- `cargo clippy -p gyre-domain --lib` → EXIT=0 but **4 warnings on new decisions.rs lines** (231 ptr_arg; 572/653/654 unnecessary_map_or) — `clippy-domain.log`.
- `python3 scripts/check-rustfmt-diff.py bfc36c9c` → **EXIT=1**: changed lines need formatting in decisions.rs (188-193, 389-392, 500, 510), lib.rs (16, 71-74), abac_middleware.rs (412-416), api/analytics.rs (818-820, 2325, 2356, 2366, 2389, 2395, 2420, 2487, 2513, 2519) — `rustfmt.log`. Rust files in the working tree match the candidate exactly (only `specs/tasks/task-147.md` differs, the harness's reset).
- Invariant scripts on the candidate tree: check-arch, check-abac-route-registry, check-abac-exempt-handlers, check-forged-scope-fields, check-inert-enforcement, check-scope-literal-defaults, check-byte-slice-truncation, check-mem-port-contracts, check-unbounded-external-http, check-lossy-secret-conversion, check-relative-path-defaults, check-forwarded-header-trust — **all EXIT=0** (`invariants.log`).
- check-task-commit-attribution.sh failed in the working tree, but only because the harness reverted `specs/tasks/task-147.md` to `commits: []`; the candidate commit itself carries `commits: ["41b23914…"]` — attribution is clean on the candidate (`attribution.log` + `git show HEAD:specs/tasks/task-147.md`).

### Verified working (no functional findings)

- **All six built-in decisions** are pure domain evaluators (`gyre-domain/src/decisions.rs`, no storage access; check-arch passes) with the spec response shapes: feature.promote `{promote, confidence, reason, metrics}` (adoption/error-delta/p99-latency-delta over `feature.used`/`feature.error`, baseline-window comparison), feature.rollback `{rollback, confidence, reason, trigger}` (gate failure-rate spike, before/after rates, distinct affected MRs), gate.effectiveness `{effective, false_positive_rate, catch_rate}`, agent.performance `{healthy, p50/p99_duration_secs, failure_rate}` (nearest-rank percentiles over `agent.completed` durations), budget.headroom `{tokens_remaining_pct, cost_remaining_pct, projected_exhaustion_at}` (from real BudgetConfig/BudgetUsage via `effective_workspace_config`, tenant-cascade-aware), merge_queue.health `{healthy, avg_wait_secs, failure_rate, blocked_count}` (blocked from live `list_queue()` — the queue is platform-global, entries carry no workspace, matching the existing list endpoint's model).
- **Routes + ABAC**: `/api/v1/analytics/decide` (GET) and `/decide/custom` (POST) registered in `api/mod.rs`; both have `RouteResourceMapping` entries (registry gate passes; no duplicates, no exemption-file growth).
- **Custom rule DSL**: parsed and validated server-side (metric/window/threshold/comparison gte|gt|lte|lt|eq/then/optional property-equality filter; invalid → 400 naming the field); first matching rule wins, else `default`, with `matched_rule` index/null and a reason naming each rule's actual count — matches the spec example response `{decision, outcome, matched_rule, reason, evaluated_at}`.
- **Real data**: handlers fetch events through the analytics port (`query_filtered`, scoped windows, 10k cap), budget through the budget stores, blocked entries through the merge-queue port. No hardcoded outcomes.
- **Confidence from sample size**: `1 − 1/√n` capped 0.99 (acceptance criterion met; unit-tested monotonicity).
- **Anti-forgery scoping**: `validate_decision_scope` (audit.rs) looks up context-named workspace/repo and compares tenant (403 cross-tenant, 404 unknown), shared with `record_event`; integration test `decide_rejects_cross_tenant_scope_in_context` proves both handlers reject a forged scope end-to-end through the real router.
- **Empty-data defaults**: all six builtins return spec-shaped defaults, not errors (integration test with a fresh store).
- **Tests are real, not self-confirming**: domain tests assert concrete numeric outcomes on constructed events (e.g. adoption 100/120, before_rate <0.05 / after_rate >0.3, exhaustion ETA = NOW+10800 from a 2500/h burn); server tests drive the production router with auth and seeded store events at controlled timestamps (the HTTP record endpoint stamps `now()`, which cannot exercise historical windows — direct seeding is justified and documented).
- **Attribution**: single product commit `41b23914`, recorded in frontmatter.

### Findings (blocking, both mechanical)

- [F1] **rustfmt CI gate fails on the candidate** — `scripts/check-rustfmt-diff.py bfc36c9c` exits 1 on the candidate's own new lines (files/lines above). CI job "rustfmt" (ci.yml:45) runs this check on HEAD^1. Additionally, the task's "Shipped" section claims "check-rustfmt-diff.py — all pass on the assignment head", which is false. Repair: `cargo fmt` on the four files in a task-147-attributed commit, append the SHA to `commits:`.
- [F2] **clippy CI gate fails on the candidate's new lines** — 4 clippy warnings entirely inside the new `decisions.rs` (ptr_arg at :231 — `percentile(&mut Vec<f64>, …)` should take `&mut [f64]`; unnecessary_map_or at :572/:653/:654 — `map_or(true, …)` → `is_none_or`). `scripts/check-clippy-diff.py` (CI "Clippy" job, ci.yml:63) fails on any warning intersecting a changed line; the file is new so all four intersect. Repair: apply the lint suggestions in the same revision commit.

### Deferred to the independent verifier (sandbox budget)

- `cargo test -p gyre-server` (focused: `cargo test -p gyre-server decide` — the 14 new tests in `api/analytics.rs` type-check via `cargo check -p gyre-server --tests` EXIT=0 but were not executed here; large server test compilation deferred per runtime budget).
- Workspace-wide `cargo clippy --all-targets --all-features` via `scripts/check-clippy-diff.py HEAD^1` (the domain-package probe above is the evidence for F2).
- `cargo fmt --all -- --check` / `scripts/check-rustfmt-diff.py HEAD^1` (F1 evidence already reproducible locally).

### Non-blocking observations (recorded, not defects)

- Per-metric event fetch is capped at 10,000 (newest-first truncation in `query_filtered`); a custom rule counting >10k events in a wide window (`lte`/`eq` comparisons) could mis-evaluate at extreme volume. Documented in Shipped ("10k cap"); consistent with the query API's documented max limit. A `count()` port method exists if exact counting is ever required.
- Unscoped decisions (no `workspace_id` in context) aggregate platform-wide — consistent with the task-146-approved analytics query API semantics, not a new exposure introduced here.
- `budget.headroom` requires `workspace_id` in the context (400 otherwise) — documented in `docs/api-reference.md`.
- Pure generated-output observation: `web/dist/` untouched by this task (no frontend changes) — nothing to note beyond that; verifier-owned.
