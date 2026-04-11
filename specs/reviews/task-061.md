# Review: TASK-061 — Populate Attestation-Chain ABAC Subject Attributes

**Reviewer:** Verifier  
**Round:** R1  
**Commit:** `07a1f928`  
**Verdict:** `needs-revision`

---

## Findings

- [ ] **F1: Missing `subject.tenant_id` in merge-time ABAC context.** The push-time ABAC evaluation at `git_http.rs:607` sets `ctx.set("subject.tenant_id", &push_tenant_id)`, but the merge-time `evaluate_attestation_abac` function at `merge_processor.rs:1890-1901` does NOT set `subject.tenant_id`. The existing ABAC middleware (`abac_middleware.rs:792`) always sets `subject.tenant_id`. Any policy conditioning on `subject.tenant_id` will silently fail to match for merge while working for push — an asymmetric context that makes multi-tenant policies unreliable. The `repo.tenant_id` is available via the `repo: &Repository` parameter already passed to `evaluate_attestation_abac`.

- [ ] **F2: Push-time uses `attestations.last()` instead of `max_by_key` to identify the leaf attestation.** At `git_http.rs:594`, the push flow uses `attestations.last()` to identify the leaf. The `find_by_task` adapter returns attestations ordered by `created_at` (SQLite, `chain_attestation.rs:223`) or insertion order (mem), NOT by `chain_depth`. The merge flow at `merge_processor.rs:1290` correctly uses `atts.iter().max_by_key(|a| a.metadata.chain_depth)`. If attestations are inserted out of chain_depth order, the push flow picks the wrong node — using its `chain_depth` and loading a partial chain via `load_chain`. The fix is to replace `attestations.last()` with `attestations.iter().max_by_key(|a| a.metadata.chain_depth)`, matching the merge flow's pattern.

- [ ] **F3: Doc comment on `SubjectAttrs.constraint_count` overclaims scope — "strategy-implied" constraints are not counted.** At `policies.rs:257`, the doc comment says "Total accumulated constraint count (explicit + strategy-implied + gate)." The `constraint_count()` function at `attestation.rs:34-51` only counts explicit (`SignedInput.output_constraints`, `DerivedInput.output_constraints`) and gate (`GateAttestation.constraint`) constraints from the chain data. Strategy-implied constraints are ephemeral — derived at evaluation time from workspace config via `derive_strategy_constraints()` in `constraint_evaluator.rs:264` — and are NOT stored in the attestation chain. The doc comment misleads policy authors into thinking the count includes strategy-implied constraints. Fix: change the doc comment to "Total accumulated constraint count (explicit + gate)" to match what the function actually computes.

- [ ] **F4: No integration tests for the push-time or merge-time ABAC evaluation code paths — acceptance criterion 3 not met.** The task's acceptance criteria require "Integration test: push flow populates `subject.chain_depth`, `subject.root_signer`, `subject.constraint_count` in ABAC context." The tests in `policy_engine.rs` are unit tests with manually constructed `AttributeContext` — they don't exercise the push or merge code. The tests in `policies.rs` exercise the dry-run HTTP endpoint — they test that `SubjectAttrs` fields are deserialized and forwarded to the policy engine, not that the push/merge flows populate the attributes from a real attestation chain. The code at `git_http.rs:591-630` and `merge_processor.rs:1252-1308` (`evaluate_attestation_abac`) has zero test coverage. These are the code paths this task was specifically designed to add — untested, they could be deleted without any test failing.
