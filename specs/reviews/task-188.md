# Review — task-188 (authorization-provenance §2.4 — SignedInput Context Binding / Replay Prevention)

Spec: `specs/system/authorization-provenance.md` §2.4 "Context Binding (Replay Prevention)" (mitigates §1.2 "Replay valid attestations to the wrong context").
Base: `fefde294` · Candidate: `17cfe9f1684c11482dd8cd570a62bd7cc83c8eb3` (working tree clean at candidate throughout review; all temporary mutations restored, byte-verified).
Verdict: **approved**.

## Evidence (all commands and output under `/tmp/stage/review-evidence/`)

- `cargo test -p gyre-server --lib -- constraint_check` → **40 passed / 0 failed** (clean candidate), including all five context-binding tests and both generation-bump tests.
- **Independent mutation proof** (reviewer-run, not implementer-claimed):
  - Push-path binding enforcement neutralized in `enforce_push_constraints` (check still executed, `Err` return removed) → `enforce_push_rejects_repo_id_mismatch`, `enforce_push_rejects_spec_sha_mismatch`, `enforce_push_rejects_stale_generation`, and `update_task_api_reassignment_bumps_generation` all **FAIL** (36 passed / 4 failed); the merge test stays green, proving the paths are independent.
  - Merge-path binding enforcement neutralized in `enforce_merge_constraints` → `enforce_merge_rejects_workspace_id_mismatch` **FAILS**.
  - File restored from byte-copy; `git status --porcelain` empty afterward; clean focused suite re-ran green.
- **SQLite persistence probe** (temporary test, removed after run): `Task.generation` defaults to 1, round-trips through the real SQLite adapter + migration `2026-10-08-000059_task_generation`, and a bumped value (7) persists through `update`. The mem-side round-trip is covered by the in-tree tests; this probe closes the SQLite leg.
- Mechanical gates at candidate: `check-arch.sh`, `check-migration-versions.sh` (no duplicate versions; 000059 is the renumbered, unique sequence), `check-migration-sql-portability.sh` (plain portable `ALTER TABLE ... ADD COLUMN ... DEFAULT 1`), `check-mem-port-contracts.sh`, `check-fabricated-scope-defaults.sh`, `check-lossy-secret-conversion.sh`, `check-scope-literal-defaults.sh`, `check-inert-enforcement.sh`, `check-task-commit-attribution.sh`, `check-relative-path-defaults.sh` → all **EXIT 0**.
- `cargo test -p gyre-adapters --lib sqlite::task` → **10 passed / 0 failed**.

## Verified working (no findings)

- **`verify_context_binding`** (`git_http.rs:3424`): compares the signed root `InputContent` against the real target — `repo_id`, `workspace_id`, `spec_sha` (vs the task's currently approved SHA, newest non-revoked approval event with ledger fallback), and `expected_generation` (vs the task's persisted generation) — as `context_binding.*` children of the existing `VerificationResult` tree. `valid_until` is not duplicated (still enforced solely at `git_http.rs:3165`). Fails closed on the generation pin: unresolvable task ⇒ `task_generation = None` ⇒ any `Some(g)` pin rejects.
- **No circular comparison**: the target identity never comes from the attestation. Push targets come from `resolve_repo_by_slug` (route-resolved repo, `git_http.rs:267`); merge targets from `repo.id` / `mr.workspace_id` (`merge_processor.rs:1607`). A SignedInput for repo-A cannot authorize repo-B.
- **Enforcement wiring**: `enforce_push_constraints` (`constraint_check.rs:357`) and `enforce_merge_constraints` (:617) run the check after `verify_chain` and return `Err` naming the failing bindings, emitting `attestation.chain_invalid` + `ConstraintViolation` events + Inbox notifications via the *existing* path (`emit_constraint_violations` / `create_violation_notifications` — no parallel notification channel). Audit-only mirrors in `evaluate_push_constraints` (:91) and `evaluate_merge_constraints` (:862) log `attestation.context_binding_mismatch` without blocking, per the Phase 2 design.
- **Real generation counter**: `Task.generation` (default 1) in domain, SQLite + Postgres row mappings (insert and update), `TaskResponse` DTO, and bumped on assignee change in all four mutation paths — `update_task` PUT (`api/tasks.rs:308`), spawn reassignment (`api/spawn.rs:632`), admin reassign (`api/admin.rs:414`), MCP reassign (`mcp.rs:1040`) — each guarded by an actual-assignee-change check (the admin path loops over one agent's tasks moving them to another agent, so every iteration is a genuine reassignment).
- **Test quality**: real Ed25519 keypairs signing real `InputContent`, real git repos, real persisted state; assertions name error strings only the new enforcement produces; both matching-context tests guard against over-rejection. Not self-confirming — proven by the mutation runs above.
- **Merge-conflict resolution** (durable finding from the rebase): the `api/spawn.rs` conflict was resolved to a semantically equivalent structure (if-let vs match); the workspace-unresolvable warn branch is preserved, no behavior lost. The duplicate-migration-version hazard from the base merge was correctly handled by renumbering to `2026-10-08-000059` (unique, verified).
- **Exemption files**: the diff touches `fabricated-scope-defaults-exemptions.txt` / `lossy-secret-conversion-exemptions.txt` only with comment lines recording *deleted* (fixed) entries — the allowed direction; no new exemptions; both gates exit 0.

## Scope notes (not findings)

- The spec-approval signing endpoint (`api/specs.rs:701`) still creates SignedInputs with `expected_generation: None`; the signer-side pin is not exposed in `ApproveSpecRequest`. The task plan did not enumerate this (it prescribed only enforcement-side comparison against the persisted `Task.generation`, which is implemented and mutation-tested), and the spec makes the pin optional. Disclosed in the Shipped notes. If product wants human signers to pin generations, that is a follow-up contract extension.
- Coverage row 10 (§2.4) remains `task-assigned` in `specs/coverage/system/authorization-provenance.md`. This is consistent with repo process — the matrix is updated by dedicated audit commits after verification, and the implementing agent correctly did not self-mark the row. The row's "Hollow" note is now stale and should be re-audited post-merge.
- Postgres adapter changes mirror SQLite exactly; Postgres is never integration-tested repo-wide (portability covered by `check-migration-sql-portability.sh`), consistent with the established pattern.
