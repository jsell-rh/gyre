# Review — task-086 (HSI Explorer Liveness Indicators, Two Layers)

Spec: `specs/system/human-system-interface.md` §3 "Design Principle: The Built
Software Is Always Alive" — Layer 1 (active-agent glow from `RunStarted`/
`RunFinished` telemetry, fading recent-change ring from `ArchitecturalDelta`
records, gate-status edges in the Evaluative lens only) + Layer 2 scaffolding
(trace play icon on nodes with span data).

Candidate `d5839630`, base `078ec12a`. Verdict: **complete** (approved).

## Scope of the candidate delta

`git diff 078ec12a..d5839630` touches only `specs/tasks/task-086.md`
(documentation: progress → ready-for-review, Shipped-section rewrite). The
merge commit `f7b1267e9` merges base into the retained checkpoint branch with
tree `5f6ae702` — byte-identical to base's tree — confirming main already
contains every content line of the checkpoint. All four `commits:` frontmatter
commits (`2aafb8f2`, `c4d00719`, `7da87564`, `a3ad5d02`) are ancestors of the
candidate. The product code under review is therefore the tree at candidate
== base; verified in place, not assumed from the label.

## Implementation verified (production code, in-tree)

- **Active agent glow** — `web/src/lib/liveness.svelte.js`:
  `createLivenessTracker` listens for `{kind: 'run_started'|'run_finished',
  payload: {agent_id, task_id}}` bus messages, tracks runs in a reactive
  `activeRuns` map, resolves task → `spec_path` via `fetchTask` (guarded
  against a run finishing mid-fetch). Wired in `App.svelte:87` to the shared
  workspace WS store. `ExplorerCanvas.svelte:3426` pulses governed nodes
  (sky-400, sine cycle, shadowBlur) — glow only while the run is tracked;
  the render loop stays alive only while a glowing node is on screen
  (`glowAnimating` at 4333, bounded by `layoutNodeMap`).
- **Recent-change fading ring** — `ExplorerView.svelte:1505`
  `loadLivenessTimeline` queries `api.repoGraphTimeline(repoId, since)` with
  `since = now − 1h` (epoch seconds, no `until`) on canvas load, so rings
  appear in the default view; `computeRecentChanges` folds deltas (epoch-sec
  timestamps ×1000, qualified_name matching) into a per-node change-time map;
  draw-time `recentChangeAlpha` fades the amber ring linearly to zero over
  exactly 60 minutes; a 60-second interval nudges redraw while unfaded rings
  exist (no per-frame timer).
- **Gate-status edges (Evaluative lens only)** — `ExplorerView.svelte:1477`
  `loadGateData` fetches repo MRs (`repository_id` + `per_page: 10` — the
  params `ListMrsQuery` actually recognizes; server-side `per_page` cap
  confirmed at `api/merge_requests.rs:763`) and per-MR gates into
  `gateRecords`; `computeNodeGateStatus` maps spec_ref → governed nodes
  (failed wins, only terminal gate states count). Rendering is double-gated
  on `lens === 'evaluative'` (derivation at `ExplorerCanvas.svelte:448`,
  edge draw at 3952), so the default Structural lens never shows gate
  colors. The later per-spec-recency `gateStatusBySpec` overlay composes
  with documented precedence.
- **Layer 2 scaffolding** — `nodeSpanStats` (derived over
  `GET /merge-requests/:id/trace` spans) drives a ▶ badge
  (`ExplorerCanvas.svelte:3735`); clicking the badge (hit-test region ==
  drawn region, 4648) switches to the Evaluative lens and starts span-tree
  playback. Full particle/scrubbing polish is explicitly a follow-on per the
  task plan.

## Probes (evidence: /tmp/stage/review-evidence/)

- `npm ci` in `web/` (locked versions).
- Liveness suites (`liveness.test.js`, `ExplorerCanvasLiveness.test.js`,
  `ExplorerViewLivenessWiring.test.js`): **46/46 passed**.
- Adjacent shared-surface suites (`ExplorerCanvas.test.js`,
  `ExplorerCanvas-performance.test.js`, `lens-rendering.test.js`):
  **204/204 passed** — covers the "canvas performance is not degraded"
  criterion.
- Full `npm test` in `web/`: **99 files, 2010 passed / 41 skipped** — the
  task's explicit acceptance criterion.
- **Mutation probes** (each restored; `git status --short` clean after):
  1. Disable the glow render branch (`if (false && activeAgentNodeIds.has…)`)
     → 2 tests FAIL (glow drawn; glow lifecycle).
  2. `recentChangeAlpha` returns constant 1 (no fade) → 3 tests FAIL across
     both suites (ring fade math + canvas fade term).
  3. Remove the Evaluative-lens gate from BOTH the derivation and the edge
     draw (gate colors in every lens) → 1 test FAILS ("structural lens
     (default) shows NO gate colors"). Mutating only one of the two gates
     passes — defense in depth, and the test anchors the spec-visible
     behavior.
  4. Disable the ▶ badge render → 2 tests FAIL (play icon shown; click-to-play).
  All four acceptance-relevant behaviors are genuinely pinned by tests that
  fail when the behavior breaks; no self-confirming or mirrored-logic tests
  found (canvas tests assert on captured ctx stroke colors/shadow/alpha from
  the real component; wiring tests assert on fetch params).
- Server-side anchors: `per_page` cap implemented in
  `api/merge_requests.rs` (newest-first, truncate before enrichment);
  `repository_id` is the recognized query key; `GET /repos/:id/graph/timeline`
  with `since`/`until` (epoch seconds) in `api/graph.rs`.

Findings:

- (none)
