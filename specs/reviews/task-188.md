# Review — task-188 (Enforce SignedInput context binding (replay prevention) at verification)

Spec: `specs/system/authorization-provenance.md` §2.4 Context Binding (Replay Prevention) —
a `SignedInput`'s signed `workspace_id`/`repo_id`/`spec_sha` and its `expected_generation`
must be compared against the actual push/merge target at verification, so a valid
authorization for repo-A cannot be replayed to authorize work on repo-B, and stale
authorizations cannot survive spec change or task reassignment.

Commits under review: `2a390708` (implementation checkpoint), `84a5f970` (merge of base
`7f8b5d9c`), `265cb5d2` (re-enable of the push-path binding check that `2a390708` had left
as `if false`), `fe569ee1` (task frontmatter). Cumulative diff:
`git diff 7f8b5d9c..fe569ee1` (+1212/−63 across 16 files; product surface:
`git_http.rs`, `constraint_check.rs`, `api/{tasks,admin,spawn}.rs`, `mcp.rs`,
domain/adapters `task.rs` + migration, `schema.rs`).

Verdict: **complete** (approved; findings: none blocking).

## Sandbox restriction (infrastructure, not a code defect)

This review sandbox has no crates.io network (`curl https://index.crates.io/config.json` →
exit 7), an empty `CARGO_HOME` (no registry cache), no `target/` artifacts, and no vendored
deps — Rust compilation and test execution are impossible here. Evidence and the exact
deferred commands for the verifier are recorded in `/tmp/stage/review-evidence/`
(`build-restriction.md`). Everything below is either executable-here evidence (mechanical
gates, real-SQLite migration probe) or full static trace of the diff.

## What was verified

**Enforcement is real and fail-closed, at every boundary with target context.**

- `verify_context_binding` (`git_http.rs:3410`) compares the signed root `InputContent`
  against the caller's real target: `repo_id` and `workspace_id` exact-string comparisons
  against the actual push/merge target, `spec_sha` against the task's currently approved SHA
  (empty signed SHA can never match), and `expected_generation` requiring exact
  `Some(g)` equality with the task's persisted generation. `valid_until` is not duplicated —
  it remains in `verify_attestation_audit_only`, which `verify_chain` invokes per node, so
  the enforce paths check expiry too. Unresolvable task ⇒ `generation = None` ⇒ any
  generation pin rejects.
- Wired at all four verification boundaries with real target values:
  `enforce_push_constraints` (`constraint_check.rs:357`, called from `git_http.rs:348` with
  the push's resolved repo id and the repo's real workspace id — no fallback), and
  `enforce_merge_constraints` (`:617`, from `merge_processor.rs:1412` with `repo.id` and
  `mr.workspace_id`), both rejecting with `Err` naming the failing binding(s) and reusing the
  existing emission path (`attestation.chain_invalid` audit event +
  `ConstraintViolation` events + Inbox notifications); audit-only mirrors in
  `evaluate_push_constraints` (`:91`) and `evaluate_merge_constraints` (`:862`) log
  `attestation.context_binding_mismatch` without blocking, per the Phase 2 requirement.
- Process integrity: at checkpoint `2a390708` the push-path check was dead code
  (`let _ = &binding_result; if false {`); commit `265cb5d2` re-enabled it. Final candidate
  grep confirms all four call sites live, no `if false`/`#[ignore]`/discard patterns remain.

**The generation counter is real persisted state, not a stand-in.**

- `Task.generation` (default 1) in the domain struct, threaded through SQLite and Postgres
  row mapping, migration `2026-10-08-000057_task_generation` (portable SQL — verified on real
  SQLite 3.45.1: existing rows backfill to 1, NOT NULL DEFAULT 1 holds for new rows, down.sql
  drops the column; Diesel version `20261008000057` is unique — the two `000056` directories
  differ by date prefix so they do not collide), exposed in `TaskResponse`.
- Bumped on assignee change in all four mutation paths: `PUT /api/v1/tasks/:id` (only when
  the assignee actually differs), spawn reassignment, admin reassign, MCP reassign.
- `resolve_task_binding_context` looks both values up through ports (`tasks.find_by_id`,
  `spec_approval_history.list_by_path` newest non-revoked approval, `spec_ledger`
  fallback) — `check-arch.sh` passes; no hardcoded constants anywhere on the compared path.

**Test kill-power (static proof, execution deferred).**

