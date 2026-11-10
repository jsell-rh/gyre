---
title: "Finish HSI §12 profile: notification filtering, judgment ledger, auth provider info"
spec_ref: "human-system-interface.md §12"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "human-system-interface.md §12 What the Profile Is"
commits: ["94af4aef33b08e09043f4eb0e0dd8542aea34e9d", "ca1ac78e4d89ee095a144a492f74fec164b944ed", "f7f3b0810106d2f195012e6aa945cd9bba427d2b", "afd28f1710b0c910109bd684c9acc7a1760c3144"]
---

## Spec Excerpt

From `specs/system/human-system-interface.md` §12 (lines 1394-1406, 1413-1421):

> **Notification Preferences**
> - Per-notification-type toggles (enable/disable each of the 10 `NotificationType` variants)
> - ...
> - The Inbox query filters out disabled notification types before returning results. Default: all enabled.

> **Judgment Ledger**
> - Chronological log of the human's judgment decisions across all workspaces:
>   - Spec approvals and rejections (with spec path, timestamp, workspace)
>   - Gate approval overrides (with MR reference, gate type)
>   - Trust level changes (from → to, workspace)
>   - Meta-spec edits published (persona/principle/standard, workspace)
> - ...
> - Endpoint: `GET /api/v1/users/me/judgments` — returns a paginated, reverse-chronological list **aggregated from existing tables (spec_approvals, gate overrides, workspace audit log, meta-spec commit history)**. Query params: `?workspace_id=`, `?type=` (approval/gate/trust/meta-spec), `?since=`, `?limit=`, `?offset=`

> **Identity & Access**
> - Display name, avatar, email, timezone, locale (editable via `PUT /api/v1/users/me`)
> - API tokens for CLI and MCP access (...)
> - **Auth provider info (OIDC issuer, last login — read-only)**

## Implementation Plan

Three independent sub-parts, verified against code on 2026-09-30. Part 1 (spec amendment) must land before Part 3 (data write path).

### Part 1 — Amend the spec's judgment-source table list

The spec names "gate overrides, workspace audit log, meta-spec commit history" as the aggregate sources, but two problems exist:

1. **No gate-override write path exists.** Gate failures resolve via close/retry (per `ui-journeys.md:137`); no `gate approval override` record is written anywhere today (grep for gate override across `crates/` finds only gate-level `otlp_port` override and trust-preset policy naming). HSI §1416 requires "Gate approval overrides (with MR reference, gate type)" — a record must first exist before a ledger can aggregate it.
2. **The judgment-source table list is stale/incomplete.** `meta_spec_versions` has no `workspace_id` column, so it cannot be filtered by `?workspace_id=` as specced, and `audit_events` is agent-oriented (`agent_id`, `event_type`, `path`, `details` JSON) with no user attribution column.

Amend `specs/system/human-system-interface.md` §12 Judgment Ledger (around line 1420) to specify the real, implementable aggregation sources:

- **Spec approvals/rejections** — `spec_approvals` table (as today; keep `?workspace_id=` filtering via `spec_approvals.workspace_id` if that column exists — verify).
- **Gate approval overrides** — requires a new write path. Choose ONE:
  - (a) Add an `AuditRepository::record` call in the gate-override flow writing an `AuditEvent` with `event_type: "gate_override"`, `agent_id` = acting user's Id, `details: {mr_id, gate_type, from_status, to_status}` — reuses `audit_events` table; OR
  - (b) Add a dedicated `gate_overrides` table.
  - Option (a) is boring/safe: the table exists, the port exists, the write is one call. Prefer (a).
  - **Which flow writes it?** There is no UI/API for a human to override a failed gate today. The spec (§1416) presumes it exists. Amending HSI to reference a write path that doesn't exist would be fake-completion; do NOT create the write path in this task — the spec is the contract: either the amendment names a real path, or the write path must be built. **Decision: build the minimal write path** (see Part 3).
