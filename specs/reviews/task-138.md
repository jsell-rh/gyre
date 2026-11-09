# Review — task-138 (agent-gates.md §Part 2 Spec Approval Ledger)

Spec: `specs/system/agent-gates.md` Part 2 §Spec Approval Ledger, §The Provenance Chain, §How It Works.
Candidate: `fee5b3b62b7a6fbcdb9518f608358c5c95511d7d` (base `fefde2946f3a0af1f212588f54591516fee0f7b2`).
Verdict: **approved**.

## Evidence (all on the exact candidate tree; probes under /tmp/stage/review-evidence/)

Focused suites (via /tmp/stage/bin/cargo):

- `cargo test -p gyre-domain --lib spec_approval` → **4/4** (status derived from
  timestamps, Pending→Approved→Revoked / Pending→Rejected enforced, invalid
  transitions rejected, revocation requires non-empty reason, mutual
  exclusivity asserted on state).
- `cargo test -p gyre-adapters --lib sqlite::spec_approval` → **10/10**, incl.
  `create_duplicate_id_fails`, `revoke_enforces_transition_and_mutual_exclusivity`
  (real SQLite round-trip: approved_at cleared after revoke, Revoked→Revoked
  rejected), `find_by_spec_sha_returns_only_that_version`,
  `revoke_all_for_path_only_touches_approved_rows`.
- `cargo test -p gyre-server --lib -- api::gates api::specs` → **91/91** (ledger
  list with derived status/active, revoked/rejected entries never verify,
  per-SHA scoping, approve 409 on SHA mismatch with no partial writes).
- `cargo test -p gyre-server --lib -- push_modifying_spec_revokes_ledger_approvals
  require_current_spec_blocks_stale_spec_ref` → **2/2**.
- **e2e `spec_approval_auto_invalidated_on_spec_change`** (git_integration,
  real server + real git pushes) → **1/1 passed**. This sandbox's
  capabilities.json now reports the TCP listener probe supported, so the one
  item the implementer could not run was run here: push spec → ledger
  registers blob SHA → approve via API → push modification → approval
  auto-revoked (`revoked_by: system:spec-lifecycle`, status `revoked`).

Mutation probes (each reverted immediately; tree verified pristine by
`git status` + sha256 before/after — hashes in pre-mutation-hashes.txt):

1. `verify_spec_ref` approval check disabled (`has_active_approval = true`) →
   `revoked_approval_no_longer_verifies_spec_ref` and
   `find_by_spec_sha_scopes_to_exact_version` both FAIL. The forge step-8
   check is real.
2. Push-hook `ledger_path` normalization disabled →
   `push_modifying_spec_revokes_ledger_approvals` FAILS. The git-path →
   ledger-path matching is load-bearing, not incidental.
3. Approve-API SHA-vs-ledger-current validation disabled →
   `approve_spec_mismatched_sha_rejected` FAILS. Fabricated/stale SHAs cannot
   mint approvals.

Static gates (exit 0): check-arch, check-migration-versions,
check-migration-sql-portability, check-mem-port-contracts,
check-task-commit-attribution, check-inert-enforcement,
check-abac-route-registry, check-abac-exempt-handlers.

Task contract: `requirement_parts` of `specs/tasks/task-138.md` equals the
assigned original from job.json (Shipped/Review sections and
progress:/commits: lines excluded, mirroring dev-contract.py); acceptance
boxes unchecked (0 `[x]`); all six frontmatter commits present and covered by
the attribution gate.

## Verified working

- **Schema**: migration `2026-11-08-000058_spec_approval_ledger` creates the
  full spec table (nullable `approved_at`, all rejection/revocation columns),
  indexes on (spec_path, spec_sha) and approver_id, dual-dialect SQLite
  rebuild with PG `ALTER COLUMN DROP NOT NULL`. Migration gates pass; SQLite
  adapter tests run the migration for real.
- **Status derived, never stored**: `SpecApproval::status()` computes from
  which timestamp column is non-null; `is_active()` requires Approved and not
  revoked/rejected. No status column anywhere.
- **Transitions**: domain `approve`/`revoke`/`reject` enforce the spec's state
  machine and clear sibling timestamp columns; SQLite/Postgres/mem all
  delegate to the domain transition (load → validate → persist full row), so
  adapter behavior cannot diverge. Mem enforces the duplicate-id create guard
  in code (gate passes).
- **API**: approve validates the client SHA against the ledger's current blob
  SHA (409, no partial writes — mutation-verified); revoke requires a reason
  (400 on empty), restricts to original approver or Admin, and writes an
  audit_events row; reject transitions Pending rows to Rejected and closes
  spec-edit MRs; GET /api/v1/specs/approvals returns full ledger rows with
  derived status/active.
- **Forge (§Provenance Chain step 8)**: `verify_spec_ref` queries the exact
  (path, sha) via `find_by_spec_sha` and accepts only active rows;
  `require_current_spec` blocks merges on stale spec SHAs via
  `resolve_spec_blob_sha` (specs/-prefix fallback for ledger paths).
- **Push-time invalidation**: push modifying a spec file revokes all active
  approvals for that path, with git-path/ledger-path normalization — proven
  end-to-end by the real-server e2e test in this sandbox.

## Minor (non-blocking, recorded for completeness)

- `git_http.rs` push hook discards the `revoke_all_for_path` result with
  `let _ =` (no log on storage failure) — pre-existing pattern on the base,
  not introduced by this candidate; a silent storage failure would leave
  stale approvals active. Worth a follow-up log-and-alert, not a contract gap.
- `revoke_spec_approval` picks the "most recent" active ledger row by id-desc
  ordering; with multiple simultaneous approvers only that row is revoked
  while the history event for the latest active approval is revoked too.
  Matches the single-approval-per-approval-action semantics of the history
  surface; the spec does not require revoking all rows.

## Host checklist (verification)

`cargo test --all`, full `npm test`, GitHub CI on the exact head. The e2e
item previously on this list was executed here and passes.
