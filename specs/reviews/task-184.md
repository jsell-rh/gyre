# Review — task-184 (Flow Traces — animated particle flow visualization)

Spec: `specs/system/system-explorer.md` §5 "Flow Traces" (+ `ui-layout.md` §4 flow-layout detail, `human-system-interface.md` §3a trace capture).
Candidate: `e83d12db` (product commit `23fbb3b6`, process `8af32f644`/`e83d12db`) on base `7cf2f81c`.
Verdict: **approved** — no blocking findings.

## Scope determination

The diff is web-only (10 files). The §5 particle machinery pre-exists at
base on the task-065/015/S4.4d substrate and was verified present and
tested there: `FlowCanvas.svelte` (SVG node/edge layer + Canvas 2D/WebGL2
particle overlay, node/particle/edge click interactions with pause +
span tooltips/detail, per-node badges with span count / error rate / mean
duration), `flow-particles.js` (span-tree → particle transitions,
success `#3b82f6` / error `#ef4444`, 200ms/20px fading trails),
`particle-layer.js` (`WEBGL_PARTICLE_THRESHOLD = 100` Canvas2D→WebGL2
switch), `PlaybackControls.svelte` (play/pause/step/speed/scrub/test-case
selector), `flow-layout.js`, and the `"flow"` layout registration in
`layoutRegistry.js` (ExplorerCanvas delegates at `isFlowLayout`).
The candidate closes the two §5 gaps that remained on top of that
substrate — the spec-mandated predefined flows and trace-source
discovery — plus a flow-layout view-query scoping defect.

## Probes run (evidence: /tmp/stage/review-evidence/evidence.md)

- `npm ci` then focused vitest on the four touched test files: **33/33
  passed** (flow-templates unit tests, ExplorerViewFlowTemplates
  component integration, ExplorerCanvas.flow scoping tests,
  ExplorerDefaultViews selector list).
- **Mutation test 1** — disabled the new `visibleNodeIds` scoping in
  FlowCanvas `flowNodes`: 3 tests fail (scoped-node set, edge dropping,
  both ExplorerView template-application tests asserting `unrelated` is
  filtered out). Mutation killed; the scoping is really tested.
- **Mutation test 2** — disabled `pickTraceMr` pattern preference: 2
  tests fail (unit preference test + the integration test asserting
  `mrTrace` is called with the template-matching MR, not the newest).
  Mutation killed.
- Regression run around the `clearGeneratedLayout` change
  (ExplorerGeneratedViews, LayoutRenderers, Scope, ViewEvents,
  ExplorerCanvasTrace): **51/51 passed**.
- Pre-existing flow machinery tests (FlowCanvas, particle-layer,
  flow-layout): **28/28 passed**, including WebGL fallback activation
  above the 100-particle threshold.
- Static: server dependency `GET /api/v1/workspaces/:id/merge-requests?has_trace=true`
  exists pre-existing and registered (`api/mod.rs:710` →
  `list_workspace_mrs`, merge_requests.rs:795-860, server tests at
  :2534-2601) — the new `api.workspaceMergeRequests` call is not dead
  wiring. `api.mrTrace` pre-exists.

## Verified working

- **Predefined flow templates (§5 "predefined flows")**: the three
  spec-mandated flows (agent spawn, MR merge, spec approval) are defined
  in `flow-templates.js` with a starting point, expected call path
  (word-token matching against graph node names — whole-token only, so
  `spawn` does not match `unsawn`; spec/file/test node types excluded),
  and governing spec linkage. The View selector gains a "Trace flows"
  section; selecting a template applies the flow layout scoped to the
  matched path via a concept-seeded ViewQuery (depth 0 — exactly the
  path), and animates it with the captured gate trace of the best-matching
  workspace MR discovered through the has_trace endpoint. Each step's
  spec linkage appears in the view annotation (asserted in DOM).
- **Honest degradation, no fabricated particles**: with no traced MR the
  expected path renders structurally with an explanatory toast and no
  playback bar — backed by the real conditional
  `lens === 'evaluative' && traceData?.spans?.length` in FlowCanvas.
  A graph matching no step warns and changes nothing (asserted: no layout
  swap, no API calls). Spans that don't map onto scoped path nodes are
  filtered out in `flow-particles.js` (`visibleNodeIds` guard).
- **Flow-layout view-query scoping**: ExplorerCanvas now forwards
  `queryMatchedIds`; FlowCanvas filters its node set by it, and edges into
  unmatched nodes drop implicitly (edgeViews requires both endpoints in
  the layout map). The null/empty → full-graph behavior genuinely mirrors
  the other renderers (queryMatchedIds null → full opacity everywhere).
- **`clearGeneratedLayout` fix**: dismissing a generated/flow view now
  clears `activeSavedViewName` so the selector label returns to the base
  view — previously the label stuck on a view no longer displayed.
- **Test quality**: no self-confirming tests found. Both mutations were
  killed; the integration tests drive the real component with mocked
  transport only, assert DOM structure and API call arguments
  (`has_trace: true, limit: 10`, template-matching MR id), and the
  no-trace/no-match paths assert absence (no playback bar, no layout
  change, no API calls).

## Non-blocking observations

- `resolveFlowTemplateTrace` reuses an already-loaded `traceData`
  regardless of which MR it came from (documented in-code as best-effort
  for the repo). Degradation is honest — only spans mapping onto the
  template's scoped path animate — but a template applied after viewing
  an unrelated MR's trace can animate that trace's spans over path nodes.
  Acceptable heuristic; spans shown are real captured spans for those
  nodes.
- `pickTraceMr` falls back to the most recent traced MR when nothing
  matches the template's `mr_match` patterns (documented; same honest
  degradation to no-particles when spans don't map).
- `web/dist/` not regenerated by the agent — verifier-owned per current
  process; not a source defect.
- Working tree note: `specs/tasks/task-184.md` arrived at review time with
  its frontmatter reverted to `not-started` (stage setup, present before
  any review action); left untouched.

## Verifier instructions

Standard integration pass; nothing task-specific beyond the pre-existing
`cargo test -p gyre-server merge_requests` coverage for the workspace MR
list route. Full `web` vitest suite and the dist rebuild + commit belong
to the verifier as usual.
