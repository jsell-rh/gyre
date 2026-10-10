---
title: "Implement spec-lifecycle accountability patrol (task-age checks + orchestrator escalation)"
spec_ref: "spec-lifecycle.md §Accountability Integration"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "spec-lifecycle.md §Accountability Integration"
commits: ["ead1d669b2f5743d2813b45e649b7e94163881cd", "56e77ec4ecd3588ab0b9ce6f355466f3c3ad5c4d", "40ded0602516a013f795654772898954c715d06a", "5147d645704a4445cb589f2b73a86cce70076d36", "91110a37bb67c2763763c405cad897058fcd81ec"]
---

## Spec Excerpt

From `spec-lifecycle.md` §Accountability Integration:

> The Accountability agent's patrol checks for:
> - `spec-drift-review` tasks that have been open longer than one Ralph loop cycle
> - `spec-implementation` tasks that have been in Backlog for more than N days
> - Specs that were modified but have no corresponding task (should never happen if the hook works, but defense in depth)
>
> If any of these are found, the Accountability agent escalates to the workspace orchestrator.

## Problem (current state — code-verified)

`crates/gyre-server/src/spec_patrol.rs` implements the **spec-links.md** graph patrol (stale links, orphaned supersessions, unresolved conflicts, dangling implementations, deep dependency chains). It does NOT implement any of the three **spec-lifecycle.md** accountability checks above. The only references to the `spec-drift-review` / `spec-implementation` labels are the task-creation code in `git_http.rs` (`process_spec_lifecycle`, ~1279-1304) and its tests. No code inspects task ages, no code cross-checks modified specs against existing tasks, and nothing escalates to the workspace orchestrator. Coverage row 8 is `not-started` (hollow).

This is a distinct concern from the existing spec-links patrol at `POST /api/v1/patrol/spec-links` (route registered in `api/mod.rs:351`). Do NOT overload that handler; add a separate spec-lifecycle patrol.

## Implementation Plan

1. **New patrol module `crates/gyre-server/src/spec_lifecycle_patrol.rs`** mirroring the structure of `spec_patrol.rs`:
   - `PatrolFinding`-style struct (reuse the existing `spec_patrol::PatrolFinding` shape if it fits: `finding_type`, `severity`, human-readable `message`, and the offending `task_id`/`spec_path`). Prefer reusing the existing type over inventing a parallel one.
   - `run_spec_lifecycle_patrol(state: &AppState, now_secs: u64, drift_review_max_age_secs: u64, implementation_backlog_max_age_secs: u64) -> Vec<PatrolFinding>`.
   - Register `pub mod spec_lifecycle_patrol;` in `lib.rs` next to `pub mod spec_patrol;` (lib.rs:43).

2. **Check 1 — stale `spec-drift-review` tasks.** Query tasks via `TaskRepository` (`crates/gyre-ports/src/task.rs`), filter to non-terminal (`status != Done && status != Cancelled`) tasks whose `labels` contains `"spec-drift-review"`, and flag any where `now_secs - created_at > drift_review_max_age_secs`. Default the threshold to one Ralph loop cycle. There is no in-code Ralph-loop-cycle constant; use `drift_review_max_age_secs` from the request with a sensible default (document the chosen default, e.g. 24h) rather than hardcoding a magic number in the check body.

3. **Check 2 — stale `spec-implementation` tasks in Backlog.** Filter tasks with label `"spec-implementation"` AND `status == TaskStatus::Backlog`, flag any where `now_secs - created_at > implementation_backlog_max_age_secs` (default N days, e.g. 7d).

4. **Check 3 — modified specs with no corresponding task (defense in depth).** For each spec-ledger entry under the watched paths (use `state.spec_ledger` — the same ledger the hook writes; watched-path filter should match `git_http.rs` `process_spec_lifecycle`), determine whether a task exists that references it. Use `TaskRepository::list_by_spec_path(spec_path)` and treat "has a corresponding task" as: at least one non-Cancelled task whose `spec_path` equals the ledger entry's path. Flag ledger entries whose `current_sha` reflects a modification (i.e., the spec exists and was updated) but for which no such task exists. Do NOT flag brand-new specs that the hook has not yet processed within the same tick — key the check off ledger `updated_at` vs. task existence, not off the push itself.

