# Review — task-204 (spec-lifecycle accountability patrol: task-age checks + orchestrator escalation)

Spec: `specs/system/spec-lifecycle.md` §Accountability Integration (lines 91–98): the Accountability agent's patrol checks for (1) `spec-drift-review` tasks open longer than one Ralph loop cycle, (2) `spec-implementation` tasks in Backlog more than N days, (3) modified specs with no corresponding task; findings escalate to the workspace orchestrator.

Candidate: `ab9fd7bd3566d1f260d5c061a737c90c5cfde2bd` (base `4d6759a0bfd637e899bbf65e725f91b07f745c69`), branch `pipeline/task-204/8e238019102747a2bebdf9cfd13537f7-1`.

Verdict: **complete** (approved, no findings).

## What was verified (independent evidence, logs under /tmp/stage/review-evidence/task204-*)

### Code inspection (diff 4d6759a0..ab9fd7bd)

- **All three checks are real** in `crates/gyre-server/src/spec_lifecycle_patrol.rs` (1048 lines incl. tests), against real storage: `TaskRepository::list()` / `list_by_spec_path()` and `SpecLedgerRepository::list_all()` (`state.spec_ledger`, the same ledger the post-receive hook writes). No in-memory stand-ins beyond the repo-standard `test_state()` mem-adapter test harness the task instructions prescribe.
- **Check 1** (`check_stale_drift_review_tasks`): non-terminal (`matches!(Done|Cancelled)` → skip) + `spec-drift-review` label + `now - created_at > drift_review_max_age_secs`. Terminal tasks never flagged.
- **Check 2** (`check_stale_implementation_backlog_tasks`): `status == Backlog` (excludes terminal by construction) + `spec-implementation` label + age threshold.
- **Check 3** (`check_modified_specs_without_tasks`): watched-path filter shared with the hook (`git_http::SPEC_WATCHED_PATHS`, now `pub(crate)`), skips `Deprecated`/no-`current_sha`/within-grace-window entries, keys the grace off ledger `updated_at` (per the contract's "not off the push itself"), resolves coverage via `list_by_spec_path` with both `system/x.md` and `specs/system/x.md` spellings, repo-scoped when both sides know the repo, `Cancelled` tasks do not count as coverage.
- **Thresholds are parameters**, defaults documented as consts (24 h / 7 d), overridable per request (`SpecLifecyclePatrolRequest`); not hardcoded in check bodies. Endpoint mirrors the `PatrolRequest.stale_threshold_secs` default pattern.
- **Escalation is real enforcement**: every finding → `state.emit_event(Some(ws), Destination::Workspace(ws), MessageKind::Escalation, payload)`. `emit_event` (lib.rs:495) signs, persists to the message store, and broadcasts; workspaceless findings → `Destination::Broadcast`, never a fabricated `"default"` scope. Payload carries `reason` (required by the message-bus `escalation` schema — validated in tests), `source`, `finding_type`, `severity`, `spec_path`, `task_id`, `message`, `suggested_action`.
- **Route + ABAC**: `POST /api/v1/patrol/spec-lifecycle` registered in `api/mod.rs` beside spec-links patrol; `RouteResourceMapping` in `abac_middleware.rs` (`spec` resource, POST→write) — real policy evaluation, NOT added to the frozen exemption file (the legacy spec-links route is exempt; the new one is stronger than "same gating as the existing patrol endpoint"). Separate handler; spec-links handler untouched.
- **Shared `PatrolFinding`** extended with `task_id`/`workspace_id` (`serde(default, skip_serializing_if)`) — backward-compatible; all existing spec_patrol constructors updated.
- **Scope discipline**: diff touches only the new module, the shared type's construction sites, route/resolver registration, the one-token visibility change in `git_http.rs`, docs/api-reference.md, coverage matrix, and task bookkeeping.

### Test runs (this checkout, /tmp/stage/bin/cargo)

- `cargo test -p gyre-server --lib spec_lifecycle_patrol` → **11 passed, 0 failed** (incl. `flags_and_escalates_only_the_accountability_gaps`, which asserts the exact flagged set AND 4 persisted escalations with identifying payloads AND schema-valid payloads; stale/fresh, Backlog/non-Backlog, orphaned/covered, cancelled-coverage, threshold-override, broadcast routing).
- `cargo test -p gyre-server --lib api::specs::tests` → **53 passed, 0 failed** (incl. both new endpoint tests through the real router).
- `cargo test -p gyre-server --lib spec_patrol` → **19 passed, 0 failed** (regression on the shared type).
- `cargo fmt --check -p gyre-server` → clean.

### Independent kill-tests (temporary mutants; tree restored and verified clean at the candidate SHA afterward)

- **Mutant A — escalation no-op** (`escalate_findings` skips `emit_event`): 3 tests fail (`flags_and_escalates_only_the_accountability_gaps` — "every finding must be escalated, not merely logged"; `spec_lifecycle_patrol_endpoint_flags_and_escalates` — "one escalation per finding"; `workspaceless_finding_broadcasts` — "escalation must be emitted, not skipped"). Log: task204-reviewer-killtest-escalation-noop.log.
- **Mutant B — check 3 no-op** (skip ledger scan): 2 tests fail (`flags_watched_specs_without_tasks` — "only the truly uncovered specs"; `flags_and_escalates_only_the_accountability_gaps` — "exactly the accountability gaps must be flagged"). Log: task204-reviewer-killtest-check3-noop.log.

### Live HTTP probe (this sandbox's listener is supported — capabilities.json; the prior round's Errno 95 no longer applies)

Built the debug server (`SKIP_WEB_BUILD=1`), ran it on port 39017 with a real SQLite database, and probed over real HTTP (logs: task204-live-http-probe.log, task204-live-escalation-probe.log):

- `GET /health` → 200.
- `POST /api/v1/patrol/spec-lifecycle` with **no token → 401**; **wrong token → 401**; valid token → **200** with `findings` array (auth middleware + ABAC resolver entry exercised end to end, not just the in-process router).
- Threshold body params accepted; `POST /api/v1/patrol/spec-links` still 200 (no regression).
- Seeded a workspace + `spec-drift-review` Backlog task **via the REST API** on the live server, ran the patrol with a 1 s drift threshold → response contained exactly one finding (`stale_drift_review_task`, correct `task_id`/`workspace_id`), and `GET /api/v1/workspaces/{id}/messages?kind=escalation` returned **1 persisted escalation** with `payload.finding_type = stale_drift_review_task`, `payload.task_id` matching, `reason` present, and `to = {workspace: <id>}`. Real enforcement over the deployed transport, not logged-only.

### Mechanical checks (all pass, logs under /tmp/stage/review-evidence/)

`check-arch.sh`, `check-abac-route-registry.sh`, `check-dead-message-kinds.sh`, `check-fabricated-scope-defaults.sh`, `check-scope-literal-defaults.sh`, `check-mem-port-contracts.sh`, `check-inert-enforcement.sh`, `check-task-commit-attribution.sh`.

### Coverage matrix

Row 8 (`spec-lifecycle.md §Accountability Integration`) `task-assigned` → `implemented`, with notes that match what this review independently verified (storage queries, default thresholds, both path spellings, repo scoping, escalation semantics, ABAC registration, 500-on-storage-failure). SUMMARY.md 80% → 90%. Accurate, not hollow.

## Findings

- (none)

Notes (not defects): `emit_event` is best-effort on persist failure (logs a warning) — the established repo-wide pattern for all event emissions; broadcast-destination messages are not persisted by design (repo-wide `emit_event` contract), and the workspaceless-finding test asserts dispatch on the broadcast channel instead.
