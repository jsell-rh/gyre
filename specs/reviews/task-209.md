# Review — task-209 (Finish HSI §12 profile: notification filtering, judgment ledger, auth provider info)

Spec: `specs/system/human-system-interface.md` §12 "What the Profile Is" (coverage row 53).
Candidate under review: `8eda66da` against assigned base `adf4df3d` (157 commits — the branch carries
several main-merge/rebase resolution rounds plus the task-209 product work).
Verdict: **approved** — all three §12 gaps are closed with real production code; no blocking findings.
Server-side test execution is explicitly the independent verifier's gate (recorded below).

## Probes run (checkout at candidate `8eda66da`)

- `cargo test -p gyre-adapters --lib` → **539 passed; 0 failed** (includes the 19-test
  `user_profile` suite: ledger merge/reverse-chron/pagination/type-filter/workspace-filter-drops-
  unattributed/rejection-attribution/compressed-detail-decompression/notifications-exclude-types/
  record_login-debounce, and the 5 postgres SQL-shape parity tests).
- `cargo test -p gyre-domain --lib` → **536 passed; 0 failed** (includes the new
  `approved_to_rejected_records_declining_human` and the AuditEventType round-trip tests).
- `SKIP_WEB_BUILD=1 cargo check -p gyre-server --lib --tests` → **clean** (compiler-verified arity
  at every `list_for_user`/`count_unresolved` call site; the fifth call-site miss found in an
  earlier round was repaired in `8e295c2e`).
- `cd web && npm ci && npm test` → **113 files / 2315 passed, 41 skipped, 0 failed**;
  `npx vitest run src/__tests__/UserProfile.test.js` → **30/30 passed**.
- Mechanical lints, all OK: `check-arch`, `check-byte-slice-truncation`, `check-relative-path-defaults`,
  `check-inert-enforcement`, `check-mem-port-contracts`, `check-forged-scope-fields`,
  `check-lossy-secret-conversion`, `check-scope-literal-defaults`, `check-fabricated-scope-defaults`,
  `check-dead-message-kinds`, `check-abac-route-registry`, `check-abac-exempt-handlers` (80 handlers),
  `check-mcp-write-tools`, `check-unbounded-external-http`, `check-in-memory-state-stores`,
  `check-fail-open-ref-resolution`, `check-forwarded-header-trust`, `check-migration-sql-portability`,
  `check-migration-versions`, `check-concurrent-shared-write`, `check-notification-toggle-coverage`
  (new lint, wired into pre-commit + CI).
- `check-task-commit-attribution`: every task-labeled commit in `adf4df3d..8eda66da` is present in
  the candidate's `specs/tasks/task-209.md` `commits:` frontmatter (verified against
  `git show 8eda66da:specs/tasks/task-209.md`; the working-tree failure of the script is the review
  stage's own reset of that file, not the candidate).
- `check-tenant-filter.sh`: cannot execute under this sandbox's mawk (gawk-only 3-arg `match()`;
  script unchanged by this candidate). Simulated its scan in Python over base and candidate:
  identical method/violation sets — the candidate introduces **no new** unfiltered read queries.
  The new judgment queries are user-scoped (`approver_id`/`audit_events.user_id`) and filter
  `workspace_id` in SQL when set.

## What was verified (per acceptance criterion)

- **Spec amendment (Part 1)** — HSI §12 now names the four concrete sources (spec_approvals LEFT
  JOIN spec_ledger_entries; audit_events rows with `event_type IN (gate_override, trust_change,
  meta_spec_publish)` attributed via `user_id`; meta-spec workspace attribution via scope/scope_id),
  and `?type=` values are `approval|rejection|gate|trust|meta-spec`, exactly `JudgmentType::as_str()`.
  `agent-gates.md` §Spec Approval Ledger amended for the Approved→Rejected decline-after-approval
  path (domain contract change is spec-lifecycle-consistent, and `gates.rs`'s test was updated to
  assert rejected approvals stop verifying at the forge).
- **`JudgmentType::GateOverride`** — `as_str() = "gate"`, `from_db_str("gate")` parses, with
  round-trip test.
- **Aggregation (Part 2)** — sqlite/postgres/mem parity. Real diesel queries against real tables;
  `?workspace_id=` FILTERS in SQL (LEFT JOIN on `spec_ledger_entries`, keyed by path so no fan-out);
  reverse-chron sort with deterministic entity_ref tie-break; limit/offset applied after union+sort.
  The mem adapter aggregates the SAME stores the repos write through (`with_sources`), wired in both
  `mem.rs::test_state_inner` and production `lib.rs`. The audit source materializes through the
  shared `AuditEventRow::into_event()` so `detail_zstd`-compressed rows decompress — a real bug class
  caught and fixed (`daf5eec5`). Hard adapter tests seed real rows incl. foreign-user and pending
  rows that must not leak/appear.
