# Review: task-209 — Finish HSI §12 profile (notification filtering, judgment ledger, auth provider info)

**Candidate:** `60075fcccfed3a65cab90f2770994ea7b2eaf760` (branch `pipeline/task-209/569e45f5f38a47188dae88a73c8e8f74-1`)
**Base:** `3b734b1af744f594ec5d903be242b41ec0f287f2`
**Verdict:** APPROVED (2026-11-12) — probe evidence under `/tmp/stage/review-evidence/`.

## Scope check

Net diff vs base is task-209's own work only (all merged intermediate mains — 199c1bb78,
e402e1ed, 3af80bd, cfeefbda, 75afe7fc, 7e9b3cfe, 3b734b1af — are ancestors of the base, so the
merge reconciliations contribute nothing to the net diff). `web/dist/` is byte-identical to base.

## What was verified

### 1. Spec amendment (Part 1) — real
HSI §12 now carries a concrete Sources table for the judgment ledger (spec_approvals joined to
spec_ledger_entries for workspace attribution; audit_events rows with event_type ∈
{gate_override, trust_change, meta_spec_publish} attributed via user_id), documents the
canonical-Id attribution contract for approve/reject/revoke, and fixes the `?type=` wire strings
to `approval|rejection|gate|trust|meta-spec` — exactly the `JudgmentType::as_str()` values.
Notification preferences section amended to require in-query filtering for both inbox and badge,
background-job exemption, and 400 on unknown type names. Coverage matrix row 53 flipped to
implemented with evidence; `grep -c '| not-started |'` = 0 for this spec.

### 2. Domain/port — real
`JudgmentType::GateOverride` (`as_str()="gate"`, `from_db_str("gate")` parses);
`AuditEventType::{TrustChange,GateOverride,MetaSpecPublish}` round-trip to named variants
(domain test explicitly asserts they do not fall through to `Custom`);
`NotificationRepository::{list_for_user,count_unresolved}` gain `exclude_types` with a documented
contract (applied in-query before limit/offset; `&[]` = see-everything for background jobs);
`UserRepository::record_login` with single-statement debounce + issuer-change carve-out
(post doc explains the provisioning-path interplay; adapter tests cover both paths).

### 3. Aggregation (Part 2) — real, proven against real SQLite
`list_for_user` (sqlite/postgres/mem parity — mem now shares the same store handles the writers
use, wired via `store_shared!` in lib.rs and test_state) aggregates all four categories:
spec_approvals LEFT JOIN spec_ledger_entries with a real SQL `WHERE workspace_id = ?` filter
(the old stamping behavior is gone — the workspace-filter test asserts unattributed entries are
dropped, count 4 not 6), and audit_events via the shared compression-aware `AuditEventRow`
materialization (a dedicated test runs the real `compress_details_before` pass and asserts
details survive). Reverse-chron sort with deterministic tie-break; limit/offset applied after the
merge (pagination test asserts page equals all[2..4]). Rejections are attributed to the
withdrawing human, not the original approver — tested both directions.

### 4. Write paths (Part 3) — real
- Trust transition: `PUT /workspaces/:id` writes `trust_change` after `apply_trust_transition`
  succeeds, propagating errors; no-event on same-value PUT (tested).
- Gate override: human (`auth.user_id` present) approval review on an MR with FAILED gate
  results writes one `gate_override` event per failed gate with mr_id/gate_id/gate_type/from/to;
  agent-token approvals excluded (tested both).
- Meta-spec publish: registry create + update write `meta_spec_publish` with
  kind/name/version/content_hash and workspace attribution from the meta-spec's own scope;
  registry DELETE additionally gained per-handler authorization (scope-matched admin checks —
  the route is ABAC-exempt, so the body is the enforcement surface; fails closed on dangling
  scope refs).
- specs.rs approve/reject/revoke now write the **canonical user Id** into
  approver_id/rejected_by/revoked_by so writer and reader keys match — end-to-end tests assert
  the rows surface in `/users/me/judgments`; the `user:<name>` KeyBinding label is preserved on
  the approval-history events where that contract requires it.

### 5. Notification filtering + auth provider info — real
Inbox list and badge count both load disabled types from `user_notification_preferences` and
pass them as `exclude_types` (handler-level test drives the real HTTP surface: baseline 2 →
disable GateFailure → list 1, count 1). PUT normalizes legacy parse aliases to canonical names
and 400s unknown types (alias test asserts the canonical-typed notification is actually
filtered). MCP `notifications://` tool applies the same exclusion; background de-dup callers
pass `&[]`. `users.oidc_issuer` added by migration 2026-10-07-000056 (unique Diesel version;
`last_login_at` column ownership delegated to task-120's 000058 to avoid a duplicate-column
collision on fresh DBs — reasoning documented in the migration). `validate_jwt` records the
verified issuer + login time via debounced `record_login`; `GET /users/me` returns both
read-only (PUT has no field for either; adapter test asserts update preserves them).

### 6. Frontend — real
`NOTIF_TYPE_IDS` moved to `web/src/lib/notification-types.js` listing all 41 canonical
`NotificationType::as_str()` names (the old hand-made list was largely fictitious names that
could never match a stored notification), kept in lockstep by the new
`scripts/check-notification-toggle-coverage.sh` (CI + pre-commit wired). Profile badge now uses
the server count; judgment ledger renders the actual `JudgmentEntryResponse` shape; auth
provider info displayed read-only. 27/27 UserProfile tests pass; the toggle-count test derives
the expected set from the Rust enum source, not a hardcoded number.

## Probes (see /tmp/stage/review-evidence/)

- `cargo test -p gyre-adapters sqlite::user_profile` — 12/12 (7m27s cold compile, real SQLite)
- `cargo test -p gyre-domain audit|user_profile` — 9/9
- `SKIP_WEB_BUILD=1 cargo check -p gyre-server` — pass
- `web: npm ci && npx vitest run src/__tests__/UserProfile.test.js` — 27/27
- `check-notification-toggle-coverage.sh`, `check-arch.sh`, `check-migration-versions.sh`,
  `check-mem-port-contracts.sh`, `check-abac-route-registry.sh`, `check-mcp-write-tools.sh` — all pass

## Deferred to verifier

gyre-server test codegen is excluded from the review sandbox (4 GiB rule). The write-path tests
exist, were read, and assert real behavior; the verifier must execute:
`cargo test -p gyre-server --lib` (at minimum: trust_transition_writes_trust_change_audit_event,
human_approval_on_failed_gate_writes_gate_override_event,
publish_writes_meta_spec_publish_audit_event,
disabled_preference_excludes_type_from_inbox_and_count,
preference_alias_normalizes_to_canonical_before_storing,
get_me_returns_recorded_auth_provider_info, and the specs.rs `*_surfaces_in_judgment_ledger`
tests), plus repo-wide `cargo test --all`, all-target clippy, and `cd web && npm test`.

## Non-blocking observations

- `?type=<unrecognized>` on `/users/me/judgments` degrades silently to no filter
  (pre-existing endpoint behavior, not introduced here).
- Working tree carries a controller-owned uncommitted reset of `specs/tasks/task-209.md`
  frontmatter (the review guard excludes that exact path from hash checks).
