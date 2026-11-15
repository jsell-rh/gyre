# Review — task-209 (Finish HSI §12 profile: notification filtering, judgment ledger, auth provider info)

Spec: `specs/system/human-system-interface.md` §12 "What the Profile Is".
Candidate under review: `58396573cef22338758d87bbf59bd4af38efaab4` (assignment base `b8d63b0d`).
This round is the merge repair: published head `2eb8bcc3` conflicted with main `b8d63b0d`
(task-113 in-app notification UI); merge `aab5f74c` resolves it. HEAD differs from `aab5f74c`
only by process commits touching `specs/tasks/task-209.md`.

Verdict: **approved** — all acceptance criteria are backed by real production code; every
feasible focused probe passes; two mutation probes confirm the key tests kill real bugs.

## Probes run (evidence in /tmp/stage/review-evidence/)

- `SKIP_WEB_BUILD=1 cargo test -p gyre-domain` → 540 + 8 passed, 0 failed (incl. the new
  `AuditEventType` named-variant parse assertions for `trust_change`/`gate_override`/
  `meta_spec_publish`, and `JudgmentType` round-trip).
- `SKIP_WEB_BUILD=1 cargo test -p gyre-adapters user_profile` → 27 passed (judgment ledger
  merge/reverse-chron/pagination-across-merged-stream/type-filter/workspace-filter/rejection-
  attribution/compressed-detail decompression, notification exclusion list+count,
  `record_login` debounce, prefs, tokens, sessions).
- `SKIP_WEB_BUILD=1 cargo test -p gyre-adapters notification` → 11 passed.
- `SKIP_WEB_BUILD=1 cargo check -p gyre-server` → clean (3m54s).
- `cd web && npm ci && npx vitest run` on UserProfile + NotificationBell/Drawer/Settings +
  notifications-store → 5 files, 50 tests passed.
- `cargo fmt --check -p gyre-server -p gyre-adapters -p gyre-domain -p gyre-ports` → clean.
- Mechanical scripts all exit 0: migration-versions, arch, mem-port-contracts,
  dead-message-kinds, scope-literal-defaults, inert-enforcement, task-commit-attribution,
  notification-toggle-coverage (new script), abac-route-registry, abac-exempt-handlers,
  mcp-write-tools, migration-sql-portability, byte-slice-truncation, in-memory-state-stores,
  unbounded-external-http, forged-scope-fields, forwarded-header-trust,
  fail-open-ref-resolution, lossy-secret-conversion, fabricated-scope-defaults,
  relative-path-defaults.
- **Mutation probe 1** (exclusion disabled in `sqlite/notification.rs` `list_for_user`):
  `notifications_exclude_types_applies_to_list_and_count` FAILS. Restored; suite re-green.
- **Mutation probe 2** (spec_approvals `?workspace_id=` filter disabled in
  `sqlite/user_profile.rs`, i.e. the old stamp-not-filter behavior):
  `judgment_ledger_workspace_filter_drops_unattributed` FAILS. Restored; suite re-green.
- `scripts/check-tenant-filter.sh` cannot execute in this mawk-only sandbox (awk syntax error
  before scanning) — infrastructure, not a code defect; verifier must run it under gawk.

## Verified against the task contract

- **Spec amendment (Part 1)** is faithful and accurate to the code: the Sources table names
  `spec_approvals` (+ `spec_ledger_entries.workspace_id` join), `audit_events` rows with
  `event_type` `gate_override`/`trust_change`/`meta_spec_publish` (both `user_id` and
  `workspace_id` columns exist — migration 2026-09-30-000054), `?type=` wire strings
  `approval|rejection|gate|trust|meta-spec` match `JudgmentType::as_str()`/`from_db_str`.
  The `spec_approval_events` reference in the amendment note is a real table (schema.rs:673).
- **JudgmentType::GateOverride** = `"gate"`, `from_db_str("gate")` parses (domain tests).
- **Aggregation (Part 2)**: `list_for_user` queries real tables (spec_approvals LEFT JOIN
  spec_ledger_entries; audit_events filtered by `user_id` + the three judgment event types,
  materialized through the shared `AuditEventRow` read path so zstd-compressed details
  decompress); `?workspace_id=` is a SQL filter (mutation-prove), union sorted
  reverse-chron with deterministic tie-break, limit/offset applied after the merge.
  Postgres parity mirrored (incl. a cross-backend `JUDGMENT_AUDIT_TYPES` parity test);
  mem adapter mirrors the same semantics.
