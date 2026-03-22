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

# Run with PostgreSQL
GYRE_DATABASE_URL=postgres://user:pass@localhost/gyre ./target/release/gyre-server
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
| `GYRE_DATABASE_URL` | _(unset — in-memory)_ | Database URL. Supports `sqlite://gyre.db` (SQLite) or `postgres://user:pass@host/db` (PostgreSQL). Diesel runs migrations automatically on startup. |
| `GYRE_REPOS_PATH` | `./repos/` | Directory for bare git repositories |
| `GYRE_BASE_URL` | `http://localhost:<port>` | Public URL used in clone URLs |
| `RUST_LOG` | `info` | Log level (`debug`, `trace`, `warn`) |
| `GYRE_OIDC_ISSUER` | _(disabled)_ | Keycloak realm URL — enables JWT auth |

See [AGENTS.md](AGENTS.md) for the full environment variable and API reference.

## What Gyre Does

- **Git forge** — bare repos on disk, Smart HTTP clone/push, branches, diffs, merge requests with reviews and a merge queue
- **Agent orchestration** — spawn agents with a dedicated worktree and auth token; agents push, open MRs, and self-complete
- **Quality gates** — per-repo gate definitions (test command, lint, required approvals, agent review and validation) enforced before merge
- **Agent identity** — built-in OIDC provider issues EdDSA JWT tokens per agent; tokens are revoked on agent completion; cross-instance JWT federation (G11)
- **Supply chain security** — per-push stack attestation, AIBOM generation, signed merge attestation bundles stored as git notes (G5)
- **Attribute-based access control** — ABAC policies evaluate JWT claims against repo-level rules; enforced on git push, agent spawn, and jj write endpoints (G6)
- **Container isolation** — agent workloads run in Docker/Podman containers with security defaults: `--network=none`, 2 GB memory cap, 512 PID limit, non-root user (G8); procfs liveness monitor detects crashed agent processes in real time (G7)
- **Workload attestation** — each agent records PID, hostname, and stack fingerprint at spawn; heartbeat re-checks process liveness; attestation embedded in JWT claims (G10)
- **SSH compute targets** — run agents on remote hosts via SSH; reverse tunnels (`-R`) let air-gapped agents phone home through NAT without inbound firewall rules (G12)
- **Full audit trail** — every agent action logged; eBPF syscall capture; SIEM forwarding

## Tech Stack

- **Rust** - server, CLI, agent runtime
- **Svelte 5 + shadcn-svelte** - web UI (pre-built, no Node required to run)
- **SQLite + PostgreSQL via Diesel ORM** - type-safe queries, auto-migrations on startup; `sqlite://` for default persistence, `postgres://` for production scale
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
| M12: Quality Gates | Done | Merge queue gates, repo mirroring, diff viewer with syntax highlighting |
| M13: Forge Native | Done | Pre-accept validation, commit provenance, zero-latency feedback, cross-agent code awareness, agent gates + spec binding |
| M14: Supply Chain Security | Done | Agent stack fingerprinting, push attestation, AIBOM generation |
| M15: Diesel ORM | Done | Diesel ORM + migrations, SQLite + PostgreSQL adapters, full persistence, multi-tenancy (tenant_id scoping) |
| M16: Security Hardening | Done | Constant-time token compare, SHA-256 API key hashing, path redaction, CORS hardening, audit guard, SSH host key enforcement |
| M17: Integration Testing | Done | 67 REST API contract tests, 21 auth/RBAC tests, 12 git smart HTTP tests, 20 Playwright E2E tests |
| M18: Agent Identity | Done | EdDSA JWT agent tokens, built-in OIDC provider, token introspection, JWT revocation on complete, stale-spec detection |
| M19: Container Runtime | Done | Docker/Podman ContainerTarget with security defaults, procfs agent monitor, workload attestation, SSH compute targets + reverse tunnels |
| M20: UI Accountability | Done | 19 frontend findings resolved: admin panels (SIEM/Compute/Network/Spawn Log), Repo Detail Policy/Activity/Gates tabs, MR dependency panel, merge queue DAG view, task detail view, token introspection, spec approvals view |
| M21: Spec Registry | In Progress | Spec manifest (`specs/manifest.yaml`), ledger state machine (Approved/Pending/Deprecated), 8 new `/api/v1/specs/` endpoints, Spec Dashboard UI |
| M22: Spec Links | In Progress | `SpecLink`/`SpecLinkType` in manifest; 6 link types (implements, supersedes, depends_on, conflicts_with, extends, references); forge enforcement (supersedes→deprecate, extends→drift); approval gates; 2 new endpoints (`/specs/{path}/links`, `/specs/graph`) |

741 Rust + 95 vitest component + 20 Playwright E2E tests passing (including E2E Ralph loop integration test). Hexagonal architecture enforced mechanically.

See [`specs/`](specs/index.md) for full specifications and [`AGENTS.md`](AGENTS.md) for the complete API and developer reference.

## License

Apache 2.0 — Copyright 2026 Red Hat, Inc. See [LICENSE](LICENSE).
