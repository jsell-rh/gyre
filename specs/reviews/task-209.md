# Review — task-209 (Finish HSI §12 profile: notification filtering, judgment ledger, auth provider info)

Spec: `specs/system/human-system-interface.md` §12 "What the Profile Is" (coverage row 53).
Candidate: `d465d3fb25338692d9071921de463742a4862d1a` on `pipeline/task-209/1eb6bebd8b2b438d8311dc50a791a66e-1`, diffed against assigned base `fb1f82e31`.
Verdict: **approved** (round 1). Evidence: `/tmp/stage/review-evidence/task-209-final-review.md`.

## What was verified (independent probes, exit codes in the evidence file)

- **Spec amendment (Part 1)** — §12 Judgment Ledger now carries a concrete Sources
  table (category → table → human-attribution column → workspace-attribution column),
  pins `?type=` to exactly the `JudgmentType` wire strings
  (`approval|rejection|gate|trust|meta-spec`), and documents the write paths. The
  auth-provider and notification-preference bullets were also tightened to the
  implemented semantics (no-row = enabled; in-query filtering for list AND badge AND
  the MCP tool; background de-dup exempt; PUT rejects unknown types).
- **`JudgmentType::GateOverride`** — added with `as_str() = "gate"` and
  `from_db_str("gate")`; `AuditEventType` gains `TrustChange`/`GateOverride`/
  `MetaSpecPublish` named variants with an explicit not-`Custom` round-trip test.
- **Aggregation (Part 2)** — `list_for_user` (SQLite/Postgres/mem parity) unions
  `spec_approvals` (LEFT JOIN `spec_ledger_entries`) with `audit_events`
  (gate_override/trust_change/meta_spec_publish, `user_id` attribution), sorts
  reverse-chronologically with a deterministic tie-break, and applies limit/offset
  after the merge only. `?workspace_id=` is a real SQL filter that drops
  unattributed rows — the old stamp-the-param defect is gone (adapter tests
  `judgment_ledger_workspace_filter_drops_unattributed`,
  `judgment_ledger_type_filter_selects_source`,
  `judgment_ledger_pagination_across_merged_stream`, 11/11 green).
- **Write paths (Part 3)** — trust transition via `PUT /workspaces/:id` writes a
  `trust_change` audit event attributed to the acting user (from/to/workspace_id;
  no event on a no-change PUT); a human approval review on an MR with failed gate
  results writes one `gate_override` event per failed gate (agent-token approvals
  excluded); meta-spec registry create/update write `meta_spec_publish` with
  kind/name/version/content_hash and workspace attribution from scope. All three
  verified by focused server tests (5/5 green) driving the real router with real
  signed OIDC JWTs.
- **Approval attribution repair (94af4aef)** — the production approve/reject/revoke
  endpoints now write the canonical user Id into
  `spec_approvals.approver_id`/`rejected_by`/`revoked_by` (the `user:<name>`
  KeyBinding label stays on the history events where that contract needs it), and
  all three ledger readers attribute a rejection/revocation to the declining human.
  Verified end-to-end: `approve/reject/revoke_via_jwt_surfaces_*_in_judgment_ledger`
  assert both the written key and the surfaced `/users/me/judgments` entry (3/3
  green). Confined to the ledger rows: `is_valid_human_approval` and the
  revoke-authorization check still compare the history-event labels, unchanged, and
  no other consumer of the ledger `approver_id` depends on the old shape. The
  adjacent `revoke_spec_approval` caller-label derivation bug (every JWT treated as
  `agent:`) is fixed to mirror approve's scope-based derivation.
- **Notification filtering** — `NotificationRepository::list_for_user`/
  `count_unresolved` take `exclude_types`, applied in-query before pagination
  (SQLite/Postgres/mem parity); users.rs inbox + badge endpoints and the MCP
  `notifications://` tool load disabled types from
  `user_notification_preferences`; background de-dup callers pass `&[]`.
  `disabled_preference_excludes_type_from_inbox_and_count` proves the handler
  wiring, not just the query. The web profile now sends the 27 canonical
  `NotificationType` names (the old 10 made-up ids could never match — latent
  defect fixed here).
- **Auth provider info** — migration `2026-10-07-000056` adds `users.oidc_issuer`
  (plain `ALTER TABLE`, portable; `last_login_at` correctly left to task-120's
  000058 to avoid a duplicate-column collision); JWT validation records issuer +
  login via a single-statement conditional UPDATE (no read-modify-write race) with
  an issuer-change carve-out that bypasses the debounce; `GET /users/me` returns
  both read-only and `PUT /users/me` has no field for either
  (`get_me_returns_recorded_auth_provider_info` green via a real JWT).
- **Suites** — `cargo test -p gyre-adapters --lib` 399 passed; `cargo test -p
  gyre-server --lib` 1545 passed; `cd web && npm ci && npm test` 2003 passed /
  41 skipped / 98 files. Mechanical checks: check-arch, check-migration-versions,
  check-migration-sql-portability, check-mem-port-contracts,
  check-inert-enforcement, check-forged-scope-fields, check-scope-literal-defaults,
  check-fabricated-scope-defaults, check-in-memory-state-stores,
  check-abac-route-registry, check-mcp-write-tools, check-dead-message-kinds,
  check-task-commit-attribution — all OK.
- **Bonus hardening** — meta-spec registry DELETE gained real per-handler
  authorization (tenant containment + workspace Owner/Admin or tenant Admin,
  403-before-404 existence-oracle ordering) on a previously label-only check.

## Minor observations (non-blocking)

- The amended spec's "PUT MUST reject an unknown notification_type (400)" is
  implemented but had no committed test; I verified it with a temporary probe test
  (typo'd `GateOvrride` → 400, probe green, source restored). A committed one-line
  test would be nice in a later round; the behavior is real.
- `submit_review` now requires authentication (added `AuthenticatedAgent`
  extractor) — a tightening consistent with every other mutation route; existing
  callers (CLI/agents) authenticate, and all mechanical route/ABAC checks pass.

No fake implementations, missing enforcement, unsafe scope handling, or
self-confirming tests found. The e2e JWT tests would fail if the writer/reader key
contract, the write paths, or the filtering regressed.
