# Review — task-209 (Finish HSI §12 profile: notification filtering, judgment ledger, auth provider info)

Spec: `specs/system/human-system-interface.md` §12 "What the Profile Is" (row 53 of
`specs/coverage/system/human-system-interface.md`).
Candidate under review: `5c4a9119f18e5bab2d8c126c264e6a7febfbfced` (base
`2824d9b186d5da08ae043772762246ee5b620e89`).
Verdict: **approved** — no blocking findings. Independent evidence below.

This candidate is the end of a long repair chain (multiple merge-repair,
rustfmt, review-repair F1–F5, and CI-repair rounds recorded in the branch
history and task frontmatter). This review re-verified the final tree from
scratch against the task contract.

## Probes run (all evidence under /tmp/stage/review-evidence/)

- `SKIP_WEB_BUILD=1 cargo check -p gyre-server --lib --tests` → **exit 0**
  (7m32s cold; log `t209-srv-check.log`). This compiles every `list_for_user`
  call site including the merged `gate_executor.rs:6514` test whose 8-argument
  form was the E0061 Clippy/Test failure on PR head 746240e9 (durable CI
  finding) — the one-line `&[]` repair in commit 9a1ab601 matches the port's
  documented "must see every notification" contract for that delivery-asserting
  test.
- `cargo test -p gyre-adapters --lib user_profile` → **19/19 passed**
  (`t209-adt-userprofile-tests.log`): judgment-ledger merge/reverse-chron,
  workspace-filter-drops-unattributed (kills the old stamping behavior — with
  stamping it returns 6, the test asserts 4), type-filter source selection,
  pagination across the merged stream, rejection-attributed-to-withdrawer,
  compressed-detail decompression, notification exclude_types list+count
  (3→2 with one disabled — fails if the exclusion is removed), record_login
  debounce/issuer-carve-out, plus postgres SQL-shape parity tests.
- `cargo test -p gyre-domain --lib audit` → **9/9 passed**
  (`t209-domain-audit-tests.log`), including the explicit not-`Custom` parse
  assertions for `trust_change`/`gate_override`/`meta_spec_publish`.
- `cd web && npm ci` (exit 0) then `npx vitest run
  src/__tests__/UserProfile.test.js` → **30/30 passed**
  (`t209-userprofile-web-tests.log`).
- Invariant scripts, all exit 0 (logs retained): `check-arch`,
  `check-mem-port-contracts`, `check-migration-versions`,
  `check-notification-toggle-coverage` (new, wired into CI + pre-commit),
  `check-abac-route-registry`, `check-inert-enforcement`,
  `check-in-memory-state-stores`, `check-forged-scope-fields`.
- `scripts/check-tenant-filter.sh` fails in this sandbox with an mawk syntax
  error (`awk: line 11: syntax error`) — the script is byte-identical to base
  (last touched in a890c651, pre-task) and uses gawk-only syntax; this is an
  environment limitation, not a candidate defect. **Verifier must re-run it
  under gawk.** (The new judgment-ledger queries are user/workspace-scoped via
  `audit_events.user_id`/`workspace_id` and the `spec_ledger_entries` join, and
  `check-abac-route-registry` passes.)

## Acceptance-criteria verification

- **Spec amendment (Part 1)** — real. HSI §12 now carries a Sources table
  naming all four concrete sources with attribution keys, fixes `?type=` to
  `approval|rejection|gate|trust|meta-spec`, and documents the inbox-filter
  and auth-provider-info semantics. Matches the task's prescribed amendment.
- **`JudgmentType::GateOverride`** — added with `as_str()="gate"`,
  `from_db_str("gate")` parses (domain round-trip tests).
