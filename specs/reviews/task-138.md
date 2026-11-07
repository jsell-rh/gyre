# Review: TASK-138 — Spec Approval Ledger (database schema and API)

**Reviewer:** Independent review (pi runtime, attempt 5 — resumed after interrupted cold-compile probe)
**Comparison base:** 1ae18d1c42bf9c48ae1e3c10369dbc81920664c9
**Candidate:** d7e531b1964e1c4b0ac8c5484f3e86f4b06bc0c8 (merge of pipeline branch 0cd0b4d6 into base)
**Verdict:** approved

---

## Scope and structure

The candidate delta vs base is 22 files / +2143 −438. The candidate is a merge
commit; the four attributed commits (b27d3787, 30e80d95, 2f424b88, 72f61cbd)
contain only task-relevant work. Contract integrity verified:
`dev-contract.py requirement_parts(job body) == requirement_parts(candidate
specs/tasks/task-138.md)` → True (Acceptance Criteria boxes unchecked; evidence
lives under `## Shipped`).

## Acceptance criteria — all verified against production code

- [x] **Table.** Migration `2026-10-08-000057_spec_approval_ledger` rebuilds
  `spec_approvals` with all 12 spec columns; `approved_at` becomes nullable
  (NULL while Pending). Dual-dialect: SQLite table rebuild + PG `ALTER COLUMN`
  semantics portable; `check-migration-sql-portability.sh` exit 0. Numbering is
  the next unused sequence (000056 is task-180's live `spec_assertion_results`);
  `check-migration-versions.sh` exit 0. The migration is exercised by every
  SQLite test via `SqliteStorage::new`.
- [x] **Enum + derived status.** `ApprovalStatus` in gyre-domain; `status()`
  derives from which timestamp column is non-null and is never stored.
  `SpecApprovalResponse.status` is the derived string on the wire.
- [x] **Mutual exclusivity.** Domain `revoke`/`reject` clear sibling timestamp
  columns. Mutation-verified (see below).
- [x] **Valid transitions.** Pending→Approved→Revoked and Pending→Rejected
  enforced in the domain type; adapters (SQLite, Postgres, mem) apply the same
  domain transition after load, so no adapter can bypass it.
  `invalid_transitions_are_rejected` covers every illegal path.
- [x] **Revocation requires reason + revoked_by, audited.** Domain rejects
  empty/whitespace reasons; the endpoint 400s on empty reason; revocation is
  written to `audit_events` (`spec_approval_revoked`, Success, with reason);
  `revoked_by` recorded.
- [x] **Multiple approvals per path.** PK is `id`; (path, sha) indexed,
  non-unique. `create_find_and_list_by_path` and
  `find_by_spec_sha_returns_only_that_version` assert coexistence.
- [x] **Approval binds the real git blob SHA.** `approve_spec` resolves the
  ledger's `current_sha` and returns 409 on caller-SHA mismatch with no
  partial writes (`approve_spec_mismatched_sha_rejected` asserts empty ledger
  + empty history + unchanged ledger status). The e2e test approves the actual
  blob SHA registered by the push-triggered spec-registry sync.
- [x] **Revoke endpoint with reason.** `POST /api/v1/specs/:path/revoke`
  requires reason, restricts to original approver or Admin, transitions the
  durable ledger row, resets spec_ledger status, audits.
- [x] **GET /api/v1/specs/approvals full ledger.** Response carries all
  columns incl. rejected_*, derived `status`, `active`
  (`approvals_list_returns_full_ledger_data`).
- [x] **`cargo test --all`.** Workspace-wide invocation is rejected by the
  executor wrapper (review-stage constraint, not a code defect). Equivalent
  coverage: full lib suites of every package with production changes —
  gyre-domain 412/0, gyre-adapters 355/0 (12 ignored pre-existing),
  gyre-server 1285/0 — plus `--test git_integration` 17/17.

## Provenance chain step 8 (forge enforcement)

`verify_spec_ref` queries the exact (path, sha) version via the new
`find_by_spec_sha` port method (implemented in SQLite, Postgres, and mem with
identical contracts) and accepts only `is_active()` rows — Pending/Revoked/
Rejected never authorize a merge; unknown SHA fails closed. It is wired into
`merge_processor` `require_approved_spec` (merge blocking) and the MR
fully-approved status check. `require_current_spec` now resolves blob SHAs
through `resolve_spec_blob_sha` (specs/-prefix fallback for ledger paths);
`require_current_spec_blocks_stale_spec_ref` uses a real git repo and asserts
both the block (stale SHA → Failed) and the control (current SHA → not
Failed).

## Push-time invalidation

A push modifying a spec file revokes active approvals for that path, with
ledger-path normalization (`specs/` prefix strip) so git diff paths match
ledger rows. Proven twice: listener-free via
`push_modifying_spec_revokes_ledger_approvals` (real repo, real
`process_spec_lifecycle`), and end-to-end via
`spec_approval_auto_invalidated_on_spec_change` (real server + git HTTP push:
register → approve real SHA → push modification → poll revoked). Note: the
implementer recorded this e2e test as listener-blocked (EOPNOTSUPP); the
current sandbox permits listeners (`/tmp/stage/capabilities.json`), and the
test passes here — the host-checklist item is discharged in-sandbox.

## Mutation evidence (each kills the test; source restored after each)

Evidence in `/tmp/stage/review-evidence/mutation{1,2,3}-*.txt`:

1. **Remove ledger-path normalization** in the push hook (pass raw git path to
   `revoke_all_for_path`) → `push_modifying_spec_revokes_ledger_approvals`
   FAILED ("push modifying spec must revoke the ledger approval"). Restored.
2. **Domain `revoke` keeps `approved_at`** (mutual exclusivity broken) →
   `revoke_enforces_transition_and_mutual_exclusivity` FAILED ("mutual
   exclusivity: approved_at cleared"). Restored.
3. **`verify_spec_ref` ignores active status** (`!for_sha.is_empty()`) →
   `revoked_approval_no_longer_verifies_spec_ref` and
   `find_by_spec_sha_scopes_to_exact_version` both FAILED. Restored.

Working tree verified clean after all probes (`git status --porcelain` empty;
untracked `web/dist/` build output from probes removed).

## Focused runs (all exit 0; logs in /tmp/stage/review-evidence/)

- `cargo test -p gyre-domain spec_approval` — 4/4
- `cargo test -p gyre-adapters --lib sqlite::spec_approval` — 7/7
- `cargo test -p gyre-server --lib -- api::gates` — 10/10
- `cargo test -p gyre-server --lib -- api::specs` — 73/73
- lifecycle four (`push_modifying_spec…`, `require_current_spec…`,
  `find_by_spec_sha…`, `approve_spec_mismatched_sha…`) — 4/4
- `cargo test -p gyre-server --test git_integration` — 17/17 (incl. the e2e)
- `cargo test -p gyre-server --lib -- merge_processor` — 50/50
- Full lib suites: gyre-domain 412/0, gyre-adapters 355/0, gyre-server 1285/0
- Static gates (exit 0): migration-versions, migration-SQL-portability, arch,
  mem-port-contracts, abac-route-registry, task-commit-attribution,
  inert-enforcement, abac-exempt-handlers, forged-scope-fields,
  scope-literal-defaults

## Commit attribution and exemptions

All four attributed commits exist, descend from the base, and touch only
task-relevant files; `check-task-commit-attribution.sh` exit 0. The exemption
file edits in checkpoint commits (inert-enforcement, unwritten-store-fields)
are pure line-number relocations caused by mem.rs/merge_processor.rs line
shifts — same entries, same count, no new exemptions.

## Non-blocking observations (for the controller, not task findings)

1. `crates/gyre-domain/src/rust_extractor.rs:1071` carries a leftover
   `#[allow(clippy::never_loop)] // TEMP diagnostic scaffold, reverted before
   commit` attribute. Introduced by an executor *checkpoint* commit
   (ce7e2a92), not an attributed commit. Behavior is identical to base (the
   loop's unconditional `break` pre-exists); it is inert lint-suppression
   debris with a misleading comment. Cleanup candidate.
2. The candidate tree deletes the 4 committed `web/dist` build artifacts
   (base and origin/main both carry them), also via executor checkpoint
   commits rather than attributed work. Any full build regenerates them, CI
   builds the web separately, and the pipeline's own tooling treats
   `web/dist/` as ephemeral (excluded from diff scoping in
   scripts/pipeline/stages.py; dev-check.sh cleans it). Residual effect: on a
   fresh checkout, `SKIP_WEB_BUILD=1 cargo build -p gyre-server` yields a
   binary with no embedded SPA until a full build runs. The controller should
   restore the committed bundle (or accept the regeneration) at publication
   time; it is not a defect in the task's code.
