# Review — task-164 (Dependency graph — CLI commands and UI visualization)

Spec: `specs/system/dependency-graph.md` §CLI, §UI.
Comparison base: `3b734b1af744f594ec5d903be242b41ec0f287f2` → candidate `d16f6f98c6844ce3fba7c22b2861b0dfbbf204a3`.
Work commits: `73928811c` (CLI repo arg + edge enrichment + detail panel), `f06e04b7d` (rustfmt), `cfdd16ce3` (heartbeat-test deflake, verification finding 98521e24).

Verdict: **complete / approved**.

## Context

The assigned base already carried the `gyre deps` command tree (show/impact/
stale/breaking/add/acknowledge/graph), the CLI client methods, and
`DependencyGraph.svelte` — earlier task-164 rounds landed them through the
base. This candidate closes the remaining gaps against the task's
implementation plan instead of duplicating working code, which the Shipped
notes state explicitly.

## Diff inspection

- **`gyre deps show [repo]` (positional)**: `DepsCommands::Show` gains an
  optional positional repo; default still infers from the git remote.
  `deps show` now prints the specced table (target/source repo names resolved
  via `list_workspace_repos` with ID fallback, type, version pinned, version
  current, drift, status, method) for both dependencies and dependents.
  Impact/stale/breaking/add/acknowledge/graph verified present at the
  candidate and unchanged (real wrappers over the real endpoints; blast
  radius renders direct+transitive dependents; stale/breaking render tables
  from `/dependencies/stale` and `/dependencies/breaking`).
- **Server edge enrichment**: `build_graph_response` shared by
  `GET /dependencies/graph` and `GET /workspaces/:id/dependency-graph`
  returns per-edge `source_artifact`, `target_artifact`, `version_pinned`,
  `target_version_current`, `version_drift`, `breaking_changes[]`. The
  breaking-change join calls the real `BreakingChangeRepository::
  list_by_source_repo` on each edge's *target* repo — the correct side per
  the domain doc (`BreakingChange.source_repo_id` = repo that introduced the
  change, i.e. the dep-edge target) — and matches to edges via
  `dependency_edge_id`. Workspace handler still filters edges to workspace
  repos before enrichment. No fabricated values; lookups fall back to IDs
  only for display names.
- **UI**: orphaned edges render gray dashed (`#9ca3af`) with a legend entry
  and gray node health; edges are focusable `role=button` (click/Enter) and
  open the new `DependencyDetail.svelte` panel (source→target names, type,
  status, artifacts, pinned/current/drift, breaking changes with ack status,
  Acknowledge → real `POST /dependencies/breaking/:id/acknowledge` then
  `onRefresh`, wired in WorkspaceHome and ExplorerView). Selection is
  id-derived (`$derived` over the `edges` prop), so refresh rebinds to fresh
  state and a removed edge deselects. `DependencyStatus` serializes
  `snake_case`, matching the frontend's lowercase status checks.
- **Heartbeat test deflake (verification finding 98521e24)**: the prior
  failure came from `.rev().find()` selecting the *oldest* event when the
  two heartbeats landed in different seconds (mem adapter sorts
  newest-first, `b.timestamp.cmp(&a.timestamp)`; SQLite/Postgres use
  `timestamp.desc()` — the new trait doc comment is accurate for all
  three). The fix filters the agent's events, asserts exactly two, and
  identifies the second by content (dead pid). This is a genuine deflake,
  not a weakening: a stale-attestation read still leaves no dead-pid event
  and fails the test; the first heartbeat stays pinned to (live_pid, true).
- **New tests**: `task164_dependency_graph_detail.rs` drives the real
  handlers against a real SQLite-backed `build_state` with real seeded
  edges/breaking changes, pinning the enriched fields, the correct-side
  join (a wrong-side lookup yields empty lists and fails), node-name
  resolution, empty-workspace and unknown-workspace behavior.
  `DependencyGraph.test.js` gains real DOM assertions for orphaned gray
  styling, node orphaned health, legend, detail-panel contents, the
  acknowledge flow (API called, refresh fired), close, and Enter selection.

## Probes (evidence: /tmp/stage/review-evidence/)

- `cargo test -p gyre-cli` → **150 unit + 1 integration passed, exit 0**
  (includes all 11 `cli_deps_*` parse tests, incl. the new named-repo one).
- `cd web && npm ci && npx vitest run src/__tests__/DependencyGraph.test.js`
  → **30 passed, exit 0**.
- `cargo check -p gyre-server --tests` → **clean, exit 0** (7m37s; type-checks
  the new integration test and the repaired heartbeat test).
- Mechanical lints on the changed tree: `check-byte-slice-truncation`,
  `check-arch`, `check-abac-route-registry` (no new routes — pure refactor),
  `check-mem-port-contracts` → all exit 0.

## Verifier must run (server test-binary builds exceed this sandbox's
budget; per review instructions these belong to the independent verifier —
not a code defect)

- `cargo test -p gyre-server --test task164_dependency_graph_detail`
- `cargo test -p gyre-server --lib api::agents::tests::heartbeat_audit_reports_attested_pid_liveness`
- `cargo test --all` and `cd web && npm test` (acceptance criteria)

## Observations (non-blocking)

- `specs/coverage/system/dependency-graph.md` rows 16/17 (§CLI, §UI) remain
  `task-assigned`; coverage reconciliation is a verify-stage/PM function in
  this pipeline (cf. `verify(task-191)` commits), consistent with the
  coverage file's own audit note ("left to PM").
- CLI render paths for the `deps show` table are parse-tested but not
  render-tested; the data contract they render is pinned by the server
  integration test, and the goal forbids new tests for already-working
  behavior.
