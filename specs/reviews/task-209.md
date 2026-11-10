# Review: task-209 — Finish HSI §12 profile (notification filtering, judgment ledger, auth provider info)

- **Candidate:** `1fe18ca3ea861d5c030b6e8bd54ce1c2a55647bd` (branch `pipeline/task-209/e856881ec79e48d5873eac1af48789a3-2`)
- **Base:** `078ec12a5745379c254fe7dd00261f02e28d86cb`
- **Result:** **APPROVED** (no findings)
- **Evidence:** `/tmp/stage/review-evidence/task-209-candidate-1fe18ca.md` (commands, exit codes, outputs)

## Scope reviewed

Diff base→candidate across 46 files: domain (`audit.rs`, `user.rs`,
`user_profile.rs`), ports (`user.rs`, `notification.rs`), sqlite/postgres/mem
adapters (`user_profile.rs`, `user.rs`, `notification.rs`), server
(`users.rs`, `workspaces.rs`, `merge_requests.rs`, `meta_specs.rs`,
`specs.rs`, `auth.rs`, `mcp.rs`, call-site adaptations), one migration,
HSI §12 spec amendment, coverage matrix row 53, docs, web UserProfile.

## What was verified

1. **Spec amendment (Part 1)** — HSI §12 Judgment Ledger now carries a
   concrete Sources table (category → table → human-attribution column →
   workspace-attribution column) naming real columns
   (`spec_approvals.approver_id`/`rejected_by`/`revoked_by`,
   `audit_events.user_id`/`workspace_id`), and pins `?type=` to
   `approval|rejection|gate|trust|meta-spec` — exactly the `JudgmentType`
   wire strings. The amendment names the real write paths that exist in this
   candidate (no fake-completion: the write paths were built, not just
   referenced).

2. **Write paths (Part 3)** — all three verified by e2e tests that pass:
   trust transition via `PUT /workspaces/:id` writes a `trust_change` audit
   event attributed to the acting user (from/to/workspace_id; no event on a
   no-change PUT); a human approval review on an MR with FAILED gate results
   writes one `gate_override` event per failed gate with
   mr_id/gate_id/gate_type/from→to (agent-token approvals excluded — negative
   case asserted); meta-spec registry create/update write `meta_spec_publish`
   events with kind/name/version/content_hash and scope-derived workspace
   attribution. `AuditEventType` variants round-trip and parse to named
   variants (not `Custom`).

3. **Aggregation (Part 2)** — `list_for_user` (sqlite/postgres/mem parity,
   all three read and compared) aggregates `spec_approvals` (LEFT JOIN
   `spec_ledger_entries` on path = PK, so no fan-out) and `audit_events`
   (gate_override/trust_change/meta_spec_publish, `user_id` attribution),
   merges, sorts reverse-chronologically with a deterministic tie-break, and
   applies limit/offset to the merged stream only. `?workspace_id=` FILTERS
   (adapter test asserts unattributed/global rows are dropped — it fails if
   the code stamps). The prior round's writer/reader identity-key mismatch is
   repaired and locked in by e2e tests that drive the real endpoints with a
   real OIDC JWT and assert both the written key (canonical user Id) and the
   surfaced ledger entry; the withdrawal-attribution rule (rejection belongs
   to the withdrawer, not the overridden approver) is covered by a dedicated
   adapter test.

4. **Notification filtering** — `exclude_types` applied in-query before
   pagination in both `list_for_user` and `count_unresolved`; users.rs inbox
   + badge and the MCP `notifications://` tool load disabled types from
   `user_notification_preferences` (no row = enabled); PUT rejects unknown
   `notification_type` with 400. **Mutation probe performed**: neutering the
   handler wiring makes `disabled_preference_excludes_type_from_inbox_and_count`
   FAIL — the test is not self-confirming. Tree restored and verified clean.

5. **Auth provider info** — `users.oidc_issuer` migration (portable SQL,
   distinct Diesel version — the version prefix includes the date, so no
   duplicate with existing 000056s; `last_login_at` column ownership resolved
   to task-120's migration, avoiding a duplicate-column failure on fresh
   databases); `record_login` is a single-statement conditional UPDATE in all
   three adapters (no read-modify-write race) with the issuer-change
   carve-out; `GET /users/me` returns both read-only; PUT has no field for
   either.

6. **Adjacent hardening** — `submit_review` gained an `AuthenticatedAgent`
   extractor (route was previously unauthenticated); meta-spec registry
   DELETE gained scope-aware per-handler authorization with
   `record_rbac_denied` on every deny path.

## Test evidence (all run on the exact candidate)

- `cargo test -p gyre-server --lib`: 1690 passed, 0 failed.
- `cargo test -p gyre-server --tests`: all integration suites green.
- `cargo test -p gyre-adapters`: 440 passed, 6 ignored (11-test user_profile
  suite green). `-p gyre-domain`: 463 passed. `-p gyre-ports -p gyre-common
  -p gyre-cli`: all green.
- `cd web && npm ci && npm test`: 99 files, 2013 passed, 41 skipped, 0 failed.
- Mechanical guards: check-arch, check-migration-versions,
  check-migration-sql-portability, check-mem-port-contracts,
  check-abac-route-registry, check-abac-exempt-handlers,
  check-forged-scope-fields, check-mcp-write-tools,
  check-in-memory-state-stores, check-inert-enforcement,
  check-task-commit-attribution — all OK. `check-tenant-filter.sh` fails at
  awk parse time under this sandbox's mawk (gawk-only script syntax) — fails
  before reading any repo file, i.e. identically on the base; environment
  restriction, not a regression. The new audit-event queries are user-scoped
  via `user_id` (the scoping HSI §12 prescribes for this endpoint), and
  `audit_events` has no tenant column by design.

## Findings

None. Every acceptance criterion is satisfied by production code proven by
tests that fail when the behavior is removed.
