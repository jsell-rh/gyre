# Review — task-159 (Wire meta-spec drift warnings into merge gates)

Spec: `specs/system/meta-spec-reconciliation.md` §9 Merge Gate Behavior.
Candidate: `d588f24` (base `f764bc00`); product commits `abbcd4aa`, `8a99bd80`, `11a702b8`, `bd834689` plus docs/process/merge-history commits, all recorded in the task `commits:` frontmatter.
Verdict: **approved** — no findings. Rust behavioral test execution and remote CI are deferred to the independent verifier (see must-run list).

## Probes run

- Full diff inspection base..candidate (all 13 files; production logic in `reconciliation.rs`, `merge_processor.rs`, `merge_policy.rs`, `api/workspaces.rs`, `api/merge_deps.rs`, `MergeQueueGraph.svelte`).
- `cargo check -p gyre-server --tests` → pass (8m34s, cold; type-checks all new test code). Build-wrapper side effect on `web/dist` reverted.
- `cd web && npm ci && npx vitest run src/__tests__/MergeQueueGraph.test.js` → 29/29 pass, including the 4 new drift tests (badge present/absent, tooltip text, legend).
- Mechanical gates on the candidate: `check-arch.sh`, `check-abac-route-registry.sh`, `check-abac-exempt-handlers.sh`, `check-mem-port-contracts.sh`, `check-in-memory-state-stores.sh`, `check-byte-slice-truncation.sh`, `check-forged-scope-fields.sh`, `check-migration-versions.sh` → all pass. `check-task-commit-attribution.sh` passes on the committed tree (the working-tree failure is an artifact of the review stage's own unstaged reset of `specs/tasks/task-159.md` to `commits: []`).

## Verified working (no findings)

- **Drift evaluation is real.** `mr_meta_spec_drift` (reconciliation.rs) resolves the MR's provenance attestation (source-branch head commit via `resolve_ref`, then the author agent's current task, walking derived nodes to the chain's root `SignedInput`), compares the recorded `meta_spec_set_sha` against `compute_meta_spec_set_sha` (tenant+workspace effective set). No bound set / no attestation / empty recorded SHA → graceful skip.
- **Gate wiring is in the real merge path.** `selection_meta_spec_drift_hold` runs in the `process_next` selection loop (step 4c'', after the trust hold), expanded to whole atomic groups. Warn-only (default): records a gate result (id `meta-spec-drift`, `GateStatus::Passed`, exact §9 text `[WARN] meta-spec-drift: code produced under meta-spec set {old}, current is {new}. Reconciliation may be needed.`), idempotently across retry cycles (update-in-place), and the merge proceeds. Strict (`require_current_meta_spec_set: true`): the entry is requeued Queued with the drift reason and the loop continues to the next candidate in the same cycle — I verified against the mem adapter that `update_status` does not rotate `enqueued_at`, so the during-selection placement is required to avoid head-of-queue starvation of later entries (same and other workspaces); the two starvation regression tests cover exactly this, including atomic groups held as a unit.
- **Policy + API.** `WorkspaceMergePolicy` is KV-store-backed (port-backed, not an in-memory AppState alias — `check-in-memory-state-stores.sh` passes), default false. `GET/PUT /api/v1/workspaces/:id/merge-policy`: PUT Admin-only with per-handler workspace-tenant ownership checks (stricter than the sibling rollout-policy GET), resolver-registered in `abac_middleware.rs`, no caller-supplied scope fields. Corrupt stored policy surfaces as an error on the API and degrades to the spec default (warn only) inside the processor — a documented, spec-aligned degradation.
- **UI.** Queue graph nodes carry `meta_spec_drift` parsed from the recorded gate result (same source as the MR detail view, so graph and gate results cannot disagree); `MergeQueueGraph.svelte` renders the ⚠ badge, tooltip "Meta-spec drift: produced under v{old}, current is v{new}", and a legend entry. Frontend tests executed: 29/29.
- **Docs/coverage.** `docs/api-reference.md` rows added; coverage row 13 → implemented with landed evidence; header counts verified consistent (7 n/a + 4 task-assigned + 7 implemented = 18 rows).
- **Test quality.** The merge-processor tests drive the real `process_next` with real attestations, bound sets, and KV policy; the hard-block test proves the flag gates behavior (MR stays Open under strict policy, merges after the policy is relaxed, and the blocked path records no Passed warn result). Match/no-attestation/idempotency/starvation/group cases all present. No self-confirming or assertionless tests found.

## Notes (non-blocking)

- `GateStatus` has no Warn variant; `Passed` + the literal `[WARN]` output is the only non-blocking realization — reasonable.
- Canonical §9's `MetaSpecPolicy` sketch (`warn_on_drift`/`drift_tolerance`) is not implemented; the assigned task excerpt (SHA match + `require_current_meta_spec_set`, block on any mismatch) is the contract, and version-distance tolerance is not computable from set SHAs. No spec contradiction shipped.
- Generated `web/dist/`: committed dist is byte-identical between base and candidate (verifier owns rebuild/publication); my `cargo check` probe's build.rs regeneration was reverted — working tree left as found.

## Verifier must-run (blocked in review stage by the build wrapper)

`cargo test -p gyre-server` — specifically: `meta_spec_drift_records_warn_gate_result_and_merge_proceeds`, `meta_spec_drift_hard_blocks_under_strict_workspace_policy`, `meta_spec_drift_matching_set_sha_merges_without_warning`, `meta_spec_drift_mr_without_attestation_skips_check`, `meta_spec_drift_warning_gate_result_is_idempotent`, `strict_drift_hold_does_not_starve_shared_merge_queue`, `strict_drift_hold_atomic_group_does_not_starve_later_entry` (merge_processor); `drift_check_*` and `drift_warning_parse_rejects_foreign_output` (reconciliation); the three `merge_policy` tests; `merge_policy_default_admin_only_update_and_roundtrip` (api::workspaces). Also confirm remote CI for the current head (durable finding on prior head `ceca6de2`: Development controller check failed, run 43464155926).

Evidence: `/tmp/stage/review-evidence/` (review-summary.md, candidate.diff, probe outputs).
