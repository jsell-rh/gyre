# Review — task-064 (View Query Grammar — Interaction Context, Composition Examples & Integration)

Spec: `specs/system/view-query-grammar.md` §10–16.
Candidate: `10951f1e894161cf0d7b4a00aa0932a9bfa39255` (base `5df14f3449d27bbcaf0cc2298f19a3ea99135f0f`, branch `pipeline/task-064/b529a023e6ac4708a949d4244fd0ee99-2`).
Verdict: **approved** (round 7; rounds 1–6 in prior attempt history repaired real defects — camelCase wire drop, programmatic-zoom attribution, LLM-boundary field drops, zoom/groups staleness, rustfmt).

## Independent evidence (logs in /tmp/stage/review-evidence/task-064-r7/)

- `cargo test -p gyre-domain --test view_query_composition` → **8 passed, 0 failed**.
- `cargo test -p gyre-server --lib -- explorer_ws::` → **42 passed, 0 failed** (includes §10 full-shape deserialization, LLM-boundary surfacing of zoom_level/active_filter, system-prompt documentation).
- `cargo test -p gyre-server --lib -- api::saved_views` → **13 passed, 0 failed** (7 system default views incl. new "High-Risk Untested Code").
- `npm ci` + `npx vitest run src/__tests__/canvas-state.test.js` → **21 passed, 0 failed**; ExplorerChat/ExplorerViewScope/ExplorerViewDrillUrl suites → **117 passed, 0 failed**.
- `rustfmt --edition 2021 --check crates/gyre-server/src/explorer_ws.rs` → clean (the round-6 checks.sh failure is repaired by `3a61cb96`).
- Web bundle rebuilt from current source (`vite build`): sha256 of `dist/assets/index-CJm3c8Zx.js`, `elk.bundled-C_zJuNCJ.js`, `dist/index.html` all match the committed dist; `git status web/dist` clean — the committed bundle is exactly the current SPA source.
- Node probe (`wire-probe.mjs`, real `toWireCanvasState` module): internal camelCase state serializes to the exact §10 shape; wire `zoom_level` equals the live zoom and agrees with the trail's `to_level`; camelCase/render-only fields do not leak.

## What was verified

**§10 Interaction Context.** The single WebSocket send site (`ExplorerChat.svelte`) now sends `toWireCanvasState(canvasState)`; the helper (`web/src/lib/canvas-state.js`) emits exactly the seven §10 fields in snake_case. The server's serde `CanvasState`/`InteractionRecord` (extended with `to_level`/`preset`, unknown fields like `base_level` tolerated) round-trips the byte-exact §10 JSON (tested). The chain continues into production: `explorer_ws.rs` derives `selected_node_id` from `canvas_state.selected_node` and passes it to `execute_tool`, whose `graph_query_dryrun` calls the same `gyre_domain::view_query_resolver::dry_run` used by the composition tests — so `$clicked`/`$selected` resolution is real end-to-end, not test-only. `build_canvas_context_json` surfaces `zoom_level` and `active_filter` to the LLM `<canvas_state>` block (regression-tested; previously silently dropped). Zoom interactions are recorded only for user-armed gestures; every programmatic camera source in `ExplorerCanvas.svelte` (auto-fit, data-change refit, `zoomToNode`, drill transitions, breadcrumb, query-driven fit/level, search fit, tree-group/leaf clicks) calls `disarmZoomTracking()` — verified site-by-site in the diff. Query actions record at all four producer sites (saved-view load, LLM view, user-composed JSON, interactive re-run). Freshness: live `zoom` preferred over snapshots; `visible_tree_groups` owned by a reactive `$effect` on `rootLayoutNodes`; the only `visibleGroups` writes are the initial default and that effect (which writes both fields together), so the wire preference order cannot diverge in the live component.

**§11–15 Composition examples.** `crates/gyre-domain/tests/view_query_composition.rs` uses the byte-exact spec JSON for all four canonical queries (diffed against `specs/system/view-query-grammar.md` lines 100–140), validated and resolved via the production `dry_run`. I independently traced each synthetic graph and the expected result sets match the assertions: blast radius from `$clicked` = clicked + incoming callers + implementors (4 nodes, depths 0/1, callee `store` and disconnected subgraph excluded, edges restricted to result nodes, `$name`/`{{count}}` resolved); re-run from a different click yields that node's radius; no selection warns "click a node". Test gaps = exactly the functions with no `Calls`-path from test nodes (`Implements` does not confer reachability; `Constant` excluded). High-risk = strict `>` 20 intersect test-unreachable (covered-complex, untested-simple, and boundary-20 all excluded); heat metric 35.0 populated. Concept scope = per-seed bidirectional BFS union of 6 nodes, per-seed minimum depths, disconnected subgraphs excluded, partial seeds warn "1 of 2". These are hard membership/exclusion assertions against the real resolver — not self-confirming.

**System default views.** New "High-Risk Untested Code" view in `saved_views.rs` carries the exact §14 shape (`$intersect($where(complexity, '>', 20), $test_unreachable)` + complexity heat), seeded per-repo/per-workspace; count and shape tests updated (6→7) in both `saved_views.rs` and `explorer_views.rs`. Omitting a §15 system default is sound — seed nodes are repo-specific, and the concept scope is reachable via the tested producers.

**§16.** All producer/consumer links exist and are exercised (LLM endpoints, preset buttons, user-composed JSON, renderer).

**Process.** Commit attribution complete: all four task-labeled commits (`8f650148`, `3b1c93fe`, `e9c6e76b`, `3a61cb96`) are listed in `specs/tasks/task-064.md` frontmatter; the two trailing `process:`/`process(task-064)` commits only record that attribution. Coverage rows §10–16 updated to `implemented` with accurate pointers.

## Minor observations (non-blocking, recorded for completeness)

- The LLM `<canvas_state>` block uses the key `visible_groups` rather than the §10 wire name `visible_tree_groups` (pre-existing key; content identical and documented in the system prompt). The §10 JSON defines the frontend→server wire format, which is exact; the prompt block is an internal format.
- `cam` in `ExplorerCanvas.svelte` is not `$state` (pre-existing Svelte warning), so the zoom-sync `$effect` re-fires only on other `canvasState` writes. In practice every recorded zoom gesture writes `recent_interactions` in the same frame, which re-syncs `zoom`; sub-threshold drift leaves wire zoom consistent with the trail. No contradiction with §10 observed.
- Zoom interaction entries carry an extra `base_level` field (coalescing bookkeeping); server serde ignores it.

Full workspace suites (`cargo test --all`, full vitest, arch/Clippy) are owned by deterministic verification; the durable controller finding (checks.sh exit 1) traced solely to the changed-lines rustfmt check, which `3a61cb96` repairs and this review re-verified clean.
