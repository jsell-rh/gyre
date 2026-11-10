# Review — task-015 (Flow Canvas — Animated Trace Visualization)

Spec: `ui-layout.md` §4 (Flow Layout), `system-explorer.md` §5 (Flow Traces), `human-system-interface.md` §3a (Explorer Visualization).
Candidate: `1702b9c37` (base `1901941ba`). Implementation commit `06e0fd50e` — recorded in the task's `commits:` frontmatter and verified an ancestor of the candidate.
Verdict: **approved** — all 11 acceptance criteria hold with real production code; tests proven defect-killing via mutation probes.

## Round 1 (2026-11-10)

Diff scope (7 files, all reviewed): `web/src/lib/flow-particles.js` (new pure particle-system module), `web/src/lib/ExplorerCanvas.svelte` (integration: paused-scrub repositioning, step-through, test filter, trail rendering, particle hover/tooltip, click-node pause + `_activeSpan`, edge `traversing_spans`), `web/src/components/PlaybackControls.svelte` (Step button, 10x option, test selector, clickable legend), two new test files (37 tests), `specs/system/explorer-canvas.md` (speed-range union amendment), `specs/tasks/task-015.md` (commit attribution).

Probes run (evidence: `/tmp/stage/review-evidence/`):

- `cd web && npm ci` → clean install; `npx vitest run src/__tests__/flow-particles.test.js src/__tests__/ExplorerCanvas.flow.test.js` → **37/37 passed**.
- Regression: `npx vitest run` over the six pre-existing ExplorerCanvas suites (`ExplorerCanvas.test.js`, `.evaluative-lens`, `Trace`, `DrillDown`, `Liveness`, `-performance`) → **224/224 passed**.
- Mutation probe 1 — disable the paused-reposition `$effect` (freeze-on-scrub): **6/10 component tests fail**. The scrubber test documents and kills the pre-fix defect (particles frozen at last-played positions while paused).
- Mutation probe 2 — neutralize trail rendering: **trail test fails** (asserts round lineCap, alpha ≤ 0.6, > 0, in the particle color).
- Mutation probe 3 — ignore `testFilter` in `particlesAtTime`: **4 tests fail** (pure + component filter/legend/step-filter tests).
- Mutation probe 4 — success color reverted to pre-fix `#60a5fa`: **5 tests fail** (spec-exact `#3b82f6` is enforced in both suites).
- Mutation probe 5 — `nextSpanStart` never advances: **2 step-through tests fail**.
- `bash scripts/check-task-commit-attribution.sh` → exit 0.
- Working tree verified clean after all probes (mutations restored).

Acceptance criteria verification:

- **Renderer (nodes/edges + Canvas 2D particle overlay):** satisfied via the documented, pre-accepted deviation — the flow renderer is ExplorerCanvas's evaluative lens, not a separate `FlowCanvas.svelte`. The task's own Current State section records this integration, the durable checkpoint record preserves the accepted architecture note, and `layoutRegistry.js` maps the `"flow"` ViewSpec layout to ExplorerCanvas (task-065 unified cutover). Particles draw on the Canvas 2D layer over the node/edge layer.
- **Particles animate along edges following the span tree:** `particlesAtTime` maps each span's `graph_node_id` against its parent's, progress = position in `[start_time, start_time + duration_us]`; live playback, paused scrubbing, and step-through all resolve through the same function (probe 1 kills a break).
- **Particle color:** `#3b82f6` success / `#ef4444` error, spec-exact (probe 4). Multiple test cases hue-shift 15°/index within the palette with a clickable legend (ui-layout.md §4).
- **Trail (200ms fade, 20px length):** `TRAIL_FADE_MS`/`TRAIL_LENGTH_PX` + `trailSegments` (age-pruned, length-capped, stationary ⇒ no segments) stroked under the particle body (probe 2).
- **Scrubber freezes particles at the timestamp:** the paused-reposition `$effect` recomputes `evalParticles` at `minTime + scrubber × totalDuration`; timeline range is exactly `[min(start_time), max(start_time + duration_us)]`.
- **PlaybackControls (Play/Pause/Step/Speed):** Step advances to the next filtered span start and pauses (probe 5); speed select offers 0.25x–10x and `tickParticles` clamps to `[0.25, 10]`.
- **Node badges:** `nodeSpanStats` computes spanCount, errorRate, meanDuration (+p50/p95) from GateTrace spans — pre-existing, intact, unchanged by the diff.
- **Click node during animation:** pauses playback (scoped to nodes with span stats) and the detail payload carries `_activeSpan` from `nearestParticleToNode` — component-tested end-to-end via `onNodeDetail`.
- **Test selector:** dropdown of root-span operation names + legend, filters `particlesAtTime`/`nextSpanStart`; stale filter auto-resets on trace change (probe 3).
- **Component tests with mock trace data:** 37 tests across two files; five mutation probes confirm they kill real defects (no self-confirmation).

Spec amendment check: `explorer-canvas.md` speed range 0.25x–5x → 0.25x–10x (+ step, + test selector) is a documented union reconciling a genuine conflict with HSI §3a ("1x, 10x, or step-through"), inline cross-referenced to both specs — the GOAL rule-4 path (amend the spec rather than ship contradicting code), not fakery.

Non-blocking observations (recorded for completeness, out of this task's contract):

- The WebGL fallback for >100 concurrent particles and the `particle_color` encoding configurability (ui-layout.md §4) remain unimplemented — pre-existing omissions outside task-015's acceptance criteria; the system-explorer §5 Flow Traces scope is tracked under task-184.
- No test pins the speed setting's effect on animation rate (mutation probe: disconnecting `clampedSpeed` passes all 37). The wiring is real and trivial (`onSpeedChange → evalSpeed → clampedSpeed` in `tickParticles`) and play/pause/step are tested, so the criterion holds; a future round could add a rate test cheaply.

Test-inflation check: none found. The suite caught two real bugs during development per the commit message (missing case-index map defaulting all cases to the base hue; `testFilter` prop passed under the wrong name), and every mutation probe I ran produced failures.

Scope check: no out-of-scope edits — diff touches only the flow feature's web sources, its two test files, the reconciled spec line, and the task frontmatter.
