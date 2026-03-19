# Milestone 3: Agent Orchestration

Make Gyre a functional agent orchestration platform. After M3, Gyre can spawn agents, assign them tasks, have them clone/push to Gyre-hosted repos, open MRs, and flow through the merge queue — all orchestrated by the server.

## Deliverables

### 1. Smart HTTP Git Protocol

Serve git repos over HTTP so agents can clone and push:

- `GET /git/{project}/{repo}.git/info/refs?service=git-upload-pack` — git clone/fetch
- `POST /git/{project}/{repo}.git/git-upload-pack` — git clone/fetch data
- `GET /git/{project}/{repo}.git/info/refs?service=git-receive-pack` — git push
- `POST /git/{project}/{repo}.git/git-receive-pack` — git push data
- Auth via Bearer token (agent token from registration)
- Post-receive hook: auto-record agent-commit mapping

### 2. Agent Spawn + Orchestration API

Server-side agent orchestration:

- `POST /api/v1/agents/spawn` — spawn an agent with task assignment, worktree setup
  - Creates agent record, generates token
  - Creates a worktree on the assigned repo
  - Returns: agent_id, token, worktree_path, clone_url
- `POST /api/v1/agents/{id}/complete` — agent signals task completion
  - Auto-creates MR from agent's branch to target
  - Triggers review workflow
- Background: monitor agent heartbeats, kill stale agents, clean up worktrees

### 3. CLI as Gyre Client

Update gyre-cli to work as a full Gyre client:

- `gyre-cli init` — register CLI as an agent, save token
- `gyre-cli clone <repo>` — clone from Gyre server
- `gyre-cli push` — push to Gyre server
- `gyre-cli mr create --title "..." --target main` — create MR
- `gyre-cli tasks list` — list tasks
- `gyre-cli tasks take <id>` — assign task to self
- `gyre-cli status` — show agent status, current task

### 4. Token-Based Auth

Replace static auth token with per-agent tokens:

- Agent registration generates unique token
- All API endpoints validate Bearer token
- Token maps to agent identity
- CLI stores token in config file (~/.gyre/config)
- WebSocket auth uses same token system

### 5. End-to-End Ralph Loop via Gyre

Demonstrate the platform working end-to-end:

- Server creates a project and repo
- Server spawns an agent (via API)
- Agent clones repo via smart HTTP
- Agent implements a change
- Agent pushes, creates MR
- MR goes through review (auto-approve for M3)
- MR enters merge queue
- Merge queue processes and merges
- Activity log captures entire lifecycle

### 6. Dashboard Updates

- Agent spawn controls (spawn button with config form)
- Clone URL display on repo pages
- Agent task assignment view

## Success Criteria

- An agent can be spawned, clone a repo, push changes, and land an MR — all via Gyre APIs
- CLI works as a full Gyre client (init, clone, push, mr create)
- 250+ tests
- End-to-end integration test covering the full Ralph loop via Gyre

## Non-Goals for M3

- No SSO/Keycloak (M4)
- No eBPF audit
- No jj integration (M4)
- No remote compute provisioning

## Dependencies

- M2 Source Control Foundation (complete)
