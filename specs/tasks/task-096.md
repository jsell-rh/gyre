---
title: "Platform Model Rollback UI"
spec_ref: "platform-model.md §6 UI"
depends_on:
  - task-095
progress: not-started
coverage_sections:
  - "platform-model.md §6 UI"
commits: []
---

## Scope Reconciliation (task-095 review R3-F4)

This task was originally scoped as "Rollback Circuit Breaker + CLI/UI".
The Circuit Breaker and the CLI/REST recovery surface were **already
delivered under task-095** (commit `5aaded21`, plus the R3 revision):

- Circuit breaker: revert counting (`merge_processor::increment_revert_count`,
  resubmission-stable keying per task-095 R3-F2), permanent queue removal,
  Critical escalation task + human notification at 3 reverts
  (`trip_circuit_breaker`).
- REST recovery surface: `GET /api/v1/repos/:id/status` (main health +
  queue state), `PUT /api/v1/repos/:id/queue/pause|resume`,
  `POST /api/v1/repos/:id/revert/:mr_id` (manual revert running the full
  §6 protocol: revert the recorded `merge_commit_sha`, side effects,
  breaker accounting), `GET|PUT /api/v1/repos/:id/post-merge-gates`.
- CLI: `gyre repo status`, `gyre repo revert`, `gyre repo queue pause`,
  `gyre repo queue resume` (`gyre-cli/src/main.rs`).

What remains — and what this task is now scoped to — is the **UI surface**
of §6, which consumes those existing endpoints. No server-side state,
endpoints, or CLI work belongs here.

## Spec Excerpt (§6 UI)

- Repo detail page shows main branch health (green/red indicator)
- Merge queue shows paused state with reason
- Activity feed shows revert events with links to original MR and failure output

## Implementation Plan

1. **Repo health indicator:** on the repo detail view, poll
   `GET /api/v1/repos/:id/status` and render main-branch health from
   `main_green` (green / red / unknown-while-paused) and the merge-queue
   state.
2. **Paused-queue banner:** when the status response reports the queue
   paused, show a banner with the `pause_reason`; surface the manual
   pause/resume controls against `PUT /repos/:id/queue/pause|resume`.
3. **Activity feed revert events:** render `mr_reverted` activity (and the
   remediation/escalation tasks they create) with links to the original MR
   and the failure output.

## Acceptance Criteria

- [ ] Repo detail shows main health indicator driven by `GET /repos/:id/status`
- [ ] Merge queue view shows paused state with the recorded reason
- [ ] Activity feed shows revert events linking to the original MR
- [ ] `npm test` passes in `web/`

## Agent Instructions

Read `specs/system/platform-model.md` §6 "UI". Everything server-side is
done (see Scope Reconciliation above) — do NOT add endpoints, CLI commands,
or breaker logic here; wire the Svelte views to the existing endpoints and
events. Check how repo detail is rendered in the Svelte components and how
activity events are consumed on the dashboard.
