# Review — task-209 (Finish HSI §12 profile: notification filtering, judgment ledger, auth provider info)

Spec: `specs/system/human-system-interface.md` §12 "What the Profile Is".
Candidate under review: `6fea6da3da607672bc64ac9094e0e70fcfaab1c6` (base `6f5a631c9fc09664a3e766cd10536f8b56d9ce2f`).
Verdict: **approved** — no findings. All prior review findings (73ab19e9, f833c3a0 rounds: test-target compile failures and overstated evidence records) are independently confirmed fixed at this candidate.

## Probes run (candidate checkout at `6fea6da`)

All logs under `/tmp/stage/review-evidence/` (`t209-*`).

- `SKIP_WEB_BUILD=1 /tmp/stage/bin/cargo check -p gyre-server --lib --tests` → **clean** in 8m44s (`t209-srv-lib-check-rerun.log`). Independently re-establishes the prior round's blocking finding is fixed: the fifth `list_for_user` call site (`graph_extraction.rs` broadcast test, now ~:2527) passes `&[]` for `exclude_types`, and the whole server lib test target type-checks.
- `SKIP_WEB_BUILD=1 /tmp/stage/bin/cargo check -p gyre-server --test graph_integration` → clean (deps cached).
- `SKIP_WEB_BUILD=1 /tmp/stage/bin/cargo test -p gyre-adapters --lib` → **522 passed, 0 failed** (`t209-adapters-test.log`), incl. all 6 `judgment_ledger_*` tests, `notifications_exclude_types_applies_to_list_and_count`, `record_login_debounce_and_update`.
- `SKIP_WEB_BUILD=1 /tmp/stage/bin/cargo test -p gyre-domain --lib` → **507 passed** (AuditEventType round-trip incl. the new non-Custom-parse assertions).
- `cd web && npm ci && npm test` → **113 files, 2312 passed, 41 skipped** (`t209-web-full-test.log`); focused `UserProfile`+`Inbox` run also green (101).
- Mechanical checks: `check-arch.sh`, `check-abac-route-registry.sh`, `check-abac-exempt-handlers.sh` (79 handlers), `check-migration-versions.sh`, `check-migration-sql-portability.sh`, `check-dead-message-kinds.sh`, `check-mem-port-contracts.sh`, `check-notification-toggle-coverage.sh` → all **exit 0**.
- **Mutation probes (source restored; tree verified clean afterwards):**
  1. Disabled the `?workspace_id=` SQL filters in `sqlite/user_profile.rs::list_for_user` (both the `spec_ledger_entries` join filter and the `audit_events` filter) → `judgment_ledger_workspace_filter_drops_unattributed` **FAILS** (6 entries vs expected 4). The test proves filtering, not stamping.
  2. Removed the `notification_type.ne_all(excl)` filter in `sqlite/notification.rs` → `notifications_exclude_types_applies_to_list_and_count` **FAILS**. The exclusion is enforced in-query, not post-hoc.

## Verified

