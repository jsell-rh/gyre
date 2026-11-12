# Review — task-209 (Finish HSI §12 profile: notification filtering, judgment ledger, auth provider info)

Spec: `specs/system/human-system-interface.md` §12 "What the Profile Is".
Candidate under review: `6876e8f56b4f47562cd1adb13351ded8f6d0482e` (base `199c1bb78`).
Verdict: **approved** — no findings.

## Round context

This round repairs the verification failure on candidate `c8354939` (checks.sh exit 1:
the changed-lines clippy gate reported 11 warnings on lines this branch added). Repair
commit `5418531` is the only product change since the prior approved lineage
(`c8354939`, reviewed under base `e402e1ed`); it is mechanical and behavior-neutral,
verified line-by-line below. All prior-round §12 work (notification filtering, judgment
ledger aggregation + write paths, auth provider info, spec amendment) is intact on
this tree and was re-probed independently.

## Probes run this round (logs in /tmp/stage/review-evidence/)

- `cargo clippy -p gyre-server -p gyre-adapters --all-features --tests -- -W clippy::all`
  → exit 0; **0 warnings on changed lines** (85 warnings in changed files, all on
  unchanged pre-existing lines — e.g. mem.rs `sort_by_key` hints at lines untouched by
  this branch). See `clippy-focused.json` + `clippy-changed-lines-analysis.txt`.
- `cargo clippy -p gyre-domain -p gyre-common -p gyre-ports -p gyre-cli --all-features --tests`
  → exit 0; 0 warnings on changed lines (`clippy-small-crates*.log`).
- `cargo test -p gyre-adapters user_profile` → **12/12 passed** (`adapters-user-profile-tests.log`):
  ledger merges 4 sources reverse-chron, workspace filter drops unattributed, `?type=`
  selects source, pagination across merged stream, rejection attributed to withdrawer,
  zstd detail decompression, notification exclusion list+count, record_login debounce
  incl. issuer-change carve-out, prefs roundtrip, token CRUD/scoping.
- `python3 scripts/check-rustfmt-diff.py 199c1bb78` → exit 0.
- `check-notification-toggle-coverage.sh` → exit 0 (41 variants ↔ 41 toggles ↔ labels).
- `check-arch.sh`, `check-abac-route-registry.sh`, `check-mem-port-contracts.sh`,
  `check-migration-versions.sh`, `check-task-commit-attribution.sh`,
  `check-fail-open-ref-resolution.sh`, `check-lossy-secret-conversion.sh`,
  `check-inert-enforcement.sh`, `check-scope-literal-defaults.sh`,
  `check-forwarded-header-trust.sh`, `check-unbounded-external-http.sh` → all exit 0.
- `check-tenant-filter.sh` → exit 2 (mawk-vs-gawk dialect error, identical failure
  class as prior rounds; sandbox restriction, not a code defect — deferred to a
  gawk-equipped verifier runner).

## Repair-commit behavior-neutrality (source-verified)

- `meta_specs.rs` `authorize_meta_spec_delete`: `state: &AppState`, `auth:
  &AuthenticatedAgent` are already references and `rbac::record_rbac_denied` takes
  `&AppState`/`&AuthenticatedAgent`, so the removed `&` at four call sites was a
  redundant double-reference the compiler auto-dereferenced — identity.
- `workspaces.rs`: `changed.then(|| (from, new_trust))` → `then_some((from,
  new_trust))`; both values already computed and owned, closure side-effect-free —
  unobservable.
- `mem.rs`: manual `Default` impl of three `Arc::default()` fields → `#[derive(Default)]`
  — identical.
- `sqlite/user_profile.rs` test: `.clone()` dropped on `Copy` type `JudgmentType` —
  identity.

## Acceptance criteria (re-verified on this tree)

- **Spec amendment** — HSI §12 Judgment Ledger carries the concrete Sources table
  (spec_approvals + spec_ledger_entries join; `audit_events` rows `event_type` in
  `gate_override`/`trust_change`/`meta_spec_publish` with human attribution via
  `user_id`, workspace via `workspace_id`) and `?type=` values
  `approval|rejection|gate|trust|meta-spec` matching `JudgmentType::as_str()`.
- **`JudgmentType::GateOverride`** — present, `as_str() = "gate"`,
  `from_db_str("gate")` parses; sqlite/postgres/mem parity.
- **Aggregation** — `list_for_user` (sqlite+postgres) filters `?workspace_id=` in SQL
  `WHERE` (`query.filter(...eq(w))`, sqlite/user_profile.rs:345,421), not a stamp;
  union → reverse-chron sort with tie-break → limit/offset after merge; per-source
  type filters; proven by the 12-test adapter suite above.
- **Trust write path** — PUT /workspaces/:id records `trust_change` only on actual
  transition, after atomic `apply_trust_transition`, errors propagate
  (workspaces.rs:332-368).
- **Gate override write path** — `submit_review` (merge_requests.rs:1030-1120):
  human (`auth.user_id.is_some()`) approval with failed gate results writes one
  `gate_override` per failed gate with mr_id/gate_id/gate_type/from→to; agent-token
  approvals excluded (negative control in test).
- **Meta-spec publish** — registry create/update call `record_meta_spec_publish`
  (meta_specs.rs:1952,2087) writing `meta_spec_publish` with kind/name/version/
  content_hash.
- **Notification filtering** — inbox list + count + MCP tool load disabled types from
  `user_notification_preferences` and pass `exclude_types` into the storage query
  before limit/offset; background de-dup callers exempted per amended spec.
- **Auth provider info** — `validate_jwt` records verified issuer + debounced
  `last_login_at` via single-statement `record_login` (auth.rs:1535, sqlite/user.rs);
  `GET /users/me` returns both; `PUT /users/me` has no field for either.
- **New migration** `2026-10-07-000056_user_auth_provider_info` (users.oidc_issuer,
  portable SQL; `last_login_at` column owned by task-120's 000058 to avoid duplicate
  column) follows the repo's existing date-differentiated version pattern
  (four sibling 000056 dirs pre-exist at base); `check-migration-versions.sh` passes.

## Deferred to the independent verifier

See `/tmp/stage/review-evidence/verifier-scope.log`:

- `python3 scripts/check-clippy-diff.py 199c1bb78` — the exact CI gate (its internal
  `cargo clippy --all-targets` is blocked by the review-stage build wrapper; the
  per-package `--tests` runs above are the closest permitted equivalent and show 0
  warnings on changed lines).
- Full `cargo test --all` (incl. gyre-server write-path tests:
  `trust_transition_writes_trust_change_audit_event`,
  `human_approval_on_failed_gate_writes_gate_override_event`,
  `publish_writes_meta_spec_publish_audit_event`, users.rs me/judgments tests).
- `cd web && npm test`.
- `check-tenant-filter.sh` and the gawk-dependent JS lint hooks on a gawk-equipped
  runner.

## Non-blocking observations

- Postgres judgment-ledger implementations are code-parity with SQLite, type-checked
  but not live-PG tested (no live-PG harness exists for this module in the repo);
  consistent with repo practice.
- Generated `web/dist/` is stale relative to authored source in this checkpoint —
  owned by the deterministic verifier, which rebuilds and binds publication to the
  tested tree; not a source defect.
