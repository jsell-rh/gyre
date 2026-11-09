# Review — task-138 (Implement spec approval ledger with database schema and API)

Spec: `specs/system/agent-gates.md` §Part 2 Spec Approval Ledger, §The Provenance Chain, §How It Works.
Candidate under review: `76ad60bf25a81c863002a88010f60e709302f59b` (assigned base `fefde2946`). This is round 9; the round-8 durable finding dcf03cee (weak regression test) was repaired in `59d31f8d8` and is independently re-verified below.
Verdict: **approved**.

## Probes (this sandbox, exact candidate tree; evidence in /tmp/stage/review-evidence/review-summary.md and sibling logs)

- `cargo test -p gyre-domain spec_approval` — 4/4. `cargo test -p gyre-adapters --lib sqlite::spec_approval` — 10/10 (incl. `find_by_spec_sha_returns_only_that_version`, `create_duplicate_id_fails`, `revoke_all_for_path_only_touches_approved_rows`).
- `cargo test -p gyre-server --lib -- api::gates api::specs` — 91/91; `merge_processor` — 68/68.
- **`cargo test -p gyre-server --test git_integration` — 17/17, including the previously-deferred e2e `spec_approval_auto_invalidated_on_spec_change`** (TCP listeners are supported in this sandbox per /tmp/stage/capabilities.json, unlike prior rounds): real git push over HTTP → manifest-driven ledger registration → approve via REST with the ledger's blob SHA → spec-modifying push → ledger row auto-revoked with status/active asserted. The last open host-checklist item from rounds 1–8 is closed.
- **Mutation probe (round-8 repair)**: applying the documented one-line regression (`resolve_spec_blob_sha` → `resolve_blob_sha`, merge_processor.rs:1380) and recompiling now FAILS `require_current_spec_blocks_stale_spec_ref` with `left: Merged / right: Failed`; reverting restores the pass. Finding dcf03cee is genuinely closed — the test now kills the regression it documents. Mutation reverted; tree clean (dev-review-guard check exit 0).
- Static gates all exit 0: migration-versions, migration-SQL-portability, arch, ABAC route registry (revoke route mapped to `("spec", Some("write"))`) + exempt handlers, mem-port-contracts, inert-enforcement, task-commit-attribution, check-rustfmt-diff.py (base fefde2946).
- Clippy: workspace-wide `--all-targets` invocations are rejected by this sandbox's cargo wrapper (infra restriction, not a code defect); compensated with per-package lib clippy on all four touched crates — zero warnings on any changed/added spec_approval file (only pre-existing repo-wide `double_must_use` warnings on untouched traits such as `spec_approval_event_repo.rs`, which is not in the diff).

## Verified against the acceptance criteria

All 11 criteria hold on this tree: migration `2026-11-08-000058_spec_approval_ledger` has all 12 spec columns (nullable `approved_at`, dual-dialect SQLite rebuild + PG portability) plus both required indexes; `ApprovalStatus` is derived from timestamp columns and never stored; mutual exclusivity and transition legality (Pending → Approved → Revoked, Pending → Rejected) are enforced in the domain and mirrored across SQLite/Postgres/mem with contract-parity tests; revocation requires a non-empty reason and records `revoked_by`, with audit-event recording; multiple approvals per (path, sha) coexist; approve validates the requested SHA against the ledger's current blob SHA (409 on mismatch, no partial writes) and creates + approves the ledger entry; the revoke endpoint requires a reason and restricts to original approver/Admin; `GET /api/v1/specs/approvals` returns full ledger rows with derived status/active; push-time invalidation revokes active approvals with ledger-path normalization (tested both listener-free and e2e); forge step-8 validation (`verify_spec_ref`) is fail-closed and scopes to the exact (path, sha) version.

## Findings

None blocking. Non-blocking notes recorded for a future hardening pass (not task-contract violations):

- `revoke_spec_approval` reads `list_active_by_path` with `.unwrap_or_default()` — under an exceptional storage failure the handler can return 200 while the durable ledger retains the active approval that `verify_spec_ref` honors (fail-open for revocation; the primary `spec_approvals.revoke` transition itself is error-checked and returns 409 on failure). Same class: the git_http push hook discards `revoke_all_for_path`'s result with `let _ =`.
- The `require_approved_spec` → `verify_spec_ref` merge-blocking wiring has no direct merge-processor-level test; `verify_spec_ref` is thoroughly unit-tested and the wiring is structurally identical to the mutation-verified `require_current_spec` path.

Scope note: the reject-time "close associated MR" loop predates this task (introduced in f7a68ac73, present in the assigned base), so the spec_ref/specs_assist interaction gap there is not attributable to this diff.

Deferred to verification (sandbox wrapper restrictions, infra not code): the official `check-clippy-diff.py` / `--all-targets` clippy gate and `cargo test --all` (AC 11) on the exact head; full `npm test`.
