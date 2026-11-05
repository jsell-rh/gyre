# Review — task-204 (spec-lifecycle §Accountability Integration — accountability patrol + orchestrator escalation)

Spec: `specs/system/spec-lifecycle.md` §Accountability Integration (lines 91-98).
Candidate: `9e71a7fe` (base `110180fb`). Verdict: **approved**.

This was a resumed review after a model timeout; the interrupted run's saved
evidence (`/tmp/stage/review-resume/review-evidence/`) was checked for
relevance (same candidate SHA, same pristine `spec_lifecycle_patrol.rs` hash
`46b13f91…`) and then every material claim was re-probed independently on the
exact candidate in this round (logs under `/tmp/stage/review-evidence/`).

## Independent probes (this round, exact candidate `9e71a7fe`)

- `cargo test -p gyre-server --lib spec_lifecycle_patrol` → **11 passed, 0
  failed** (cold build; includes both endpoint tests
  `spec_lifecycle_patrol_endpoint_flags_and_escalates` and
  `..._honours_thresholds`, which drive the real router via `oneshot` with the
  auth header and assert persisted escalations in the message store).
  `task204-review-patrol-tests.log`, `task204-review-clean-rerun.log`.
- `cargo test -p gyre-server --lib spec_patrol` → **19 passed, 0 failed**
  (regression: shared `PatrolFinding` gained optional
  `task_id`/`workspace_id`, serde-skipped when `None`, so existing consumers
  are unaffected). `task204-review-spec-patrol-regression.log`.
- `cargo test -p gyre-server --lib api::specs::tests` → **53 passed, 0
  failed**. `task204-review-api-specs-tests.log`.
- **Kill-test (re-done independently this round):** stubbed
  `escalate_findings` with `if true { return 0; }` → exactly 3 tests fail
  (`flags_and_escalates_only_the_accountability_gaps`,
  `spec_lifecycle_patrol_endpoint_flags_and_escalates`,
  `workspaceless_finding_broadcasts`), exit 101 — the escalation assertions
  are real enforcement checks, not theater. Mutant removed; file hash
  restored to pristine `46b13f91…` (verified); clean re-run 11 passed.
  `task204-killtest-mutant.log`, `task204-killtest-pristine-sha.txt`.
- All 18 mechanical invariants re-run on this tree, all pass: `check-arch`,
  `check-abac-route-registry`, `check-dead-message-kinds`,
  `check-fabricated-scope-defaults`, `check-scope-literal-defaults`,
  `check-inert-enforcement`, `check-abac-exempt-handlers`,
  `check-task-commit-attribution`, `check-byte-slice-truncation`,
  `check-fail-open-ref-resolution`, `check-forwarded-header-trust`,
  `check-lossy-secret-conversion`, `check-mem-port-contracts`,
  `check-mcp-write-tools`, `check-migration-sql-portability`,
  `check-migration-versions`, `check-relative-path-defaults`,
  `check-unbounded-external-http`, `check-forged-scope-fields`
  (`task204-rerun-*.log`).

## Verified against the task contract

- **All three checks are real and against real storage.** Check 1 flags
  non-terminal (`≠ Done/Cancelled`) `spec-drift-review` tasks past
  `drift_review_max_age_secs` (default 24 h, documented as the loop-cycle
  proxy); check 2 flags `spec-implementation` + `status == Backlog` past
  `implementation_backlog_max_age_secs` (default 7 d); check 3 (defense in
  depth) flags watched-path ledger entries (`SPEC_WATCHED_PATHS`, made
  `pub(crate)` and shared with the hook so patrol and hook cannot disagree)
  that have a `current_sha`, are not `Deprecated`, are past the hook grace
  window, and have no non-`Cancelled` task via
  `TaskRepository::list_by_spec_path` — with both path spellings
  (`system/x.md` and `specs/system/x.md`) normalized and repo-scoped when both
  sides know the repo (an identically-named spec in another repo does not mask
  the gap; covered by a dedicated test). Labels match the hook's
  `classify_spec_change` exactly. Storage failures propagate as `Err` → 500,
  never an empty finding list.
- **Thresholds are parameters, not magic numbers** in the check bodies;
  request-body override is honoured and tested in both directions
  (tightening changes what is flagged; loosening silences it) — task-109
  config-feed-ready.
- **Escalation is real enforcement.** Every finding →
  `state.emit_event(Some(ws), Destination::Workspace(ws),
  MessageKind::Escalation, payload)`; `emit_event` persists non-Broadcast
  messages through `messages.store` and the tests assert the persisted rows
  via `list_by_workspace(..., Some("escalation"), ...)`, including payload
  identity (`finding_type`, `task_id`, `source`) and schema validity
  (`MessageKind::Escalation::validate_payload` — `reason` required). Findings
  with no resolvable workspace broadcast; the routing test asserts nothing is
  persisted under a fabricated `""`/`"default"` scope.
- **Route + gating.** `POST /api/v1/patrol/spec-lifecycle` registered in
  `api/mod.rs` beside spec-links patrol, with a real ABAC
  `RouteResourceMapping` (`spec` resource, POST → `write` action) — *stronger*
  than the legacy spec-links route, which sits in the frozen exemption file;
  the new route correctly does not. `check-abac-route-registry.sh` and
  `check-abac-exempt-handlers.sh` both pass.
- **Coverage matrix** row 8 (`spec-lifecycle.md §Accountability Integration`)
  updated `task-assigned` → `implemented` with an evidence-bearing note;
  SUMMARY counts updated consistently (13 sections: 9 covered).

## Deferred to verification (not defects)

- `cargo test --all` (workspace-wide) — the review executor's wrapper rejects
  workspace-wide invocations by design; the focused `-p gyre-server` suites
  above and `check-arch.sh` pass on this exact tree.
- Live HTTP transport probe — the route is exercised through the real
  `api_router()` in-process (with auth header) rather than a socket; an
  exact-head deployed-transport check remains with CI/verification.
