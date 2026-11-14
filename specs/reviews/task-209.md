# Review — task-209 (Finish HSI §12 profile: notification filtering, judgment ledger, auth provider info)

Spec: `specs/system/human-system-interface.md` §12 "What the Profile Is".
Candidate under review: `aded630d9d32b87af0b52940eedb3c637dd0b8e2` (branch
`pipeline/task-209/2fd8330f049646b7bae6f06328f61b48-1`, assigned base `5984177c`,
merge-base confirmed = base). This is a re-review of a long-lived candidate that
has been through several prior repair rounds and main merges; the diff from base
is 64 files / +5135 −364.

Verdict: **approved** — all ten acceptance criteria are satisfied by real
production code; the focused probes below are green. Two execution items are
delegated to the independent verifier per the stage's build-budget constraints
(recorded under "Verifier gates").

## Probes run (evidence in /tmp/stage/review-evidence/)

- `SKIP_WEB_BUILD=1 cargo test -p gyre-adapters --lib user_profile` → **19/19
  passed** (judgment ledger merge/reverse-chron/pagination/type-filter/
  workspace-filter-drops-unattributed/rejection-attributed-to-withdrawer,
  compressed-audit-detail decompression, notifications exclude_types for list
  AND count, record_login debounce, PG SQL-shape parity tests).
- `SKIP_WEB_BUILD=1 cargo test -p gyre-adapters --lib notification` → **11/11
  passed**.
- `SKIP_WEB_BUILD=1 cargo test -p gyre-domain --lib` → **536/536 passed**
  (includes `JudgmentType` gate round-trip, `AuditEventType` named-variant
  parse arms, `ApprovalStatus` Approved→Rejected transition + terminality).
- `SKIP_WEB_BUILD=1 cargo check -p gyre-server --lib --tests` → **clean**
  (4m54s; every `list_for_user`/`count_unresolved` call site compiles with the
  new `exclude_types` arity).
- `cd web && npm ci` then `npx vitest run src/__tests__/Inbox.test.js
  src/__tests__/UserProfile.test.js` → **104/104 passed** (toggle-per-variant
  coverage asserted against the canonical enum list, not the implementation's
  own list; alias normalization; prefs round-trip; auth-provider display).
- Invariant scripts: `check-arch.sh`, `check-mem-port-contracts.sh`,
  `check-migration-versions.sh` (no duplicate Diesel versions;
  `2026-10-07-000056_user_auth_provider_info` is unique and its
  `ALTER TABLE users ADD COLUMN oidc_issuer TEXT` is SQLite+PG portable),
  `check-notification-toggle-coverage.sh` (new; 41/41 variants ↔ toggles ↔
  labels), `check-forged-scope-fields.sh`, `check-abac-exempt-handlers.sh`
  (80 handlers) → **all OK**.
- `scripts/check-tenant-filter.sh` fails with `awk: line 11: syntax error` in
  this sandbox — **environment, not a code defect**: the script uses gawk-only
  syntax and this sandbox provides mawk 1.3.4; the failure is a parse-time
  error identical at the assigned base, independent of the candidate's diff.
  Verifier should re-run it under gawk.

## Verified against each acceptance criterion

1. **Spec amendment** — HSI §12 now carries a Sources table naming the four
   concrete aggregation sources with attribution keys, and `?type=` values
   `approval|rejection|gate|trust|meta-spec` exactly matching
   `JudgmentType::as_str()`. `agent-gates.md` §Spec Approval Ledger amended to
   permit the Approved→Rejected decline-after-approval path (spec lifecycle
   used correctly: code and spec changed together, not around each other).
2. **`JudgmentType::GateOverride`** — `as_str() = "gate"`,
   `from_db_str("gate")` parses; explicit non-`Custom` parse-arm test for the
   three new `AuditEventType` variants.
