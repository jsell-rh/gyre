---
title: "Dependency graph — CLI commands and UI visualization"
spec_ref: "dependency-graph.md §CLI"
depends_on: [task-163]
progress: not-started
coverage_sections:
  - "dependency-graph.md §CLI"
  - "dependency-graph.md §UI"
commits: []
---

## Spec Excerpt

### §CLI (dependency-graph.md)

The CLI should provide commands for inspecting the cross-repo dependency graph:

- `gyre deps show [repo]` — list dependencies of a repo (outgoing edges)
- `gyre deps impact [repo]` — blast radius: direct and transitive dependents
- `gyre deps stale` — list stale dependencies across the workspace
- `gyre deps breaking` — list unacknowledged breaking changes

### §UI (dependency-graph.md)

A dedicated dependency graph visualization showing cross-repo relationships, version drift, and breaking change indicators.

## Implementation Plan

### CLI Commands

1. **`gyre deps show [repo]`**: Call `GET /api/v1/repos/:id/dependencies`. Display table: target repo, type, version pinned, version current, drift, status.

2. **`gyre deps impact [repo]`**: Call `GET /api/v1/repos/:id/blast-radius`. Display direct and transitive dependents as a tree.

3. **`gyre deps stale`**: Call `GET /api/v1/dependencies/stale`. Display table of stale dependency edges.

4. **`gyre deps breaking`**: Call `GET /api/v1/dependencies/breaking`. Display unacknowledged breaking changes with dependency context.

5. Add `deps` subcommand to the CLI command tree in `gyre-cli`.

### UI Components

1. **DependencyGraphView.svelte**: A workspace-scope component showing the cross-repo dependency DAG. Nodes are repos, edges are dependency relationships. Color-code edges by status (green=Active, yellow=Stale, red=Breaking, gray=Orphaned).

2. **DependencyDetail panel**: Clicking a dependency edge shows: source/target artifacts, version info, drift, breaking changes, acknowledgment status.

3. Wire into the Admin → Workspace scope as a "Dependencies" tab or into the Explorer workspace view.

## Acceptance Criteria

- [ ] `gyre deps show <repo>` displays outgoing dependencies
- [ ] `gyre deps impact <repo>` displays blast radius
- [ ] `gyre deps stale` lists stale dependencies
- [ ] `gyre deps breaking` lists unacknowledged breaking changes
- [ ] Web UI shows dependency graph at workspace scope
- [ ] Dependency edges color-coded by status
- [ ] `cargo test --all` and `cd web && npm test` pass

## Agent Instructions

Read `specs/system/dependency-graph.md` §CLI and §UI. For CLI: follow the existing command patterns in `crates/gyre-cli/src/` (see `gyre-cli/src/main.rs` for the command tree structure). The API endpoints already exist — the CLI is a thin wrapper. For UI: follow existing Svelte component patterns in `web/src/` (e.g., MergeQueueGraph.svelte for DAG rendering patterns). Use the existing `GET /api/v1/workspaces/:id/dependency-graph` endpoint for workspace-scoped data.
