# Review — task-209 (Finish HSI §12 profile: notification filtering, judgment ledger, auth provider info)

Spec: `specs/system/human-system-interface.md` §12 "What the Profile Is" (coverage row 53).
Base: `103d665e` → candidate `32566e2d` (diff spans the branch's product/merge-repair history;
generated `web/dist` regeneration commits are verifier-owned and excluded from this review).
Verdict: **approved** — no blocking findings.

## Probes run (independent, this round)

All logs retained under `/tmp/stage/review-evidence/`:

- `cargo test -p gyre-domain` → **540 passed, 0 failed** (`domain-tests.log`) — includes the new
  `AuditEventType::{TrustChange,GateOverride,MetaSpecPublish}` round-trip + named-variant-parse
  tests and the amended `SpecApproval` transition tests (Approved→Rejected records decliner,
  terminal-state enforcement).
- `cargo test -p gyre-adapters --lib user_profile` → **27 passed, 0 failed**
  (`adapters-user-profile-tests.log`) — ledger merge across all 4 sources reverse-chron with
  deterministic tie-break, `?workspace_id=` real-SQL filtering (unattributed rows excluded),
  `?type=gate` source selection, pagination after union+sort, rejection attributed to the
  withdrawing human (not the original approver), zstd-compressed `audit_events.detail`
  decompression on ledger reads, `notifications_exclude_types` on list AND count,
  `record_login` debounce/issuer-change-carve-out/preservation-through-update, and the
  postgres query-shape parity tests.
- `SKIP_WEB_BUILD=1 cargo check -p gyre-server --lib --tests` → **clean in 5m03s**
  (`server-check.log`) — every `list_for_user`/`count_unresolved`/`record_login` call site
  type-checks on the merged tree (the prior-round arity miss in `graph_extraction.rs` is fixed).
- `cd web && npm ci && npm test` → **2371 passed, 41 skipped, 116 files**
  (`web-tests.log`) — includes the expanded `UserProfile.test.js` (canonical-type toggles,
  server-side ledger filtering/pagination, badge from count endpoint, read-only OIDC
  issuer/last login).
- Mechanical: `check-arch.sh`, `check-migration-versions.sh`,
  `check-migration-sql-portability.sh`, `check-notification-toggle-coverage.sh` (new, 41 enum
  variants = 41 toggles = 41 locale labels) → all exit 0 (`script-*.log`).

## Verified against the acceptance criteria

1. **Spec amendment (Part 1)** — HSI §12 now names the four concrete sources
   (`spec_approvals` ⊕ `spec_ledger_entries` join; `audit_events` rows
   `gate_override`/`trust_change`/`meta_spec_publish` with `user_id` attribution) and the
   `?type=` values `approval|rejection|gate|trust|meta-spec` matching `JudgmentType::as_str()`.
   The human-vs-agent attribution-key trap (`audit_events.agent_id` = agent-about;
   `spec_approval_events.approver_id` = KeyBinding label) is documented and consistently
   implemented: the approve/reject/revoke endpoints write the **canonical user Id**, agents keep
   their labels. `agent-gates.md` §Spec Approval Ledger amended for the Approved→Rejected
   decline-after-approval path (domain, adapters, and gate-executor tests updated in lockstep).
2. **`JudgmentType::GateOverride`** — `as_str() = "gate"`, `from_db_str("gate")` parses; unknown
   `?type=` now 400s (`judgments_unknown_type_returns_400`) instead of silently meaning "no
   filter".
3. **Aggregation** — `list_for_user` (sqlite/postgres/mem parity) queries real tables;
   `?workspace_id=` is a SQL WHERE on the joined `spec_ledger_entries.workspace_id` /
   `audit_events.workspace_id` (NULL-attributed rows excluded when set, matching the amended
   spec), not the old param-stamping. Limit/offset applied after the merged sort. The mem
   adapter aggregates the *same* shared stores the write paths use (wired via
   `store_shared!` in `lib.rs` and `test_state_inner`), so handler tests exercise real data flow.
