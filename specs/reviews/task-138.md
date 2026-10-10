# Review: TASK-138 — Implement spec approval ledger with database schema and API

**Reviewer:** Independent reviewer (fresh pass; prior assignment's review model did not complete)
**Date:** 2026-10-10
**Verdict:** approved

**Candidate under review:** `f26b761c479c56bcbe7410ccbcc3b964eb6c7d1a`
**Assigned base:** `27bd585ca7eb429905ccbded1f48b4d0167c0c20` (verified: base is an ancestor of candidate; `git diff base..candidate -- web/src web/tests crates/gyre-cli` empty — no out-of-scope code surfaces touched)
**Spec:** `agent-gates.md` §Spec Approval Ledger, §The Provenance Chain, §How It Works

**Verification performed:** full diff review of all 15 changed crate files; contract-hash comparison of the shipped task file against the assigned original at `0564e747` via `scripts/dev-contract.py requirement_parts` (front and prose both equal — the c059a12e contract finding is repaired); static gates (arch, migration versions, SQL portability, mem-port-contracts, ABAC registry, ABAC exempt handlers, task-commit attribution — all exit 0); focused suites on the exact candidate tree: `gyre-domain spec_approval` 4/4, `gyre-adapters sqlite::spec_approval` 7/7, `api::gates` 10/10, `api::specs` 73/73, `push_modifying_spec_revokes_ledger_approvals` 1/1, `require_current_spec_blocks_stale_spec_ref` 1/1 — 85/0, matching the candidate's recorded counts. Evidence: `/tmp/stage/review-evidence/task-138-review.md`.

---

## Findings

None. All ten acceptance criteria are backed by production code on this tree:

1. **`spec_approvals` table per spec schema** — migration `2026-10-08-000056_spec_approval_ledger` carries all 12 columns; SQLite rebuild relaxes `approved_at` to NULL-able (Pending), PG dialect-portable (portability gate exit 0); Diesel `schema.rs` matches.
2. **`ApprovalStatus` enum** — `gyre-domain/src/spec_approval.rs`, four variants, snake_case serde.
3. **Status derived, not stored** — `SpecApproval::status()` reads which timestamp column is non-null; no status column exists anywhere in the schema or struct.
4. **Mutual exclusivity** — `revoke`/`reject` clear sibling timestamp (and attribution) columns; verified persisted by `revoke_enforces_transition_and_mutual_exclusivity` (asserts `approved_at == None` after reload from SQLite).
5. **Valid transitions** — enforced in the domain and applied through every adapter's `transition` (load → domain-validate → full-row persist); Pending→Revoked, double-approve, Approved→Rejected, post-terminal transitions all rejected (domain + adapter tests).
6. **Revocation requires reason + revoked_by** — domain rejects empty/whitespace reasons; handler additionally 400s; `revoked_by` recorded; revocation audited into `audit_events` with path/sha/actor/reason.
7. **Multiple approvals per path** — PK is `id` only; (path, sha, approver) rows coexist; tested in all three surfaces.
8. **Ledger entry with git blob SHA** — `approve_spec` validates the requested SHA against the ledger's synced `current_sha` (409 on mismatch, no partial writes — asserted by `approve_spec_mismatched_sha_rejected`), then creates + transitions the ledger row. Push-time registry sync keeps `current_sha` bound to the real git blob.
9. **Revoke endpoint with reason** — `POST /api/v1/specs/:path/revoke`, ABAC `spec`/`write`, original-approver-or-Admin authorization, domain-enforced transition, audit trail.
10. **`GET /api/v1/specs/approvals` full ledger data** — returns every field including derived `status`/`active` (`approvals_list_returns_full_ledger_data`).

Beyond the letter of the criteria, the candidate closes the real holes the base had: `verify_spec_ref` (forge step 8) now queries the exact (path, sha) version via `find_by_spec_sha` instead of path-wide `list_active_by_path`; `require_current_spec` resolves ledger paths against `specs/`-prefixed repo files (was silently never resolving — policy enforced nothing); push-time invalidation normalizes git diff paths to ledger paths (was silently matching no rows). Each is regression-tested with a test that fails if the behavior regresses.

**Sandbox limitation (infrastructure, not a code defect):** the e2e test `spec_approval_auto_invalidated_on_spec_change` requires a TCP listener; this sandbox blocks `accept(2)` with errno 95 (probe recorded in the evidence file, consistent with `/tmp/stage/capabilities.json`). The identical lifecycle is proven listener-free by the unit tests listed above. Host checklist: e2e test, `cargo test --all`, `npm test`, GitHub CI on the exact candidate head.
