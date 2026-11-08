# Review — task-088 (HSI Explorer Default Views: Boundary, Spec Realization, Change, System Trace)

Spec: `specs/system/human-system-interface.md` §3 "Default Views".
Candidate: `a517589f` (base `2100127c4`, assigned pair). Evidence bundle:
`/tmp/stage/review-evidence/` (probe commands, outputs, mutation probes, actual screenshot).
Verdict: **needs-revision** (one CI-breaking finding; the feature work itself is real and well-tested).

## Round 1

### Verified working (no findings)

- **All four views are real, data-driven implementations** against pre-existing endpoints — no LLM, no stubs:
  - Boundary: Level 1 workspace repo cards enriched from `GET /workspaces/:id/graph` (type/endpoint counts) + `GET /repos/:id/dependencies` chips (best-effort, failures degrade gracefully); Levels 2–4 are the existing ExplorerCanvas drill-down (`ExplorerCanvasDrillDown.test.js` passes).
  - Spec Realization: two-panel `SpecRealizationView.svelte`; linkage via GovernedBy edges (target resolved through spec nodes or edge metadata), `spec_path`, and `spec_paths`; spec nodes excluded from realization; task progress from `GET /specs/:path/progress`.
  - Change: `ChangeView.svelte` queries `GET /repos/:id/graph/timeline?since=…`; Added/Modified/Removed parsed from `delta_json` (nodes_added / nodes_modified with field_changes / nodes_removed); module resolution via Contains edges with qualified-name fallback; entry click navigates to Boundary.
  - System Trace: `SystemTraceView.svelte` renders `GET /merge-requests/:id/timeline` events with expandable gate rows and View Conversation/Diff/Attestation inline sections (conversation SHA resolved from the attestation bundle).
- **`X-Gyre-Last-Seen` wiring is correct**: server middleware (`middleware.rs:170`) stamps `x-gyre-last-seen`; `api.js request()` captures it centrally; `last-seen.js` snapshots the pre-visit value once per page load so `since` anchors to the *previous* visit (the subtle spec requirement) with a 24h first-visit fallback.
- **Backend fix is real**: `parse_delta_counts` now counts both the array shape `graph_extraction.rs` actually serialises and bare-number fixtures; the producer-shape test pins it.
- **Test quality is genuine — three mutation probes all killed** (sources restored after each):
  1. Mutating `getChangeViewSince()` to return the current persisted value → anchoring test fails.
  2. Dropping `since` from ChangeView's timeline query → 2 tests fail (component + end-to-end through ExplorerView).
  3. Removing the array-count arm from `parse_delta_counts` → producer-shape Rust test fails.
- Focused suites: 34/34 new-view tests + 180/180 merge-relevant tests pass; `cargo test -p gyre-server --lib api::mr_timeline` → 13 passed.
- The 11 replaced explorer-visual baselines are a uniform −2px vs base (dimension check across all 12 PNGs), consistent with the view-selector toolbar claim, and all pass live.

### Findings

- [-] **F1 (major, CI-breaking): stale filter-preset visual baselines — explorer-visual suite fails 1/16 at the exact candidate.** `npx playwright test explorer-visual` (server built from current candidate sources, chromium): **15 passed, 1 failed** — `filter_preset_buttons_show_correct_subsets`: `Expected an image 919px by 363px, received 919px by 361px` (deterministic across two runs). The two task-080 baselines `filter-preset-endpoints-linux.png` / `filter-preset-types-linux.png` entered the candidate only via the final base merge `755da0096` (they do not exist at `d4e72e32`/`80c00acd2`/`61e430cbf`), while both of this task's baseline-regeneration rounds (`8407bd1c`, `d4e72e32`) predate that merge and covered only the 11 baselines on the task-088 lineage. The merged toolbar (view selector) renders `.treemap-canvas-area` 2px shorter, so both filter-preset baselines are stale; `filter-preset-types.png` fails identically once the first assertion is repaired. The Shipped claim "Post-update full explorer-visual suite: 15/15 passed" holds only at pre-merge `d4e72e32` (15 tests); at the assigned candidate the suite has 16 tests. Remedy: regenerate both filter-preset baselines against the merged toolbar at the candidate head (same repair as `8407bd1c`/`d4e72e32`).

### Observations (not charged to the task)

- The committed `web/dist` at the candidate head is byte-identical to base's (task-086 bundle, contains none of the four views); task-088's own verified-asset build (`1a50c2276`, which does contain them) was superseded by the task-083 and base merges. Per the convention documented in `specs/tasks/task-083.md` ("the executor restores generated assets from the assignment base and final integration rebuilds them remotely"), this state is executor-owned. Since the SPA is `rust_embed`-embedded at compile time (`spa.rs`), the verifier/publication stage must confirm the final integration rebuilds `web/dist` from the merged sources — a `SKIP_WEB_BUILD=1` build of the raw candidate would serve the task-086 UI.
- `graph_extraction.rs` serialises `nodes_modified` as a bare count when no agent context is present; ChangeView renders only the array shape. Not a defect (the bare-count shape carries no qualified names to render and no agent/spec metadata), but worth knowing when reading the Change View against non-agent extractions.
