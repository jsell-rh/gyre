# Review — task-209 (Finish HSI §12 profile: notification filtering, judgment ledger, auth provider info)

Spec: `specs/system/human-system-interface.md` §12 "What the Profile Is" (row 53).
Candidate under review: `766b9cc4ad5e299dfde808cd332331e0f362b182` (merge-repair branch remerged
onto current base `9bfab365`, per the durable `merge_conflict` finding; the repair commits
`239c4b6b`/`0f4486ab`/`766b9cc4` are process/merge commits).
Verdict: **approved** — no blocking findings. Evidence under `/tmp/stage/review-evidence/`.

## Probes run (candidate checkout at `766b9cc4`)

- `SKIP_WEB_BUILD=1 cargo check -p gyre-adapters --lib --tests` → clean (5m49s; log
  `t209-adapters-check.log`).
- `SKIP_WEB_BUILD=1 cargo check -p gyre-server --lib --tests` → clean (4m25s; log
  `t209-server-check.log`) — confirms every `NotificationRepository::list_for_user`/`count_unresolved`
  call site (incl. the merge-repair `graph_extraction.rs` arity fix) and all new server test code
  type-check.
- `cargo test -p gyre-adapters --lib user_profile` → **19 passed** (incl. all 7 new judgment-ledger
  tests, `notifications_exclude_types_applies_to_list_and_count`,
  `record_login_debounce_and_update`, and the postgres/sqlite `JUDGMENT_AUDIT_TYPES` parity test).
- `cargo test -p gyre-adapters --lib notification` → 11 passed.
- `cargo test -p gyre-adapters --lib sqlite::user::` → 12 passed.
- `cargo test -p gyre-adapters --lib spec_approval` → 14 passed (covers the amended
  Approved→Rejected lifecycle).
- `cargo test -p gyre-domain` → 544 passed (incl. `JudgmentType` wire round-trip and the
  `AuditEventType` named-variant parse guards).
- `cd web && npm ci && npm test` → **116 files, 2371 passed / 41 skipped** (log
  `t209-web-full-tests.log`); focused `UserProfile.test.js` 30/30.
- Mechanical checks (log `t209-mechanical-checks.log`, `...-batch2.log`): check-arch,
  check-abac-route-registry, check-abac-exempt-handlers (80 handlers), check-mcp-write-tools,
  check-mem-port-contracts, check-inert-enforcement, check-migration-versions,
  check-dead-message-kinds, check-byte-slice-truncation, check-notification-toggle-coverage
  (new script: 41 enum variants ↔ 41 toggles ↔ labels), check-forged-scope-fields,
  check-lossy-secret-conversion, check-scope-literal-defaults, check-fabricated-scope-defaults,
  check-in-memory-state-stores, check-relative-path-defaults, check-fail-open-ref-resolution,
  check-forwarded-header-trust, check-unbounded-external-http, check-api-auth,
  check-conditional-test-guards, check-aspirational-test-names, check-unnamed-tuple-carriers —
  all exit 0. No exemption-file changes in the diff.

## Verified (acceptance criteria)

- **Spec amendment (Part 1)** — HSI §12 now names the four concrete sources with attribution
  columns (`spec_approvals` + `spec_ledger_entries` join; `audit_events` rows with
  `event_type IN (gate_override, trust_change, meta_spec_publish)` attributed via `user_id`),
  documents the canonical-user-Id vs KeyBinding-label distinction, and fixes `?type=` to
  `approval|rejection|gate|trust|meta-spec` — exactly `JudgmentType::as_str()`.
- **`JudgmentType::GateOverride`** — `as_str() = "gate"`, `from_db_str("gate")` parses
  (`gyre-domain/src/user_profile.rs`), with a wire round-trip test.
- **Aggregation (Part 2)** — `list_for_user` (sqlite/postgres/mem parity; postgres parity is
  itself asserted by `judgment_audit_type_list_matches_sqlite_sibling`) unions all four
  categories from real tables; `?workspace_id=` is a SQL `WHERE` filter that drops
  NULL-workspace rows (test `judgment_ledger_workspace_filter_drops_unattributed` — the old
  stamping behavior would fail it); reverse-chronological with deterministic `entity_ref`
  tie-break; limit/offset applied after the merge (`judgment_ledger_pagination_across_merged_stream`).
  Source-2 rows materialize through the shared `AuditEventRow::into_event` decompression path —
  proven by `judgment_ledger_decompresses_compressed_audit_details` running the real
  `compress_details_before` pass first. Rejection attribution (decliner, not original approver)
  is covered by `judgment_ledger_rejection_attributed_to_withdrawer`.
- **Trust-change write path** — `PUT /workspaces/:id` writes a `trust_change` audit event on real
  transitions only (from/to/workspace_id in detail; same-value PUT writes nothing), attributed to
  the acting user; end-to-end HTTP+JWT test `trust_transition_writes_trust_change_audit_event`
  (compile-verified here; see Verifier gates below).
