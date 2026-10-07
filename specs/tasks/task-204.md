---
title: "Implement spec-lifecycle accountability patrol (task-age checks + orchestrator escalation)"
spec_ref: "spec-lifecycle.md §Accountability Integration"
depends_on: []
progress: not-started
coverage_sections:
  - "spec-lifecycle.md §Accountability Integration"
commits: ["f96c51e711da6153131bd0f8e419051f536ab894", "dd25029e12d4ee953338c54e6ac1a9907ba41949"]
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
