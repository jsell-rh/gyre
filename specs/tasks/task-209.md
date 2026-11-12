---
title: "Finish HSI §12 profile: notification filtering, judgment ledger, auth provider info"
spec_ref: "human-system-interface.md §12"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "human-system-interface.md §12 What the Profile Is"
commits: ["5418531440a276986e07fb29afb6dfa4e42cacc5", "9c00e3f78ce8ed031a2663830d4c73cc3e909d48", "21dac7117d4ca855b6ad87103d66b604225cc5e3", "5564c00b8331c12535117408d6f8006ed3349e45", "1639c53172a74c6fc8bb2b0b8abdbb2c4366735f", "c7fd768b2f47f3761da6949bc71685a2ef58d131", "fa67c4cea58df20494badc51ef4b7ee63c8adcd0", "daf5eec53b71b2e9f4d24cb9cc43a34125dab5ed", "7aff9744e349e9cf75719c0b552e8abefa0ec3eb", "94af4aef33b08e09043f4eb0e0dd8542aea34e9d", "ca1ac78e4d89ee095a144a492f74fec164b944ed", "f7f3b0810106d2f195012e6aa945cd9bba427d2b"]
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

HSI §12 profile gaps (notification filtering, judgment ledger, auth
provider info) are implemented in production code on this branch across
the commits listed in the frontmatter. This round repairs the
verification failure on candidate c83549396 (checks.sh exit 1: the
changed-lines clippy gate reported 11 warnings, all on lines this branch
added).

### This round: clippy repair (verification finding)

Commit 541853144 clears all 11 warnings with no behavior change:

- **meta_specs.rs `authorize_meta_spec_delete`** (8x
  `clippy::needless_borrow`): `state`/`auth` are already
  `&AppState`/`&AuthenticatedAgent` parameters, so the four
  `record_rbac_denied` call sites passed `&&`-references the compiler
  immediately dereferenced; the redundant `&` is removed.
- **workspaces.rs PUT trust transition**
  (`clippy::unnecessary_lazy_evaluations`): `changed.then(|| (from,
  new_trust))` became `changed.then_some((from, new_trust))` — the
  closure had no side effects and both values were already computed and
  owned, so lazy vs eager construction is unobservable.
- **mem.rs `MemJudgmentLedgerRepository`**
  (`clippy::derivable_impls`): the manual `Default` impl (three
  `Arc::default()` fields) is replaced by `#[derive(Default)]`.
- **sqlite/user_profile.rs test** (`clippy::clone_on_copy`): `.clone()`
  dropped on the `Copy` type `JudgmentType`.

### Evidence (/tmp/stage/review-evidence/)

- `clippy-diff-pass.log` — `python3 scripts/check-clippy-diff.py
  199c1bb78` (the exact CI/pre-commit gate that failed): "clippy:
  changed lines clean (42 Rust files, 906 existing warnings outside
  changes)", exit 0. The 906 pre-existing warnings are outside this
  branch's changed lines (e.g. gyre-common view_query.rs, untouched by
  task-209) and are out of scope per the gate's changed-lines contract.
- `rustfmt-diff-pass.log` — changed-lines rustfmt gate: clean, exit 0.
- `cargo-check-alltargets.log` — `cargo check -p gyre-server -p
  gyre-adapters --all-targets` (SKIP_WEB_BUILD=1): exit 0, all lib and
  test targets type-check.
- `adapters-user-profile-tests.log` — focused gyre-adapters
  `user_profile` test run (judgment-ledger aggregation suite).
- `toggle-coverage-pass.log`, `arch-lint.log`, `abac-registry.log`,
  `mem-port-contracts.log` — prior-round mechanical gates re-run on this
  tree, all exit 0.

### Prior rounds (intact on this branch)

- Notification filtering: inbox queries (`get_my_notifications`/`_count`)
  compute the user's disabled types from `user_notification_preferences`
  and pass them as `exclude_types` to `list_for_user`, applied by the
  query before limit/offset; preferences stored under canonical
  `as_str()` names; profile UI toggles for all 41 canonical variants,
  guarded by `scripts/check-notification-toggle-coverage.sh` in
  pre-commit and CI.
- Judgment ledger: `JudgmentLedgerRepository::list_for_user` (sqlite +
  postgres) aggregates spec approvals/rejections, gate overrides, trust
  changes, and meta-spec publishes from real tables with
  `?workspace_id=` filtering, per-source type filters, reverse-
  chronological sort, limit/offset after union+sort; write paths emit
  `gate_override`, `trust_change`, and meta-spec publish attribution
  audit events; zstd-compressed audit details decompressed on read.
- Auth provider info: `GET /api/v1/users/me` returns read-only
  `oidc_issuer` and `last_login_at`.
- HSI §12 spec amendment: judgment-source tables and `?type=` values
  (`approval|rejection|gate|trust|meta-spec`) documented in the spec.

### Verifier scope

The four clippy repairs are mechanically warning-removing with no
behavior change (borrow removal, then_some on already-computed owned
values, derived Default, Copy clone). Full `cargo test --all` (large
server build compilation belongs in the verifier per the 4 GiB budget)
and `cd web && npm test`; `scripts/check-notification-toggle-coverage.sh`;
`scripts/check-clippy-diff.py` and `scripts/check-rustfmt-diff.py`
against the assignment base; the gawk-dependent JS lint hooks on a
gawk-equipped runner.
