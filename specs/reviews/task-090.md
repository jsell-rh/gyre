# Review — task-090 (HSI Explorer Three Lenses: Structural, Evaluative, Observable)

Spec: `specs/system/human-system-interface.md` §3 "Three Lenses" + `specs/system/explorer-canvas.md` §4 "Lens Definitions".
Candidate: `0e4d78537f0a574f669bf2e181e0eb32b6793e6b` (assigned base `30a14669`; sole product-surface commit `0766a167` "feat(task-090): HSI Explorer three lenses — evaluative overlay", an ancestor of the candidate — verified with `git merge-base --is-ancestor`). The only other changes are the task record (## Shipped, contract restored with checkboxes unchecked), the coverage-matrix row reclassification, and the three test files.
Verdict: **approved**.

## Round 1 (round-14 operational history; independent fresh probes)

All probes run on the exact candidate tree (`0e4d785`, working tree clean, HEAD == candidate):

- `npm ci` (web/) — exit 0.
- `npx vitest run src/__tests__/lens-rendering.test.js src/__tests__/ExplorerViewLensUrl.test.js` — 32/32 passed, exit 0.
- `npx vitest run src/__tests__/ExplorerCanvas.test.js` — 157/157 passed, exit 0.
- `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib api::gates` — 7 passed (incl. `list_spec_approvals_includes_rejection_fields`), exit 0.
- `npm test` (web/, full suite — acceptance criterion) — 69 files, 1707 passed / 41 skipped, exit 0.
- Logs with exit codes under `/tmp/stage/review-evidence/` (`task090-*-r14.log`, `task090-npm-ci.log`).

## Mutation testing (temporary edits, restored and verified byte-identical to candidate)

- **M1** — disable all evaluative node decorations in `ExplorerCanvas.svelte` (`if (lens !== 'evaluative') return m;` → `if (true)`): **3 lens tests fail**. The suite is not vacuous; coverage badges, gate rings, and approval icons are all mutation-killed.
- **M2** — disable the gate **edge**-coloring block only (`if (false)`): all 7 lens tests still pass. The committed "colors edges and node ring" test is satisfied by the node **ring** alone — in its jsdom fixture the `governed_by` edge target (spec node) is alpha-culled (`nodeOpacity` = 0), so the edge never renders.
- **M3** — disable the gate ring only: **1 test fails** at the red-ring assertion.

**Edge-color reachability probe** (instrumented temporary run, evidence `task090-edge-probe-final.json` + `task090-mutation-summary.md`): with a visible fixture (canvas mocked 1200×800, wheel-zoom until leaf opacity > 0), the `drawEdges` gate-coloring branch executes every frame with `{srcId: 'fn1', gateStatus: 'fail'}` and assigns red `#ef4444` strokes. The gate edge coloring is real, reachable production code; edges render only when both endpoints are visible leaves — the standard LOD condition applying to *all* edge rendering, not specific to gate colors. Non-blocking note: the committed component test does not isolate the edge-color block (M2); a future regression there would ship green. The production behavior itself is verified above.

## Verified against the acceptance criteria

- **Lens selector** — three buttons in the `.lens-group` toolbar (Structural default/active, Evaluative, Observable `aria-disabled="true"` with "requires production telemetry integration" title + inline label, matching explorer-canvas.md §4's required grayed-out label). `ExplorerViewLensUrl.test.js` asserts the disabled state + tooltip.
- **Structural default + knowledge-graph base** — `lens` defaults to `'structural'`; `normalizeLens`/`lensFromUrl` reject `observable`/bogus values back to structural (unit-tested); graph loads from `GET /api/v1/repos/:id/graph` (pre-existing).
- **Evaluative overlays** — `lens-rendering.js` pure decoration pipeline: coverage badge from `GraphNode.test_coverage` (real field, `gyre-common/src/graph.rs:127`), green ≥70% / yellow ≥30% / red below; node→governing-spec join via `spec_path` + N:M `spec_paths` + `GovernedBy` edge targets (soft-deleted edges ignored); gate status fail-wins across governing specs; approval status from ledger history (active→✓, rejected→✗, revoked→⏳, no record→⏳). All drawn on top of the unchanged structural fill/border/label in `drawLeafNode` — lenses compose; mutation-killed tests confirm the badges/icons appear only in the evaluative lens.
- **Gate status edge colors** — real code in `drawEdges` (probe-proven, see above); node rings additionally colored green/red.
- **Data sources** — `ExplorerView.loadEvaluativeData` joins gate outcomes from recent MRs (`spec_ref` "path@sha" → per-spec most-recent MR → per-gate latest result) and the approval ledger (`GET /api/v1/specs/approvals`), best-effort/non-blocking. Field shapes verified against the real server types: `MrResponse.created_at: u64`, `spec_ref: Option<String>`; `GateResultResponse.status` "passed"/"failed", `started_at`, `gate_id`; `GraphNode.spec_path/spec_paths/test_coverage`. The gates.rs change only serializes the pre-existing domain fields `SpecApproval.rejected_at/rejected_reason` (distinguishing ✗ rejected from revoked→⏳ pending) — additive, with a server round-trip test.
- **URL persistence** — `?lens=evaluative` restored on mount, synced via `history.replaceState` on switch (other params and `#drill=` hash preserved, structural default drops the param); 5 ExplorerViewLensUrl tests assert against `window.location.href`.
- **No re-layout on lens switch** — component test asserts the node label position is byte-identical across the switch and the evaluative legend appears (re-render really happened).
- **`npm test` passes in web/** — full suite, exit 0.

No scope/ABAC surface changes (no new routes; `/api/v1/specs/approvals` pre-existing), no hexagonal violations, task commit attribution intact (`0766a167` in `commits:` frontmatter), assigned task text byte-identical with checkboxes unchecked.