The fixtures use real git repos, real Ed25519 keypairs signing the real `InputContent`, and
real mem-port state. The matching-context tests (`enforce_push_matching_context_passes`,
`enforce_push_matching_generation_passes`) assert `Ok(())` with fixtures identical to the
mismatch tests except for the signature-covered binding fields — so with the binding check
removed, the mismatch tests' pipelines are byte-for-byte the passing pipelines and their
`is_err` assertions must fail. `enforce_push_rejects_spec_sha_mismatch` additionally proves
the real approval-history lookup; `update_task_api_reassignment_bumps_generation` drives the
real PUT route and then proves the stale-pinned authorization is rejected at the enforcement
boundary; `verify_context_binding_unit_matrix` covers each binding independently including
the fail-closed (pin vs unresolvable task) and skip (no pin / unresolvable spec SHA) cases.
The verifier must run these (commands in `build-restriction.md`).

**Over-rejection audit (signer interplay).** The production signer (`api/specs.rs:540-580`)
fills `InputContent.workspace_id`/`repo_id` from the spec ledger entry, which
`spec_registry.rs` backfills from the owning repo on sync. For the standard same-repo
provenance flow the bindings match the push target. A SignedInput whose spec has no repo
binding signs empty strings and is rejected at push time — fail-closed per the plan
("a missing required comparison value yields `valid: false`, never allow"), and rejecting
cross-repo pushes is the §2.4 contract itself.

**Mechanical gates** (all run here, from repo root): migration versions, migration SQL
portability, arch, fabricated-scope-defaults, lossy-secret-conversion, relative-path-defaults,
conditional-test-guards, inert-enforcement, scope-literal-defaults, mem-port-contracts,
in-memory-state-stores, forged-scope-fields, dead-message-kinds, forwarded-header-trust,
unbounded-external-http, byte-slice-truncation, no-em-dash, task-commit-attribution,
abac-exempt-handlers, api-auth, aspirational-test-names, fail-open-ref-resolution,
mcp-write-tools, mcp-wrapper-parity — **all exit 0**. `check-crypto-verify.sh`,
`check-assertionless-tests.sh`, `check-dead-test-code.sh` fail on mawk 1.3.4 (gawk-only
syntax) **identically at base `7f8b5d9c`** (byte-identical log pairs saved) — environment,
not branch; their intent was confirmed manually for this diff (real `ring` verify, all new
tests assert hard outcomes).

## Minor (not blocking, recorded for completeness)

1. `expected_generation` and `valid_until` are not signature-covered (the Ed25519 signature
   covers `InputContent` only, and the signer sets both server-side with
   `expected_generation: None` today). This is the pre-existing §2.2/§2.4 design, and the
   task explicitly scoped the signer-side pin exposure out; enforcement against the persisted
   `Task.generation` is what §2.4 requires of the verifier and is fully implemented.
2. Spawning onto a previously unassigned task (`None` → `Some`) bumps generation 1→2. This
   follows the plan's literal "increment whenever `assigned_to` changes", errs toward
   invalidation rather than replay, and has no behavioral impact while the signer never pins
   a generation.
3. The header comments added to `scripts/fabricated-scope-defaults-exemptions.txt` and
   `scripts/lossy-secret-conversion-exemptions.txt` claim spawn.rs sites were "fixed (entries
   deleted)" — but those files never listed spawn.rs entries (base spawn.rs already
   conformed; the accompanying spawn.rs change is a functionally-equivalent refactor). Pure
   prose inaccuracy in governance-file comments; no entries were added anywhere, both checks
   pass, and the one removal (relative-path-defaults, spawn.rs:597 prose literal) is
   legitimate.
4. The coverage matrix row (`specs/coverage/system/authorization-provenance.md` row 10) still
   reads `task-assigned`/Hollow — repo convention updates coverage rows in separate
   audit/verify commits (confirmed via `git log` on that file), so flipping it belongs to the
   verification/audit stage with this review as input, not to the task branch.

## Deferred to verification (exact commands in `/tmp/stage/review-evidence/build-restriction.md`)

1. `cargo test -p gyre-server --lib -- constraint_check` (expect all pass, including the nine
   binding tests named above).
2. Mutation proof: disable the two enforce-path binding blocks; expect the four reject tests
   to fail.
3. Touched-crate suites (`gyre-common`, `gyre-domain`, `gyre-adapters`) and `cargo build --all`.
