# Review — task-067 (Progressive Drill-Down & Breadcrumb Navigation)

Spec: `specs/system/explorer-canvas.md` §5 Progressive Drill-Down.
Candidate: `5892b98a100626e7d7ac2417474da78b9188a912` (base `9bef21a4`). The
product surface is carried by recovered checkpoint `7519d04c` (original final
commit `34a9d4a1` lost to a worker lease expiry; `web/src` byte-identical at
branch head, per the process commits `cfcfc2fc`/`5892b98a`, both docs-only).
Diff scope is clean: ExplorerCanvas.svelte, Breadcrumb.svelte, a 1-line
`rootLabel` wiring in ExplorerView.svelte, the new
`ExplorerCanvasDrillDown.test.js`, the task file, and the coverage row.
Verdict: **approved**.

## Evidence

All runs in `web/` after `npm ci`, via `/tmp/stage/bin/npx vitest`
(`/tmp/stage/bin/cargo` not needed — web-only diff). Full evidence, logs, and
mutation diffs: `/tmp/stage/review-evidence/task-067-review-summary.md`.

Pristine focused runs (exit 0):

- `ExplorerCanvasDrillDown.test.js` → 3/3 passed.
- `ExplorerCanvas.test.js` → 150/150 passed (regression check on the modified
  canvas).
- `ExplorerViewDrillUrl.test.js` → 4/4 passed.

Mutation probes (temporary edits, reverted; tree verified clean after each) —
the shipped tests are not self-confirming, they kill removal of every core §5
behavior:

| Mutation | Caught by shipped suite |
|---|---|
| Fade-out disabled (`drillFadeAlpha` pinned 1.0) | yes — animation test fails |
| Jump-cut (camera interpolation removed) | yes — fails at ≥3 distinct camera poses |
| Contains-edge filtering disabled | yes — all 3 tests fail |
| Leaf nodes allowed to drill | yes — breadcrumb assertions fail |
| Deferred scope swap reverted (immediate breadcrumb push) | **no** — see below |

The escaped mutation concerns only a transition nuance (which layout the fade
plays on). It was verified directly with a temporary discriminating probe
(deleted afterward): in the pristine candidate, 150ms after dblclick the
breadcrumb is still unpushed and the scope lands only after phase 1 completes
(the claimed `pendingDrill` deferral is real); with the deferral reverted the
probe fails (breadcrumb pushed within the same tick). Per the umbrella goal's
out-of-scope rule ("new tests for already-working behavior — not needed, not
wanted"), the missing regression protection is recorded as an observation, not
a blocker.

## Verified against the acceptance criteria

- **Double-click container → children via Contains edges**: real — the layout
  `$effect` filters visible nodes to Contains-descendants of the breadcrumb's
  last entry; container hits (real nodes, or synthetic treemap groups resolved
  via `resolveTreeGroupScopeNode`) drill; mutation-killed test asserts the full
  L0→L1→L2→L3 chain with sibling absence checks.
- **Breadcrumb full path with clickable segments**: real — every segment
  (including the root, now labeled with the repo name via the `rootLabel` prop
  wired from ExplorerView) is a button wired to `navigateBreadcrumb(i)` /
  root reset; navigation restores each level's topology (mutation-killed).
- **Smooth zoom transition + fade-out + reverse transition**: real — two-phase
  rAF ease-out cubic camera interpolation (`drillStartCam` kept off the `cam`
  object, fixing the NaN-camera hit-testing regression), per-node
  `drillFadeAlpha` fade of non-target cells with recovery, deferred scope swap
  so the fade plays on the old layout, and an animated reverse zoom-out on
  breadcrumb navigation (cancels in-flight `pendingDrill`).
- **Lens toggle at every level**: real and tested — toggled at L1 in the chain
  test and at a deep-linked L3 with evaluative overlay, per-node span stats
  scoped to current-level nodes, out-of-level spans verified not to leak into
  tooltips.
- **Leaf double-click opens detail panel, never drills**: real, tested both
  click and dblclick on a leaf, mutation-killed.
- **`cd web && npm test`**: focused suites above pass; the full run is owned by
  the verification stage.

## Notes (non-blocking)

- The shipped `ExplorerCanvasDrillDown.test.js` does not protect the deferred
  scope swap against regression (mutation 5 escaped); the behavior itself is
  present and was verified by an independent probe. If a future round touches
  the drill transition, adding the breadcrumb-timing assertion from the review
  probe would be cheap insurance.
- The during-drill `min alpha < 0.1` assertion is satisfied by low-alpha draws
  unrelated to the drill fade (it passed with the fade fully disabled); the
  suite's actual fade detection is the p75 during/after comparison, which does
  discriminate on this fixture.
