# Review — task-159 (Wire meta-spec drift warnings into merge gates)

Spec: `specs/system/meta-spec-reconciliation.md` §9 "Merge Gate Behavior".
Candidate under review: `fae71f8817ca80ae8c09f54f6aab14c8a73e3323` (base
`18adceb684926b31138c6d05be1403b1621d3b02`). Product commits: `abbcd4aad`
(feat), `8a99bd808` (starvation repair), `11a702b89` (attestation-ABAC test
repair), plus checkpoint `333030908` — all recorded in the task's `commits:`
frontmatter (attribution gate exit 0).

Verdict: **complete** — every acceptance criterion holds with independent
evidence; both prior-round findings (f05caa26 F1 stale dist, F2 coverage
header) are resolved. No new findings.

## Probes (evidence: /tmp/stage/review-evidence/task-159-r-fae71f88/)

All run at candidate `fae71f881` via `/tmp/stage/bin/cargo`:

- `cargo test -p gyre-server --lib meta_spec_drift` → **5 passed** (warn
  gate result + merge proceeds, hard block under strict policy + relax
  recovery, match-no-warning, no-attestation skip, gate-result idempotency).
- `cargo test -p gyre-server --lib strict_drift_hold` → **2 passed**
  (strictly-blocked drifted head does not starve later entries, same- and
  cross-workspace; drift-held atomic group does not starve later entries).
- `cargo test -p gyre-server --lib merge_policy` → **4 passed** (KV default,
  roundtrip, corrupt-fallback, API default/403/roundtrip/404).
- `cargo test -p gyre-server --lib reconciliation::` → **39 passed**;
  `merge_deps::` → **13 passed**.
- `cd web && npm ci && npx vitest run src/__tests__/MergeQueueGraph.test.js`
  → **29 passed** (5 drift tests: badge present/absent, tooltip text with
  8-char SHA prefixes, tooltip row omitted without drift, legend entry).
- Mechanical: check-arch, check-abac-route-registry,
  check-abac-exempt-handlers, check-in-memory-state-stores,
  check-task-commit-attribution, check-forged-scope-fields,
  check-inert-enforcement — all exit 0.

## Kill tests (tests fail when the behavior is disabled)

- **Kill 1** — `mr_meta_spec_drift` forced to `return None`: 3 of 5 drift
  tests FAIL (`records_warn_gate_result_and_merge_proceeds`,
  `hard_blocks_under_strict_workspace_policy`,
  `warning_gate_result_is_idempotent`); the two no-drift-outcome tests
  correctly still pass. Exit 101. The warn path and the strict-block path
  both genuinely depend on drift detection.
- **Kill 2** — strict-policy conversion disabled
  (`policy.require_current_meta_spec_set && false`):
  `meta_spec_drift_hard_blocks_under_strict_workspace_policy` FAILS. The
  hard-block test genuinely depends on the policy flag, not on incidental
  queue behavior.

Source restored byte-exact after both kills (`git status` clean, HEAD
unchanged at `fae71f881`); drift suite re-run green post-restore.

## Verified against §9 and the acceptance criteria

- **Drift evaluation is real, not structural.** `mr_meta_spec_drift`
  (reconciliation.rs) resolves the MR's provenance attestation by real git
  ref resolution (`resolve_ref` on the source branch head) with a
  task-fallback path, walks derived nodes to the chain's root `SignedInput`,
  and compares the recorded `meta_spec_set_sha` against
  `compute_meta_spec_set_sha` (SHA-256 of the stored set JSON). Skips
  gracefully on no bound set / no attestation / empty recorded SHA — each
  covered by a test.
- **Warn path.** Mismatch records a gate result with id `meta-spec-drift`,
  `GateStatus::Passed`, output exactly
  `[WARN] meta-spec-drift: code produced under meta-spec set {old}, current is {new}. Reconciliation may be needed.`
  (asserted byte-for-byte against the formatter in the test), and the MR
  still merges. Idempotent across retry cycles (existing result updated in
  place — dedicated test).
- **Hard block.** `require_current_meta_spec_set: true` requeues the entry
  with the drift reason and the MR stays Open; relaxing the policy lets the
  same entry merge next cycle (tested end-to-end). The hold is evaluated
  during candidate selection with continue-to-next-candidate, so a blocked
  head cannot starve the shared queue (two regression tests, previously
  kill-tested by the repair round; selection ordering re-inspected this
  round). Atomic-group candidates expand to all members — a drifted member
  holds the group without starving later entries (tested).
- **Policy surface.** Port-backed KV store (no bare AppState mutex —
  in-memory-state-store gate green), corrupt policy degrades to the spec
  default inside the processor while erroring on direct lookup (tested both
  ways). `GET/PUT /api/v1/workspaces/{id}/merge-policy`: PUT Admin-only,
  both handlers load the workspace and compare tenant against the caller
  (cross-tenant → 404), route registered in the ABAC resolver, documented in
  docs/api-reference.md. API test covers default, Developer 403 with no
  state change, Admin roundtrip, unknown workspace 404.
- **UI.** Queue graph API surfaces `meta_spec_drift` per node by parsing the
  same recorded gate result the MR detail view reads (single source, cannot
  disagree). MergeQueueGraph renders the ⚠ badge (`data-drift`,
  `mq-drift-{id}`), tooltip "Meta-spec drift: produced under v{old}, current
  is v{new}", and legend entry — all asserted positively and negatively in
  vitest.

## Prior-round findings (f05caa26) — resolution verified

- **F2 (coverage header):** fixed in `fae71f881`. Mechanically recounted
  this round: 7 `n/a`, 5 `task-assigned`, 6 `implemented` rows → header
  `6/11 (7 n/a, 5 task-assigned, 6 implemented)` is now correct. Row 13
  accurately describes the shipped §9 behavior.
- **F1 (stale committed web/dist):** resolved at the source level, and the
  runtime justification is genuine. Verified: (a) all four marker classes
  present in `web/src/components/MergeQueueGraph.svelte`; (b) the fresh
  `npm run build` executed by build.rs during this review's test compile
  produced a bundle containing `Meta-spec drift` ×2, `data-drift`,
  `mq-drift` — a build of the candidate source reproduces the drift UI;
  (c) the executor's own artifact script
  (`/tmp/stage/pipeline-artifacts.py:21-23`) restores `web/dist` from the
  assignment base and a dedicated final stage regenerates and commits the
  assets (`build(web): regenerate verified assets`), so a dist commit inside
  this assignment would be reverted. Dist restored to committed state after
  the probe; working tree clean.

## Notes (non-blocking, recorded for completeness)

- Under strict policy the hold path intentionally records no `Passed` warn
  gate result (asserted by the hard-block test); the drift text rides the
  queue entry's hold reason instead, so the graph badge appears only once
  the policy is relaxed or the code is reconciled. §9 does not require a
  warn gate result in strict mode ("converts the warning to a hard block"),
  and the hold reason carries the same information — design choice, not a
  violation.
- The warn gate result is recorded during selection, i.e. only for
  candidates that pass the earlier ready/dependency/trust checks — no
  spurious warnings on MRs that are not about to merge.

Test-inflation check: none found — both kill tests fail the suites, the
frontend tests assert rendered DOM both positively and negatively, and the
API test drives the real router.

Scope check: diff base..candidate touches only the task-159 surface
(merge_processor, reconciliation, merge_policy, workspaces/merge_deps APIs,
ABAC resolver registration, docs, coverage, MergeQueueGraph + tests) plus
merge-commit content from the assigned bases; no unrelated production code
modified.
