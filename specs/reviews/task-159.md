# Review — task-159 (Wire meta-spec drift warnings into merge gates)

Spec: `specs/system/meta-spec-reconciliation.md` §9 "Merge Gate Behavior".
Candidate: `3d4fa2d5` (branch `pipeline/task-159/20ff89f8b73c404ca87ff81ba197c372-1`),
assigned base `603cc2f7`. This round is the **rebase repair** of the previously
approved implementation (head `5b85bc88`, PR 784, merge state DIRTY): merge
commit `74439bfa` merges current main `603cc2f7` (task-183 Architectural
Timeline) into the branch; `7aefa4e4`/`3d4fa2d5` touch only task notes.
Verdict: **approved** — no blocking findings; observations recorded below.

## Probes run (evidence in /tmp/stage/review-evidence/)

- `cargo check -p gyre-server` → exit 0 (8m 15s, via /tmp/stage/bin/cargo wrapper).
- `cargo check -p gyre-server --tests` → exit 0 (3m 47s) — the merged test code
  (incl. main's task-183 reconciliation tests and task-159's §9 tests) type-checks.
- `cd web && npm ci && npx vitest run src/__tests__/MergeQueueGraph.test.js` →
  **29/29 passed**, including the 5 new drift tests (badge on drifted nodes,
  `data-drift` attribute, tooltip row with "Meta-spec drift: produced under
  v{old}, current is v{new}", no-badge/no-tooltip-row without drift, legend entry).
  (The `npm ci` side effect of regenerating `web/dist/` was reverted with
  `git checkout -- web/dist && git clean -fd web/dist`; the working tree was
  left exactly as received.)
- Mechanical gates, all exit 0: `check-arch.sh`, `check-abac-route-registry.sh`
  (new `/api/v1/workspaces/:id/merge-policy` route is resolver-registered),
  `check-abac-exempt-handlers.sh`, `check-in-memory-state-stores.sh` (policy is
  port-backed KV, not a bare AppState map), `check-fail-open-ref-resolution.sh`,
  `check-forged-scope-fields.sh`, `check-inert-enforcement.sh`,
  `check-relative-path-defaults.sh`, `check-api-auth.sh`,
  `check-conditional-test-guards.sh`.
- `check-assertionless-tests.sh` and `check-comparative-test-claims-js.sh` exit 2
  with truncated output **identically at the assigned base** (verified in a
  throwaway worktree of `603cc2f7`) — pre-existing sandbox/script behavior, not
  a candidate defect.
- `check-task-commit-attribution.sh` fails in the working tree only because the
  executor's unstaged revert restored `commits: []`; at candidate HEAD
  `3d4fa2d5` the frontmatter lists all 11 attributed commits and all are
  ancestors of the candidate (the four product-surface commits the script
  flags — `bd834689`, `11a702b8`, `8a99bd80`, `abbcd4aa` — are all in the list).

## Merge-resolution verification (this round's specific risk)

`git diff 5b85bc88 74439bfa` (excluding `web/dist`) is **byte-identical** to
`git diff 4ec1e526 603cc2f7` (main's delta since the previous verified base)
except for five hunk-header line-number offsets in `reconciliation.rs`'s test
module, shifted by exactly the retained §9 block's +163 lines. I.e. main's
task-183 changes (wave labels, timeline, graph port changes) were applied in
full and the approved §9 implementation is preserved verbatim
(`META_SPEC_DRIFT_GATE_ID`/`mr_meta_spec_drift` present; `merge_processor.rs`,
`merge_policy.rs`, `api/workspaces.rs`, `abac_middleware.rs`, `api/mod.rs`
untouched by the merge). Post-merge commits touch only `specs/tasks/task-159.md`.

## Implementation verification (task acceptance criteria)

- **Drift evaluation is real and wired into the merge processor.**
  `reconciliation.rs` `mr_meta_spec_drift` resolves the MR's provenance
  attestation (source-branch head commit via `resolve_ref`, then the author
  agent's current task, walking derived nodes to the chain's root
  `SignedInput`) and compares the recorded `meta_spec_set_sha` against
  `compute_meta_spec_set_sha` of the workspace's *effective* set (tenant
  defaults merged with workspace overrides). Skip paths are graceful and
  spec-consistent: no bound set, no attestation, empty recorded SHA.
- **Warning format is exact**: `format_meta_spec_drift_warning` emits
  `[WARN] meta-spec-drift: code produced under meta-spec set {old}, current is {new}. Reconciliation may be needed.`
  recorded as a gate result (`META_SPEC_DRIFT_GATE_ID`, `GateStatus::Passed` —
  `GateStatus` has no Warn variant, and Passed is what keeps it non-blocking),
  idempotent across retry cycles (existing result updated in place).
- **Warn-only default is enforced structurally**: the warning path never
  returns a hold; blocking is decided solely by the workspace policy lookup.
- **Strict mode is real enforcement**: `selection_meta_spec_drift_hold` runs
  inside the `process_next` candidate-selection loop (same slot as the
  TASK-077 trust hold), requeues the entry `Queued` with the drift reason, and
  `continue`s to the next candidate in the same cycle — so a strictly-blocked
  head-of-queue entry cannot starve the shared queue (regression tests cover
  same-workspace, cross-workspace, and atomic-group-held-as-unit cases).
  Atomic-group candidates are expanded to all group members, and
  `merge_atomic_group` is only reachable after the selection loop, so no merge
  path bypasses the check.
- **Policy storage and API**: `merge_policy.rs` persists
  `WorkspaceMergePolicy { require_current_meta_spec_set }` through the
  port-backed KV store (default false; corrupt value errors on direct lookup
  and degrades to the spec default inside the processor, logged).
  `GET/PUT /api/v1/workspaces/:id/merge-policy`: PUT Admin-only + tenant
  containment; GET tenant-scoped; route registered in the ABAC resolver;
  documented in `docs/api-reference.md`.
- **UI**: `merge_deps.rs` surfaces a recorded drift gate result as a
  `meta_spec_drift` node field (parsed from the same gate results the MR
  detail reads, so graph and gates cannot disagree); `MergeQueueGraph.svelte`
  renders the ⚠ badge + tooltip + legend entry.
- **Tests are hard, not self-confirming**: the merge-processor tests drive the
  real `process_next` pipeline against mem storage and assert observable
  state (MR `Merged`/`Open`, entry status + reason text, gate-result presence
  and exact output incl. the literal `[WARN] meta-spec-drift: ` prefix). The
  hard-block test also verifies policy-relax recovery. The API test drives the
  real router with JWTs and asserts 403 for a Developer-role caller, 404s,
  default, and round-trip. Removing the drift check, the strict block, or the
  warning format each fails at least one assertion.

## Server tests the verifier must execute

(Sandbox budget: server test-binary codegen exceeds 4 GiB with the model
resident; `cargo check --tests` was the in-sandbox bound.)

    cargo test -p gyre-server --lib reconciliation::tests::drift_
    cargo test -p gyre-server --lib reconciliation::tests::drift_warning_parse
    cargo test -p gyre-server --lib merge_processor::tests::meta_spec_drift
    cargo test -p gyre-server --lib merge_processor::tests::strict_drift_hold
    cargo test -p gyre-server --lib merge_policy::tests
    cargo test -p gyre-server --lib api::workspaces::tests::merge_policy_default_admin_only_update_and_roundtrip

plus the verifier's standard full suite, clippy, `npm run build` + dist
commit, and the mechanical gates.

## Observations (non-blocking)

1. **Spec-model divergence, recorded for the spec-fidelity auditor.** Canonical
   §9 (Draft) sketches `MetaSpecPolicy { warn_on_drift, block_on_drift,
   drift_tolerance }` — a version-distance model ("how many versions behind").
   The assigned task contract instead specifies `require_current_meta_spec_set:
   bool`, and the implementation follows the task (as does the pre-task
   coverage row 13, which framed the gap as "No require_current_meta_spec_set
   workspace setting"). The platform's meta-spec sets are SHA-identified with
   no version ordering (§8/task-158), so `drift_tolerance` has no representable
   analog; warn-always matches `warn_on_drift`'s default and the strict flag is
   `block_on_drift`. If the MetaSpecPolicy triple is still wanted, amend §9 via
   spec lifecycle rather than reopening this task.
2. **`get_queue_graph`'s `meta_spec_drift` field wiring has no direct Rust
   endpoint test** (seed a drift gate result, assert the node field). The
   parser is unit-tested (round-trip + foreign-output rejection) and the
   component rendering is tested; the wiring itself is a 10-line
   find→parse→map. Minor coverage gap only.
3. **Generated output (verifier-owned, per checkpoint rules):** the committed
   `web/dist/` at the candidate reflects the assignment-base build, not the
   merged source — the verifier rebuilds remotely after integration, runs all
   checks, and binds publication to that tested tree. Not a source defect.
4. **Pre-existing, out of scope:** `PUT /api/v1/merge-requests/:id/status` can
   transition an MR to `Merged` directly without queue processing (bypassing
   all queue gates, drift included). This path exists at the assigned base and
   is untouched by this task; §9 scopes the check to the merge queue
   processor. Noted in case a later task wants the direct-merge path gated.
5. Two lint scripts (`check-assertionless-tests.sh`,
   `check-comparative-test-claims-js.sh`) exit 2 with truncated output on this
   sandbox **identically at base** — infrastructure/script behavior, not a
   candidate regression.
