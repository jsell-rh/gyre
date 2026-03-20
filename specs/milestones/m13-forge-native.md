# M13: Forge-Native Agent Capabilities

## Goal

Unlock the capabilities that are only possible because Gyre owns the source control layer.
Agents get sub-second feedback, tamper-evident commit provenance, conflict awareness before
implementation begins, and a VCS that maps naturally to tool-execution semantics.

## Problem

Gyre has a working git forge with merge requests, merge queue, and quality gates (M12).
The forge layer currently behaves like an external forge: pushes are accepted, refs updated,
webhooks would be sent (we broadcast domain events instead). The deep integration potential is
untapped: commit metadata has no task linkage, push validation is coarse, agents discover
conflicts only at MR submission, and the merge queue operates without awareness of who is
working on what file.

Reference: [`specs/system/forge-advantages.md`](../system/forge-advantages.md) defines all eight
capabilities in detail.

---

## Deliverables

### M13.1 Server-Side Pre-Accept Validation

Reject pushes in the receive-pack handler before the ref is updated.

**Implementation:**
- `PreAcceptGate` trait in `gyre-ports`: `fn check(push: &PushContext) -> GateResult`
- Built-in gates registered at server startup:
  - `ArchLintGate`: run `scripts/check-arch.sh` equivalent logic in-process
  - `ConventionalCommitGate`: validate commit message format for all commits in push
  - `TaskRefGate`: `feat/` and `fix/` branches must reference a known TASK-id in commit message
  - `FmtGate`: `cargo fmt --check` on changed `.rs` files (configurable, disabled by default)
  - `NoEmDashGate`: reject commits containing em-dash characters in message or changed files
- Gates configured per-repo via `repo_gates` table (JSON gate list)
- Push rejected with human-readable error message listing which gates failed and why
- Gate results recorded in activity log with agent attribution
- `GET /api/v1/repos/{id}/gates` -- list configured gates
- `PUT /api/v1/repos/{id}/gates` -- update gate list (Developer role required)

**Acceptance criteria:**
- [ ] Push with non-conventional commit message is rejected before ref update
- [ ] Push to `feat/` branch without TASK reference is rejected
- [ ] Gate failure message names the specific gate and explains the fix
- [ ] Gate results appear in activity log with agent_id
- [ ] Gate list is configurable per-repo via REST API

---

### M13.2 Rich Commit Provenance

Every accepted commit is linked to its agent session, task, loop step, and spawning user.

**Implementation:**
- Extend `agent_commits` table: add `task_id`, `ralph_step`, `spawned_by_user_id`,
  `parent_agent_id`, `model_context` (JSON blob) columns
- Populate during post-receive hook (server has session context at push time)
- `ralph_step` enum: `spec | implement | review | merge` -- derived from task status at push time
- `model_context` captures: model name, active tools list, spec revision hash
- `GET /api/v1/repos/{id}/provenance?commit={sha}` -- full provenance for a commit
- `GET /api/v1/repos/{id}/provenance?task_id={id}` -- all commits for a task, grouped by loop step
- `GET /api/v1/repos/{id}/provenance?agent_id={id}` -- all commits by an agent session
- `GET /api/v1/repos/{id}/provenance?ralph_step=implement` -- all implement-phase commits
- Provenance visible in Repo Detail -> Commits tab (add "Agent" column with task link)

**Acceptance criteria:**
- [ ] Commit pushed by agent includes task_id, ralph_step, spawned_by in provenance record
- [ ] `GET /api/v1/repos/{id}/provenance?task_id=X` returns commits grouped by loop step
- [ ] Provenance API response includes parent_agent_id chain (full spawn lineage)
- [ ] Dashboard Commits tab shows agent attribution with task link

---

### M13.3 Zero-Latency Feedback (Extended Post-Receive)

Enrich the existing post-receive hook to deliver immediate, actionable feedback to the pushing agent.

**Implementation:**
- Extend `POST /git/{project}/{repo}/git-receive-pack` response to include feedback payload:
  ```
  remote: [GYRE] Push accepted for branch feat/my-feature
  remote: [GYRE] Gate checks: 3/3 passed
  remote: [GYRE] Merge queue position: 2 (1 ahead)
  remote: [GYRE] Speculative merge: clean (no conflicts detected)
  remote: [GYRE] Task TASK-007: status -> in_progress
  ```
- Remote-info lines are surfaced via standard git sideband mechanism (no client changes needed)
- Push response includes JSON metadata in `X-Gyre-Push-Result` header:
  ```json
  {
    "accepted": true,
    "branch": "feat/my-feature",
    "gates_passed": 3,
    "queue_position": 2,
    "speculative_merge": "clean",
    "task_id": "TASK-007"
  }
  ```
