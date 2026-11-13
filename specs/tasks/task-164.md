---
title: "Dependency graph — CLI commands and UI visualization"
spec_ref: "dependency-graph.md §CLI"
depends_on: [task-163]
progress: complete
coverage_sections:
  - "dependency-graph.md §CLI"
  - "dependency-graph.md §UI"
commits: ["cfdd16ce3174f045099419e864c4b9740205c8ff", "f06e04b7db064c2e302d0dd8e7461433bf723d1e", "73928811cafb9f46adc65689ac75291ae815022b", "900b3dd369cb7b0f34fe36ab695bc5ff454fe0bc"]
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

## Shipped

Dependency-graph §CLI and §UI, building on the `gyre deps` command tree and
the `DependencyGraph.svelte` DAG that already existed on this task's base;
this branch closes the remaining gaps rather than re-implementing what
worked. Acceptance-criteria boxes above are left exactly as assigned —
satisfaction is reported here.

### Behavior

- **`gyre deps show [repo]` (positional repo)**: `DepsCommands::Show` takes an
  optional positional repository name; without it the repo is inferred from
  the git remote as before. The show table reports target/source repo names
  (resolved via `list_workspace_repos`, falling back to IDs), type, version
  pinned, version current, drift, status, and detection method, for both
  dependencies and dependents. `deps impact`, `deps stale`, `deps breaking`,
  `deps add`, `deps acknowledge`, and `deps graph [--format dot]` were already
  present on the base and are unchanged.
- **Graph edge enrichment (server)**: `GET /api/v1/dependencies/graph` and
  `GET /api/v1/workspaces/:id/dependency-graph` return each edge with
  `source_artifact`, `target_artifact`, `version_pinned`,
  `target_version_current`, `version_drift`, and `breaking_changes[]`
  (id, commit_sha, description, detected_at, acknowledged, acknowledged_by,
  acknowledged_at). Both handlers share one `build_graph_response` builder.
  Breaking changes are joined through the real `BreakingChangeRepository`
  via `list_by_source_repo` on each edge's target repo — the repo that
  introduced the change — and matched to edges via `dependency_edge_id`.
  A wrong-side join yields empty lists; the task-164 server integration test
  pins the correct side.
- **Orphaned status color-coding (§UI)**: orphaned edges render gray dashed
  with an `orphaned` legend entry; nodes whose edges are all orphaned get
  gray health coloring. Stale-yellow / breaking-red / type-based active
  coloring was already present and is unchanged.
- **DependencyDetail panel (§UI)**: clicking a dependency edge (mouse or
  Enter; edges are focusable `role=button`) opens a `DependencyDetail.svelte`
  panel showing source→target names, type, status, source/target artifacts,
  version pinned/current, drift, and the edge's breaking changes with
  acknowledgment status — plus an Acknowledge button calling the real
  `POST /api/v1/dependencies/breaking/:id/acknowledge`, then refreshing the
  parent's graph data (`onRefresh` wired in WorkspaceHome and ExplorerView).
  Selection is id-derived, so a refresh rebinds the panel to fresh edge state
  and a removed edge deselects.

### Repair round 3 (verification finding 98521e24)

`cargo test --all` on the prior candidate failed
`api::agents::tests::heartbeat_audit_reports_attested_pid_liveness`
(assertion left `Number(62362)`, right `62477` at agents.rs:886). Root
cause: `AuditRepository::query` returns newest-first in all three adapters
(mem, SQLite `timestamp.desc()`, Postgres `timestamp.desc()`), but the test's
second lookup used `.iter().rev().find(...)`, which selects the **oldest**
event whenever the two heartbeat events get different second-granularity
timestamps; equal timestamps tie under the mem adapter's stable sort and
accidentally preserved the intended selection, masking the bug until a run
straddled a second boundary (62362 was the first event's live test-process
pid; 62477 the expected dead child). Fix (commit `cfdd16ce3`): the test now
filters the agent's heartbeat events, asserts exactly two were recorded,
and identifies the second by content (dead pid) — order-independent, with
the first heartbeat still pinned to (live_pid, true) by the earlier block,
so stale-attestation reads and hardcoded liveness still fail. The
newest-first contract is now documented on the `AuditRepository::query`
trait method. No production behavior changed.

### Test evidence

- `cargo test -p gyre-cli` — 150 unit + 1 integration test pass, including
  the 11 `cli_deps_*` parse/render tests (show bare/named/workspace/tenant,
  impact, stale, breaking, add, acknowledge, graph, graph --format dot).
- `cd web && npx vitest run src/__tests__/DependencyGraph.test.js` — 30
  tests pass (DAG render, edge status colors incl. orphaned gray, detail
  panel contents, acknowledge flow, refresh rebinding).
- `cargo check -p gyre-server --all-targets` — clean (type-checks the
  server integration test `task164_dependency_graph_detail.rs` and the
  repaired heartbeat test).
- Verifier must run (blocked in this sandbox by the 4 GiB build wrapper,
  which routes gyre-server test compilation to the independent verifier):
  `cargo test -p gyre-server --lib api::agents::tests::heartbeat` and
  `cargo test -p gyre-server --test task164_dependency_graph_detail`, then
  the full `cargo test --all` gate.