- **Gate-override write path** — a human (`user_id`-bearing principal) approval review on an MR
  with failed gate results writes one `gate_override` event per failed gate
  (mr_id/gate_id/gate_type/failed→overridden); agent-token approvals excluded. End-to-end test
  `human_approval_on_failed_gate_writes_gate_override_event` also drives
  `GET /users/me/judgments?type=gate` and asserts the entry surfaces.
- **Meta-spec publish** — registry create/update write `meta_spec_publish` events
  (kind/name/version/content_hash, workspace attribution from scope); test
  `publish_writes_meta_spec_publish_audit_event` covers both, plus the ledger endpoint. The
  DELETE handler additionally gained real per-handler scope authorization (tenant/workspace
  containment, fail-closed) with cross-tenant/membership tests — consistent with the
  ABAC-exempt-handler invariant, and check-abac-exempt-handlers passes.
- **Inbox filtering** — `exclude_types` is applied inside the SQL (`ne_all` before limit/offset)
  in both `list_for_user` and `count_unresolved`, wired into the inbox list, the badge count, and
  the MCP `notifications://` tool; background/de-dup callers pass `&[]` per the port contract.
  `PUT /users/me/notification-preferences` rejects unknown types (400) and normalizes legacy
  parse aliases to canonical `as_str()` names before storing (alias test proves the stored toggle
  actually filters). Handler-level test `disabled_preference_excludes_type_from_inbox_and_count`.
- **Auth provider info** — migration `2026-10-07-000056_user_auth_provider_info` adds
  `users.oidc_issuer` (numbering check clean; `last_login_at` ownership de-duplicated against
  task-120's 000058); `UserRepository::record_login` (sqlite/postgres/mem) is a single-statement
  debounced write with an issuer-change carve-out — behavior proven by
  `record_login_debounce_and_update` (in-window no-op, issuer-change bypass, update() preserves);
  JWT validation records the verified issuer; `GET /users/me` returns both read-only and
  `PUT /users/me` has no field for either (test `get_me_returns_recorded_auth_provider_info`).
- **Frontend** — profile renders OIDC issuer + last login read-only, ledger type/workspace
  filters + server-side pagination (tests assert the actual `?type=`/`?workspace_id=`/offset
  params on `api.myJudgments`), and a toggle for every canonical `NotificationType` (41/41),
  mechanically enforced by the new `check-notification-toggle-coverage.sh` in pre-commit + CI.
- **Lifecycle amendment** — `agent-gates.md` §Spec Approval Ledger now permits
  Approved→Rejected (decline-after-approval, `rejected_by` records the declining human;
  terminal states unchanged), with domain + adapter tests; coherent with the HSI §12 rejection
  attribution and amended in the spec rather than shipped silently.

## Non-blocking observations

- `merge_requests.rs` test `lineage deps ...` (pre-existing test) now sets the
  `authorization` header twice with the same value — harmless duplication from the merge repair.
- `scripts/check-tenant-filter.sh`, `check-assertionless-tests.sh`, `check-dead-test-code.sh`,
  `check-cross-surface-parity.sh`, `check-comparative-test-claims-js.sh` exit 2 in this sandbox
  with an mawk parse error (gawk-only `match($0, re, arr)` syntax); the scripts are unchanged by
  this candidate and `check-tenant-filter.sh` fails identically at the base commit (verified in a
  base worktree) — environment incompatibility, not a code defect.
- `specs/tasks/task-209.md` in the working tree was reset by the review harness (progress
  `not-started`, `commits: []`); the committed candidate `766b9cc4` carries the full attribution
  frontmatter. Not a candidate defect.

## Verifier gates (not executable in this sandbox within budget)

Server test compilation is verified (cargo check above); execution of the full `gyre-server`
suite belongs to the independent verifier. Minimum focused set for this task:

    cargo test -p gyre-server --lib trust_transition_writes_trust_change_audit_event
    cargo test -p gyre-server --lib human_approval_on_failed_gate_writes_gate_override_event
    cargo test -p gyre-server --lib publish_writes_meta_spec_publish_audit_event
    cargo test -p gyre-server --lib delete_requires_admin_for_global_meta_specs
    cargo test -p gyre-server --lib delete_tenant_scoped_meta_spec_requires_scope_tenant_admin
    cargo test -p gyre-server --lib delete_workspace_meta_spec_scopes_to_membership
    cargo test -p gyre-server --lib disabled_preference_excludes_type_from_inbox_and_count
    cargo test -p gyre-server --lib preference_alias_normalizes_to_canonical_before_storing
    cargo test -p gyre-server --lib get_me_returns_recorded_auth_provider_info
    cargo test -p gyre-server --lib judgments_unknown_type_returns_400
    cargo test -p gyre-server --lib surfaces_in_judgment_ledger   # specs.rs approve/reject/revoke end-to-end

plus repo-wide `cargo test --all` and the gawk-based lint scripts under a gawk-equipped runner.
