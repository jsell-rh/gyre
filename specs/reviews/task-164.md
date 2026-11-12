# Review — task-164 (Dependency graph — CLI commands and UI visualization)

Spec: `specs/system/dependency-graph.md` §CLI, §UI.
Base `e981bf29c` → candidate `408e89316`. Product commits: `73928811`
(implementation), `f06e04b7` (rustfmt repair of `73928811`'s test formatting —
verified formatting-only by diff). Process commits `f11b66bd9`/`408e89316`
touch only `specs/tasks/task-164.md`. Attribution frontmatter at HEAD lists
both product SHAs (the attribution gate fails only against the review-stage
working-tree reset of the task file, not against the committed candidate).

Verdict: **approved**.

## What the candidate ships (diff vs assigned base)

The base already carried most of §CLI (`deps show/impact/stale/breaking/add/
acknowledge/graph`) and the workspace DAG in WorkspaceHome + ExplorerView.
The candidate closes the remaining gaps:

- `gyre deps show [repo]` — optional positional repo (defaults to the
  git-remote inference); the show tables now carry the plan's columns
  (target/source repo **names** resolved via `list_workspace_repos`, type,
  pinned, current, drift, status, method) for both dependencies and
  dependents. Fields read are exactly those `GET /api/v1/repos/:id/dependencies`
  returns (`DependencyEdgeResponse` carries `version_pinned`,
  `target_version_current`, `version_drift`, `source_artifact` — pre-existing).
- Server: `GET /api/v1/dependencies/graph` and
  `GET /api/v1/workspaces/:id/dependency-graph` edges enriched with
  source/target artifacts, version info, drift, and `breaking_changes[]`
  (with acknowledgment status), via one shared `build_graph_response`.
- §UI: orphaned edges gray dashed + legend entry + node health; new
  `DependencyDetail.svelte` panel on edge click (mouse or Enter; edges are
  focusable `role=button`) showing source→target names, type, status,
  artifacts, pinned/current, drift, breaking changes with ack status, and an
  Acknowledge button calling the real
  `POST /api/v1/dependencies/breaking/:id/acknowledge`
  (`api.acknowledgeBreakingChange`), then refreshing the parent graph
  (`onRefresh` wired to `loadWsDepGraph` in ExplorerView and
  `handleDepScopeChange` in WorkspaceHome).

## Correctness scrutiny

- **Breaking-change join side verified against the production writer.**
  `git_http.rs::process_breaking_changes` constructs
  `BreakingChange::new(bc_id, dep_edge.id, Id::new(repo_id), …)` where
  `repo_id` is the **pushed** repo — the dependency edge's **target**. The
  domain doc comment agrees ("The repo that introduced the breaking change
  (target of the dep edge)"). `build_graph_response` fetches
  `list_by_source_repo` per distinct edge **target** and matches BCs to edges
  via `dependency_edge_id` — the correct side; a wrong-side (edge-source)
  join yields empty lists, and the new integration test pins this (asserting
  both the acknowledged and unacknowledged BCs attach with status preserved).
- **No fabrication / real queries.** Node names resolved through
  `state.repos.find_by_id`; BCs through the real `BreakingChangeRepository`
  port. With `GYRE_DATABASE_URL` set, `store!` wires dependencies and
  breaking_changes to the SQLite storage, so the test's "real SQLite-backed
  state" claim is accurate (checked `lib.rs:1101` + `store!` at `lib.rs:1224`).
- **Workspace scoping intact.** `get_workspace_dependency_graph` still
  404s on unknown workspace and filters edges to workspace members before
  enrichment; the test covers empty-workspace and 404 paths. Route set and
  ABAC registration unchanged (no new routes; enrichment is read-only).
- **Selection rebinding.** `selectedEdge` is `$derived` from
  `edges.find(e => e.id === selectedEdgeId)` — after a refresh the panel
  rebinds to fresh edge state and a removed edge deselects.

## Test-inflation check

None found. The Rust integration test (`task164_dependency_graph_detail.rs`)
drives the real handlers with real SQLite state, one edge + two BCs, and
fails on wrong-side join, missing enrichment, name-echo, missing 404, or
missing workspace filter. The new vitest cases (orphaned styling, detail
panel content incl. repo-name resolution and ack-status gating of the
Acknowledge button, acknowledge→API→refresh flow, keyboard selection) assert
concrete DOM/attribute outcomes that break if the behavior regresses; the
`api.js` mock sits at the network boundary, and the real client method +
endpoint exist.

## Probes run (evidence in /tmp/stage/review-evidence/)

- `cargo fmt -p gyre-cli -- --check` → exit 0;
  `cargo fmt -p gyre-server -- --check` → exit 0
  (rustfmt-changed-lines was the sole failure of verification finding
  `92fd8ef9` against `7e60f105`; the assigned candidate differs from
  `7e60f105` only by that formatting fix, process commits, and the merge of
  the assigned base — verified by diff).
- `cargo test -p gyre-cli cli_deps` → 11/11 passed, including the new
  `cli_deps_show_named_repo_parses` (positional repo, flags preserved).
- `cargo check -p gyre-server --test task164_dependency_graph_detail`
  → exit 0 (full compilation of the server test binary deferred, see below).
- `cd web && npm ci && npm test` (full suite) → **110 files, 2215 passed,
  41 skipped, exit 0**, including the extended `DependencyGraph.test.js`
  (30/30).
- `bash scripts/check-arch.sh` → exit 0.
- `bash scripts/check-task-commit-attribution.sh` → fails only because the
  review stage reset the working-tree `specs/tasks/task-164.md` to its
  assigned pristine state (`commits: []`); the committed candidate records
  both product SHAs.

## Deferred to the verifier (build-budget rules; not runnable as focused probes here)

- `cargo test -p gyre-server --test task164_dependency_graph_detail`
- `cargo test --all` (acceptance criterion; prior controller verification of
  the formatting-equivalent tree `7e60f105` passed everything except the
  now-fixed rustfmt gate).
- `cd web && npm run build` + dist regeneration (generated `web/dist/` is
  verifier-owned; the candidate intentionally leaves it untouched).

## Observations (non-blocking)

- Active-status edges are colored by dependency **type** (code=blue,
  spec=purple, api=green) rather than uniformly green as the plan's
  parenthetical suggests; stale/breaking/orphaned status coloring is
  explicit and legend-documented. This is the pre-existing base design,
  unchanged by this candidate, and satisfies "edges color-coded by status"
  for every status that carries a warning.
- Coverage rows 16/17 in `specs/coverage/system/dependency-graph.md` remain
  `task-assigned`; per repo convention they are flipped by the verify/PM
  stage, not the implementing branch.
- Generated `web/dist/` unchanged in the candidate — pure generated-output
  observation, owned by the deterministic verifier.
