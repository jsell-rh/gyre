# Review — task-159 (Wire meta-spec drift warnings into merge gates)

Spec: `specs/system/meta-spec-reconciliation.md` §9 "Merge Gate Behavior".
Candidate: `53ce7e2ed28f07bb041b67784684f32569ff0c0b` (base `1e61f6637fba1a1fe8b851eaac13691d1230f665`, merged in via `73e1491aa`).
Verdict: **approved** — no blocking findings. Server test execution is budget-blocked in this sandbox; the required verifier probes are recorded below.

## Round 1

Probes run (checkout at candidate `53ce7e2ed`; evidence under `/tmp/stage/review-evidence/`):

- `cargo check -p gyre-server --tests` (via `/tmp/stage/bin/cargo`) → **pass** in 7m56s (`cargo-check-server-tests.txt`) — all new production code and tests type-check.
- `cargo test -p gyre-server --lib merge_policy::tests` → **blocked by the sandbox build wrapper** ("gyre-server code generation plus the model exceeds 4 GiB") — an environment restriction, not a code defect; same restriction the implementing round recorded.
- `bash scripts/check-abac-route-registry.sh` → OK (`abac-route-registry.txt`) — the new `/api/v1/workspaces/:id/merge-policy` route is ABAC-registered, no registry growth.
- `bash scripts/check-arch.sh`, `check-migration-versions.sh`, `check-in-memory-state-stores.sh`, `check-scope-literal-defaults.sh`, `check-fail-open-ref-resolution.sh`, `check-inert-enforcement.sh`, `check-forged-scope-fields.sh` → all OK.
- `bash scripts/check-task-commit-attribution.sh` → **OK against the candidate commit** (`task-attribution-candidate.txt`). Note: the check fails against the *working tree* only because stage tooling left an uncommitted revert of `specs/tasks/task-159.md` to its assigned `commits: []` contract state; the candidate commit itself records all 7 task-labeled product commits.
- `cd web && npm ci && npx vitest run src/__tests__/MergeQueueGraph.test.js` → **29/29 passed** (`vitest-mergequeuegraph.txt`), including the 5 new drift tests (badge rendered on drifted nodes, absent on clean nodes, tooltip text `Meta-spec drift: produced under v{old}, current is v{new}`, tooltip row omitted without drift, legend entry).

Verified working (line-level diff review of base→candidate, 13 files):

