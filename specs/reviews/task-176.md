# Review — task-176 (Meta-specs Preview Loop layout — editor split states and preview workflow)

Spec: `specs/system/ui-layout.md` §9 "Meta-specs Preview Loop Layout".
Assigned base `fbf1a63b`, candidate `e2fa203f` (round 3).
Product commits: `eee86b3e` (State 2 locked-editor pending-changes diff), `6e3bb1a7` (publish baseline refresh), `91fe1e86` (Iterate results retention), `f51cc2296` (§9 State 1 LLM chat wired to workspace-scoped assist endpoint).

Verdict: **needs-revision** — one material finding (F1, stale shipped bundle, third consecutive round); all other acceptance criteria verified clean.

## Round 3 probes (evidence in `/tmp/stage/review-evidence/`)

- `git diff --shortstat fbf1a63b e2fa203f -- web/dist` → empty (dist byte-identical to assigned base); `git grep -c 'locked-diff'|'pending_changes' e2fa203f -- web/dist` → 0 matches (`task-176-dist-staleness-r3.log`).
- `cargo check -p gyre-server` → Finished, 0 errors (`task-176-cargo-check.log`).
- Mechanical invariants: `check-abac-route-registry.sh`, `check-forged-scope-fields.sh`, `check-arch.sh`, `check-inert-enforcement.sh`, `check-lossy-secret-conversion.sh`, `check-unbounded-external-http.sh` → all OK (`task-176-mechanical-checks.log`).
- `cd web && npm ci` then `npx vitest run EditorSplit.test.js MetaSpecs.test.js lineDiff.test.js` → **169/169 passed** (`task-176-frontend-tests.log`).
- Mutation probe: removing the `|| Boolean(metaPreviewResult)` retention clause from `showResultTabs` makes `meta-spec Iterate returns to editing with the selector AND the last preview results still visible (§9)` **fail** — the test genuinely guards the §9 Iterate retention behavior (`task-176-mutation-iterate-v2.log`; first attempt `task-176-mutation-iterate.log` had a non-applying sed and was discarded). File restored; `git status` clean afterwards.
- build.rs side-effect probe: `cargo check -p gyre-server` regenerated `web/dist` from the candidate's `web/src`; the fresh bundle contains `locked-diff`/`metaSpecsAssist`/`pending_changes` (2 matches each) while the committed candidate bundle contains **none** — direct proof the shipped artifact lacks this task's UI (`task-176-dist-rebuild-proof.log`). Working tree restored to committed state afterwards.

## Verified working (no findings)

- **State machine + Iterate retention (round-1 F2/F3) fixed and tested.** `showResultTabs` (`EditorSplit.svelte:249`) keeps the Architecture/Code Diff tabs rendered in the meta-spec editing state when `metaPreviewResult` is retained; the selector stacks above them (`.spec-selector.stacked`). The meta-spec-context test (`EditorSplit.test.js:645`) drives a real preview to complete, clicks Iterate, and asserts selector + `tab-architecture` + `meta-impact` + `tab-code-diff` statuses all remain — and the mutation probe confirms it fails when the behavior is removed.
- **State 2 locked diff is real.** `lineDiff.js` is a genuine LCS line diff (bounded DP with block-diff degradation, round-trip property test); MetaSpecs passes `baselineContent` (the published registry prompt) and the locked pane renders +/- markers with an add/remove summary; baseline is refreshed after in-session publish (`6e3bb1a7`), covered by tests.
- **State 1 chat (round-1 F4 limitation) now wired for real.** New route `POST /api/v1/workspaces/:id/meta-specs/assist` (`specs_assist.rs::assist_meta_spec`) with ABAC mapping (`meta_spec`/`generate`, consistent with the sibling generate endpoints `briefing/ask`, `explorer-views/generate`, `repos/:id/specs/assist`), real streaming LLM call, workspace-scoped rate limit (10/60 s), cost/budget accounting, cross-workspace meta-spec → 403, unknown workspace/meta-spec → 404, no draft and no id → 400, LLM unconfigured → 503. Frontend `llmChatEnabled` enables the chat in the meta-spec context with a workspace; `api.metaSpecsAssist` replaces the honest-404 `specsAssistGlobal` stub. Tests include a recording LLM factory asserting the stored registry prompt vs draft precedence from the actual system prompt — not just status codes.
- **Task attribution** at the candidate records all four product commits in `task-176.md` `commits:`; coverage row 37 → `implemented` backed by the shipped code.
- Observation (non-blocking): in a default deployment the middleware `generate` action is Admin-only (the Developer allow-list has read/write/spawn/complete/approve/evaluate; `developer-generate-access` is defined in `gyre-domain` but not startup-seeded). This is the pre-existing posture of *all* LLM-generate endpoints, not something this task introduced; the new route matches its siblings exactly.

## Findings

### F1 (code, `web/dist/`) — stale shipped bundle, third consecutive round

`git diff --shortstat fbf1a63b e2fa203f -- web/dist` is **empty**: the committed bundle at the candidate is byte-identical to the assigned base's (last dist-touching product commit is still task-091's `0674cb65c`). The committed bundle contains none of this task's UI (`git grep` for `locked-diff`, `pending_changes`, `metaSpecsAssist` against `e2fa203f -- web/dist` → 0 matches), while the task's source carries all of it (`EditorSplit.svelte`, `lineDiff.js`, `api.js`, `MetaSpecs.svelte`). The merges `5b0e5b491`/`671873eea` resolved `web/dist` back to base's bundle, dropping `eee86b3e6`'s regenerated dist. `gyre-server` RustEmbeds `web/dist` (`spa.rs`) and `SKIP_WEB_BUILD=1` builds serve it, so the documented Rust-only deployment renders none of the §9 preview loop. Direct proof: a build.rs regeneration from the candidate's own `web/src` produces `locked-diff`/`metaSpecsAssist`/`pending_changes` in the fresh bundle and different asset hashes (`task-176-dist-rebuild-proof.log`). Same defect class as task-208 R1-F1 and this task's rounds 1 and 2; the prescribed repair was again not performed — there is no `build(web)` commit in `fbf1a63b..e2fa203f`. Repair: `cd web && npm run build`, commit as `build(web): regenerate verified assets (task-176)`, append that SHA to `task-176.md` `commits:`, re-grep the dist for `locked-diff`/`metaSpecsAssist`.

## Verifier instructions (run after model exit)

- `cargo test -p gyre-server specs_assist` — covers the seven new `assist_meta_spec_*` tests (404/400/403 scope/success SSE/draft-precedence/503) plus the existing assist suite; only `cargo check` was run in-review (4 GiB budget).
- `cd web && npm ci && npx vitest run` — full frontend suite (focused run of the three task files passed 169/169 in-review).