- **Spec amendment (Part 1).** HSI §12 now documents the four concrete aggregation sources with table/column attribution (`spec_approvals` + `spec_ledger_entries` join; `audit_events` rows `gate_override`/`trust_change`/`meta_spec_publish` attributed via `user_id`), and `?type=` values are exactly the `JudgmentType` wire strings (`approval|rejection|gate|trust|meta-spec`). The amendment also documents the inbox-filter scope (list + badge count + MCP tool; background de-dup callers exempt) and the PUT validation contract.
- **`JudgmentType::GateOverride`** with `as_str() = "gate"` and `from_db_str("gate")` parsing (domain round-trip test asserts the named parse arm).
- **Aggregation (Part 2).** `list_for_user` unions `spec_approvals` (LEFT JOIN `spec_ledger_entries` for workspace; rejection attributed to the declining human, not the original approver) with `audit_events` (3 judgment event types, read through the shared zstd-decompressing `AuditEventRow` path so compressed rows keep their detail), sorts reverse-chronologically with a deterministic tie-break, and applies limit/offset **after** the merge. Workspace filter is a real SQL filter (mutation-verified). SQLite/Postgres/mem parity maintained.
- **Write paths (Part 3).** Trust transition (`workspaces.rs:~352`) writes `trust_change` with from/to/workspace after `apply_trust_transition` succeeds, error-propagating; human approval on a failed-gate MR (`merge_requests.rs:~1083`) writes one `gate_override` per **failed** gate result with mr_id/gate_id/gate_type/status transition, and agent-token approvals (no `user_id`) are excluded — the test seeds one failed + one passed gate and asserts exactly one event; meta-spec registry create/update (`meta_specs.rs:2192`) writes `meta_spec_publish` with kind/name/version/content_hash and workspace attribution from the meta-spec's own scope.
- **Inbox filtering.** `disabled_notification_types` feeds `exclude_types` into the storage query for the inbox list, the unread badge count, and the MCP `notifications://` tool; applied before limit/offset. PUT `/users/me/notification-preferences` rejects unknown types (400) and normalizes legacy aliases to canonical `NotificationType::as_str()` names (a stored alias would be a dead toggle).
- **Auth provider info.** Migration `2026-10-07-000056_user_auth_provider_info` adds `users.oidc_issuer` (portable SQL, next-unused sequence number, `last_login_at` column-ownership collision with task-120's 000058 resolved rather than duplicated); JWT validation records issuer + last login via debounced `UserRepository::record_login` (sqlite/postgres/mem parity, debounce carve-out for issuer changes is tested); `GET /users/me` returns both read-only (`UpdateProfileRequest` has no field for either, so client-supplied values are dropped).
- **Evidence records are accurate.** `specs/tasks/task-209.md` "Shipped" and coverage row 53 state exactly what was executed in-sandbox vs. what remains the verifier's gate; no "all green" overstatement remains (prior round's docs finding addressed).
- **ABAC exemption file unchanged** (58 entries at base and candidate; the `/users/me/judgments` + `/notification-preferences` entries pre-exist at base `6f5a631`).
- **Frontend.** `notification-types.js` is the canonical toggle list, kept in lockstep with the Rust enum by `check-notification-toggle-coverage.sh` (CI + pre-commit wired); `UserProfile.test.js` derives the expected variant set from the Rust enum source itself (not a mirrored count), uses the real wire shapes, and the full vitest suite passes.

## Environment restriction (not a code defect)

`scripts/check-tenant-filter.sh` cannot execute in this sandbox: it uses gawk-only 3-argument `match($0, /re/, m)` and the sandbox `awk` is mawk 1.3.4 (parse error before scanning). The script is byte-identical to base (not touched by this task), CI runs it advisively (`|| true`), and a Python re-implementation of its heuristic shows the new judgment-ledger queries are consistent with the pre-existing handling of tables without a `tenant_id` column (the ledger is user-scoped via `audit_events.user_id` / `spec_approvals.approver_id`, matching the sibling `audit.rs` read paths). Verifier should run it under gawk.

## Tests the independent verifier must execute

Per the documented 4 GiB split, server test *execution* is deferred:

- `trust_transition_writes_trust_change_audit_event`
- `human_approval_on_failed_gate_writes_gate_override_event`
- `publish_writes_meta_spec_publish_audit_event`
- `disabled_preference_excludes_type_from_inbox_and_count`
- `get_me_returns_recorded_auth_provider_info`
- the `graph_extraction` broadcast test whose call site `8e295c2e` repaired
- full gates: `cargo test --all`, clippy/rustfmt, `bash scripts/check-tenant-filter.sh` (under gawk), exact-head GitHub checks

All five named tests exist in the lib test target (read and inspected; each drives the real router with real JWT/DB state), and that target compiles clean — but they have not been *executed* in this sandbox.
