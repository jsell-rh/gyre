# Review — task-209 (Finish HSI §12 profile: notification filtering, judgment ledger, auth provider info)

Spec: `specs/system/human-system-interface.md` §12 "What the Profile Is" (row 53).
Candidate under review: `60c4eb9af` (branch `pipeline/task-209/2c3a7d28b9e34fbe9b73fa69b1f023fd-1`), assigned base `e402e1ed`.
Commits scoped per task frontmatter (repair round `21dac7117` + the task's earlier product commits listed in `specs/tasks/task-209.md`).
Verdict: **needs-revision** — one material finding (F1: missing `WorkspaceOwnerless` toggle → the task's own CI gate fails on the candidate tree); everything else verified with independent evidence.

## Probes run (evidence in `/tmp/stage/review-evidence/`)

- `cargo test -p gyre-adapters --lib sqlite::user_profile` → **12/12 passed** (`adapter-tests.log`; includes judgment-ledger merge/filter/pagination/rejection-attribution/compressed-detail tests, `notifications_exclude_types_applies_to_list_and_count`, `record_login_debounce_and_update`).
- `cargo check -p gyre-server` → **exit 0** in 7m04s (`server-check.log`) — the E0061 merge-repair claim holds (lib target; `--all-targets` is wrapper-blocked here, deferred to verifier).
- `cd web && npm ci && npm test` → **110 files, 2210 passed / 41 skipped** (`npm-ci.log`, `npm-test.log`).
- Mechanical scripts: `check-arch`, `check-migration-versions`, `check-abac-route-registry`, `check-abac-exempt-handlers`, `check-mcp-write-tools`, `check-mem-port-contracts`, `check-byte-slice-truncation`, `check-relative-path-defaults`, `check-fail-open-ref-resolution`, `check-fabricated-scope-defaults`, `check-lossy-secret-conversion`, `check-scope-literal-defaults`, `check-in-memory-state-stores`, `check-forwarded-header-trust`, `check-forged-scope-fields`, `check-migration-sql-portability`, `check-dead-message-kinds`, `check-inert-enforcement`, `check-unbounded-external-http` → all OK (`scripts.log`, `scripts2.log`).
- `bash scripts/check-notification-toggle-coverage.sh` → **exit 1, FAIL: WorkspaceOwnerless** (`toggle-coverage-fail.log`).
- Diff inspection of all 55 changed files base→HEAD; targeted reads of the aggregation SQL, the three write paths, `record_login` (sqlite/postgres/mem), the MCP inbox parity path, and the Svelte profile source.

## Verified working (no findings)

- **Spec amendment (Part 1) is real and precedes the readers.** HSI §12 now carries a Sources table naming `spec_approvals` (LEFT JOIN `spec_ledger_entries` on path), `audit_events` rows with `event_type IN (gate_override, trust_change, meta_spec_publish)` attributed via `user_id`, and the canonical-Id attribution rule for approve/reject/revoke; `?type=` values match `JudgmentType::as_str()` exactly (`approval|rejection|gate|trust|meta-spec`).
- **Judgment ledger aggregation is genuine.** `list_for_user` (sqlite; postgres and mem in parity) queries all four categories from real tables; `?workspace_id=` is a SQL `WHERE` filter (the seeded adapter test `judgment_ledger_workspace_filter_drops_unattributed` counts 4-of-6 and would fail under the old stamping behavior); reverse-chronological sort with deterministic tie-break; limit/offset applied after the union (`judgment_ledger_pagination_across_merged_stream`). Rejection attribution to the declining human (`rejected_by`/`revoked_by`) is tested end-to-end in both adapter and handler layers. Compressed `detail_zstd` rows are decompressed through the shared `AuditEventRow` materialization — verified by `judgment_ledger_decompresses_compressed_audit_details`, which runs the real `compress_details_before` pass first.
- **Write paths are real, not stubs.** Trust transition (`workspaces.rs` PUT, propagates audit failure, no event on no-change PUT), gate override (`merge_requests.rs` `submit_review`: one event per FAILED gate result, only for authenticated humans — the agent-token negative case is asserted), meta-spec publish (`meta_specs.rs` create/update via `record_meta_spec_publish`, workspace attribution from `meta_specs.scope`). `AuditEventType` gained `TrustChange`/`GateOverride`/`MetaSpecPublish` with explicit named-variant round-trip tests.
- **Inbox filtering is in-query, not post-pagination**, on both list and badge count, plus MCP `notifications://` parity; `PUT /users/me/notification-preferences` rejects non-canonical type names (400) and normalizes legacy parse aliases before storing (both handler-tested). Background dedup callers pass `&[]` per the port contract.
- **Auth provider info**: migration `2026-10-07-000056_user_auth_provider_info` (portable SQL, unique Diesel version `20261007000056`), `record_login` single-statement debounce with the issuer-change carve-out (sqlite/postgres/mem parity, adapter-tested), `GET /users/me` returns `oidc_issuer`/`last_login_at` read-only (`UpdateProfileRequest` has no field for either), handler test drives a real signed JWT through `validate_jwt`.
- **Domain/enum**: `JudgmentType::GateOverride` with `as_str()="gate"` and `from_db_str("gate")`, exactly per AC.

## Findings

- [-] **F1 (major, code): `WorkspaceOwnerless` has no profile toggle — the task's own CI gate fails on the candidate tree.** `web/src/lib/notification-types.js` lists 40 canonical ids; the `NotificationType` enum has 41 variants. `WorkspaceOwnerless` (Priority-2, sent to every TenantAdmin by the ownerless-workspace job, `workspace_ownership.rs:196`) is absent from the toggle list and from `user_profile.notif_types` in `web/src/locales/en.json`. Reproduce: `bash scripts/check-notification-toggle-coverage.sh` → exit 1. Consequences: (a) `check-notification-toggle-coverage.sh` — added to `.pre-commit-config.yaml` and `.github/workflows/ci.yml` by this task — fails at HEAD, so CI/verification is red; (b) HSI §12 "enable/disable any NotificationType variant" is unmet for this variant (a tenant admin cannot silence ownerless-workspace notifications); (c) coverage row 53's "all three §12-cycle gaps closed" overstates by this one toggle. The variant arrived with base `e402e1ed` (task-121 lineage); the merge-repair round (`21dac7117`) fixed the `list_for_user` arity and rustfmt fallout but not the toggle list. **Repair:** add `'WorkspaceOwnerless'` to `NOTIF_TYPE_IDS`, add the `user_profile.notif_types.WorkspaceOwnerless` label, and fix F2.
- [-] **F2 (minor, test): `UserProfile.test.js:236` hardcodes `expect(NOTIF_TYPE_IDS.length).toBe(40)`** — it mirrors the implementation's stale list rather than the enum, which is why the web suite stayed green through F1. This is the exact "test hardcodes the implementation's count" failure mode the task's own coverage script header warns about (the prior `ComputeTargetUnavailable` F1). After the repair, assert 41 or derive the expectation from the canonical list.

## Non-blocking observations (for the verifier)

- `scripts/check-tenant-filter.sh` exits 2 in this sandbox with a mawk dialect error ("awk: line 11: syntax error") — it fails identically on base `e402e1ed`, so it is an environment/dialect issue, not a candidate defect. The verifier should run it under gawk.
- `scripts/check-unnamed-tuple-carriers.sh` exits 1 with 4 pre-existing errors, identical at base — not introduced by this task (the task's own judgment-ledger carriers were converted to named structs, `fa67c4cea`).
- `web/dist/` in the checkout was rewritten by the harness's background build process during review (mtimes mid-session, outside my actions). Generated output is the verifier's domain; the authored sources and tests were reviewed. The dist observation from task-208's F1 does not recur as a source defect here.
- **Deferred server tests the verifier must execute** (gyre-server test compilation is wrapper-blocked in this sandbox; `cargo check -p gyre-server` lib passed):
  - `api::workspaces::tests::trust_transition_writes_trust_change_audit_event`
  - `api::merge_requests::tests::human_approval_on_failed_gate_writes_gate_override_event`
  - `api::meta_specs::tests::publish_writes_meta_spec_publish_audit_event`
  - `api::specs::tests::{approve,revoke,reject}_via_jwt_surfaces_in_judgment_ledger`
  - `api::users::tests::{disabled_preference_excludes_type_from_inbox_and_count, preference_alias_normalizes_to_canonical_before_storing, get_me_returns_recorded_auth_provider_info}`
  - plus full `cargo test --all` and `cd web && npm test` (web suite already green in this review).
- Working tree carried the controller's staged reset of `specs/tasks/task-209.md` (frontmatter `progress: not-started`, `commits: []`, Shipped section removed) throughout the review; the committed candidate at `60c4eb9af` has `progress: ready-for-review` and the populated commit list. I restored nothing and left it as staged.

Test-inflation check: the new adapter tests kill real bugs (workspace stamping, alias normalization, compressed-detail reads, agent-token vs human attribution); the handler tests drive the real router with real signed JWTs and assert both positive and negative cases. The single weak assertion is F2's hardcoded count.
