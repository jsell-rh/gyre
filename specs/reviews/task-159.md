# Review: task-159 — Wire meta-spec drift warnings into merge gates

**Reviewer:** Independent review (pi runtime)
**Date:** 2026-12-04
**Candidate:** `a68939f950b0b2287137fa7bcd86e8e604f487dd` (base `349a32ac6f5557786f82d17c13f8815de600e837`)
**Verdict:** approve

## Scope reviewed

Full diff base..candidate: `reconciliation.rs` (§9 drift evaluation + warning
format/parse), `merge_processor.rs` (selection-time drift hold + warn gate
result), `merge_policy.rs` (new, KV-backed workspace policy),
`api/workspaces.rs` + `api/mod.rs` + `abac_middleware.rs`
(GET/PUT `/api/v1/workspaces/:id/merge-policy`), `api/merge_deps.rs`
(queue-graph `meta_spec_drift` node field), `MergeQueueGraph.svelte` +
tests, docs, coverage row 13. No `web/dist` changes in the diff.

## Spec conformance (meta-spec-reconciliation.md §9 + task acceptance criteria)

- **Drift check on attestations**: `mr_meta_spec_drift` resolves the MR's
  provenance attestation exactly like the merge-time attestation-ABAC path
  (source-branch head commit via `resolve_ref`, then author agent's current
  task, walking derived nodes to the chain root `SignedInput`), and compares
  the recorded `meta_spec_set_sha` against `compute_meta_spec_set_sha` —
  which is the EFFECTIVE set (tenant defaults merged with workspace
  overrides, §8), so the baseline is correct for tenant-inheriting
  workspaces. No set bound / no attestation / empty recorded SHA → skip
  gracefully.
- **`[WARN] meta-spec-drift` gate result**: exact §9 text
  (`[WARN] meta-spec-drift: code produced under meta-spec set {old}, current
  is {new}. Reconciliation may be needed.`), recorded idempotently
  (update-in-place on retry cycles) with `GateStatus::Passed` — consistent
  with `GateStatus` having no Warn variant and merge blocking keying only on
  `Failed`, so the warning structurally cannot block.
- **Warn only by default**: verified — warn-path MRs merge.
- **`require_current_meta_spec_set: true` → hard block**: strict policy
  requeues the entry with the drift reason (stays Queued until reconciled or
  policy relaxed); relax-and-merge recovery is tested. The hold is evaluated
  DURING candidate selection (same pattern as `selection_trust_hold`), so a
  drifted head-of-queue entry cannot starve the shared queue — same-workspace
  and cross-workspace starvation both covered by regression tests. Atomic
  groups are expanded to all members and held as a unit.
- **Workspace merge policy API**: port-backed KV store (no in-memory state),
  default false, Admin-only PUT with tenant containment, per-handler
  authorization present, ABAC resolver entry registered; corrupt stored
  policy degrades to the spec default (warn) in the processor path while
  surfacing an error on direct lookup. Documented in api-reference.md.
- **UI**: queue graph surfaces recorded drift warnings parsed from the gate
  result (cannot disagree with gate results), ⚠ badge + tooltip + legend;
  no-drift nodes carry no badge.

## Independent probes (evidence: /tmp/stage/review-evidence/)

- `cargo check -p gyre-server` — pass (7m45s, build wrapper).
- `cargo check -p gyre-server --tests` — pass (4m15s): implementation and
  tests type-check.
- `cd web && npm ci && npx vitest run src/__tests__/MergeQueueGraph.test.js`
  — 29/29 pass, including the 5 new drift tests.
- **Mutation probe**: disabling the drift badge render in
  `MergeQueueGraph.svelte` (`{#if false && drift}`) makes the badge test
  fail (1/29) — the frontend drift tests are kill-capable, not
  self-confirming. Component restored afterwards; tree clean.
- `scripts/check-arch.sh`, `check-abac-route-registry.sh`,
  `check-api-auth.sh`, `check-abac-exempt-handlers.sh`,
  `check-in-memory-state-stores.sh`, `check-unbounded-external-http.sh` —
  all pass.
- Rust test logic reviewed line-by-line: assertions target MR merge status,
  queue-entry status/reason, and gate-result presence/format — behavioral,
  not mirrored-logic.

## Deferred to the verifier (sandbox 4 GiB cgroup budget)

Server test compilation must run in the independent verifier. Commands:

- `cargo test -p gyre-server` — specifically the task-159 drift tests:
  `merge_processor::tests::meta_spec_drift_*` (warn-proceeds, hard-block +
  policy-relax recovery, match-no-warning, no-attestation, gate-result
  idempotency), `merge_processor::tests::strict_drift_hold_*` (shared-queue
  starvation, atomic-group hold), `reconciliation::tests::drift_check_*`
  (match/mismatch/no-attestation/no-set/empty-sha), and
  `merge_policy::tests::*` (default/roundtrip/corrupt-fallback).
- `cd web && npx vitest run` — full frontend suite.

## Notes (non-blocking)

- Coverage row 13's note says "process_next + merge_atomic_group §9 gate";
  the actual enforcement lives entirely in `process_next` selection
  (`merge_atomic_group` carries only an explanatory comment). Substance of
  the row is accurate; phrasing slightly overstates the second location.
- The working-tree modification to `specs/tasks/task-159.md` (progress
  reset, `commits: []`) is the executor's assigned-contract restore, not a
  candidate change. Candidate frontmatter attributes the implementation
  commit (`abbcd4aa` feat(task-159)) plus repair/build commits.
