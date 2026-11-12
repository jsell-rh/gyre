# Review — task-159 (Wire meta-spec drift warnings into merge gates)

Spec: `specs/system/meta-spec-reconciliation.md` §9 "Merge Gate Behavior".
Candidate under review: `dea4cd5bd` (assigned base `4a203a06`).
Verdict: **approved** — all six acceptance criteria implemented in production code paths; no blocking findings.

## Probes run (independent evidence, logs in /tmp/stage/review-evidence/)

- `/tmp/stage/bin/cargo check -p gyre-server` → exit 0 (7m45s; no rustc warnings).
- `/tmp/stage/bin/cargo check -p gyre-server --tests` → exit 0 (test code type-checks).
- `cd web && npm ci && npx vitest run src/__tests__/MergeQueueGraph.test.js` → 29/29 passed (5 new drift tests: badge on drifted nodes, no badge without drift, tooltip text `Meta-spec drift: produced under v{old}, current is v{new}`, tooltip row omitted without drift, legend entry).
- `bash scripts/check-abac-route-registry.sh` → OK (`/api/v1/workspaces/:id/merge-policy` has a `RouteResourceMapping` resolver entry).
- `bash scripts/check-arch.sh` → pass. `check-mem-port-contracts.sh` → OK. `check-in-memory-state-stores.sh` → OK.
- `bash scripts/check-task-commit-attribution.sh` → fails against the **working tree only**: the stage harness reset `specs/tasks/task-159.md` to the assignment contract (`commits: []`). The committed candidate frontmatter (`git show dea4cd5bd:specs/tasks/task-159.md`) lists all flagged SHAs (`bd834689`, `11a702b8`, `8a99bd80`, `abbcd4aa`, plus `2d15f77c`, `631e8215`, `33303030`) — attribution is complete in the candidate. Not a candidate defect.

## Verified (code reading + probes)

- **Drift evaluation is real and correctly placed.** `selection_meta_spec_drift_hold()` runs in `process_next()`'s candidate-selection loop (merge_processor.rs, step 4c''), mirroring the pre-existing `selection_trust_hold` pattern: a strictly-blocked entry is requeued (Queued + drift reason) and the loop tries the next candidate in the same cycle, so a drifted head-of-queue entry cannot starve the shared queue — regression tests cover same-workspace and cross-workspace starvation plus atomic-group hold-as-unit. Attestation resolution is real: source-branch head commit via `git_refs::resolve_ref` (skip on None — no fail-open ref handling), then author agent's current task (max-`chain_depth` leaf), walking derived nodes to the chain's root `SignedInput` (`attestation_recorded_set_sha`). Compared against `compute_meta_spec_set_sha` for the MR's workspace; no bound set / no attestation / empty recorded SHA all skip gracefully.
- **§9 warning format is exact** (`[WARN] meta-spec-drift: code produced under meta-spec set {old}, current is {new}. Reconciliation may be needed.`), recorded as a `GateResult` (gate id `meta-spec-drift`, `GateStatus::Passed` — `GateStatus` has no Warn variant, so Passed is the correct non-blocking carrier; verified `check_gates_for_mr` treats Passed as terminal-pass, so the synthetic result cannot interfere with gate readiness). Idempotent across retry cycles (existing result updated in place).
- **Warn-only default does not block** — test drives the real `process_next` loop and asserts `MrStatus::Merged` with the drift gate result present; **strict policy hard-blocks** — `WorkspaceMergePolicy.require_current_meta_spec_set` (port-backed `KvJsonStore`, namespace `workspace_merge_policies` — durable, satisfies the in-memory-state invariant; default false) requeues the entry with the drift reason, MR stays Open, and relaxing the policy lets the same entry merge on the next cycle (covered by test).
- **API**: `GET/PUT /api/v1/workspaces/:id/merge-policy` — PUT is Admin-only (handler-level role check in addition to middleware ABAC) and tenant-scoped (cross-tenant → 404); partial-update semantics; corrupt stored policy surfaces as an error on direct lookup and degrades to the spec default (warn only) inside the merge processor. API test covers default, Developer 403 (with no state change), admin roundtrip, unknown-workspace 404.
- **UI**: queue-graph endpoint surfaces a *recorded* drift gate result as a `meta_spec_drift` node field parsed from the gate-result output (same source as the MR detail view — graph cannot disagree with gate results); `MergeQueueGraph.svelte` renders the ⚠ badge, tooltip row, and legend entry. Frontend tests pass.
- **Docs/coverage/attribution**: `docs/api-reference.md` documents both endpoints and the graph field; coverage row 13 → `implemented` with accurate notes (header counts consistent: 6 implemented / 5 task-assigned / 7 n/a); task `commits:` frontmatter complete in the candidate.

## Verifier must execute (gyre-server test compilation deferred per 4 GiB build budget)

- `cargo test -p gyre-server -- drift` — covers merge_processor (warn-proceeds, hard-block + policy-relax recovery, match-no-warning, no-attestation skip, gate-result idempotency, both starvation regressions) and reconciliation (§9 evaluation: match/mismatch/no-attestation/no-set/empty-sha, warning-format round-trip, foreign-output rejection).
- `cargo test -p gyre-server -- merge_policy` — covers merge_policy.rs unit tests and the workspaces.rs merge-policy API test.

## Non-blocking observations

1. Drift evaluation runs at candidate-selection time rather than at enqueue time; an MR that sits queued without being selected has no drift warning recorded until the processor selects it. Enforcement is unaffected (the block fires exactly when a merge would) and the queue graph surfaces recorded warnings; visibility for never-selected MRs is deferred. Reasonable reading of "when an MR enters the merge queue, the merge processor checks".
2. `merge_policy_or_default()` fails open to warn-only on a policy-store error — documented in-code, consistent with the spec's opinionated default ("warn always, block never"); strict mode is opt-in.
3. Atomic-group expansion filters by group string across workspaces — identical to the pre-existing `atomic_group_ready()`/`merge_atomic_group()` semantics; not a new defect.
4. `get_queue_graph()` uses `.unwrap_or_default()` on gate-result listing — display-only degradation, no enforcement impact.
