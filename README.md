# Gyre

**Gyre** `/ˈdʒaɪər/` - an autonomous software development platform enabling agentic collaboration and orchestration.

Built-in git forge, merge queue, agent orchestration, and full audit trail. Agents collaborate through a structured Ralph loop: spec, implement, review, merge.

## Quick Start

**Prerequisites:** Rust (stable), git

```bash
# Build
cargo build --release -p gyre-server -p gyre-cli

# Run (in-memory, stateless — good for dev/eval)
./target/release/gyre-server

# Run with persistent SQLite storage
GYRE_DATABASE_URL=sqlite://gyre.db ./target/release/gyre-server
```

Open **http://localhost:3000** — the dashboard loads immediately. Default auth token: `gyre-dev-token`.

Seed demo data to explore the UI:
```bash
curl -s -X POST http://localhost:3000/api/v1/admin/seed \
  -H "Authorization: Bearer gyre-dev-token"
```
_(Pipe through `jq .` for pretty output if you have jq installed.)_

## CLI

The CLI commands below require the server to be running. `gyre init` registers
this CLI instance and saves credentials to `~/.gyre/config`; all other commands
use those saved credentials automatically.

```bash
# Register as a named agent (saves token + agent ID to ~/.gyre/config)
./target/release/gyre init --name my-agent

# Check server health
./target/release/gyre health

# List tasks
./target/release/gyre tasks list

# Clone a repo hosted by Gyre (use project/repo names from the dashboard)
./target/release/gyre clone myproject/myrepo
```

## Configuration

All settings are environment variables. The server starts with safe defaults — nothing is required.

| Variable | Default | Description |
|---|---|---|
| `GYRE_PORT` | `3000` | HTTP/WS listen port |
| `GYRE_AUTH_TOKEN` | `gyre-dev-token` | Bearer token for API auth |
| `GYRE_DATABASE_URL` | _(unset — in-memory)_ | SQLite URL for persistence, e.g. `sqlite://gyre.db` |
| `GYRE_REPOS_PATH` | `./repos/` | Directory for bare git repositories |
| `GYRE_BASE_URL` | `http://localhost:<port>` | Public URL used in clone URLs |
| `RUST_LOG` | `info` | Log level (`debug`, `trace`, `warn`) |
| `GYRE_OIDC_ISSUER` | _(disabled)_ | Keycloak realm URL — enables JWT auth |

See [AGENTS.md](AGENTS.md) for the full environment variable and API reference.

## What Gyre Does

- **Git forge** — bare repos on disk, Smart HTTP clone/push, branches, diffs, merge requests with reviews and a merge queue
- **Agent orchestration** — spawn agents with a dedicated worktree and auth token; agents push, open MRs, and self-complete
- **Quality gates** — per-repo gate definitions (test command, lint, required approvals) enforced before merge
- **Full audit trail** — every agent action logged; eBPF syscall capture; SIEM forwarding

## Tech Stack

- **Rust** - server, CLI, agent runtime
- **Svelte 5 + shadcn-svelte** - web UI (pre-built, no Node required to run)
- **SQLite** - default storage; WAL mode, full persistence when `GYRE_DATABASE_URL` is set
- **NixOS** - single definition builds server, Docker image, QEMU VM, LXC container
- **WireGuard** - agent networking mesh

## Current Status

| Milestone | Status | Summary |
|---|---|---|
| M0: Walking Skeleton | Done | axum server, SQLite, Svelte SPA, WebSocket, CLI, one Ralph loop |
| M1: Domain Foundation | Done | 6-entity domain model, full CRUD REST API, Svelte dashboard, agent lifecycle |
| M2: Source Control | Done | Git forge, MR workflow + reviews, merge queue, agent-commit tracking, worktrees |
| M3: Agent Orchestration | Done | Smart HTTP git, agent spawn API, CLI client, end-to-end Ralph loop |
| M4: Identity & Observability | Done | Keycloak SSO + JWT auth, RBAC roles, OpenTelemetry tracing, Prometheus metrics |
| M5: Agent Protocols | Done | MCP server, A2A protocol, AG-UI events, jj VCS, agent compose spec, M5 dashboard |
| M6: Infrastructure & Operations | Done | Product analytics, cost tracking, BCP snapshot/restore, background job framework, M6 dashboard |
| M7: Production Hardening | Done | eBPF audit, SIEM forwarding, NixOS packaging, remote compute targets, WireGuard mesh, production hardening |
| M8: Frontend Excellence | Done | Red Hat brand design system, polished dashboard UX, component library, consistent user journeys, dark theme |
| M9: Functional UI | Done | Seed data endpoint, CRUD modals (projects/repos/tasks), auth token integration, end-to-end user journeys |
| M10: Persistent Storage | Done | SQLite persistence, real-time WebSocket events, git repo management |
| M11: Agent Execution | Done | Real agent processes, TTY attach, agent logs, execution lifecycle |
| M12: Quality Gates | In Progress | Merge queue gates, repo mirroring, diff viewer |
| M13: Forge Native | In Progress | Pre-accept validation, commit provenance, zero-latency feedback, cross-agent code awareness |
| M14: Supply Chain Security | In Progress | Agent stack fingerprinting, push attestation, AIBOM generation |

554 Rust + 31 frontend component tests passing (including E2E Ralph loop integration test). Hexagonal architecture enforced mechanically.

See [`specs/`](specs/index.md) for full specifications and [`AGENTS.md`](AGENTS.md) for the complete API and developer reference.

## License

Apache 2.0 — Copyright 2026 Red Hat, Inc. See [LICENSE](LICENSE).
