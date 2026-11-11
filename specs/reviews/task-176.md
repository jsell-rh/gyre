# Review — task-176 (Meta-specs Preview Loop layout — editor split states and preview workflow)

Spec: `specs/system/ui-layout.md` §9 "Meta-specs Preview Loop Layout".
Candidate under review: `5b1d8540` (base `22ca8fd8`). Round 4.
Verdict: **needs-revision** — one material finding, unchanged for the fourth consecutive round: the shipped `web/dist` bundle is stale and contains none of this task's §9 UI. All source-level work is verified clean.

## Round 4 probes

- `cd web && npm ci` → OK (locked versions).
- `cd web && npx vitest run src/__tests__/EditorSplit.test.js src/__tests__/MetaSpecs.test.js` → **156/156 passed** (evidence: `task-176-focused-tests.log`).
- `cargo check -p gyre-server` → **Finished `dev` profile ... in 7m 28s, exit 0** (evidence: `task-176-cargo-check.log`) — confirms the merge of base `22ca8fd8` (task-175's notification fan-out in `specs_assist.rs`, +405 lines) did not break the task-176 server code.
- `bash scripts/check-abac-route-registry.sh` → OK; `bash scripts/check-arch.sh` → OK.
- Route `/api/v1/workspaces/:id/meta-specs/assist` registered (`api/mod.rs:444`) with ABAC `RouteResourceMapping` present (`abac_middleware.rs:461`); `assist_meta_spec` handler intact post-merge.
- Dist staleness probe (evidence: `task-176-dist-staleness.log`, `task-176-dist-rebuild.log`): `git diff --shortstat 22ca8fd8 5b1d8540 -- web/dist` is empty; `git grep` for `locked-diff`/`metaSpecsAssist`/`pending_changes`/`Pending changes` at the candidate over `web/dist` → 0 matches; a fresh `npm run build` from the candidate's own `web/src` produces `index-C1lqdB9v.js` containing all of them.

Verified working (no findings):

- **All §9 acceptance criteria are met in source.** EditorSplit in `context="meta-spec"` drives the three states: State 1 target-spec checkbox selector in the right pane + Preview/Publish; State 2 locked editor with the pending-changes LCS diff (`locked-diff`, 18 refs in `EditorSplit.svelte`) and per-spec ◐/✓ progress with Cancel Preview; State 3 Architecture (default)/Code Diff tabs. Iterate retention (`showResultTabs`, 91fe1e86) and publish-baseline refresh (6e3bb1a) are present and unchanged since round 3 — the round-2/3 merge did not regress `EditorSplit.svelte` or `MetaSpecs.svelte` (diff `e2fa203f..5b1d8540` touches neither).
- **Round-1 minor finding (State 1 chat disabled on the workspace §9 surface) is fixed in source**: workspace-scoped `POST /api/v1/workspaces/:id/meta-specs/assist` (f51cc2296) with real 404/403/503/rate-limit behavior and SSE `{diff, explanation}` protocol, wired to `metaSpecsAssist` in `api.js`; 156 focused tests including the assist-meta-spec integration tests pass.
- Focused §9 tests are real guards (render real `PreviewResponse` shapes, assert DOM presence of tabs/indicators/diff and their removal on cancel), not mirrored-logic.

Findings:

- **F1 (repeat, 4th round) — stale shipped bundle.** The committed `web/dist` at `5b1d8540` is byte-identical to the assigned base's task-175 bundle (`git diff --shortstat` empty; last dist-touching commit is `5454c2d8e build(web): regenerate verified assets (task-175)`). It contains zero §9 strings. The task's original State 2 commit `eee86b3e6` did commit a regenerated dist (`index-BsanhgHD.js`, `locked-diff` ×2), but every merge (5b0e5b491, 671873eea, and now 4361d5501) resolved `web/dist` back to the base side, dropping it. `gyre-server` RustEmbeds `web/dist` (`crates/gyre-server/src/spa.rs`) and `SKIP_WEB_BUILD=1` builds serve the committed bundle, so the documented Rust-only deployment renders none of the §9 preview-loop UI this task claims. Repair, exactly as prescribed in rounds 1–3: `cd web && npm run build`, commit as `build(web): regenerate verified assets (task-176)`, append that SHA to `specs/tasks/task-176.md` `commits:` frontmatter, and re-grep the dist for `locked-diff`/`metaSpecsAssist`.

Server tests the verifier should run (after the dist regen repair): `cargo test -p gyre-server assist_meta_spec` and the full `cargo test -p gyre-server specs_assist`; `cd web && npm test`.
