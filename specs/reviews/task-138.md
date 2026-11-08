# Review: TASK-138 — Spec Approval Ledger with Database Schema and API

**Reviewer:** Independent review (stage)
**Date:** 2026-11-08
**Comparison base:** def0add9eb44d8e790aae3c1502317435104c0bf
**Candidate:** 6b1e893a225518dfcb1f24b37918f61ee2bae76f
**Verdict:** approved

---

## Scope

Reviewed the full diff base..candidate (16 files, +2176/−320): domain types,
port trait, migration 2026-11-08-000058, SQLite/Postgres/mem adapters,
approve/revoke/reject/approvals API handlers, forge enforcement
(`verify_spec_ref`, `require_current_spec`), and push-time invalidation.

## Findings

None blocking. Two observations checked and dismissed as non-defects:

- `let _ = state.spec_approvals.revoke_all_for_path(...)` and the
  path-counting `invalidated` log counter in the git_http push hook are
  byte-identical **base** code (def0add9e lines ~1480-1497); the candidate's
  only change there is the `ledger_path()` normalization (strip `specs/`)
  that makes revocation actually match ledger-keyed rows — a real fix.
- `caller_id` jwt-claims prefix asymmetry in revoke (jwt ⇒ "agent:" even for
  human-scope JWTs) is pre-existing base behavior, not introduced here.

## Behavior verification (this sandbox, exact tree)

- **Domain** (`gyre-domain/src/spec_approval.rs`): `ApprovalStatus` derived
  from timestamp columns (never stored); `approve`/`revoke`/`reject` enforce
  Pending → Approved → Revoked and Pending → Rejected, reject empty
  revocation reasons, and clear sibling timestamp columns. Suite 4/4.
- **Port + adapters**: `find_by_spec_sha` (exact (path, sha) version query)
  implemented in SQLite, Postgres, and mem with parallel contracts; mem
  enforces the duplicate-id create guard in code
  (check-mem-port-contracts.sh exit 0). SQLite suite 10/10 against a real
  temp-file DB, including `find_by_spec_sha_returns_only_that_version` and
  `create_duplicate_id_fails`.
- **Migration** `2026-11-08-000058_spec_approval_ledger`: all 12 spec-schema
  columns, nullable `approved_at`, indexes on (spec_path, spec_sha) and
  approver_id; no version collision (check-migration-versions.sh exit 0) and
  dual-dialect portable SQL (check-migration-sql-portability.sh exit 0).
- **API**: approve validates the requested 40-hex SHA against the ledger's
  current blob SHA (409 on mismatch, no partial writes) and creates +
  approves the durable ledger row; revoke requires a non-empty reason,
  restricts to original approver or Admin, writes an audit_events row, and
  recomputes ledger status §9-aware; reject transitions Pending ledger rows
  to Rejected, closes spec-edit MRs, and cancels in-flight tasks;
  `GET /api/v1/specs/approvals` returns full ledger rows with derived
  status/active. Server suites: `api::specs` 80/80, `api::gates` 10/10.
- **Forge enforcement (Provenance Chain step 8)**: `verify_spec_ref` queries
  the exact (path, sha) via `find_by_spec_sha` and accepts only active
  (Approved, not revoked/rejected) rows; storage failure fails closed (empty
  ⇒ deny). Wired into `merge_processor` under `require_approved_spec`;
  `require_current_spec` blocks stale SHAs via the new
  `resolve_spec_blob_sha` (ledger-path aware, strictly more capable than
  base's raw-path lookup; the None ⇒ non-stale semantics is pre-existing
  base behavior). Lifecycle trio passes:
  `push_modifying_spec_revokes_ledger_approvals` (real git repo, real push
  hook, asserts approval Revoked + mutual exclusivity + verify denial),
  `require_current_spec_blocks_stale_spec_ref`,
  `find_by_spec_sha_scopes_to_exact_version`.
- **Mutation probe**: replacing `find_by_spec_sha(path, sha)` with
  `list_by_path(path)` in `verify_spec_ref` makes
  `find_by_spec_sha_scopes_to_exact_version` FAIL (0/1); restoring the
  original makes it pass again and `git diff` is clean — the per-SHA scoping
  test genuinely kills the path-wide-approval regression.
- **Static gates** (all exit 0 on this tree): arch, ABAC route registry +
  exempt handlers, migration versions, migration SQL portability, mem port
  contracts, task-commit-attribution. No conflict markers anywhere in the
  diff.
- **Compile surface**: focused probes compiled gyre-domain, gyre-ports,
  gyre-adapters, gyre-server (test builds); `cargo check -p gyre-cli` clean.

## Deferred to verifier (sandbox limits, not code defects)

- `cargo test --all` — the executor wrapper rejects workspace-wide
  invocations; all five crates compile and every focused suite passes.
- Full `npm test` and the e2e `spec_approval_auto_invalidated_on_spec_change`
  (needs a TCP listener; `accept(2)` EOPNOTSUPP per
  /tmp/stage/capabilities.json — the identical lifecycle is proven
  listener-free by `push_modifying_spec_revokes_ledger_approvals`).

Evidence: /tmp/stage/review-evidence/ (suite outputs, gate logs,
mutation probe, gates.rs.orig restore verification).