- **Trust level changes** — `workspaces.rs` update handler (PUT /api/v1/workspaces/:id) already performs trust transitions (`apply_trust_transition`, workspaces.rs:249-255) — write an `AuditEvent` there (`event_type: "trust_change"`, `agent_id` = acting user, `details: {from, to, workspace_id}`).
- **Meta-spec edits published** — `meta_spec_versions` rows created by the meta-spec publish flow — see Part 3 for where the write happens and how workspace attribution is obtained.

**Amendment text goes into the HSI spec §12, not just this task.** Insert into the Judgment Ledger bullet list a "Sources" note listing the four concrete aggregation sources:

- Spec approvals/rejections — `spec_approvals`
- Gate approval overrides — `audit_events` rows with `event_type: "gate_override"` (written by the gate-override handler, agent_id = acting user)
- Trust level changes — `audit_events` rows with `event_type: "trust_change"` (written by workspace trust-transition path)
- Meta-spec edits published — `meta_spec_versions` joined to `meta_specs` (workspace attribution via `meta_specs.scope`/`scope_id`)

Also fix the `?type=` param values in §1420: the spec says `(approval/gate/trust/meta-spec)` but `JudgmentType::from_db_str` currently accepts `approval/rejection/trust/meta-spec` — add `gate` to the domain enum and accept both spellings or amend the spec values to match. Do not break existing clients: accept `approval|rejection|gate|trust|meta-spec`.

### Part 2 — Implement the aggregation in the adapters

`crates/gyre-adapters/src/sqlite/user_profile.rs` `JudgmentLedgerRepository::list_for_user` (lines 236-330) currently queries ONLY `spec_approvals` and stamps (not filters) the workspace param. Extend:

