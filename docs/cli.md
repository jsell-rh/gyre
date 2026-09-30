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

Config file is stored at `~/.gyre/config` (JSON):

```json
{
  "server": "http://localhost:3000",
  "token": "<per-agent-auth-token>",
  "agent_id": "<uuid>",
  "agent_name": "my-agent"
}
```

### Bootstrap (platform-model.md §8)

```bash
# Full non-dev bootstrap: tenant, admin user, workspace, repo, built-in
# personas, default gates, and a repo orchestrator — one command.
gyre bootstrap \
  --tenant "Acme Corp" \
  --workspace "Platform Team" \
  --repo gyre \
  --repo-path /home/user/code/gyre \
  --admin-user jsell \
  --oidc-issuer https://keycloak.example.com/realms/acme \
  --server http://localhost:3000 \
  --token gyre-dev-token

# Dev mode: skips OIDC + admin user, uses static tokens, tenant "dev",
# workspace "default"
gyre bootstrap --dev

# Also write a starter spec structure (specs/manifest.yaml, specs/index.md,
# specs/system/design-principles.md, AGENTS.md, .prek.yaml) into --repo-path
gyre bootstrap --dev --starter-kit --repo-path ./myrepo
```

Steps performed in order: health check → create tenant → create admin user
(skipped in `--dev`) + API key saved to `~/.gyre/config` → create workspace →
register repo (bare repo initialized server-side) → register + pre-approve the
four built-in personas (workspace-orchestrator, repo-orchestrator,
accountability, security) → spec registry check (syncs on first push) →
default gates (`cargo test`, `cargo clippy`, `scripts/check-arch.sh`, detected
from `--repo-path`) → spawn the repo orchestrator → print summary with all
IDs, the one-time API key, and URLs.

The run is resumable: re-running `gyre bootstrap` with the same names reuses
already-created resources (tenant, workspace, repo, personas are matched by
slug/name; the admin user's saved API key in `~/.gyre/config` is reused; a
live repo orchestrator is kept rather than re-spawned). If a step still fails,
the CLI reports which steps completed. The admin API key is shown exactly
once. Auth token precedence: `--token` flag > `GYRE_AUTH_TOKEN` env var >
`gyre-dev-token` default.

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

## Spec Operations

```bash
# Get LLM-suggested edits for a spec file
gyre spec assist system/vision.md "Add a section on observability"
gyre spec assist system/vision.md "Simplify principle 3" --repo myrepo --workspace myws

# Show all links (outbound and inbound) for a spec
gyre spec links system/identity-security.md

# Show specs that depend on a given spec (impact analysis)
gyre spec dependents system/source-control.md

# Full tenant-wide spec dependency graph (text summary)
gyre spec graph

# Spec graph in Graphviz DOT format (pipe to dot for PNG)
gyre spec graph --format dot | dot -Tpng -o graph.png

# List all stale spec links across the tenant
gyre spec stale-links

# List all active conflicts (specs with conflicts_with where both are approved)
gyre spec conflicts
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