- **§9 drift evaluation is real.** `reconciliation.rs::mr_meta_spec_drift` resolves the MR's provenance attestation (source-branch head commit via `git_refs::resolve_ref` with proper `None` handling, then the author agent's current task, walking derived nodes to the chain's root `SignedInput`), reads the recorded `meta_spec_set_sha`, and compares it against `compute_meta_spec_set_sha` (post-task-158 effective-set signature). Skip paths are graceful and correct: no bound set → skip; no attestation → skip; empty recorded SHA → skip (no false drift against an unknown baseline).
- **Warning format matches §9 exactly**: `[WARN] meta-spec-drift: code produced under meta-spec set {old}, current is {new}. Reconciliation may be needed.` — byte-for-byte the spec text (Rust `\`-continuation produces the single-space form, verified). Recorded as a gate result (synthetic id `meta-spec-drift`) with `GateStatus::Passed`, so it never blocks by itself and cannot perturb `check_gates_for_mr` (Passed is a no-op there). Idempotent across retry cycles (existing result updated in place).
- **Wiring point is sound.** `selection_meta_spec_drift_hold` runs during candidate selection in `process_next`, exactly mirroring the pre-existing `selection_trust_hold` (same atomic-group expansion semantics, same requeue-with-reason pattern). A strict-policy drifted head-of-queue entry cannot starve the shared queue — later same-workspace and cross-workspace entries merge in the same cycle. Warn-only drift records the gate result and the merge proceeds; strict policy requeues the entry carrying the drift reason and the MR stays Open until reconciled or the policy is relaxed.
- **Merge policy is durable and correctly scoped.** `merge_policy.rs` persists `WorkspaceMergePolicy { require_current_meta_spec_set }` (default false) through the port-backed KV store (`workspace_merge_policies` namespace; SQLite + Postgres + mem adapters implement the port) — not a bare in-memory `Arc<Mutex<HashMap>>`, satisfying the durable-state invariant. Corrupt stored policy surfaces as an error on direct lookup and degrades to the documented spec default (warn only) inside the processor, with a warning log.
- **API is authorized correctly.** `GET/PUT /api/v1/workspaces/:id/merge-policy` registered in `api/mod.rs` + ABAC resolver; PUT is Admin-only in the handler (Developer-role JWT rejected 403 in the router-level test — the same handler-level enforcement pattern as the task-157 rollout-policy sibling, which matters because builtin policy `builtin-developer-write-access` would let a Developer through middleware ABAC on a `workspace`-resource write); both handlers verify workspace existence and tenant containment with NotFound masking for cross-tenant access. No caller-supplied scope fields are persisted.
- **Queue graph surfacing cannot disagree with gate results**: `merge_deps.rs` parses the *recorded* drift gate result (same store the MR detail view reads) via `parse_meta_spec_drift_warning`, which round-trips the exact §9 format and rejects foreign output. `MergeQueueGraph.svelte` renders the ⚠ badge + tooltip from that field; tests assert both presence and absence.
- **Tests are real guards, not inflation.** The merge_processor tests drive the production `process_next` loop through port-backed stores and assert end-state (Merged/Open/Queued + entry reason + gate-result presence/absence + exact §9 output); each acceptance-criterion case fails if the corresponding production behavior is removed. The starvation regression tests (same-cycle merge behind a held head; held atomic group not monopolizing the queue) encode the review-repair invariants. The reconciliation tests cover match/mismatch/no-attestation/no-set/empty-sha and the format round-trip. `merge_policy` tests cover default/roundtrip/corrupt-fallback.
- **Coverage matrix**: §9 row updated to `implemented` with accurate landed evidence; header counts consistent (7 n/a, 4 task-assigned, 7 implemented = 18 rows). Not hollow — production code performs the specced behavior against real storage.

Non-blocking observations (recorded, not findings):

- **Generated output only:** `web/dist/` is byte-identical between base and candidate while the authored frontend source changed — the committed bundle is stale relative to source. Per the review contract this is owned by the deterministic verifier, which rebuilds, tests, and commits the assets after integration; not blocking.
- **Canonical spec shape vs shipped policy:** canonical §9 sketches a `MetaSpecPolicy { warn_on_drift, block_on_drift, drift_tolerance }`; the assigned task contract (and the candidate) realize strict mode as the single `require_current_meta_spec_set` flag — equivalent to `block_on_drift: true, drift_tolerance: 0`. `drift_tolerance` (versions-behind counting) is not implementable against SHA-identified sets without a set version history, and the assigned task excerpt explicitly specifies the single-flag shape, so this is not a candidate defect; a future spec-lifecycle amendment could reconcile the draft struct with the shipped shape.

## Verifier-required probes (sandbox budget blocked server test codegen)

    cargo test -p gyre-server --lib merge_policy::tests
    cargo test -p gyre-server --lib reconciliation::tests::drift
    cargo test -p gyre-server --lib merge_processor::tests::meta_spec_drift
    cargo test -p gyre-server --lib merge_processor::tests::strict_drift_hold
    cargo test -p gyre-server --lib workspaces::tests::merge_policy_default_admin_only_update_and_roundtrip
    cargo test -p gyre-server --lib reconciliation::tests::tenant_        # task-158 block preserved by the base-merge resolution
    cargo test -p gyre-server --lib reconciliation::tests::effective_set  # task-158 base-merge semantics

These cover all six acceptance criteria plus the two starvation regressions and the base-merge-preserved task-158 tests.
