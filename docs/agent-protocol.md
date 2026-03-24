# Gyre Agent Protocol

This document covers the full agent lifecycle: spawn, operation, and completion.

---

## Agent Spawn / Complete API (M3.2)

`POST /api/v1/agents/spawn` -- single-call agent provisioning:

```json
// Request
{
  "name": "worker-1",               // required; must match [a-zA-Z0-9._-]{1,63} -- shell metacharacters rejected with 400 (M19.5-A)
  "repo_id": "<repo-uuid>",
  "task_id": "<task-uuid>",
  "branch": "feat/my-feature",
  "parent_id": "<orchestrator-agent-uuid>",    // optional
  "compute_target_id": "<target-uuid>",        // optional -- remote compute target
  "disconnected_behavior": "Pause"             // optional -- "Pause" (default) | "ContinueOffline" | "Abort"; controls stale-agent-detector behavior when agent loses connectivity
}

// Response 201
{
  "agent": {
    "id": "...", "name": "worker-1", "status": "Active",
    "spawned_by": "<caller-agent-id or user-id>",   // M13.2: who initiated spawn
    ...
  },
  "token": "<signed-EdDSA-JWT>",   // M18: starts with "ey", 3 dot-separated parts; claims: sub=agent_id, task_id, spawned_by, exp. G10: when spawned on a real process, also embeds wl_pid, wl_hostname, wl_compute_target, wl_stack_hash. M19.4: container-spawned agents additionally embed wl_container_id, wl_image_hash. Verify via /.well-known/jwks.json. Legacy UUID tokens still accepted from POST /api/v1/agents.
  "worktree_path": "/path/to/worktree",
  "clone_url": "http://localhost:3000/git/project/repo.git",
  "branch": "feat/my-feature",
  "jj_change_id": "<jj-change-id-or-null>",   // present when jj is initialized in worktree (best-effort)
  "container_id": "<docker-container-id-or-null>",   // present when agent was launched in a container via GYRE_DEFAULT_COMPUTE_TARGET=container or a container compute_target_id (M19.1)
  "meta_spec_set_sha": "<sha256-or-null>"   // SHA256 of the workspace's bound meta-spec set at spawn time; null when workspace has no meta-spec-set configured; used for commit provenance (M32)
}
```

`POST /api/v1/agents/{id}/complete` -- teardown after work is done:

```json
// Request
{
  "branch": "feat/my-feature",
  "title": "Add my feature",
  "target_branch": "main"
}

// Response 200 -- returns the opened MergeRequest
```

The server automatically: opens the MR, marks the task done, removes the git worktree, and marks the agent Idle.

**Idempotent (M13.7):** Calling complete a second time returns **202 Accepted** rather than an error -- safe to retry on network failure or agent restart.

**Token revocation (M13.7):** The agent's bearer token is revoked in the database on successful completion. Any subsequent API call with the same token will be rejected with 401. Agents must not reuse a token after completing.

**Commit provenance (M13.2):** When an agent pushes via Smart HTTP, the server automatically captures:
- `spawned_by` -- the identity of whoever called `POST /api/v1/agents/spawn` for this agent
- `X-Gyre-Model-Context` -- optional request header agents may set on the `git-receive-pack` call to record model/context metadata (e.g. `claude-opus-4@anthropic`)

These fields appear on `AgentCommit` records returned by `GET /api/v1/repos/{id}/agent-commits`.

**Spec binding on MR create (M12.3):** `POST /api/v1/merge-requests` accepts an optional `spec_ref` field in the request body -- a string of the form `"specs/system/agent-gates.md@<40-char-sha>"` -- to cryptographically bind the MR to the spec version it implements. A `GateFailure` domain event is broadcast if an `AgentReview` or `AgentValidation` gate fails.

---

## Container Agent Environment Variables (M24)

When spawning an agent into a container compute target, the server pre-mints the JWT (so it can be passed in at container start) and injects the following environment variables into the container:

