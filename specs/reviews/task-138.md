# Review — task-138 (Spec Approval Ledger with database schema and API)

Spec: `specs/system/agent-gates.md` §Part 2 Spec Approval Ledger, §The Provenance Chain, §How It Works.
Base: `fefde2946` — candidate: `20c5881f` (7 task commits in `commits:` frontmatter, all present on the branch; task-commit-attribution gate exit 0).
Verdict: **needs-revision** (one finding — a weak regression test; the underlying production behavior is real and independently verified).

## Round 8 (this review)

Sandbox note: the TCP listener is AVAILABLE this run (`capabilities.json`:
`tcp_listener_probe.supported: true`; verified with a python bind+listen
probe), so the e2e test deferred by every prior round was executed for real.

### Verified working (independent evidence, `/tmp/stage/review-evidence/task-138-r8-review.md`)

- **Domain** (`gyre-domain/src/spec_approval.rs`): `ApprovalStatus` derived
  from which timestamp column is non-null (never stored); transitions enforce
  Pending → Approved → Revoked and Pending → Rejected; empty revocation
  reasons rejected; sibling timestamp columns cleared on transition (mutual
  exclusivity). Suite 4/4.
- **Port + adapters**: `SpecApprovalRepository` implemented by SQLite,
  Postgres, and mem with identical contracts — duplicate-id `create` fails in
  all three (PG plain INSERT → PK violation; mem guards in code);
  `find_by_spec_sha` scopes to the exact (path, sha) version. SQLite suite
  10/10; mem-port-contracts gate exit 0.
- **Migration** `2026-11-08-000058_spec_approval_ledger`: nullable
  `approved_at` (NULL while Pending), SQLite rebuild + PG-portable form,
  indexes on (spec_path, spec_sha) and approver_id; migration-versions and
  SQL-portability gates exit 0.
- **API**: `POST /specs/:path/approve` validates the caller-supplied SHA
  against the ledger's server-computed git blob SHA (409 on mismatch, no
  partial writes; approver identity from the verified token, not the body);
  `POST /specs/:path/revoke` requires a reason, restricts to original
  approver/Admin, writes an audit_events row; `POST /specs/:path/reject`
  transitions Pending rows and closes spec-edit MRs; `GET /specs/approvals`
  returns full ledger rows with derived status/active. `api::specs` 80/80,
  `api::gates` 11/11.
- **Forge enforcement (provenance chain step 8)**: `verify_spec_ref` queries
  the exact (path, sha) and accepts only active rows, failing CLOSED on
  storage error; wired into the merge processor under
  `require_approved_spec`; `require_current_spec` blocks stale SHAs.
- **Push-time invalidation**: git-path→ledger-path normalization; proven
  three ways — unit test `push_modifying_spec_revokes_ledger_approvals`,
  merge-processor regression test, and the **e2e test
  `spec_approval_auto_invalidated_on_spec_change` (1/1 passed this run over
  real HTTP + real git pushes: approve → push modification → revoked_by
  "system:spec-lifecycle", status revoked)**.
- Static gates on the exact tree: arch, migration-versions,
  mem-port-contracts, abac-route-registry, inert-enforcement,
  task-commit-attribution — all exit 0; rustfmt-diff clean. ABAC note:
  `/specs/:path/reject` is a pre-existing frozen exemption (present in base
  fefde2946) with in-handler role enforcement; the candidate added no
  exemption entries.

### Finding

- [ ] **F1 (major, test-quality): `require_current_spec_blocks_stale_spec_ref`
  does not fail when the behavior it guards is disabled.** The test's doc
  comment names the exact regression it claims to guard — "without the
  specs/-prefix fallback the blob SHA never resolved, `is_stale` was always
  false, and the policy silently enforced nothing" — but reverting
  `resolve_spec_blob_sha` → `resolve_blob_sha` at `merge_processor.rs:1380`
  (one-line sed; recompiled) leaves the test green. Under the mutation the
  stale-spec entry terminates **Merged** (dumped via `find_by_id`), yet the
  test's only assertion on it is absence from `list_queue()`, which Merged
  satisfies exactly like Failed; the control `assert_ne!(status, Failed)`
  likewise passes spuriously (entry2 is Merged). This is the task-contract
  definition of a non-probative test ("a test that doesn't fail when the
  specced behavior breaks does not close a section") and it is one of the
  Shipped section's cited verification artifacts. The production behavior
  itself is real — the unmutated code fails the entry at the
  `require_current_spec` check (independently confirmed: status Failed) — so
  the remedy is assertion strength, not behavior: assert the stale entry's
  terminal status is exactly `Failed` via `merge_queue.find_by_id` (and
  optionally that `error_message` contains "requires current spec") instead
  of absence from `list_queue()`. Reproduction transcript:
  `/tmp/stage/review-evidence/task-138-r8-mutation-probe.log`.

### Minor (not blocking, recorded for completeness)

- `git_http.rs` push hook discards the `revoke_all_for_path` Result (`let _ =`)
  and its `invalidated` counter counts stale paths, not actually-revoked
  approvals; a storage write failure would leave stale approvals active (the
  registry-status layer and `verify_spec_ref`'s fail-closed reads are the
  backstops). Consider logging the error and the returned revoke count.
- `revoke_spec_approval` revokes only the newest active ledger row per call;
  with multiple active approvals for a path (human_and_agent mode), revoking
  one leaves the sibling active. Consistent with per-approver withdrawal +
  §9 recompute, but worth documenting.

### Deferred to verification stage

- clippy diff gate (workspace-wide clippy blocked by the sandbox wrapper;
  round-7's scoped repairs inspected by hand), `cargo test --all`, full
  `npm test`, GitHub CI on the exact head.