- `gyre push` CLI command parses this header and displays structured summary
- Domain event `PushAccepted` broadcast to all WebSocket clients with same payload

**Acceptance criteria:**
- [ ] `git push` output includes Gyre feedback lines (remote: [GYRE] ...)
- [ ] `gyre push` displays structured push summary from `X-Gyre-Push-Result` header
- [ ] `PushAccepted` domain event visible in dashboard activity feed immediately after push

---

### M13.4 Cross-Agent Code Awareness

Real-time queries combining git history with agent provenance.

**Implementation:**
- `GET /api/v1/repos/{id}/blame?path={file}` -- per-line agent attribution
  - Response: array of `{line_start, line_end, commit_sha, agent_id, task_id, ralph_step}`
  - Falls back to git blame for commits without provenance records
- `GET /api/v1/repos/{id}/hot-files?limit=20` -- files with most distinct active agents
  - "Active" = agent has touched the file in the last 24h and is not in Idle/Dead status
  - Response: `{path, agent_count, agents: [{id, name, task_id}]}`
- `GET /api/v1/agents/{id}/touched-paths` -- all file paths written by an agent in its session
- `GET /api/v1/repos/{id}/review-routing?path={file}` -- ordered list of agents to notify for review
  - Ranked by: recency of last touch, number of commits to path, current task relevance
- `HotFilesChanged` domain event emitted when hot-files list changes (new agent touches tracked file)
- Dashboard Repo Detail: "Active Agents" tab showing hot files with agent avatars

**Acceptance criteria:**
- [ ] `GET /api/v1/repos/{id}/blame?path=src/main.rs` returns agent attribution for each line range
- [ ] `GET /api/v1/repos/{id}/hot-files` returns files with concurrent agent activity
- [ ] `GET /api/v1/repos/{id}/review-routing?path=X` returns ranked agent list
- [ ] Dashboard shows active agent count per file in Repo Detail

---

### M13.5 Speculative Merging

Continuous background merge attempts for all in-flight branches.

**Implementation:**
- Background job `SpeculativeMergeJob` (joins existing job framework from M6):
  - Runs every 60 seconds (configurable)
  - For each active agent branch, attempt `git merge --no-commit --no-ff main`
  - On conflict: record conflict details, emit `SpeculativeConflict` domain event
  - On clean merge: store result at `refs/speculative/{branch}`, emit `SpeculativeMergeClean` event
  - Ephemeral refs garbage-collected when branch is merged or agent completes
- `GET /api/v1/repos/{id}/speculative` -- list all speculative merge results
- `GET /api/v1/repos/{id}/speculative/{branch}` -- result for a specific branch:
  ```json
  {
    "branch": "feat/my-feature",
    "status": "conflict",
    "conflicting_branch": "feat/other-feature",
    "conflicting_agent_id": "...",
    "conflicting_files": ["src/lib.rs"],
    "detected_at": "2026-03-20T15:00:00Z"
  }
  ```
- `GET /api/v1/admin/jobs` lists `speculative-merge` job with last run time and conflict count
- Dashboard: conflict badge on branch list rows, link to conflicting agent

**Acceptance criteria:**
- [ ] Two branches with overlapping file edits produce a `SpeculativeConflict` event within 60s
- [ ] Conflict event names both branches and the conflicting agent
- [ ] Dashboard branch list shows conflict badge with link to conflicting agent
- [ ] Clean branches show `refs/speculative/{branch}` ref in repo

---

### M13.6 Custom Ref Namespaces

Structured server-managed git refs for Ralph loop state.

**Implementation:**
- Server writes lifecycle refs automatically:
  - On task status -> `in_progress`: write agent's current HEAD to `refs/agents/{agent-id}/head`
  - On task `ralph_step` transition: write HEAD to `refs/ralph/{task-id}/{step}`
  - On speculative merge: write result to `refs/speculative/{branch}` (see M13.5)
  - On agent complete: write final HEAD to `refs/agents/{agent-id}/snapshots/{n}`
- Agents may NOT push directly to `refs/agents/`, `refs/ralph/`, `refs/speculative/`, `refs/reviews/`
  -- pre-accept gate rejects such pushes
- `GET /api/v1/repos/{id}/refs` -- list all refs including server-managed namespaces
- `GET /api/v1/repos/{id}/refs?namespace=ralph` -- filter by namespace
- `GET /api/v1/tasks/{id}/refs` -- all refs associated with a task across its lifecycle
- Dashboard Task Detail: "Artifacts" tab listing Ralph refs with diff links between phases

