# AGENTS.md - Gyre Agent Entry Point

Gyre is an autonomous software development platform built in Rust.
Humans design (specs), orchestrators decompose (tasks), agents implement (Ralph loops).
See [specs/system/agent-runtime.md](specs/system/agent-runtime.md) §1 for the canonical agent lifecycle definition.

**This file is the map. Follow the links below for detail.**

---

## Quick Start

```bash
# Build and run (requires Node.js ≥ 18 + npm — build.rs auto-builds the Svelte UI)
cargo build --all
cargo run -p gyre-server        # starts on port 3000, token: gyre-dev-token

# Rust-only build (skip web rebuild, uses committed web/dist/)
SKIP_WEB_BUILD=1 cargo build -p gyre-server

# Run tests
cargo test --all
cd web && npm test && cd ..     # frontend component tests (vitest)

# Access
open http://localhost:3000      # Svelte SPA dashboard
```

See [docs/server-config.md](docs/server-config.md) for all configuration options.

---

## Codebase Map

| Layer | Crate | Role |
|---|---|---|
| Shared types | `gyre-common` | Errors, Id, protocol types -- no external deps |
| Interfaces | `gyre-ports` | Port traits (interfaces) -- no infrastructure deps |
| Domain logic | `gyre-domain` | Pure business logic -- depends ONLY on ports + common |
| Adapters | `gyre-adapters` | SQLite, Diesel ORM implementations -- implements ports |
| Server | `gyre-server` | HTTP/WebSocket server -- wires domain + adapters |
| CLI | `gyre-cli` | Terminal client + TUI |

**Hexagonal boundary invariant:** `gyre-domain` MUST NOT import `gyre-adapters` or any infrastructure crate. Enforced by `scripts/check-arch.sh` and CI.

---

## Documentation Index

**Before writing any code, read the docs relevant to your task.** This file is the entry point only — endpoint signatures, auth requirements, env vars, and protocol details all live in the docs/ files below.

| What you need | Where to look |
|---|---|
| **All API endpoints** (REST, git HTTP, WebSocket, MCP, A2A) — auth, roles, request/response shapes | [docs/api-reference.md](docs/api-reference.md) |
| **Running the server**, env vars, OIDC, database, WireGuard config | [docs/server-config.md](docs/server-config.md) |
| **Agent spawn/complete**, container env vars, gate protocol, spec lifecycle automation | [docs/agent-protocol.md](docs/agent-protocol.md) |
| **Building, testing**, branching, commit conventions, pre-commit hooks, architecture decisions | [docs/development.md](docs/development.md) |
| **Dashboard UI** -- workspace home, repo mode tabs, keyboard shortcuts, components | [docs/ui.md](docs/ui.md) |
| **CLI usage** -- init, clone, push, tasks, MRs, diagnostics | [docs/cli.md](docs/cli.md) |
| **Product specs** (vision, architecture, milestones) | [specs/index.md](specs/index.md) |

---

## Key Architecture Specs

| Topic | Spec |
|---|---|
| Tech stack + hexagonal invariants | [specs/development/architecture.md](specs/development/architecture.md) |
| Agent Runtime (lifecycle, signal chain, compute targets, budget, prompts) | [specs/system/agent-runtime.md](specs/system/agent-runtime.md) |
| Platform model (tenant/workspace/repo hierarchy, personas, orchestration) | [specs/system/platform-model.md](specs/system/platform-model.md) |
| Vision (7 principles: judgment not generation, right context, specs as artifact...) | [specs/system/vision.md](specs/system/vision.md) |
| Realized Model (knowledge graph extracted from code) | [specs/system/realized-model.md](specs/system/realized-model.md) |
| Meta-Spec Reconciliation (safe iteration on personas, principles, standards) | [specs/system/meta-spec-reconciliation.md](specs/system/meta-spec-reconciliation.md) |
| UI Navigation (workspace home, repo mode, no-sidebar model) | [specs/system/ui-navigation.md](specs/system/ui-navigation.md) |
| Agent Gates & Spec Binding | [specs/system/agent-gates.md](specs/system/agent-gates.md) |
| Spec Lifecycle Automation | [specs/system/spec-lifecycle.md](specs/system/spec-lifecycle.md) |
| ABAC policy engine | [specs/system/abac-policy-engine.md](specs/system/abac-policy-engine.md) |

---

## Milestone Status

M0 through M35 and HSI are all **Done**. See [specs/index.md](specs/index.md) for the full milestone table.

Current work: **Authorization Provenance** (`authorization-provenance.md`) -- cryptographic work authorization chain. See `specs/tasks/` for decomposed tasks.