| Variable | Value | Purpose |
|---|---|---|
| `GYRE_SERVER_URL` | Server base URL | API endpoint for heartbeat, complete, logs |
| `GYRE_AUTH_TOKEN` | Pre-minted EdDSA JWT | Bearer token for all API calls -- revoked on `complete` |
| `GYRE_CLONE_URL` | Git Smart HTTP URL | Clone URL for the assigned repo |
| `GYRE_BRANCH` | Branch name | Branch to clone and work on |
| `GYRE_AGENT_ID` | Agent UUID | Identity for API calls |
| `GYRE_TASK_ID` | Task UUID | Assigned task reference |
| `GYRE_REPO_ID` | Repository UUID | Repo being worked on |
| `GYRE_AGENT_COMMAND` | _(optional)_ | Command for the entrypoint to exec after setup (e.g. a CI script) |
| `GYRE_CRED_PROXY` | `http://127.0.0.1:8765` | Address of the `cred-proxy` sidecar (M27); injected so entrypoint and agent-runner can reference the proxy address |
| `ANTHROPIC_BASE_URL` | `http://127.0.0.1:8765` | Routes Anthropic SDK calls through cred-proxy; raw API key never exposed to agent process (M27) |
| `ANTHROPIC_API_KEY` | `proxy-managed` | Placeholder so Anthropic SDK initialises; cred-proxy injects the real `x-api-key` header per request (M27) |

The `docker/gyre-agent/` directory contains a reference `Dockerfile` (Node 22 Alpine + git + curl) and `entrypoint.sh` that validates these vars, configures git credentials via a credential helper (token not embedded in the clone URL), clones the branch, sends an initial heartbeat, then `exec`s `GYRE_AGENT_COMMAND` or -- if unset -- `node /gyre/agent-runner.mjs` for fully autonomous operation. `agent-runner.mjs` connects to the Gyre MCP server, reads the assigned task, implements it, commits, pushes, and calls `gyre_agent_complete`.

**WireGuard mesh (M26):** `setup-wg.sh` in the same directory handles agent-side mesh setup -- generates a Curve25519 keypair, registers the pubkey via `POST /api/v1/network/peers`, fetches the peer list, brings up the `wg0` interface with the allocated `mesh_ip`, and adds routes. Run it after clone and before `agent-runner.mjs` when `GYRE_WG_ENABLED=true`. The peer response includes `mesh_ip: Option<String>` (allocated from `GYRE_WG_CIDR` pool) and `is_stale: bool`.

Build and register:
```bash
docker build -t gyre-agent:latest docker/gyre-agent/

# M25 zero-config: start server with API key -- gyre-agent-default is auto-registered
GYRE_AGENT_CREDENTIALS=ANTHROPIC_API_KEY=sk-ant-xxx cargo run -p gyre-server

# Or manually create a container compute target (Admin -> Compute -> Add) with type=container
# Agent containers need bridge networking to reach server:
# Set config: {"image": "gyre-agent:latest", "network": "bridge"}
```

---

## Custom Git Ref Namespaces (M13.6)

The server writes refs into reserved namespaces on each lifecycle event:

| Event | Ref written | Purpose |
|---|---|---|
| `spawn` | `refs/agents/{agent-id}/head` | Points to the HEAD commit at spawn; survives branch force-pushes |
| `spawn` | `refs/ralph/{task-id}/implement` | Marks the implement step of the Ralph loop for this task |
| `complete` | `refs/agents/{agent-id}/snapshots/{n}` | Immutable snapshot of the branch tip at completion (n increments per call) |
| `merge` | `refs/notes/attestations` | Signed `MergeAttestation` JSON attached as git note to the merge commit SHA; inspect via `git notes --ref=refs/notes/attestations show <sha>` (G5) |

These refs survive agent restarts. Query them via standard git: `git ls-remote <clone-url> 'refs/agents/*'`.

---

