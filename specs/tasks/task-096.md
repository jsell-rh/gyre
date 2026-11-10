---
title: "Platform Model Rollback Circuit Breaker + CLI/UI"
spec_ref: "platform-model.md §6 Circuit Breaker"
depends_on:
  - task-095
progress: complete
coverage_sections:
  - "platform-model.md §6 Circuit Breaker"
  - "platform-model.md §6 CLI"
  - "platform-model.md §6 UI"
commits: ["109ec4d0ba38140d9a885d2861ce8585bc212b23", "d3ce6e03f70fc2581e6e78c70298e8d03fa131bb"]
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
## Shipped

The task's declared surface splits into what task-095's lineage (commits
5aaded213 and the R3 revision, merged as 7e693c20e) had already delivered
and the one genuinely open gap this task closes: the §6 **UI** (coverage
row 38). This commit (109ec4d0) ships that UI plus one additive server
capability it needs; it does not duplicate or rewrite the delivered
circuit-breaker/REST/CLI surface.

### What was already delivered (verified at base 7e693c20e, not re-done here)

- Circuit breaker (row 36): `increment_revert_count` / `trip_circuit_breaker`
  in `merge_processor.rs`, keyed on the resubmission-stable key
  (task-095 R3-F2), trips at ≥3 reverts on the automatic recovery paths and
  on manual revert, cancels queue entries, escalates to a Critical
  circuit-breaker task, notifies workspace members.
- CLI (row 37): `gyre repo status|revert|queue pause|queue resume`
  (`gyre-cli/src/main.rs`, client methods in `client.rs`) against
  `GET /repos/:id/status`, `PUT queue/pause|resume`, `POST revert/:mr_id`.
  The task-095 R3-F1/F3 defects (merge-commit-sha-scoped revert,
  fail-closed `main_green`) are fixed at base — the fail-open exemptions
  file is empty.
- Activity feed revert events: the server passes `MrReverted`
  notifications through `activity.rs` with `mr_id`/`repo_id` from the
  payload and the feed renders entity links.

### What this task ships (commit 109ec4d0)

- **api.js client surface**: `repoStatus(id, { withGates })`,
  `pauseRepoQueue(id, reason)`, `resumeRepoQueue(id)` for the existing
  Admin-gated endpoints.
- **Repo detail page (RepoMode.svelte)**: green/red/unknown main-branch
  health badge in the repo header (from `main_green`), and an MRs-tab
  queue-state banner showing main health, the paused state **with reason**
  (`role=status`), and Pause/Resume controls wired to the queue endpoints.
  Status is fetched when the MRs tab is visited because
  `GET /repos/:id/status` executes the repo's post-merge gates server-side
  (up to minutes per gate) — too heavy to run on every repo page view.
- **Repo-mode merge queue section fixed**: the `/merge-queue` response
  carries no `repository_id`, so the previous client-side filter
  (`e.repository_id === repoId`) never matched anything and the section
  never rendered. Entries are now scoped by joining with the repo's MRs.
  Reverted MRs badge as `danger`.
- **MergeQueueGraph**: `paused`/`pauseReason` props render a
  "Merge queue paused — \<reason\>" banner, visible even when the queue
  is empty.
- **WorkspaceHome**: the sidebar merge-queue widget fetches pause state
  for repos with queued entries and surfaces it as a `paused` chip (list
  view, reason in the title) and the DAG banner. `MrReverted` activity
  events get the "MR reverted" label and danger variant.
- **Server (additive)**: `GET /repos/:id/status?gates=false` skips
  post-merge gate execution (`main_green: null`) so multi-repo dashboards
  can read pause state without running gate commands; default behavior
  (and the CLI call) is unchanged, and the task-095 R3-F3 fail-closed
  semantics are preserved.

### Test evidence

- `cargo test -p gyre-server --lib api::recovery` — 7/7 pass, including
  the new `repo_status_gates_false_skips_gate_execution` (a gate whose
  command is `false` proves via `main_green: null` that it never ran).
- `cd web && npm test` — 75 files, 1778 passed / 0 failed (41 pre-existing
  environment-gated skips unchanged). New: `RepoModeHealth.test.js` (8
  tests: badge variants, paused banner + reason, pause/resume wiring,
  queue-entry repo scoping), `WorkspaceHomeQueuePause.test.js` (3 tests:
  paused chip, DAG banner, no chip when running), +4 `MergeQueueGraph`
  paused-banner tests, +4 `api.test.js` client-method tests.
- Mechanical checks pass: `check-arch.sh`, `check-fail-open-ref-resolution.sh`
  (still 0 exemptions), `check-abac-route-registry.sh`,
  `check-relative-path-defaults.sh`. No new routes, exemptions, or verifier
  changes.
- `web/dist` regenerated by the test run was restored to the assignment
  base and is not part of the commit, per controller policy.

### Out of scope / notes for review

- `cargo test --all` and GitHub CI are owned by verification/publication;
  the focused probes above cover every file this commit touches.
- Row 38's "activity feed … failure output" bullet: the revert reason
  travels in the notification payload and the feed links to the original
  MR (pre-existing, verified at base); the sidebar feed does not render
  free-text descriptions for any event type, which is a feed-wide design
  choice, not a revert-specific gap.
