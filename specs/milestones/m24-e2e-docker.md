# M24: E2E Docker Agent Workflow

**Status:** Done

## Goal

Close the gap between "container target exists" and "agent can actually run a Ralph loop inside a container." M24 wires all the pieces so a container-spawned agent can bootstrap, clone its assigned repo, develop, heartbeat, and push without any manual intervention.

## Acceptance Criteria

### M24.1 — JWT Pre-mint and Container Env Injection

The server pre-mints the agent JWT _before_ spawning the container so the token can be injected as an environment variable. The workload attestation record is still created after spawn (queryable via `GET /api/v1/agents/{id}/workload`).

Seven standard environment variables are injected into every container agent at spawn:

| Variable | Value |
|---|---|
| `GYRE_SERVER_URL` | Server base URL |
| `GYRE_AUTH_TOKEN` | Pre-minted EdDSA JWT (revoked on complete) |
| `GYRE_CLONE_URL` | Git Smart HTTP URL for the assigned repo |
| `GYRE_BRANCH` | Branch name to clone and work on |
| `GYRE_AGENT_ID` | Agent UUID |
| `GYRE_TASK_ID` | Assigned task UUID |
| `GYRE_REPO_ID` | Repository UUID |

Optional: `GYRE_AGENT_COMMAND` — if set, the entrypoint execs this command after setup.

### M24.2 — Bridge Networking for Agent Containers

Container agent networking uses a two-tier model:

- **Agent containers** (spawned via `/api/v1/agents/spawn`): default to `bridge` networking so they can reach the Gyre server for clone, heartbeat, and complete calls. Override via `network` field on the compute target config.
- **Gate/validation containers** (spawned by the merge processor for `AgentReview`/`AgentValidation` gates): should use `--network=none` since they only inspect diffs and do not need server access.

### M24.3 — Configurable Entrypoint

The container compute target `config` JSON accepts two new fields:

| Field | Default | Description |
|---|---|---|
| `command` | `/gyre/entrypoint.sh` | Entrypoint binary to run |
| `args` | `[]` | Argument list passed to the command |

### M24.4 — Reference `gyre-agent` Docker Image

`docker/gyre-agent/` provides a minimal reference agent image:

- **Base**: `ubuntu:22.04` + `git` + `curl`
- **Entrypoint**: `/gyre/entrypoint.sh`

The entrypoint script:
1. Validates required env vars (`GYRE_SERVER_URL`, `GYRE_AUTH_TOKEN`, `GYRE_CLONE_URL`, `GYRE_BRANCH`, `GYRE_AGENT_ID`)
2. Configures git credentials using `GYRE_AUTH_TOKEN` as the HTTP password
3. Clones `GYRE_CLONE_URL` branch `GYRE_BRANCH` into `/workspace/repo`
4. Sends an initial heartbeat to `POST /api/v1/agents/{GYRE_AGENT_ID}/heartbeat`
5. Execs `GYRE_AGENT_COMMAND` if set, otherwise sleeps 1h for interactive use

Build the image:
```bash
docker build -t gyre-agent:latest docker/gyre-agent/
```

### M24.5 — Compute Target Dropdown in Spawn Modal

The "Spawn Agent" modal in the dashboard now includes a **Compute Target** dropdown. It fetches available targets from `GET /api/v1/admin/compute-targets` on mount and passes the selected `compute_target_id` in the spawn request body.

## How to Run the E2E Flow

1. Start the server with Docker available: `cargo run -p gyre-server`
2. Build the agent image: `docker build -t gyre-agent:latest docker/gyre-agent/`
3. Register a container compute target via Admin → Compute → Add (type: container, image: `gyre-agent:latest`)
4. Create a project + repo via the dashboard
5. Create a task
6. Open "Spawn Agent", select the container target, fill in repo/task/branch
7. The agent container boots, clones the repo, sends a heartbeat
8. Monitor via Agent List → Logs tab
9. When work is done, call `POST /api/v1/agents/{id}/complete` or implement it in `GYRE_AGENT_COMMAND`

## Files Changed

| File | Change |
|---|---|
| `crates/gyre-server/src/api/spawn.rs` | JWT pre-mint, env injection, bridge network default, configurable entrypoint |
| `docker/gyre-agent/Dockerfile` | New — minimal Ubuntu agent image |
| `docker/gyre-agent/entrypoint.sh` | New — bootstrap script |
| `web/src/components/AgentList.svelte` | Compute target dropdown in spawn modal |
