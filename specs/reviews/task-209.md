# Review — task-209 (Finish HSI §12 profile: notification filtering, judgment ledger, auth provider info)

Spec: `specs/system/human-system-interface.md` §12 "What the Profile Is" (coverage row 53).
Candidate: `fd087b4c8da3c5bc4f0132afcd10fa8445cec9bd` (base `42d894f8d62f35dc9f9583b95a99af1c23bc4933`).
Product commits: c7fd768b2, fa67c4cea, daf5eec53, 7aff9744e, 94af4aef3, ca1ac78e4 (+ f7f3b081 from an earlier round) — all recorded in the committed task-209.md frontmatter.
Verdict: **approved** — all acceptance criteria satisfied by production code with real-query evidence; one publication-stage requirement recorded below (not a candidate defect).

Probes (full log: `/tmp/stage/review-evidence/probe-log.md`):

- `cargo test -p gyre-adapters --lib sqlite::user_profile` → 12/12 passed (real SQLite).
- Mutation probe: disabling the `?workspace_id=` SQL filter in `list_for_user` makes
  `judgment_ledger_workspace_filter_drops_unattributed` FAIL (got 6, expected 4) — the tests kill
  the specced behavior, they are not self-confirming. File restored byte-identical.
- `cargo test -p gyre-domain --lib` → 463 passed. `cargo check -p gyre-server --tests` → exit 0.
- `cd web && npm ci && npm test` → 2079 passed / 41 skipped, exit 0. `npm run build` → exit 0.
- Mechanical: check-arch, migration-versions, migration-sql-portability, abac-route-registry,
  abac-exempt-handlers, mem-port-contracts, dead-message-kinds, inert-enforcement,
  byte-slice-truncation, fabricated/scope-literal-defaults — all clean or pre-existing-baseline
  only (unnamed-tuple-carriers: same 4 errors on a base worktree; tenant-filter script's awk
  failure is a mawk incompatibility present on base).

## Verified working

- **Part 1 (spec amendment)**: HSI §12 now names the four concrete aggregation sources with
  attribution columns (spec_approvals + spec_ledger join; audit_events rows with
  event_type gate_override/trust_change/meta_spec_publish attributed via user_id),
  `?type=` values fixed to the exact `JudgmentType` wire strings, inbox-filtering semantics
  spelled out (in-query, badge parity, background de-dup exempt, PUT 400 on unknown type),
  auth-provider info specified read-only with recording semantics. Amendment precedes the
  implementation in the same lineage.
- **Part 2 (aggregation)**: `list_for_user` (SQLite/Postgres/mem parity, mem sharing the same
  stores the write paths use) unions spec approvals/rejections (LEFT JOIN spec_ledger for
  workspace attribution; rejection attributed to the declining human via rejected_by/revoked_by,
  verified end-to-end by specs.rs JWT tests) with the three audit event types, sorts
  reverse-chronologically with a deterministic tie-break, and applies limit/offset after the
  merge. `?workspace_id=` is a real SQL filter (mutation-killed); audit rows are materialized
  through the shared `AuditEventRow::into_event` so detail_zstd-compressed rows (the 7-day
  compression job) decompress transparently — covered by a dedicated adapter test that runs the
  real compression pass first. Postgres stubs (`bail!`) for the user-profile ports were replaced
  with real implementations (parity repair beyond the task minimum).
- **Part 3 (write paths)**: trust transition (PUT /workspaces/:id) writes `trust_change` only on
  a real transition after `apply_trust_transition` succeeds; human approval on an MR with failed
  gate results writes one `gate_override` per failed gate (agent-token approvals excluded —
  tested); meta-spec registry create/update write `meta_spec_publish` with kind/name/version/
  content_hash and workspace attribution from scope. All three propagate record errors. The
  meta-spec DELETE handler additionally gained real per-handler authorization (tenant containment,
  workspace Owner/Admin vs tenant Admin by scope, 403-before-404 for unprivileged callers).
- **Notification filtering**: `exclude_types` applied inside list_for_user/count_unresolved
  before pagination (SQL `ne_all`; mem mirror), wired into the REST inbox, the badge count, and
  the MCP notifications:// tool; background de-dup callers pass `&[]` per the amended spec. PUT
  normalizes legacy parse aliases to canonical `as_str()` names before storing and 400s unknown
  names; the UI exposes exactly the 39 canonical `NotificationType` variants (set-equality
  checked against the enum; 39/39 en.json label keys).
