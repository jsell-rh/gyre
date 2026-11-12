---
title: "Dependency graph — CLI commands and UI visualization"
spec_ref: "dependency-graph.md §CLI"
depends_on: [task-163]
progress: ready-for-review
coverage_sections:
  - "dependency-graph.md §CLI"
  - "dependency-graph.md §UI"
commits: ["f06e04b7db064c2e302d0dd8e7461433bf723d1e", "73928811cafb9f46adc65689ac75291ae815022b"]
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

Dependency-graph §CLI and §UI, on top of the `gyre deps` command tree and the
`DependencyGraph.svelte` DAG that already existed on this task's base. The base
already satisfied most of §CLI (Show/Impact/Stale/Breaking/Add/Acknowledge/
Graph) and rendered the workspace DAG in WorkspaceHome + ExplorerView; this
branch closes the remaining gaps rather than re-implementing what worked.
Acceptance-criteria boxes above are left exactly as assigned — satisfaction is
reported here.

### Repair round 2 (verification finding 92fd8ef9)

The prior candidate failed `checks.sh` on rustfmt only: the four
`DepsCommands::Show` match arms in the gyre-cli test module and the
`gyre_domain` import wrap in the task-164 server integration test. Fixed by
`cargo fmt` under the workspace edition (2021); formatting-only, no semantic
change (commit `f06e04b7`). Note for future rounds: bare `rustfmt --edition
2024` produces different output than this repo's `cargo fmt` and would itself
fail the changed-lines check. Everything else in the failing run had passed
(clippy changed-lines, all static gates, web build).

### Behavior

- **`gyre deps show [repo]` (positional repo)**: `DepsCommands::Show` now takes
  an optional positional repository name; without it the repo is inferred from
  the git remote as before. The show table now reports the columns the plan
  specifies — target/source repo **names** (resolved via
  `list_workspace_repos`, falling back to IDs), type, version pinned, version
  current, drift, status, and detection method — for both dependencies and
  dependents.
- **Graph edge enrichment (server)**: `GET /api/v1/dependencies/graph` and
  `GET /api/v1/workspaces/:id/dependency-graph` now return each edge with
  `source_artifact`, `target_artifact`, `version_pinned`,
  `target_version_current`, `version_drift`, and `breaking_changes[]`
  (id, commit_sha, description, detected_at, acknowledged, acknowledged_by,
  acknowledged_at). Both handlers share one `build_graph_response` builder.
  Breaking changes are joined through the real `BreakingChangeRepository`
  via `list_by_source_repo` on each edge's **target** repo — the repo that
  introduced the change (`BreakingChange.source_repo_id` is the pushed repo,
  i.e. the dependency target, per `process_breaking_changes`) — and matched to
  edges via `dependency_edge_id`. A wrong-side join (edge source) yields empty
  lists; the new integration test pins the correct side.
- **Orphaned status color-coding (§UI)**: orphaned edges render gray dashed
  (`#9ca3af`) with an `orphaned` legend entry; nodes whose edges are all
  orphaned get gray health coloring. Stale-yellow / breaking-red / type-based
  active coloring was already present and is unchanged.
- **DependencyDetail panel (§UI)**: clicking a dependency edge (mouse or Enter;
  edges are focusable `role=button`) opens a `DependencyDetail.svelte` panel
  showing source→target names, type, status, source/target artifacts, version
  pinned/current, drift, and the edge's breaking changes with acknowledgment
  status — plus an **Acknowledge** button that calls the real
  `POST /api/v1/dependencies/breaking/:id/acknowledge` and then refreshes the
  parent's graph data (`onRefresh` wired in WorkspaceHome and ExplorerView),
  since the server reverts the edge to Active once all its breaking changes
  are acknowledged. Selection is id-derived, so a refresh rebinds the panel to
  the fresh edge state and a removed edge deselects.
- **web/dist**: no committed changes; generated assets are restored/rebuilt by
  the executor.

### Test evidence

Focused suites (full `cargo test --all` + whole-file `npm test` deferred to the
independent verifier per the 4 GiB budget rule; raw logs and exit codes under
`/tmp/stage/review-evidence/task-164/`):

- Round 1 (`73928811c`):
  - `cargo check -p gyre-server` and `cargo check -p gyre-server --test
    task164_dependency_graph_detail` — exit 0.
  - `cargo test -p gyre-cli` — 150 passed / 0 failed, including
    `cli_deps_show_named_repo_parses`.
  - `vitest run src/__tests__/DependencyGraph.test.js` — 30 passed / 0 failed
    (8 new cases: orphaned edge/node styling + legend, edge click/Enter → detail
    panel, version + drift + artifact rendering, breaking-change ack status,
    acknowledge → API + onRefresh, close).
  - `vitest run` on the six suites covering the touched parent components —
    89 passed / 0 failed.
- Round 2 (`f06e04b7`, formatting-only repair):
  - `cargo fmt -p gyre-cli -- --check` and `cargo fmt -p gyre-server
    -- --check` — exit 0 (the failing gate).
  - `cargo check -p gyre-cli --tests` — exit 0.
  - `cargo test -p gyre-cli deps` — 11 passed / 0 failed. No web or server
    source changed this round.

### Verifier must execute

- `cargo test -p gyre-server --test task164_dependency_graph_detail` — the
  graph-enrichment integration test (SQLite-backed `build_state`; fails on a
  wrong-side breaking-change join). Not run in this sandbox per the
  large-server-build rule; type-checked only.
- `cargo test -p gyre-server --test task163_dependency_persistence` and
  `--test dependency_persistence` — the graph handler refactor touched
  `api/dependencies.rs`; these must stay green.
- Full `cargo test --all`, `cd web && npm test`, and remaining static gates.

### Remaining work

None for §CLI/§UI on this task. Adjacent gaps already tracked elsewhere:
coverage rows 4 (`task-199`, dependency persistence wiring) and 8 (coarse
openapi/mcp detection) are not part of this assignment.
