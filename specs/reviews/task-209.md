# Review — task-209 (Finish HSI §12 profile: notification filtering, judgment ledger, auth provider info)

Spec: `specs/system/human-system-interface.md` §12 "What the Profile Is" (coverage row 53).
Candidate under review: `916078cb670203998917dc59024e0c2f76a8c2ff` (base `cdd1dd02`).
Verdict: **approved** — no findings. All three 2026-09-30 §12-cycle gaps are closed by real
production code; independent probe evidence below.

## Probes run (this round, at candidate `916078cb`)

- `cargo test -p gyre-domain --lib` → **511 passed** (incl. `JudgmentType` gate round-trip,
  `AuditEventType` named-variant parse arms, `Approved → Rejected` domain transitions).
  Log: `review-evidence/domain-tests.log`.
- `cargo test -p gyre-adapters --lib user_profile` → **19 passed** against real SQLite files:
  ledger merge/reverse-chron, workspace-filter-drops-unattributed, type-filter-selects-source,
  pagination-across-merged-stream, rejection-attributed-to-withdrawer,
  decompresses-compressed-audit-details (runs the real `compress_details_before` pass first),
  notifications-exclude-types (list + count), record_login debounce. PG sibling carries
  SQL-shape pins (statement-level WHERE/JOIN/attribution-key assertions via `debug_query`,
  repo precedent: no PG harness). Log: `review-evidence/adapters-user-profile-tests.log`.
- `SKIP_WEB_BUILD=1 cargo check -p gyre-server --lib --tests` → clean in 5m15s
  (compiler-verified arity at every `list_for_user` call site). Log: `review-evidence/server-check.log`.
- `cd web && npm ci && npx vitest run src/__tests__/UserProfile.test.js` → **30/30 passed**;
  full `npm test` → **113 files, 2315 passed / 41 skipped**. Logs: `review-evidence/web-*.log`.
- Mechanical checks (all exit 0): `check-arch.sh`, `check-migration-versions.sh`,
  `check-notification-toggle-coverage.sh` (new script, wired into pre-commit + CI; 41 enum
  variants = 41 toggles = 41 locale labels), `check-abac-route-registry.sh`,
  `check-abac-exempt-handlers.sh`, `check-mem-port-contracts.sh`, `check-inert-enforcement.sh`,
  `check-forged-scope-fields.sh`, `check-unbounded-external-http.sh`, `check-scope-literal-defaults.sh`.
  Logs under `review-evidence/check-*.log`.

## Verification per acceptance criterion

- **Spec amendment (Part 1)** — HSI §12 Judgment Ledger now carries a concrete Sources table
  (spec_approvals + audit_events `gate_override`/`trust_change`/`meta_spec_publish` + the
  attribution-key rules) and `?type=` values `approval|rejection|gate|trust|meta-spec` exactly
  matching `JudgmentType::as_str()`. The domain change the amendment presumes (Approved →
  Rejected decline-after-approval) was propagated to `agent-gates.md` §Spec Approval Ledger —
  spec lifecycle followed, not worked around.
- **`JudgmentType::GateOverride`** — `as_str() = "gate"`, `from_db_str("gate")` parses; domain
  test asserts the parse arm explicitly.
- **Aggregation** — `list_for_user` (sqlite/postgres/mem parity, PG SQL-shape-pinned) unions
  spec_approvals (LEFT JOIN spec_ledger_entries on path — PK, so no row multiplication) with
  audit_events filtered `user_id = approver AND event_type IN (3 judgment types)`; `?workspace_id=`
  is a SQL WHERE filter (NULL-workspace rows drop when set — verified by
  `judgment_ledger_workspace_filter_drops_unattributed`), merged stream sorted
  reverse-chron with entity_ref tie-break, limit/offset applied after the merge
  (verified by `judgment_ledger_pagination_across_merged_stream`). Compressed audit rows
  (detail_zstd) decompress through the shared `AuditEventRow::into_event` — covered by a test
  that runs the real compression pass first.
- **Trust-change write path** — `PUT /workspaces/:id` writes a `trust_change` AuditEvent
  (user_id = acting human, workspace_id, from/to in detail) only on a real transition
  (same-value PUT writes nothing — asserted); server test drives the real router with a real
  signed JWT and asserts the stored event.
- **Gate-override write path** — human (`user_id`-bearing principal) approval review on an MR
  with failed gate results writes one `gate_override` event per failed gate (mr_id, gate_id,
  gate_type, failed→overridden — honest transition name); agent-token approvals excluded
  (asserted). Test also proves the ledger surfaces it via `GET /users/me/judgments?type=gate`.
