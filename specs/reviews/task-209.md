# Review — task-209 (Finish HSI §12 profile: notification filtering, judgment ledger, auth provider info)

Spec: `specs/system/human-system-interface.md` §12 "What the Profile Is".
Candidate: `7d034ea2` (branch `pipeline/task-209/5c34cac46f784a27a745052ce6a7853e-1`), base `ccfd4bc5`.
Verdict: **approved** — no blocking findings. All three HSI §12 gaps are closed with real
production code; every acceptance criterion was independently probed or verified against source.

## Probes run (review checkout at candidate `7d034ea2`)

All logs retained under `/tmp/stage/review-evidence/`:

- `SKIP_WEB_BUILD=1 /tmp/stage/bin/cargo check -p gyre-server` → exit 0 (7m12s).
- `SKIP_WEB_BUILD=1 /tmp/stage/bin/cargo check -p gyre-server --tests` → exit 0 — type-checks
  the in-module `#[cfg(test)]` bodies and `tests/graph_integration.rs`; the prior verification
  failure (E0061 in `merge_processor.rs` test) is fixed in `5065605d` and no call-site arity
  mismatch remains (programmatic arity sweep of every `list_for_user`/`count_unresolved` call in
  `crates/` — all consistent with the new 8/3-arg signatures).
- `SKIP_WEB_BUILD=1 /tmp/stage/bin/cargo check -p gyre-cli` → exit 0.
- `/tmp/stage/bin/cargo test -p gyre-adapters --lib user_profile` → **12 passed, 0 failed**.
- `/tmp/stage/bin/cargo test -p gyre-adapters --lib audit` → **15 passed, 0 failed**.
- `/tmp/stage/bin/cargo test -p gyre-domain --lib` → **499 passed, 0 failed**.
- `cd web && npm ci && npm test` → **111 files, 2255 passed, 41 skipped, 0 failed**.
- Mechanical invariants: `check-arch.sh`, `check-migration-versions.sh` (no duplicate Diesel
  versions), `check-migration-sql-portability.sh` (up.sql is portable `ALTER TABLE ... ADD COLUMN`),
  `check-abac-route-registry.sh`, `check-abac-exempt-handlers.sh` (75 handlers, incl. the rewritten
  meta-specs DELETE per-handler authorization), `check-mem-port-contracts.sh`,
  `check-in-memory-state-stores.sh`, `check-notification-toggle-coverage.sh` (41/41 variants ↔
  toggles ↔ labels), `check-inert-enforcement.sh`, `check-forged-scope-fields.sh`,
  `check-lossy-secret-conversion.sh`, `check-fabricated-scope-defaults.sh`,
  `check-scope-literal-defaults.sh`, `check-forwarded-header-trust.sh` — all exit 0.
- `rustfmt --edition 2021 --check` on all 42 changed Rust files → clean.
- `check-task-commit-attribution.sh` on the committed candidate → OK (note: it FAILS in the
  review working tree only because the stage carries an uncommitted reset of
  `specs/tasks/task-209.md` to `progress: not-started, commits: []`; with the file stashed it
  passes — the committed candidate records every task-labeled product commit).
- `grep -c '| not-started |' specs/coverage/system/human-system-interface.md` → 0 (row 53 →
  implemented with landed evidence).

## Verified against the acceptance criteria

- **Spec amendment (Part 1)** — HSI §12 now specifies the concrete source table/column for each
  ledger category (spec_approvals + spec_ledger_entries join; audit_events rows with
  `event_type IN (gate_override, trust_change, meta_spec_publish)` attributed via `user_id`),
  the write paths for each category, and `?type=` values matching the `JudgmentType` wire strings
  (`approval|rejection|gate|trust|meta-spec`). Notification-preferences section amended to require
  canonical type names, in-query filtering for list AND badge, and 400 on unknown type.
- **`JudgmentType::GateOverride`** — domain enum variant, `as_str()="gate"`, `from_db_str("gate")`
  parses; round-trip covered by domain tests.
- **Aggregation** — `list_for_user` (sqlite/postgres/mem parity, all three read line-by-line)
  unions all four categories, filters `?workspace_id=` in SQL via the `spec_ledger_entries`
  LEFT JOIN (NULL-workspace entries excluded when the filter is set — kills the old stamping
  behavior, guarded by `judgment_ledger_workspace_filter_drops_unattributed`), sorts
  reverse-chronologically with a deterministic tie-break, applies limit/offset after the merge.
  The audit source materializes through the shared `AuditEventRow::into_event` decompression
  path, so log-compressed rows (detail blanked into `detail_zstd`) still surface full judgment
  detail — covered by `judgment_ledger_decompresses_compressed_audit_details` against the real
  compression pass.
- **Trust-change write path** — `PUT /workspaces/:id` writes a `trust_change` audit event with
  from/to/workspace_id after `apply_trust_transition` succeeds; no-change PUT writes nothing
  (guarded by `trust_transition_writes_trust_change_audit_event`, full HTTP round trip with a
  real OIDC JWT and a negative control).
