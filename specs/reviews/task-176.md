# Review — task-176 (ui-layout §9 — Meta-specs Preview Loop: editor split states and preview workflow)

Spec: `specs/system/ui-layout.md` §9 (Meta-specs Preview Loop Layout), cross-referenced with `specs/system/meta-spec-reconciliation.md` §5.
Commit under review: `eee86b3e` (candidate `c4a8a8c6`, base `3c39ab1f`; docs commits `bcbda88a`/`109e8d35`/`c4a8a8c6` flip coverage row 37 and record shipped notes).
Verdict: **needs-revision** (verdict.json: approved=false).

## What was verified working (no findings)

- The commit's contribution — §9 State 2's "left panel shows diff of changes" — is real: `web/src/lib/lineDiff.js` is a correct LCS line diff (independently property-tested over 5000 random line pairs: rows reconstruct both inputs exactly at the line-array level; script retained at `/tmp/stage/review-evidence/linediff-property2.mjs`), and `EditorSplit.svelte` renders the draft-vs-baseline diff with `+`/`-` markers, green/red styling, and a `+N -M` summary while the preview runs, with verbatim-draft fallback when no baseline exists or the draft is unchanged.
- The new tests genuinely gate the behavior: mutating `lineDiff` to emit context-only rows fails 11 tests across all three files; forcing the verbatim fallback in `EditorSplit` fails 3 tests. Clean run: 163/163 passed (`npm ci` + vitest on lineDiff/EditorSplit/MetaSpecs test files), matching the implementer's claim.
- `web/dist` was rebuilt from this source (`locked-diff` markup present in `index-BsanhgHD.js`).
- The rest of the §9 loop (State 1 selector + Preview/Publish, State 2 lock + per-spec ◐/✓ progress + Cancel via button and Esc, State 3 Architecture/Code Diff tabs rendering the real PreviewResponse fields, Iterate with results retained, Publish → registry PUT) landed in task-172's rounds and is present in the base; spot-verified.
- Scope boundary is honest: the backend preview endpoint spawns no agents on throwaway branches, but that hollowness is pre-existing, disclosed, and tracked by task-206 against meta-spec-reconciliation.md row 9. The UI calls the real `POST/GET /workspaces/:id/meta-specs/preview` endpoints and consumes their real response shapes; a ui-layout task is not where that backend gap must close.
- No mid-preview content clobbering (all `content` mutations are user-driven; the textarea is removed while locked). No locale gaps (only `en.json` exists).

## Findings

- [-] **F1 (major): State 2 diff baseline goes stale after publish — the locked pane fabricates pending changes.** `MetaSpecs.svelte publish()` (lines 392-403) PUTs the draft as the new published registry prompt but never updates `selectedMsBaseline` (nor the `wsMetaSpecs` entry it derives from). The §9 loop is edit → preview → iterate → publish, then repeats; previewing after a publish is a first-class flow, and there the locked pane diffs against the *pre*-publish prompt. Reproduced with a temporary vitest probe (removed after the run; output in `/tmp/stage/review-evidence/task-176-review-evidence.md` Probe 5): edit persona-1's draft to add "Handle all errors with Result<T, E>.", click Publish (mock resolves), click Preview with no further edits → `locked-diff-summary` shows "+1 -0" and the already-published line carries the `+` pending marker, when nothing is pending. The shipped-notes claim "the locked diff is always against the real published version" is false after an in-session publish. Fix: on successful publish, set `selectedMsBaseline = selectedMsContent` and update the `wsMetaSpecs` entry, so the baseline tracks the registry.
- [-] **F2 (minor, docs): coverage row 37's Depth column corrupted from 2 to 3.** `## 9. Meta-specs Preview Loop Layout` is an H2 heading (`scripts/bootstrap-coverage.sh` derives depth 2), and every sibling top-level `§N` row (4, 28, 32, 36, 38) has depth 2. The "3" appears to be "3 states" written into the wrong column.
- [-] **F3 (minor, docs): stale per-file coverage header.** `specs/coverage/system/ui-layout.md` still reads `**Coverage:** 1/38` after flipping a second row to implemented; it should be 2/38 (`scripts/update-coverage-summary.sh` regenerates only SUMMARY.md, so this header is hand-maintained).

## Probes run

All under `/tmp/stage/review-evidence/task-176-test-evidence.md` (implementer) and `task-176-review-evidence.md` (this review): clean focused vitest run (163/163), lineDiff property test (pass), two mutation probes (both killed), stale-baseline reproduction (fails as described), dist provenance grep, §9 feature spot-checks. Working tree left pristine (`git status --porcelain` empty; temp probe file removed).

## Remedy

Small revision to `eee86b3e`'s surface: refresh the baseline on successful publish in `MetaSpecs.svelte` (one assignment plus the registry-entry update, ideally with a regression test covering publish → preview with no edits → no locked diff), restore row 37's Depth to 2, and update the `Coverage:` header to 2/38.