1. **spec_approvals branch** (existing): add the `workspace_id` filter to the SQL `WHERE` clause instead of stamping entries with the param value. Verify `spec_approvals` has a `workspace_id` column (check `schema.rs`); if not, join through `repos`/`specs` as the approval flow does.
2. **gate overrides**: query `audit_events` where `event_type = 'gate_override'` and `agent_id = approver_id` (after Part 3 lands the writes). `JudgmentEntry::new(JudgmentType::GateOverride, entity_ref = MR title/ID, workspace_id from details, timestamp, detail)`.
3. **trust changes**: query `audit_events` where `event_type = 'trust_change'` and `agent_id = approver_id`. `JudgmentEntry::new(TrustGrant, entity_ref = workspace name, ...)`.
4. **meta-spec edits**: query `meta_spec_versions` joined with `meta_specs` where `meta_specs.created_by = approver_id` (verify the publish flow sets `created_by` to the human publisher — if it's always the system, switch to `meta_specs.approved_by` or the actual actor; check the meta-spec publish flow under `crates/gyre-server/src/api/` meta-spec endpoints).
5. Union all sources, sort by timestamp desc, apply limit/offset. Type filter applied per-source (as the existing approval/rejection filter logic does).
6. Add `GateOverride` variant to `JudgmentType` (domain `user_profile.rs:58-68`) with `as_str() = "gate"` and `from_db_str` accepting `"gate"`. Mirror in the postgres adapter (`crates/gyre-adapters/src/postgres/user_profile.rs`) if it implements the same trait.

### Part 3 — Write paths for the missing judgment events

Without these, the ledger aggregates nothing for 3 of 4 categories. Real queries against real state only.

1. **Trust changes**: in `crates/gyre-server/src/api/workspaces.rs` PUT handler where `trust_changed` is true (line 223-255), after `apply_trust_transition` succeeds, call `state.audit.record(AuditEvent { event_type: TrustChange, agent_id: acting user's Id, details: {from, to, workspace_id} })`. Use the existing `AuditEvent` domain type; add a `TrustChange` variant to `AuditEventType` if missing.
2. **Gate overrides**: this is the write path the spec presumes. Minimal real implementation: the MR review flow — the human MR-approval review (`ReviewDecision::Approved` submitted by a human user via the existing MR review API) that unblocks a previously-failed gate. Find the human review submission endpoint (reviews are stored via `state.reviews`; human review API — grep `reviews` routes in `mod.rs`). When a human submits an approval review on an MR that has a failed gate, write `AuditEvent { event_type: GateOverride, agent_id: user, details: {mr_id, gate_type, from_status: failed, to_status: passed} }`. The spec wants the override concept where a human approves past a failed gate; the human review path is the natural carrier.
   - Verify the failed-gate state query is feasible (`state.gates` or `gate_traces` table).
3. **Meta-spec publishes**: find the meta-spec publish/version-bump endpoint; write `AuditEvent { event_type: MetaSpecPublish, agent_id: publisher, details: {meta_spec_id, name, kind, workspace attribution} }` OR rely on `meta_spec_versions` join (Part 2 item 4). Prefer the audit write for user attribution; keep the join for workspace attribution.
4. Ensure `AuditEventType` has the needed variants (`GateOverride`, `TrustChange`, `MetaSpecPublish` — check existing `AuditEventType` enum in `gyre-domain`; extend with `as_str`/`from_str`).

### Tests

Per repo conventions (in-module tests, direct DB assertions):

- Adapter test: seed `spec_approvals` + `audit_events` (gate_override, trust_change) + `meta_spec_versions` + `meta_specs` rows; call `list_for_user`; assert 4 categories returned, reverse-chron, `?workspace_id=` actually filters, `?type=gate` returns only gate overrides.
- Write-path tests: trust transition via PUT /workspaces/:id writes an audit event; human approval review on failed-gate MR writes gate_override event.
- Notification-filtering test: disable a type in `user_notification_preferences`, call the inbox query, assert the disabled type is excluded.
- Endpoint tests: `GET /users/me/judgments?type=gate` shape.

## Acceptance Criteria

- [ ] HSI §12 Judgment Ledger bullet list includes the concrete source tables/columns, and `?type=` values match `JudgmentType` strings (`approval|rejection|gate|trust|meta-spec`).
- [ ] `JudgmentType` has a `GateOverride` variant, `as_str() = "gate"`, `from_db_str("gate")` parses.
- [ ] `list_for_user` aggregates all 4 categories from real tables; `?workspace_id=` filters (not stamps); reverse-chronological; limit/offset applied after union+sort.
- [ ] Trust transition via `PUT /api/v1/workspaces/:id` writes a `trust_change` audit event with from/to/workspace_id.
- [ ] Human approval review on a failed-gate MR writes a `gate_override` audit event with mr_id/gate_type/from/to.
- [ ] Meta-spec publish writes the attribution event (audit event or version row with correct created_by).
- [ ] Inbox notification query (`get_my_notifications` + count) excludes types disabled in `user_notification_preferences`.
- [ ] `GET /api/v1/users/me` returns `oidc_issuer` (read-only, from the user's most recent auth event or `users.oidc_issuer` if that column exists) and `last_login_at`.
- [ ] `cargo test --all` passes.
- [ ] `cd web && npm test` passes.

## Agent Instructions

Read `specs/system/human-system-interface.md` §12 lines 1381-1421. The three gaps are documented in `specs/coverage/system/human-system-interface.md` row 53 with file:line evidence — this task closes them.

Order: Part 1 (spec amendment) → Part 3 (write paths) → Part 2 (aggregation) → tests. The amendment defines the sources; the write paths make data exist; the aggregation reads it; the tests prove it.

Port trait: `crates/gyre-ports/src/user_profile.rs:25-34`. SQLite adapter: `crates/gyre-adapters/src/sqlite/user_profile.rs:236-330`. Postgres adapter: `crates/gyre-adapters/src/postgres/user_profile.rs` — check and mirror changes (repo policy: SQLite and Postgres adapters stay in parity).

Check `scripts/check-tenant-filter.sh` and `scripts/check-arch.sh` run clean (audit_events has no tenant/workspace column — if the tenant-filter script complains about new queries, the audit-event queries must be tenant-scoped via `details->workspace_id` JSON filtering or via joins, consistent with how other non-workspace tables are handled).

Key files: `crates/gyre-server/src/api/users.rs` (get_me, get_my_notifications, get_judgments), `crates/gyre-adapters/src/sqlite/user_profile.rs`, `crates/gyre-domain/src/user_profile.rs` (JudgmentType), `crates/gyre-server/src/api/workspaces.rs:223-255` (trust transitions), `crates/gyre-adapters/src/schema.rs:236-246` (audit_events), `:846-875` (meta_specs/meta_spec_versions).

Do not duplicate task-208's work (My Tasks/MRs/Agents removal — different files).

## Shipped

All three HSI §12 gaps are implemented in production code (checkpoint
f7f3b081, call-site repair ca1ac78e, base-77473763 merge 960cfb43,
review-repair 94af4aef on the base-fb1f82e31 merge 0cfd963, and the
requirements_changed remerge afd28f171 onto current base 078ec12).
Every acceptance criterion was re-verified on the final merged tree.

### Behavior shipped

- **Spec amendment (Part 1)** — `specs/system/human-system-interface.md`
  §12 Judgment Ledger carries a concrete Sources table (category →
  table → human-attribution column → workspace-attribution column), states
  when each event is written, pins `?type=` values to
  `approval|rejection|gate|trust|meta-spec` (exactly the `JudgmentType`
  wire strings), and — as of 94af4aef — specifies the attribution keys
  exactly: approvals attribute via `spec_approvals.approver_id` (canonical
  user Id), rejections/revocations via `rejected_by`/`revoked_by` (the
  declining human's canonical user Id), with the KeyBinding
  `user:<display name>` label explicitly named as NOT the ledger key.
- **Write paths (Part 3)** — `AuditEventType` has `TrustChange` /
  `GateOverride` / `MetaSpecPublish` variants. Trust transition via
  `PUT /api/v1/workspaces/:id` writes a `trust_change` audit event
  attributed to the acting user. A human approval review on an MR with
  failed gate results writes one `gate_override` event per failed gate
  (mr_id/gate_id/gate_type/from/to); agent-token approvals do not
  qualify. Meta-spec registry create/update write `meta_spec_publish`
  events with kind/name/version/content_hash and workspace attribution
  from scope. JWT validation records `oidc_issuer` + `last_login_at` via
  `UserRepository::record_login` (SQLite/Postgres/mem parity; fires when
  the stored issuer is unset or differs, so the provisioning path's
  last_login_at stamp cannot erase issuer provenance).
- **Aggregation + filtering (Part 2)** — `JudgmentType::GateOverride`
  (`"gate"`). `list_for_user` (SQLite/Postgres/mem parity) aggregates
  `spec_approvals` (LEFT JOIN spec_ledger_entries; `?workspace_id=`
  filters) and `audit_events` (gate_override/trust_change/
  meta_spec_publish, `user_id` attribution), merges, sorts
  reverse-chronologically (deterministic tie-break), and applies
  limit/offset to the merged stream only. Inbox:
  `NotificationRepository::list_for_user`/`count_unresolved` take
  `exclude_types` applied in-query before pagination; users.rs inbox +
  badge endpoints and the MCP `notifications://` tool load disabled
  types from `user_notification_preferences` (no row = enabled; PUT
  rejects unknown notification_type with 400). `GET /users/me` returns
  read-only `oidc_issuer`/`last_login_at`.

### Review repair 94af4aef (review 4d877e50: approval category inert —
writer/reader identity-key mismatch)

The production approval writers stored the KeyBinding label
(`"user:<display name>"`) into `spec_approvals.approver_id` while
`GET /users/me/judgments` filtered by the caller's canonical user Id,
so no production approval row ever matched and the approval/rejection
category surfaced nothing end-to-end; the adapter tests masked it by
seeding their own key shape.

- **Writer** — `approve_spec`/`reject_spec`/`revoke_spec_approval` now
  write the authenticated user's canonical Id into
  `spec_approvals.approver_id`/`rejected_by`/`revoked_by`. The
  `user:<name>` label stays on the `spec_approval_events` history rows
  where the KeyBinding contract requires it (the KeyBinding lookup and
  the revoke-authorization check still compare labels, unchanged).
  Agents and the global system token keep their `agent:<id>` /
  `user:<name>` labels (not human ledger subjects).
- **Reader** — all three `JudgmentLedgerRepository` impls attribute an
  approval to its approver and a rejection/revocation to the human who
  declined/withdrew it (`rejected_by`/`revoked_by`): a withdrawn
  approval surfaces in the withdrawer's ledger, not the original
  approver's (whose approval no longer stands — mutual exclusivity
  cleared `approved_at`).
- **Adjacent defect fixed** — `revoke_spec_approval` derived its caller
  label as `"agent:<name>"` for ANY JWT, but `approve_spec` derives the
  type from the JWT scope (Keycloak user JWTs are `"user:<name>"`), so
  the original human approver could never revoke their own approval
  (403; only Admins could). The derivation now mirrors approve's.
- **Merge repair** — one base-side `git_http.rs` (task-141) test call
  site adapted to `NotificationRepository::list_for_user`'s
  `exclude_types` parameter (same class as the earlier
  `secret_rotation.rs` call sites; the auto-merge missed it and it broke
  the test build).
- **Tests** — the e2e gap that let the mismatch ship green is closed:
  `approve_via_jwt_surfaces_in_judgment_ledger`,
  `revoke_via_jwt_surfaces_rejection_in_judgment_ledger`, and
  `reject_via_jwt_surfaces_rejection_in_judgment_ledger` drive the real
  endpoints with a real OIDC JWT through the full router and assert
  both the written ledger key (canonical user Id, not the label) and
  the surfaced `/users/me/judgments` entry. Adapter test
  `judgment_ledger_rejection_attributed_to_withdrawer` proves the
  withdrawal-attribution rule (withdrawer sees the rejection, original
  approver sees nothing); seed comments now state the key shape matches
  the production writer.

### Remerge afd28f171 (requirements_changed: current base 078ec12, task-123
notification entity taxonomy)

