# Review: TASK-078 — Explorer Phase 4 Polish (Streaming, Keyboard Shortcuts & Performance)

**Reviewer:** Independent review round
**Candidate:** `100a9f6bba6893e1aadef081f38345bbc1f48f73` (branch `pipeline/task-078/a93bcab9d1d543e589d710d4d59788bc-2`, HEAD == candidate)
**Base:** `78cd03102d1d2a0226bc6ad4a45d454cb8aac928`
**Spec ref:** explorer-implementation.md §28 Phase 4: Polish
**Commits reviewed:** 293246b7c (production), 08660fe83, 2cfe7e161, 100a9f6bb (process/contract)

## Scope

Diff touches only `web/src/**` + `specs/tasks/task-078.md` (+ `en.json` locale key): 278 insertions, 11 deletions, zero Rust changes — cargo test/fmt surface unaffected by this candidate. The production change closes the one genuine §28 Phase 4 gap (the >10k-node performance warning banner); the other five polish items were audited against production code at the exact candidate head.

## Audit of §28 Phase 4 items (production code at candidate HEAD)

- **Streaming**: `ExplorerChat.svelte` accumulates `done:false` text chunks into `$state` streaming text rendered per-chunk (not batched); `done:true` finalizes; 65s no-chunk timeout and disconnect both finalize orphaned streams. Confirmed at lines ~157–420.
- **Status indicators**: status state machine (connecting/ready/thinking/refining/error/disconnected) with header dot + label and a thinking indicator with Cancel. Confirmed.
- **Escape**: `ExplorerView.svelte:998` overlay cascade (spec editor → query editor → detail panel → `activeViewQuery = null`). Confirmed.
- **`/`**: `ExplorerView.svelte:1007` — `preventDefault`, focuses `.chat-input`, suppressed while typing in INPUT/TEXTAREA/SELECT/contentEditable. Confirmed.
- **Responsive**: `@media (max-width: 768px)` stacks canvas/chat; `chatCollapsed` bottom sheet; reset when viewport widens past 1025px. Confirmed (lines 631–641, 1977–2007, 3784).
- **Large-graph rendering**: frustum culling (`ExplorerCanvas.svelte` lines 2453/3007/3475), zoom-level LOD label guards, edge bundling, rAF-coalesced redraw loop (`scheduleRedraw` → `requestAnimationFrame(runFrame)`). Confirmed.
- **Perf warning banner (this task's gap-fill)**: `LARGE_GRAPH_THRESHOLD = 10000`, `$derived.by` over `effectiveGraph.nodes.length`, dismissible `role="status"` banner naming the actual count, `perfWarningDismissed` reset on new repo graph load; toolchain + perf warnings stack in `.canvas-warning-stack` (positioning preserved). Confirmed (`ExplorerView.svelte` ~1035–1043, 1408–1430, 2867–2876).
- **Visual regression**: 14 committed Playwright baselines in `web/tests/e2e/explorer-visual.spec.js-snapshots/`.

## Probes (full detail: `/tmp/stage/review-evidence/task-078-review-probes.md`)

All frontend probes after `npm ci` in `web/`:

- `npx vitest run src/__tests__/ExplorerViewLargeGraph.test.js` — 2/2 pass.
- **Mutation probe**: raised the production threshold to 99999999 (banner can never render) → suite fails 1/2 (`× shows a warning banner above 10k nodes...`). The new test is mutation-sensitive, not self-confirming. File restored; tree clean.
- `ExplorerViewShortcuts + ExplorerChat-streaming + ExplorerView-responsive` — 20/20 pass.
- `ExplorerCanvas-performance` (10k/15k-node graphs, draw-call counting) — 19/19 pass.
- `ExplorerChat + ExplorerViewDrillUrl + ExplorerChat-component` — 133/133 pass.
- `scripts/check-task-commit-attribution.sh` — exit 0.

## Contract compliance (durable finding)

- Acceptance-criteria checkboxes restored to `[ ]` (round-2 head commit).
- Notes recorded under `## Shipped` with `###` round subheadings.
- Round-1 production fix (`293246b7c`) preserved unchanged.
- Coverage row §28 remains `task-assigned` — correct: rows move to `verified` via the Spec-Fidelity Auditor, not the implementing agent.

## Verdict

**Approved.** The genuine gap (perf warning banner) is closed with real production code and a mutation-sensitive boundary test (10001 shows / 10000 doesn't / dismiss hides); the five pre-existing items are confirmed present in production code with green focused suites (174 tests across the claimed files); the protected task contract is intact.
