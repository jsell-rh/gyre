# Review — task-209 (Finish HSI §12 profile: notification filtering, judgment ledger, auth provider info)

Spec: `specs/system/human-system-interface.md` §12 "What the Profile Is".
Candidate under review: `1aba5d5e8314c13496e1bdf2677de88fd06df489` (branch
`pipeline/task-209/acb21473589d465fb4df80b5578a2d62-1`), assigned base `bfc36c9c`.
Verdict: **approved** — all ten acceptance criteria are satisfied by real production
code; independent probes (executed tests, mutation kill, mechanical invariants,
compiler-verified arities) support the task contract. Server-side test *execution*
is deferred to the independent verifier per the 4 GiB budget protocol; the exact
list is recorded below and in the task body.

## Probes run (all evidence under /tmp/stage/review-evidence/)

- `SKIP_WEB_BUILD=1 cargo test -p gyre-adapters --lib user_profile` → **19 passed,
  0 failed** (sqlite + postgres judgment-ledger, notification exclusion,
  record_login debounce/issuer-carve-out, tokens, channel prefs).
- `SKIP_WEB_BUILD=1 cargo test -p gyre-domain --lib` → **511 passed, 0 failed**
  (incl. Approved → Rejected domain transition, AuditEventType round-trips).
- `SKIP_WEB_BUILD=1 cargo test -p gyre-adapters --lib spec_approval` → 14 passed;
  `... notification` → 11 passed.
- `SKIP_WEB_BUILD=1 cargo check -p gyre-server --lib --tests` → **exit 0** (5m53s)
  — compiler-verified 9-arg `list_for_user` arity at every call site the rebase
  repair touched.
