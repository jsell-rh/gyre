# Review — task-176 (Meta-specs Preview Loop layout — editor split states and preview workflow)

Spec: `specs/system/ui-layout.md` §9 "Meta-specs Preview Loop Layout".
Candidate under review: `403d6af5` (base `cfeefbda`; product commits `eee86b3e` State 2 locked-editor pending-changes diff, `6e3bb1a7` post-publish baseline refresh; remainder docs/process/merge commits).
Verdict: **needs-revision** — two material findings (F1 stale shipped bundle, F2 Iterate hides preview results in the §9 context); the candidate's own delta is otherwise real, well-tested work.

## Round 1

Probes run (checkout at `403d6af5`; the unstaged `specs/tasks/task-176.md` revert is controller stage-prep, no test reads it):

- `cd web && npm ci` → clean; `./node_modules/.bin/vitest run src/__tests__/MetaSpecs.test.js src/__tests__/EditorSplit.test.js src/__tests__/lineDiff.test.js` → **165/165 passed** (evidence 01).
- Mutation probes (both restored afterwards; `git status` clean apart from the stage-prep file):
  - removed the post-publish baseline refresh from `MetaSpecs.svelte publish()` → **2 tests failed** (evidence 02);
  - removed the `baselineContent={selectedMsBaseline}` prop pass → **2 tests failed** (evidence 03).
  The new tests are real guards, not self-confirming.
- Temporary vitest probe of §9 Iterate semantics in the meta-spec context with real `PreviewResponse`/`PreviewStatusResponse` shapes (evidence 04) — see F2.
- Dist freshness: `git diff --stat cfeefbda 403d6af5 -- web/dist` → **empty**; `git grep 'locked-diff\|pending_changes' 403d6af5 -- web/dist` → **no matches** (evidence 05).
- `scripts/update-coverage-summary.sh` in a detached worktree at `403d6af5` → zero diff vs committed `specs/coverage/SUMMARY.md` (merge-conflict resolution was mechanical and consistent).
- Commit attribution at the committed candidate: both product commits (`eee86b3e`, `6e3bb1a7`) listed in `task-176.md` `commits:` frontmatter. (The attribution script fails only against the working tree because of the controller's stage-prep revert.)
- Server claims verified in source: preview endpoints exist with Admin/Developer RBAC and record-scope checks (`meta_specs.rs`); request shape matches the UI; registry PUT resets approval to `Pending` on content edit (`meta_specs.rs:1597-1598`).

Verified working (no findings):

- **The candidate's own delta is genuine.** `lineDiff.js` is a correct bounded LCS line diff (prefix/suffix trim, 4M-cell cap with honest block-diff degradation, trailing-newline and null handling); `lineDiff.test.js` includes round-trip reconstruction and LCS-minimality properties that a naive index diff would fail. The locked-editor diff renders `+`/`-` markers, per-op classes and an add/remove summary; the no-baseline and unchanged-draft cases fall back to the verbatim draft. The post-publish baseline refresh (including the cached registry entry) is a real bug fix with real regression tests.
- **State machine wiring is honest.** Preview calls the real `POST/GET /workspaces/:id/meta-specs/preview`; a failed/unavailable/timed-out poll returns to editing with an error banner and never fabricates State 3; per-spec ◐/✓ progress and the Progress m/n summary render server data; Cancel Preview works via button and Esc; Publish calls the registry PUT (approval reset verified server-side); Iterate returns to an editable textarea.
- **Scope boundary accepted:** the preview endpoint spawns no agents (marks specs complete immediately — `meta_specs.rs` comment admits it). That backend hollowness is pre-existing at base, tracked by task-206 against `meta-spec-reconciliation.md` coverage row 9 ("Hollow"), and task-176's instructions say to use the existing preview API. The UI consumes exactly the real response shapes and never fakes a completed preview. Not counted against this candidate.

Findings:

- [F1] **Major (artifact): the committed `web/dist/` is byte-identical to the base's — the task's UI is absent from the shipped bundle.** `eee86b3e` did commit a regenerated dist, but the rebase-repair merge `5b0e5b491` dropped it in favor of base's task-091 bundle, so `grep 'locked-diff\|pending_changes' web/dist` finds nothing while `web/src` carries the feature. `gyre-server` RustEmbeds `web/dist` and `SKIP_WEB_BUILD=1` builds serve it, so the documented Rust-only deployment renders no §9 State 2 diff. Same class as task-208 R1-F1. **Repair:** `cd web && npm run build`, commit as `build(web): regenerate verified assets (task-176)`, append the SHA to `task-176.md` `commits:`, re-grep the dist.
- [F2] **Major (spec deviation): Iterate removes the preview results from view in the meta-spec context.** §9: "Iterate: returns to State 1 with the preview results still visible"; acceptance criterion: "[Iterate] returns to editing with results visible." Empirically (evidence 04), after Iterate the right pane renders only the target-spec selector branch (`context === 'meta-spec' && previewState === 'editing'`); `meta-impact`, the Architecture tab and the Code Diff tab are all removed from the DOM. The results are retained in component state but rendered nowhere. **Repair:** render the retained results (e.g. the Architecture/Code Diff tabs or a collapsed impact summary) alongside the selector in the meta-spec editing state after a completed preview, and cover it with a meta-spec-context test.
- [F3] **Minor (test gap):** the only iterate-results test ("iterating returns to the editing state with results still visible", `EditorSplit.test.js`) exercises the spec/repo context, where the tabs legitimately remain; the §9 meta-spec context — where F2 fails — is uncovered. Add the meta-spec-context case from the probe as a permanent test.
- [F4] **Minor (known limitation):** the production §9 surface never enables the State 1 LLM chat — `App.svelte` mounts the workspace meta-specs page without `repoId`, EditorSplit disables chat without one, and no workspace-scoped assist endpoint exists (`specsAssistGlobal` 404s by design). Implementation-plan item 4 is unreachable in the §9 surface. Blocked by the existing repo-scoped API surface; record as a limitation or address in a follow-up.

Test-inflation check: none in the candidate's delta — mutation probes kill the new tests, and the lineDiff tests assert exact row structures and round-trip properties. The pre-existing suite was not weakened.

Verifier should run (after the model exits): full `cd web && npm test`; `cargo check -p gyre-server`; `scripts/check-task-commit-attribution.sh` against the committed candidate tree; after F1's repair, `grep -r 'locked-diff' web/dist/assets` must match.