5. **Escalation to the workspace orchestrator.** For every finding, escalate via the message bus using the established pattern in `api/specs.rs:684-706`: `state.emit_event(Some(workspace_id), Destination::Workspace(workspace_id), <MessageKind>, Some(payload))`. Derive `workspace_id` from the offending task (`task.workspace_id`) for checks 1/2, and from the spec-ledger entry's `workspace_id` for check 3 (fall back to `Destination::Broadcast` when absent, matching specs.rs). Choose an appropriate existing `MessageKind` from `crates/gyre-common/src/message.rs`; only add a new variant (e.g. `AccountabilityEscalation`) if none fits, and if you add one, wire it through all match arms that exhaustively handle `MessageKind`. The payload MUST identify the finding (`finding_type`, `task_id`/`spec_path`, `message`). Escalation is REAL enforcement — the message must actually be emitted, not logged.

6. **HTTP endpoint.** Add `POST /api/v1/patrol/spec-lifecycle` in `api/specs.rs` (handler `patrol_spec_lifecycle`) and register it in `api/mod.rs` next to the existing `/api/v1/patrol/spec-links` route (mod.rs:351). Request body accepts optional `drift_review_max_age_secs` and `implementation_backlog_max_age_secs` (mirror how `PatrolRequest.stale_threshold_secs` defaults in `api/specs.rs:1271`). Response returns the findings. Apply the same auth/role gating the existing patrol endpoint uses.

7. **Do NOT hardcode the "N days" / cycle thresholds inside the check logic.** Accept them as request parameters with documented defaults so task-109 (spec-lifecycle configuration) can later feed per-repo values without a rewrite. This task does NOT depend on task-109 landing.

## Acceptance Criteria

- [ ] New `spec_lifecycle_patrol` module runs all three checks against real storage (`TaskRepository`, `state.spec_ledger`), not in-memory stand-ins.
- [ ] `POST /api/v1/patrol/spec-lifecycle` route registered in `api/mod.rs` and returns findings; auth/role gating matches the existing patrol endpoint.
- [ ] Each finding produces a REAL message-bus escalation to `Destination::Workspace(workspace_id)` via `state.emit_event`, verified by asserting the message was enqueued/emitted (not merely logged).
- [ ] A test seeds: (a) a `spec-drift-review` task with `created_at` older than the threshold and one within it, (b) a `spec-implementation` Backlog task past the threshold and a non-Backlog one, (c) a spec-ledger entry with no corresponding task and one with a task — and asserts exactly the stale/orphaned items are flagged and escalated, while the fresh/covered items are NOT. The test MUST fail if any check is a no-op, if thresholds are inverted, or if escalation is skipped.
- [ ] Terminal (`Done`/`Cancelled`) tasks are never flagged.
- [ ] `cargo test --all` and `bash scripts/check-arch.sh` pass.

## Agent Instructions

- Read `specs/system/spec-lifecycle.md` §Accountability Integration (the contract) and §Configuration (for future threshold sourcing).
- Model the new module on `crates/gyre-server/src/spec_patrol.rs` (types, `run_patrol` shape, `create_notifications_for_error_findings`, and its `#[cfg(test)]` harness for seeding `AppState`).
- Task querying: `crates/gyre-ports/src/task.rs` (`list`, `list_by_status`, `list_by_spec_path`). Task fields (`labels`, `status`, `created_at`, `workspace_id`, `spec_path`): `crates/gyre-domain/src/task.rs`.
- Escalation pattern: `crates/gyre-server/src/api/specs.rs:684-706` (`emit_event` + `Destination::Workspace`). Message kinds: `crates/gyre-common/src/message.rs`.
- The task-creation side (labels/priorities) it must reconcile against: `crates/gyre-server/src/git_http.rs` `process_spec_lifecycle` (~1279-1304).
- Preserve hexagonal boundaries — patrol logic is server-side and depends only on ports/domain, never adapters directly.
- Run validation once at the end. On completion set `progress: ready-for-review` and record commit SHAs in the frontmatter `commits` list.

## Shipped

The three `spec-lifecycle.md` §Accountability Integration checks are live in
`crates/gyre-server/src/spec_lifecycle_patrol.rs` against real storage:

- **Check 1** flags non-terminal `spec-drift-review` tasks older than
  `drift_review_max_age_secs` (default 24 h ≈ one loop cycle).
