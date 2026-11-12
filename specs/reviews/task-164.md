# Review — task-164 (Dependency graph — CLI commands and UI visualization)

Spec: `specs/system/dependency-graph.md` §CLI, §UI.
Candidate under review: `7e60f105692bc9a9144e8f9eafb18e41ba2dee23`
(product commit `73928811cafb9f46adc65689ac75291ae815022b`, attributed in the
task frontmatter) on base `d31906c5a16ab8045ab435dfad2bd7dc1ce3f616`.
Verdict: **approved** — no blocking findings.

## Scope of the diff

The base already carried the bulk of §CLI (`deps show/graph/impact/stale/
breaking/add/acknowledge` with real handlers calling the real API client) and
the workspace-scope DAG (`DependencyGraph.svelte` wired into WorkspaceHome and
ExplorerView with stale-yellow/breaking-red/type-based coloring). The candidate
closes the remaining gaps:

1. **`gyre deps show [repo]`** — optional positional repo (spec literal
   `gyre deps show [repo]`); without it the repo is inferred from the git
   remote as before. Table upgraded to the plan's columns (target/source repo
   *names* resolved via `list_workspace_repos` with ID fallback, type, pinned,
   current, drift, status, method). New parse test
   `cli_deps_show_named_repo_parses` plus updated destructuring in the three
   existing parse tests.
2. **Graph edge enrichment (server)** — `build_graph_response` shared by
   `GET /api/v1/dependencies/graph` and `GET /api/v1/workspaces/:id/
   dependency-graph`; each edge now carries `source_artifact`,
   `target_artifact`, `version_pinned`, `target_version_current`,
   `version_drift`, and `breaking_changes[]` with acknowledgment fields.
   Verified against `git_http.rs process_breaking_changes`: a `BreakingChange`
   is created with `dep_edge.id` and the *pushed* repo's id, i.e. the edge's
   TARGET — so the join (`list_by_source_repo` on each distinct edge target,
   matched by exact `dependency_edge_id`) is on the correct side and is exact,
   not heuristic. `acknowledge_breaking_change` (pre-existing) reverts the edge
   to Active once all its BCs are acknowledged, which the UI's post-ack
   refresh relies on.
3. **§UI detail panel** — edges are focusable (`role=button`, Enter/Space)
   and clicking one opens `DependencyDetail.svelte`: source→target names,
   type, status, artifacts, pinned/current versions, drift, breaking changes
   with acknowledged/unacknowledged status, and an Acknowledge button calling
   the real `POST /api/v1/dependencies/breaking/:id/acknowledge` (204 is
   handled by the shared `request` helper) then `onRefresh`, wired to real
   graph reloaders in both parents (`handleDepScopeChange` in WorkspaceHome,
   `loadWsDepGraph` in ExplorerView). Selection is id-derived and rebinds to
   fresh edge state after refresh.
4. **Orphaned color-coding** — gray dashed edges (`#9ca3af`), legend entry,
   orphaned node health; `DependencyStatus` is `snake_case`-serialized so
   `'orphaned'` matches the wire format.

No routes were added or changed (ABAC registry gate green); `web/dist` is
untouched by the candidate (verifier-owned, as agreed).

## Probes (evidence: /tmp/stage/review-evidence/task-164/)

- `cd web && npm ci` → exit 0.
- `npx vitest run src/__tests__/DependencyGraph.test.js` → **30/30 passed**.
- `npx vitest run` on ExplorerViewWorkspaceGraph + WorkspaceHome → 13/13.
- Full `npm test` → **110 files, 2215 passed / 41 skipped, exit 0** (AC met).
- `cargo test -p gyre-cli` → **150 + 1 passed**.
- `cargo check -p gyre-server --test task164_dependency_graph_detail` →
  exit 0 (test-binary execution deferred to the verifier per the 4 GiB rule).
- `cargo test -p gyre-adapters breaking_change` → 7/7, including
  `list_by_source_repo_scopes_correctly` — the port semantics the new join
  relies on.
- Static gates all exit 0: arch, abac-route-registry, byte-slice-truncation,
  relative-path-defaults, in-memory-state-stores, inert-enforcement.
- **Mutation probe**: disabling the detail-panel render
  (`{#if false && selectedEdge}`) makes 6 of the 30 DependencyGraph tests
  fail; restored and re-verified 30/30. The new tests are real guards, not
  self-confirming.

## Test-quality check

The Rust integration test (`task164_dependency_graph_detail.rs`) drives the
real handlers against a real SQLite-backed `build_state` (dependency stores go
through the `store!` macro, so `GYRE_DATABASE_URL` selects the SQLite
adapters), asserts edge enrichment field-by-field, pins the breaking-change
join side (a source-side lookup yields an empty list and fails the 2-BC
assertion), checks node-name resolution, and covers empty and unknown-
workspace cases. The frontend tests assert observable DOM (panel appears on
click/Enter, values rendered from nodes/edge data, ack button only for
unacknowledged BCs, API called with the right id, onRefresh fired, close
works). No mirrored-logic or assertionless tests found.

## Non-blocking observations (recorded, no repair required)

- `gyre deps show <repo> --workspace` silently ignores the positional repo
  (no clap conflict declared). Cosmetic UX nit; the spec's `[repo]` form works.
- `build_graph_response` issues one `list_by_source_repo` query per distinct
  edge-target repo (N+1). Functionally exact; a batching concern only at
  large graph sizes.
- Coverage rows 16/17 in `specs/coverage/system/dependency-graph.md` remain
  `task-assigned` — per the coverage file's own audit note these transitions
  are "left to PM"; the candidate did not falsely mark anything verified, and
  the task file's `ready-for-review` + shipped notes are accurate.
- Server test-binary execution (`cargo test -p gyre-server --test
  task164_dependency_graph_detail`, `--test task163_dependency_persistence`,
  `--test dependency_persistence`, full `cargo test --all`) recorded for the
  independent verifier per the hard 4 GiB budget rule; type-checked here.

## Acceptance criteria

- `deps show <repo>` outgoing dependencies — base handlers + candidate
  positional arg/columns; parse-tested, client methods verified against the
  server's `DependencyEdgeResponse` serialization.
- `deps impact` / `deps stale` / `deps breaking` — real handlers on base
  (verified in base source: `get_blast_radius`, `list_stale_dependencies`,
  `list_breaking_changes` against the live endpoints), unchanged by the
  candidate, `cargo test -p gyre-cli` green.
- Web UI graph at workspace scope — WorkspaceHome + ExplorerView wiring
  (base) with candidate detail panel, refresh, and orphaned coloring; full
  web suite green.
- Edges color-coded by status — active (type colors), stale yellow, breaking
  red, orphaned gray dashed, with legend; asserted in tests.
- `cargo test --all` + `cd web && npm test` — web suite fully green here;
  Rust workspace suite deferred to the verifier with the focused packages
  (gyre-cli, gyre-adapters) green and gyre-server test targets type-checked.
