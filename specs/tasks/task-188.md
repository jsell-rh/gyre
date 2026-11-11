---
title: "Enforce SignedInput context binding (replay prevention) at verification"
spec_ref: "authorization-provenance.md §2.4"
depends_on: []
progress: complete
coverage_sections:
  - "authorization-provenance.md §2.4 Context Binding (Replay Prevention)"
commits: ["f293796daa7ba9edbb9875c56873ebb9ecc6df5a", "10d235004c24553468a4ebeb9fc55a1efd6e5714", "265cb5d2eade8aab0d9063856d6607a746a82a9b", "2a3907082535dbf9f4a84d5cb40ae9bc4ed6c905"]
---

## Spec Excerpt

From `specs/system/authorization-provenance.md` §2.4:

> ### 2.4 Context Binding (Replay Prevention)
>
> Each `SignedInput` is bound to a specific context:
>
> - `workspace_id` + `repo_id` — the input cannot be replayed to a different repo.
> - `spec_sha` — the input is bound to a specific spec version. A modified spec produces a different SHA, requiring a new approval and new `SignedInput`.
> - `expected_generation` — optional monotonic counter. If present, the input is only valid for a specific generation of the deployment (task assignment). Prevents replay of an old authorization after a task has been reassigned.
> - `valid_until` — hard expiry. After this time, the authorization is invalid regardless of other checks.

Residual-risk statement (§1.2) this section mitigates:

> A compromised platform can: **Replay valid attestations** to the wrong context (mitigated by context binding — §2.4)

`InputContent` fields available on the signed root (§2.2): `spec_path`, `spec_sha`, `workspace_id`, `repo_id`, `persona_constraints`, `meta_spec_set_sha`, `scope`. `SignedInput.expected_generation: Option<u32>`, `SignedInput.valid_until: u64`.

## Problem

The signed `workspace_id`/`repo_id`/`spec_sha` and `expected_generation` are stored and signature-covered but **never compared against the actual verification context**. Grep confirms zero `content.repo_id` / `content.workspace_id` comparisons and `expected_generation` is only ever `None` or a test fixture — never read at verify time. Only `valid_until` (git_http.rs:3109) and signature integrity are enforced today. Result: a valid `SignedInput` authorizing work on `repo-A` can be replayed to authorize a push/merge on `repo-B`. The core requirement — "the input cannot be replayed to a different repo" — is unimplemented. This section is **hollow**, not implemented.

## Implementation Plan

Add a real, fail-closed context-binding check that runs at every verification boundary and compares the signed root `SignedInput.content` against the actual target of the push/merge.

1. **New verification function** in `crates/gyre-server/src/git_http.rs` (near `verify_chain`, ~line 3235), e.g. `verify_context_binding(root: &SignedInput, target_repo_id: &str, target_workspace_id: &str, task_spec_sha: Option<&str>, task_generation: Option<u32>, now: u64) -> VerificationResult`. It MUST:
   - Compare `root.content.repo_id` against `target_repo_id`; mismatch → `valid: false`, label `context_binding.repo_id`.
   - Compare `root.content.workspace_id` against `target_workspace_id`; mismatch → `valid: false`, label `context_binding.workspace_id`.
   - When `task_spec_sha` is `Some`, compare `root.content.spec_sha` against it; mismatch → `valid: false`, label `context_binding.spec_sha` (a spec modified without re-approval must not authorize work).
   - When `root.expected_generation` is `Some(g)`, require `task_generation == Some(g)`; mismatch (or missing) → `valid: false`, label `context_binding.expected_generation`.
   - `valid_until` remains enforced where it already is; do not duplicate.
   - Fail closed: any error resolving the target or a missing required comparison value yields `valid: false`, never allow.
