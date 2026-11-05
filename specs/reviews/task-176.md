# Review — task-176 (Meta-specs Preview Loop layout — editor split states and preview workflow)

Spec: `specs/system/ui-layout.md` §9 "Meta-specs Preview Loop Layout" (lines 725-780), building on the Editor Split (§2) delivered by task-172.
Commit under review: candidate `564128a4` (base `2f215b1e`; task commits `eee86b3e` + repair `6e3bb1a7`).
Verdict: **approved** (round on the post-repair candidate; the prior round's stale-baseline finding is fixed and regression-covered).

## Probes (evidence: /tmp/stage/review-evidence/task-176-review-evidence.md)

- `npm ci` then `vitest run` MetaSpecs.test.js + EditorSplit.test.js + lineDiff.test.js → **165/165 passed**; DetailPanel.test.js (the other EditorSplit consumer) → **30/30 passed**.
- **Regression-test lethality**: temp worktree at `6e3bb1a7` with only the fix reverted (MetaSpecs.svelte from `eee86b3e`) → both new baseline tests fail with exactly the documented symptoms (phantom `+1 -0` pending line after publish; `+2 -0` for a single post-publish edit). The tests kill the real bug; they are not self-confirming.
- Scope check: diff touches only web/ (src, tests, dist), specs/coverage, and the task file. No production Rust, scripts, or unrelated tasks modified.

## Verified working (no findings)

- **State 2 "shows diff of changes"** (the task's increment): the locked left pane renders an LCS line diff of the draft vs the published registry prompt (`selectedMsBaseline`, set on load, on persona switch, and — the repair — after an in-session publish). `+`/`-` markers, add/remove summary, verbatim fallback when no baseline exists or the draft is unchanged (each path tested). `lineDiff.js` is a real bounded LCS (prefix/suffix trim, Int32Array DP, block-diff degradation above 4M cells) with round-trip property tests reconstructing both inputs exactly.
- **Baseline refresh after publish** (prior round's finding): `publish()` updates `selectedMsBaseline` and the cached `wsMetaSpecs` entry on success, so the publish → iterate → preview loop never diffs against a stale pre-publish baseline. Both regression tests verified to fail on the pre-fix component and pass on the candidate.
- **Rest of §9** (present in the base via task-172, re-confirmed on the candidate): EditorSplit mounted in `context="meta-spec"` by the MetaSpecs workspace view; State 1 target-spec checkbox selector + Preview/Publish; per-spec ◐/✓ progress from the real `previewPersonaStatus` poll; Cancel Preview (button + Esc); State 3 Architecture (default)/Code Diff tabs rendering the real `PreviewResponse` fields; Iterate returns to editing with results retained; Publish → `PUT /meta-specs-registry/{id}` which server-side resets `approval_status` to `Pending` (meta_specs.rs:1075-1076).
- **No fabricated State 3**: `pollPreviewPersona` completes only on the server's `state === 'complete'`; poll errors and failure states fall back to editing. The test helper `realPreviewShape` matches the server's `PreviewResponse`/`PreviewStatusResponse` field-for-field.
- **Coverage bookkeeping**: ui-layout.md row 37 → implemented, Depth 2 (§9 is an H2), header 2/38 consistent with the row statuses (2 implemented, 4 n/a, 36 assigned); SUMMARY.md row and TOTAL deltas match. No `not-started` rows remain for ui-layout.md.
- **Dist freshness**: no web/src changes after `6e3bb1a7`; the committed bundle contains the new locked-diff UI and locale string.

## Notes (non-blocking)

- §9's State 3 wireframe shows `+`-marked pending lines in the left panel; the implementation unlocks the editor with the full draft (changes included) plus LLM suggestion blocks, without diff markers in State 3. The spec text attaches "shows diff of changes" to State 2 (locked), which is implemented; reading State 3's markers as illustrative is defensible and consistent with the prior rounds' acceptance.
- Backend preview semantics (agents actually spawned on `preview/{preview_id}/*` throwaway branches, line-level code diffs, DELETE cleanup) remain hollow and are tracked by task-206 against meta-spec-reconciliation.md row 9 — a legitimate scope boundary for a layout-spec task; the UI consumes exactly what the real endpoints return today.