- **`list_for_user` aggregation** — sqlite/postgres/mem parity. All four
  categories from real tables: `spec_approvals` LEFT JOIN
  `spec_ledger_entries` (path is that table's primary key — no join fan-out)
  with `?workspace_id=` as a real SQL filter (not stamping);
  `audit_events` filtered on `user_id` + `event_type IN (gate_override,
  trust_change, meta_spec_publish)` read through the shared
  zstd-decompressing `AuditEventRow` materialization (compression-survival
  test proves it). Union → sort desc (deterministic entity_ref tie-break) →
  skip/take. Type filter narrows the physical sources consulted.
- **Trust-change write path** — `PUT /workspaces/:id` records a
  `trust_change` AuditEvent on transition only (same-value PUT writes
  nothing — asserted), attributed to the acting JWT user via `auth.user_id`,
  with from/to/workspace_id. Failure propagates (`?`).
- **Gate-override write path** — human (`auth.user_id.is_some()`) approval
  on an MR with FAILED gate results writes one `gate_override` event per
  failed gate with mr_id/gate_id/gate_type/from→to; agent-token approvals
  excluded (asserted). Ledger surfaces it via `?type=gate` (asserted
  end-to-end in the same test).
- **Meta-spec publish** — `record_meta_spec_publish` on registry create and
  update, attributed to `auth.user_id`, workspace from the meta-spec's own
  scope/scope_id (Global/Tenant honestly carry NULL workspace and are
  excluded by the `?workspace_id=` filter, per the amended spec).
- **Inbox filtering** — `exclude_types` added to the port, applied in SQL
  (`ne_all`) before limit/offset in sqlite+postgres, mirrored in mem; wired
  into the inbox list, the badge count, and the MCP `notifications://` tool;
  background de-dup callers pass `&[]` per the port doc. PUT normalizes
  legacy parse aliases to canonical `as_str()` names and 400s on unknown
  types (both tested — an alias-named row would otherwise be an inert
  toggle).
- **Auth provider info** — migration 2026-10-07-000056 adds
  `users.oidc_issuer` (next-unused sequence, no duplicate versions);
  `last_login_at` ownership ceded to mainline task-120's 000058 to avoid a
  duplicate-column collision (documented in the migration). JWT validation
  calls `UserRepository::record_login` (single-statement conditional update,
  issuer-change carve-out; sqlite/postgres/mem parity + debounce tests);
  `GET /users/me` returns both read-only; `PUT /users/me` has no field for
  either (verified against `UpdateProfileRequest`). Static-token/agent
  fallback returns nulls rather than fabricating values.
- **Task hygiene** — `check-task-commit-attribution` semantics respected
  (task-209.md frontmatter lists the branch commits; the working tree
  currently holds a harness-side reset of that file to `not-started`, which
  is not part of the reviewed candidate).

## Tests the independent verifier must execute (server targets were
type-checked, not executed, in this sandbox per the 4 GiB budget rule)

- `cargo test -p gyre-server --lib` (or at minimum):
  - `api::workspaces::tests::trust_transition_writes_trust_change_audit_event`
  - `api::merge_requests::tests::human_approval_on_failed_gate_writes_gate_override_event`
  - `api::users::tests::disabled_preference_excludes_type_from_inbox_and_count`
  - `api::users::tests::preference_alias_normalizes_to_canonical_before_storing`
  - `api::users::tests::get_me_returns_recorded_auth_provider_info`
  - `api::users::tests::judgments_unknown_type_returns_400`
  - `api::specs::tests` judgment-ledger surface tests
    (`*_surfaces_in_judgment_ledger`) and the unauthorized-approver test
- `cargo test --all` and `cd web && npm test` (full suites)
- `bash scripts/check-tenant-filter.sh` under gawk (mawk-incompatible at
  base, unchanged by this task)
- Clippy all-targets (the durable E0061 finding is compiler-verified fixed
  by `cargo check --tests`, but Clippy lints beyond compile errors are the
  verifier's gate)

## Generated-output observation (non-blocking, verifier-owned)

`web/dist/` regeneration commits exist on the branch
(`d1e30d9c`/`92931154`/…); freshness/reproducibility of the bundle is owned
by the deterministic verifier per pipeline policy and was not evaluated
here. The authored frontend source (`UserProfile.svelte`,
`notification-types.js`, `en.json`, `UserProfile.test.js`) was reviewed and
tested — 30/30 focused vitest tests pass after `npm ci`.

## Test-inflation check

None found. The adapter tests assert concrete row counts and field values
that invert if the behavior regresses (workspace filter 6→4, exclusion 3→2,
badge parity); the server tests drive the real router via `oneshot` with
real signed JWTs and assert against the audit/user stores, including
negative controls (agent-token approval writes nothing; no-change PUT
writes nothing; unknown `?type=` 400s).
