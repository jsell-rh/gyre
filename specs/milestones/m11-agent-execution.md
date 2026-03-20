# M11: Agent Execution Engine

## Goal

Agents actually run. Spawn creates a real process on a compute target, the UI
can attach to the agent's TTY, and agent lifecycle events flow through the
system in real-time.

## Problem

Currently `POST /api/v1/agents/spawn` creates a database record but doesn't
launch any process. The compute target adapters (Local, Docker, SSH) exist but
aren't wired to the spawn flow. There's no TTY attach capability.

## Deliverables

### M11.1 Spawn Execution
- Wire spawn API to compute target adapters
- Local target: launch `gyre-cli agent run` as a child process
- Docker target: `docker run` with the Gyre image
- SSH target: remote command execution
- Agent process lifecycle: start -> running -> heartbeat -> complete/dead
- Stale agent detector kills processes that miss heartbeats
- Process stdout/stderr captured and stored

### M11.2 TTY Attach
- WebSocket endpoint: `GET /ws/agents/{id}/tty`
- Server proxies between browser WebSocket and agent process PTY
- Frontend TTY component using xterm.js
- Read-only mode by default, interactive mode with auth
- Agent detail view shows live terminal output

### M11.3 Agent Logs & Output
- `GET /api/v1/agents/{id}/logs` — paginated log output
- Log streaming via SSE or WebSocket
- Frontend log viewer with auto-scroll and search
- Logs persisted to disk, rotated by retention policy

## Acceptance Criteria

- [ ] Spawning an agent from the UI launches a real process
- [ ] Agent heartbeats are visible in real-time
- [ ] User can attach to a running agent's terminal from the browser
- [ ] Agent completion/failure updates UI automatically
- [ ] Stale agents are detected and cleaned up
