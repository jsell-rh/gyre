# AGENTS.md - Gyre Agent Entry Point

Gyre is an autonomous software development platform built in Rust.
It orchestrates AI agents through a structured Ralph loop: spec, implement, review, merge.

For full specs: [specs/index.md](specs/index.md)

---

## Crate Structure (Hexagonal Architecture)

```
crates/
  gyre-common/     # Shared types, errors, Id - no external deps
  gyre-ports/      # Port traits (interfaces) - no infrastructure deps
  gyre-domain/     # Pure domain logic - depends ONLY on ports + common
  gyre-adapters/   # Adapter implementations (SQLite, etc.) - implements ports
  gyre-server/     # Binary: HTTP/WS server - wires domain + adapters
  gyre-cli/        # Binary: CLI + TUI - thin client
```

**Hexagonal boundary invariant:**
- `gyre-domain` MUST NOT import `gyre-adapters` or any infrastructure crate.
- Violation is caught by `scripts/check-arch.sh` and CI (will fail the build).

Dependency flow:
```
gyre-server ──> gyre-domain ──> gyre-ports ──> gyre-common
gyre-server ──> gyre-adapters ──> gyre-ports ──> gyre-common
gyre-cli    ──> gyre-common
```

---

## Key Commands

```bash
# Build everything
cargo build --all

# Build release binaries
cargo build --release -p gyre-server -p gyre-cli

# Run all tests
cargo test --all

# Format check
cargo fmt --all -- --check

# Lint (warnings are errors)
cargo clippy --all-targets --all-features -- -D warnings

# Architecture lint (enforces hexagonal boundaries)
bash scripts/check-arch.sh

# Auto-format
cargo fmt --all

# Watch mode (requires cargo-watch)
cargo watch -x "test --all"
```

---

## Running the Server

```bash
# Dev mode (defaults: port 3000, token gyre-dev-token, db gyre.db)
cargo run -p gyre-server

# With custom settings
GYRE_PORT=8080 GYRE_AUTH_TOKEN=my-token GYRE_DB_PATH=/tmp/gyre.db RUST_LOG=debug \
  cargo run -p gyre-server

# Release build
cargo build --release -p gyre-server && ./target/release/gyre-server
```

