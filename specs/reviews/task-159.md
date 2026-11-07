# Review — task-159 (Wire meta-spec drift warnings into merge gates)

Spec: `specs/system/meta-spec-reconciliation.md` §9 "Merge Gate Behavior" (task excerpt contract: `require_current_meta_spec_set` strict-mode switch).
Candidate under review: `965fdc1e6` (branch `pipeline/task-159/6d882c3e6e464d929755e19501b3e9e7-1`), base `233664cf8`; task commits `333030908`, `abbcd4aad`, `8a99bd808` (repair round: strict-policy drift block evaluated during candidate selection instead of post-selection early-return).
Verdict: **approved** (round 2 — the round-1 starvation finding is repaired and regression-covered).

## Probes run (evidence: /tmp/stage/review-evidence/task-159-*.log)

- `cargo test -p gyre-server --lib meta_spec_drift` → 5 passed (warn-records-gate-result-and-merges, hard-block + policy-relax recovery, match-no-warning, no-attestation-skip, gate-result idempotency).
- `cargo test -p gyre-server --lib strict_drift_hold` → 2 passed (same-workspace and cross-workspace non-starvation behind a strictly-blocked drifted head; drift-held atomic group does not starve a later independent entry).
- `cargo test -p gyre-server --lib merge_policy` → 4 passed (KV default / roundtrip / corrupt-fallback + API test: default warn-only, Developer PUT 403 with no state change, Admin PUT roundtrip, unknown workspace 404).
- `cargo test -p gyre-server --lib merge_processor::` → 63 passed (selection loop changed; trust-hold/atomic-group suites included).
- `cargo test -p gyre-server --lib reconciliation::` → 24 passed; `merge_deps::` → 13 passed.
- `npx vitest run src/__tests__/MergeQueueGraph.test.js` → 25 passed (node_modules installed via npm ci by build.rs during the cargo run).
- Mechanical: check-abac-route-registry, check-arch, check-inert-enforcement, check-forged-scope-fields, check-in-memory-state-stores, check-task-commit-attribution → all exit 0.
- **Kill test** (evidence: task-159-kill-test.md): temporarily disabled `selection_meta_spec_drift_hold` (forced `Ok(None)`) → 4/7 focused tests fail (warn-recording, hard-block, both starvation regressions), proving the tests are load-bearing rather than self-confirming; the starvation tests fail because the drifted head then merges. File restored, tree verified clean (`git status --porcelain` empty), pristine re-run green.

## Verified against the task contract

- **Drift evaluation is real**: `mr_meta_spec_drift` (reconciliation.rs) resolves the MR's provenance attestation by source-branch head commit (`git_refs::resolve_ref` + `find_by_commit`), then by the author agent's current task (`find_by_task`, max chain_depth), walking derived nodes to the chain's root `SignedInput`, and compares the recorded `meta_spec_set_sha` against `compute_meta_spec_set_sha` for the workspace. Graceful skips tested: no bound set, no attestation, empty recorded SHA.
- **§9 warning in gate results**: exact `[WARN] meta-spec-drift: code produced under meta-spec set {old}, current is {new}. Reconciliation may be needed.` text, recorded as a `GateResult` (gate id `meta-spec-drift`, `GateStatus::Passed` — informational, cannot block `check_gates_for_mr`), idempotent across retry cycles (existing result updated in place, not duplicated — directly tested).
- **Warn only by default**: mismatched SHA records the warning and the MR merges (asserted `MrStatus::Merged`).
- **Strict policy hard-block**: `WorkspaceMergePolicy { require_current_meta_spec_set }` KV-backed (namespace `workspace_merge_policies`, port-backed store — no in-memory state-store violation), default false, corrupt-store degrades to warn-only inside the processor while erroring on direct lookup. Under strict policy the entry returns to `Queued` with the drift reason, MR stays `Open`, no Passed warn gate result is recorded on the block path, and relaxing the policy lets the same entry merge next cycle (tested end-to-end).
- **Starvation repair confirmed**: the drift+policy evaluation runs during candidate selection (step 4c'' in `process_next`, mirroring the task-077 trust-hold precedent), requeue-with-reason and `continue` — the loop iterates the pre-fetched topologically-sorted list, so one cycle always terminates and later entries (same workspace, other workspaces, and entries behind a drift-held atomic group) merge in the same cycle. Atomic-group candidates expand to all group members so a drifted member holds the whole group as a unit.
- **Policy API**: `GET/PUT /api/v1/workspaces/{id}/merge-policy`, PUT Admin-only, both handlers tenant-scoped with 404 masking for foreign-tenant workspaces (same pattern as neighboring workspace handlers), registered in the ABAC resolver (`abac_middleware.rs` RouteResourceMapping), documented in `docs/api-reference.md`.
- **UI**: `MergeQueueGraph.svelte` ⚠ drift badge (`data-drift`, `mq-drift-{mr_id}`), tooltip "Meta-spec drift: produced under v{old}, current is v{new}" (8-char prefixes), legend entry — positive and negative vitest tests. The graph API surfaces the recorded drift by parsing the same gate result the MR detail view reads, so graph and gate results cannot disagree.

## Non-blocking notes (recorded, no action required)

- The canonical spec §9 sketches a richer `MetaSpecPolicy { warn_on_drift, block_on_drift, drift_tolerance }`; the implementation follows the task's assigned contract instead (single `require_current_meta_spec_set` boolean, block on any drift — the spec's `drift_tolerance: 0` behavior). The task excerpt is the assigned contract for this review; if the fuller policy shape is ever wanted it is a new task, not a defect here.
- Atomic-group membership is matched by group name only (no workspace predicate) in `selection_meta_spec_drift_hold` — this exactly mirrors the pre-existing `atomic_group_ready` / `merge_atomic_group` group semantics (same filter at merge_processor.rs:227/385/605), so it introduces no new conflation.
- The warn-only gate result is recorded during candidate selection even if the merge subsequently fails for unrelated reasons; it is informational, idempotent, and matches "the drift is recorded" in §9.