2. **Real generation counter.** Add `generation: u32` (default `1`) to `Task` (`crates/gyre-domain/src/task.rs`), thread it through the SQLite + Postgres task adapters (schema migration + row mapping in `crates/gyre-adapters/src/sqlite/task.rs` and `crates/gyre-adapters/src/postgres/task.rs`) and the API DTOs. Increment `generation` whenever `assigned_to` changes in `update_task` (`crates/gyre-server/src/api/tasks.rs:281`). This makes `expected_generation` a real, looked-up value rather than a hardcoded stand-in.
3. **Wire enforcement** into `crates/gyre-server/src/constraint_check.rs`:
   - In `enforce_push_constraints` (line 232) and `enforce_merge_constraints` (line 460): after `verify_chain` succeeds and the root `signed_input` is resolved, call `verify_context_binding` with the real `repo_id`/`workspace_id` arguments already in scope, the task's current spec SHA (look up the task's `spec_path` and resolve its approved SHA from the spec ledger; if unavailable, pass `None`), and the task's current `generation` (via `state.tasks.find_by_id(task_id)`). If the result is not `valid`, reject: return `Err(...)` with a human-readable message naming the failing binding, and emit the `attestation.chain_invalid` audit event + `ConstraintViolation` message (reuse the existing emit path used for chain-invalid at line 289-320).
   - Mirror the check (audit-only, no rejection) into `evaluate_push_constraints` (line 40) and the audit-only merge path so Phase 2 observability records mismatches without blocking.
   - Also add the same call inside `verify_attestation_audit_only` is NOT required (that function has no target context); do the binding check only where target context is available.
4. **No spec amendment needed** — the spec is correct; this is pure implementation.

## Acceptance Criteria

- `enforce_push_constraints` and `enforce_merge_constraints` REJECT (return `Err`) when the signed root `SignedInput.content.repo_id` or `.workspace_id` does not equal the actual target repo/workspace. A `SignedInput` for `repo-A` cannot authorize a push or merge targeting `repo-B`.
- Enforcement REJECTS when `content.spec_sha` no longer matches the task's currently approved spec SHA.
- When `expected_generation` is `Some(g)`, enforcement REJECTS if the task's current `generation != g`; `Task.generation` is persisted, defaults to 1, and increments on reassignment.
- Rejections emit the `attestation.chain_invalid` audit event and a `ConstraintViolation` message; audit-only paths log the mismatch without blocking.
- Checks fail closed: unresolved target/task never results in "allow."
- Tests (in `constraint_check.rs` and/or `git_http.rs`) that FAIL if the binding check is removed:
  - `enforce_push` rejects a chain whose root repo_id ≠ target repo_id (real chain, real signature, only the repo differs).
  - `enforce_merge` rejects a chain whose root workspace_id ≠ target workspace_id.
  - `expected_generation = Some(2)` against a task at `generation = 1` is rejected; matching generation passes.
  - A matching context (all four bindings equal) still passes so the check does not over-reject legitimate work.
- `cargo build --all` and the touched crates' tests pass. `bash scripts/check-arch.sh` passes (no hexagonal boundary violation — the binding logic lives in `gyre-server`, target lookups go through existing ports).

## Agent Instructions

- Do NOT add a structural-only check (e.g., "field is non-empty"). The comparison MUST be against the real target of the current push/merge.
- Do NOT set `expected_generation` to a hardcoded constant to make the path fire; the compared generation MUST be looked up from the persisted `Task.generation`.
- Reuse the existing `VerificationResult` tree shape and the existing violation/audit emission helpers; do not invent a parallel notification path.
- Verify route/handler wiring by reading `crates/gyre-server/src/api/tasks.rs` for the reassignment path before editing.
- Run only the touched crates' tests plus `scripts/check-arch.sh`; do not run the full workspace suite or formatters as part of this task.

## Shipped

SignedInput §2.4 context binding is enforced at every verification boundary
with real comparisons against the actual push/merge target; a valid
authorization for repo-A/workspace-A cannot authorize work on
repo-B/workspace-B.

### Implementation

