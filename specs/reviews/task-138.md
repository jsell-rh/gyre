# Review: TASK-138 — Spec Approval Ledger (database schema and API)

**Reviewer:** Independent review (pi runtime)
**Date:** 2026-10-09
**Comparison base:** 722179f03aef141e9e546285e3abef724086d25b
**HEAD (candidate):** 0cd0b4d6815a78bd5c6e68b0cbd1a38a8b4aa606
**Verdict:** approved
**Evidence:** /tmp/stage/review-evidence/task-138-review.md (+ per-gate *.out files)

---

## Summary

The candidate implements agent-gates.md §Part 2 Spec Approval Ledger and the
§Provenance Chain step-8 forge check as real, port-backed production code.
Every acceptance criterion was verified against the exact assigned tree with
focused probes, including one mutation probe proving the per-SHA scoping test
kills a real bug.

## Behavior verification

- **Schema.** Migration `2026-10-08-000057_spec_approval_ledger` creates all 12
  spec columns with nullable `approved_at` (NULL = Pending), portable
  SQLite-rebuild/PG form, and indexes on (spec_path, spec_sha) and approver_id.
  Numbering is unique (main's 000056 collision avoided); check-migration-versions
  and check-migration-sql-portability pass.
- **Status derivation.** `SpecApproval::status()` derives Pending/Approved/
  Revoked/Rejected from which timestamp column is non-null; status is never
  stored. Verified in domain tests (4/4) and in the API response layer
  (`SpecApprovalResponse.status`/`active` computed via `status()`/`is_active()`).
- **Mutual exclusivity + transitions.** Domain `approve`/`revoke`/`reject`
  enforce Pending→Approved→Revoked and Pending→Rejected, reject empty
  revocation reasons, and clear sibling timestamp columns; the SQLite adapter
  persists the full post-transition row so exclusivity holds in storage
  (adapter suite 7/7, incl. `revoke_enforces_transition_and_mutual_exclusivity`).
- **Ledger entry with git blob SHA.** `approve_spec` validates the requested
  SHA (40-char hex) against the ledger's current blob SHA, 409 on mismatch
  with no partial writes (`approve_spec_bad_sha`), then creates + approves the
  ledger entry.
- **Revoke endpoint.** Requires a non-empty reason, restricts to original
  approver or Admin, records revoked_by/revocation_reason, and writes an audit
  event (`approve_then_revoke`).
- **Rejection closes MRs.** `reject_spec` transitions Pending ledger rows to
  Rejected and closes open spec-edit MRs referencing the spec.
- **Full ledger query.** `GET /api/v1/specs/approvals` returns every ledger
  column plus derived status/active, with optional ?path= filter.
- **Forge enforcement (step 8).** `verify_spec_ref` queries the exact
  (path, sha) version via `find_by_spec_sha` and accepts only active rows;
  wired into merge_processor under `require_approved_spec`, with
  `require_current_spec` blocking stale SHAs.
- **Push-time invalidation.** A push modifying/deleting/renaming a spec file
  revokes all active approvals for that path, with git↔ledger path
  normalization. Verified listener-free by
  `push_modifying_spec_revokes_ledger_approvals` and end-to-end by the e2e test
  (see below).

## Mutation probe

Degrading `verify_spec_ref` to a path-level lookup (`list_by_path` instead of
`find_by_spec_sha`) makes `find_by_spec_sha_scopes_to_exact_version` fail
("Pending version must not verify even though a sibling SHA is approved").
The per-SHA scoping is genuinely enforced, not structural-only. Source was
restored; tree verified clean at the candidate SHA and the test re-passes.

## e2e note

The shipped notes list `spec_approval_auto_invalidated_on_spec_change` as
sandbox-blocked (EOPNOTSUPP on accept(2)). The current sandbox permits TCP
listeners; the test was run and **passes** (1/1). The claimed limitation is
outdated, not a code defect.

## Mechanical invariants (all exit 0)

migration-versions, arch, abac-route-registry, mem-port-contracts,
migration-sql-portability, inert-enforcement, task-commit-attribution.

## Minor observations (no action required)

- task-068.md's `commits:` frontmatter entries were reordered (same set) by a
  merge resolution in branch history; semantically neutral for the
  membership-based attribution gate, which passes.
- When several active ledger rows exist for one path, `revoke_spec_approval`
  revokes only the newest; the spec does not define multi-approver revocation
  semantics and the safety-critical invalidation path (push hook) revokes all.

## Host checklist (verification stage)

- `cargo test --all` (workspace-wide invocations are rejected by this sandbox's
  cargo wrapper; all focused suites pass: domain 4/4, adapters 7/7, server
  lib filtered 85/85, git_integration e2e target passes).
- Full `npm test` and GitHub CI on the exact head.
