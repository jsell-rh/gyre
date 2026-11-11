# Review — task-209 (Finish HSI §12 profile: notification filtering, judgment ledger, auth provider info)

Spec: `specs/system/human-system-interface.md` §12 "What the Profile Is".
Assignment: base `cfeefbda9`, candidate `aa156fb23` (remerged/rebased lineage; product commits
`c7fd768b2`, `fa67c4cea`, `daf5eec53`, `7aff9744e`, `94af4aef3`, `ca1ac78e4`, `f7f3b0810` —
all recorded in the task's `commits:` frontmatter at the candidate commit).
Verdict: **approved** — no findings.

## Probes run (independent, on the candidate checkout)

- `cargo test -p gyre-adapters --lib sqlite::user_profile` → **12 passed / 0 failed**
  (`/tmp/stage/review-evidence/cargo-test-gyre-adapters-user_profile.log`). Covers: ledger
  merge of all four categories from real seeded SQLite tables, reverse-chron across the merged
  stream, `?workspace_id=` as a real SQL filter (unattributed entries dropped — the old
  stamp-not-filter behavior would fail this test), pagination after union+sort,
  rejection-attributed-to-withdrawer, zstd detail decompression, notification exclusion on
  both list and count, `record_login` debounce incl. the issuer-change carve-out.
- `cargo test -p gyre-domain --lib` → **463 passed / 0 failed** (includes the new
  `AuditEventType` named-variant parse assertions and `JudgmentType` round-trip).
- `cargo check -p gyre-server` and `cargo check -p gyre-server --tests` → exit 0 (server and
  its test code type-check; gyre-server test *execution* deferred to the verifier per the
  4 GiB budget policy — see below).
- `cd web && npm ci && npm test` → **104 files, 2081 passed / 41 skipped** (UserProfile tests
  assert real wire shapes: `judgment_type`/`entity_ref`, server-side badge count, 39 toggles).
- Mechanical scripts, all exit 0: `check-arch`, `check-mem-port-contracts`,
  `check-abac-route-registry`, `check-migration-versions`, `check-migration-sql-portability`,
  `check-forged-scope-fields`, `check-scope-literal-defaults`, `check-lossy-secret-conversion`,
  `check-inert-enforcement`. `check-task-commit-attribution.sh` → OK when run against the
  candidate commit in a temp worktree (the main checkout's unstaged `specs/tasks/task-209.md`
  is the harness's assignment-input reset — `commits: []` — not candidate content).
- `check-tenant-filter.sh` exits 2 with a mawk syntax error **identically on base `cfeefbda9`
  and on the candidate** (reproduced in both worktrees) — sandbox awk limitation, not a branch
  defect. The new queries were reviewed manually instead: judgment-ledger reads are scoped by
  `audit_events.user_id` / `spec_approvals.approver_id|rejected_by|revoked_by` (all derived
  from the auth extractor via `resolve_user_id`, never client input) with workspace filtering
  in SQL.
- Set-identity check: the Svelte `NOTIF_TYPE_IDS` toggle list (39) is identical to
  `gyre_common::NotificationType`'s 39 variants, and all 39 `user_profile.notif_types.*`
  labels exist in `en.json`.

## Verified working (code + probe evidence)

- **Part 1 (spec amendment)** — HSI §12 now carries a concrete Sources table (category →
  table → human-attribution key → workspace-attribution key), pins `?type=` to
  `approval|rejection|gate|trust|meta-spec` (exactly the `JudgmentType` wire strings, and
  `from_db_str` accepts all five), and specifies when each event is written. The amendment
  names real, existing write paths — no fake-completion.
- **Part 3 (write paths)** — all three are real production writes with failure propagation:
  `PUT /workspaces/:id` writes `trust_change` on the successful transition only (same-value
  PUT writes nothing); human MR approval with failed gate results writes one `gate_override`
  event per failed gate (agent-token approvals correctly excluded — agent JWTs construct
  `AuthenticatedAgent` with `user_id: None`, verified in `auth.rs`); meta-spec registry
  create/update write `meta_spec_publish` with kind/name/version/content_hash and workspace
  attribution from the meta-spec's own scope.
- **Part 2 (aggregation)** — `list_for_user` in SQLite, Postgres (parity verified by
  side-by-side diff of the filter logic) and mem (shares the same stores the repos write
  through, wired in both `lib.rs` and `test_state_inner`) unions all four categories,
  filters workspace in SQL, sorts reverse-chronologically with a deterministic tie-break,
  applies limit/offset after the merge. `JudgmentType::GateOverride` with `as_str() == "gate"`
  parses via `from_db_str("gate")`.
- **Auth provider info** — `users.oidc_issuer` migration (`2026-10-07-000056`, no duplicate
  version per `check-migration-versions.sh`; `last_login_at` column owned by pre-existing
  task-120 migration — no collision); JWT validation records issuer + login via a
  single-statement conditional `UPDATE` (no read-modify-write race, debounced, issuer change
  never suppressed); `GET /users/me` returns both read-only; `PUT /users/me` has no field for
  either; API-key-only users get `null` for both.
- **Notification filtering** — `exclude_types` applied in-query before limit/offset in all
  three adapters; inbox list, badge count, and the MCP inbox tool all load disabled types
  from `user_notification_preferences` (no row = enabled); background de-dup callers pass
  `&[]`; PUT normalizes legacy aliases to canonical names and 400s on unknown types.
- Web tests use real wire shapes and would fail if the production behavior regressed (e.g.
  the badge test mocks `notificationCount` = 7 against a 2-item list so a client-side
  `list.filter` badge would fail).

## Server tests the independent verifier must execute

(deferred from this review per the build-budget policy; the corresponding test code
type-checks via `cargo check -p gyre-server --tests`)

- `cargo test -p gyre-server trust_transition_writes_trust_change_audit_event`
- `cargo test -p gyre-server human_approval_on_failed_gate_writes_gate_override_event`
- `cargo test -p gyre-server publish_writes_meta_spec_publish_audit_event`
- `cargo test -p gyre-server users::` (disabled-preference inbox/count exclusion, alias
  normalization, `get_me` auth-provider info via real JWT)
- `cargo test -p gyre-server specs::` (approve/reject/revoke → judgment-ledger attribution)
- Full `cargo test --all`
- Publication check (executor policy, per the shipped notes): the remotely rebuilt
  `web/dist` bundle must contain the 39 canonical toggle ids and no
  `["SpecApproval","GateOverride",...]` 10-entry list. Source-level correctness is verified
  in this review; the committed dist is intentionally left at the assignment-base state.

## Non-blocking observations

- `GET /users/me/judgments` with an unrecognized `?type=` value silently drops the filter
  rather than 400ing (pre-existing endpoint behavior; the spec pins the accepted values but
  does not require rejection). Not a defect for this task.
- One pre-existing test in `merge_requests.rs` now sets the `authorization` header twice
  (harmless duplicate line introduced in a call-site adaptation).
