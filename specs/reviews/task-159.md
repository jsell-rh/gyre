# Review — task-159 (Wire meta-spec drift warnings into merge gates)

Spec: `specs/system/meta-spec-reconciliation.md` §9 "Merge Gate Behavior".
Candidate under review: `bac689df0` (base `44a01390c`, diff: 13 files, +1777/−7).
Verdict: **approved** — every acceptance criterion is met by real production code; no findings.

## Probes run (evidence: /tmp/stage/review-evidence/)

- `SKIP_WEB_BUILD=1 cargo check -p gyre-server --tests` → **exit 0** (7m32s, cold build, wrapper-bounded). Type-checks the candidate's server crate including all new test code.
- `cd web && npm ci` → exit 0; `npx vitest run src/__tests__/MergeQueueGraph.test.js` → **29/29 passed** (5 new §9 drift tests: badge on drifted node, no badge on clean nodes, tooltip text `Meta-spec drift: produced under v{old}, current is v{new}`, tooltip row omitted without drift, legend entry).
- Mechanical invariant scripts, all **exit 0**: check-arch, check-abac-route-registry (new `/api/v1/workspaces/:id/merge-policy` route is ABAC-registered), check-abac-exempt-handlers (75), check-mcp-write-tools, check-migration-versions, check-in-memory-state-stores (KV-port-backed policy store, no bare HashMap), check-scope-literal-defaults, check-fabricated-scope-defaults, check-fail-open-ref-resolution, check-inert-enforcement, check-byte-slice-truncation, check-relative-path-defaults, check-dead-message-kinds, check-mem-port-contracts, check-lossy-secret-conversion, check-forwarded-header-trust, check-forged-scope-fields, check-unbounded-external-http, check-migration-sql-portability.
- `check-task-commit-attribution.sh` → **exit 0 against the candidate's committed task file** (the working-tree failure is the executor's standard restore of the assigned contract with `commits: []`; all four flagged product-surface SHAs — bd834689, 11a702b8, 8a99bd80, abbcd4aa — are present in the candidate's frontmatter).

## Verified against the task contract

- **Drift evaluation is real** (`reconciliation.rs`): `mr_meta_spec_drift` resolves the MR's provenance attestation via the same two-path lookup the merge-time attestation-ABAC gate uses (source-branch head commit via `git_refs::resolve_ref`, then the author agent's current task, walking derived nodes to the chain's root `SignedInput`), then compares the recorded `meta_spec_set_sha` against `compute_meta_spec_set_sha` (the effective set: tenant defaults merged with workspace overrides). Skips gracefully on: no bound set, no attestation, empty recorded SHA — all three covered by direct unit tests.
- **§9 warning format is exact**: `format_meta_spec_drift_warning` emits `[WARN] meta-spec-drift: code produced under meta-spec set {old}, current is {new}. Reconciliation may be needed.`; `parse_meta_spec_drift_warning` round-trips it and rejects foreign/empty-SHA outputs (unit-tested).
- **Gate-result wiring is correct** (`merge_processor.rs`): `record_meta_spec_drift_warning` records a `GateResult` with gate id `meta-spec-drift`, `GateStatus::Passed`, and the §9 output — informational, idempotent across retry cycles (existing result updated in place; unit-tested). `GateStatus` has no Warning variant, so `Passed` + WARN output is the codebase's only faithful representation; `gate_executor::check_gates_for_mr` treats unknown gate ids as required but `Passed` never blocks, so the synthetic gate id cannot interfere with real gate evaluation.
- **Warn-doesn't-block is proven end-to-end**: `meta_spec_drift_records_warn_gate_result_and_merge_proceeds` runs the real `process_next` on a drifted MR (builtin ABAC policies seeded exactly as `main.rs` does at startup) and asserts both the recorded gate result AND `MrStatus::Merged`.
- **Strict policy hard-blocks** (`merge_policy.rs` + selection wiring): `WorkspaceMergePolicy { require_current_meta_spec_set }` is KV-store-backed (port-backed, durable — in-memory-state-store check passes), default false. `selection_meta_spec_drift_hold` runs during candidate selection in `process_next` (step 4c'', mirroring the `selection_trust_hold` pattern): under strict policy the entry is requeued `Queued` with the drift reason and the processor continues to the next candidate in the SAME cycle — a drifted head-of-queue entry cannot starve the shared queue, in its own workspace or others. The hard-block test asserts entry stays Queued with reason, MR stays Open, no warn gate result is recorded, and **relaxing the policy lets the same entry merge on the next cycle**. Two starvation regression tests cover the shared queue (cross-workspace and same-workspace clean MRs) and atomic groups (all members held as a unit — both member entries carry the reason via separate selection-loop iterations, matching the code).
- **API is properly guarded** (`api/workspaces.rs`): GET/PUT `/api/v1/workspaces/:id/merge-policy`, PUT Admin-only with tenant containment (404 on cross-tenant), partial update, registered in the ABAC resolver. The router-level test drives the real `api_router()` via oneshot: default false, Developer-JWT PUT → 403 with no state change, Admin PUT → 200 and persisted, unknown workspace → 404. The GET's tenant check is stricter than the sibling rollout-policy GET (which has none).
- **UI chain is complete**: gate result → `merge_deps.rs` graph node field `meta_spec_drift` (parsed from the recorded gate result, so the graph cannot disagree with gate results) → `WorkspaceHome.svelte` passes `graph.nodes` through unchanged → `MergeQueueGraph.svelte` renders the ⚠ badge (`data-drift`, `mq-drift-{id}`), tooltip row, and legend entry. All verified by the executed vitest suite.
- **Coverage matrix** row 13 → `implemented | task-159` with notes that match the verified code; header counts (7/11, 7 n/a, 4 task-assigned, 7 implemented) are internally consistent.

