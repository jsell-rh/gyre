---
title: "Enforce SignedInput context binding (replay prevention) at verification"
spec_ref: "authorization-provenance.md §2.4"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "authorization-provenance.md §2.4 Context Binding (Replay Prevention)"
commits: ["e97df1461b20d6c16063e79c1af888ae4ba64856", "cf9549ab8b722c8bbeb3990d577fee92dc81681b"]
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

SignedInput §2.4 context binding is now enforced at every verification
boundary with real comparisons against the actual push/merge target; a valid
authorization for repo-A/workspace-A can no longer authorize work on
repo-B/workspace-B.

- **`verify_context_binding`** (`crates/gyre-server/src/git_http.rs:3367`):
  compares the signed root `InputContent` against the real target —
  `repo_id`, `workspace_id`, `spec_sha` (vs the task's currently approved
  SHA), and `expected_generation` (vs the task's persisted generation) —
  each as a `context_binding.*` child of the existing `VerificationResult`
  tree. `valid_until` stays where it already was (not duplicated). Fails
  closed: unresolvable task ⇒ `generation = None` ⇒ any generation pin
  rejects.
- **Enforcement** (`constraint_check.rs`): `enforce_push_constraints`
  (:357) and `enforce_merge_constraints` (:616) call the check after
  `verify_chain` succeeds, using the real `repo_id`/`workspace_id` already
  in scope (git_http.rs:348, merge_processor.rs:1412). Mismatch ⇒ `Err`
  naming the failing binding(s) + `attestation.chain_invalid` audit event +
  `ConstraintViolation` messages and Inbox notifications via the existing
  emission path. Audit-only mirrors in `evaluate_push_constraints` (:91)
  and `evaluate_merge_constraints` (:860) log
  `attestation.context_binding_mismatch` without blocking (Phase 2).
- **Real generation counter**: `Task.generation` (default 1) persisted in
  domain, SQLite + Postgres adapters (migration
  `2026-10-08-000056_task_generation`, portable SQL, next unused sequence
  number) and exposed in `TaskResponse`. Bumped on every reassignment to a
  different agent in all four mutation paths: `update_task` (PUT
  /api/v1/tasks/:id), spawn reassignment, admin reassign, MCP reassign.

Tests that kill removal of the check (mutation-verified): with the binding
block disabled, `enforce_push_rejects_repo_id_mismatch`,
`enforce_push_rejects_stale_generation`, and
`enforce_push_rejects_spec_sha_mismatch` FAIL; `enforce_merge_rejects_workspace_id_mismatch`
covers merge; `enforce_push_matching_generation_passes` /
`enforce_push_matching_context_passes` guard against over-rejection. All use
real git repos, real Ed25519 signatures, and real persisted state.
`task_generation_increments_on_reassignment` (port) and
`update_task_api_reassignment_bumps_generation` (PUT API) prove the bump.

Evidence: `cargo test -p gyre-server --lib -- constraint_check` → 40 passed /
0 failed; `gyre-common -p gyre-domain -p gyre-adapters` → 344+94+363 passed /
0 failed; `check-arch.sh`, `check-migration-versions.sh`,
`check-migration-sql-portability.sh`, `check-relative-path-defaults.sh`,
`check-conditional-test-guards.sh` → EXIT 0. Full run details and the
mutation proof at `/tmp/stage/review-evidence/test-results.md`.

Sandbox restrictions recorded for host verification
(`/tmp/stage/review-evidence/`): TCP `accept()` never completes (errno 95),
so listener-based `ws::`/`tty::`/smart-http-clone tests can't run here —
pre-existing tests, unchanged by this task, none touch the enforcement
paths. `check-crypto-verify.sh` / `check-assertionless-tests.sh` fail on
mawk 1.3.4 (gawk-only syntax) identically at the base commit — environment,
not branch; manual confirmation of their intent for this diff recorded in
`mechanical-checks.md`. Full workspace suite, all-target clippy, and
exact-head GitHub CI remain with the verification stage as mandated.

Scope note: the spec-approval signing endpoint (`api/specs.rs:610`) still
creates SignedInputs with `expected_generation: None` — the signer-side pin
is not exposed in the `ApproveSpecRequest` API. Enforcement (comparison
against the persisted `Task.generation`) is fully implemented and
mutation-tested; exposing a signer-side pin would extend the request
contract and was not part of this task's enumerated scope.
