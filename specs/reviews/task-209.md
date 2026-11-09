# Review — task-209 (Finish HSI §12 profile: notification filtering, judgment ledger, auth provider info)

Spec: `specs/system/human-system-interface.md` §12 "What the Profile Is" (coverage row 53 — three 2026-09-30 §12-cycle gaps: notification-type filtering, judgment-ledger aggregation + write paths, auth provider info).
Comparison base: `fefde2946f3a0af1f212588f54591516fee0f7b2` → candidate `d25512b16d1c96cdc6962cc3214678a3e1ea89c1` (task branch head; includes the merge of the newer assigned base and the duplicate-column migration repair).
Verdict: **complete / approved**.

## Round 1

Evidence persisted under `/tmp/stage/review-evidence/` (probes.txt summarizes commands, exit codes, outputs; full logs alongside). Working tree clean at the candidate before and after every probe; both mutation probes were restored and the tree re-verified.

### Acceptance criteria — all verified

- **Spec amendment (Part 1)** — HSI §12 now carries a concrete Sources table (category → table → human-attribution column → workspace-attribution column), states when each event is written, and pins `?type=` values to `approval|rejection|gate|trust|meta-spec` — exactly the `JudgmentType` wire strings. The amendment names real, existing write paths (not aspirational ones), satisfying the no-fake-completion rule: each of the four categories has a genuine writer (see write paths below).
- **`JudgmentType::GateOverride`** — domain enum variant added, `as_str() = "gate"`, `from_db_str("gate")` parses; round-trip asserted.
- **Aggregation (Part 2)** — `list_for_user` (SQLite, Postgres, mem parity — all three read the same four sources) unions `spec_approvals` (LEFT JOIN `spec_ledger_entries` for workspace attribution) with `audit_events` rows of the three judgment event types attributed by `user_id`, sorts reverse-chronologically with a deterministic entity_ref tie-break, and applies limit/offset to the merged stream only. `?workspace_id=` is now a real SQL WHERE filter on both sources (unattributed entries excluded when set) — the old stamp-the-param behavior is gone. The `?type=` filter selects the physical source (approvals vs audit) per-source.
- **Trust-change write path** — `PUT /api/v1/workspaces/:id` writes a `trust_change` audit event on successful transition only (same-value PUT writes nothing — asserted), attributed to the acting user via `auth.user_id`, carrying from/to/workspace_id.
- **Gate-override write path** — a human approval review (OIDC JWT or API-key user, i.e. `user_id` is Some; agent tokens have `user_id: None` and are excluded — asserted) on an MR with FAILED gate results writes one `gate_override` event per failed gate carrying mr_id/gate_id/gate_type/from→to. Endpoint test additionally proves the ledger surfaces it via `GET /users/me/judgments?type=gate` with the right shape.
- **Meta-spec publish write path** — registry create/update write `meta_spec_publish` events (kind/name/version/content_hash, workspace attribution from scope); ledger surfaces both publishes via `?type=meta-spec` with workspace_id.
- **Notification filtering** — `NotificationRepository::list_for_user`/`count_unresolved` take `exclude_types` applied in-query (SQL `ne_all` before limit/offset) in SQLite/Postgres/mem; users.rs inbox + badge endpoints and the MCP `notifications://` tool load disabled types from `user_notification_preferences`; background de-dup callers pass `&[]` (exemption is port-documented); `PUT` rejects unknown `notification_type` with 400 (validated via `NotificationType::parse`).
- **Auth provider info** — migration `2026-10-07-000056` adds only `users.oidc_issuer` (the duplicate `last_login_at` column collision with the base's `2026-10-08-000058` was correctly resolved to a single owner — every fresh-DB SQLite adapter test proves the chain runs); JWT validation records issuer + login via `record_login` (single-statement debounce with an issuer-change carve-out, so the base's provisioning path that re-stamps `last_login_at` cannot erase issuer provenance — regression-tested with a changed-issuer-in-window case); `GET /users/me` returns both read-only; `PUT /users/me` has no field for either.
- **Test suites** — `cargo test -p gyre-server --lib`: 1417 + 74 (graph) = 1491 passed, 0 failed; `cargo test -p gyre-adapters --lib`: 389 passed, 0 failed, 12 ignored (live-PG-gated, pre-existing); `cargo test -p gyre-server --test graph_integration`: 38 passed; `cd web && npm ci && npm test`: 89 files, 1939 passed, 41 skipped.

### Probe highlights (independent evidence, not the implementer's claims)

- **Mutation probe A**: removing the audit-source workspace filter from `sqlite/user_profile.rs` → `judgment_ledger_workspace_filter_drops_unattributed` FAILED (3/4). The filter test kills the regression it guards.
- **Mutation probe B**: stubbing `disabled_notification_types()` to return empty in `users.rs` → `disabled_preference_excludes_type_from_inbox_and_count` FAILED. The endpoint test proves handler wiring, not just the SQL.
- Both mutations restored; `git status` clean at `d25512b16d1c96cdc6962cc3214678a3e1ea89c1`.
- Mechanical checks all exit 0: check-arch, check-migration-versions, check-mem-port-contracts, check-abac-exempt-handlers (88 handlers), check-fabricated-scope-defaults, check-migration-sql-portability, check-in-memory-state-stores, check-task-commit-attribution. `check-tenant-filter.sh` cannot execute under this sandbox's mawk (gawk-only 3-arg `match()` at line 30) — infrastructure restriction, not a code defect; manual inspection confirms the new queries are keyed by the caller's own identity (`audit_events.user_id`, `spec_approvals.approver_id`), no tenant-crossing surface added. CI runs it under gawk.

### Notes (non-blocking observations, not defects)

- A spec-approval row that was later rejected/revoked surfaces in the ledger at `approved_at`, not `rejected_at`/`revoked_at`. HSI §12 says "with spec path, timestamp, workspace" without pinning which timestamp; the chosen reading (the approval record's timestamp) is defensible and consistently implemented across all three adapters. If the spec is later amended to mean the decision timestamp, the adapters should switch to `rejected_at`/`revoked_at` — noting for a future cycle.
- `submit_review` gained an `AuthenticatedAgent` extractor (previously none). The route was already ABAC-mapped (`merge_request` resource), so this is strictly hardening; internal flows (gate_executor) call `state.reviews.submit_review` directly and are unaffected; all existing review tests pass with the added auth header.
- The registry DELETE handler gained real per-handler authorization (scope-aware, tenant-containment-first with 403-before-404 ordering) — beyond the task's letter but in the same file and in the direction of more enforcement, not less; `check-abac-exempt-handlers` confirms compliance.

Conclusion: every acceptance criterion is backed by production code and hard tests; the coverage row 53 flip to `implemented` is justified. Approved.
