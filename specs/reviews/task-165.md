# Review: TASK-165 — Supply chain — gyre-stack.lock parsing and attestation level enforcement

**Reviewer:** Independent review (pi runtime)
**Candidate:** `26cc85e587f1eabe9fe25a8081991ef1f2eaaa44`
**Base:** `fefde2946f3a0af1f212588f54591516fee0f7b2`
**Round:** R2 (repair round for review `7de814ab95e745639162e0bbb564e4ad` on candidate `66677b96`)
**Date:** 2026-10-10
**Verdict:** NOT APPROVED — one new finding (F1 below). All prior-round findings verified fixed.

## Prior-round findings — resolution verified

- **Migration collision (prior blocking finding): FIXED.** Renumbered to
  `2026-10-10-000059_agent_attestation_level`; no version collision remains.
  Independent evidence: `bash scripts/check-migration-versions.sh` exit 0;
  `cargo test -p gyre-adapters sqlite::agent` → 17 passed, 0 failed (each test
  builds a fresh `SqliteStorage` which replays every embedded migration, then
  INSERT/SELECTs agents with `attestation_level` — a dropped migration would
  fail all 17 with 'no such column', as it did at candidate 66677b96).
  Migration SQL is plain `ALTER TABLE ... ADD/DROP COLUMN` — portable across
  SQLite and PostgreSQL.
- **Lockfile drift gated on repo policy (prior spec-fidelity note): FIXED.**
  `enforce_stack_policy` now runs the gyre-stack.lock integrity + drift check
  whenever the pushed tree carries a lockfile and the pushing agent has an
  attested stack, independent of a policy entry; without a policy, findings
  default to warn (ConstraintViolation event + accept). New test
  `lockfile_drift_without_policy_warned_and_accepted` covers it.

## Independent verification performed (logs in /tmp/stage/review-evidence/)

- Focused suites at candidate `26cc85e5`: `gyre-domain stack` 9 passed;
  `gyre-server git_http::` 46 passed (all 8 task-165 push tests, real git
  round-trips with ref-undo and event assertions); `gyre-server
  api::stack_attest` 14 passed; `gyre-adapters sqlite::agent` 17 passed;
  `cargo build -p gyre-cli` exit 0.
- **Kill-check (independent):** with `enforce_stack_policy` temporarily
  neutered (early `Ok(1)` return), 7 of the 8 task-165 push tests fail
  (only `lockfile_match_accepted` survives, correctly — neutering also
  accepts). The tests are load-bearing, not self-confirming. Patch reverted;
  working tree verified clean and suite re-run green (46 passed).
- Mechanical gates: `check-arch.sh`, `check-migration-versions.sh`,
  `check-abac-route-registry.sh` all exit 0. Full check-script battery run at
  both candidate and base in a clean worktree: failure sets are identical
  (34 pre-existing environmental failures — mawk syntax errors, usage-arg
  requirements — fail at base too) **except one**: `check-event-payload.sh`
  passes at base and fails at the candidate → F1.
- L3 production path verified real: spawn.rs writes
  `workload_attestations` KV with container_id/image_hash for M19.4 container
  targets (`attest_agent_with_container`); `resolve_push_attestation_level`
  reads exactly that KV; the `level3_detected_from_workload_attestation` test
  seeds the KV the same way spawn.rs does. The JWT `wl_container_id`/
  `wl_image_hash` branch has no production minting site (`mint_with_workload`
  is only called from `#[cfg(test)]` code in specs.rs) — honestly recorded in
  coverage row 9 as defense-in-depth, consistent with the task plan's scoping
  of L3 detection to container identity.
- New route `/api/v1/repos/:id/stack-policy` is registered in the ABAC
  resolver (`abac_middleware.rs:129`), Admin-role-checked in the handler,
  validates `min_attestation_level` 1..=3 and enforcement mode. No exemption
  files grew; `scripts/` is byte-identical to base.
- Coverage rows 7/8/9/12 record real behavior and honest partials (unsigned
  lockfile, JWT branch unwired, no SPIFFE/eBPF).

## Findings

### F1 (new, blocking this round) — ConstraintViolation payloads missing spec-required fields; event-payload gate regressed

`crates/gyre-server/src/git_http.rs` lines 1424 and 1484 (both emit sites in
`enforce_stack_policy`; line 1424 from task commit `96dfd8b8`, line 1484 from
repair commit `c1ad62f9`): the two new `MessageKind::ConstraintViolation`
emissions (below-minimum warn mode; no-policy lockfile-drift warn flag) carry
only `agent_id`/`repo_id` plus task-specific fields and omit six
spec-required fields for this message kind — `attestation_id`,
`constraint_name`, `expression`, `context_snapshot`, `action`, `timestamp`
(authorization-provenance.md §7.5; the schema was established as a review
finding in specs/reviews/task-007.md F1).

Reproduce: `bash scripts/check-event-payload.sh` at the candidate exits 1
with "6 violation(s) found" (2 emit sites × 6 missing fields); the same
script exits 0 at base `fefde294` (verified in a clean worktree). The gate is
a pre-commit hook (`.pre-commit-config.yaml` id `event-payload`) and its
header states "Run by pre-commit and CI" — the repo's standing constraints
keep the existing workflow machinery intact. `git_http.rs` had zero
ConstraintViolation emissions at base; both were added by this task.

**Fix:** populate the spec-required fields in both payloads —
`constraint_name` (e.g. `stack_policy_min_attestation_level` /
`stack_lockfile_drift`), `expression` (the evaluated rule), `context_snapshot`
(resolved level / min / fingerprint context), `action: "push"`, `timestamp`,
and `attestation_id` (the pushing agent's attested stack fingerprint or
workload-attestation record id) — matching the compliant pattern in
`constraint_check.rs:298-311`. Alternatively, if a field genuinely does not
apply, add the `// event-payload:ok` marker with spec justification per the
script's remediation guidance.

### Non-blocking notes (no action required this round)

- Multi-ref edge: the drift loop breaks after the first ref update whose tree
  carries a gyre-stack.lock; a single push updating two branches that carry
  *different* lockfiles only checks the first. Single-ref pushes (the normal
  case and everything the tests exercise) are unaffected.
- The lockfile fingerprint is self-verifying (recomputed from components) but
  not approver-signed — an attacker with repo write access can regenerate it.
  Already recorded as a known partial in coverage row 7; consistent with the
  task plan.

## Verdict

NOT APPROVED. The task's core contract — lockfile schema, `gyre stack lock`
CLI, three-level resolution with a real L3 production path, per-level policy
with block/warn enforcement, ungated drift detection — is genuinely
implemented and independently verified, and both prior-round findings are
properly fixed. But the candidate regresses a repo-enforced, spec-derived
gate that passes at the assigned base (F1), reintroducing the exact defect
class task-007 F1 fixed. One small repair round should clear it.
