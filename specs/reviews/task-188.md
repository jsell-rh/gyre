# Review — task-188 (SignedInput §2.4 Context Binding / Replay Prevention at verification)

Spec: `specs/system/authorization-provenance.md` §2.4 "Context Binding (Replay Prevention)".
Candidate: `a8cd5c8251699646f8a7f69084a31b36e1b3ca85` (assigned base `2427ccc1`).
Verdict: **approved** (independent evidence supports the task contract; no findings).

## What was verified

**Real enforcement, not structural.** `verify_context_binding`
(`crates/gyre-server/src/git_http.rs`) compares the signature-covered root
`InputContent` against the actual target of the current push/merge:
`repo_id`, `workspace_id`, `spec_sha` (against the task's currently approved
SHA), and `expected_generation` (against the task's persisted generation),
each as a `context_binding.*` child of the existing `VerificationResult` tree.
Fail-closed confirmed: `expected_generation = Some(g)` with an unresolvable
task (`generation = None`) is invalid; `valid_until` is not duplicated.

**Wired at the real boundaries.** `enforce_push_constraints`
(`constraint_check.rs`; push call site `git_http.rs:348`, rejection undoes ref
updates) and `enforce_merge_constraints` (merge call site
`merge_processor.rs:1412`, rejection fails the queue entry) run the check
after `verify_chain` succeeds, using the real `repo_id`/`workspace_id` from
the push/merge context. Mismatch returns `Err` naming the failing binding(s)
and reuses the existing emission path (`attestation.chain_invalid` audit
event, `emit_constraint_violations`, `create_violation_notifications`).
Audit-only mirrors in `evaluate_push_constraints` /
`evaluate_merge_constraints` log `attestation.context_binding_mismatch`
without blocking, per the plan.

**Real generation counter.** `Task.generation` (default 1) persisted through
the domain type, SQLite + Postgres adapters, and portable migration
`2026-10-08-000056_task_generation` (next unused sequence). Bumped on
reassignment in all four mutation paths (PUT /tasks/:id, spawn reassignment,
admin reassign, MCP reassign), each guarded to bump only on a real assignee
change. `resolve_task_binding_context` looks the generation up from
`state.tasks.find_by_id` — no hardcoded stand-in.

## Probe evidence (full detail in `/tmp/stage/review-evidence/summary.md`)

- `cargo test -p gyre-server --lib -- constraint_check` → 40 passed / 0
  failed at the exact candidate head; full gyre-server lib suite 1222/0,
  gyre-domain 371/0, gyre-adapters 344/0 (12 pg ignored), gyre-cli check
  clean.
- **Mutation-verified**: neutralizing the push binding block makes
  `enforce_push_rejects_repo_id_mismatch`, `enforce_push_rejects_stale_generation`,
  and `enforce_push_rejects_spec_sha_mismatch` FAIL (exit 101); neutralizing
  the merge block makes `enforce_merge_rejects_workspace_id_mismatch` FAIL.
  The tests use real git repos, real Ed25519 signatures over content that
  differs only in the bound field, and pass `verify_chain` first, so only the
  binding comparison decides the outcome. Mutations reverted; tree clean.
- Changed-line CI gates at the assigned base: `check-rustfmt-diff.py` clean;
  clippy changed-line filter clean for both the normal and `--profile test`
  compilations of the touched crate. (The rustfmt/clippy failures retained in
  `/tmp/stage/findings.log` reference line numbers from an earlier candidate
  state and do not reproduce at the assigned head.) `--all-targets` workspace
  clippy deferred to verification per executor wrapper restrictions.
- Mechanical gates all EXIT 0: check-arch, migration-versions,
  migration-sql-portability, fabricated-scope-defaults, lossy-secret-conversion,
  relative-path-defaults, mem-port-contracts, scope-literal-defaults,
  inert-enforcement, task-commit-attribution.

## Notes (non-blocking)

- The diff additionally fixes two pre-existing exemption sites (task-097 F3
  fabricated-`"default"`-tenant fallback in `api/spawn.rs` secret resolution
  and `constraint_check.rs` violation-notification tenant fallback; task-097
  F4 lossy UTF-8 secret conversion) and lowers the frozen exemption counts
  exactly as the exemption files prescribe ("fix a site, delete its line,
  lower FROZEN_EXEMPTION_COUNT"). Both touched check scripts pass at head; no
  entries were added. This is a safe-scope improvement in files the task
  already touched, though it was not called out in the shipped notes.
- Signer-side `expected_generation` pin is not exposed in
  `ApproveSpecRequest` (`api/specs.rs` still creates roots with `None`).
  Enforcement against the persisted generation is complete and
  mutation-tested; the acceptance criteria require enforcement behavior only,
  and the shipped notes explicitly record the request-contract extension as
  out of scope.
- Coverage row §2.4 remains `task-assigned task-188` in
  `specs/coverage/system/authorization-provenance.md` — accurate while the
  task is in review; the audit stage owns the matrix transition.
