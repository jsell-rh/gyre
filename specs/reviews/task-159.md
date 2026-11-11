# Review — task-159 (Wire meta-spec drift warnings into merge gates)

Spec: `specs/system/meta-spec-reconciliation.md` §9 "Merge Gate Behavior".
Candidate under review: `94b456b1` (base `cba416b4`, branch
`pipeline/task-159/b0d9f60a14c64d2cab6e1e5851a63751-1`).
Verdict: **approved** — no findings. Independent evidence supports the task
contract; the server-test execution deferred to the independent verifier is
recorded below and in `/tmp/stage/review-evidence/verifier-probes.md`.

## Probes run (review sandbox, candidate HEAD checked out)

- `cargo check -p gyre-server` → exit 0 (7m55s cold; evidence:
  `cargo-check-server.txt`).
- `cargo check -p gyre-server --tests` → exit 0 (3m13s; all new test code
  type-checks; evidence: `cargo-check-server-tests.txt`).
- `cargo test -p gyre-server --lib drift` → **blocked by the review
  sandbox's 4 GiB cargo wrapper** ("Server builds/tests belong to the
  independent verifier"). Infrastructure restriction, not a code defect;
  required test list recorded in `verifier-probes.md`.
- `cd web && npm ci && npx vitest run src/__tests__/MergeQueueGraph.test.js`
  → **29/29 passed**, including the 5 new drift tests (evidence:
  `vitest-mergequeuegraph.txt`).
- Invariant scripts, all exit 0: `check-arch.sh`, `check-abac-route-registry.sh`,
  `check-abac-exempt-handlers.sh`, `check-mem-port-contracts.sh`,
  `check-mcp-write-tools.sh`, `check-task-commit-attribution.sh`,
  `check-fail-open-ref-resolution.sh`, `check-inert-enforcement.sh`,
  `check-no-em-dash.sh` (evidence: `invariant-scripts.txt`).
- `scripts/ci-install-mold.sh` stub-environment simulations, both outcomes:
  network-failure path retries 3x, emits `::warning::mold unavailable`,
  removes `.cargo/config.toml`, exits 0; healthy-apt path installs, keeps the
  config, exits 0 (evidence: `mold-script-*.txt`). Verified
  `.cargo/config.toml` contains only the mold/clang linker pin, so the
  fallback cannot change build semantics beyond the linker.
- Full diff inspection `cba416b4..94b456b1` (16 files, +1834/−11); the 11
  deleted lines are workflow mold steps, an import-list expansion, docs
  rows, and coverage-matrix/task-frontmatter updates — no pre-existing test
  or production behavior was weakened.

## Verified working (no findings)

- **§9 drift evaluation is real.** `mr_meta_spec_drift` (reconciliation.rs)
  resolves the MR's provenance attestation exactly the way the existing
  merge-time attestation-ABAC gate does (source-branch head commit via
  `resolve_ref`, then the author agent's current task with
  `max_by_key(chain_depth)`, walking derived nodes to the chain root
  `SignedInput`), and compares the recorded `meta_spec_set_sha` against
  `compute_meta_spec_set_sha` — the same function that populates the field
  at authorization time (constraint_check.rs:1246), so equality is
  meaningful. Skips are graceful and correct: no bound set, no attestation,
  empty recorded SHA.
- **Warning is wired into gate results, warn-only by default.**
  `record_meta_spec_drift_warning` persists a `meta-spec-drift` gate result
  (idempotent: update-in-place on retry cycles) whose output is the exact
  §9 task-contract text `[WARN] meta-spec-drift: code produced under
  meta-spec set {old}, current is {new}. Reconciliation may be needed.` and
  carries `GateStatus::Passed` (the enum has no Warning variant), so it
  never blocks by itself. The warn test asserts both the exact output and
  `MrStatus::Merged` — it fails if the warning blocks or the format drifts.
- **Strict mode is real enforcement.** `require_current_meta_spec_set: true`
  (port-backed KV store, namespace `workspace_merge_policies`, default
  false) converts drift to a hold: the entry is requeued `Queued` with the
  drift reason and the MR stays Open; relaxing the policy lets the same
  entry merge on the next cycle (asserted in the hard-block test). The hold
  is evaluated DURING candidate selection (same pattern as the trust hold),
  so a strictly-blocked head cannot starve later entries in the same or
  other workspaces — covered by two dedicated starvation regression tests
  (single MR and atomic group). Group candidates are expanded to all
  members, so a drifted member holds the whole group.
- **Merge-policy API is safe.** `GET/PUT /api/v1/workspaces/:id/merge-policy`
  is ABAC-registered (registry check green), PUT is Admin-only, both
  handlers verify tenant ownership of the workspace (404 otherwise). The
  API test drives the real router end-to-end: default false, Developer-role
  JWT → 403 with no state change, Admin round-trip true. A corrupt stored
  policy errors on direct lookup and degrades to the spec default inside
  the merge processor (logged), so a policy-store outage cannot wedge the
  queue — consistent with the spec's "warn always, block never" default.
- **All merge paths covered.** Merging happens only through the merge-queue
  processor (`run_once`/`process_next` — no direct-merge HTTP route), and
  the drift gate runs in the selection loop before any merge transaction.
  The merge tests drive the production `process_next` path with the
  repo's standard `MemGitOps` test double — the same infrastructure every
  pre-existing merge-processor test uses; the assertions are discriminating
  (each drift test fails if its half of the behavior is disabled).
- **UI.** `MergeQueueGraph.svelte` renders the ⚠ drift badge
  (`data-drift`, legend entry, tooltip "Meta-spec drift: produced under
  v{old8}, current is v{new8}") from the queue graph's `meta_spec_drift`
  node field, which `get_queue_graph` derives by parsing the recorded gate
  result — the same source as the MR detail view, so the graph cannot
  disagree with gate results. 29/29 web tests pass.
- **Coverage matrix update is faithful** (row 13 → implemented with landed
  evidence; counts corrected).
- **CI repair is scoped and sound.** The four mold steps now call
  `ci-install-mold.sh`; mold is a link-performance optimization and
  `.cargo/config.toml` only pins the mold/clang linker, so degrading to the
  default GNU linker on an apt/network failure cannot change build
  semantics. Both script outcomes verified by stub simulation.

## Observations (not findings)

- Canonical §9 sketches a `MetaSpecPolicy { warn_on_drift, block_on_drift,
  drift_tolerance }`. The implementation follows the assigned task
  contract's `require_current_meta_spec_set: bool` (block on any drift ==
  the spec's `block_on_drift` + `drift_tolerance: 0` variant); both
  canonical behaviors (warn-don't-block default, per-workspace opt-in
  block) are enforced. Version-count tolerance granularity is not
  implemented — out of the assigned acceptance criteria; a possible
  follow-up for the spec owner.
- The strict-block path records the hold reason on the queue entry (not a
  Failed gate result on the MR) — the entry stays Queued per the task's
  "stays in queue until reconciled or overridden" requirement.

## Verifier must execute (recorded in /tmp/stage/review-evidence/verifier-probes.md)

`cargo test -p gyre-server --lib` with filters `meta_spec_drift`,
`strict_drift_hold`, `drift_check`, `drift_warning_parse`, `merge_policy`;
plus `cd web && npm ci && npx vitest run
src/__tests__/MergeQueueGraph.test.js`. Remote CI on the exact head must
confirm the mold-fallback build path (failure class of run 43046986838).