- **Trust-change write path** — `PUT /workspaces/:id` writes `trust_change` after a successful
  `apply_trust_transition` only (no event on same-value PUT), with from/to/workspace_id, attributed
  to `auth.user_id`; error propagation. Test drives the real router with a JWT and asserts via
  `state.audit.query`.
- **Gate-override write path** — a human (`user_id` present) approval review on an MR with FAILED
  gate results writes one `gate_override` event per failed gate (mr_id/gate_id/gate_type/
  failed→overridden); agent-token approvals excluded (tested). Semantics mirror
  `check_gates_for_mr` (any Failed result row = MR has a failed gate).
- **Meta-spec publish attribution** — registry create/update write `meta_spec_publish` with
  kind/name/version/content_hash and user attribution; workspace attribution from the meta-spec's
  own scope (Global/Tenant correctly carry no single-workspace attribution). Registry DELETE also
  gained real per-handler authorization (`authorize_meta_spec_delete`) — verified by
  `check-abac-exempt-handlers` (80 handlers OK).
- **Inbox filtering** — `NotificationRepository::list_for_user`/`count_unresolved` gained
  `exclude_types` applied IN the query before limit/offset (sqlite/postgres/mem parity; `ne_all`
  NOT-IN, empty list = no-op); users.rs inbox + badge and the MCP `notifications://` tool load
  disabled types from `user_notification_preferences`; background de-dup callers pass `&[]`
  (documented exemption). PUT rejects unknown notification_type with 400 and normalizes legacy
  parse aliases to canonical names (an alias would be a silently inert toggle — tested).
- **Auth provider info** — `users.oidc_issuer` added by migration `2026-10-07-000056`
  (version `20261007000056` is unique; no duplicate Diesel version — lint OK); `last_login_at`
  column ownership left to mainline task-120's `000058` with the collision documented in the
  migration header. JWT validation records issuer + login via `UserRepository::record_login`
  (single-statement debounced write, sqlite/postgres/mem parity, issuer-change carve-out);
  `GET /users/me` returns both; `PUT /users/me` has no field for either and the sqlite UPDATE
  statement does not touch `oidc_issuer` — read-only holds. End-to-end test drives real JWT auth.
- **Frontend** — canonical `NOTIF_TYPE_IDS` list kept in lockstep with the Rust enum by the new
  `check-notification-toggle-coverage.sh` lint (41/41); prefs load/save use the server wire format;
  ledger tab uses server-side `?type=`/`?workspace_id=` filters and offset pagination; badge count
  from the count endpoint. 30 component tests pass.

## Test-inflation check

None found. The adapter tests assert against seeded real SQLite rows with cross-user-leak,
pending-exclusion, and workspace-filter-drop assertions that fail if filtering regresses to
stamping. The handler tests drive the real router with real JWTs and assert persisted audit rows
via `state.audit.query` — they fail if the write path is removed. The alias-normalization and
unknown-type-400 tests kill real inert-toggle failure modes.

## Verifier-required execution (not run in-sandbox per the 4 GiB budget rule)

`cargo test -p gyre-server --lib` (or the focused set):
`api::users::tests::disabled_preference_excludes_type_from_inbox_and_count`,
`api::users::tests::preference_alias_normalizes_to_canonical_before_storing`,
`api::users::tests::get_me_returns_recorded_auth_provider_info`,
`api::users::tests::judgments_unknown_type_returns_400`,
`api::workspaces::tests::trust_transition_writes_trust_change_audit_event`,
`api::merge_requests::tests::human_approval_on_failed_gate_writes_gate_override_event`,
`api::meta_specs::tests::publish_writes_meta_spec_publish_audit_event`,
`api::specs::tests::{approve,revoke,reject}_via_jwt_surfaces_{,rejection_in_}judgment_ledger`,
`api::specs::tests::agent_can_revoke_its_own_approval_with_agent_jwt`,
`api::specs::tests::reject_approved_spec_records_declining_human`, plus the full
`cargo test --all` and `cd web && npm test` gates. Server lib + test targets type-check clean at
the candidate (`cargo check -p gyre-server --lib --tests`).

## Non-blocking observations

- `update_me` persists `last_login_at` from the loaded row; a `record_login` write landing between
  the load and the update can be reverted within the 60s debounce window (cosmetic last-seen
  regression only; `oidc_issuer` is not in the UPDATE's SET list and is safe).
- A human submitting a second approval review on the same failed-gate MR records a second
  `gate_override` event per failed gate — each approval is defensibly a judgment; noted for
  semantics, not a defect.
- `scripts/check-tenant-filter.sh` scans `src/pg` (dir is `postgres`) and uses gawk-only syntax —
  pre-existing, unchanged by this candidate.
