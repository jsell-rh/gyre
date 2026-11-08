# Review — task-088 (HSI Explorer Default Views: Boundary, Spec Realization, Change, System Trace)

Spec: `specs/system/human-system-interface.md` §3 "Default Views (Automatic, No LLM)".
Candidate under review: `80c00acd2781e83a791d4e0259a0c0b1fdd873df` on `pipeline/task-088/09580bddb4c24eac83389d6554b3771a-1`, diffed against assigned base `637be7187` (task-083 canonical navigation). All six `commits:` frontmatter entries (`3463e28`, `8e025c2`, `1a50c22`, `8407bd1`, `2620dd8`, `d4e72e3`) are present in the base..candidate range and carry the task label.
Verdict: **complete** (approved; no findings). Evidence under `/tmp/stage/review-evidence/`.

## Test runs (this checkout, candidate head, working tree clean)

- Focused vitest on the five new suites (`ExplorerDefaultViews`, `ChangeView`, `SpecRealizationView`, `SystemTraceView`, `last-seen`) → **34 passed, 0 failed**, exit 0.
- Full `npm test` in `web/` (explicit acceptance criterion; `npm ci` first) → **78 files, 1793 passed, 41 skipped**, exit 0.
- `cargo test -p gyre-server --lib api::mr_timeline` → **13 passed, 0 failed**, exit 0 (includes the new producer-shape regression test).

## Mutation probes (restored afterwards; `git status --porcelain` clean)

- Frontend: force `getChangeViewSince()` to ignore the persisted last-seen value → `ExplorerDefaultViews` "queries the timeline since last visit" and `ChangeView` "queries the graph timeline with since = cached last-seen value" both **FAIL**. The `since` wiring assertions genuinely pin the `X-Gyre-Last-Seen` cache → `GET /repos/:id/graph/timeline?since=` contract.
- Rust: remove the `Value::Array` arm from the new `count()` helper in `parse_delta_counts` (restoring the old number-only behavior) → `parse_delta_counts_producer_array_shape` **FAILS**. Independently confirmed the producer (`graph_extraction.rs:362-384`) serialises `nodes_added`/`nodes_modified` as arrays (one branch emits a bare `nodes_modified` count, which the fixed parser also accepts), so the old parser's 0/0 on every real GraphExtraction event was a genuine defect and the fix is real.

## Verified working (no findings)

- **All four views are real, data-backed, no LLM.** Every data source was traced to a real backend: `GET /repos/:id/graph/timeline` (mod.rs:883 → `graph::get_graph_timeline`), `GET /api/v1/merge-requests/:id/timeline` (`api::mr_timeline::get_mr_timeline`, pre-existing, 13 tests), `X-Gyre-Last-Seen` set by real middleware (middleware.rs:100,170), `GET /workspaces/:id/graph` (`get_workspace_graph` aggregates real graph-store nodes; node responses carry `repo_id` so the Level 1 type/endpoint counts are computed, not hardcoded), `GET /repos/:id/dependencies` (real `target_repo_id`/`target_artifact` fields). All `api.js` helpers used by the new components pre-exist.
- **Boundary View:** Level 1 workspace repo cards with type/endpoint counts and cross-repo dependency chips (best-effort enrichment, failures degrade gracefully — not a break-the-list failure mode); Levels 2–4 are the pre-existing ExplorerCanvas Contains-hierarchy drill-down pinned by the pre-existing `ExplorerCanvasDrillDown.test.js`; Level 4 spec linkage/modification metadata renders in the pre-existing `NodeDetailPanel` (spec_path/spec_confidence, last_modified_*, churn, complexity — all present and pre-tested).
- **Change View:** Added (+)/Modified (~)/Removed (−) entries parsed from real `delta_json` shapes with name, type, containing module (Contains-edge lookup with qualified-name fallback), spec ref, authoring agent, timestamp; modified entries expand to per-field old→new; resolvable entries navigate to Boundary via `navigateToBoundaryNode` (switches mode + opens the detail panel). `last-seen.js` snapshots the pre-visit persisted value at module init so "since last visit" stays anchored to the previous visit even though the current visit's first response already stamps a newer header — correct handling of the spec's client-side header cache; 24h first-visit fallback matches the Briefing semantics.
- **Spec Realization View:** genuine two-panel layout; left panel from `GET /specs` with status badges and linked-node counts; right panel links nodes via all three real sources (GovernedBy edges with spec-node/metadata/path-string target resolution, `spec_path`, `spec_paths`); task progress from `GET /specs/:path/progress`; spec nodes excluded from realization; Read/Edit opens the inline spec editor; node click navigates to Boundary.
- **System Trace View:** MR selector over `mergeRequests({repository_id})`, vertical timeline of typed events from the real MR timeline endpoint, gate summary counts, expandable gate rows, and View Conversation/Diff/Attestation inline sections (conversation SHA resolved from the attestation bundle → `GET /conversations/:sha`).
- **View selector:** `explorer-view-selector` tablist with exactly Boundary | Spec Realization | Change | System Trace, Boundary default (canvas renders), panels swap the canvas out and back. Composes correctly with task-083's Graph|Briefing sub-tab tablist (test queries scoped to the selector tablist).
- **Backend fix is genuine:** see mutation probe above.
- **Visual baseline regeneration is drift, not fakery:** the 11 replaced baseline PNG dimensions at candidate head match the shipped notes exactly (uniform 34px vertical reduction from the merged Graph|Briefing sub-tabs row — 919x395→919x361, 579x333→579x299, 579x296→579x262, 579x466→579x432; widths unchanged) while the toolbar, blast-radius-tiered and full-page baselines are untouched. The candidate's local mutation probe (view-selector block removed → zoom_level_0 size mismatch) plus the dimension corroboration support that the baselines pin the new toolbar; the live E2E re-run belongs to CI/verification.
- **No production-code hazards:** no routes added (no ABAC registry exposure), no scripts/specs/verifiers touched, no invariants from AGENTS.md implicated by the diff (frontend + one self-contained Rust parser fix + baselines).

## Notes for verification (not findings)

- Live Playwright explorer-visual E2E against the new baselines should be re-run in CI (not re-run here per smallest-meaningful-probe; sandbox listener/browser evidence recorded in capabilities.json).
- `specs/coverage/system/human-system-interface.md` row 20 still reads `task-assigned | task-088`; per repo convention the coverage row is finalized by the verification/coverage stages, and this review found the underlying behavior real, so no coverage edit was made by the reviewer.
