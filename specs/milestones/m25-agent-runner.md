# M25: Agent Runner

**Status:** Done

## Goal

Make Gyre self-driving out of the box: one `docker build` + one env var, and agents run autonomously using Claude Code without any manual compute target setup.

## Deliverables

### M25.1 — Bundled Claude Code runner image

`docker/gyre-agent/Dockerfile` upgraded from Ubuntu 22.04 to Node 22 Alpine. `@anthropic-ai/claude-agent-sdk` installed globally at a pinned version (`0.2.81`) for reproducible builds.

`docker/gyre-agent/agent-runner.mjs` (new) — autonomous agent script:
1. Reads task details from `GYRE_TASK_ID` / `GYRE_SERVER_URL`
2. Connects to Gyre's MCP server (`/mcp`) using the agent's pre-minted JWT
3. Uses Claude Agent SDK (`query()`) to implement the task in `/workspace/repo`
4. Commits and pushes the branch via git
5. Calls `gyre_agent_complete` MCP tool — server opens the MR, marks task done, revokes token

`entrypoint.sh` updated: when `GYRE_AGENT_COMMAND` is unset, defaults to `node /gyre/agent-runner.mjs` instead of sleeping.

### M25.2 — Zero-config compute target registration

On server startup, if Docker or Podman is detected on `PATH` and `GYRE_AGENT_CREDENTIALS` is set, the server auto-registers a compute target named `gyre-agent-default` with:
- `target_type`: `container`
- `config`: `{"image": "gyre-agent:latest", "network": "bridge"}`
- Credentials forwarded into every container spawn from `GYRE_AGENT_CREDENTIALS`

The spawn modal pre-selects `gyre-agent-default` when it exists — one fewer click for operators.

### M25.3 — GYRE_AGENT_CREDENTIALS passthrough

New server env var `GYRE_AGENT_CREDENTIALS`: newline-separated `KEY=value` pairs injected into every container agent spawn. Allows passing `ANTHROPIC_API_KEY` (and any other secrets) into agent containers without embedding them in the compute target config or image.

## Operator Workflow (zero-config)

```bash
# 1. Build the image once
docker build -t gyre-agent:latest docker/gyre-agent/

# 2. Start server with API key — gyre-agent-default auto-registered
GYRE_AGENT_CREDENTIALS=ANTHROPIC_API_KEY=sk-ant-xxx cargo run -p gyre-server

# 3. Open http://localhost:3000, create a task, click Spawn Agent
#    Compute target pre-selected as gyre-agent-default — just fill name + repo + task
```

No manual compute target creation, no manual credential wiring, no manual Docker run.

## Security Notes

- `GYRE_AGENT_CREDENTIALS` values are injected at container start and never stored in the DB
- `--network=none` G8 invariant: `gyre-agent-default` uses bridge networking explicitly to allow MCP/API calls back to the server; all other container targets default to `--network=none`
- Agent JWT pre-minted at spawn (M24) and revoked on `gyre_agent_complete` (M18)
- SDK version pinned in Dockerfile for supply chain reproducibility

## Files Changed

| File | Change |
|---|---|
| `docker/gyre-agent/Dockerfile` | Node 22 Alpine base, pinned SDK version |
| `docker/gyre-agent/agent-runner.mjs` | New — autonomous Claude Code runner |
| `docker/gyre-agent/entrypoint.sh` | Default to `agent-runner.mjs` when `GYRE_AGENT_COMMAND` unset |
| `crates/gyre-server/src/lib.rs` | Auto-register `gyre-agent-default` on startup |
| `crates/gyre-server/src/api/spawn.rs` | Inject `GYRE_AGENT_CREDENTIALS` into container env |
| `crates/gyre-server/src/main.rs` | Read `GYRE_AGENT_CREDENTIALS` env var |
| `web/src/components/AgentList.svelte` | Pre-select `gyre-agent-default` in spawn modal |