## Gate Agent Protocol (M12.1)

When the merge queue executes an `AgentReview` or `AgentValidation` gate, it spawns the configured command as a subprocess with these environment variables:

| Variable | Value |
|---|---|
| `GYRE_SERVER_URL` | Server base URL |
| `GYRE_REVIEW_TOKEN` / `GYRE_VALIDATION_TOKEN` | Scoped per-run Bearer token -- revoked on process exit |
| `GYRE_MR_ID` | UUID of the MR being reviewed |
| `GYRE_GATE_ID` | UUID of the gate triggering this run |
| `GYRE_GATE_AGENT_ID` | Identity to use when submitting reviews via API |
| `GYRE_DIFF_URL` | URL to fetch the MR diff |
| `GYRE_SPEC_REF` | Spec reference bound to the MR (if any) |
| `GYRE_PERSONA` | Persona file path for the gate |

**`AgentReview` protocol:** exit with any code; server checks for an Approved/ChangesRequested review submitted by `GYRE_GATE_AGENT_ID` after the process exits.

**`AgentValidation` protocol:** exit 0 = pass, non-zero = fail.

**Security:** each execution gets a unique `gyre_gate_<uuid>` token (revoked on completion even on crash/timeout); command split on whitespace -- no `sh -c` shell wrapper; 5-minute default timeout prevents hung gate agents.

**Merge processor dep failure handling (P5):** before processing each queued entry, the merge processor checks dependency health: if a dependency MR is `Closed`, the queue entry is marked `Failed` and a High-priority task `"Dependency MR-{id} was closed, reassess MR-{dependent}"` is auto-created; if a dependency has 3+ gate failures, an escalation warning is logged.

---

## Agent Compose Spec (M5.2)

Declarative multi-agent team blueprints. Apply via `POST /api/v1/compose/apply` with JSON body or YAML body (`Content-Type: application/yaml`).

**Example `agent-compose.yaml`:**
```yaml
agents:
  - name: orchestrator
    role: Orchestrator
    capabilities: [planning, decomposition]
    task:
      title: "Implement feature X"
      description: "Break down and delegate feature X"
      priority: high

  - name: backend-worker
    role: Developer
    parent: orchestrator
    capabilities: [rust, api-design]
    task:
      title: "Implement REST endpoints"
      priority: medium

  - name: reviewer
    role: Reviewer
    parent: orchestrator
    capabilities: [code-review]
```

Agents are created in dependency order (parents before children). Parent links are set automatically. After apply, poll `GET /api/v1/compose/status` for agent states. Call `POST /api/v1/compose/teardown` to stop all agents when done.

---

## Spec Lifecycle Automation (M13.8)

When an agent pushes to the **default branch** of any repo, the post-receive hook scans for changes to watched spec paths. If spec files are added, modified, deleted, or renamed, the server automatically creates a task and broadcasts a `SpecChanged` domain event.

**Watched paths** (changes outside these prefixes are ignored):
- `specs/system/`
- `specs/development/`

**Auto-created task titles and labels by change type:**

| `git diff --name-status` | Task title | Labels | Priority |
|---|---|---|---|
| `A` (Added) | `Implement spec: <path>` | `spec-implementation`, `auto-created` | Medium |
| `M` (Modified) | `Review spec change: <path>` | `spec-drift-review`, `auto-created` | High |
| `D` (Deleted) | `Handle spec removal: <path>` | `spec-deprecated`, `auto-created` | High |
| `R` (Renamed) | `Update spec references: <old> -> <new>` | `spec-housekeeping`, `auto-created` | Medium |

**Auto-revocation of spec approvals:** When a watched spec file is **modified**, **deleted**, or **renamed** in a push to the default branch, all active approvals for that path are automatically revoked. `revoked_by` is set to `"system:spec-lifecycle"` and `revocation_reason` records the push branch. For renames, approvals on the old path are revoked; the new path starts with no approvals.
