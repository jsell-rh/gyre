# Milestone 0: Walking Skeleton

The thinnest possible vertical slice through the entire system, proving the development loop and architecture work end-to-end.

## Deliverables

### 1. Repo Scaffold

- Rust workspace (cargo workspace with hexagonal crate structure)
- NixOS flake for reproducible builds
- Pre-commit hooks (cargo fmt, clippy, deny warnings, architecture lint stubs, em-dash check)
- GitHub Actions CI (build, test, lint)
- Conventional commits + auto-versioning setup
- `specs/index.md` with this spec decomposed into agent-optimized files
- AGENTS.md as the map

### 2. Server Boots

- Rust HTTP server (axum or similar)
- SQLite storage behind an abstracted port
- WebSocket endpoint for agent connections
- REST health check endpoint
- Basic OTel tracing wired in
- Serves embedded Svelte 5 SPA (hello world)

### 3. CLI Connects

- Cross-platform Rust CLI binary
- Connects to server via WebSocket
- Authenticates (even if just a static token for now)
- Sends/receives a ping/pong message
- TUI stub (ratatui) showing connection status

### 4. One Ralph Loop Executes

- Manager agent (in agent-boss) dispatches a sub-agent
- Sub-agent works in a git worktree
- Sub-agent implements a trivial change, self-reviews, runs tests, opens a PR
- Manager merges the PR
- Full cycle visible in activity log

## Success Criteria

- `nix build` produces server binary, CLI binary, and Docker image from the same definition
- CI is green
- An agent can connect to the server and exchange messages
- One complete Ralph loop lands a PR with no human intervention
- Architecture lints pass (hexagonal boundaries enforced)

## Non-Goals for Milestone 0

- No SSO/SCIM, no ABAC, no SPIFFE, no WireGuard
- No admin panel beyond health check
- No analytics, no feature flags
- No eBPF, no SIEM
- No i18n
- No remote compute provisioning
