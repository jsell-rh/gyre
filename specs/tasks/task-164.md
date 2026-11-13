---
title: "Dependency graph — CLI commands and UI visualization"
spec_ref: "dependency-graph.md §CLI"
depends_on: [task-163]
progress: complete
coverage_sections:
  - "dependency-graph.md §CLI"
  - "dependency-graph.md §UI"
commits: ["882909facd58cc3feb21d1f0c3a852184e5e0fe1", "900b3dd369cb7b0f34fe36ab695bc5ff454fe0bc", "cfdd16ce3174f045099419e864c4b9740205c8ff", "f06e04b7db064c2e302d0dd8e7461433bf723d1e", "73928811cafb9f46adc65689ac75291ae815022b"]
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
the `DependencyGraph.svelte` DAG on this task's base; the branch closes the
remaining gaps. Acceptance-criteria boxes above are left exactly as
assigned — satisfaction is reported here. Source commits:
`73928811` (CLI positional repo + graph edge enrichment + UI detail panel),
`f06e04b7` (rustfmt normalization), `cfdd16ce` (deflake heartbeat audit
test). `900b3dd3` is the generated web/dist asset build riding the branch;
the executor restored `web/dist` to the assignment base and final
integration rebuilds and publishes those assets remotely — they were not
rebuilt or recommitted in this round.

### Behavior

- **`gyre deps show [repo]`** takes an optional positional repository name
  (repo inferred from the git remote when omitted) and reports target/source
  repo names, type, version pinned, version current, drift, status, and
  detection method for both dependencies and dependents. `deps impact`,
  `deps stale`, `deps breaking`, `deps add`, `deps acknowledge`, and
  `deps graph [--format dot]` were present on the base and unchanged.
- **Graph edge enrichment (server)**: `GET /api/v1/dependencies/graph` and
  `GET /api/v1/workspaces/:id/dependency-graph` return each edge with
  source/target artifacts, version pinned/current, drift, and
  `breaking_changes[]` (id, commit_sha, description, detected_at,
  acknowledged, acknowledged_by, acknowledged_at) joined through the real
  `BreakingChangeRepository` (`list_by_source_repo` on the edge's target
  repo, matched via `dependency_edge_id`). The wrong-side join yields empty
  lists; `tests/task164_dependency_graph_detail.rs` pins the correct side.
- **UI**: orphaned edges render gray dashed (with legend entry); stale
  yellow, breaking red, type-based active coloring unchanged. Clicking an
  edge (mouse or Enter; edges are focusable `role=button`) opens
  `DependencyDetail.svelte` showing source→target names, type, status,
  artifacts, versions, drift, and breaking changes with an Acknowledge
  button calling the real `POST /api/v1/dependencies/breaking/:id/acknowledge`;
  the parent graph refreshes and the panel rebinds to fresh edge state.

### CI repair round (run 43366709797)

The prior candidate `04128b82` failed the "Development controller" CI job
(`131692696829`) at step `python3 -m unittest discover -s scripts -p
'test_dev*.py'` (go tests passed; later steps skipped). Diagnosis, evidence
in `/tmp/stage/review-evidence/` (`ci-inspection.md` + logs):

- Exact-head reproduction at a worktree of `04128b82` in a CI-like
  environment: 61/61 tests pass; current HEAD passes 7/7 runs.
- Every other job of that run passed at the same head, including
  `cargo nextest run --all` (Test), Web build + bundle with vitest, Clippy,
  and Format; the E2E run passed.
- The same step passed for the previous candidate of this PR (`fb90563b`,
  run `43354684756`) with byte-identical `test_dev*.py` files; the only
  script changes between them came from main (passed main CI) and are not
  imported by any dev test. Neighboring PR runs passed the same step within
  minutes.

No code defect was reproduced, so no code was changed this round; the
failure is a runner-local flake. The raw step log could not be downloaded
from this sandbox (`results-receiver.actions.githubusercontent.com` egress
denied — recorded transport restriction); exact-head GitHub checks remain
mandatory after independent review.

### Test evidence (this round)

- `cargo test -p gyre-cli` — 150 unit + 1 integration tests pass, including
  the 11 `cli_deps_*` parse/render tests.
- `cd web && npx vitest run src/__tests__/DependencyGraph.test.js` — 30/30
  pass (DAG render, edge status colors incl. orphaned gray, detail panel
  contents, acknowledge flow, refresh rebinding).
- `cargo check -p gyre-server --all-targets` — clean; type-checks the
  server integration test and the repaired heartbeat test.
- Verifier must run (blocked in this sandbox by the 4 GiB build wrapper):
  `cargo test -p gyre-server --test task164_dependency_graph_detail`,
  `cargo test -p gyre-server --lib api::agents::tests::heartbeat_audit_reports_attested_pid_liveness`,
  then the full `cargo test --all` and `cd web && npm test` suites, and the
  CI dev-controller steps (`test_dev*.py`, `test_pipeline*.py`, node
  script tests, `go test -race ./...` in tools/fullsend-dispatcher).