## Test-inflation check

None found. The merge-processor tests drive the real `process_next` against mem-backed production stores and assert on persisted state (MR status, queue-entry status/reason, gate results); each would fail if the corresponding production behavior were removed: deleting `selection_meta_spec_drift_hold` fails the warn test's `expect("drift gate result must be recorded")`; making drift block by default fails the `Merged` assertions; breaking the strict hold fails the `Queued`/`Open` assertions. The reconciliation unit tests call `mr_meta_spec_drift` directly with real fixtures. Fixtures mirror the pre-existing, passing `guided_workspace_merge_proceeds_with_startup_seeded_builtins` pattern (including the production-parity `seed_builtin_policies` call that `main.rs:55` performs).

## Deferred to the independent verifier (server test execution — build-wrapper policy)

The task's own recorded list omits three test groups; the verifier must run all of:

    cargo test -p gyre-server --lib reconciliation::tests::drift_
    cargo test -p gyre-server --lib reconciliation::tests::drift_warning_parse
    cargo test -p gyre-server --lib merge_processor::tests::meta_spec_drift
    cargo test -p gyre-server --lib merge_processor::tests::strict_drift_hold
    cargo test -p gyre-server --lib merge_policy::tests
    cargo test -p gyre-server --lib api::workspaces::tests::merge_policy
    cargo test -p gyre-server --lib reconciliation::tests::tenant_
    cargo test -p gyre-server --lib reconciliation::tests::effective_set
    cargo test -p gyre-server --lib reconciliation::tests::workspace_get_meta_spec_set

plus the standard post-integration suite (`cargo test --all`, `cd web && npm test`, clippy, script gates). Note `strict_drift_hold` and `api::workspaces::tests::merge_policy` are the two groups the task file's deferred list missed.

## Non-blocking observations (recorded for the auditor / future spec work)

- **Canonical-spec divergence (deliberate, task-governed):** §9's draft sketch shows `MetaSpecPolicy { warn_on_drift, block_on_drift, drift_tolerance }`. The shipped realization follows the assigned task contract's `require_current_meta_spec_set` boolean: warn is always on (≙ `warn_on_drift: true`), strict mode blocks on ANY drift (≙ `drift_tolerance: 0`), and there is no way to disable warnings or set a tolerance. The assigned task document is the governing decomposition (its spec excerpt and acceptance criteria specify exactly this), and the spec header still reads "Status: Draft"; all task acceptance criteria are met. If tolerance granularity is ever wanted, it needs a spec-lifecycle amendment, not silent code.
- `merge_policy_or_default` degrades to the spec default (warn only, logged) on a policy-store outage. Defensible: the spec's opinionated default IS "warn always, block never", and wedging every merge on a KV blip would be worse; the direct API lookup still surfaces corrupt policies as errors (tested).
- No direct server-side test asserts the graph endpoint's `meta_spec_drift` node field end-to-end (the parser is unit-tested and the field is 3 lines of wiring reading the same gate results the MR detail view reads). The acceptance criteria do not require one.
- The coverage row's phrase "merge_atomic_group §9 gate" is slightly imprecise — `merge_atomic_group` carries only a pointer comment; the gate is evaluated during candidate selection, which the row's body also states. Substance is accurate.
- Working-tree note: this review ran with the executor-restored assigned contract in `specs/tasks/task-159.md` (`commits: []`); the candidate's committed task file conforms to the durable contract-repair finding (notes under `## Shipped` with `###` subheadings, 8-SHA commit attribution verified).
