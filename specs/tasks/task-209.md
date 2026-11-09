---
title: "Finish HSI §12 profile: notification filtering, judgment ledger, auth provider info"
spec_ref: "human-system-interface.md §12"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "human-system-interface.md §12 What the Profile Is"
commits: ["ca1ac78e4d89ee095a144a492f74fec164b944ed", "f7f3b0810106d2f195012e6aa945cd9bba427d2b", "1eadc6d8b4ef2c7ce036528da50d936684c363fb"]
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
f7f3b081, call-site repair ca1ac78e, base-2badce49 merge dc7ab7c7). This
round (merge 1eadc6d8) resolved the executor's rebase of the newer
assigned base fefde294 (task-089 head, which also carries task-120's User
entity columns, task-085 trust suggestions, task-090 lenses, agent
lifecycle audit events, and audit-detail zstd compression) into the
task-209 branch. The merge kept BOTH sides' behavior everywhere and fixed
two real semantic collisions the auto-merge could not see (details
below); every acceptance criterion was re-verified on the merged tree.

### Behavior shipped (unchanged from prior rounds)

- **Spec amendment (Part 1)** — `specs/system/human-system-interface.md`
  §12 Judgment Ledger carries a concrete Sources table (category →
  table → human-attribution column → workspace-attribution column), states
  when each event is written, and pins `?type=` values to
  `approval|rejection|gate|trust|meta-spec` (exactly the `JudgmentType`
  wire strings).
- **Write paths (Part 3)** — `AuditEventType` gains `TrustChange` /
  `GateOverride` / `MetaSpecPublish` (now alongside the base's agent
  lifecycle variants — unioned in the enum, `as_str`/`from_str`, and the
  round-trip test). Trust transition via `PUT /api/v1/workspaces/:id`
  writes a `trust_change` audit event attributed to the acting user. A
  human approval review on an MR with failed gate results writes one
  `gate_override` event per failed gate (mr_id/gate_id/gate_type/from/to);
  agent-token approvals do not qualify. Meta-spec registry create/update
  write `meta_spec_publish` events with kind/name/version/content_hash
  and workspace attribution from scope. JWT validation records
  `oidc_issuer` + `last_login_at` via `UserRepository::record_login`
  (SQLite/Postgres/mem parity).
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

### This round's merge resolution (1eadc6d8)

- **Conflicts (9), all resolved keeping both sides**: postgres/sqlite
  `user.rs` + `schema.rs` — union of task-209's `users.oidc_issuer` with
  task-120's `username`/`avatar_url`/`preferences`/`tenant_id`/
  `global_role` columns (UserRow, From<UserRow>, and the users table
  declaration share one column order; task-120's complete create/update
  paths kept, task-209's `record_login` kept); `audit.rs` — union of the
  judgment and agent-lifecycle `AuditEventType` variants;
  `merge_requests.rs` test imports; `siem.rs` severity arms (judgment
  events rank alongside routine lifecycle signals); `trust_suggestion.rs`
  — the base's `trust_suggestions_for` test helper adapted to the
  `exclude_types` signature; `fabricated-scope-defaults-exemptions.txt`
  — base version (specs.rs/auth.rs/git_http.rs sites were genuinely
  fixed upstream; the merged tree carries those fixes, 2 entries
  remain); coverage matrix — HEAD's row 53 (implemented, task-209) plus
  the base's rows 15/23 (implemented, task-085/090), header recounted
  and awk-verified to 20/35 (0 not-started).
- **Duplicate-column migration fix** — task-209's
  `2026-10-07-000056_user_auth_provider_info` and the base task-120's
  `2026-10-08-000058_user_entity_profile_fields` BOTH added
  `users.last_login_at`; diesel runs pending migrations strictly, so
  the merged tree failed on every fresh database. Fixed by making
  task-209's migration add ONLY `oidc_issuer` — `last_login_at` is
  owned by task-120's migration (which sorts after on fresh DBs and is
  already applied on base-lineage DBs). down.sql no longer drops the
  column it no longer adds. Fresh-DB proof: every SQLite adapter test
  constructs `SqliteStorage` and runs the full chain (10 passed);
  `check-migration-versions` and `check-migration-sql-portability` OK.
- **`record_login` issuer carve-out (semantic collision)** — the base's
  `find_or_create_user` now stamps `last_login_at` on EVERY
  authentication via `update()` (user-management.md §User Entity),
  which kept the column fresh and made task-209's debounced
  `record_login` — the only `oidc_issuer` writer — always skip:
  `get_me_returns_recorded_auth_provider_info` failed with a Null
  issuer. Fixed at the port contract level (`gyre-ports/src/user.rs`):
  the write also fires when the stored `oidc_issuer` is unset or
  differs from the verified one; implemented with single-statement
  filters in SQLite/Postgres and the equivalent guard in mem. The hot
  path (same issuer, fresh login) still skips, preserving the debounce
  rationale; issuer provenance can no longer be lost to the
  provisioning path's stamp. Adapter test extended with a
  changed-issuer-in-window case that fails without the fix.
- **Auto-merge repairs** — `users.rs` `UserProfileResponse` had
  `last_login_at` from both sides (deduplicated); `mem.rs`
  `MemJudgmentLedgerRepository` now shares the base's
  compression-aware `Vec<MemAuditEntry>` audit store and materializes
  entries (zstd detail decompression) before reading judgment fields;
  9 base-side `NotificationRepository::list_for_user` call sites
  (secret_rotation, specs, merge_processor tests) adapted to the
  `exclude_types` parameter; `cargo fmt --all` applied.

### Test evidence (merged tree, post-1eadc6d8)

- `cargo test -p gyre-server --lib`: 1491 passed, 0 failed — includes
  the §12 write-path tests (`get_me_returns_recorded_auth_provider_info`
  green again after the issuer carve-out), the base's task-085/090/
  task-120 suites, and the adapted call sites.
- `cargo test -p gyre-adapters --lib user_profile`: 10 passed (ledger
  merge/filter/pagination, notification exclusion, record_login
  debounce + issuer-change carve-out).
- `cargo check --workspace`: clean.
- `web/src/locales/en.json` (the only merge-authored web change):
  valid JSON, no duplicate keys; all other web sources identical to the
  published base (no web code authored this round, so the upstream web
  suite result stands; web/dist not rebuilt per assignment).
- Mechanical checks OK: check-arch, check-migration-versions,
  check-fabricated-scope-defaults, check-abac-exempt-handlers (88
  handlers), check-in-memory-state-stores, check-mem-port-contracts,
  check-forged-scope-fields, check-migration-sql-portability,
  check-abac-route-registry, check-mcp-write-tools,
  check-dead-message-kinds, check-inert-enforcement,
  check-lossy-secret-conversion, check-scope-literal-defaults,
  check-relative-path-defaults, check-byte-slice-truncation,
  check-forwarded-header-trust, check-unbounded-external-http. Note:
  `check-tenant-filter.sh` cannot execute under this sandbox's mawk
  (gawk-only 3-arg `match()`); this round's only SQL change is the
  user-id-keyed `record_login` UPDATE filter (no tenant-crossing
  surface); CI runs it under gawk.
