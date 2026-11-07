# Review — task-209 (Finish HSI §12 profile: notification filtering, judgment ledger, auth provider info)

Spec: `specs/system/human-system-interface.md` §12 (lines 1389–1432 post-amendment). Coverage row 53.
Candidate under review: `d6e4c4154` on `pipeline/task-209/ee39ada659cb4b45a805220fbc9718c7-1` (merge of the task-209 checkpoint lineage onto assigned base `2badce49d`). Diff scope: 41 files, +2838/−275.

Verdict: **complete** (approved).

## Probes run (this checkout, tree clean at `d6e4c4154`)

- `cargo test -p gyre-adapters --lib user_profile` → **10 passed, 0 failed** (ledger merge/reverse-chron, pagination across merged stream, type-filter source selection, workspace filter drops unattributed, notification exclusion list+count, record_login debounce, prefs/token roundtrips).
- `cargo test -p gyre-server --lib -- <six §12 tests>` → **6 passed, 0 failed**: `human_approval_on_failed_gate_writes_gate_override_event`, `trust_transition_writes_trust_change_audit_event`, `publish_writes_meta_spec_publish_audit_event`, `disabled_preference_excludes_type_from_inbox_and_count`, `get_me_returns_recorded_auth_provider_info`, `delete_requires_admin_for_global_meta_specs`.
- `cargo test -p gyre-server --lib -- api::users` → **11 passed, 0 failed**.
- `cd web && npm ci && npm test` → **67 files, 1667 passed / 41 skipped** — matches the shipped claim exactly.
- **Mutation probes** (each restored, `git status --short` verified clean after; logs under `/tmp/stage/review-evidence/`):
  - (A) Disabled the `exclude_types` condition in `MemNotificationRepository::list_for_user` (mem.rs:2126) → `disabled_preference_excludes_type_from_inbox_and_count` **FAILS** (got 2, expected 1).
  - (B) Guarded off the gate-override audit write in `submit_review` (merge_requests.rs:979) → `human_approval_on_failed_gate_writes_gate_override_event` **FAILS**.
  - (C) Removed the `?workspace_id=` SQL filter in the SQLite ledger's spec_approvals branch (regression to the old stamping behavior) → `judgment_ledger_workspace_filter_drops_unattributed` **FAILS** (got 5, expected 4).
  All three shipped tests are anchored to production behavior, not self-confirming.
- Mechanical checks all OK: check-arch, check-abac-exempt-handlers (88 handlers), check-mem-port-contracts, check-dead-message-kinds, check-mcp-write-tools, check-fabricated-scope-defaults, check-lossy-secret-conversion, check-scope-literal-defaults, check-inert-enforcement, check-forged-scope-fields, check-in-memory-state-stores, check-unbounded-external-http, check-abac-route-registry, check-migration-versions, check-migration-sql-portability, check-task-commit-attribution. The fabricated-scope-defaults exemption file change is a line-number refresh only (no growth).

## Verified working (per acceptance criterion)

