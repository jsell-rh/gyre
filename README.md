# Gyre

**Gyre** `/ˈdʒaɪər/` - an autonomous software development platform enabling agentic collaboration and orchestration.

Opinionated about governance, identity, and maximizing throughput and quality of code. Flexible about where and how agents run.

## What Gyre Does

Gyre is a self-contained platform that:
- **Is the source control host** - built-in git forge with merge requests and merge queues as first-class primitives
- **Orchestrates agents** - spawn, manage, and coordinate coding agents across any compute target
- **Engineers the Ralph loop** - automates the iterative develop → review → test → merge cycle and solves coordination between concurrent loops
- **Audits everything** - full traceability from agent context windows to eBPF syscall capture

## Tech Stack

- **Rust** - server, CLI, agent runtime
- **Svelte 5 + shadcn-svelte** - web UI
- **NixOS** - single definition builds server, Docker image, QEMU VM, LXC container
- **SQLite** (default) / **PostgreSQL** - abstracted storage
- **WireGuard** (Tailscale) - agent networking mesh
- **SPIFFE** - cryptographic agent identity

## Specs

All specifications live in [`specs/`](specs/index.md). Start there.

## Design Principles

| Principle | Detail |
|---|---|
| Simplicity | Minimal infrastructure stacks; avoid unnecessary complexity |
| Vertical scaling | Scale up before scaling out |
| NixOS as foundation | Single definition, many targets. Safe sudo for agents. |
| Engineer the Ralph Loop | Every decision evaluated by: does this make the loop faster, tighter, or more reliable? |
| Reconciliation as a primitive | Desired state → observe → converge → repeat |
| No shortcuts | The most correct way is mandated. Time is not a constraint - correctness is. |
| Specs first, always | No implementation without an approved spec |
| Single-minded agents | One agent, one task. Spin up, execute, tear down. |
| Opinionated by design | Where an opinion improves throughput or quality, enforce it. |

## Current Status

| Milestone | Status | Summary |
|---|---|---|
| M0: Walking Skeleton | Done | axum server, SQLite, Svelte SPA, WebSocket, CLI, one Ralph loop |
| M1: Domain Foundation | Done | 6-entity domain model, full CRUD REST API, Svelte dashboard, agent lifecycle |
| M2: Source Control | Done | Git forge, MR workflow + reviews, merge queue, agent-commit tracking, worktrees |
| M3: Agent Orchestration | In Progress | Smart HTTP git, agent spawn API, CLI client, end-to-end Ralph loop |

203 tests passing. Hexagonal architecture enforced mechanically.

See [`specs/`](specs/index.md) for full specifications.
