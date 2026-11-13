# Review — task-209 (Finish HSI §12 profile: notification filtering, judgment ledger, auth provider info)

Spec: `specs/system/human-system-interface.md` §12 "What the Profile Is" (row 53).
Candidate under review: `7283db37ab3319c2474ca9d7ba55c3ae56981151` (base `4ec1e526`).
Verdict: **approved** — no blocking findings. Two non-blocking observations recorded at the end.

## Probes run (evidence: /tmp/stage/review-evidence/)

Executed locally against the candidate tree (`/tmp/stage/bin/cargo`, review-stage wrapper;
workspace-wide builds deferred to the verifier per the 4 GiB policy):

- `cargo test -p gyre-adapters --lib judgment` → **6/6 passed** against real SQLite:
  merges all four categories reverse-chronologically across sources; `?workspace_id=`
  filters (unattributed entries dropped, not stamped); `?type=gate` selects only gate
  overrides; limit/offset applied to the merged sorted stream; rejection attributed to
  the withdrawing human, not the original approver; zstd-compressed `audit_events.detail`
  decompressed transparently (real `compress_details_before` pass, 4 rows).
- `cargo test -p gyre-adapters --lib notification` → **8/8 passed**, including
  `notifications_exclude_types_applies_to_list_and_count` (exclusion in-query for both
  list and badge count).
- `cargo test -p gyre-adapters --lib record_login` → 1/1 passed (debounce window holds;
  issuer-change carve-out fires inside the window).
- `cargo test -p gyre-domain` → 503 + 8 passed (incl. `AuditEventType` round-trip with an
  explicit not-`Custom` assertion for the three new judgment event types).
- `cargo check -p gyre-server` and `cargo check -p gyre-server --tests` → exit 0: the
  lib and **every test target type-checks**, including the CI-repaired
  `specs_assist.rs` `list_for_user` 8-arg call sites (the E0061 from CI run 43395891760,
  fixed in `a640c67f`, which is in the candidate).
