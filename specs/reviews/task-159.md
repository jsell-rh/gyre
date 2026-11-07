# Review — task-159: Wire meta-spec drift warnings into merge gates

**Candidate:** `93c8b5a5e933c39bc6bcaaaf7ee5c73b186eabc6` (branch `pipeline/task-159/22e5ba0fa725428ebd1df74d888c326b-1`)
**Base:** `66a5fc6791eeb259c247b9a96f2ea93668c999b1`
**Spec:** meta-spec-reconciliation.md §9 (Merge Gate Behavior)
**Verdict:** NOT APPROVED — 1 finding (mechanical-check regression). Evidence: `/tmp/stage/review-evidence/`.

## What was verified as real and correct

The §9 implementation itself is genuine and complete; no fakery found:

- **Drift evaluation** (`reconciliation.rs`): `mr_meta_spec_drift` resolves the MR's provenance attestation (source-branch head commit via `resolve_ref`/`find_by_commit`, then the author agent's current task, walking derived nodes to the chain's root `SignedInput`) and compares the recorded `meta_spec_set_sha` against `compute_meta_spec_set_sha`. No-set / no-attestation / empty-SHA all skip gracefully. `format_meta_spec_drift_warning` produces the exact §9 text; `parse_meta_spec_drift_warning` round-trips it and rejects foreign output.
- **Merge-gate wiring** (`merge_processor.rs`): both `process_next` and `merge_atomic_group` run the check before the transactional merge. Warn-only path records an idempotent `meta-spec-drift` gate result (`GateStatus::Passed`, §9 output) and merges; strict policy requeues the entry (and every atomic-group member) with the drift reason — the 5s processor loop retries it and there is no dead-letter/attempt counter, so it genuinely stays queued until reconciled or relaxed.
- **Workspace merge policy** (`merge_policy.rs`): KV-backed per-workspace (`workspace_merge_policies`), default false. `GET/PUT /api/v1/workspaces/{id}/merge-policy` registered in the router and ABAC resolver, tenant-scoped in both handlers, PUT Admin-only in-handler (test proves Developer 403 with no state change). Documented in `docs/api-reference.md`.
- **UI**: graph API surfaces recorded drift by parsing the same gate result the MR detail reads; `MergeQueueGraph.svelte` renders the ⚠ badge/tooltip/legend.

**Independent test reproduction (all counts match the shipped claims):**
`cargo test -p gyre-server --lib` — `meta_spec_drift` 5 passed, `merge_policy` 4 passed, `reconciliation::` 24 passed, `merge_processor` 54 passed, `merge_deps::` 13 passed; `npx vitest run src/__tests__/MergeQueueGraph.test.js` 25 passed (after `npm ci`). The tests are behavior-coupled: the warn test asserts `MrStatus::Merged` plus the exact §9 gate-result output, the hard-block test asserts the entry stays `Queued`/MR `Open` under strict policy and merges after policy relax — both fail if the drift check is removed. Mechanical checks green on the candidate: arch, abac-route-registry, abac-exempt-handlers (88), scope-literal-defaults, in-memory-state-stores, forged-scope-fields, mem-port-contracts. Commit attribution correct (process commits touch only the task file; both frontmatter commits exist).

## Finding (blocking) — CI mechanical gate regression

`scripts/check-inert-enforcement.sh` **fails on the candidate (exit 1) but passes on the assigned base (exit 0)**. The two pre-existing `evaluate_attestation_abac` statement-position sites (task-077-owned F5 hazards) are pinned in `scripts/inert-enforcement-exemptions.txt` by exact `path:lineno` — `merge_processor.rs:1457` and `:1479`. The task-159 diff inserts ~151 lines above them (drift gate in `merge_atomic_group` ~line 665 and `process_next` ~line 1251), shifting the sites to `:1608`/`:1630`; the stale anchors no longer match, so the check reports 2 inert evaluation sites. A full sweep of all `scripts/check-*.sh` on both candidate and base confirms this is the **only** candidate-caused regression (all other failures are identical on base — environmental/pre-existing).

Reproduction: `bash scripts/check-inert-enforcement.sh` at candidate HEAD → exit 1 (errors at merge_processor.rs:1608, :1630); same command on a worktree of base `66a5fc679` → exit 0 "OK". Logs: `/tmp/stage/review-evidence/task-159-check-inert-enforcement.log`, `task-159-inert-enforcement-regression.txt`.

Fix direction: update the two exemption anchors to the new line numbers (count unchanged — no new exemptions; the frozen-baseline rules prohibit growth, not anchor maintenance), or restructure the insertion so the pinned anchors hold. Note: the shipped evidence's "mechanical checks green" list omits exactly this check.

## Non-blocking observations (no action required for this task)

- `merge_policy_or_default` degrades a corrupt/unreadable strict policy to warn-only (fail-open on the enforcement flag). This is logged, tested, and defensible as "unknown policy → spec default" (the spec default IS warn-only), but a corrupted strict-policy row silently permits drifted merges. Worth revisiting if a fail-closed stance is ever specified.
- Empty recorded `meta_spec_set_sha` skips the check rather than warning (attestation predates any set binding) — consistent with the pre-existing §10 sweep convention (`sweep_ignores_empty_provenance_sha`).
- The task-based attestation fallback uses the author agent's *current* task; an agent that moved to a new task after opening the MR resolves the newer attestation. Mirrors the existing merge-time attestation-note lookup; heuristic, not a defect.
- The strict-block path re-evaluates every 5s processor cycle (warn-log noise only).
- The shipped round note claims evidence logs "retained under /tmp/stage/review-evidence/task-159-*.log"; that directory was empty in this sandbox — all counts were re-verified from scratch by this review instead.
