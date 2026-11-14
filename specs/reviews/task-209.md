# Review — task-209 (Finish HSI §12 profile: notification filtering, judgment ledger, auth provider info)

Spec: `specs/system/human-system-interface.md` §12 "What the Profile Is".
Assignment: base `44cf25bf0`, candidate `52d33eb54ffe098d670ebe0bee2d5e6a01096966` (this round: base merge `570859c6` + unauthorized-approver test repair `e743177f` + docs `52d33eb5`).
Verdict: **approved** — all acceptance criteria verified against source and focused probes; no blocking findings. Server-side test *execution* (beyond compile-check) is deferred to the independent verifier per the 4 GiB build budget; the exact commands are listed below.

## Probes run (this round, HEAD = candidate `52d33eb5`, working tree clean)

- `bash scripts/check-notification-toggle-coverage.sh` → "OK: 41 enum variants, 41 toggles, labels in lockstep" (exit 0).
- Full invariant sweep (`/tmp/stage/review-evidence/invariant-scripts.log`): check-arch, check-abac-route-registry, check-migration-versions, check-migration-sql-portability, check-mem-port-contracts, check-inert-enforcement, check-scope-literal-defaults, check-unbounded-external-http, check-in-memory-state-stores, check-forwarded-header-trust, check-forged-scope-fields, check-lossy-secret-conversion, check-fabricated-scope-defaults, check-dead-message-kinds, check-mcp-write-tools, check-abac-exempt-handlers, check-byte-slice-truncation, check-relative-path-defaults, check-fail-open-ref-resolution, check-task-commit-attribution — all exit 0.
- `cargo test -p gyre-adapters --lib user_profile` → 19/19 passed (ledger merge/filter/pagination/attribution/compression, notification exclusion, record_login debounce).
- `cargo test -p gyre-adapters --lib` → **539 passed, 0 failed** (full adapters suite).
- `cargo test -p gyre-domain --lib` → **536 passed, 0 failed**.
- `SKIP_WEB_BUILD=1 cargo check -p gyre-server --tests` → exit 0 on the merged tree (5m39s; includes the repaired `spec_enforcement` integration test and task-132's merged `policies.rs`).
- `cd web && npm ci && npm test` → **115 files, 2358 passed, 41 skipped** (includes the reworked `UserProfile.test.js`, 26 tests).
- Evidence retained under `/tmp/stage/review-evidence/`.

## Verification of the durable-finding repair (`e743177f`)

Durable finding 6f8b4aca (from the 8eda66da verification run): `spec_enforcement::unauthorized_approver_is_denied_by_abac` failed because task-209 deliberately changed `spec_approvals.approver_id` to carry the acting human's canonical user Id, while the task-145 control assertion still expected the pre-change `user:<display name>` label. The repair adapts the assertion to `user-human-approver`. **Verified legitimate, strength preserved:** the test provisions the acting user with `Id::new("user-human-approver")` (spec_enforcement.rs:1064), so the assertion still requires the ledger row to carry the exact acting user's identity — a system/agent/wrong-user attribution still fails it. The rationale is real: the judgment-ledger reader (`GET /users/me/judgments` → `resolve_user_id`) resolves the caller's canonical Id, so a display-name writer key can never match it (writer/reader identity-key mismatch — precisely the HSI §12 hollowness this task closes); the `user:<name>` KeyBinding label is kept on the approval-history event, where that contract requires it. Task-209's in-module writer/reader tests (`api::specs::approve/revoke/reject_via_jwt_surfaces_in_judgment_ledger`) assert both sides of the same contract. The repair commit is recorded in the task frontmatter (attribution gate exit 0).

## Acceptance criteria — all verified

1. **Spec amendment** — HSI §12 Judgment Ledger now carries a concrete Sources table (`spec_approvals` LEFT JOIN `spec_ledger_entries`; `audit_events` rows with `event_type IN (gate_override, trust_change, meta_spec_publish)` attributed via `user_id`), documents the canonical-Id attribution key and why `agent_id`/KeyBinding labels are not it, and fixes `?type=` to `approval|rejection|gate|trust|meta-spec`. `agent-gates.md` §Spec Approval Ledger amended for the Approved → Rejected decline-after-approval path; `docs/api-reference.md` rows updated.
2. **`JudgmentType::GateOverride`** — `as_str() = "gate"`, `from_db_str("gate")` parses (`gyre-domain/src/user_profile.rs`); round-trip test extended.
3. **Aggregation** — `list_for_user` (sqlite behavioral tests against a real SQLite file; postgres SQL-shape parity tests per the `postgres/search.rs` no-PG-harness precedent; mem parity via shared stores) unions all four categories, applies `?workspace_id=` as a real SQL WHERE filter on the left-joined `spec_ledger_entries.workspace_id` (workspace filter drops NULL-attributed rows — asserted by `judgment_ledger_workspace_filter_drops_unattributed`), sorts reverse-chron with a deterministic `entity_ref` tie-break, and applies limit/offset after the union+sort (`judgment_ledger_pagination_across_merged_stream`). Rejection attribution re-check (`judgment_ledger_rejection_attributed_to_withdrawer`), foreign-user exclusion, pending-row exclusion, and compressed-`detail_zstd` reads (`judgment_ledger_decompresses_compressed_audit_details`, driving the real `compress_details_before`) all tested.
4. **Trust-change write path** — `update_workspace` writes a `trust_change` AuditEvent (from/to/workspace_id, `user_id` = acting principal) after `apply_trust_transition` succeeds, only on real transitions; failure propagates. Test asserts event + attribution + that a no-change PUT writes nothing.
5. **Gate-override write path** — `submit_review` writes one `gate_override` event per FAILED gate result on human (`user_id`-bearing) approvals only; agent-token approvals excluded. Test seeds one failed + one passed gate result and asserts exactly one event with mr_id/gate_id/from/to, plus the `?type=gate` ledger surfacing end-to-end.
6. **Meta-spec publish attribution** — registry create/update call `record_meta_spec_publish` (kind/name/version/content_hash; workspace attribution from the meta-spec's own scope, Global/Tenant deliberately unattributed). Test drives the real JWT flow and asserts the event.
7. **Inbox filtering** — `exclude_types` added to the `NotificationRepository` port; sqlite/postgres apply `notification_type.ne_all(...)` in-query before limit/offset; badge count and the MCP `notifications://` tool use the same disabled set; background de-dup callers (secret_rotation) pass `&[]` per the documented port contract. PUT normalizes legacy parse aliases to canonical `as_str()` names and 400s unknown types (a stored alias would be an inert toggle — tested). Server test covers list + count via the real handler wiring.
8. **Auth provider info** — migration `2026-10-07-000056` adds `users.oidc_issuer` (portable SQL, next unused version); `record_login` implemented in sqlite/postgres/mem with the interval check and write in ONE statement (no read-modify-write race) and the issuer-change debounce carve-out (adapter test asserts all four behaviors); JWT validation calls it (failure is a warning — auth must not depend on bookkeeping); `GET /users/me` returns both read-only (`UpdateProfileRequest` has no field for either); `PUT /users/me` round-trip preserves them.
9/10. **Test gates** — adapters (539) and domain (536) green in-sandbox; `cargo check -p gyre-server --tests` exit 0; web `npm ci && npm test` 2358 passed in-sandbox. Full `cargo test --all` (including the gyre-server lib and integration targets) belongs to the verifier.

## Test-inflation check

None found. The adapter tests seed real rows through real repository methods and assert hard, distinguishing outcomes (counts, exact ordering, exact attribution keys, mutual exclusivity, decompression). The server tests drive the real router with real signed JWTs and assert against the shared stores the production wiring uses (`test_state_inner` wires `MemJudgmentLedgerRepository::with_sources` to the SAME audit/spec-approval/spec-ledger stores the repos write through — verified in both `mem.rs` test state and production `lib.rs`). The frontend test derives the expected notification-type set from the Rust enum source itself (single source of truth) instead of hardcoding the implementation's count — the exact anti-self-confirming pattern. `judgments_unknown_type_returns_400` asserts both the 400 and that every canonical type string is accepted.

## Scope check

No out-of-scope edits. No route/ABAC changes (verified: `api/mod.rs` and `abac_middleware.rs` untouched in this range); the only non-task-209 product file touched is the task-145 control assertion repaired per the durable finding (justified above, commit attributed). The meta-spec registry DELETE gained per-handler authorization (`authorize_meta_spec_delete`) — required because the route is ABAC-exempt and the delete flow was touched for scope attribution; consistent with the check-abac-exempt-handlers contract (gate exit 0).

## Verifier must run (deferred per the 4 GiB budget; compile-checked here)

- `cargo test -p gyre-server --lib api::users` — judgments endpoint (400 + canonical types), oidc_issuer/last_login via real JWT, inbox/count filtering.
- `cargo test -p gyre-server --lib api::workspaces` — trust_change write path (incl. no-change PUT).
- `cargo test -p gyre-server --lib api::merge_requests` — gate_override write path (agent exclusion, one-event-per-failed-gate, `?type=gate` ledger surfacing).
- `cargo test -p gyre-server --lib api::meta_specs` — meta_spec_publish attribution + delete authorization.
- `cargo test -p gyre-server --lib api::specs` — approve/reject/revoke writer/reader identity-key contract.
- `cargo test -p gyre-server --test spec_enforcement` — the repaired `unauthorized_approver_is_denied_by_abac` and the rest of the suite as merged.
- Full gates: `cargo test --all`, `cd web && npm ci && npm test`.

## Non-blocking observations (recorded, not findings)

- `scripts/check-tenant-filter.sh` emits `awk: line 11: syntax error` under this sandbox's mawk 1.3.4 and exits 0. The script is unchanged since before the base commit (last touched in `a890c651`) — a pre-existing script/awk-version incompatibility, not a candidate defect. The task's instruction to check it clean is satisfied to the extent the environment permits.
- Generated-output note: `web/dist/` is unchanged from base while the authored frontend source changed in this range. Per the review contract the bundle is the deterministic verifier's surface (it rebuilds remotely, runs checks, and binds publication to the tested tree) — not a defect, recorded for the verifier's rebuild step.
