# Review — task-138 (Spec Approval Ledger with Database Schema and API)

Spec: `agent-gates.md` §Part 2 Spec Approval Ledger, §The Provenance Chain, §How It Works.
Candidate under review: `7f420860f41c83a288b29031f553d059b0adc782` (base `8c2d1775`).
Evidence: `/tmp/stage/review-evidence/task-138-review.md` (+ per-suite logs).

## Round 1

Verified working (independently executed on the candidate tree, this sandbox):

- **Domain lifecycle** (`gyre-domain/src/spec_approval.rs`): `ApprovalStatus`
  {Pending, Approved, Revoked, Rejected} is *derived* from which timestamp
  column is non-null (`status()`), never stored. Transition methods enforce
  Pending → Approved → Revoked and Pending → Rejected, reject empty
  revocation reasons, and clear sibling timestamp columns for mutual
  exclusivity (approve() only sets approved_at from Pending; revoke() clears
  approved_at/rejected_*; reject() clears approved_at/revoked_*).
  `cargo test -p gyre-domain spec_approval` → 4/4 pass.
- **Port** (`gyre-ports/src/spec_approval.rs`): `SpecApprovalRepository` with
  create (duplicate-id fails), find_by_id, list_by_path, find_by_spec_sha
  (exact (path, sha) version query), list_active_by_path, list_all,
  approve/revoke/reject transitions, revoke_all_for_path — matches the task's
  Implementation Plan step 2.
- **SQLite adapter** implements the port against the real migration
  (2026-10-08-000056: nullable approved_at via dual-dialect rebuild,
  indexes on (spec_path, spec_sha) and approver_id):
  `cargo test -p gyre-adapters --lib sqlite::spec_approval` → 7/7 pass,
  including `find_by_spec_sha_returns_only_that_version`,
  `revoke_enforces_transition_and_mutual_exclusivity`,
  `revoke_all_for_path_only_touches_approved_rows`, `create_duplicate_id_fails`.
  Postgres and mem adapters implement the same contract (mem guards
  duplicate-id in code; `check-mem-port-contracts.sh` passes).
- **API**: `POST /api/v1/specs/:path/approve` validates the requested SHA
  against the ledger's current blob SHA (409 on mismatch, no partial writes —
  `approve_spec_mismatched_sha_rejected`), creates a Pending ledger entry and
  transitions it to Approved. `POST /api/v1/specs/:path/revoke` requires a
  non-empty reason, restricts to original approver or Admin, revokes the
  latest active ledger row, and audits to `audit_events`. `POST
  /api/v1/specs/:path/reject` transitions Pending ledger rows to Rejected and
  closes spec-edit MRs referencing the spec. `GET /api/v1/specs/approvals`
  returns full ledger rows with derived `status`/`active` (all 12 schema
  columns + derived fields).
  `cargo test -p gyre-server --lib -- api::specs` → 73/73; `api::gates` → 10/10
  (incl. `approvals_list_returns_full_ledger_data`,
  `revoked_approval_no_longer_verifies_spec_ref`,
  `find_by_spec_sha_scopes_to_exact_version`,
  `reject_transition_pending_to_rejected_and_audit`).
- **Forge enforcement (Provenance Chain step 8)**: `verify_spec_ref` queries
  the exact (path, sha) version via `find_by_spec_sha` and accepts only
  active rows; wired into the merge processor's `require_approved_spec`
  policy path and the merge attestation's `spec_fully_approved`.
  `require_current_spec` blocks stale spec SHAs at merge, with the
  ledger-path ("system/foo.md") → git-path ("specs/system/foo.md") fallback
  so the blob SHA actually resolves.
- **Push-time invalidation**: a push modifying a spec file revokes active
  approvals for that path, with git-path→ledger-path normalization.
  `push_modifying_spec_revokes_ledger_approvals` (real git repo, drives
  `process_spec_lifecycle`, asserts Approved→Revoked + mutual exclusivity +
  verify_spec_ref failure) and `require_current_spec_blocks_stale_spec_ref`
  (real git repo, stale SHA → entry Failed, current SHA control passes) both
  pass.
- **Acceptance criterion "multiple approvals per spec path (different
  SHAs)"**: create() has no unique constraint on (path, sha); a fresh row per
  approval; adapter and API tests cover two SHAs of one path.
- **Mechanical invariants** all pass on the candidate: migration-versions,
  arch, mem-port-contracts, inert-enforcement, migration-sql-portability,
  ABAC route registry, ABAC exempt handlers, task-commit attribution,
  forged-scope-fields, fabricated-scope-defaults.
- **Contract repair verified**: the assigned task body (job.json) vs the
  candidate task file's requirement sections are identical; only frontmatter
  progress/commits fields and the appended `## Shipped` evidence section
  differ. Both frontmatter commits (2f424b88, 72f61cbd) exist on the branch
  and are the only product-surface commits; `check-task-commit-attribution.sh`
  passes.

Notes (not findings):

- **require_signed_approval policy** (agent-gates.md §Forge Enforcement
  Policies table) is not implemented — no such field in `SpecPolicy`. This is
  *not* a task-138 acceptance criterion or plan step (the task's plan and
  criteria cover the ledger, transitions, endpoints, per-SHA query; the
  policy set predates this task unchanged at base: `SpecPolicy` is identical
  to base). Out of scope for this review; the ledger's `signature` column
  stores the Sigstore signature when provided, which is the task's slice.
- **e2e test transport restriction (environmental, not a code defect)**:
  `spec_approval_auto_invalidated_on_spec_change` needs a TCP listener;
  this sandbox blocks `accept(2)` with EOPNOTSUPP (errno 95, per
  /tmp/stage/capabilities.json). The `#[ignore]` was *removed* and the test
  rewritten against the real ledger flow (manifest-registered spec → push →
  ledger SHA → approve → modify → push → revoked row with
  revoked_by=system:spec-lifecycle). The identical lifecycle is proven
  listener-free by the git_http unit test above. Host verification
  checklist: run the e2e test, `cargo test --all`, and GitHub CI on the
  exact candidate head.
- The web/dist bundle churn in fc2da8ab is a rebuild artifact of the
  pipeline checkpoint (build.rs web build), not a hand-authored UI change;
  no web/src changes are in the candidate.

Verdict: **approved** — every acceptance criterion has independently
re-executed production-code evidence; no findings.
