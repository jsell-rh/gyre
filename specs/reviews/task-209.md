# Review — task-209 (Finish HSI §12 profile: notification filtering, judgment ledger, auth provider info)

Spec: `specs/system/human-system-interface.md` §12 "What the Profile Is" (lines ~1394-1476).
Candidate under review: `df1d05cd3e5f50a312b8c68adb509f3294a379c6` (branch
`pipeline/task-209/e9162ac84f614861a7b7fcb75ed2de09-1`), diffed against assigned base
`9fb371ac1b527256496cc05c740ea5bf85ae435c` (66 files, ~4.9k insertions; the branch also
contains merge/process rounds re-merging the recovered checkpoint onto the current base,
including the task-111 session-management merge).
Verdict: **approved** — no blocking findings. All acceptance criteria independently verified
or compiler/type-check verified with the remaining server-test execution recorded for the
verifier below.

## Probes run (evidence: /tmp/stage/review-evidence/)

- `cargo test -p gyre-domain --lib` → **536 passed, 0 failed** (incl. `AuditEventType`
  round-trip + named-variant-parse guards for `trust_change`/`gate_override`/`meta_spec_publish`,
  `JudgmentType` round-trip, `SpecApproval` Approved→Rejected transition tests).
- `cargo test -p gyre-adapters --lib` → **551 passed, 6 ignored, 0 failed** — including all
  judgment-ledger aggregation tests (`judgment_ledger_merges_sources_reverse_chron`,
  `judgment_ledger_workspace_filter_drops_unattributed`,
  `judgment_ledger_type_filter_selects_source`,
  `judgment_ledger_pagination_across_merged_stream`,
  `judgment_ledger_rejection_attributed_to_withdrawer`,
  `judgment_ledger_decompresses_compressed_audit_details`),
  `notifications_exclude_types_applies_to_list_and_count`, `record_login_debounce_and_update`
  (SQLite, real tables).
- `SKIP_WEB_BUILD=1 cargo check -p gyre-server --lib --tests` → clean at the exact candidate
  (the `graph_extraction.rs` arity miss found in the prior round is fixed in `8e295c2e`, an
  ancestor).
- `cd web && npm ci && npm test` → **116 files, 2371 passed, 41 skipped, 0 failed**.
- Script lints: `check-arch.sh`, `check-migration-versions.sh` (new `2026-10-07-000056`
  maps to unique Diesel version `20261007000056`; SQL is SQLite/PG-portable),
  `check-notification-toggle-coverage.sh` (new, wired into pre-commit + CI),
  `check-abac-route-registry.sh`, `check-abac-exempt-handlers.sh` (86 handlers),
  `check-mcp-write-tools.sh`, `check-mem-port-contracts.sh`, `check-inert-enforcement.sh`,
  `check-forged-scope-fields.sh`, `check-in-memory-state-stores.sh` → all OK.
- `check-tenant-filter.sh` fails in this sandbox with a **mawk** syntax error at its gawk-only
  3-arg `match()` — pre-existing environment incompatibility, script unchanged by this
  candidate (CI uses gawk). Manually confirmed the new queries are user-scoped
  (`user_id =` caller) exactly like the sibling `/users/me/*` reads; `audit_events` has
  pre-existing `user_id`/`workspace_id` columns used for scoping. Not a candidate defect.

## Verified against the acceptance criteria

1. **Spec amendment** — HSI §12 now carries a concrete Sources table (approval/rejection ←
   `spec_approvals` LEFT JOIN `spec_ledger_entries` on path; gate/trust/meta-spec ←
   `audit_events` rows with `event_type IN (gate_override, trust_change, meta_spec_publish)`,
   human attribution via `user_id`, workspace via the `workspace_id` column), `?type=` values
   are exactly the `JudgmentType` wire strings (`approval|rejection|gate|trust|meta-spec`),
   inbox-filter semantics (in-query, both list and badge, MCP parity, background de-dup exempt)
   and PUT-validation requirement are specified. The amendment also honestly documents that a
   gate override is recorded on human approval past failed gates without mutating the gate row.
2. **`JudgmentType::GateOverride`** — `as_str() = "gate"`, `from_db_str("gate")` parses;
   round-trip tested in gyre-domain.
3. **`list_for_user` aggregation** — SQLite/Postgres/mem parity. All four categories from real
   tables; `?workspace_id=` FILTERS in SQL (LEFT JOIN drops NULL workspace rows — verified by
   `judgment_ledger_workspace_filter_drops_unattributed`, which asserts the previously-stamped
   behavior is gone); reverse-chronological with deterministic `entity_ref` tie-break;
   limit/offset applied after the merged sort (verified by
   `judgment_ledger_pagination_across_merged_stream`). Audit rows are materialized through the
   shared compression-aware `AuditEventRow` path, and
   `judgment_ledger_decompresses_compressed_audit_details` proves detail survives the
   log-compression job. Rejection attribution (the declining human, not the original approver)
   is implemented in all three adapters and tested end-to-end via real JWT approve/reject/revoke
   in `specs.rs` (`*_surfaces_in_judgment_ledger`).
