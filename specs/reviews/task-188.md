# Review — task-188 (Enforce SignedInput context binding — replay prevention — at verification)

Spec: `specs/system/authorization-provenance.md` §2.4 Context Binding (Replay Prevention), mitigating §1.2 "Replay valid attestations to the wrong context".
Candidate: `a41967543` (branch pipeline/task-188/07d3f58b6c1f4c6b88d91c60c27f7c69-19) against base `dbf9edaeae`. Product commits: `265cb5d2`, `2a390708`, `10d235004`, `f293796da` (all recorded in `commits:` frontmatter; attribution gate exit 0).
Verdict: **complete** (approved).

## Round 1

Probes (all at the candidate; evidence with exit codes under `/tmp/stage/review-evidence/`, summary in `probe-summary.md`):

- `cargo test -p gyre-server --lib constraint_check` → **40 passed, 0 failed**.
- **Mutation probe**: `verify_context_binding` body temporarily replaced with an unconditional `valid: true` early return; re-run → **6 tests FAILED** (`enforce_push_rejects_repo_id_mismatch`, `enforce_merge_rejects_workspace_id_mismatch`, `enforce_push_rejects_stale_generation`, `enforce_push_rejects_spec_sha_mismatch`, `update_task_api_reassignment_bumps_generation`, `verify_context_binding_unit_matrix`); the matching-context guards still passed. File restored; tree verified pristine (`git status`/`git diff HEAD` empty). The tests are mutation-sensitive, not self-confirming.
- `cargo test -p gyre-adapters --lib sqlite::task` → 11 passed (generation column round-trips through the adapter).
- `cargo test -p gyre-server --lib api::spawn` → 52 passed; `--lib reassign` → 5 passed; `--lib mcp` → 89 passed (both former merge-conflict surfaces green on the merged head).
- `cargo check -p gyre-cli` → clean (new `Task.generation` field breaks no CLI construction).
- Gates: `check-arch.sh`, `check-migration-versions.sh`, `check-migration-sql-portability.sh`, `check-fabricated-scope-defaults.sh`, `check-lossy-secret-conversion.sh`, `check-task-commit-attribution.sh` — all exit 0.
- Deferred to verification (executor wrapper rejects workspace-wide invocations, exit 64 — infrastructure restriction, not a code defect): `cargo build --all` and the full workspace suite. Every touched crate compiled in focused probes.

Verified working (no findings):

- **`verify_context_binding`** (git_http.rs) performs the four real comparisons from §2.4 — `content.repo_id`, `content.workspace_id`, `content.spec_sha` (vs the task's currently approved SHA), `expected_generation` (vs the task's persisted generation) — each as a `context_binding.*` child of the existing `VerificationResult` tree. `valid_until` is correctly NOT duplicated: it is enforced inside `verify_attestation_audit_only` (git_http.rs:4030-4041), which `verify_chain` invokes per node (git_http.rs:4178), so it already covers both enforce paths.
- **Enforcement is against the real target, not a structural check.** `enforce_push_constraints` is called from the push handler (git_http.rs:591) with the `repo_id` from the push URL and the repo's actual workspace; `enforce_merge_constraints` is called from the merge processor (merge_processor.rs:2189) with the MR's repo/workspace. Mismatch → `Err` naming the failing binding(s), `attestation.chain_invalid` audit event, and `ConstraintViolation` events + priority-2 Inbox notifications via the existing `emit_constraint_violations`/`create_violation_notifications` path — no parallel notification channel was invented.
- **Fail-closed**: an unresolvable task yields `generation = None`, so any `expected_generation` pin rejects (`None != Some(g)`); the repo/workspace bindings need no task lookup and always run. `spec_sha` is skipped only when unresolvable — exactly what the task plan directs ("if unavailable, pass `None`").
- **Real generation counter**: `Task.generation` (default 1) in the domain, threaded through SQLite + Postgres adapters with portable migration `2026-10-08-000060_task_generation` (renumbered after the base merge introduced a same-version `000059` sibling — both now run; no duplicate versions), exposed in `TaskResponse`, and bumped on reassignment in all four mutation paths: `update_task` (PUT /api/v1/tasks/:id), `spawn_agent_core`, `admin_reassign_agent`, MCP `handle_update_task`. The compared generation is always looked up from persisted state, never hardcoded.
- **Audit-only mirrors** in `evaluate_push_constraints` and `evaluate_merge_constraints` log `attestation.context_binding_mismatch` per failing binding without blocking (Phase 2 observability), as required.
- **Tests are real**: real git repos, real Ed25519 keypairs signing the full `InputContent` (so only the binding comparison decides), real persisted task/agent/MR/approval state, and the API-level test drives the actual router (`app.oneshot`) through PUT /tasks/:id before asserting the stale generation-1 authorization is rejected end-to-end. Mutation probe proves all four required rejection scenarios fail when the check is disabled; the two matching-context tests guard against over-rejection and still pass under mutation.
- **Merge resolutions verified**: the dbf9edaea merge kept both sides' `Task::new` initializers (generation + timestamps), kept the base's functionally-superseding `api/spawn.rs` versions (secret-accessed audit; jj-workspace `effective_work_dir`), and renumbered the migration; spawn (52) and admin_reassign (3) tests confirm no regression.
- Exemption-file edits are comment-only documentation of previously fixed sites; no new entries; both checks exit 0.

Non-blocking observations (recorded for the next audit cycle / process owner):

1. `specs/coverage/system/authorization-provenance.md` row 10 still reads "task-assigned | task-188 | Hollow — replay-prevention enforcement absent" — now stale in the conservative direction. Coverage flips are not part of this task's contract (precedent: task-189 approved complete without one); the §2.4 row should be re-verified and flipped to `implemented` with landed evidence at the next audit.
2. `spawn_agent_core` bumps `generation` on first assignment of a previously unassigned task (`None ≠ Some(agent)`) — a superset of "increments on reassignment", fail-safe direction, no behavioral impact while the signing endpoint never pins `expected_generation`.
3. Signer-side pin: `api/specs.rs` still creates SignedInputs with `expected_generation: None`; exposing a pin in `ApproveSpecRequest` was out of the enumerated scope and is honestly documented in the Shipped note. The enforcement side is complete and mutation-tested; if the signer-side pin is wanted, it is a small follow-up task.

Test-inflation check: none found — every new test either fails under mutation or guards a real over-rejection risk. Scope check: no out-of-scope edits; the only non-task files touched are comment-only exemption-file documentation and the task/coverage bookkeeping.