- **`verify_context_binding`** (`crates/gyre-server/src/git_http.rs`):
  compares the signed root `InputContent` against the real target —
  `repo_id`, `workspace_id`, `spec_sha` (vs the task's currently approved
  SHA), and `expected_generation` (vs the task's persisted generation) —
  each as a `context_binding.*` child of the existing `VerificationResult`
  tree. `valid_until` stays where it already was (not duplicated). Fails
  closed: unresolvable task means `generation = None`, so any generation
  pin rejects.
- **Enforcement** (`crates/gyre-server/src/constraint_check.rs`):
  `enforce_push_constraints` and `enforce_merge_constraints` call the check
  after `verify_chain` succeeds, using the real `repo_id`/`workspace_id`
  already in scope. Mismatch rejects with `Err` naming the failing
  binding(s) plus the `attestation.chain_invalid` audit event and
  `ConstraintViolation` messages/Inbox notifications via the existing
  emission path. Audit-only mirrors in `evaluate_push_constraints` and
  `evaluate_merge_constraints` log
  `attestation.context_binding_mismatch` without blocking (Phase 2).
- **Real generation counter**: `Task.generation` (default 1) persisted in
  the domain, SQLite + Postgres adapters (portable migration
  `2026-10-08-000060_task_generation`, renumbered from 000059 after the
  dbf9edaea base merge introduced a same-version sibling) and exposed in
  `TaskResponse`.
  Bumped on every reassignment to a different agent in all four mutation
  paths: `update_task` (PUT /api/v1/tasks/:id), spawn reassignment, admin
  reassign, MCP reassign.

### Test evidence

Mutation-verified tests (with the binding block disabled they FAIL):
`enforce_push_rejects_repo_id_mismatch`,
`enforce_push_rejects_stale_generation`,
`enforce_push_rejects_spec_sha_mismatch`,
`enforce_merge_rejects_workspace_id_mismatch`,
`verify_context_binding_unit_matrix`;
`enforce_push_matching_generation_passes` /
`enforce_push_matching_context_passes` guard against over-rejection. All
use real git repos, real Ed25519 signatures, and real persisted state.
`task_generation_increments_on_reassignment` (port) and
`update_task_api_reassignment_bumps_generation` (PUT API) prove the bump.

On the current head (merge of repair base dbf9edaea, then commit
f293796da): focused filters pass — `constraint_check` 40/40,
`verify_context_binding` 1/1, `admin_reassign` 3/3, `api::spawn` 52/52
(both former conflict files) — `cargo build --all` (SKIP_WEB_BUILD=1)
succeeds, and `check-arch.sh`, `check-migration-versions.sh`,
`check-migration-sql-portability.sh` all exit 0. Evidence:
`/tmp/stage/review-evidence/` (per-round details, including earlier
full-suite runs: gyre-server lib 1493 passed, gyre-adapters 383 passed /
12 ignored, on head 10d235004).

### Merge-resolution rounds

Four base merges were resolved (fefde294, fb1f82e31, then repair base
dbf9edaea), each verified by re-running the focused tests and mechanical
gates on the merged head. The fb1f82e31 round had a single conflict in
`api/spawn.rs` (scoped secret resolution): resolved in favor of the base
version, which functionally supersedes the branch side by adding the
Platform Model §7 `secret.accessed` audit event; the branch side was a
comment-only rewording with identical behavior.

The dbf9edaea round had two conflicts. `gyre-domain/src/task.rs`: the
branch's `generation: 1` field initialization was combined with the
base's explicit `created_at`/`updated_at` initialization in `Task::new`
(both sides kept). `api/spawn.rs` again: the base's task-106
jj-workspace-preferred `effective_work_dir` resolution was kept — it
functionally supersedes the branch side (comment-only rewording) and
still canonicalizes relative worktree paths. The merge also collided on
Diesel migration version 20261008000059 (base `spec_lifecycle_configs`
vs branch `task_generation`): the branch migration was renumbered to
`2026-10-08-000060_task_generation` (content unchanged) in commit
f293796da so both migrations run. Task-188 code was otherwise unchanged
by every merge round.

### Scope note

The spec-approval signing endpoint (`api/specs.rs`) still creates
SignedInputs with `expected_generation: None` — the signer-side pin is not
exposed in the `ApproveSpecRequest` API. Enforcement (comparison against
the persisted `Task.generation`) is fully implemented and
mutation-tested; exposing a signer-side pin would extend the request
contract and was not part of this task's enumerated scope.
