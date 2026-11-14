# Review — task-209 (Finish HSI §12 profile: notification filtering, judgment ledger, auth provider info)

Spec: `specs/system/human-system-interface.md` §12 "What the Profile Is".
Candidate under review: `c32a6637` (base `c844ea52`; diff includes task-209 work plus the
recorded merge resolutions against mainline).
Verdict: **approved** — no blocking findings. Server-side test execution is recorded below
as the verifier's gate (the review sandbox's build wrapper rejects gyre-server test
compilation by policy); everything independently executable here passed.

## Probes run (checkout at `c32a6637`)

Rust (via `/tmp/stage/bin/cargo`, `CARGO_TARGET_DIR=/tmp/stage/review-target`):

- `cargo test -p gyre-adapters --lib user_profile` → **19 passed** (judgment-ledger
  aggregation against real SQLite: 4-source merge, reverse-chron + deterministic
  tie-break, `?workspace_id=` SQL filter dropping unattributed rows, `?type=` source
  selection, limit/offset after merge, `detail_zstd` decompression through the shared
  `AuditEventRow` path, notification `exclude_types` on list+count, `record_login`
  debounce/issuer-carve-out/update-preservation).
- `cargo test -p gyre-adapters --lib` (full) → **538 passed, 0 failed**.
- `cargo test -p gyre-domain --lib` → **511 passed** (incl. `JudgmentType`/`AuditEventType`
  round-trips with the explicit not-`Custom` parse arms, and the `Approved → Rejected`
  decline-after-approval transition with mutual exclusivity + terminality).
- `SKIP_WEB_BUILD=1 cargo check -p gyre-server --lib --tests` → clean (type-checks every
  server write-path/endpoint test listed below).

Web (`npm ci` first, locked versions):

- `npx vitest run src/__tests__/UserProfile.test.js` → **30/30 passed**.
- `npm test` (full) → **113 files, 2315 passed, 41 skipped**.

Mechanical gates (all OK): `check-arch.sh`, `check-migration-versions.sh`,
`check-abac-route-registry.sh`, `check-abac-exempt-handlers.sh`, `check-mcp-write-tools.sh`,
`check-mem-port-contracts.sh`, `check-in-memory-state-stores.sh`, `check-dead-message-kinds.sh`,
`check-forged-scope-fields.sh`, `check-forwarded-header-trust.sh`, `check-scope-literal-defaults.sh`,
`check-fabricated-scope-defaults.sh`, `check-lossy-secret-conversion.sh`,
`check-inert-enforcement.sh`, `check-unbounded-external-http.sh`,
`check-relative-path-defaults.sh`, and the new `check-notification-toggle-coverage.sh`
(41 enum variants ↔ 41 toggles ↔ locale labels). `check-task-commit-attribution.sh` → OK
against the candidate commit (all 33 frontmatter SHAs are ancestors of `c32a6637`; the
working-tree FAIL observed during review came from the stage controller's uncommitted
revert of `specs/tasks/task-209.md` to `not-started`/`commits: []`, which is not candidate
content).

## Verified against the task contract

- **Part 1 (spec amendment)** — HSI §12 Judgment Ledger now carries a concrete Sources
  table (`spec_approvals` joined to `spec_ledger_entries.workspace_id`; `audit_events` rows
  with `event_type IN (gate_override, trust_change, meta_spec_publish)` attributed via
  `audit_events.user_id`), documents why `agent_id`/KeyBinding labels are not the human
  attribution key, fixes `?type=` to `approval|rejection|gate|trust|meta-spec` exactly
  matching `JudgmentType::as_str()`, and specifies filter-before-pagination semantics for
  the inbox + badge. `agent-gates.md` §Spec Approval Ledger amended for the
  `Approved → Rejected` decline-after-approval path with rationale tied to HSI §12 — a
  spec-lifecycle amendment, not a contradiction smuggled into code.
- **`JudgmentType::GateOverride`** — `as_str() = "gate"`, `from_db_str("gate")` parses;
  domain round-trip test executed green.
- **Part 2 (aggregation)** — sqlite `list_for_user` unions all four categories with the
  workspace filter applied in SQL (`spec_ledger_entries.workspace_id.eq(w)` on a LEFT JOIN
  whose right side is keyed by `path` PRIMARY KEY — no fan-out; NULL-workspace rows drop
  under a set filter, matching the amended spec), sorts reverse-chronologically with an
  `entity_ref` tie-break, and applies limit/offset after the merge. Postgres mirrors it
  (including a cross-backend `JUDGMENT_AUDIT_TYPES` parity test); `mem.rs` got a real
  parity implementation over shared stores (wired in `lib.rs` and `mem::test_state` via
  `with_sources`, so handler tests read what the write paths wrote). Rejection attribution
  (the declining human, not the overridden approver) is implemented and tested at all
  three tiers.
- **Part 3 (write paths)** — trust transition (`PUT /workspaces/:id`) writes a
  `trust_change` audit event after `apply_trust_transition` succeeds, only on real
  transitions, failure propagating; human approval on an MR with FAILED gate results
  writes one `gate_override` event per failed gate (agent-token approvals excluded via
  `auth.user_id.is_some()`), carrying mr_id/gate_id/gate_type/failed→overridden;
  meta-spec registry create/update write `meta_spec_publish` events with workspace
  attribution derived from `ms.scope`/`scope_id`. `AuditEventType` gained the three
  variants with explicit parse arms (guarded against `Custom` fallback).