4. **Trust-change write path** — `PUT /workspaces/:id` writes a `trust_change` audit event with
   `from`/`to`/`workspace_id`, attributed to `auth.user_id`, only on an actual transition
   (same-value PUT writes nothing); record failure propagates. Test
   `trust_transition_writes_trust_change_audit_event` drives the real router with a real signed
   JWT and queries the audit repo.
5. **Gate-override write path** — human approval review (`decision == Approved &&
   auth.user_id.is_some()`) on an MR with failed gate results writes one `gate_override` event
   per failed gate carrying `mr_id`/`gate_id`/`gate_type`/`from_status`/`to_status`; agent-token
   approvals excluded. Test `human_approval_on_failed_gate_writes_gate_override_event` proves
   both the negative (agent token) and positive (JWT human) paths plus the `?type=gate` ledger
   round trip.
6. **Meta-spec publish write path** — registry create/update write `meta_spec_publish` events
   with `kind`/`name`/`version`/`content_hash` and workspace attribution from the meta-spec's
   own scope (Global/Tenant carry none, matching the `?workspace_id=` exclusion semantics);
   test `publish_writes_meta_spec_publish_audit_event`. The registry DELETE handler
   additionally gained real per-handler scoped authorization (Global→Admin, Tenant→scope-tenant
   Admin, Workspace→tenant-contained Owner/Admin), replacing a flat role check — hardening, in
   scope for the touched handler.
7. **Inbox filtering** — `exclude_types` added to the port contract and applied in-query before
   pagination in `list_for_user` AND `count_unresolved` (SQLite, Postgres, mem); the inbox and
   badge handlers load disabled types from `user_notification_preferences` (no row = enabled);
   the MCP `notifications://` tool applies the same exclusion; background de-dup callers pass
   `&[]` per the amended spec. PUT rejects unknown `notification_type` with 400 and normalizes
   legacy parse aliases to canonical `NotificationType::as_str()` names (dead-toggle class
   guarded by `preference_alias_normalizes_to_canonical_before_storing`).
8. **Auth provider info** — `users.oidc_issuer` added by portable migration; JWT validation
   records issuer + `last_login_at` via a single-statement debounced
   `UserRepository::record_login` (issuer-change carve-out so provenance is never lost;
   mem/sqlite/postgres parity; port contract documented); `GET /users/me` returns both
   read-only (`UpdateProfileRequest` has no field for either); UI renders them.
9. **`?type=` hardening** — unknown judgment type now 400s instead of silently unfiltering
   (consistent with sibling params); canonical acceptance tested.
10. **Frontend** — `notification-types.js` is the canonical toggle list kept in lockstep with
    the Rust enum by the new `check-notification-toggle-coverage.sh` (41/41 at probe time);
    UserProfile consumes the real wire shapes, server-side filters/pagination, badge from the
    count endpoint; tests derive the expected enum set from the Rust source itself
    (anti-mirrored-logic).

## Tests the independent verifier must execute

Per review-harness budget rules the gyre-server test binaries were not compiled here
(`cargo check -p gyre-server --lib --tests` is clean). The verifier must run:

- `cargo test -p gyre-server --lib` — at minimum:
  `api::users::tests::{disabled_preference_excludes_type_from_inbox_and_count,
  preference_alias_normalizes_to_canonical_before_storing,
  get_me_returns_recorded_auth_provider_info, judgments_unknown_type_returns_400}`,
  `api::workspaces::tests::trust_transition_writes_trust_change_audit_event`,
  `api::merge_requests::tests::human_approval_on_failed_gate_writes_gate_override_event`,
  `api::meta_specs::registry_tests::publish_writes_meta_spec_publish_audit_event`,
  `api::specs::tests::{approve,revoke,reject}_via_jwt_surfaces_*_in_judgment_ledger`.
- `cargo test --all` and `cd web && npm test` (web suite independently run in this review: pass).

## Non-blocking observations

- **Generated output**: `web/dist/` was not rebuilt in this checkpoint — owned by the
  deterministic verifier per standing instructions; not reviewed, not blocking.
- The working tree carries a local (uncommitted, harness-staged) revert of
  `specs/tasks/task-209.md` to the assigned task body; the committed candidate itself has
  `progress: ready-for-review` and a fully populated `commits:` attribution list. No action.
- `check-tenant-filter.sh` cannot run under this sandbox's mawk (gawk-only `match()`); rerun
  under gawk in CI. The underlying tenant-scoping concern was checked manually (see probes).
