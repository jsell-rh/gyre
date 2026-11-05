# Review — task-138 (agent-gates.md §Part 2 Spec Approval Ledger)

Spec: `specs/system/agent-gates.md` §Spec Approval Ledger, §The Provenance Chain, §How It Works.
Candidate under review: `dc158992025ee0ab2938434f5f092cf6b948e04d` (base `3c39ab1f796bcf52ac0b95d60a4101e27aa69fe8`).
Verdict: **reject** (task implementation itself is sound; the candidate tree carries merge-resolution corruption that must be repaired before landing).

## Round 4 findings (blocking)

1. **Committed merge-conflict markers in the shipped web bundle** (`web/dist/index.html`,
   `web/dist/assets/elk.bundled-C7RdDx88.js`, `web/dist/assets/elk.bundled-OwdVv5y8.js`).
   Merge commit `ce7e2a92` ("implement(task-138): pipeline checkpoint", both parents on the
   branch) resolved the `web/dist` conflicts by committing the raw markers:
   `index.html` lines 11–17 contain `<<<<<<< HEAD` / `=======` / `>>>>>>> 59ff5d01…`, and both
   1.5 MB elk chunks begin with `<<<<<<<< HEAD:…` and carry `>>>>>>>> …` at line 5 (both files
   are the identical corrupted blob `9c75aaa2`). `SKIP_WEB_BUILD=1 cargo build -p gyre-server`
   (the documented Rust-only path in AGENTS.md) serves this `web/dist/`: the marker text in
   `index.html` opens an HTML bogus comment that swallows the module script and stylesheet
   tags, so the SPA loads neither — blank dashboard — and the elk chunks are syntactically
   invalid JS. The merge also destroyed main's valid `elk.bundled-C7RdDx88.js` (blob
   `3ab1854c` → corrupt `9c75aaa2`) and left two duplicate full bundles plus two index
   bundles in the tree. Reproduce: `grep -rn '^<<<<<<<' web/dist/` at `dc158992`.
   Evidence: `/tmp/stage/review-evidence/web-dist-conflict-markers.txt`.

2. **Unexplained out-of-scope production-code edit smuggled into the merge resolution**
   (`crates/gyre-domain/src/rust_extractor.rs:1071`). Neither parent of `ce7e2a92` touched
   this file (both have identical blob `619ee22d`); the merge result adds
   `#[allow(clippy::never_loop)] // TEMP diagnostic scaffold, reverted before commit`.
   The comment itself says it was meant to be reverted before commit; it was not. A lint
   suppression in an unrelated file, outside the task's scope, with no justification in the
   task record. Must be reverted (and if `never_loop` genuinely fires there, the underlying
   cause fixed, not suppressed).

3. Minor: the same merge reordered `specs/tasks/task-068.md` `commits:` frontmatter
   (`a11ba8d3` moved to first; origin/main keeps the original order). Benign for the
   membership-based attribution gate, but it is an unexplained edit to an unrelated task
   file that should not survive the merge repair.

## Task-138 implementation assessment (verified, no findings)

- Domain (`gyre-domain/src/spec_approval.rs`): `ApprovalStatus` derived from which timestamp
  column is non-null (never stored); `approve`/`revoke`/`reject` enforce
  Pending→Approved→Revoked and Pending→Rejected, reject empty revocation reasons, and clear
  sibling timestamp columns for mutual exclusivity. Ran `cargo test -p gyre-domain
  spec_approval` → 4/4 pass (evidence: `/tmp/stage/review-evidence/task-138-review-probes.md`).
- Port + adapters: `SpecApprovalRepository` with `find_by_spec_sha` (exact (path, sha)
  version query); SQLite/PG/mem implement identical contracts, transitions applied through
  the domain rules with full-row persistence so mutual exclusivity holds in storage; mem
  guards duplicate ids in code (`check-mem-port-contracts.sh` exit 0).
- Migration `2026-10-08-000056_spec_approval_ledger`: nullable `approved_at`, dual-dialect
  SQLite rebuild + PG `DROP NOT NULL`, indexes on (spec_path, spec_sha) and approver_id.
  `check-migration-versions.sh` and `check-migration-sql-portability.sh` exit 0.
- API: `approve_spec` validates the caller-supplied SHA against the ledger's current git
  blob SHA (409 on mismatch, no partial writes); `revoke_spec_approval` requires a reason,
  restricts to original approver/Admin, and writes an audit event; `reject_spec` transitions
  Pending ledger rows to Rejected and closes spec-edit MRs; `GET /api/v1/specs/approvals`
  returns full ledger rows with derived `status`/`active`. New/extended routes are in the
  ABAC resolver (`/api/v1/specs/:path/revoke` → spec:write; `check-abac-route-registry.sh`
  exit 0).
- Forge enforcement (Provenance Chain step 8): `verify_spec_ref` queries the exact
  (path, sha) version via `find_by_spec_sha` and accepts only active (Approved, not
  revoked/rejected) rows; `require_current_spec` blocks merges on stale spec SHAs
  (`resolve_spec_blob_sha` bridges ledger paths to `specs/`-prefixed git paths).
- Push-time invalidation: a push modifying a spec file revokes active approvals for that
  path, with git-path→ledger-path normalization (regression test present).
- E2E `spec_approval_auto_invalidated_on_spec_change` was un-ignored and made real
  (manifest registration, real blob SHA), but needs a TCP listener — `accept(2)` is
  EOPNOTSUPP in this sandbox (`/tmp/stage/capabilities.json`); not a code defect.

## Sandbox limitations (recorded for the verifier)

crates.io is unreachable (connection refused) and `ahash 0.8.12`/`darling_core 0.21.3`
are not in the local cargo cache, so `gyre-adapters`/`gyre-server` suites could not compile
here; `Cargo.lock` is byte-identical to base, so this is environmental. Host checklist:
`cargo test -p gyre-adapters --lib sqlite::spec_approval`; `cargo test -p gyre-server
--lib -- api::specs` and `-- api::gates`; `cargo test -p gyre-server --test git_integration
spec_approval_auto_invalidated_on_spec_change`; `cargo test --all`; full `npm test`;
GitHub CI on the exact head. Also confirm `cargo clippy` after reverting the
`rust_extractor.rs` `never_loop` allow.