- **Write paths (Part 3)**: trust transition in `PUT /workspaces/:id` writes `trust_change`
  (from/to/workspace_id, attributed to `auth.user_id`, only on real transitions); human
  approval on an MR with failed gate results writes one `gate_override` event per failed
  gate (mr_id/gate_id/gate_type/from_status=failed/to_status=overridden; agent JWTs and the
  global token have `user_id: None` in auth.rs so agent approvals are excluded — verified in
  auth.rs); meta-spec registry create/update write `meta_spec_publish` (kind/name/version/
  content_hash, workspace attribution only for Workspace scope, with a reasoned Global/
  Tenant carve-out). All event-writes propagate errors (no swallowed audit failures).
- **Notification filtering**: `exclude_types` threaded through the port and all three
  adapters, applied in-query before limit/offset; inbox list, badge count, and the MCP
  notifications:// tool all load disabled types from `user_notification_prefs`;
  background de-dup callers pass `&[]` per the amended spec's exemption.
  `PUT /users/me/notification-preferences` 400s on unknown types and normalizes legacy
  parse aliases to the canonical `NotificationType::as_str()` before storing (guarded by a
  dedicated alias-normalization test).
- **Auth provider info**: migration 2026-10-07-000056 adds `users.oidc_issuer`;
  `last_login_at` column is owned by task-120's existing migration (duplicate-column
  collision resolved coherently — commented in the migration). `validate_jwt` calls
  `UserRepository::record_login` (single-statement conditional update, debounced 60s;
  sqlite/postgres/mem parity). `GET /users/me` returns both read-only; `PUT /users/me` has
  no field for either (request struct verified).
- **Merge integrity**: `git diff b8d63b0d..aab5f74c` has zero deletion-only files; the only
  deletions relative to main are call-site updates adding the `exclude_types` argument
  (mechanical arity fix). Task-113's NotificationBell/Drawer/Settings web components are
  byte-identical to main; the two features meet in one preference store
  (`user_notification_prefs`), no split-brain.
- **Coverage matrix** row 53 flipped to `implemented` with honest evidence, explicitly
  recording that server-side test execution is the verifier's gate.

## Test-inflation check

None found. The adapter tests seed real SQLite rows and assert specific merged behavior
(filtering, ordering, pagination, attribution, compressed-detail decompression); both
mutation probes fail the corresponding tests. The server tests (read in the diff) drive the
real router with real OIDC test JWTs, include negative cases (agent-token approval must not
write `gate_override`; no-change PUT must not write `trust_change`; same-value re-PUT), and
assert end-to-end ledger surfacing via `GET /users/me/judgments?type=gate`.

## Verifier must run (not executable in this sandbox)

- `cargo test --all` on the exact candidate head (server test compilation is beyond the
  4 GiB sandbox budget; `cargo check -p gyre-server` is clean at this head).
- Server-side task-209 tests in particular:
  `api::merge_requests::tests::human_approval_on_failed_gate_writes_gate_override_event`,
  `api::workspaces::tests::trust_transition_writes_trust_change_audit_event`,
  `api::meta_specs::tests::publish_writes_meta_spec_publish_audit_event`,
  `api::users::tests::{disabled_preference_excludes_type_from_inbox_and_count,
  preference_alias_normalizes_to_canonical_before_storing,
  get_me_returns_recorded_auth_provider_info}`, the judgments-endpoint shape tests in
  `api::users.rs` / `api::specs.rs` (`*_surfaces_in_judgment_ledger`).
- `scripts/check-tenant-filter.sh` under gawk.
- Full `cd web && npm ci && npm test` (implementer reports 2391 passed; I verified the five
  task-relevant suites, 50 tests).

## Non-blocking observations (recorded separately)

- The implementer's Shipped notes cite mechanical-check logs "in /tmp/stage/review-evidence/"
  from their round; that directory was not present in this sandbox. I re-ran all listed
  checks independently — all exit 0 — so the stale log citation has no bearing on the code.
- `web/dist` is resolved to main's (assignment-base) assets; per the stage contract the
  verifier rebuilds and publishes remotely. Generated output — not reviewed, not blocking.
