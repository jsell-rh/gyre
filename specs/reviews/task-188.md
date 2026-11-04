# Review — task-188 (Enforce SignedInput context binding / replay prevention at verification)

Spec: `specs/system/authorization-provenance.md` §2.4 Context Binding (Replay Prevention) — `workspace_id`+`repo_id`, `spec_sha`, `expected_generation`, `valid_until` bindings; mitigates §1.2 "Replay valid attestations to the wrong context".
Candidate: `f00c8dc1240989dde64a50fb80fd6812bf232c21` (base `27bd585ca7eb429905ccbded1f48b4d0167c0c20`).
Verdict: **complete** — approved, no findings.

## Independent verification (this checkout at the candidate, tree clean)

Full commands, outputs, and exit codes: `/tmp/stage/review-evidence/test-results.md`.

- `cargo test -p gyre-server --lib -- constraint_check` → **40 passed, 0 failed** (includes all nine task-188 tests).
- Mutation probes (source restored and verified byte-identical to the candidate after each):
  - Push gate disabled (`if !binding_result.valid` → `if false && ...` in `enforce_push_constraints` only) → `enforce_push_rejects_repo_id_mismatch`, `enforce_push_rejects_spec_sha_mismatch`, `enforce_push_rejects_stale_generation` all **FAIL**; the matching-context tests still pass, proving the chains are cryptographically valid and only the binding comparison rejects.
  - Merge gate disabled (same edit in `enforce_merge_constraints`) → `enforce_merge_rejects_workspace_id_mismatch` **FAILS**.
  The tests are genuinely anchored to the enforcement, not self-confirming.
- `cargo build --all` → EXIT 0. Touched-crate suites: gyre-server lib **1214 passed**, gyre-adapters **344 passed**, gyre-domain **371 passed**, all 0 failed.
- Mechanical gates on the candidate HEAD: check-arch, check-migration-versions (000056 is the next unused sequence), check-migration-sql-portability, check-task-commit-attribution, check-fabricated-scope-defaults, check-relative-path-defaults, check-lossy-secret-conversion, check-mem-port-contracts, check-inert-enforcement, check-conditional-test-guards, check-scope-literal-defaults → all EXIT 0. The exemption-file diffs base→candidate are pure line-number shifts of pre-existing entries (code insertions moved the flagged lines); no new entries, frozen counts intact.

## What was verified as real (no findings)

- **`verify_context_binding`** (`git_http.rs:3367`): compares the signed root `InputContent` against the actual push/merge target — `repo_id`, `workspace_id`, `spec_sha` (only when the task's current approved SHA is resolvable, per plan), and `expected_generation` (only when pinned; unresolvable task ⇒ `generation = None` ⇒ any pin rejects — fail closed on the pin). Emits `context_binding.*` children of the existing `VerificationResult` tree; `valid_until` is not duplicated (stays in `verify_attestation_audit_only`). Not structural: every comparison is against the real target arguments.
- **Wiring uses the real target.** Push: `git_receive_pack` resolves the repo by tenant-scoped slug (`resolved.id`, `resolved.workspace_id`) and passes those to `enforce_push_constraints` (git_http.rs:348). Merge: the merge processor passes `repo.id` / `mr.workspace_id` (merge_processor.rs:1412); `enforce_merge_constraints` resolves the task from the MR author's `current_task_id` — real lookups, no constants.
- **Enforcement ordering and emission.** The binding gate runs after `verify_chain` succeeds in both `enforce_push_constraints` (:357) and `enforce_merge_constraints` (:616); mismatch ⇒ `Err` naming the failing binding(s), `attestation.chain_invalid` audit event, `ConstraintViolation` events + priority-2 Inbox notifications via the existing `emit_constraint_violations` / `create_violation_notifications` path — no parallel notification path invented. Audit-only mirrors in `evaluate_push_constraints` (:91) and `evaluate_merge_constraints` (:860) log `attestation.context_binding_mismatch` without blocking (Phase 2), and `verify_attestation_audit_only` is correctly left alone (no target context there).
- **Real generation counter.** `Task.generation` (default 1) in the domain struct, persisted through both SQLite and Postgres task adapters (row mapping both directions; `max(1)` guard on read), portable migration `2026-10-08-000056_task_generation` (`ALTER TABLE tasks ADD COLUMN generation INTEGER NOT NULL DEFAULT 1` — runs on both backends), exposed in `TaskResponse`. Bumped on reassignment in all four mutation paths: `update_task` (PUT /api/v1/tasks/:id, only when the assignee actually changes), `spawn_agent_core` (only when previously assigned to a different agent), `admin_reassign_agent`, MCP `handle_update_task`. The compared generation at verification time comes from `state.tasks.find_by_id` — looked up, never hardcoded.
- **spec_sha resolution is real state**: `resolve_task_binding_context` takes the newest non-revoked approval event for the task's `spec_path` from `spec_approval_history`, falling back to the ledger's current SHA; `None` (unresolvable) skips that binding exactly as the plan prescribes ("if unavailable, pass `None`"), while repo/workspace bindings always run.
- **Tests are hard.** All binding tests build real Ed25519 keypairs, sign the real `InputContent` hash, persist real tasks/agents/MRs/attestations in the port-backed test state, and push to a real on-disk git repo. `update_task_api_reassignment_bumps_generation` goes through the real HTTP router (PUT, 200) and then proves the stale generation-1 authorization is rejected at the enforcement boundary. Error-message assertions name the specific failing binding. Mutation probes (above) confirm removal breaks them.
- **Coverage scope respected.** `git diff base..candidate` touches only the four crates' task/generation/binding code, the migration, task bookkeeping, and line-number shifts in three exemption files. No gates weakened, no exemptions added, no tests deleted. The task-155 frontmatter addition (base commit 27bd585c) is the attribution gate's prescribed remediation for a pre-existing main-side omission, reproduced at base.

## Observations (non-blocking, outside the task contract)

- **Signer-side generation pin not exposed**: `api/specs.rs` still creates SignedInputs with `expected_generation: None`; the `ApproveSpecRequest` contract has no pin field. The task plan did not enumerate this (it requires only the enforcement-side comparison against the persisted counter, which is implemented and mutation-tested). A signer-side pin would be new API surface — correctly deferred, not a §2.4 enforcement gap.
- **web/dist staleness is pre-existing in kind**: the candidate's committed dist (rebuilt at checkpoint `add4ba14` from the pre-merge pipeline web/src) does not include task-196's web/src changes that arrived via the base merge — but base's own dist (from `44a8187f`) predates task-196 as well, so the candidate introduces no regression and partially improves freshness. Dist regeneration belongs to verification/CI with a full web rebuild.
- Sandbox restriction (infrastructure, not a code defect): TCP `accept()` does not complete in this sandbox, so listener-based integration tests cannot run here; none are required for §2.4 — all evidence is lib-level with real git repos and real signatures.

Findings:

- (none)