**Acceptance criteria:**
- [ ] Agent push to `refs/ralph/` is rejected by pre-accept gate
- [ ] Task status transition writes corresponding Ralph ref automatically
- [ ] `GET /api/v1/tasks/{id}/refs` returns all phase refs for the task
- [ ] Dashboard Task Detail shows diff between spec and implement phases

---

### M13.7 Atomic Agent Operations (Hardening)

Extend the existing spawn/complete atomicity to cover all failure modes.

**Implementation:**
- Wrap spawn transaction in a SQLite `BEGIN IMMEDIATE` covering:
  agent insert, token mint, worktree creation, task assignment, initial ref write
- Add `agent_spawn_log` table: records each step with timestamp and result (for recovery)
  - Columns: `agent_id`, `step`, `status` (ok/failed), `detail`, `occurred_at`
- Compensating transactions: if any step fails post-agent-insert, server runs cleanup:
  - Delete agent record
  - Remove worktree if created
  - Revoke token (mark in `revoked_tokens` table)
  - Reset task to prior status
- `GET /api/v1/admin/agents/{id}/spawn-log` -- diagnostic view of spawn attempt steps
- Same pattern for complete: wrap in transaction, log each step, compensate on failure
- Idempotent complete: calling complete twice returns the existing MR rather than erroring

**Acceptance criteria:**
- [ ] Spawn with invalid repo_id fails atomically (no orphaned records)
- [ ] Spawn log records all steps with timestamps
- [ ] Double-complete returns existing MR with `202 Already Completed`
- [ ] Admin spawn log visible in admin panel Agent detail

---

### M13.8 jj Native Identity Integration

Apply Gyre agent identity to jj change descriptions and enforce signing at squash time.

**Implementation:**
- Extend `POST /api/v1/repos/{id}/jj/new` to accept optional `ralph_step` and inject into
  change description template:
  ```
  {task_title}

  Agent: {agent_name}
  Session: {session_id}
  Task: {task_id}
  Loop-step: {ralph_step}/iteration-{n}
  Spawned-by: {spawned_by_user}
  ```
- `POST /api/v1/repos/{id}/jj/squash` triggers Sigstore signing flow:
  - Server fetches agent's current OIDC token
  - Requests Fulcio certificate for the token identity
  - Signs the squashed commit SHA using the certificate
  - Records certificate + Rekor log entry in provenance record
  - Returns `signed_commit_sha` and `rekor_log_url` in response
- `GET /api/v1/repos/{id}/commits/{sha}/signature` -- verify commit signature, return
  `{valid, agent_id, task_id, rekor_url, certificate_subject}`
- Dashboard Repo Detail: signature badge on commits from jj squash, click to view provenance

**Acceptance criteria:**
- [ ] `jj new` via API produces change description with full agent identity context
- [ ] `jj squash` returns signed_commit_sha and rekor_log_url
- [ ] `GET /api/v1/repos/{id}/commits/{sha}/signature` returns valid signature metadata
- [ ] Dashboard shows signature badge on signed commits

---

## Priority Order

| # | Deliverable | Rationale |
|---|---|---|
| 1 | M13.1 Pre-Accept Validation | Immediate value: blocks bad pushes before they land. Simplest to ship. |
| 2 | M13.2 Rich Commit Provenance | Enables all downstream capabilities. Low risk: extends existing table. |
| 3 | M13.3 Zero-Latency Feedback | High agent UX impact. Builds on existing post-receive machinery. |
| 4 | M13.4 Cross-Agent Code Awareness | Enables automatic review routing. Requires provenance (M13.2). |
| 5 | M13.5 Speculative Merging | High value for preventing late conflicts. Background job pattern is established. |
| 6 | M13.6 Custom Ref Namespaces | Clean Ralph loop artifact tracking. Requires provenance refs (M13.2). |
| 7 | M13.7 Atomic Operations Hardening | Reliability improvement on existing feature. Lower urgency. |
| 8 | M13.8 jj Identity Integration | Depends on Sigstore infrastructure. Highest complexity. |

## Overall Acceptance Criteria

- [ ] Push with bad commit message is rejected before ref update (M13.1)
- [ ] All commits show agent + task attribution in provenance API (M13.2)
- [ ] `git push` output includes Gyre feedback lines (M13.3)
- [ ] Hot-files endpoint identifies concurrent agent activity (M13.4)
- [ ] Conflicting branches trigger speculative conflict event within 60s (M13.5)
- [ ] Task phase transitions write corresponding `refs/ralph/` entries (M13.6)
- [ ] Double-spawn and double-complete are handled atomically (M13.7)
- [ ] jj squash returns Sigstore-signed commit with Rekor URL (M13.8)
