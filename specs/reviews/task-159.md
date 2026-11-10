# Review — task-159 (Wire meta-spec drift warnings into merge gates)

Spec: `specs/system/meta-spec-reconciliation.md` §9 "Merge Gate Behavior" (coverage row 13).
Base `b5a8720878` → candidate `b2db02bdc` (includes base merges `936508da8`/`34b4b8ebc`, starvation repair `8a99bd808`, ABAC-test repair `11a702b89`, original feature `abbcd4aad`/`333030908`).
Verdict: **approved** — all six acceptance criteria verified with independent, kill-tested evidence; no findings.

## Round 1 (this review)

Probes run (checkout at candidate HEAD `b2db02bdc`, `/tmp/stage/bin/cargo`):

- `cargo test -p gyre-server --lib meta_spec_drift` → **5/5 passed** (warn-records-gate-result-and-merges, hard-block + policy-relax recovery, match-no-warning, no-attestation-skip, gate-result idempotency).
- `cargo test -p gyre-server --lib -- strict_drift_hold merge_policy drift_check` → **11/11 passed** (2 starvation regressions, 3 policy KV tests + Admin-only API test, 5 reconciliation §9 evaluation tests).
- `cargo test -p gyre-server --lib merge_processor::` → **86/86 passed** — verifies the `34b4b8ebc` base-merge conflict resolution (both task-159 drift suites and the base's task-106 rebase / task-116 attestation suites coexist intact).
- `cargo test -p gyre-server --lib merge_deps::` → **13/13 passed**; `reconciliation::` → **39/39 passed**.
- `npx vitest run src/__tests__/MergeQueueGraph.test.js` (web/, locked toolchain) → **29/29 passed** (5 new drift badge/tooltip/legend tests).
- **Kill-test** (temporary, restored, tree verified clean at `b2db02bdc`): with `mr_meta_spec_drift` forced to return `None`, **6 of the 12 drift tests fail** (warn-records, hard-block, idempotency, both strict-hold starvation tests, mismatched-SHA). Only the negative skip-cases (match/no-attestation/no-set/empty-sha) still pass, as they must. The tests genuinely kill disabled production behavior — no test inflation.
- `bash scripts/check-arch.sh`, `check-abac-route-registry.sh`, `check-inert-enforcement.sh`, `check-task-commit-attribution.sh` → all OK.
- Logs under `/tmp/stage/review-evidence/task-159-*.log`.

Verified working (no findings):

- **Drift evaluation is real** (`reconciliation.rs`): `mr_meta_spec_drift` resolves the MR's provenance attestation (source-branch head commit via `resolve_ref` → `find_by_commit`, then author agent's current task, walking derived nodes to the chain's root `SignedInput`) and compares the recorded `meta_spec_set_sha` against `compute_meta_spec_set_sha`. No-attestation, empty-recorded-SHA, and no-bound-set all skip gracefully (each covered by a dedicated test).
- **§9 warning format is exact**: `format_meta_spec_drift_warning` produces `[WARN] meta-spec-drift: code produced under meta-spec set {old}, current is {new}. Reconciliation may be needed.` — asserted verbatim in tests and round-tripped by `parse_meta_spec_drift_warning` (which rejects foreign/empty-SHA output).
- **Merge-gate wiring is in the selection loop** (`merge_processor.rs` step 4c''): warn-only drift records a `meta-spec-drift` gate result with `GateStatus::Passed` (informational, idempotent — updated in place, not duplicated) and the merge proceeds; the hard-block test proves the MR stays `Open` with the entry requeued carrying the drift reason, and the policy-relax phase proves recovery.
- **No queue starvation under strict policy**: the drift hold is evaluated during candidate selection exactly like the task-077 trust hold, so a strictly-blocked drifted head (single MR or atomic-group member) is requeued and the processor continues to the next candidate in the same cycle — verified by `strict_drift_hold_does_not_starve_shared_merge_queue` (same-workspace + cross-workspace clean MRs) and `strict_drift_hold_atomic_group_does_not_starve_later_entry` (both group member entries individually requeued with the drift reason; later independent entry merges).
- **Merge policy is durable and correctly gated**: KV-backed (`workspace_merge_policies` namespace, port-backed store — not an in-memory `Arc<Mutex>` alias), default false, corrupt-store error surfaces on direct lookup and degrades to the spec default inside the merge processor (logged). `GET/PUT /api/v1/workspaces/{id}/merge-policy` is tenant-scoped in both handlers (cross-tenant → 404), PUT Admin-only (Developer 403 with no state change, verified through the real router with JWTs), registered in the ABAC resolver, and documented in `docs/api-reference.md`.
- **UI**: `MergeQueueGraph.svelte` renders the ⚠ drift badge (`data-drift`, `mq-drift-{mr_id}`), tooltip "Meta-spec drift: produced under v{old}, current is v{new}", and legend entry; the graph API surfaces `meta_spec_drift` per node by parsing the same gate result the MR detail reads, so graph and gate results cannot disagree.
- **Coverage row 13** (`specs/coverage/system/meta-spec-reconciliation.md`) flipped to `implemented` with accurate evidence — consistent with the probes above (one wording nit: the note says the gate runs in `merge_atomic_group` "before the transactional merge", but the gate actually runs during selection and `merge_atomic_group` deliberately does not re-check; the code comments state this correctly).

Minor (non-blocking, recorded for completeness):

- The graph endpoint's `meta_spec_drift` field glue (`list_by_mr_id` → find → parse → serialize) has no direct backend test; its components are tested (gate-result recording end-to-end via the merge processor, parser round-trip) and the UI tests mock the field. Display-only seam, ~10 lines.
- The commit-based attestation lookup path (`resolve_ref` → `find_by_commit`) mirrors the pre-existing merge-time attestation pattern (`merge_processor.rs:2366`) and is not directly exercised with a real git repo; the task-based fallback is fully tested. Graceful `None` on unresolvable refs.
- Canonical spec §9 sketches a `MetaSpecPolicy { warn_on_drift, block_on_drift, drift_tolerance }`; the assigned task contract concretizes this to `require_current_meta_spec_set` (set SHAs carry no version ordering, so `drift_tolerance` is not computable as sketched — the orchestrator's decomposition, which this review treats as authoritative, resolved it to the single boolean). Semantics map: warn always on (default), block opt-in per workspace (default false).

Test-inflation check: none found — the kill-test is the primary evidence; the tests assert real merge outcomes (`MrStatus::Merged`/`Open`), real queue-entry states and reasons, and the verbatim §9 warning text.
