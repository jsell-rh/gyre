# Review — task-088 (HSI §3 Explorer Default Views: Boundary, Spec Realization, Change, System Trace)

Spec: `specs/system/human-system-interface.md` §3 "Default Views".
Candidate: `269f9325` (rebased onto task-078 base `a4ebdf6f5` via merge `531ca48b7`); product commits `8e025c2` (views) and `3463e28` (mr_timeline delta-count fix), both attributed in the task frontmatter.
Verdict: **approved**.

## Probes (evidence in /tmp/stage/review-evidence/)

- `npm ci` in `web/` → exit 0.
- Focused: `npx vitest run` on the five new suites (`ExplorerDefaultViews`, `ChangeView`, `last-seen`, `SpecRealizationView`, `SystemTraceView`) → **5 files / 34 tests passed**.
- Full `npm test` in `web/` (explicit acceptance criterion) → **76 files, 1771 passed, 41 skipped** — matches the shipped claim, and includes task-078's shortcut/streaming/causal-trace suites, confirming the rebase kept the base task's behavior.
- `cargo test -p gyre-server --lib api::mr_timeline` → **13 passed** (incl. the new producer-shape regression test).
- Mutation probe: broke `getChangeViewSince()` to always use the 24h fallback (ignoring the persisted previous-visit value) → **3 tests failed across 3 independent suites** (last-seen unit, ChangeView component, ExplorerView end-to-end wiring). Tests are not self-confirming; mutation reverted, tree verified clean afterwards.
- `scripts/check-task-commit-attribution.sh` → OK. No new server routes → no ABAC registry implications.

## Verified real (no fakery found)

- **All four views consume real endpoints with matching shapes** (verified against server source): `GET /repos/:id/graph/timeline?since=` (`graph.rs::get_graph_timeline` → `list_deltas`), `GET /workspaces/:id/graph` (nodes carry `repo_id` → Level-1 type/endpoint counts are real per-repo aggregation), `GET /repos/:id/dependencies` (`target_repo_id`/`target_artifact` → dep chips), `GET /specs` + `/specs/:path/progress`, `GET /merge-requests/:id/timeline|diff|attestation`, `GET /conversations/:sha`. No LLM anywhere in the paths.
- **Change View `delta_json` parsing matches the producer**: `graph_extraction.rs` serialises `nodes_added` as an array of `{name, node_type, qualified_name}`, `nodes_modified` as `{qualified_name, field_changes}` (one variant emits a bare count — the fixed `parse_delta_counts` handles both), `nodes_removed` as qualified names. `ChangeView.parseDelta` mirrors exactly this.
- **`last_seen_at` semantics are correct and spec-faithful**: `X-Gyre-Last-Seen` is set by `last_seen_middleware` (debounced upsert); `last-seen.js` snapshots the persisted pre-visit value once per page load so `since` stays anchored to the *previous* visit even after the current visit's first response stamps a newer value — the subtle correctness point, pinned by tests and killed by the mutation probe.
- **`parse_delta_counts` fix is genuine**: the old parser only accepted bare numbers and probed nonexistent `added`/`modified` keys while the producer writes arrays — every real GraphExtraction event reported 0/0. The new test pins the producer shape; reading `graph_extraction.rs` confirms the shapes asserted are the ones actually serialised.
- **Spec Realization linkage** uses canonical data: `GovernedBy` edge type and node `spec_path`/`spec_paths` are real `gyre-common` graph types produced by the extractor; the view resolves spec-node targets, edge metadata, and node fields — three real sources, spec nodes excluded from realization.
- **Boundary Level 4** spec linkage + modification metadata (spec_path/confidence, last-modified SHA/agent/time, churn, complexity) render in the pre-existing `NodeDetailPanel` — present in the base and pinned by its tests; Levels 2–4 drill-down pinned by `ExplorerCanvasDrillDown.test.js`.
- **View selector**: tablist with Boundary default, canvas ↔ panel swapping verified both directions by tests.

## Minor observations (non-blocking, recorded for completeness)

- Canonical spec prose for System Trace says "clicking the graph extraction shows the architectural delta" and "clicking a turn opens the conversation at that point". GraphExtraction rows render the delta summary inline but are not click-through, and `ConversationTurn` events are not emitted — the latter is a **pre-existing documented deferral** in `mr_timeline.rs` (present verbatim at base `a4ebdf6f`, pending `TurnCommitLink` records from the conversation-provenance feature), not a regression of this candidate. The task's decomposed plan scoped clickability as gate expansion + the `[View Conversation] [View Diff] [View Attestation]` links (matching the spec's ASCII mock), all of which are implemented and tested.
- Canonical spec mentions System Trace "can also be accessed from the Explorer's Change View by clicking any change entry"; the task plan (orchestrator decomposition) instead specifies change entries navigate to the node in the Boundary View, which is what shipped. Access to System Trace is via the view selector. Access-path nuance, not a behavioral guarantee.
- Committed `web/dist` was not regenerated — per repo convention that happens post-verification ("regenerate verified assets" commits are attributed after review; `dev-check.sh` restores `web/dist` as disposable). Default `cargo build` rebuilds the UI from source via build.rs.