- **Auth provider info**: migration 2026-10-07-000056 adds `users.oidc_issuer` (portable SQL;
  `last_login_at` owned by task-120's 000058 — duplicate-column collision correctly resolved);
  `UserRepository::record_login` (sqlite/pg/mem parity) is a single-statement debounced write
  with an issuer-change carve-out (no read-modify-write race; debounce behavior + issuer-change
  bypass + update() preservation all adapter-tested); `validate_jwt` records the verified `iss`;
  GET /users/me returns both read-only; PUT /users/me has no field for either and preserves them
  (server test with a real signed JWT).
- **Prior-round durable findings all repaired and verified**: alias toggle ids → canonical +
  server-side normalization + round-trip test; detail_zstd bypass → shared decompressing read
  path + compression adapter test; 10-tuple carriers → named `SpecApprovalJudgmentRow` structs
  (tuple-carrier lint back to pre-existing baseline); toggle subset → all canonical variants;
  stale dist → see below.
- **Security/scope**: `submit_review` now requires authentication (was callable unauthenticated;
  the internal gate_executor path uses the repository directly and is unaffected); the route
  keeps its ABAC resolver entry; judgment ledger is caller-scoped with foreign-user isolation
  asserted in adapter tests; no fabricated scopes, no client-settable provenance fields.

## Non-blocking observations

- `GET /users/me/judgments` silently ignores an unrecognized `?type=` value (returns the
  unfiltered ledger rather than 400). The spec does not require rejection; cosmetic only.
- `scripts/check-tenant-filter.sh` errors under mawk — pre-existing (script unchanged from base).
- `check-task-commit-attribution.sh` fails only against the executor's working-tree injection of
  task-209.md (`commits: []`); the committed candidate records every in-range product commit.

## Publication-stage requirement (not a candidate defect — recorded so it cannot be dropped)

The committed `web/dist` equals the base's dist (controller regen 0eb44399c, built from a
mainline state without task-209's web/src) and therefore still contains the old fabricated
notification-toggle list and no last-login display. Per this pipeline's division of labor the
review guard excludes `web/dist/` from source integrity (controller-managed), and every
`build(web): regenerate verified assets (task-NNN)` commit in history is controller-side — a
candidate-side dist commit would be discarded by the executor's restore. The candidate's web/src
builds cleanly to a bundle with all required changes (verified here: fresh build =
index-BtTOWNlS.js, canonical 39-toggle list, last_login display, judgment wire shape).

**Publication MUST rebuild dist from the final merged source and verify**: 0 hits for
`MetaSpecEdit` / `MergeRequestReview` in `web/dist/assets/index-*.js`, nonzero hits for
`PersonaChangePendingApproval` and `last_login_at`. If those greps fail at the published head,
the regen step was skipped — not the source.

## Verifier must execute (gyre-server test compilation excluded from the review sandbox per the 4 GiB budget rule)

`cargo test --all` (goal AC); at minimum the focused names:
`api::users::tests::{disabled_preference_excludes_type_from_inbox_and_count,
preference_alias_normalizes_to_canonical_before_storing, get_me_returns_recorded_auth_provider_info}`,
`api::workspaces::tests::trust_transition_writes_trust_change_audit_event`,
`api::merge_requests::tests::human_approval_on_failed_gate_writes_gate_override_event`,
`api::meta_specs::registry_tests::publish_writes_meta_spec_publish_audit_event`,
`api::specs::tests::{approve,revoke,reject}_via_jwt_surfaces_*_in_judgment_ledger`.
All compile (cargo check --tests, exit 0); the equivalent adapter-layer behavior is green here.

Test-inflation check: none found. The adapter tests assert against real seeded SQLite rows
(foreign-user rows, pending approvals, compressed audit rows all present as negative controls);
the mutation probe confirmed the workspace-filter test fails when the behavior is disabled; the
web badge test distinguishes server-side count from client-side filtering by mocking divergent
values; the alias-normalization test drives the real router.
