# Review: task-209 — Finish HSI §12 profile (notification filtering, judgment ledger, auth provider info)

**Candidate:** `498a9bdcaa604a415ca4df8b402174a8446f76b6` (base `7e9b3cfeea8598e05c8b0ae971e06f214dd21806`)
**Reviewer:** independent review agent, 2026-11-12 (round after prior F1 repair `5564c00b8`)
**Verdict:** **Approved** — all three HSI §12 §"What the Profile Is" gaps closed with real production code; prior finding F1 fixed and mechanically guarded.

## What was verified (evidence: `/tmp/stage/review-evidence/probe-log.md`)

### 1. Inbox notification filtering (spec §1406)
- `NotificationRepository::{list_for_user, count_unresolved}` gained `exclude_types`, applied **in the SQL query before limit/offset** in both SQLite and Postgres adapters; mem adapter parity (`!exclude_types.contains(...)` in both list and count filters).
- `users.rs` inbox + badge endpoints load disabled types from `user_notification_preferences` (no row = enabled; only `enabled = 0` disables). MCP `notifications://` inbox tool applies the same exclusion (mcp.rs); background de-dup callers pass `&[]` per the port doc.
- `PUT /users/me/notification-preferences` normalizes through `NotificationType::parse` and 400s unknown types — a stored alias would be a dead toggle (this was the inert-toggle bug class).
- **Mutation probe:** disabling the `ne_all` exclusion in sqlite/notification.rs makes `notifications_exclude_types_applies_to_list_and_count` fail (both list and count); restored, green. The test kills the bug.

### 2. Judgment ledger (spec §1413-1421)
- `JudgmentType::GateOverride` added, `as_str() = "gate"`, `from_db_str("gate")` parses; wire strings `approval|rejection|gate|trust|meta-spec` match the amended spec §12.
- `list_for_user` (sqlite/postgres/mem parity) aggregates all four categories from real tables: `spec_approvals` LEFT JOIN `spec_ledger_entries` (workspace attribution), plus `audit_events` rows with `event_type IN (gate_override, trust_change, meta_spec_publish)` attributed via `audit_events.user_id`, materialized through the shared `AuditEventRow` path so `detail_zstd`-compressed rows decompress. Merged stream sorted reverse-chron (deterministic tie-break), limit/offset after the merge. Rejections are attributed to the declining human, not the original approver — writer (specs.rs approve/reject/revoke write canonical user Ids) and reader agree on the attribution key.
- **Real write paths:** trust transition via `PUT /workspaces/:id` writes a `trust_change` audit event (only on actual transition; propagates errors); a human (user_id-bearing) approval on an MR with FAILED gate results writes one `gate_override` event per failed gate (agent-token approvals excluded); meta-spec registry create/update write `meta_spec_publish` events with kind/name/version/content_hash and scope-correct workspace attribution.
- **Mutation probe:** disabling both workspace SQL filters (the old "stamp instead of filter" behavior) makes `judgment_ledger_workspace_filter_drops_unattributed` fail; restored, green. Adapter suite 12/12.
- Mem `JudgmentLedgerRepository` aggregates the *same* stores the write paths use (`with_sources` shared handles in `test_state`), so server-level endpoint tests are end-to-end, not stubbed.

### 3. Auth provider info (spec §1407)
- Migration `2026-10-07-000056_user_auth_provider_info` adds `users.oidc_issuer`; `last_login_at` column ownership correctly left to task-120's later migration (duplicate-column collision documented in the migration header). Portable SQL (passes `check-migration-sql-portability.sh`).
- `auth.rs` JWT validation records both via `UserRepository::record_login` — single-statement conditional update (no read-modify-write race), debounced 60s, issuer-change carve-out; sqlite/postgres/mem parity. Failure is a warning on the auth hot path, which is correct (auth must not depend on bookkeeping).
- `GET /users/me` returns `oidc_issuer`/`last_login_at` read-only; no client-writable path (PUT has no field for either — passes `check-forged-scope-fields.sh` reasoning). UI displays them; UserProfile.test.js asserts the read-only rendering.

### 4. Prior finding F1 (re-review)
`ComputeTargetUnavailable` toggle added via shared canonical list `web/src/lib/notification-types.js` + locale label; new `scripts/check-notification-toggle-coverage.sh` (wired into CI + pre-commit) mechanically diffs the Rust enum (40 variants) against toggle ids and locale labels — currently "40 enum variants, 40 toggles, labels in lockstep". The UserProfile toggle test asserts against the shared list length (40 = enum count), not a hardcoded render count.

### 5. Spec amendment
HSI §12 amended with the concrete source table/attribution matrix (approval/rejection/gate/trust/meta-spec), the filtering semantics of `?workspace_id=` (excludes unattributed when set), and `?type=` values matching `JudgmentType` wire strings — the amendment names real, shipped write paths, so it is not fake-completion. Coverage matrix row 53 flipped to implemented with accurate notes.

## Probes run
- `cargo test -p gyre-adapters --lib sqlite::user_profile` — 12/12 pass (real SQLite file DB).
- Two mutation probes (workspace filter, notification exclusion) — both killed by tests; sources restored, re-run green.
- `cargo check -p gyre-server` — clean.
- `cd web && npm ci && npm test` — 109 files, 2185 passed / 41 skipped (pre-existing), exit 0; focused UserProfile.test.js 27/27.
- All relevant mechanical guards pass (arch, ABAC registry/exempt handlers, mem port contracts, migration versions/portability, dead message kinds, forged scope fields, in-memory state stores, unbounded HTTP, inert enforcement, MCP write tools, lossy secrets, scope literal/fabricated defaults, notification toggle coverage). `check-tenant-filter.sh` prints an awk error identically on the base commit — environment awk, not a candidate defect.

## Verifier must run (gyre-server test compilation exceeds review-stage budget)
- `cargo test --all`
- Focused: `cargo test -p gyre-server --lib api::users`, `api::merge_requests` (`human_approval_on_failed_gate_writes_gate_override_event`), `api::workspaces` (`trust_transition_writes_trust_change_audit_event`), `api::meta_specs` (`publish_writes_meta_spec_publish_audit_event`), `api::specs` (`*_surfaces_in_judgment_ledger`).

## Non-blocking observations
- `GET /users/me/judgments?type=<unknown>` silently ignores the unrecognized value (returns all types) rather than 400. The spec does not require rejection on the GET, and the PUT prefs endpoint (where a typo would silently create an inert toggle) does 400; noting for a possible future hardening, not a defect against the contract.
- Working-tree modification of `specs/tasks/task-209.md` (progress reset to the assigned task text) is executor stage prep, not candidate content; the committed task file at the candidate records all nine branch commits in `commits:` per the attribution rule.
