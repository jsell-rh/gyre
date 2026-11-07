# Review — task-159 (Meta-Spec Reconciliation §9 — Wire drift warnings into merge gates)

Spec: `specs/system/meta-spec-reconciliation.md` §9 "Merge Gate Behavior".
Candidate: `54dff26eb` (base `a884329d7`; task commits `abbcd4aad`, `333030908`, both verified ancestors).
Verdict: **needs-revision** (one code finding).

## Round 1

Evidence: `/tmp/stage/review-evidence/` (README maps each log). All probes run
at candidate HEAD with a clean tree; the temporary starvation probe test was
reverted and web/dist build artifacts restored after the runs.

### Verified working (no findings)

- **Drift evaluation** (`reconciliation.rs`): `mr_meta_spec_drift` resolves the
  MR's provenance attestation (source-branch head commit via `resolve_ref`, then
  the author agent's current task, walking derived nodes to the chain root
  `SignedInput`) and compares the recorded `meta_spec_set_sha` against
  `compute_meta_spec_set_sha`. Graceful skips verified by tests for: no bound
  set, no attestation, empty recorded SHA. The §9 warning text is exact and
  `parse_meta_spec_drift_warning` round-trips it (rejects foreign/empty-SHA
  output).
- **Warn-only path**: a gate result (`META_SPEC_DRIFT_GATE_ID`,
  `GateStatus::Passed`, §9 output) is recorded on the MR and the merge proceeds;
  idempotent across retry cycles (existing result updated in place). Reproduced:
  `cargo test -p gyre-server --lib meta_spec_drift` → 5 passed.
- **Policy store** (`merge_policy.rs`): KV-backed per workspace (port-backed,
  not an in-memory `Arc<Mutex>` alias — check-in-memory-state-stores green),
  default false, corrupt value errors on direct lookup and degrades to the
  spec default in the processor. 4 tests green.
- **API**: `GET/PUT /api/v1/workspaces/{id}/merge-policy` — PUT Admin-only,
  both handlers tenant-scoped (stricter than the sibling meta-spec-set handlers,
  which have no tenant check), registered in the ABAC resolver, documented in
  `docs/api-reference.md`. API test covers default / Developer-403 (no state
  change) / Admin roundtrip / unknown-workspace 404.
- **UI**: MergeQueueGraph ⚠ badge (`data-drift`, `mq-drift-{id}`), tooltip with
  8-char SHA prefixes, legend entry; graph API surfaces recorded drift by
  parsing the same gate result the MR detail reads. `npx vitest run
  src/__tests__/MergeQueueGraph.test.js` → 25 passed.
- **Prior CI-repair finding resolved**: `bash scripts/check-inert-enforcement.sh`
  exits 0 on the candidate — the merged base restructured both pinned
  `evaluate_attestation_abac` call sites to bind their results with real
  Deny-effect branches and deleted the two stale exemptions (count went down;
  no new exemptions). Other mechanical checks (arch, abac-route-registry,
  abac-exempt-handlers, in-memory-state-stores, forged-scope-fields,
  scope-literal-defaults, unbounded-external-http) all green.
- Both `commits:` frontmatter entries are ancestors of the candidate.

### Findings

- [open] **F1 (major): the strict-policy drift block starves the shared merge
  queue.** In `process_next` (`crates/gyre-server/src/merge_processor.rs`,
  ~:1308-1347) the §9 drift check runs AFTER candidate selection: the selected
  entry is claimed `Processing`, and on strict-policy drift it is requeued via
  `merge_queue.update_status` (which does not rotate `enqueued_at` or priority)
  while `process_next` returns `Ok(())` early. `list_queue` is global (all
  non-terminal entries across workspaces, priority desc / enqueued_at asc) and
  `spawn_merge_processor` runs `process_next` once per 5s tick, so a
  strictly-blocked drifted MR at the head of the queue is re-selected and
  re-blocked every cycle and **no later entry ever merges** — including clean
  MRs in *other* workspaces and non-drifted MRs in the same workspace — until
  the drifted MR is reconciled or the policy relaxed. A workspace-scoped
  governance setting thereby denies merge service to unrelated workspaces.
  Reproduced with a temporary test (`probe_review_strict_drift_block_starves_
  shared_queue`, source and failing output in `/tmp/stage/review-evidence/ev5-*`):
  a clean MR in workspace B stays `Open` after 3 `process_next` cycles while a
  higher-priority drifted MR in workspace A (`require_current_meta_spec_set:
  true`) is blocked. The tree itself documents this exact pattern as a defect:
  the task-077 `selection_trust_hold` comment ("Evaluating the hold only after
  selection and returning early starves the shared queue … monopolizing the
  processor and blocking later entries"), and both analogous holds
  (`selection_trust_hold` step 4c', `merge_queue_paused` step 4b') are evaluated
  DURING the selection loop with `continue` so the processor tries the next
  candidate in the SAME cycle. `merge_atomic_group`'s drift block
  (~:661-712) has the same requeue-and-return shape: a strict-blocked group
  member requeues the whole group and returns, so entries after the group in
  that cycle are never tried. The shipped hard-block test enqueues a single
  entry, so it cannot catch this. **Remedy:** evaluate drift + policy during
  candidate selection (like `selection_trust_hold`): requeue-with-reason and
  `continue` to the next candidate; mirror the check in the atomic-group
  readiness path so a drifted group member holds only that group; add a
  multi-entry regression test (strict-blocked head + clean tail must merge).

### Notes

- The §9 acceptance criteria are literally satisfied (warn recorded in gate
  results, warn does not block, strict policy blocks the drifted MR, no-
  attestation skip, tests for all four cases), and the coverage row 13 update
  to `implemented` is substantiated for the warn-only path; F1 is a defect in
  how the hard-block path integrates with the shared queue, not a hollow
  implementation. Once F1 is fixed the row stands.
- `merge_policy_or_default`'s corrupt-fallback to warn-only is a disclosed
  degradation choice consistent with the spec's "velocity over strict
  consistency" default; not a finding.