- **Meta-spec publish attribution** — registry create/update write `meta_spec_publish` events
  (kind/name/version/content_hash, workspace attribution from scope, Global/Tenant correctly
  NULL); tested incl. version-bump second event. (Bonus hardening in the same diff:
  registry DELETE gained real per-handler authorization with tenant containment.)
- **Inbox filtering** — `exclude_types` added to the port; applied in-query before limit/offset
  in both `list_for_user` and `count_unresolved` (sqlite/postgres parity, `ne_all`), wired into
  the inbox list, the badge count, and the MCP `notifications://` tool; background de-dup
  callers pass `&[]` per the amended spec's exemption. PUT normalizes legacy parse aliases to
  canonical `as_str()` names and 400s unknown types (a stored typo would be an inert toggle).
  Handler-level test proves exclusion end-to-end for list + badge.
- **Auth provider info** — migration `2026-10-07-000056` adds `users.oidc_issuer` (unique
  sequence number, portable ALTER TABLE; `last_login_at` correctly left to task-120's
  000058 to avoid duplicate-column); `record_login` (sqlite/postgres/mem parity) does the
  debounce interval check and write in ONE statement (no read-modify-write race), with an
  issuer-change carve-out; JWT validation records both; `GET /users/me` returns them read-only
  and `PUT /users/me` has no field for either. Adapter test covers debounce + issuer update;
  server test proves a real OIDC JWT drives `record_login` and both fields round-trip.
- **Writer/reader identity-key parity** — the approve/reject/revoke endpoints write the
  canonical user Id into `spec_approvals.approver_id`/`rejected_by`/`revoked_by` (KeyBinding
  labels stay on the history events where that contract needs them), and `get_judgments`
  resolves the caller the same way. End-to-end tests in `specs.rs`
  (`approve_via_jwt_surfaces_in_judgment_ledger`,
  `revoke_via_jwt_surfaces_rejection_in_judgment_ledger`) pin this — the exact mismatch class
  that made the pre-task ledger hollow.

## Test-inflation check

None found. The adapter tests seed real SQLite databases and assert behavior (a workspace
filter regression, a stamp-only regression, a per-source pagination regression, a compressed-
detail regression, and a debounce regression each fail the corresponding test). The server
tests drive the real router with real signed JWTs and assert stored rows. The frontend tests
assert server-side `?type=`/`?offset=` re-fetch, canonical wire names, and the server-side
badge count. `check-notification-toggle-coverage.sh` mechanically pins the frontend type list
to the Rust enum.

## Verifier gates (not executable in this sandbox — recorded per review rules)

1. `cargo test --all` (the server lib tests were compile-checked here but not executed:
   `cargo test -p gyre-server --lib` must run `trust_transition_writes_trust_change_audit_event`,
   `human_approval_on_failed_gate_writes_gate_override_event`,
   `publish_writes_meta_spec_publish_audit_event`,
   `disabled_preference_excludes_type_from_inbox_and_count`,
   `preference_alias_normalizes_to_canonical_before_storing`,
   `get_me_returns_recorded_auth_provider_info`,
   `approve_via_jwt_surfaces_in_judgment_ledger`,
   `revoke_via_jwt_surfaces_rejection_in_judgment_ledger`, the gates.rs
   rejected-approval-stops-verifying test, and the meta-spec delete-authorization tests).
2. `cd web && npm test` — already executed in this review round: **pass** (2315 tests).

## Environment notes (not code defects)

- `scripts/check-tenant-filter.sh` and `scripts/check-cross-surface-parity.sh` exit 2 in this
  sandbox with awk dialect errors (`mawk 1.3.4` lacks gawk's 3-argument `match()`); both fail
  identically at the base commit and are unmodified by this candidate. The verifier (gawk
  environment, per CI) should re-run them.
- Generated `web/dist/` regeneration is verifier-owned per checkpoint policy; the authored
  frontend source and tests are what this review examined (and they pass).

Minor observations (non-blocking, no action required):

- `GET /api/v1/notifications/unread-count` (user-management.md spec path, read-tracking
  semantics via `count_unread`) does not apply the disabled-type exclusion. The amended HSI §12
  contract scopes the filter to the inbox list and the active badge count — both filter — so
  this is outside the amended contract, but a future reconciliation may want to decide
  deliberately whether a toggled-off type should count as unread.
- A pre-existing merge_requests test gained a duplicate `authorization` header line
  (harmless; HTTP header set is identical).