- **Spec amendment**: HSI §12 Judgment Ledger now carries a concrete Sources table (category → table → human-attribution column → workspace-attribution column) and pins `?type=` to exactly the `JudgmentType` wire strings `approval|rejection|gate|trust|meta-spec`. Auth-provider and notification-preference bullets amended to match implementation. The spec's old "gate overrides, workspace audit log, meta-spec commit history" source list is replaced by implementable, real sources.
- **`JudgmentType::GateOverride`** added (`as_str() = "gate"`, `from_db_str("gate")` parses); `AuditEventType` gains named `TrustChange`/`GateOverride`/`MetaSpecPublish` variants with round-trip tests asserting they do NOT fall through to `Custom`.
- **Aggregation**: SQLite `list_for_user` queries `spec_approvals` LEFT JOIN `spec_ledger_entries` (join on `path`, the ledger's primary key — no row duplication; `?workspace_id=` is a real SQL `WHERE` filter, and NULL-workspace rows are dropped when the filter is set, matching the amended spec) plus `audit_events` filtered by `user_id` and `event_type IN (gate_override, trust_change, meta_spec_publish)`; merged stream sorted reverse-chron with deterministic `entity_ref` tie-break; limit/offset applied after merge only. Postgres adapter: the previous `bail!` stubs for notification-prefs/user-tokens/judgment-ledger are replaced with real Diesel implementations (parity restored, not just mirrored). Mem adapter aggregates the *same* shared stores the repos write through (`with_sources`), wired in both `mem::test_state` and production `build_state`.
- **Trust-change write path**: `PUT /workspaces/:id` writes one `trust_change` audit event per actual transition only (same-value PUT writes nothing — asserted), attributed to `auth.user_id`, carrying from/to/workspace_id; failure propagates.
- **Gate-override write path**: human (`user_id`-bearing) approval on an MR with FAILED gate results writes one `gate_override` event per failed gate with mr_id/gate_id/gate_type/from/to; agent-token approvals excluded (asserted). Honest `to_status: "overridden"` — the gate row is not silently mutated.
- **Meta-spec publish write path**: registry create and update write `meta_spec_publish` events with kind/name/version/content_hash and workspace attribution from the meta-spec's own scope; composed with the base's admin gates and §6/§11 reconciliation triggers. Registry DELETE gained scope-aware per-handler authorization with 403-before-404 ordering (no existence oracle), and the ABAC exemption file correspondingly shrank (base's removal kept).
- **Notification filtering**: `exclude_types` applied in-query before pagination in both `list_for_user` and `count_unresolved` (badge matches inbox); MCP `notifications://` resource honors the same preferences; background de-dup callers (abandoned_branch, trust_suggestion, spec_patrol, reconciliation, merge_processor, siem) correctly pass `&[]`; `PUT notification-preferences` rejects unknown `notification_type` with 400.
- **Auth provider info**: migration `2026-10-07-000056_user_auth_provider_info` (portable SQL, passes both migration scripts; version distinct from the base's other 000056s because Diesel derives version from the full date-prefixed name); `record_login` is a single conditional UPDATE (interval check and write in one statement — no read-modify-write race), SQLite/PG/mem parity; JWT validation records the *verified* issuer; `UpdateProfileRequest` has no field for either (not client-settable — struct inspected and round-trip test asserts they survive a profile edit); `GET /users/me` returns both read-only; UI displays both.
- **Commit attribution**: `implement(task-209)` checkpoint commits are merge commits (exempt); the non-merge product-surface commit `ca1ac78e` is listed in the frontmatter; `check-task-commit-attribution.sh` passes.

## Non-blocking observations (no action required for this task)

- `GET /users/me/judgments` with an unrecognized `?type=` value silently falls through to no filter (returns all types for the caller) rather than 400. User-scoped read, no cross-scope exposure, and the spec does not mandate rejection — but it is inconsistent with the task's own (correct) 400-on-unknown treatment of `notification_type` in the preferences PUT. Candidate for a follow-up hardening.
- Rejection entries carry `approved_at` (not `revoked_at`/`rejected_at`) as their timestamp — pre-existing behavior carried forward; the spec does not pin which timestamp a rejection shows.
- `submit_review`'s audit write happens after the review row is persisted; an audit failure returns 500 with the review already stored (client retry double-submits the review). Deliberate, documented trade-off ("a silently dropped judgment event is audit theatre"), consistent with the write-path error-propagation principle used in workspaces.rs and meta_specs.rs.
- `submit_review` now requires an authenticated principal (extractor added; previously callable without credentials). The route was already ABAC-registered; this strictly strengthens authorization.
- SQLite/mem ledger aggregation loads all matching rows and paginates in Rust — correct (limit/offset must apply to the merged stream), just not push-down-optimized.

Findings: none blocking.
