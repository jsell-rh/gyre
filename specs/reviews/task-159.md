# Review — task-159 (Wire meta-spec drift warnings into merge gates)

Spec: `specs/system/meta-spec-reconciliation.md` §9 "Merge Gate Behavior".
Candidate: `21d799ff6` (base `fca5de5fb`); task commits `333030908`, `abbcd4aad`, `8a99bd808`, `11a702b89` (all present on branch, attribution gate green).
Verdict: **needs-revision** — one material finding (F1, stale shipped web bundle); backend, policy, and drift evaluation verified clean with a kill-test.

## Probes run (checkout at `21d799ff6`, tree clean before and after)

- `cargo test -p gyre-server --lib meta_spec_drift` → **5/5 passed** (warn-records-gate-result-and-merges, hard-block + policy-relax recovery, match-no-warning, no-attestation-skip, gate-result idempotency).
- `cargo test -p gyre-server --lib strict_drift_hold` → **2/2 passed** (starvation regression: same-workspace + cross-workspace clean MRs merge behind a strictly-blocked drifted head; drift-held atomic group does not starve a later entry).
- `cargo test -p gyre-server --lib merge_policy` → **4/4 passed** (KV default, roundtrip, corrupt-fallback, API default/Developer-403/Admin-roundtrip/unknown-404).
- `cargo test -p gyre-server --lib drift_check` → **5/5 passed** (reconciliation §9 evaluation: match, mismatch + format round-trip, no-attestation, no-set, empty-sha).
- **Kill-test**: `mr_meta_spec_drift` patched to return `None` → `meta_spec_drift` suite **3 failed** (hard-block, warn-record, idempotency) — the tests detect disabled production behavior, not self-confirming. File restored; tree clean.
- `cd web && npx vitest run src/__tests__/MergeQueueGraph.test.js` → **29/29 passed** (5 new drift badge/tooltip/legend tests).
- Mechanical checks green: `check-abac-route-registry`, `check-arch`, `check-in-memory-state-stores`, `check-inert-enforcement`, `check-scope-literal-defaults`, `check-forged-scope-fields`, `check-abac-exempt-handlers`, `check-task-commit-attribution`.
- Dist-staleness probe (see F1): committed `web/dist` grep → 0 matches for `mq-drift`/`data-drift`/`Meta-spec drift`/`mq-tooltip-drift`; fresh `npm run build` of current `web/src` → all present.

Evidence: `/tmp/stage/review-evidence/task-159-*.log`.

## Verified working (no findings)

- **Drift evaluation is real.** `mr_meta_spec_drift` (reconciliation.rs) resolves the MR's provenance attestation (source-branch head commit via `resolve_ref`, fallback to the author agent's current task, walking derived nodes to the chain's root `SignedInput`) and compares the recorded `meta_spec_set_sha` against `compute_meta_spec_set_sha` for the MR's workspace. No set bound / no attestation / empty recorded SHA all skip gracefully (each covered by a test). The `find_by_commit`-first ordering means real git repos use the commit the agent produced.
- **§9 warning format is exact and round-trips.** `format_meta_spec_drift_warning` emits `[WARN] meta-spec-drift: code produced under meta-spec set {old}, current is {new}. Reconciliation may be needed.`; the mismatch test asserts exact equality with the formatter output plus prefix/suffix, and `parse_meta_spec_drift_warning` rejects foreign/empty-SHA output (dedicated test).
- **Gate wiring follows the existing pattern and is starvation-safe.** `selection_meta_spec_drift_hold` runs during candidate selection in `process_next` (step 4c'', mirroring the task-077 trust hold): warn-only records a `meta-spec-drift` gate result (`GateStatus::Passed` — never blocks; unknown-gate results default to required but Passed passes) idempotently (update-in-place, tested); strict policy requeues the entry with the drift reason and `continue`s to the next candidate in the same cycle, including atomic-group expansion (both regression tests pass and would fail under the old post-selection early-return shape per the shipped kill-test note). `merge_atomic_group`/single-entry paths correctly defer to selection.
- **Merge policy is durable and correctly permissioned.** KV-backed (`workspace_merge_policies` namespace) — passes `check-in-memory-state-stores`. Corrupt store errors on direct lookup and degrades to the spec default (warn only) inside the processor — a documented, defensible choice (erroring would wedge every merge cycle). `GET/PUT /api/v1/workspaces/{id}/merge-policy`: registered in the ABAC resolver, both handlers tenant-scoped, PUT Admin-only with a real 403-no-state-change test; docs updated.
- **Graph API derives drift from the same gate result the MR detail view reads** (`parse_meta_spec_drift_warning` over the recorded gate result), so the graph cannot disagree with gate results.
- Hard-block path records no Passed warn gate result until the policy is relaxed (asserted in the hard-block test's relax phase).
- Test-inflation check: none found — every new backend test drives `process_next` or the real router and asserts merged/queued status and gate-result content; the kill-test confirms they fail when the behavior is disabled.

## Findings

- [-] **F1 (major): the committed `web/dist/` bundle does not contain the task's UI change — the §9 drift badge is absent from the Rust-only shipped artifact.** Task-159 modified `web/src/components/MergeQueueGraph.svelte` (+ `MergeQueueGraph.test.js`), but no dist-regen commit exists in the range (`git log fca5de5fb..21d799ff6 -- web/dist` shows only base-merge commits). Grep of the committed bundle for `mq-drift`, `data-drift`, `Meta-spec drift`, `mq-tooltip-drift` → **0 matches** (the `meta_spec_drift`/`produced under` strings present in the bundle are pre-existing notification-kind labels and rollout-policy hints, not the badge); a fresh `npm run build` of the current sources produces all four markers. The documented Rust-only path (AGENTS.md: `SKIP_WEB_BUILD=1 cargo build -p gyre-server` "uses committed web/dist/") therefore serves a SPA whose merge-queue graph shows no drift warning — implementation-plan item 3 (UI display) and the coverage row 13 evidence ("MergeQueueGraph ⚠ badge/tooltip") are not satisfied in the shipped artifact. Same defect class as task-208 review round 1 F1.

  **Repair path:** `cd web && npm run build`, commit the regenerated `web/dist/` (e.g. `build(web): regenerate dist with meta-spec drift badge (task-159)`), append that commit's SHA to task-159's `commits:` frontmatter, and re-grep the committed bundle for the four markers. (Note: the attribution gate's product-surface regex does not cover `web/dist/` — the commit must be recorded manually, as in task-159's existing convention of listing every task-labeled commit.)

- [-] **F2 (minor, docs): the coverage header counts are swapped/wrong.** `specs/coverage/system/meta-spec-reconciliation.md` line 5 reads `Coverage: 5/11 (7 n/a, 6 task-assigned, 5 implemented)`, but the table has **6 implemented** rows (5, 6, 7, 8, 11, 13) and **5 task-assigned** rows (9, 10, 12, 14, 15). Under the file-family convention (implemented/assignable) it should read `6/11 (7 n/a, 5 task-assigned, 6 implemented)`. (The pre-base value `4/11` was already stale — the candidate's edit propagated the off-by-one instead of correcting it.) Row 13 itself is accurate and well-evidenced.

## Verdict

**needs-revision** — F1 requires a dist regeneration commit (mechanical); F2 is a one-line header fix. No backend, policy, evaluation, or test defects found; the §9 merge-gate behavior itself is correctly implemented and genuinely tested.
