# Review — task-159 (Meta-Spec Reconciliation §9 — Wire meta-spec drift warnings into merge gates)

Spec: `specs/system/meta-spec-reconciliation.md` §9 "Merge Gate Behavior", via the assigned task excerpt (warn-only `[WARN] meta-spec-drift` gate result by default; `require_current_meta_spec_set: true` workspace merge policy converts it to a hard block; MRs without attestation skip gracefully; queue-graph UI badge).
Candidate: `1e29f2e2922d199c7fd73e038d8f3e28dfefe7ea` (base `fefde2946f3a0af1f212588f54591516fee0f7b2`). All four attributed commits (`abbcd4aad`, `333030908`, `8a99bd808`, `11a702b89`) are on the branch and match the task frontmatter.
Verdict: **approved**.

## Evidence (all probes run at the candidate; logs under /tmp/stage/review-evidence/task-159-*.log)

- `cargo test -p gyre-server --lib meta_spec_drift` → **5 passed** (warn-records-gate-result-and-merges, hard-block + policy-relax recovery, match-no-warning, no-attestation-skip, gate-result idempotency).
- `cargo test -p gyre-server --lib strict_drift_hold` → **2 passed** (starvation regressions: clean MRs behind a strictly-blocked drifted head — same workspace and cross-workspace — merge in the same cycle; drift-held atomic group holds as a unit without starving a later independent entry).
- `cargo test -p gyre-server --lib merge_policy` → **4 passed** (KV default / roundtrip / corrupt-fallback + API test: default warn-only, Developer PUT 403 with no state change, Admin PUT roundtrip, unknown workspace 404).
- `cargo test -p gyre-server --lib drift_check` → **5 passed** (reconciliation-level evaluation: match / mismatch+format round-trip / no-attestation / no-bound-set / empty-recorded-sha).
- `cargo test -p gyre-server --lib merge_processor` → **74 passed** (full merge-processor suite — selection-loop and atomic-group changes covered).
- **Kill-test (mutation probe):** with `selection_meta_spec_drift_hold` disabled (`return Ok(None)` inserted, then restored; tree verified clean at `1e29f2e29` afterwards), all 4 behavioral tests FAIL (`meta_spec_drift_records_warn_gate_result_and_merge_proceeds`, `meta_spec_drift_hard_blocks_under_strict_workspace_policy`, both `strict_drift_hold_*` starvation tests) — the tests exercise the production drift gate, not mirrored logic. Post-restore re-run green.
- `npx vitest run src/__tests__/MergeQueueGraph.test.js` (node_modules installed via `npm ci`, run by build.rs) → **29 passed**, including the 5 new drift badge / tooltip / legend tests.
- Mechanical checks on the tree: `check-abac-route-registry`, `check-arch`, `check-inert-enforcement`, `check-in-memory-state-stores`, `check-forged-scope-fields`, `check-task-commit-attribution` — all PASS.

## Verified against the acceptance criteria

1. **Merge processor checks meta_spec_set_sha on MRs with attestations** — `selection_meta_spec_drift_hold` (merge_processor.rs, last selection filter in `process_next`, same slot as the task-077 trust hold) calls `mr_meta_spec_drift` (reconciliation.rs), which resolves the MR's provenance attestation by source-branch head commit (`resolve_ref` + `find_by_commit`), falling back to the author agent's current task, and walks derived nodes to the chain's root `SignedInput` for the recorded SHA. Compared against `compute_meta_spec_set_sha` for the MR's workspace.
2. **Mismatched SHA produces `[WARN] meta-spec-drift` in gate results** — `record_meta_spec_drift_warning` persists a gate result (id `meta-spec-drift`) whose output is byte-exact the task's §9 format; the mismatch test asserts the exact string and the format/parse round-trip. Idempotent across cycles (existing result updated in place — asserted by a dedicated test).
3. **Warning does not block merge by default** — the gate result carries `GateStatus::Passed` and the warn-only test asserts `MrStatus::Merged`.
4. **`require_current_meta_spec_set: true` converts warning to hard block** — `WorkspaceMergePolicy` is KV-port-backed (namespace `workspace_merge_policies`, default false), exposed via `GET/PUT /api/v1/workspaces/{id}/merge-policy` (PUT Admin-only, both handlers tenant-scoped, route registered in the ABAC resolver). Under strict policy the entry is requeued `Queued` with the drift reason and the MR stays Open; relaxing the policy lets the same entry merge on the next cycle (asserted). Strict hold is evaluated during candidate selection with `continue`, so a held head cannot starve later entries in the same or other workspaces (kill-tested regression pair).
5. **MRs without attestation skip gracefully** — no-attestation, empty-recorded-SHA, and no-bound-set cases all return no drift and merge without a drift gate result (three dedicated tests).
6. **Tests cover match, mismatch, hard-block, no-attestation** — all present, plus idempotency, format round-trip, corrupt-policy fallback, API authorization, and starvation regressions; all confirmed non-self-confirming by the mutation probe.

The coverage matrix row 13 (§9) was updated to `implemented` with an accurate evidence note — not hollow: every claim in the row maps to code I inspected and probes I ran. The task body's acceptance checkboxes were deliberately left unchecked by the implementer (disclosed in the round note); the criteria themselves are satisfied per the evidence above.

## Minor observations (non-blocking, no action required)

- The canonical §9 sketch shows a `MetaSpecPolicy { warn_on_drift, block_on_drift, drift_tolerance }` struct and a persona-versioned warning format; the assigned task excerpt (the operative contract for this task) specifies the simpler `require_current_meta_spec_set: bool` switch and the set-SHA warning format, both of which are implemented exactly. `drift_tolerance` is not part of the assigned contract — if wanted, it is future work under the spec lifecycle.
- `GraphNode.meta_spec_drift` serializes as `null` (no `skip_serializing_if`) while the API doc marks it optional (`?`); the Svelte side handles null correctly. Cosmetic only.
- `merge_policy_or_default` degrades a corrupt/unreadable strict policy to warn-only (logged) so a policy-store outage cannot wedge the queue — a deliberate availability-over-strictness trade documented in code; the strict flag is a velocity governance control, not a security boundary.