3. **Aggregation** — sqlite `list_for_user` unions spec_approvals (LEFT JOIN
   spec_ledger_entries for workspace attribution, `?workspace_id=` applied as a
   SQL WHERE filter — the prior stamping behavior is gone and
   `judgment_ledger_workspace_filter_drops_unattributed` proves NULL-workspace
   entries are excluded) with audit_events rows (`event_type IN
   (gate_override, trust_change, meta_spec_publish)`, attributed via
   `user_id`), sorts reverse-chronologically with a deterministic
   entity_ref tie-break, and applies limit/offset after the merge. Postgres
   mirror implements the same query (SQL-shape tests pin the WHERE/JOIN
   predicates and the judgment-type list is asserted identical to sqlite's).
   The mem adapter aggregates the SAME shared stores (not a stub) with
   identical attribution/eligibility semantics. Compressed audit rows
   (`detail_zstd`) are decompressed via the shared `AuditEventRow` read path.
4. **Trust-change write path** — `PUT /workspaces/:id` writes a `trust_change`
   AuditEvent (from/to/workspace_id, `user_id` = acting human) only on a real
   transition; failure propagates. In-module test drives the real router with
   a signed JWT and asserts attribution + no-event-on-no-change.
5. **Gate-override write path** — a human (`auth.user_id` present) approval
   review on an MR with FAILED gate results writes one `gate_override` event
   per failed gate (mr_id/gate_id/gate_type/failed→overridden); agent-token
   approvals are excluded. In-module test covers both branches plus the
   `?type=gate` ledger surfacing.
6. **Meta-spec publish attribution** — registry create/update write
   `meta_spec_publish` events (kind/name/version/content_hash, workspace from
   scope/scope_id); registry DELETE additionally gained real per-handler
   authorization (scope-level admin containment) — verified by
   check-abac-exempt-handlers.
7. **Inbox filtering** — `exclude_types` is a port-trait parameter applied
   inside the SQL/queries before limit/offset; the inbox list, badge count,
   and the MCP `notifications://` tool all load disabled types from
   `user_notification_preferences` (no row = enabled); background dedup
   callers pass `&[]` per the documented exemption. PUT rejects unknown
   `notification_type` (400) and normalizes legacy parse aliases to canonical
   `as_str()` names before storing (an alias would be an inert toggle).
   Adapter test proves list+count exclusion; endpoint test proves handler
   wiring.
8. **Auth provider info** — `users.oidc_issuer` migration (unique version,
   portable SQL, honest down.sql); `UserRepository::record_login` implemented
   in sqlite (single-statement conditional update — no read-modify-write
   race), postgres, and mem with the same debounce + issuer-change carve-out
   contract; JWT validation records the verified issuer; `GET /users/me`
   returns `oidc_issuer`/`last_login_at`; `UpdateProfileRequest` has no field
   for either so they are not client-settable. Endpoint test drives a real
   signed JWT through validate_jwt and asserts both fields plus PUT
   round-trip leaving them untouched.
9/10. **`cargo test --all` / `cd web && npm test`** — see Verifier gates.

## Test-quality spot check

The judgment-ledger adapter tests seed real rows in real tables (foreign-user
rows, pending approvals, NULL-workspace attributions, passed+failed gates) and
assert exclusion semantics that a stamping or key-mismatch regression would
violate; the endpoint tests use real signed JWTs and the real router, and
assert the acting user's canonical Id — the writer/reader identity-key mismatch
class that earlier rounds caught is pinned by dedicated tests on both sides.
The web toggle test derives its expected set from the canonical enum list, not
the implementation's own list.

## Verifier gates (delegated per stage constraints)

Per the review-stage build budget, gyre-server test compilation was NOT run
here (cargo check only, clean). The verifier must execute:

- `SKIP_WEB_BUILD=1 cargo test --all` (acceptance criteria 9).
  Focused server tests of record, at minimum:
  `cargo test -p gyre-server --lib trust_transition_writes_trust_change_audit_event`,
  `... human_approval_on_failed_gate_writes_gate_override_event`,
  `... publish_writes_meta_spec_publish_audit_event`,
  `... disabled_preference_excludes_type_from_inbox_and_count`,
  `... preference_alias_normalizes_to_canonical_before_storing`,
  `... get_me_returns_recorded_auth_provider_info`,
  `... judgments_unknown_type_returns_400`.
- `cd web && npm test` (acceptance criterion 10; UserProfile + Inbox suites
  already green here, 104/104).
- `bash scripts/check-tenant-filter.sh` under gawk (mawk parse failure in this
  sandbox is environmental).
- `bash scripts/check-task-commit-attribution.sh` (needs full git history).

## Non-defect observations

- The working tree carries an uncommitted modification to
  `specs/tasks/task-209.md` restoring the assigned task text
  (progress `not-started`, empty `commits:`) — this matches the executor's
  canonical task text in `job.json` and is stage bookkeeping, not candidate
  content; the committed HEAD retains the attribution frontmatter.
- `web/dist/` regeneration is verifier-owned per stage policy; not reviewed.