- **Mutation probe (test strength):** with the `?workspace_id=` SQL filter in
  `sqlite/user_profile.rs::list_for_user` disabled (the pre-task "stamp, don't
  filter" bug class), `judgment_ledger_workspace_filter_drops_unattributed`
  **fails** (5 ≠ 4); original restored byte-identical (`git status` clean, suite
  re-green). The filter tests are hard tests, not self-confirming.
- Mechanical invariants, all exit 0: `check-arch.sh`, `check-abac-route-registry.sh`,
  `check-abac-exempt-handlers.sh`, `check-migration-versions.sh`,
  `check-mem-port-contracts.sh`, `check-dead-message-kinds.sh`,
  `check-notification-toggle-coverage.sh` (new, wired into CI + pre-commit),
  `check-task-commit-attribution.sh`.
- `scripts/check-tenant-filter.sh` fails with an mawk 1.3.4 syntax error; the script
  is **byte-identical to the assigned base** (verified `git diff --quiet`), i.e. a
  pre-existing sandbox-environment incompatibility (CI runs gawk), not a code defect.
- `cd web && npm ci && npm test` → **113 files, 2315 passed, 41 skipped, 0 failed**.

## Verified against the acceptance criteria

1. **Spec amendment** — HSI §12 Judgment Ledger now names the four concrete sources
   (spec_approvals; audit_events rows `event_type IN (gate_override, trust_change,
   meta_spec_publish)` with `user_id`/`workspace_id` attribution columns — both
   pre-existing schema, verified in `schema.rs`; meta_spec publish via audit event);
   `?type=` values are `approval|rejection|gate|trust|meta-spec`, exactly the
   `JudgmentType` wire strings. `agent-gates.md` amended for the Approved → Rejected
   decline-after-approval path the rejection attribution requires.
2. **JudgmentType::GateOverride** — `as_str() = "gate"`, `from_db_str("gate")`
   parses; round-trip covered in domain tests.
3. **Aggregation** — `list_for_user` (sqlite/postgres/mem parity, mem wired to the
   shared stores in `lib.rs`) unions all four categories from real tables,
   filters `?workspace_id=` in SQL (LEFT JOIN spec_ledger_entries; NULL
   workspace drops when the filter is set), sorts reverse-chron with a
   deterministic entity_ref tie-break, applies limit/offset after the merge.
   Rejection attributed to the declining human (`rejected_by`/`revoked_by`),
   not the overridden approver; pending rows and foreign users excluded.
   Compressed `detail_zstd` audit rows decompress via the shared read path.
4. **Trust change write path** — PUT /workspaces/:id writes a `trust_change` audit
   event (from/to/workspace_id, acting user) on real transitions only; failure
   propagates. Test covers event + no-event-on-same-value.
5. **Gate override write path** — human (user-principal) approval review on an MR
   with failed gate results writes one `gate_override` event per failed gate
   (mr_id/gate_id/gate_type/failed→overridden); agent-token approvals excluded;
   end-to-end ledger surfacing via `?type=gate` asserted.
6. **Meta-spec publish attribution** — registry create/update write
   `meta_spec_publish` events (kind/name/version/content_hash, workspace from
   scope; Global/Tenant intentionally unattributed). Registry DELETE also gained
   per-handler authorization (route is ABAC-exempt).
7. **Inbox filtering** — `exclude_types` applied in-query before pagination in
   both `list_for_user` and `count_unresolved` (sqlite/postgres parity, mem
   parity); inbox, badge, and MCP inbox tool all load disabled types from
   `user_notification_preferences`; background de-dup callers pass `&[]`; PUT
   normalizes legacy aliases to canonical names and 400s unknown types.
8. **Auth provider info** — migration `2026-10-07-000056` adds `users.oidc_issuer`
   (single owner; `last_login_at` deliberately left to task-120's 000058 to avoid
   a duplicate-column collision — migration-version check passes); JWT validation
   records issuer + login via atomic single-statement debounced
   `UserRepository::record_login` (issuer change bypasses debounce);
   `GET /users/me` returns both read-only; `PUT /users/me` has no field for either.
9. **cargo test --all** — deferred to verifier (below); adapters + domain + web
   suites executed green in-sandbox; server lib+tests compile clean.
10. **web npm test** — green (above).

## Test quality

The new tests assert observable behavior through real surfaces (router oneshot
with real JWTs, direct DB round-trips) and include hard negative cases
(cross-user leakage, pending-row exclusion, agent-token exclusion, no-event on
no-change, alias-normalization, debounce window). The mutation probe confirms
the workspace-filter tests kill the exact pre-task bug class.

## Non-blocking observations

- One edited pre-existing test in `api/merge_requests.rs` now sets a duplicate
  `Authorization` header (same value both times) — harmless, no behavioral effect.
- A human re-approving the same MR writes a fresh gate_override event per failed
  gate per approval — each approval is a distinct judgment, so this is defensible.
- `submit_review`'s audit write can fail the request after the review row is
  stored (error propagation chosen over silent drop, documented in-code) — an
  acceptable consistency trade-off for an audit-must-not-be-dropped invariant.
- `web/dist/` regeneration is verifier-owned per protocol; not reviewed, not
  blocking.

## Server tests the independent verifier must execute (per protocol)

At candidate `1aba5d5e`: `cargo test -p gyre-server --lib` at minimum:
`trust_transition_writes_trust_change_audit_event` (api/workspaces.rs),
`human_approval_on_failed_gate_writes_gate_override_event` (api/merge_requests.rs),
`publish_writes_meta_spec_publish_audit_event` (api/meta_specs.rs),
`judgments_unknown_type_returns_400`, `disabled_preference_excludes_type_from_inbox_and_count`,
`get_me_returns_recorded_auth_provider_info`, `preference_alias_normalizes_to_canonical_before_storing`
(api/users.rs), `reject_transition_pending_to_rejected_and_audit` and the
Approved→Rejected gates tests (api/gates.rs), `approve_via_jwt_surfaces_in_judgment_ledger`,
`reject_via_jwt_surfaces_rejection_in_judgment_ledger`,
`agent_can_revoke_its_own_approval_with_agent_jwt`, `reject_approved_spec_records_declining_human`
(api/specs.rs), the judgment-ledger tests in mem.rs, plus the full standard gates
(`cargo test --all`, diff-scoped clippy, exact-head GitHub checks).
