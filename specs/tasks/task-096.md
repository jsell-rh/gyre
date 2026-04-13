---
title: "Platform Model Rollback Circuit Breaker + CLI/UI"
spec_ref: "platform-model.md §6 Circuit Breaker"
depends_on:
  - task-095
progress: not-started
coverage_sections:
  - "platform-model.md §6 Circuit Breaker"
  - "platform-model.md §6 CLI"
  - "platform-model.md §6 UI"
commits: []
---

## Spec Excerpt

### Circuit Breaker

If the same MR is reverted 3 times:
1. MR is removed from the merge queue permanently
2. Task escalated to human: "MR #{id} has failed post-merge validation 3 times"
3. The spec it references may need revisiting

### CLI

```bash
gyre repo status       # Shows if main is green/broken, merge queue paused/running
gyre repo revert MR-042  # Manual revert of a specific MR
gyre repo queue pause  # Manual merge queue pause
gyre repo queue resume # Manual resume
```

### UI

- Repo detail page shows main branch health (green/red indicator)
- Merge queue shows paused state with reason
- Activity feed shows revert events with links to original MR and failure output

## Implementation Plan

1. **Circuit breaker tracking:**
   - Add `revert_count: u32` field to MR domain entity
   - Increment on each revert in recovery protocol
   - When `revert_count >= 3`: remove from queue permanently, create critical human notification

2. **CLI commands:**
   - `gyre repo status`: query `GET /api/v1/repos/:id` for health status + `GET /api/v1/repos/:id/queue` for queue state
   - `gyre repo revert <mr-id>`: call `POST /api/v1/merge-requests/:id/revert` (new endpoint)
   - `gyre repo queue pause`: call `POST /api/v1/repos/:id/queue/pause`
   - `gyre repo queue resume`: call `POST /api/v1/repos/:id/queue/resume`

3. **Queue pause/resume API:**
   - `POST /api/v1/repos/:id/queue/pause` — sets merge queue paused state
   - `POST /api/v1/repos/:id/queue/resume` — clears paused state
   - Both require Admin role

4. **Manual revert endpoint:**
   - `POST /api/v1/merge-requests/:id/revert` — triggers the recovery protocol for a specific merged MR
   - Requires Admin role
   - Only works on MRs with status `Merged`

5. **UI indicators:**
   - Repo detail: green/red health badge based on post-merge gate status
   - Merge queue view: "Paused" banner with reason when queue is paused
   - Activity feed: revert events with links

## Acceptance Criteria

- [ ] revert_count tracked per MR
- [ ] MR removed from queue permanently after 3 reverts
- [ ] Critical notification created for 3x revert
- [ ] `gyre repo status` shows health and queue state
- [ ] `gyre repo revert` triggers manual revert
- [ ] `gyre repo queue pause/resume` work
- [ ] Repo detail shows health indicator
- [ ] Merge queue shows paused state
- [ ] `cargo test --all` passes
- [ ] `npm test` passes in `web/`

## Agent Instructions

Read `specs/system/platform-model.md` §6 "Circuit Breaker", "CLI", and "UI" sections. This depends on task-095 (recovery protocol) being implemented first. The CLI is in `gyre-cli/src/main.rs`. For the queue pause/resume API, check the existing merge queue endpoint patterns in `gyre-server/src/api/mod.rs`. The merge queue state is in the merge processor. For UI, check how repo detail is rendered in the Svelte components.
