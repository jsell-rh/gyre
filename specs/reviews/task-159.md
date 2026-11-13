---
taskId: "159"
taskName: "Wire meta-spec drift warnings into merge gates"
round: "1 (rebase-repair candidate 1f7356d4, base b9487b1b)"
date: "2026-12-19"
verdict: "approved"
---

# Review — task-159 (meta-spec drift warnings in merge gates)

Candidate: `1f7356d4` against assigned base `b9487b1b`. Task source files are
byte-identical to the previously approved head `3e724333`; the only additional
changes are the documented rebase merge of the assigned base (`f8076591`,
generated `web/dist` conflicts only) and docs/process commits.

Probes (full log: `/tmp/stage/review-evidence/task-159-review-round.txt`):

- `/tmp/stage/bin/cargo check -p gyre-server --tests` → pass (9m 23s).
- `cd web && npm ci && npx vitest run src/__tests__/MergeQueueGraph.test.js`
  → 29/29 pass.
- Mutation kill-test: forcing `driftLabel()` to null makes the badge and
  tooltip tests fail (2 failed), restored → 29/29. The UI tests are real.
- `check-abac-route-registry.sh`, `check-arch.sh`,
  `check-abac-exempt-handlers.sh`, `check-mem-port-contracts.sh` → all pass.
  New `/api/v1/workspaces/:id/merge-policy` route registered exactly once in
  the ABAC resolver; no exemption-file growth.

Verified against the acceptance criteria:

- **Drift evaluation is real and production-wired.**
  `selection_meta_spec_drift_hold` (merge_processor.rs) runs inside the
  `process_next` candidate-selection loop — before claim — and expands atomic
  groups to all members. It resolves the MR's provenance exactly like the
  pre-existing merge-time attestation ABAC lookup (source-branch head commit
  first, then the author agent's current task, derived nodes walked to the
  chain-root `SignedInput`), compares the recorded `meta_spec_set_sha` against
  `compute_meta_spec_set_sha` for the workspace, and skips gracefully on
  no-attestation / empty SHA / unbound workspace (each covered by a test).
- **§9 warning format is exact.** `format_meta_spec_drift_warning` emits
  `[WARN] meta-spec-drift: code produced under meta-spec set {old}, current is
  {new}. Reconciliation may be needed.`; recorded as gate result id
  `meta-spec-drift` with `GateStatus::Passed` (no Warn variant exists in
  `gyre-common::gate`; Passed + warning output is the only non-blocking
  representation), idempotent across retry cycles via in-place update.
- **Warn-only default and strict block both enforced and tested.** Warn mode:
  MR merges with the warning recorded. `require_current_meta_spec_set: true`:
  entry requeued `Queued` with the drift reason, MR stays Open, and relaxing
  the policy lets it merge on the next cycle. The hold is evaluated during
  selection with `continue`, so a strictly-blocked head cannot starve later
  entries — regression-tested both same-workspace and cross-workspace, plus
  the atomic-group-held-as-unit case.
- **Policy API is scoped.** `GET/PUT /api/v1/workspaces/:id/merge-policy`,
  KV-backed port store, Admin-only PUT enforced in-handler, tenant scoping on
  both verbs, ABAC-registered; API test drives the real router with JWTs and
  asserts 403 for a Developer caller plus round-trip.
- **UI** surfaces recorded drift (badge, tooltip, legend) from the gate
  results — same source as the MR detail view.

Test quality: no inflation found. The merge-processor tests drive the full
production path (`process_next` with seeded builtin ABAC policies) and assert
end state (Merged/Open, gate results, entry status+reason); the
reconciliation tests exercise the real evaluation against mem-port state; the
frontend tests are mutation-verified.

Server tests for the verifier (compilation exceeds the in-sandbox budget;
`cargo check --tests` proves they compile):

    cargo test -p gyre-server --lib reconciliation::tests::drift_check
    cargo test -p gyre-server --lib reconciliation::tests::drift_warning_parse
    cargo test -p gyre-server --lib merge_processor::tests::meta_spec_drift
    cargo test -p gyre-server --lib merge_processor::tests::strict_drift_hold
    cargo test -p gyre-server --lib merge_policy::tests
    cargo test -p gyre-server --lib api::workspaces::tests::merge_policy_default_admin_only_update_and_roundtrip

Non-blocking observations (follow-ups, not defects in this candidate):

1. Canonical spec §9 illustrates strict mode as `MetaSpecPolicy {
   warn_on_drift, block_on_drift, drift_tolerance }`, while the assigned task
   contract (and the decomposition audit row, 0c1180c2a) mandates
   `require_current_meta_spec_set`, which this candidate implements.
   `drift_tolerance` presumes versioned meta-spec sets; the merged set model
   (task-158) is content-SHA addressed with no version ordering, so shipped
   block-on-any-mismatch equals the spec's `drift_tolerance: 0` mode. A future
   spec-lifecycle amendment of §9's illustrative struct to the realized
   surface would close the naming gap; §9's prose behaviors are all shipped.
2. `merge_policy_or_default` degrades a corrupt stored strict policy to
   warn-only (logged; the direct GET surfaces the error) rather than wedging
   the shared merge queue — documented and tested; defensible.
3. The queue-graph handler's `unwrap_or_default()` on a gate-results listing
   error hides the drift badge in the UI only; the gate result stays recorded
   server-side.