- **Notification filtering** — `NotificationRepository::list_for_user`/`count_unresolved`
  take `exclude_types`, applied in-query before limit/offset (sqlite/postgres/mem
  parity); the inbox, badge, and MCP `notifications://` tool load disabled types from
  `user_notification_preferences` (no row = enabled); background de-dup callers pass `&[]`
  per the documented exemption; PUT normalizes legacy parse aliases to canonical
  `as_str()` names and 400s on unknown types (an alias-stored toggle would be inert —
  tested). The new `check-notification-toggle-coverage.sh` (pre-commit + CI) keeps the
  UI toggle list in lockstep with the Rust enum and locale labels.
- **Auth provider info** — `users.oidc_issuer` added by migration
  `2026-10-07-000056_user_auth_provider_info` (portable SQL, single owner of the column;
  `last_login_at` correctly left to mainline task-120's `2026-10-08-000058` to avoid a
  duplicate-column collision — the merge resolution is documented in the migration
  header); `record_login` implements the debounce + issuer-change carve-out in ONE
  statement (no read-modify-write race) in sqlite/postgres/mem; JWT validation calls it
  with a warn-only failure path (auth must not depend on bookkeeping); `GET /users/me`
  returns both read-only; `PUT /users/me` has no field for either.
- **Attribution-key integrity** — the approve/reject/revoke endpoints write the canonical
  user Id into `spec_approvals.approver_id`/`rejected_by`/`revoked_by` (KeyBinding labels
  stay on the history events where the contract requires them), and the
  `*_surfaces_in_judgment_ledger` server tests pin writer/reader key equality end-to-end.
  The agent-vs-user JWT label derivation (`scope` as a non-empty array) fixes a real
  self-revoke lockout class and is documented in-line.
- **UI** — `UserProfile.svelte` renders server-side ledger filtering (`?type=`,
  `?workspace_id=`, Load More via offset), the badge from the count endpoint (not the
  current page), read-only issuer + last-login, and toggles driven by the canonical type
  list; `notification-types.js` + locale labels are mechanically enforced against the
  Rust enum. 30 component tests + full suite green.
- **Coverage matrix** — row 53 flipped to `implemented` with per-gap landed evidence and
  honest scoping of what was NOT executed in-sandbox (server test execution); counts are
  internally consistent (21/35 = 14 implemented + 7 verified of 35 non-n/a).

## Test-inflation check

None found. The adapter tests assert cross-user isolation, unattributed-row exclusion,
per-source type selection, pagination across the merged stream, and decompression of
aged rows — each fails if the corresponding production behavior regresses (e.g. reverting
to workspace-stamping fails `judgment_ledger_workspace_filter_drops_unattributed`;
reading raw `detail` fails `judgment_ledger_decompresses_compressed_audit_details`). The
server tests (not executable here) drive the real router with real JWTs and assert both
the written rows and the ledger/`/users/me` reads; they compile clean under
`cargo check -p gyre-server --tests`.

## Verifier must execute (review sandbox blocked by the build wrapper)

Full: `cargo test --all`, `cd web && npm ci && npm test` (already green here), plus the
workspace-wide script suite. Focused server lib tests for this task's write paths:

```
cargo test -p gyre-server --lib -- \
  trust_transition_writes_trust_change_audit_event \
  human_approval_on_failed_gate_writes_gate_override_event \
  publish_writes_meta_spec_publish_audit_event \
  disabled_preference_excludes_type_from_inbox_and_count \
  preference_alias_normalizes_to_canonical_before_storing \
  get_me_returns_recorded_auth_provider_info \
  judgments_unknown_type_returns_400 \
  approve_via_jwt_surfaces_in_judgment_ledger \
  revoke_via_jwt_surfaces_rejection_in_judgment_ledger \
  reject_via_jwt_surfaces_rejection_in_judgment_ledger
```

(vitest filter strings are OR'd by libtest; if the local runner requires a single
substring, run them individually.)

## Observations (non-blocking)

- **Generated output:** `web/dist` is byte-identical between base and candidate. Per
  checkpoint policy the deterministic verifier rebuilds and binds publication to the
  tested tree; the authored frontend source and tests are what was reviewed. Not a
  finding.
- `scripts/check-tenant-filter.sh` emits an awk syntax error in this sandbox (mawk
  dialect) but exits 0; the script is unchanged by this candidate, and the new
  `audit_events` queries are `user_id`-scoped with an optional `workspace_id` equality —
  tighter than the surrounding baseline.
- The stage controller's uncommitted working-tree revert of `specs/tasks/task-209.md`
  (`not-started`, `commits: []`) is not candidate content; attribution was verified
  against the committed file at `c32a6637` via a temporary worktree (removed after).
- A second approval review on the same failed-gate MR would write a further
  `gate_override` event per still-failed gate (each approval is arguably its own
  judgment); no dedup was specced. Cosmetic at most.
