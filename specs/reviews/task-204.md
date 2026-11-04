# Review — task-204 (spec-lifecycle §Accountability Integration: task-age checks + orchestrator escalation)

Spec: `specs/system/spec-lifecycle.md` §Accountability Integration. Candidate: `8666c899` (base `63aa1486`). Verdict: **approved**.

## What was verified

Diff scope (`63aa1486..8666c899`): new `crates/gyre-server/src/spec_lifecycle_patrol.rs` (1050 lines incl. tests), additive `task_id`/`workspace_id` fields on the shared `spec_patrol::PatrolFinding`, `POST /api/v1/patrol/spec-lifecycle` handler + route registration + ABAC `RouteResourceMapping`, `SPEC_WATCHED_PATHS` visibility change in `git_http.rs` (const visibility only, value unchanged), docs/coverage/task-file updates, rebuilt web bundle.

- **All three checks are real and keyed correctly.** `run_spec_lifecycle_patrol` queries `TaskRepository::list()` and `SpecLedgerRepository::list_all()` through the port traits — implemented in SQLite, Postgres, and mem (mem's `list_by_spec_path` does real per-path filtering, `mem.rs:702-711`). Check 1 flags non-terminal `spec-drift-review` tasks past `drift_review_max_age_secs` (default 24 h); check 2 flags `spec-implementation` + `Backlog` past `implementation_backlog_max_age_secs` (default 7 d); check 3 flags watched-path ledger entries with a `current_sha`, non-`Deprecated`, past the hook grace window, with no non-`Cancelled` task referencing the path (both `system/x.md` and `specs/system/x.md` spellings queried; repo-scoped when both sides know the repo).
- **Check 3's `updated_at` keying is sound, not a false-positive storm.** `spec_registry::sync_spec_ledger` bumps `updated_at` only on SHA change, new registration, or Deprecated-reactivation (`spec_registry.rs:389/407/424/439`) — stable specs keep their old `updated_at` and the post-receive hook creates the task in the same tick as the ledger write, so the grace window covers hook ordering. The check is genuine defense in depth, not a periodic escalation of every healthy spec.
- **Escalation is real enforcement.** `escalate_findings` → `state.emit_event(Some(ws), Destination::Workspace(ws), MessageKind::Escalation, payload)`; `emit_event` persists non-Broadcast messages via `messages.store` (`lib.rs:517-521`) and dispatches. Workspaceless findings broadcast; no fabricated `"default"` scope anywhere. Payload carries `reason` (satisfies the message-bus escalation schema — asserted in-test via `validate_payload`) plus `source`/`finding_type`/`task_id`/`spec_path`/`severity`/`message`/`suggested_action`.
- **Route + gating.** Registered in `api/mod.rs` next to spec-links patrol and in the ABAC resolver (`abac_middleware.rs:289`, `spec` resource, POST→write) — no exemption-file entry (the list is frozen; `check-abac-route-registry.sh` passes). This is stronger than the legacy spec-links patrol, which sits in the frozen exemption file. Builtin policies allow write-on-`spec` for Admin/Developer/Agent; ReadOnly is denied.
- **Kill-tests (mine, run fresh this round, tree restored to clean afterwards):**
  - Mutant: `escalate_findings` no-op → 3 failures (`flags_and_escalates_only_the_accountability_gaps`, `workspaceless_finding_broadcasts`, endpoint `spec_lifecycle_patrol_endpoint_flags_and_escalates`), exit 101 (`/tmp/stage/review-evidence/task204-killtest-escalate-noop.log`).
  - Mutant: check 3 early-return → 2 failures (`flags_watched_specs_without_tasks`, `flags_and_escalates_only_the_accountability_gaps`), exit 101 (`task204-killtest-check3-noop.log`).
  - Mutant: check 1 threshold inverted (`>=` instead of `<=`) → 7 failures, exit 101 (`task204-killtest-check1-inverted.log`).
  - After restore: `git status` clean; full focused suite green again.
- **Clean runs:** `cargo test -p gyre-server --lib spec_lifecycle_patrol` → 11 passed; `api::specs::tests` → 53 passed (incl. both new endpoint tests through the real router, asserting persisted escalations and threshold overrides in both directions); `spec_patrol` → 19 passed (shared-type regression). `scripts/check-arch.sh`, `check-abac-route-registry.sh`, `check-dead-message-kinds.sh`, `check-inert-enforcement.sh` all pass. Logs under `/tmp/stage/review-evidence/`.

## Notes (non-blocking)

- Endpoint tests drive `crate::api::api_router()` directly, which does not include the ABAC/auth middleware layers (`build_router` adds them). The resolver registration and builtin-policy evaluation were therefore verified by code inspection; the deployed-transport HTTP check remains for host verification — this sandbox's listener capability (`accept` → `Errno 95`) blocks a live server probe. Not a code defect.
- `spec_has_live_task` treats a task with an empty `repo_id` as a wildcard covering any repo's identically-named spec. Documented in-code, defensible (ledger rows can predate repo attribution); noted for task-109 if per-repo scoping is tightened.
- The check-3 grace window reuses `drift_review_max_age_secs` rather than a separate parameter. Documented; consistent with the task's "no magic numbers" constraint.

## Conclusion

Every acceptance criterion is met with independently reproduced evidence: three real checks against port-backed storage, route registered with real ABAC policy evaluation, persisted per-finding `Escalation` messages asserted by tests that fail when escalation is skipped (verified by mutation), exact stale/orphaned-only flag sets, terminal tasks excluded, architecture checks green. Approved.