4. **Trust write path** — `PUT /workspaces/:id` writes one `trust_change` `AuditEvent`
   (user_id = acting human, from/to/workspace_id) only on an actual transition, failure
   propagates (`?`), no event on same-value PUT. In-module test drives the real router with a
   signed JWT and asserts attribution + no-event-on-no-change.
5. **Gate-override write path** — human (`auth.user_id.is_some()`) `Approved` review on an MR
   with failed gate results writes one `gate_override` event per FAILED gate carrying
   mr_id/gate_id/gate_type/from→to; agent-token approvals write nothing (explicitly asserted);
   the test then verifies the event surfaces through `GET /users/me/judgments?type=gate`.
6. **Meta-spec publish** — registry create/update write `meta_spec_publish` with
   kind/name/version/content_hash; workspace attribution only for `MetaSpecScope::Workspace`
   (Global/Tenant carry none — documented rationale matches the `?workspace_id=` exclusion
   semantics). The previously missing per-handler authorization on registry DELETE was added
   with role/scope tests.
7. **Inbox filtering** — `exclude_types` threaded through the port into both
   `list_for_user` and `count_unresolved` (sqlite `ne_all` before limit/offset; postgres
   parity; mem parity), wired in the inbox handler, badge count, and the MCP `notifications://`
   tool; background de-dup callers pass `&[]` per the amended spec. PUT normalizes legacy parse
   aliases to canonical `NotificationType::as_str()` names and 400s unknown types — closing the
   silently-inert-toggle class (alias stored → never matches a persisted row). The web toggle
   list was replaced with the canonical 41-variant list guarded by the new CI/pre-commit lint.
8. **Auth provider info** — migration `2026-10-07-000056` adds `users.oidc_issuer` (portable
   SQL, no duplicate-column collision with task-120's `last_login_at` owner);
   `validate_jwt` calls `UserRepository::record_login` (single-statement debounce in
   sqlite/postgres, mutex-guarded in mem; issuer change always bypasses the debounce);
   `GET /users/me` returns both read-only; `PUT /users/me` has no field for either and
   `update()` preserves them (adapter test).

## Non-blocking observations (recorded, not findings)

- `scripts/check-tenant-filter.sh` exits 2 in this sandbox with a mawk syntax error
  (`match($0, /re/, m)` is gawk-only). The script is **unchanged by this candidate** and the
  failure is environmental, not a code defect; its intent was verified manually: the new
  `audit_events` ledger queries filter by `user_id` + `workspace_id` (real columns), and
  `audit_events` has no `tenant_id` column to filter on (pre-existing table shape, consistent
  with how the rest of the codebase treats it).
- Large gyre-server test-binary builds were deferred to the independent verifier per the
  4 GiB budget rule; `cargo check -p gyre-server --lib --tests` was run instead and is clean.

## Verifier must execute (server test gate)

- `cargo test --all` (full suite).
- Focused server tests for this task's write paths and handlers:
  `api::workspaces::tests::trust_transition_writes_trust_change_audit_event`,
  `api::merge_requests::tests::human_approval_on_failed_gate_writes_gate_override_event`,
  `api::meta_specs::tests::publish_writes_meta_spec_publish_audit_event` (plus the new
  meta-spec DELETE authorization tests),
  `api::users::tests::disabled_preference_excludes_type_from_inbox_and_count`,
  `api::users::tests::preference_alias_normalizes_to_canonical_before_storing`,
  `api::users::tests::get_me_returns_recorded_auth_provider_info`,
  `api::users::tests::judgments_unknown_type_returns_400`, and the
  `api::specs::tests::*_surfaces_in_judgment_ledger` trio
  (approve/revoke/reject via real signed JWTs).
- Full mechanical check battery (`/tmp/stage/checks.sh` equivalent) and the frontend suite on
  the rebuilt `web/dist`.
