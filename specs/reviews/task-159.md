# Review — task-159: Wire meta-spec drift warnings into merge gates

**Candidate:** `c2819820131be4ae4306cb77535b49ad1c178eea` (base `41f4d56e8419e24a2a66c7fab33e79445d4a656b`)
**Spec:** meta-spec-reconciliation.md §9 (Merge Gate Behavior)
**Verdict:** Approved — independent evidence supports the task contract.

## What was reviewed

The full base→candidate diff excluding generated `web/dist`:
`reconciliation.rs` (§9 drift evaluation + warning format/parse + 6 tests),
`merge_processor.rs` (selection-time drift hold + warn-gate recording + 6
tests incl. two starvation regressions), `merge_policy.rs` (new, KV-backed
per-workspace policy + 3 tests), `api/workspaces.rs` (GET/PUT merge-policy +
API test), `api/mod.rs`/`abac_middleware.rs` (route + ABAC registration),
`api/merge_deps.rs` (graph node `meta_spec_drift` field),
`MergeQueueGraph.svelte` + tests (drift badge/tooltip/legend),
`docs/api-reference.md`, and the §9 coverage-matrix row.

## Findings

No blocking defects. The implementation is real end-to-end:

- **Drift evaluation is real** — `mr_meta_spec_drift` resolves the MR's
  provenance attestation through the production lookup paths
  (source-branch head commit via `git_refs::resolve_ref` +
  `find_by_commit`, falling back to the author agent's current task via
  `find_by_task`, walking `load_chain` to the root `SignedInput`) and
  compares the recorded `meta_spec_set_sha` against the live
  `compute_meta_spec_set_sha` for the workspace. No attestation / no bound
  set / empty recorded SHA all skip gracefully (each covered by a test).
- **Warning is exact and non-blocking** — `[WARN] meta-spec-drift: code
  produced under meta-spec set {old}, current is {new}. Reconciliation may
  be needed.` recorded as a gate result with `GateStatus::Passed`,
  idempotent across retry cycles (existing result updated in place); the
  warn-only test asserts the MR still reaches `Merged`.
- **Strict mode is a real hard block** — `require_current_meta_spec_set:
  true` (KV-port-backed, default false, corrupt-store degrades to the spec
  default with a log) requeues the entry with the drift reason and the MR
  stays `Open`; the test also proves policy relaxation lets the same entry
  merge afterwards. The hold is evaluated during candidate selection
  (mirroring the established `selection_trust_hold` pattern), and the
  starvation regressions prove clean MRs — same-workspace,
  cross-workspace, and behind a held atomic group — still merge while a
  drifted head is held.
- **API is properly scoped** — GET/PUT `/workspaces/:id/merge-policy`
  registered in both the router and the ABAC resolver; PUT Admin-only with
  tenant-ownership check; GET mirrors the existing `get_workspace`
  ownership pattern; no caller-supplied scope fields.
- **UI** — MergeQueueGraph renders the ⚠ drift badge, tooltip
  ("Meta-spec drift: produced under v{old}, current is v{new}"), and
  legend entry from the backend-recorded gate result (single source of
  truth — parsed from gate results, not re-derived).
- **Coverage matrix** §9 row updated to `implemented | task-159` with
  notes that match the shipped code.

## Probe evidence (saved under /tmp/stage/review-evidence/)

- `SKIP_WEB_BUILD=1 cargo check -p gyre-server` — PASS (6m 25s).
- `scripts/check-arch.sh`, `check-abac-route-registry.sh`,
  `check-in-memory-state-stores.sh`, `check-abac-exempt-handlers.sh`,
  `check-forged-scope-fields.sh`, `check-inert-enforcement.sh` — all PASS.
- `web`: `npm ci` then `npx vitest run src/__tests__/MergeQueueGraph.test.js`
  — 29/29 PASS; the drift tests fail if the badge/tooltip rendering is
  removed (they assert `data-drift`, the `mq-drift-*` testid, the tooltip
  text, and the no-drift absence), so they are not self-confirming.

## Verifier must run (gyre-server test-binary compilation exceeds the
4 GiB sandbox budget)

    cargo test -p gyre-server --lib merge_processor::tests::meta_spec_drift
    cargo test -p gyre-server --lib merge_processor::tests::strict_drift_hold
    cargo test -p gyre-server --lib reconciliation::tests::drift_
    cargo test -p gyre-server --lib reconciliation::tests::drift_warning_parse
    cargo test -p gyre-server --lib merge_policy::tests
    cargo test -p gyre-server --lib workspaces::tests::merge_policy

## Non-blocking observations

- Under strict mode the blocked MR carries the drift warning in the
  queue-entry reason rather than as a `Passed` gate result — defensible
  (a Passed gate result on a hard-blocked MR could be misread as a pass)
  and the §9 warning content is preserved in the reason text.
- `web/dist` at the candidate is the assignment-base build — generated
  output owned by the deterministic verifier, not a source defect.
