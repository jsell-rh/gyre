# Review — task-090 (HSI Explorer Three Lenses: Structural, Evaluative, Observable)

Spec: `specs/system/human-system-interface.md` §3 "Three Lenses" (lines 810-822); cross-referenced `specs/system/explorer-canvas.md` §3 "Lens Definitions".

Assignment: base `124a852b`, candidate `be9b08b9`. Product-surface commit `0766a167` (listed in `commits:` frontmatter); the candidate head additionally contains the round-14 rebase-repair merge `be494b399` (process-type) and the process record commit `be9b08b9` (touches only `specs/tasks/task-090.md`). Cumulative product diff base→candidate: 9 files, +1318/−5 — `web/src/lib/lens-rendering.js` (new, 235 lines), evaluative overlays in `ExplorerCanvas.svelte` (+146) and `ExplorerView.svelte` (+107), `SpecApprovalResponse` rejection fields in `crates/gyre-server/src/api/gates.rs`, three new/extended test files, coverage matrix row 23, task record.

Verdict: **complete** (approved).

## Probes (logs under /tmp/stage/review-evidence)

- `npx vitest run` on the six suites covering both merge-conflicted files (`lens-rendering`, `ExplorerViewLensUrl`, `ExplorerCanvas`, `ExplorerCanvasTrace`, `causal-trace`, `ExplorerViewLargeGraph`) after `npm ci`: **238/238 passed**, exit 0 (`task090-review-vitest-lens.log`).
- Full `npm test` in `web/` (acceptance criterion): **73 files, 1776 passed, 41 skipped** (pre-existing skips; none in the lens suites), exit 0 (`task090-review-npm-test-full.log`).
- `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib api::gates`: **7 passed** incl. `list_spec_approvals_includes_rejection_fields`, exit 0 (`task090-review-cargo-gates.log`).
- `scripts/check-task-commit-attribution.sh`: OK — `0766a167` recorded in frontmatter; process/merge commits out of scope by design.
- Merge integrity: no conflict markers tree-wide; `git diff be494b399^1 be494b399` on the two conflicted files shows **zero deletions of lens/evaluative/coverage/approval/gate code** — the only removed lines are pre-existing trace/warning code evolved by the incoming base (task-193), and both import groups are present and used. Candidate head vs merge commit differs only in the task record.

## Verified working (per acceptance criterion)

- **Lens selector** (`ExplorerCanvas.svelte:5530-5534`): three toolbar buttons — Structural (default), Evaluative, Observable with `aria-disabled="true"` and "requires production telemetry integration" tooltip/badge; component test asserts the disabled state and tooltip text. `normalizeLens` maps `observable` (and any unknown value) back to structural, so the future lens cannot be activated via URL.
- **Evaluative overlays are real data joins, not fixtures**: `loadEvaluativeData` (ExplorerView) fetches recent MRs, resolves each spec path's most recent MR with concluded gates, keeps the latest result per gate (`latestByGate` keyed on `started_at`), and derives pass/fail; spec approvals come from `GET /api/v1/specs/approvals`. Both are best-effort with the structural base rendering fine without them.
- **Coverage badge**: `coverageDecoration` reads `GraphNode.test_coverage` (real field: SQLite column `graph_nodes.test_coverage` → `api/graph.rs:66,105` serialization → frontend), classifies green ≥70% / yellow ≥30% / red below. Component test draws the badge at 85% and asserts fillStyle `#86efac`, plus the differential negative (no `85%` text in structural lens).
- **Gate status**: node ring + edge colors green/red; component tests capture strokeStyle assignments and assert red absent at baseline, present on fail, and green count strictly greater on pass than baseline — a genuine differential, not a mirrored assertion.
- **Approval icons**: ✓/⏳/✗ from the ledger with correct semantics (active→approved; inactive without revocation→rejected; revoked→pending; spec-linked with no record→pending). The `gates.rs` change serializes the pre-existing domain fields `rejected_at`/`rejected_reason` (`gyre-domain/src/spec_approval.rs:27,62`) so rejection is distinguishable from revocation on the wire — server round-trip test proves it, and the frontend resolution logic consumes exactly those fields.
- **Composition / no re-layout**: decorations draw after the structural fill/spec-border/label in `drawLeafNode`; the lens-switch test asserts the node label's fillText coordinates are byte-identical before/after switching and that the evaluative legend actually appeared (the re-render really happened).
- **URL persistence**: 5 component tests against real `window.location` — structural default has no param, evaluative sets `?lens=evaluative` via `history.replaceState` (Back not polluted), structural switch drops it, `?lens=evaluative` deep link restores the active lens on mount, other params/hash preserved (`lensToUrl` unit tests).

## Observations (non-blocking)

- `GraphNode.test_coverage` has no production writer today — extraction sets `None` (`graph_extraction.rs:1532,1957,2082`), as do complexity and churn, which the codebase itself documents as "metrics not extracted by the RustExtractor" (`graph_extraction.rs:726`). The task contract designates `GraphNode.test_coverage` as the coverage data source, the field round-trips end-to-end (persisted by the `create_node` upsert, serialized by the graph API), and the badge renders real data whenever the field is populated. The missing coverage-measurement ingestion is a pre-existing, systemic metrics gap (the "Test Coverage Gaps" saved view from task-088 depends on the same field) and was not assigned to task-090.
- Gate edge coloring applies to edges whose **source** node is governed by a spec with a concluded gate run (outcomes flow outward, matching the trace-flow reading of the canvas). The spec text ("edges colored green/red based on most recent gate results") does not pin the direction; the choice is documented in the shipped record and consistently implemented and tested.