The retained task-209 checkpoint (d465d3f) was remerged onto current base
078ec12, which lands task-123's notification-entity work (mark_read /
read_at, 18-variant NotificationType with legacy-name parse aliases,
entity-source + action_url columns) alongside task-208's My-Stuff removal.
Eight conflicted files resolved so both feature sets compose:

- `gyre-domain/src/audit.rs` — both event-variant sets kept: task-209's
  TrustChange/GateOverride/MetaSpecPublish AND the base's source-control /
  access-control / impersonation variants (as_str/from_str/round-trip test
  merged for all).
- `sqlite/notification.rs`, `postgres/notification.rs`, `mem.rs` — the
  base's `mark_read` implementation kept AND task-209's
  `count_unresolved(user, ws, exclude_types)` signature kept (the merged
  port trait requires both; the exclusion filter stays in-query).
- `api/meta_specs.rs` — task-209's scope-aware per-handler DELETE
  authorization kept (Global → tenant Admin; Workspace → tenant
  containment + workspace Owner/Admin), with the base's task-104
  `record_rbac_denied` audit instrumentation re-added on every deny path
  (Global non-admin, cross-tenant, non-ws-admin, and the missing-row
  branch), so the stronger authorization did not silently drop RbacDenied
  events.
- `api/users.rs` tests — the disabled-preference test now seeds/asserts
  `SpecApprovalRequested` (the merged taxonomy's name; the legacy
  `SpecPendingApproval` string still parses via `NotificationType::parse`
  alias, so pre-unification rows keep loading).
