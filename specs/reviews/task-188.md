# Review — task-188 (Enforce SignedInput context binding, replay prevention)

Spec: `specs/system/authorization-provenance.md` §2.4 Context Binding (Replay
Prevention). Assignment: candidate `47e2c1ade45c158536283e4b3b314f929e2bb3d3`
on base `110180fbb248d6fa408492c0f9a3ec4a5a724372`.

Verdict: **complete** (approved).

## Evidence (independent re-run on the exact candidate)

- `cargo test -p gyre-server --lib -- constraint_check` (SKIP_WEB_BUILD=1,
  cold target dir): **40 passed / 0 failed**, including the six new
  binding tests (`enforce_push_rejects_repo_id_mismatch`,
  `enforce_merge_rejects_workspace_id_mismatch`,
  `enforce_push_rejects_spec_sha_mismatch`,
  `enforce_push_rejects_stale_generation`,
  `enforce_push_matching_generation_passes`,
  `enforce_push_matching_context_passes`,
  `update_task_api_reassignment_bumps_generation`,
  `verify_context_binding_unit_matrix`).
  Log: `/tmp/stage/review-evidence/test-constraint-check-rerun.log`.
- **Mutation probe**: `verify_context_binding` was temporarily disabled
  (early-return valid) and the suite re-run: **6 tests FAILED** — all four
  enforcement mismatch tests, the API-reassignment generation-pin test, and
  the unit matrix. The tests genuinely kill removal of the check; they are
  not self-confirming. Source restored afterwards; `git status` verified
  pristine. Log: `/tmp/stage/review-evidence/mutation-rerun.log`.
- Mechanical gates on the candidate (all EXIT 0): `check-arch.sh`,
  `check-migration-versions.sh`, `check-migration-sql-portability.sh`,
  `check-fabricated-scope-defaults.sh`, `check-lossy-secret-conversion.sh`,
  `check-inert-enforcement.sh`, `check-relative-path-defaults.sh`,
  `check-scope-literal-defaults.sh`, `check-mem-port-contracts.sh`,
  `check-conditional-test-guards.sh`, `check-task-commit-attribution.sh`.
  Logs: `/tmp/stage/review-evidence/*-rerun.log`.

## Verified working (no findings)

- **`verify_context_binding`** (`git_http.rs:3347`): real comparisons of the
  signed root `InputContent` against the actual target — `repo_id`,
  `workspace_id`, `spec_sha` (vs the task's currently approved SHA), and
  `expected_generation` (vs the task's persisted generation) — each a
  `context_binding.*` child of the existing `VerificationResult` tree.
  `valid_until` is not duplicated (stays in `verify_attestation_audit_only`).
  Fail-closed: a generation pin against an unresolvable task
  (`task_generation = None`) rejects; an empty signed spec_sha cannot match a
  resolved current SHA.
- **Enforcement is wired at both real boundaries.** `enforce_push_constraints`
  (constraint_check.rs:353) and `enforce_merge_constraints` (:619) call the
  check after `verify_chain` succeeds. The target IDs are the real push/merge
  targets, not caller claims: the push path resolves the repository record
  first (`git_http.rs:266-267` — `resolved.id` / `resolved.workspace_id`) and
  the merge path uses `repo.id` / `mr.workspace_id`
  (merge_processor.rs:1412-1417). Mismatch → `Err` naming the failing
  binding(s), `attestation.chain_invalid` audit warn, `ConstraintViolation`
  events + Inbox notifications via the existing
  `emit_constraint_violations` / `create_violation_notifications` path — no
  parallel notification channel was invented.
- **Audit-only mirrors** in `evaluate_push_constraints` (:86) and
  `evaluate_merge_constraints` (:857) log
  `attestation.context_binding_mismatch` per failing binding without
  blocking, per the Phase 2 observability requirement. As the plan specifies,
  `verify_attestation_audit_only` was not touched (no target context there).
- **`Task.generation` is real persisted state.** Domain field (default 1),
  SQLite + Postgres row mapping (`generation.max(1) as u32` on read),
  migration `2026-10-08-000056_task_generation` — next unused sequence after
  `000055`, portable SQL (`ALTER TABLE tasks ADD COLUMN generation INTEGER
  NOT NULL DEFAULT 1`), exposed in `TaskResponse`. Bumped on reassignment to
  a different agent in all four mutation paths: `update_task` API
  (api/tasks.rs:301-311), MCP `handle_update_task` (mcp.rs:1033-1043),
  `spawn_agent_core` (api/spawn.rs:547-557), `admin_reassign_agent`
  (api/admin.rs:376-382). The `update_task_api_reassignment_bumps_generation`
  test proves the bump end-to-end through `PUT /api/v1/tasks/:id` and then
  proves the stale generation-1 authorization is rejected at the enforcement
  boundary.
- **Test realism**: `make_binding_signed_input` signs SHA256(content) with a
  real Ed25519 keypair; the mismatch tests differ only in the bound context
  values, so chain/signature verification passes and only the binding
  comparison decides — confirmed by the mutation probe.
- **Exemption files**: no new entries. `fabricated-scope-defaults` and
  `lossy-secret-conversion` changes are comment-only documentation of earlier
  fixes; `relative-path-defaults` REMOVES one entry (spawn.rs:597 prose
  comment quoting `"./repos/"`, rewritten in this diff) — strictly stricter.
  All three checks re-run EXIT 0.
- **Spec_sha comparison target** matches the plan: newest non-revoked
  approval event's SHA for the task's `spec_path`, falling back to the spec
  ledger's current SHA, `None` (skip) when unresolvable — the plan's
  "approved SHA from the spec ledger; if unavailable, pass None".

## Scope notes (not blocking)

- The signer side (`api/specs.rs` approve endpoint) still creates
  SignedInputs with `expected_generation: None`; the request contract has no
  pin field. Enforcement (compare against persisted `Task.generation`) is
  complete and mutation-tested; exposing a signer-side pin extends the API
  contract beyond this task's enumerated scope. The enforcement path is live
  the moment any SignedInput carries a pin.
- The coverage matrix row for §2.4 still reads `task-assigned / task-188` —
  honest pre-verification state; flipping it to implemented is the
  post-verification step, correctly not pre-claimed here.
- Pre-existing posture unchanged by this task: pushes with no attestation
  chain at all are allowed (Phase 3 migration rationale, §7.2 row documents
  this separately); attestation-store query errors are non-blocking. These
  predate the candidate and are outside §2.4's binding requirement.

## Sandbox restrictions (for host verification)

TCP `accept()` never completes in this sandbox (errno 95), so listener-based
tests (ws::, tty::, smart-http-clone) cannot run here — pre-existing tests,
none touching the enforcement paths changed by this task. Full workspace
suite, all-target clippy, and exact-head GitHub CI remain with the
verification stage. `check-crypto-verify.sh` / `check-assertionless-tests.sh`
fail under mawk 1.3.4 identically at the base commit (gawk-only syntax;
environment, not branch).
