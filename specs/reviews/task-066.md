# Review — task-066 (ExplorerCanvas — Lens Definitions & Evaluative Overlay)

Spec: `specs/system/explorer-canvas.md` §4 Lens Definitions.
Candidate: `1fb542e07` on `pipeline/task-066/985d4f12610e4ee0b0da511b3027fc50-1`, base `66a5fc679`. Diff scope: `web/src/lib/ExplorerCanvas.svelte` (evaluative badge chips + render-loop scheduling), new `web/src/__tests__/ExplorerCanvas.evaluative-lens.test.js`, ctx-mock additions to `ExplorerCanvas.test.js`, and the task-file record. No production code outside the task surface was touched.

Verdict: **approved**.

## Evidence

All probe logs under `/tmp/stage/review-evidence/` (summary: `probe-summary.md`).

- Focused suite at candidate head: `ExplorerCanvas.evaluative-lens.test.js` → 4/4 passed.
- Mutation check at the pre-fix merge base `876ca6ec4` (detached git worktree, `npm ci`-locked tools): **3/4 failed** — both badge-chip tests and the redraw-after-prop-change test. The new tests kill the two real defects they claim to pin; they are not self-confirming.
- Sibling ExplorerCanvas suites: 168/168 passed.
- Full web suite (`cd web && npm test` after `npm ci`): 65 files, 1630 passed, 41 skipped, exit 0 — matches the shipped claim.
- Test-quality and process guards at candidate head: `check-assertionless-tests`, `check-tautological-assertions-js`, `check-conditional-test-guards`, `check-dead-test-code`, `check-dead-test-variables-js`, `check-comparative-test-claims-js`, `check-aspirational-test-names`, `check-task-commit-attribution` — all clean.
- Debug scaffolds from the interrupted checkpoints (`tmp-repro.test.js`, `zz-debug.test.js`) are absent at the head.

## §4 audit (candidate head, code-verified)

- **Structural**: spec-coverage coloring green/amber/red via precomputed `governedByIndex` plus `spec_confidence` fallback (fill `ExplorerCanvas.svelte:3144-3157`, border `specBorderColor:113`); node size = complexity (`nodeWeight:1437`); border thickness = churn (`churnThickness = min(4, 1+churn×0.3)`, `:3180`); click → detail panel, double-click → drill, edge click → relationship detail; right-click context menu with "View spec", "View provenance" (primary, `:5489-5502`) and "Trace from here" (secondary, `:5525`); ghost overlays hardcode `basePulse=0` in the structural lens (`:2729`) — no particles, no animation.
- **Evaluative**: particles along trace execution edges (`evalParticles`/`tickParticles`, advanced only while `evalPlaying`); node heat blue=fast → red=slow (`heatColor:2197`, `blue-red` palette `:724`); badge chips render span count, error rate, AND mean duration on every traced node independent of the heat metric (`:3340-3370`, the repaired defect); edge thickness 1.5–6px proportional to trace call frequency (`:3560-3566`); failed-span red glow (`nodeHasErrors:773` + shadowBlur); PlaybackControls with play/pause, scrubber, speed 0.25x–5x (select options; `evalSpeed` clamped `:890`); particle click → span detail panel (`particleHitTest:4158` wired `:4282`); hover tooltip with p50/p95/error rate (`:5453-5466`); structural fill stays visible under the heat overlay (`:3158-3161`).
- **Observable**: toolbar button grayed (opacity 0.35, `aria-disabled`, 🔒) with "(requires production telemetry integration)" label; clicking shows `ObservableBanner` instead of switching lenses (`:5312`, `:5756`).
- **Lens switching**: `onLensChange` (`:778`) only syncs canvas state and redraws — camera/viewport untouched, topology preserved.

## Repair soundness (7a3d96f0)

1. **Badge chips**: condition changed from `evalColor &&` (single heat-metric badge) to `lens === 'evaluative' && stats.spanCount > 0`, rendering a right-to-left chip row with all three §4 metrics. `evalColor` remains in scope and non-null whenever the lens is evaluative; the `?? '#3b82f6'` fallback is purely defensive.
2. **Render-loop scheduling**: pending-frame tracking moved from the rAF handle to an `animScheduled` flag (a synchronous rAF runs the callback before returning, so `animFrame = requestAnimationFrame(...)` restores a stale handle after a nested stop and every later `scheduleRedraw()` bailed — permanent freeze, reproduced by the failing pre-fix test). The synchronous re-frame chain is depth-capped at 100: `syncFrameDepth` is incremented only around the `requestAnimationFrame` call and decremented in `finally`, so under real (async) browser rAF the counter always returns to 0 before the next callback runs and the cap is unreachable in production; under sync rAF (jsdom/sync-polyfill webviews) the chain stops cleanly and a later `scheduleRedraw()` starts a fresh chain. `runFrame` wraps `animLoop` in `untrack()` so the loop never registers reactive dependencies in the scheduling effect. `animFrame` is consumed only by scheduling and `onDestroy` cancellation.

## Notes (non-blocking)

- The fourth test ("untraced nodes get no badge chips") also passes at the pre-fix base — a weak negative test — but the other three carry the mutation-killing weight, so the suite is not test-inflated.
- In a genuinely synchronous-rAF embedded webview, a continuous animation now runs in 100-frame bursts between `scheduleRedraw()` calls rather than recursing to stack exhaustion; this is the documented, intended degradation and unreachable in real browsers.
- The `specs/coverage/system/explorer-canvas.md` §4 row still reads `task-assigned`; per the pipeline convention (task-199 review) coverage-row promotion is the controller auditor's call, not the implementer's, so this is not a finding against the candidate.
