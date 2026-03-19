# Milestone 2: Source Control Foundation

Gyre IS the source control host. After M2, Gyre can create and manage git repositories, handle merge requests with review workflows, and operate a merge queue for serializing concurrent agent work.

## Deliverables

### 1. Git Repository Management

Server-side git operations via the Repository domain:

- **Create repository** — `git init --bare` on the server, register in DB
- **Clone URL** — serve git repos over HTTP (smart HTTP protocol via `git-http-backend` or a Rust implementation)
- **Push/Pull** — agents can push to and pull from Gyre-hosted repos
- **Branch listing** — `GET /api/v1/repos/{id}/branches` returns branches with head commit
- **Commit log** — `GET /api/v1/repos/{id}/commits?branch=&limit=` returns commit history
- **Diff** — `GET /api/v1/repos/{id}/diff?from=&to=` returns diff between refs

Use `git2` (libgit2 bindings) for server-side git operations.

### 2. Merge Request Workflow

Enhance MRs from simple status tracking to functional review workflow:

- **Create MR** — auto-generates diff stats, validates source/target branches exist
- **Review comments** — `POST /api/v1/merge-requests/{id}/comments` (reviewer, body, file, line)
- **Approve/Request changes** — reviewer actions that gate merge
- **Status auto-transitions** — MR status changes based on reviews and CI state
- **Conflict detection** — detect when source branch conflicts with target, mark MR

### 3. Merge Queue

The serialization point where parallel Ralph loops converge:

- **Enqueue MR** — `POST /api/v1/merge-queue/enqueue` adds approved MR to queue
- **Queue processing** — background task that:
  - Takes the next MR from the queue
  - Rebases source branch onto target HEAD
  - Runs validation (tests pass — for now, just check it merges cleanly)
  - Fast-forward merges to target
  - Updates MR status to Merged
- **Priority ordering** — Critical MRs jump the queue
- **Conflict handling** — if rebase fails, notify the authoring agent, dequeue

### 4. Agent-to-Commit Tracking

Since Gyre owns the forge, link every commit to the agent that made it:

- **Commit metadata** — store agent_id in commit metadata (git notes or DB mapping)
- **Query** — `GET /api/v1/repos/{id}/commits?agent_id=` — find commits by agent
- **Session tracking** — which agent session touched which branches/commits

### 5. Worktree Management

Gyre manages git worktrees for agents:

- **Create worktree** — `POST /api/v1/repos/{id}/worktrees` creates a git worktree for an agent task
- **List worktrees** — `GET /api/v1/repos/{id}/worktrees`
- **Delete worktree** — cleanup after agent completes
- Worktrees are linked to agent + task

### 6. Dashboard Updates

- **Repository detail page** — branches, recent commits, open MRs
- **MR detail page** — diff view, comments, approve/reject buttons, merge button
- **Merge queue view** — show queued MRs in order, processing status

## Success Criteria

- Agents can clone, push, and pull from Gyre-hosted repos
- MRs can be created, reviewed, approved, and merged through the queue
- Merge queue serializes concurrent merges correctly
- Every commit links back to its agent
- 80+ tests across git operations, MR workflow, merge queue

## Non-Goals for M2

- No jj integration yet (M3)
- No external repo mirroring (GitHub/GitLab)
- No CI runner integration (validation is merge-clean check only)
- No branch protection rules (M3)

## Dependencies

- M1 Domain Foundation (complete)
