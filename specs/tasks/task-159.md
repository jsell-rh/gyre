---
title: "Wire meta-spec drift warnings into merge gates"
spec_ref: "meta-spec-reconciliation.md §9"
depends_on: [task-156]
progress: ready-for-review
coverage_sections:
  - "meta-spec-reconciliation.md §9 Merge Gate Behavior"
commits: ["abbcd4aadae36785f7dfe7efa4e4dec03ca73d95", "333030908ce0ff64b2810ea0003fc0e28598abf3"]
---

## Spec Excerpt

From `meta-spec-reconciliation.md` §9 — Merge Gate Behavior:

> When an MR enters the merge queue, the merge processor checks whether the code was produced under the current meta-spec set version:
>
> - If `meta_spec_set_sha` on the MR's attestation matches the workspace's current set → pass
> - If they differ → warn: `[WARN] meta-spec-drift: code produced under meta-spec set {old_sha}, current is {new_sha}. Reconciliation may be needed.`
> - Warn only, do not block. The code may be valid even under the new meta-spec set (many meta-spec changes are additive or don't affect existing code).
>
> If a workspace admin wants to block merges during reconciliation, they set the workspace merge policy to `require_current_meta_spec_set: true`. This converts the warning to a hard block.

## Implementation Plan

1. **Add meta-spec drift check to merge processor** (`crates/gyre-server/src/merge_processor.rs`):
   - Before processing an MR, retrieve the MR's attestation (if present) and check `meta_spec_set_sha`
   - Compare against the workspace's current meta-spec set SHA
   - If they differ, add a `[WARN] meta-spec-drift` entry to the MR's gate results
   - The warning is informational by default (does not block merge)

2. **Add workspace merge policy option**:
   - Add `require_current_meta_spec_set: bool` to workspace settings or merge policy
   - When true, meta-spec drift converts from warning to hard block (MR stays in queue until reconciled or overridden)
   - Default: false (warn only)

3. **Display drift warning in UI**:
   - MergeQueueGraph component should show drift warnings alongside other gate results
   - Warning badge: "Meta-spec drift: produced under v{old}, current is v{new}"

4. **Add API for workspace merge policy** (if not already covered by workspace settings):
   - This may be part of existing workspace settings — check before creating new endpoints

5. **Tests**:
   - Unit test: matching meta_spec_set_sha → no warning
   - Unit test: mismatched SHA → warning added to gate results
   - Unit test: `require_current_meta_spec_set: true` → hard block on mismatch
   - Unit test: MR without attestation → skip check (no crash)

## Acceptance Criteria

- [ ] Merge processor checks meta_spec_set_sha on MRs with attestations
- [ ] Mismatched SHA produces `[WARN] meta-spec-drift` in gate results
- [ ] Warning does not block merge by default
- [ ] `require_current_meta_spec_set: true` workspace setting converts warning to hard block
- [ ] MRs without attestation skip the check gracefully
- [ ] Tests cover match, mismatch, hard-block, and no-attestation cases

## Agent Instructions

- Read `crates/gyre-server/src/merge_processor.rs` for the merge queue processing loop — find where gate checks are evaluated
- Read `crates/gyre-server/src/gate_executor.rs` for how gate results are structured and reported
- Read `crates/gyre-domain/src/attestation.rs` for the MergeAttestation struct (meta_spec_set_sha field)
- Read `crates/gyre-server/src/constraint_check.rs` for existing constraint evaluation (meta_spec_set_sha constraint exists but may not be wired into gates)
- The meta_spec_set_sha is already captured in attestations — the check is comparing it against the workspace's current set SHA
- Follow the existing gate result pattern for adding the drift warning

## Shipped

**Drift evaluation (§9)** — `crates/gyre-server/src/reconciliation.rs`:
`mr_meta_spec_drift(state, mr)` resolves the MR's provenance attestation (by
source-branch head commit first, then the author agent's current task,
walking derived nodes to the chain's root `SignedInput`) and compares the
recorded `meta_spec_set_sha` against the workspace's current set SHA
(`compute_meta_spec_set_sha`). Returns no drift when the workspace has no
bound set, the MR has no attestation, or the attestation records an empty
SHA — all skip the check gracefully. `format_meta_spec_drift_warning`
produces the exact §9 text `[WARN] meta-spec-drift: code produced under
meta-spec set {old}, current is {new}. Reconciliation may be needed.` and
`parse_meta_spec_drift_warning` round-trips it for the queue graph.

**Merge-gate wiring (§9)** — `crates/gyre-server/src/merge_processor.rs`:
both merge paths (`process_next` single-entry and `merge_atomic_group`)
run the drift check before the transactional merge. Default (warn only):
a gate result with id `meta-spec-drift`, `GateStatus::Passed` and the §9
warning output is recorded on the MR (idempotent across retry cycles —
an existing result is updated in place, not duplicated) and the merge
proceeds. Strict policy (`require_current_meta_spec_set: true`): the entry
returns to `Queued` carrying the drift reason and stays there until the
code is reconciled under the current set or the policy is relaxed; every
atomic-group member is re-queued the same way.

**Workspace merge policy** — `crates/gyre-server/src/merge_policy.rs`:
`WorkspaceMergePolicy { require_current_meta_spec_set }` persisted through
the port-backed KV store (namespace `workspace_merge_policies`, key =
workspace id), default false. A corrupt stored policy surfaces as an error
on direct lookup and degrades to the spec default (warn only) inside the
merge processor so a policy-store outage cannot wedge the queue.
Exposed as `GET/PUT /api/v1/workspaces/{id}/merge-policy` (partial update;
PUT Admin-only, both handlers tenant-scoped; `crates/gyre-server/src/api/workspaces.rs`),
registered in the ABAC resolver (`abac_middleware.rs`) and documented in
`docs/api-reference.md`.

**UI** — `web/src/components/MergeQueueGraph.svelte`: nodes carry a ⚠
meta-spec drift badge (`data-drift`, `mq-drift-{mr_id}`) when the backend
recorded a drift warning, the hover tooltip shows "Meta-spec drift:
produced under v{old}, current is v{new}" (8-char SHA prefixes), and the
legend lists "Meta-spec drift (warn only)". The graph API
(`GET /api/v1/merge-queue/graph`, `crates/gyre-server/src/api/merge_deps.rs`)
surfaces the recorded drift per node by parsing the same gate result the
MR detail view reads, so the graph cannot disagree with gate results.

### Test evidence

- `cargo test -p gyre-server --lib meta_spec_drift` — 5 passed:
  warn-records-gate-result-and-merges, hard-block + policy-relax recovery,
  match-no-warning, no-attestation-skip, gate-result idempotency.
- `cargo test -p gyre-server --lib merge_policy` — 4 passed: KV default /
  roundtrip / corrupt-fallback + API test (default warn-only, Developer
  PUT 403 with no state change, Admin PUT roundtrips, unknown workspace
  404).
- `cargo test -p gyre-server --lib reconciliation::` — 24 passed
  (drift match / mismatch + §9 format round-trip / no-attestation / no-set /
  empty-sha / foreign-output parse rejection, plus the pre-existing
  reconciliation suite).
- `cargo test -p gyre-server --lib merge_processor` — 61 passed
  (54 task-159 + 7 task-077 trust-gate tests merged in from the assigned
  base), `merge_deps::` — 13 passed, `api::workspaces::` — 17 passed
  (merge-policy API test + task-077 trust-transition tests from the base).
- `cargo test -p gyre-server --lib` (full lib) — 1353 passed, 0 failed
  (merge-correctness probe after combining base a884329d7 with the branch).
- `npx vitest run src/__tests__/MergeQueueGraph.test.js` — 25 passed
  (20 pre-existing + 5 drift badge/tooltip/legend tests); the merge touched
  no web/ sources.
- Mechanical checks green: check-inert-enforcement, check-arch,
  check-abac-route-registry, check-abac-exempt-handlers (88 handlers),
  check-scope-literal-defaults, check-in-memory-state-stores,
  check-forged-scope-fields, check-migration-versions.

### Round note (review repair + base merge)

The backend, policy API, drift evaluation, UI, and tests shipped in
recovered checkpoint 333030908 + feat(task-159) commit abbcd4aad. This
round resolved the merge with the newly assigned base a884329d7 (merge
commit c17c22498): both conflicted test modules keep BOTH sides' suites —
the task-159 drift/merge-policy tests and the base's task-077 trust-gate
and trust-transition tests. The CI repair finding
(scripts/check-inert-enforcement.sh exiting 1 on the candidate because the
task-159 diff had shifted the exemption-pinned `evaluate_attestation_abac`
sites at merge_processor.rs:1457/:1479 to :1608/:1630) is resolved by the
base itself, not by new exemptions: the base restructured both call sites
to bind their result (`let attestation_eval = evaluate_attestation_abac(
...)` with a real Deny-effect branch) and deleted the two stale
merge_processor exemption entries — the exemption count went DOWN, the
F5 hazards are actually fixed. `bash scripts/check-inert-enforcement.sh`
now exits 0 on the merged tree. Focused evidence re-run after the merge:
5 / 4 / 61 / 13 / 24 / 17 / 1353 (lib full) / 25 (vitest) — logs retained
under /tmp/stage/review-evidence/task-159-*.log. The acceptance-criteria
checkboxes are left unchecked in the task body per the assigned contract;
the criteria themselves are satisfied by the shipped behavior above.