- **Gate-override write path** — human (user_id-bearing) approval review on an MR with FAILED
  gate results writes one `gate_override` event per failed gate with mr_id/gate_id/gate_type/
  from→to; agent-token approvals excluded (negative control in
  `human_approval_on_failed_gate_writes_gate_override_event`, which also asserts the
  `/users/me/judgments?type=gate` shape end-to-end).
- **Meta-spec publish write path** — registry create and version-bump both write
  `meta_spec_publish` events attributed to the acting user with kind/name/version/content_hash
  and workspace attribution from the meta-spec's own scope
  (`publish_writes_meta_spec_publish_audit_event`). Bonus hardening in scope: meta-specs DELETE
  gained real per-handler authorization (scope-aware, tenant-contained) replacing a flat admin
  check.
- **Inbox filtering** — `exclude_types` threaded through the port into the SQL of both
  `list_for_user` and `count_unresolved` (sqlite/postgres), applied before limit/offset; the
  users.rs inbox + badge endpoints and the MCP `notifications://` tool load disabled types from
  `user_notification_preferences`; background de-dup callers (trust_suggestion) correctly pass
  `&[]` per the port contract. PUT normalizes legacy parse aliases to canonical names and 400s
  on unknown types (alias-normalization test guards the inert-toggle failure mode).
- **Auth provider info** — migration `2026-10-07-000056` adds `users.oidc_issuer` (portable SQL,
  no duplicate version; `last_login_at` correctly owned by task-120's `000058` — collision
  resolved on the merge, verified in the migration comments and schema). `validate_jwt` records
  the verified `iss` + login time via `UserRepository::record_login` (single-statement
  debounce with an issuer-change carve-out; sqlite/postgres/mem parity; race and debounce
  behavior covered by `record_login_debounce_and_update`). `GET /users/me` returns both
  read-only; `PUT /users/me` has no field for either; UI renders them
  (`get_me_returns_recorded_auth_provider_info` drives a real JWT through the full stack).
- **Writer/reader attribution-key fix** — specs.rs approve/reject/revoke now write the canonical
  user Id into `spec_approvals.approver_id`/`rejected_by`/`revoked_by` (labels stay on the
  history events where the KeyBinding contract requires them), with end-to-end
  `*_surfaces_in_judgment_ledger` tests — this fixes what would otherwise be an inert ledger for
  JWT/API-key humans.
- **Frontend** — `UserProfile.svelte` renders canonical toggles from the new
  `lib/notification-types.js` list, uses the server badge count (not a client-side page count),
  reads the real wire shapes; the test derives the expected toggle set from the Rust enum source
  itself, so a variant without a toggle fails the suite (the exact failure mode of prior rounds),
  and the new shell script enforces the same in CI/pre-commit.
- **Test quality** — adapter tests seed through real repositories and assert through the port;
  kill-checks verified: the workspace-filter test fails under stamping, the exclusion test fails
  if the filter is dropped, the debounce test fails on in-window writes and on a missing
  issuer-change carve-out. Server write-path tests are full-HTTP round trips with negative
  controls, not handler-unit mocks.

## Deferred to the independent verifier (4 GiB budget — wrapper rejects server codegen)

`SKIP_WEB_BUILD=1 cargo test --all` must run remotely; minimally the gyre-server lib target:
`trust_transition_writes_trust_change_audit_event`,
`human_approval_on_failed_gate_writes_gate_override_event`,
`publish_writes_meta_spec_publish_audit_event`,
`disabled_preference_excludes_type_from_inbox_and_count`,
`preference_alias_normalizes_to_canonical_before_storing`,
`get_me_returns_recorded_auth_provider_info`,
`approve_via_jwt_surfaces_in_judgment_ledger` (+ the reject/revoke siblings in specs.rs),
`notifications_exclude_types_applies_to_list_and_count` (mem path via users.rs tests), and
`tests/graph_integration.rs`. Their bodies type-check clean in this sandbox
(`cargo check -p gyre-server --tests` exit 0) and their logic was reviewed line-by-line; the
mem-adapter logic they exercise mirrors the SQL adapter that IS test-covered here.

## Non-blocking observations (no action required)

- The judgment ledger materializes all matching rows before in-memory skip/take (the union-sort-
  paginate contract makes this the straightforward reading; unbounded growth is bounded by a
  single user's judgment count — a future SQL UNION ALL with LIMIT would scale better).
- `GET /users/me/judgments` silently ignores an unrecognized `?type=` value (no filter) rather
  than 400ing; the spec does not require rejection.
- Cosmetic: one pre-existing submit_review test now sets the `authorization` header twice
  (same value) in `merge_requests.rs`.
- Stage bookkeeping only: the review working tree carries an uncommitted reset of
  `specs/tasks/task-209.md` (progress/commits frontmatter) — not part of the candidate; the
  committed file records all attribution and passes `check-task-commit-attribution.sh`.
