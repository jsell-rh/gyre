# Review — task-176 (Meta-specs Preview Loop layout — editor split states and preview workflow)

Spec: `specs/system/ui-layout.md` §9 "Meta-specs Preview Loop Layout".
Assigned base `fbf1a63b`, candidate `e8782818` (branch
`pipeline/task-176/938fa20b45b34af2af2243843334c681-1`).
Product commits under review: `eee86b3e` (State 2 pending-changes diff),
`6e3bb1a` (publish refreshes the diff baseline), `91fe1e86` (Iterate keeps
results visible); remainder docs/process/merge commits.
Verdict: **needs-revision** — one material finding (F1, stale shipped bundle,
carried over unresolved from round 1); round-1 findings 2/3/4 are verified
fixed at the source level.

## Round 2 probes (all evidence in /tmp/stage/review-evidence/)

- `cd web && npm ci` (locked versions) → exit 0.
- `npx vitest run src/__tests__/EditorSplit.test.js src/__tests__/MetaSpecs.test.js src/__tests__/lineDiff.test.js`
  → **166/166 passed** (`task-176-focused-tests.log`).
- Mutation probe (`task-176-mutation-showResultTabs.log` / `task-176-restored-iterate-test.log`):
  temporarily reverted `showResultTabs` in `web/src/lib/EditorSplit.svelte` to
  the pre-fix logic (`context !== 'meta-spec' || previewState !== 'editing'`,
  dropping `|| Boolean(metaPreviewResult)`) and re-ran the §9 Iterate test —
  it **fails** (1 failed) exactly at the retained-results assertions; restored
  the file (git diff HEAD -- web/src is empty afterwards) and it **passes**
  again. The test genuinely kills the round-1 Iterate defect; it is not
  self-confirming.
- Dist staleness probe (`task-176-dist-staleness.log`, script
  `probe-dist-staleness.sh`): see F1.

## Verified fixed from round 1 (no findings)

- **Iterate retains results (round-1 finding 2).** `EditorSplit.svelte:249-251`
  derives `showResultTabs = context !== 'meta-spec' || previewState !== 'editing'
  || Boolean(metaPreviewResult)`; the right pane renders the target-spec
  selector (`spec-selector`) in meta-spec editing state with the retained
  Architecture/Code Diff results (`meta-impact`, `tab-architecture`,
  `tab-code-diff`, `meta-spec-statuses`) below it. Confirmed both by the
  passing §9 test (`EditorSplit.test.js` ~line 582, real
  PreviewResponse/PreviewStatusResponse shapes from `meta_specs.rs`, poll run
  to completion, Iterate clicked, selector AND tabs asserted) and by the
  mutation probe above.
- **Test gap closed (round-1 finding 3).** The iterate-results-visibility
  test now exercises the meta-spec context (`context: 'meta-spec'`,
  `workspaceId`, `targetSpecs`) — the exact surface that failed in round 1 —
  not just the spec/repo context.
- **LLM chat limitation recorded (round-1 finding 4).** The task doc carries
  an explicit "Known limitation" section: the State 1 chat input is rendered
  but disabled in the workspace-level meta-spec surface because EditorSplit
  requires a repo context and there is no workspace-scoped assist endpoint.
  Not in the acceptance-criteria list and blocked by the existing API
  surface — acceptable as recorded follow-up.
- **Publish baseline refresh.** `MetaSpecs.svelte` tracks
  `selectedMsBaseline` (set on load/select), passes it as `baselineContent`
  to the EditorSplit, and after a successful `publishPersona` PUT refreshes
  both the baseline and the cached registry entry so the next locked diff is
  computed against the post-publish prompt.
- **Coverage matrix** (`specs/coverage/system/ui-layout.md` row 37, SUMMARY)
  updated to `implemented` with landed evidence; the task doc's `commits:`
  frontmatter lists the three product commits.

## Findings

- [-] **F1 (major, carried over unresolved from round 1): the committed
  `web/dist/` bundle is byte-identical to the assigned base, so none of this
  task's production UI ships in the Rust-only deployment path.**
  `git diff --shortstat fbf1a63b e8782818 -- web/dist` is empty; the last
  dist-touching commit in the candidate's history is still `0674cb65c`
  ("build(web): regenerate verified assets (task-091)"). `git grep` for
  `locked-diff`, `pending_changes`, and `Pending changes` against `e8782818 --
  web/dist` all return no matches, while the source carries the §9 State 2
  pending-changes diff (`EditorSplit.svelte:810-812`, `en.json:964`,
  `lib/lineDiff.js`), the Iterate retention fix (`91fe1e86`), and the publish
  baseline refresh (`6e3bb1a`). The task's original State 2 commit `eee86b3e`
  *did* commit a regenerated dist (`web/dist/assets/index-BsanhgHD.js`), but
  the merges `5b0e5b491`/`671873eea` resolved `web/dist` back to base's
  task-091 bundle and no later commit regenerated it.
  `crates/gyre-server/src/spa.rs` RustEmbeds `#[folder = "../../web/dist"]`
  and `build.rs` skips the npm build under `SKIP_WEB_BUILD=1` — the documented
  Rust-only build (AGENTS.md quick start) — so that deployment serves a bundle
  with none of the §9 features this task's coverage row claims as
  "implemented" (including "State 2 locked editor showing the pending-changes
  diff"). This is the same defect class as task-208 R1-F1 and was round 1's
  finding 1 with an explicitly prescribed repair that was not performed:
  there is no `build(web)` commit in `fbf1a63b..e8782818`.

  **Repair path (mechanical):** `cd web && npm run build`, commit the
  regenerated `web/dist/` (e.g. `build(web): regenerate verified assets
  (task-176)`), append that commit's SHA to `specs/tasks/task-176.md`
  `commits:` frontmatter, and re-grep the dist — `git grep -c 'locked-diff'
  <candidate> -- web/dist` must return a match and
  `git diff --shortstat fbf1a63b <candidate> -- web/dist` must be non-empty.

Test-inflation check: none found. The §9 Iterate test fails under the
showResultTabs mutation (independently reproduced this round); the suites use
real API response shapes and the real poll loop, not mirrored logic.

Verifier note: no Rust server probes were required — this task is
frontend-only (plus docs). Frontend focused suites are the relevant gate and
pass at the source level; the blocking defect is purely the un-regenerated
shipped bundle.
