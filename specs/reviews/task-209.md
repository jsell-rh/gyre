# Review — task-209 (HSI §12 profile: notification filtering, judgment ledger, auth provider info)

Spec: `human-system-interface.md` §12 "What the Profile Is".
Candidate under review: `2b2d9d18` (branch `pipeline/task-209/2cc4fdee1df54b0ca6cbf2e289849ca9-1`, base `e608ac78` merged in via `3a39b6d8`).
Evidence: `/tmp/stage/review-evidence/` (README.md indexes every probe command, source, exit code, and output).

**Verdict: approved.** All ten acceptance criteria verified against the merged tree with independent probes; two mutation probes confirm the key tests kill real defects; all invariant scripts pass.

## Verified working

- **Spec amendment (AC1).** HSI §12 Judgment Ledger now carries a concrete Sources table (category → table → human-attribution column → workspace-attribution column), states when each event is written, and pins `?type=` to `approval|rejection|gate|trust|meta-spec` — exactly the `JudgmentType` wire strings. The amendment's attribution claims were checked against the schema: `audit_events.user_id` and `workspace_id` are real columns (schema.rs:242-248), and `spec_ledger_entries` is keyed by `path` (schema.rs:537) so the LEFT JOIN cannot fan out.
- **JudgmentType::GateOverride (AC2).** `as_str() = "gate"`, `from_db_str("gate")` parses (domain round-trip test extended to assert named-variant parsing, not a Custom fallthrough).
- **Aggregation (AC3).** `list_for_user` (sqlite + postgres + mem parity) queries `spec_approvals` LEFT JOIN `spec_ledger_entries` with `?workspace_id=` as a real SQL filter (NULL-workspace entries excluded when the filter is set, matching the amended spec) plus `audit_events` rows with `event_type IN (gate_override, trust_change, meta_spec_publish)` attributed via `user_id`; merged stream sorted reverse-chronologically with an `entity_ref` tie-break; limit/offset applied after the merge only. Adapter tests cover merge/reverse-chron/type-filter/workspace-filter/pagination and foreign-user isolation.
- **Trust-change write path (AC4).** `PUT /api/v1/workspaces/:id` writes a `trust_change` audit event attributed to the acting user, only on a real transition (same-value PUT writes nothing — tested).
- **Gate-override write path (AC5).** A human (`auth.user_id.is_some()`) approving an MR with FAILED gate results writes one `gate_override` event per failed gate (mr_id/gate_id/gate_type/from→to = failed→overridden, workspace from the MR). Agent-token approvals do not qualify (negative case tested). Hardening bonus: `submit_review` had **no auth extractor at base** — the candidate adds `AuthenticatedAgent` to a previously unauthenticated review-submission route.
- **Meta-spec publish attribution (AC6).** Registry create and update each write a `meta_spec_publish` event (kind/name/version/content_hash, workspace from the meta-spec's own scope); registry DELETE additionally gained real per-handler authorization (tenant containment + Owner/Admin or tenant-Admin, per agent-runtime §214) and its exemption line was removed from `scripts/abac-exempt-handlers-exemptions.txt` — a shrink, allowed by the frozen-baseline rule.
- **Inbox filtering (AC7).** `NotificationRepository::list_for_user`/`count_unresolved` take `exclude_types` applied in-query before pagination (NOT IN, empty-list-safe); users.rs inbox + badge endpoints and the MCP `notifications://` tool load disabled types from `user_notification_preferences`; background de-dup callers pass `&[]`; PUT rejects unknown `notification_type` with 400. The web component's type list now matches the canonical `NotificationType` variants exactly (the old list used fabricated names that could never match).
- **Auth provider info (AC8).** Migration 000056 (unique next sequence number; portable SQL — both check scripts green) adds `users.oidc_issuer`/`last_login_at`; `validate_jwt` records both via `UserRepository::record_login` — the issuer is the config issuer the token's `iss` claim was validated against (auth.rs:755), debounced in a single filtered UPDATE (no read-modify-write race; sqlite/postgres/mem parity, debounce test included); `GET /users/me` returns both read-only and `PUT /users/me` has no field for either (round-trip test).
- **Test suites (AC9/AC10).** `cargo test -p gyre-server --lib` → 1249 passed; `-p gyre-adapters --lib` → 343 passed; `-p gyre-domain --lib` → 371 passed; `cd web && npm ci && npm test -- --run src/__tests__/UserProfile.test.js` → 26 passed.

## Test-strength probes (mutations)

- Neutering the inbox exclusion (`disabled = vec![]` in `get_my_notifications`) → `disabled_preference_excludes_type_from_inbox_and_count` FAILS.
- Disabling the gate-override write (`if false && …` in `submit_review`) → `human_approval_on_failed_gate_writes_gate_override_event` FAILS.
Both mutations reverted; `git status` clean afterward.

## Invariant scripts

check-arch, check-migration-versions, check-migration-sql-portability, check-abac-exempt-handlers, check-abac-route-registry, check-inert-enforcement, check-dead-message-kinds, check-mem-port-contracts, check-lossy-secret-conversion, check-scope-literal-defaults, check-fabricated-scope-defaults, check-in-memory-state-stores, check-task-commit-attribution — all exit 0. (`check-tenant-filter.sh` is gawk-only and cannot run under this sandbox's mawk; consistent with the shipped note — CI runs it.)

## Observations (non-blocking, not findings)

- The ledger's approval/rejection category reads `spec_approvals`, which has no production writer today — the live spec-approval flow writes `spec_approval_history` (SpecApprovalEvent), and `SpecApprovalRepository::create` has no caller outside tests. This predates the task (the base ledger already queried `spec_approvals` only), is tracked as agent-gates coverage row 21 (task-assigned **task-138** "Spec Approval Ledger"), and both the original HSI §1420 and the amended Sources table name `spec_approvals` as the source. The task-209 contract does not include building that write path, so it does not gate this review — but task-138 should be aware that the judgment ledger's approval category stays empty in production until its writer lands, and that the ledger joins on `spec_approvals.approver_id` = user Id (not the `user:<name>` KeyBinding identity the history table stores).
- `MemJudgmentLedgerRepository::Default` constructs disconnected empty stores; unused today (all constructions use `with_sources`), but a future `Default` use would silently serve an empty ledger.