- **Check 2** flags `spec-implementation` tasks with `status == Backlog` older
  than `implementation_backlog_max_age_secs` (default 7 d).
- **Check 3** (defense in depth) flags watched-path ledger entries
  (`SPEC_WATCHED_PATHS`, shared with the post-receive hook) that have a
  `current_sha`, are not `Deprecated`, are past the hook grace window, and have
  no non-`Cancelled` task referencing the path (both `system/x.md` and
  `specs/system/x.md` spellings normalized; repo-scoped when the ledger entry
  carries a `repo_id`).

Every finding escalates to the workspace orchestrator as a REAL persisted
`MessageKind::Escalation` message on the workspace event stream via
`state.emit_event` (workspaceless findings broadcast — never a fabricated
`"default"` scope). `POST /api/v1/patrol/spec-lifecycle` is registered in
`api/mod.rs` beside spec-links patrol with an ABAC `RouteResourceMapping`
(`spec` resource, write action) — real policy evaluation, no exemption file
entry.

This repair round removed a leftover kill-test mutant (`if true { return 0; }`
stubbing `escalate_findings`) from the interrupted assignment's checkpoint.

## Test evidence

- Kill-test (mutant in place, `escalate_findings` no-op): 3 tests fail with
  `every finding must be escalated, not merely logged` /
  `one escalation per finding` / `escalation must be emitted, not skipped` —
  `spec_lifecycle_patrol` + endpoint tests detect skipped escalation
  (`/tmp/stage/review-evidence/task204-killtest-mutant.log`, exit 101).
- Clean run: `cargo test -p gyre-server --lib spec_lifecycle_patrol` — 11
  passed, 0 failed (incl. stale/fresh, Backlog/non-Backlog, orphaned/covered,
  cancelled-task coverage, threshold-override, broadcast routing tests).
- Endpoint tests: `cargo test -p gyre-server --lib api::specs::tests` — 53
  passed (incl. `spec_lifecycle_patrol_endpoint_flags_and_escalates` and
  `..._honours_thresholds`, which assert persisted escalations via the real
  router).
- Spec-links patrol regression (shared `PatrolFinding` gained
  `task_id`/`workspace_id`): `cargo test -p gyre-server --lib spec_patrol` —
  19 passed.
- `bash scripts/check-arch.sh` passed; `check-abac-route-registry.sh`,
  `check-fabricated-scope-defaults.sh`, `check-scope-literal-defaults.sh`,
  `check-mem-port-contracts.sh`, `check-dead-message-kinds.sh` all passed.
- HTTP checks deferred to host verification: this sandbox's listener probe is
  unsupported (`accept` → `Errno 95`, see `/tmp/stage/capabilities.json`), so
  the route was verified through the in-process router tests above; exact-head
  GitHub CI checks remain mandatory for the deployed transport check.

### Repair round 2 (post-merge re-verification at merged HEAD `fb294f6e`)

- Resumed from checkpoint `1dba1c36` with base `a1751da1` merged in (merge
  touched no files under `crates/` or `scripts/`); working tree was already
  clean and free of the kill-test mutant — the interrupted assignment's final
  frontmatter commit had landed. No code changes were needed this round.
- All evidence re-collected fresh at merged HEAD
  (`/tmp/stage/review-evidence/task204-clean-run.log`):
  `spec_lifecycle_patrol` 11 passed, `api::specs::tests` 53 passed,
  `spec_patrol` 19 passed, all exit 0.
- Kill-test re-executed at HEAD (mutant re-applied from the retained
  `/tmp/stage/resume-0.patch`, then reverted; log retained at
  `/tmp/stage/review-evidence/task204-killtest-mutant.log`, exit 101): the
  same 3 tests fail — `flags_and_escalates_only_the_accountability_gaps`,
  `spec_lifecycle_patrol_endpoint_flags_and_escalates`,
  `workspaceless_finding_broadcasts` — and the working tree is clean after
  revert.
- `check-arch.sh`, `check-abac-route-registry.sh`,
  `check-abac-exempt-handlers.sh` (89 handlers),
  `check-dead-message-kinds.sh`, `check-mem-port-contracts.sh`,
  `check-fabricated-scope-defaults.sh`, `check-scope-literal-defaults.sh`,
  `check-inert-enforcement.sh`, and `check-task-commit-attribution.sh` all
  pass at HEAD.