- Base-side test call sites adapted to the 8-arg `list_for_user`
  (exclude_types) signature with `&[]` (background callers, no preference
  filtering): merge_processor.rs ×2, notifications.rs helper,
  personas.rs ×2, dep_staleness.rs ×3, meta_specs.rs ×2.
- `docs/api-reference.md` — users/me block merged: task-209's
  notification-preferences/judgments/tokens rows, the base's full
  18-variant notifications row + read/unread endpoints, My Agents/Tasks/
  MRs rows dropped (routes deleted by task-208 in the base).
- `specs/coverage/system/human-system-interface.md` — rows 53 (task-209)
  and 54 (task-208) both implemented; matrix 21/35; audit header records
  both landings.
- `web/src/__tests__/UserProfile.test.js` — object-wrapped
  `{notifications: [...]}`/`{judgments: [...]}` mocks kept (matches the
  real wire shape and UserProfile.svelte's tolerant unwrap); the
  myAgents/myTasks/myMrs mocks dropped with task-208's API-client removal.

### Test evidence (post-afd28f171, merged tree)

- `cargo test -p gyre-server --lib`: 1690 passed, 0 failed — includes
  the §12 write-path tests (`disabled_preference_excludes_type_from_
  inbox_and_count`, `get_me_returns_recorded_auth_provider_info`,
  `trust_transition_writes_trust_change_audit_event`,
  `human_approval_on_failed_gate_writes_gate_override_event`,
  `publish_writes_meta_spec_publish_audit_event`), the approve/reject/
  revoke e2e judgment tests, and the base's task-123 mark_read /
  entity-source suites.
- `cargo test -p gyre-server --tests`: all integration suites green
  (graph_integration, search_wiring, e2e bundles).
- `cargo test -p gyre-adapters`: 440 passed, 6 ignored, 0 failed
  (11-test user_profile suite: ledger merge/filter/pagination/withdrawal-
  attribution, notification exclusion, record_login debounce +
  issuer-change carve-out).
- `cargo test -p gyre-domain -p gyre-ports -p gyre-common -p gyre-cli`:
  all green (audit round-trip covers both variant sets).
- `cd web && npm ci && npm test`: 99 files, 2013 passed, 41 skipped,
  0 failed (UserProfile 26/26).
- `cargo check --all` clean (SKIP_WEB_BUILD=1); `cargo fmt` applied to
  the touched crates; web/dist left at the executor's staged state
  (generated assets are owned by the executor per assignment).
- Mechanical checks OK: check-arch, check-migration-versions,
  check-migration-sql-portability, check-mem-port-contracts,
  check-fabricated-scope-defaults, check-abac-exempt-handlers,
  check-in-memory-state-stores, check-forged-scope-fields,
  check-abac-route-registry, check-mcp-write-tools,
  check-dead-message-kinds, check-inert-enforcement,
  check-lossy-secret-conversion, check-scope-literal-defaults,
  check-relative-path-defaults, check-byte-slice-truncation,
  check-forwarded-header-trust, check-unbounded-external-http,
  check-fail-open-ref-resolution, check-task-commit-attribution.
  Note: `check-tenant-filter.sh` cannot execute under this sandbox's
  mawk (gawk-only syntax); it fails identically on both merge parents
  (verified: HEAD d465d3f and base 078ec12), so it is an environment
  restriction, not a regression — this round adds no new queries (only
  test call-site adaptations); CI runs it under gawk.

Evidence retained at /tmp/stage/review-evidence/task-209-remerge-078ec12.md
(commands, exit codes, and outputs; earlier controller artifact paths
/tmp/stage/review-evidence/task-2....md and
/tmp/stage/review-evidence/task-209-rebase-77473763.md remain valid for
their rounds).
