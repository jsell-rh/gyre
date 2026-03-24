# Gyre CLI Reference

The `gyre` CLI is a thin client for interacting with a Gyre server.

---

## Setup (M3.3)

```bash
# Register this CLI instance as a named agent; saves token + agent ID to ~/.gyre/config
gyre init --server http://localhost:3000 --name my-agent --token gyre-dev-token

# Clone a Gyre-hosted repository (uses token from ~/.gyre/config)
gyre clone myproject/myrepo            # clones into ./myrepo/
gyre clone myproject/myrepo --dir /tmp/work

# Push current branch (uses token from ~/.gyre/config)
gyre push                              # pushes to origin
gyre push --remote gyre
```

Config file is stored at `~/.gyre/config` (TOML):

```toml
server = "http://localhost:3000"
token = "<per-agent-auth-token>"
agent_id = "<uuid>"
agent_name = "my-agent"
```

---

## Agent Operations (M3.3)

```bash
# Show this agent's registered status and current task
gyre status

# List tasks (optional filters)
gyre tasks list
gyre tasks list --status in_progress
gyre tasks list --mine                 # only tasks assigned to this agent

# Assign a task to this agent and mark it in_progress
gyre tasks take <task-id>
```

---

## Merge Requests (M3.3)

```bash
# Create a merge request for the current branch
gyre mr create --title "My feature" --repo-id <repo-uuid>

# Custom source/target branches
gyre mr create --title "Fix bug" --repo-id <repo-uuid> \
  --source fix/my-bug --target main
```

---

## Connection / Diagnostics

```bash
# Connect to a running gyre-server (interactive session)
gyre connect --server ws://localhost:3000/ws --token gyre-dev-token

# Ping the server and measure round-trip time
gyre ping --server ws://localhost:3000/ws --token gyre-dev-token

# Check server health via HTTP
gyre health --server http://localhost:3000

# Launch the TUI dashboard (exits on 'q')
gyre tui --server ws://localhost:3000/ws --token gyre-dev-token
```

Default `--server` is `ws://localhost:3000/ws` and default `--token` is `gyre-dev-token`
(matches server defaults, so bare `gyre ping` works against a local dev server).
