# Review — task-088 (HSI §3 Explorer Default Views: Boundary, Spec Realization, Change, System Trace)

Spec: `specs/system/human-system-interface.md` §3 "Default Views" (Automatic, No LLM).
Candidate: `f879fad18d6bd80344f29fd9ecf46112d0aaad9e` (base `1d9547defeb08d9cd2a40f3c656e20644b625602`).
Verdict: **approved** (round 1).

## Scope

Diff: 14 files, +2465/−28 — four new web components/views + view selector in
`ExplorerView.svelte`, client-side `X-Gyre-Last-Seen` cache
(`web/src/lib/last-seen.js` wired into `api.js` `request()`), five new test
files, one focused backend fix (`parse_delta_counts` in
`api/mr_timeline.rs`), docs, and task-record updates.

## Evidence (all under `/tmp/stage/review-evidence/`)

- `cargo test -p gyre-server --lib api::mr_timeline` → **13 passed** (incl.
  the new `parse_delta_counts_producer_array_shape` regression test).
- `npm ci` then `npx vitest run` on the 5 new test files → **34 passed**.
- Full `npm test` in `web/` (acceptance criterion) → **69 files, 1660
  passed, 41 skipped, exit 0**.
- Mutation probe 1: hardcoding `sinceValue = 0` in `ChangeView.svelte`
  (bypassing `getChangeViewSince()`) → **2 tests fail** (ChangeView since
  propagation + ExplorerDefaultViews end-to-end since wiring). Restored.
- Mutation probe 2: reverting `parse_delta_counts` to array-blind counting
  (`_ => 0` for arrays) → **`parse_delta_counts_producer_array_shape`
  FAILS** (exit 101). Restored; suite re-run green on restored source.
- `bash scripts/check-task-commit-attribution.sh` → OK.

## Verified real (no findings)

- **Boundary View**: Level 1 workspace repo cards carry type/endpoint counts
  computed from `GET /workspaces/:id/graph` (node response includes
  `repo_id` — graph.rs:48/87, so counts are real per-repo data) plus
  cross-repo dependency chips from `GET /repos/:id/dependencies`
  (`target_repo_id`/`target_artifact`); best-effort degradation on failure.
  Levels 2–4 are the pre-existing ExplorerCanvas C4 drill-down — honestly
  claimed as pre-existing and pinned by the pre-existing
  `ExplorerCanvasDrillDown.test.js` (packages → modules → types →
  methods/fields); Level 4 spec linkage / churn / complexity / last-modified
  metadata renders in the pre-existing `NodeDetailPanel`. New Level-1 test
  pins counts, dep chips, and repo-id attribution.
- **Spec Realization View**: genuine two-panel layout; linkage derived from
  real graph data via all three sources (GovernedBy edges with spec-node
  target resolution, `spec_path`, `spec_paths`), spec nodes excluded from
  realization, status badges from `GET /specs`, task progress from
  `GET /specs/:path/progress`. Tests cover all three linkage sources and
  exclusion.
- **Change View**: queries the real `GET /repos/:id/graph/timeline?since=`
  endpoint (mod.rs:883, `Query<TimelineQuery>` since/until →
  `list_deltas`); `since` is the previous visit's `X-Gyre-Last-Seen`
  (middleware.rs:104-173 sets the header on tracked workspace-scoped
  responses with a 60s debounce — verified real). The client cache
  (`last-seen.js`) snapshots the pre-visit value at module init before any
  current-visit response can overwrite it — the correct semantics for
  "since last visit" — with a 24h first-visit fallback. Delta entries
  parsed from real `delta_json` producer shape; added/modified/removed
  rendering, field-change expansion, and Boundary navigation all
  mutation-verified or directly asserted.
- **System Trace View**: MR selector + vertical timeline from the
  pre-existing `GET /api/v1/merge-requests/:id/timeline`; gate rows
  expandable; View Conversation resolves `conversation_sha` from the real
  `AttestationBundle` (`MergeAttestation` carries `conversation_sha`,
  `gate_results`, `merge_commit_sha`, `merged_at`, `completion_summary`)
  through the real `GET /api/v1/conversations/:sha` route (lib.rs:703);
  View Diff and View Attestation likewise backed by existing endpoints.
- **Backend fix**: `graph_extraction.rs:320-350` serialises
  `nodes_added`/`nodes_modified` as arrays (and `nodes_modified` as a bare
  count in the no-agent-context branch) — the old `parse_delta_counts`
  accepted only bare numbers plus nonexistent `added`/`modified` keys, so
  real GraphExtraction events reported 0/0. The fix counts both shapes and
  the new test pins the producer shape (mutation-verified: reverting to
  array-blind counting fails the test).
- **No LLM anywhere** in the four views — all rendering is derived from
  knowledge graph / delta / SDLC records.
- View selector (tablist) in the Explorer control bar with Boundary
  default; per-view swapping and return-to-Boundary tested end-to-end
  through the real `ExplorerView` component.
- No new routes, migrations, MessageKinds, or AppState stores — the ABAC /
  migration / dead-kind / in-memory-state invariants are untouched.
  Working tree left clean; all temporary mutations restored.

## Notes (non-blocking)

- New sub-view components use hardcoded English strings while
  `ExplorerView` uses `$t()` i18n — cosmetic inconsistency only; HSI §3 has
  no i18n requirement.
- The view selector renders at repo scope only; Change/System Trace are
  repo-scoped queries and Boundary Level 1 is the workspace-scope default,
  so this matches the spec's data model.
- Coverage matrix row 20 (`human-system-interface.md` §3 "Default Views")
  remains `task-assigned | task-088` — correct per process rules until
  verification completes this round.
