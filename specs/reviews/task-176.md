# Review: TASK-176 — Meta-specs Preview Loop Layout (§9)

**Reviewer:** Review
**Date:** 2026-11-09 (repair round — candidate includes review repair 6e3bb1a7)
**Comparison base:** fefde2946f3a0af1f212588f54591516fee0f7b2
**HEAD (candidate):** 0af01f76a0be8efa7e082befd7561d74d1b83d07
**Verdict:** approved

---

## Scope

Second review round of the §9 preview loop. The three-state workflow
(State 1 selector + Preview/Publish, State 2 locked editor + per-spec ◐/✓
progress + Cancel, State 3 Architecture/Code Diff tabs + Iterate + Publish)
was verified present in the merged tree. This round focused on the two
assigned commits: eee86b3e (State 2 pending-changes diff via
`web/src/lib/lineDiff.js` + `baselineContent` prop) and 6e3bb1a7 (review
repair: baseline refresh after in-session publish).

## Probes (evidence: /tmp/stage/review-evidence/task-176-review-evidence.md)

- **Focused suite:** `npm ci` + vitest on MetaSpecs/EditorSplit/lineDiff
  test files → **165/165 passed**, matching the shipped claim.
- **Mutation probe:** removed the 6e3bb1a7 baseline-refresh lines from
  `publish()` — both regression tests ("refreshes the diff baseline after an
  in-session publish", "diffs post-publish edits against the new baseline")
  **fail**; restored, they pass. The tests kill the real bug (stale
  pre-publish baseline rendering already-published lines as pending
  `+1 -0`/`+2 -0`); they are not self-confirming.
- **lineDiff correctness:** LCS with common prefix/suffix trim, bounded DP
  (4M cells) with block-diff degradation, trailing-newline normalization.
  Tests include round-trip reconstruction of both sides and the moved-lines
  case (LCS, not index-wise).
- **Merge integrity:** `git diff 564128a4f 0af01f76` shows every task-176
  source file preserved through the merge with base's task-089 work; related
  merged-tree suites (DetailPanel, ConcurrentEditBanner) pass 40/40.

## Dist observation (resolved, not a defect)

web/dist at the merge commit is byte-identical to base's (no `locked-diff`),
while both assigned commits shipped rebuilt bundles. Verified this is the
canonical controller artifact flow: `/tmp/stage/pipeline-artifacts.py` resets
web/dist to base at merge and rebuilds it from the merged source in a
downstream "build(web): regenerate verified assets" commit (same flow as
task-089 at base). Confirmed the candidate source rebuilds a correct bundle
(`npm run build` → locked-diff and "Pending changes" present; experiment
restored, tree clean).

## Bookkeeping

- Coverage row 37 → implemented with accurate notes; Depth 2 and header 2/38
  consistent with SUMMARY.md (42 rows − 4 n/a = 38, 2 implemented).
- Scope boundary truthful: backend preview gaps (no agent spawns, no
  throwaway-branch diffs, no DELETE cleanup) are tracked by task-206 against
  meta-spec-reconciliation.md row 9 ("Hollow"); the UI consumes the real
  PreviewResponse/PreviewStatusResponse shapes and does not fabricate State 3.
- Conventional commits respected.

## Findings

None. All §9 acceptance criteria are met by production code with
mutation-verified tests.