### Server Endpoints

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/health` | Returns `{"status":"ok","version":"0.1.0"}` |
| `GET` | `/ws` | WebSocket upgrade (requires `Auth` handshake first) |
| `GET` | `/api/v1/version` | Returns `{"name":"gyre","version":"0.1.0","milestone":"M0"}` |
| `GET` | `/api/v1/activity` | Query activity log (`?since=&limit=&agent_id=&event_type=`) |
| `POST/GET` | `/api/v1/projects` | Create / list projects |
| `GET/PUT/DELETE` | `/api/v1/projects/{id}` | Read / update / delete project |
| `POST/GET` | `/api/v1/repos` | Create / list repos (`?project_id=`) |
| `GET` | `/api/v1/repos/{id}` | Get repository |
| `GET` | `/api/v1/repos/{id}/branches` | List branches in repository |
| `GET` | `/api/v1/repos/{id}/commits` | Commit log (`?branch=<name>&limit=50`) |
| `GET` | `/api/v1/repos/{id}/diff` | Diff between refs (`?from=<ref>&to=<ref>`) |
| `POST/GET` | `/api/v1/agents` | Register (returns auth_token) / list (`?status=`) |
| `GET` | `/api/v1/agents/{id}` | Get agent |
| `PUT` | `/api/v1/agents/{id}/status` | Update agent status |
| `PUT` | `/api/v1/agents/{id}/heartbeat` | Agent heartbeat |
| `POST/GET` | `/api/v1/agents/{id}/messages` | Send/poll agent messages |
| `POST/GET` | `/api/v1/tasks` | Create / list (`?status=&assigned_to=&parent_task_id=`) |
| `GET/PUT` | `/api/v1/tasks/{id}` | Read / update task |
| `PUT` | `/api/v1/tasks/{id}/status` | Transition task status |
| `POST/GET` | `/api/v1/merge-requests` | Create / list (`?status=&repository_id=`) |
| `GET` | `/api/v1/merge-requests/{id}` | Get merge request |
| `PUT` | `/api/v1/merge-requests/{id}/status` | Transition MR status |
| `POST/GET` | `/api/v1/merge-requests/{id}/comments` | Add / list review comments |
| `POST/GET` | `/api/v1/merge-requests/{id}/reviews` | Submit / list reviews (approve/request changes) |
| `GET` | `/api/v1/merge-requests/{id}/diff` | Get MR diff |
| `POST` | `/api/v1/merge-queue/enqueue` | Add approved MR to merge queue |
| `GET` | `/api/v1/merge-queue` | List merge queue entries (priority ordered) |
| `DELETE` | `/api/v1/merge-queue/{id}` | Cancel queued entry |
| `POST` | `/api/v1/repos/{id}/commits/record` | Record agent-commit mapping |
| `GET` | `/api/v1/repos/{id}/agent-commits` | Query commits by agent (`?agent_id=`) |
| `POST/GET` | `/api/v1/repos/{id}/worktrees` | Create / list worktrees |
| `DELETE` | `/api/v1/repos/{id}/worktrees/{wt_id}` | Delete worktree |
| `GET` | `/git/{project}/{repo}/info/refs?service=git-upload-pack` | Smart HTTP clone/fetch capability advertisement |
| `GET` | `/git/{project}/{repo}/info/refs?service=git-receive-pack` | Smart HTTP push capability advertisement |
| `POST` | `/git/{project}/{repo}/git-upload-pack` | Git clone / fetch data |
| `POST` | `/git/{project}/{repo}/git-receive-pack` | Git push data (triggers post-receive: records agent-commit mapping) |
| `GET` | `/*` | Svelte SPA dashboard (served from `web/dist/`) |

### HTTP Bearer Token Auth (Git endpoints)

All `/git/` endpoints require `Authorization: Bearer <token>`. Two valid token types:

| Token | How to get | Who uses it |
|-------|-----------|-------------|
| Global token | `GYRE_AUTH_TOKEN` env var (default: `gyre-dev-token`) | System/dev use |
| Per-agent token | Returned in body of `POST /api/v1/agents` registration | Agent use — preferred |

```bash
# Clone a repo as an agent
git clone http://localhost:3000/git/my-project/my-repo.git
# Git will prompt for credentials — use any username, token as password
# Or embed in URL:
git clone http://agent-token@localhost:3000/git/my-project/my-repo.git
```

### Server Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `GYRE_PORT` | `3000` | TCP port to listen on |
| `GYRE_AUTH_TOKEN` | `gyre-dev-token` | Global auth token (WS `Auth` handshake + git HTTP Bearer) |
| `GYRE_DB_PATH` | `gyre.db` | SQLite database file path |
| `RUST_LOG` | `info` | Log level filter (e.g. `debug`, `gyre_server=trace`) |

### WebSocket Protocol (`gyre-common::WsMessage`)

All WS messages are JSON with a `"type"` discriminant. Auth must be the first message.
See `crates/gyre-common/src/protocol.rs` for the full type definitions.

```json
// 1. Auth handshake (required first):
{"type":"Auth","token":"gyre-dev-token"}
{"type":"AuthResult","success":true,"message":"authenticated"}

// 2. Liveness probe:
{"type":"Ping","timestamp":1234567890}
{"type":"Pong","timestamp":1234567890}

// 3. Record an activity event (server stores + broadcasts to all clients):
{"type":"ActivityEvent","event_id":"abc","agent_id":"server","event_type":"task.started","description":"Task started","timestamp":1234567890}

// 4. Query activity log over WebSocket:
{"type":"ActivityQuery","since":1234567800,"limit":50}
{"type":"ActivityResponse","events":[...]}
```

The in-memory `ActivityStore` holds up to 1000 events (oldest dropped when full).
The same events are also queryable via `GET /api/v1/activity?since=<ts>&limit=<n>`.

> `web/dist/` is committed so the server can serve the SPA without requiring `npm` at build
> time. Agents and CI do not need Node installed to build or run `gyre-server`.

---

## CLI Usage

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

---

## Branching Convention

| Branch pattern | Purpose |
|---|---|
| `main` | Always green, deployable |
| `feat/<name>` | New features |
| `fix/<name>` | Bug fixes |
| `chore/<name>` | Maintenance, deps, tooling |
| `docs/<name>` | Documentation only |
| `ci/<name>` | CI/CD changes |

Rules:
- Branch from `main`.
- All work lands via PR.
- PRs require CI green before merge.
- No force-push to `main`.

---

## Commit Message Convention

Format: `<type>(<scope>): <description>`

| Type | When to use |
|---|---|
| `feat` | New feature or capability |
| `fix` | Bug fix |
| `docs` | Documentation changes only |
| `style` | Formatting, no logic change |
| `refactor` | Code change that is not a fix or feature |
| `perf` | Performance improvement |
| `test` | Adding or fixing tests |
| `build` | Build system, Cargo.toml changes |
| `ci` | CI/CD pipeline changes |
| `chore` | Dependency updates, tooling |
| `revert` | Revert a previous commit |

Scope is optional but recommended. Use the crate name or subsystem.

Examples:
```
feat(server): add WebSocket endpoint for agent connections
fix(domain): correct task status transition from review to done
docs(agents): update AGENTS.md with new crate structure
ci: cache cargo target directory in GitHub Actions
build(gyre-ports): add async-trait dependency
```

Enforced by `scripts/check-commit-msg.sh` (pre-commit hook on commit-msg stage).

---

## Architecture Decisions

Key specs to read before making changes:

| Topic | Spec |
|---|---|
| Tech stack + hexagonal invariants | [specs/development/architecture.md](specs/development/architecture.md) |
| Design principles (invariants) | [specs/system/design-principles.md](specs/system/design-principles.md) |
| M0 milestone deliverables | [specs/milestones/m0-walking-skeleton.md](specs/milestones/m0-walking-skeleton.md) |
| Agent experience + legibility | [specs/development/agent-experience.md](specs/development/agent-experience.md) |
| CI, docs, release | [specs/development/ci-docs-release.md](specs/development/ci-docs-release.md) |

---

## Pre-Commit Hooks

Install once per clone:
```bash
pre-commit install
pre-commit install --hook-type commit-msg
```

Hooks run automatically on `git commit`. To run manually:
```bash
pre-commit run --all-files
```

Hook summary:
- `cargo-fmt`: formatting check
- `cargo-clippy`: lint with denied warnings
- `arch-lint`: hexagonal boundary enforcement
- `no-em-dash`: rejects Unicode em-dashes in source
- `conventional-commits`: commit message format (commit-msg stage)
