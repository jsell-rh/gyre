# Review: TASK-164 — Dependency graph — CLI commands and UI visualization

**Reviewer:** Independent review (pi runtime)
**Date:** 2026-11-13
**Candidate:** `12ca09c5bbc6fbfd9e582d87b39b9284a8582725` (base `890a70dec844985f778b19f81dadd29a94d19de0`)
**Verdict:** approved

---

## Scope reviewed

Diff base→candidate touches 11 files. The `gyre deps` command tree (show
--workspace/--tenant, graph, impact, stale, breaking, add, acknowledge) and the
`DependencyGraph.svelte` DAG existed on the base; this candidate closes the
remaining §CLI/§UI gaps:

1. **CLI `deps show [repo]`** — new optional positional repository name
   (defaults to the git-remote repo), with the table upgraded to target/source
   repo *names* (resolved via `list_workspace_repos`), type, version pinned,
   version current, drift, status, detection method.
2. **Server graph edge enrichment** — `GET /api/v1/dependencies/graph` and
   `GET /api/v1/workspaces/:id/dependency-graph` (shared
   `build_graph_response`) now return per-edge source/target artifacts,
   pinned/current versions, drift, and `breaking_changes[]` joined through the
   real `BreakingChangeRepository` (`list_by_source_repo` on the edge's
   target — matching the domain contract that
   `BreakingChange.source_repo_id` is the repo that *introduced* the change —
   then matched by `dependency_edge_id`). Real queries against the port-backed
   stores wired via `store!` (task-163), no fabrication.
3. **UI** — orphaned status renders gray dashed with a legend entry; edges are
   focusable/clickable (`role=button`, mouse + Enter) and open the new
   `DependencyDetail.svelte` panel showing source→target names, type, status,
   artifacts, pinned/current versions, drift, and breaking changes with
   acknowledgment status and a working Acknowledge action
   (`POST /api/v1/dependencies/breaking/:id/acknowledge`); the parent graph
   refreshes via `onRefresh` in both WorkspaceHome and ExplorerView and the
   panel rebinds to fresh edge state.
4. **Heartbeat audit test deflake** (`agents.rs`) — identifies the second
   heartbeat event by content (dead child PID) instead of position under
   tied second-granularity timestamps, plus an exact-count assertion. The
   first heartbeat's live-PID assertion is unchanged (it runs while only one
   event exists). Not a weakening: a handler that re-read a stale attestation
   leaves no dead-PID event and still fails.
5. Doc-only ordering contract note on `AuditRepository::query` (verified
   against the SQLite adapter's `timestamp.desc()` ordering).

## Evidence (full logs: /tmp/stage/review-evidence/)

- `npm ci` + `npx vitest run src/__tests__/DependencyGraph.test.js` — **30/30
  pass**.
- **Mutation probe**: changed the orphaned stroke color and removed the edge
  `onclick` in `DependencyGraph.svelte` → **6 tests fail**; restored → 30/30
  pass. The tests are behavior-sensitive, not self-confirming.
- `cargo test -p gyre-cli cli_deps` — **11/11 pass**, including the new
  `cli_deps_show_named_repo_parses`.
- `cargo check -p gyre-server --test task164_dependency_graph_detail` —
  clean (7m13s). Running it is blocked by the stage wrapper (gyre-server
  codegen + model exceeds the 4 GiB budget) — see deferred checks.
- `cd web && npm test` — **111 files, 2250 passed / 41 skipped, exit 0** (the
  new component is imported transitively by ExplorerView/WorkspaceHome; no
  regressions).
- Mechanical checks at candidate HEAD: `check-inert-enforcement` OK,
  `check-byte-slice-truncation` OK, `check-task-commit-attribution` OK (the
  working-tree failure of the last is solely the executor's reset of the task
  file for this round; the candidate's frontmatter lists all attributed
  commits).
- CI dev-controller step (`python3 -m unittest discover -s scripts -p
  'test_dev*.py'`): 58/61 pass; the 3 failures are all exit-64 rejections by
  the stage cargo wrapper's in-sandbox budget guard — environment
  restriction, not a code defect. This matches the prior GitHub run
  43366709797 flake diagnosis (same step passed at the previous candidate and
  neighboring PRs; every other job of that run passed at the same head).

## Code-level verification

- The breaking-change join direction was checked against the domain type
  (`dependency.rs`: `source_repo_id` = "repo that introduced the breaking
  change (target of the dep edge)") and the SQLite adapter's
  `list_by_source_repo_scopes_correctly` test; the dedicated integration test
  `task164_dependency_graph_detail.rs` pins the correct side (a wrong-side
  lookup yields empty lists and fails the test), uses real SQLite-backed
  `build_state`, and asserts acknowledgment fields and name resolution.
- CLI field access (`dependency_type`, `version_pinned`,
  `target_version_current`, `version_drift`, `status`, `detection_method`,
  `source_repo_id`/`target_repo_id`) matches `DependencyEdgeResponse`
  serialization exactly; null drift renders as `-`; unknown repo names error
  out (no fabricated defaults).
- UI wiring: `onRefresh` reloads data in both parents; `selectedEdge` is
  `$derived` so the panel rebinds after refresh and deselects removed edges.

## Non-blocking observations (recorded, not findings)

- Active edges are colored by dependency type (code=blue, spec=purple,
  api=green dotted, schema=orange, manual=gray) rather than uniformly green
  as the plan's parenthetical suggests; stale/breaking/orphaned status colors
  are exact and legend-documented. Pre-existing base design; the substance of
  "edges color-coded by status" is satisfied.
- `web/dist` at the candidate is identical to the assignment base (generated
  assets restored; the verifier rebuilds and publishes them remotely).
- Coverage rows 16/17 in `specs/coverage/system/dependency-graph.md` remain
  `task-assigned` — coverage-matrix updates are a PM/auditor concern per the
  audit note; the task file honestly reports `ready-for-review`.

## Deferred to the independent verifier (blocked in-sandbox by the wrapper)

- `cargo test -p gyre-server --test task164_dependency_graph_detail`
- `cargo test -p gyre-server --lib api::agents::tests::heartbeat_audit_reports_attested_pid_liveness`
- Full `cargo test --all`
- GitHub CI "Development controller" job (incl. `test_dev*.py`,
  `test_pipeline*.py`, node script tests, `go test -race ./...` in
  tools/fullsend-dispatcher) at the final integrated head.

## Verdict

The candidate implements the §CLI/§UI contract with real production code —
real repository queries, real API calls, real DOM behavior — and the tests
proved mutation-sensitive in probes. Independent evidence supports the task
contract. **Approved.**