- `cd web && npm ci && npm test` → **111 files, 2255 passed / 41 skipped**.
- Mechanical gates at the candidate commit: `check-notification-toggle-coverage.sh`
  (41 enum variants ↔ 41 toggles ↔ labels), `check-migration-versions.sh`,
  `check-migration-sql-portability.sh` (plain `ALTER TABLE ... ADD COLUMN`, both
  backends), `check-arch.sh`, `check-abac-route-registry.sh`, `check-abac-exempt-handlers.sh`
  (75 handlers), `check-inert-enforcement.sh`, `check-mem-port-contracts.sh`,
  `check-forged-scope-fields.sh`, `check-rustfmt-diff.py` (42 Rust files clean),
  `check-task-commit-attribution.sh` (passes **at the candidate commit**; the working-tree
  failure is the stage's pristine task-file overlay, see observation 4).
- `cargo clippy -p gyre-adapters` / `-p gyre-domain -p gyre-ports`: warnings exist but
  none on candidate-changed lines (the two warnings landing in touched files —
  `audit.rs:98` `cnt: i64` and `audit.rs:180` `AuditEvent::new` arity — sit on unchanged
  pre-existing lines outside every `+` hunk, so the diff-scoped CI gate is unaffected).

Deferred to the independent verifier (recorded, not runnable here under the wrapper):
`SKIP_WEB_BUILD=1 cargo test --all`; the focused server tests
`trust_transition_writes_trust_change_audit_event`,
`human_approval_on_failed_gate_writes_gate_override_event`,
`publish_writes_meta_spec_publish_audit_event`,
`disabled_preference_excludes_type_from_inbox_and_count`,
`preference_alias_normalizes_to_canonical_before_storing`,
`get_me_returns_recorded_auth_provider_info`,
`{approve,revoke,reject}_via_jwt_surfaces_in_judgment_ledger`,
`specs_assist::tests::save_spec_notifies_team_members_from_manifest_approvers`;
and `cargo clippy --all-targets --all-features -- -W clippy::all` +
`python3 scripts/check-clippy-diff.py <merge-ref base>`.

## Verified working

- **Judgment ledger aggregation (AC 1–3).** `JudgmentType::GateOverride` added with
  `as_str() = "gate"` and `from_db_str("gate")` parsing; wire strings are exactly
  `approval|rejection|gate|trust|meta-spec` and the HSI §12 amendment states them.
  SQLite `list_for_user` (executed) queries `spec_approvals` LEFT JOIN
  `spec_ledger_entries` (workspace filter in SQL, NULL dropped) plus `audit_events`
  (`event_type IN (gate_override, trust_change, meta_spec_publish)`, `user_id`
  attribution, `workspace_id` column filter), merges, sorts reverse-chronologically with
  a deterministic `entity_ref` tie-break, and paginates after the merge. Postgres mirrors
  it structurally; the mem adapter re-implements the same semantics over the *shared*
  audit/approval/ledger stores (wired via `with_sources` in both `mem::test_state_inner`
  and `lib.rs build_state`), so mem-backed tests exercise the same data the writers use.
- **Write paths (AC 4–6).** Trust transition (`PUT /workspaces/:id`) writes a
  `trust_change` event only when the level actually changed, after
  `apply_trust_transition` succeeds, with from/to/workspace_id; errors propagate.
  Human approval review on an MR with failed gate results writes one `gate_override`
  event per FAILED gate (mr_id/gate_id/gate_type/from→to), gated on
  `decision == Approved && auth.user_id.is_some()` — agent-token approvals excluded.
  Meta-spec registry create AND update write `meta_spec_publish` events with
  kind/name/version/content_hash and workspace attribution derived from the meta-spec's
  own scope (Workspace only; Global/Tenant deliberately unattributed). All three use the
  shared `AuditEvent` domain type and real `AuditRepository::record`.
- **Attribution-key integrity.** The approve/reject/revoke endpoints now write the
  caller's canonical user Id into `spec_approvals.approver_id`/`rejected_by`/`revoked_by`
  (the `user:<display name>` KeyBinding label stays on the history event), and the ledger
  reader matches that same key — the writer/reader mismatch class that previously made
  human approvals invisible is closed and covered by end-to-end JWT tests on both sides.
  The revocation caller-label derivation (`agent:` only for `scope=agent` JWTs) fixes a
  real 403 lockout for human approvers without weakening the admin override.
- **Notification filtering (AC 7).** `NotificationRepository::list_for_user` and
  `count_unresolved` take `exclude_types`, applied by the SQL `ne_all` filter **before**
  limit/offset (pg + sqlite parity), and by the mem filter. Inbox list, badge count, and
  the MCP `notifications://` resource all load disabled types from
  `user_notification_preferences`; background de-dup callers pass `&[]` per the port
  doc. `PUT .../notification-preferences` normalizes legacy parse aliases to canonical
  `as_str()` names before storing and 400s on unknown types (an inert-toggle guard).
  The new `scripts/check-notification-toggle-coverage.sh` (wired into CI + pre-commit)
  mechanically keeps the UI toggle list and locale labels in lockstep with the Rust enum
  — 41/41/41 today.
- **Auth provider info (AC 8).** Migration `2026-10-07-000056` adds `users.oidc_issuer`
  (portable SQL; `last_login_at` ownership delegated to task-120's 000058 to avoid a
  duplicate-column collision — sound). `UserRepository::record_login` is a new port
  method implemented in sqlite/postgres/mem parity: single-statement conditional update
  (no read-modify-write race) with a 60 s debounce and an issuer-change carve-out;
  `validate_jwt` calls it with the *verified* issuer; failures warn without failing
  auth. `GET /users/me` returns `oidc_issuer`/`last_login_at` read-only;
  `UpdateProfileRequest` has no field for either and `update()` does not write the
  column, so they cannot be client-set.
- **Spec/coverage discipline.** The HSI §12 amendment precedes the implementation
  contract: it names the concrete source tables, human-attribution keys, workspace
  attribution, the `?type=` wire strings, the filtering (not stamping) semantics of
  `?workspace_id=`, and the gate-override recording rule. Coverage row 53 flipped to
  `implemented` with landing evidence; no `not-started` rows remain in the file.
- **No route/ABAC/MCP-gate drift.** No routes added or removed (`api/mod.rs` unchanged);
  ABAC registry and exempt-handler checks pass; the MCP change is a read-only resource
  handler (no `needs_write` implications).

Test-inflation check: none found. The new tests kill real bugs — the adapter suite
catches workspace-stamping, wrong-source type filters, compression-blind reads and
attribution swaps; the server tests drive the real router with real JWTs and assert
both the written audit rows and the ledger's read-side shape; the web tests assert the
server wire format and the full toggle census (`NOTIF_TYPE_IDS` compared against the
Rust enum variants).

## Non-blocking observations (recorded, no repair required)

1. **Legacy `spec_approvals` rows are invisible to the ledger.** Rows written by
   pre-task-209 writers carry `user:<display name>` labels in `approver_id`; the reader
   matches the canonical user Id only, so those historical approvals do not surface in a
   human's ledger. This is the documented design decision in the amended spec (system/
   agent-token rows are intentionally not human ledger subjects) and no data is lost,
   but a deployment with label-keyed history will show an empty approval history until
   new approvals land. A future backfill (label → canonical Id via `users.display_name`)
   could close it if it ever matters.
2. **Stale phrase in `specs/tasks/task-209.md` "Shipped" notes.** It lists
   "`meta_spec_versions` joined to `meta_specs`" as a ledger source; the implementation
   (and the amended HSI §12 source table) uses `audit_events` (`meta_spec_publish`)
   exclusively. Documentation-only inaccuracy in the task file; the spec and coverage
   matrix are accurate.
3. **`scripts/check-tenant-filter.sh` cannot run under mawk** (gawk-only `awk` syntax);
   pre-existing script, unchanged by this candidate — an environment restriction for
   remote verification, not a code defect. The candidate's new `audit_events` queries
   are workspace-filtered via the `workspace_id` column and `user_id` equality, which is
   the containment the script exists to encourage.
4. **The review checkout's working tree carries the stage's pristine
   `specs/tasks/task-209.md` overlay** (progress `not-started`, `commits: []`), which
   makes `check-task-commit-attribution.sh` fail *in the working tree*. Verified in a
   temp worktree at the candidate commit: the check passes — all 13 task-labeled
   product-surface commits are recorded in the candidate's frontmatter.
