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

**Milestone 0 (Walking Skeleton) - Complete**

| Deliverable | Status |
|---|---|
| M0.1 Repo Scaffold | Done - Cargo workspace, Nix flake, CI, pre-commit hooks |
| M0.2 Server Boots | Done - axum HTTP/WS, SQLite, Svelte SPA, OTel tracing |
| M0.3 CLI Connects | Done - clap CLI, WebSocket client, ratatui TUI |
| M0.4 One Ralph Loop | Done - Activity log, full Ralph loop with activity trail |

26 tests passing. Hexagonal architecture enforced mechanically.

See [`specs/milestones/m0-walking-skeleton.md`](specs/milestones/m0-walking-skeleton.md) for full spec.
