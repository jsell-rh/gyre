# Review — task-173 (Interaction Patterns — scope transitions, drill-down, inline expansion)

Spec: `specs/system/ui-layout.md` §3 (Interaction Patterns: Scope Transitions, Drill-Down, Inline Expansion, Contextual Chat).
Candidate: `dd14d3de` (base `7c6ac232`). Verdict: **approve**.

## Evidence (all under `/tmp/stage/review-evidence/task-173/`)

Focused suite on the exact candidate (after `npm ci`, `--maxWorkers=2`): 11 files, **271 tests passed**, exit 0 (`vitest-review-focused.log`). Baseline re-run of Briefing 28/28 pass. `vite build` clean from candidate source (`vite-build.log`); `web/dist` is not part of the candidate diff (0 diff lines vs base). `bash scripts/check-task-commit-attribution.sh` → OK. Working tree verified clean after every probe (`git status`/`git diff HEAD` empty besides `node_modules`).

Mutation probes (each broke a production behavior, ran the corresponding suite, restored source byte-identical; logs in `MUTATIONS.md`):

1. Briefing `toggleSection` never collapses → Briefing.test.js FAIL (collapse + single-expansion caught).
2. `onScopeDrill` branch removed from ExplorerCanvas.onDblClick → ExplorerCanvas.test.js FAIL ("double-click on a repo_id leaf calls onScopeDrill").
3. `:global()` scoping regressed in Briefing → AccordionScoping.test.js FAIL (compile-time guard kills the dead-CSS defect class).
4. Inbox `toggleExpand` broken → Inbox.test.js 15 failures.
5. `fadeContent()` no-op → ScopeTransitions.test.js FAIL (150ms fade window test).
6. ExplorerView `selectRepo` drops `?repo=` pushState → ExplorerViewDrillUrl.test.js 3/4 FAIL.

## Findings: none blocking

- **Scope Transitions** — `App.svelte` `fadeContent()` (150ms timer, `.content-inner.faded` opacity, `--transition-fast: 150ms ease` in design-system.css) + `pushState` on every scope change path (`goToWorkspaceHome`, `goToRepo`, `goToRepoTab`, settings/rules/profile/cross-workspace). No `location.href` assignment on these paths; App-level test asserts same content-root node survival, pushState spy, breadcrumb, fade window (fake timers: faded at +100ms, visible at +160ms).
- **Drill-Down** — `DetailPanel.svelte` `transition: width 200ms ease-out` (candidate fixed the base's `--transition-normal` token to a literal 200ms — token value is also 200ms, but the literal removes ambiguity), `.detail-panel.open` = 40% width with flex main → effective 60/40 compression; Esc/✕/replace semantics covered by the 38 pre-existing DetailPanel tests. Double-click drill: `ExplorerCanvas.onDblClick` routes `repo_id` leaf nodes (no Contains children, callback gated) to `onScopeDrill`; `WorkspaceHome` wires it to `onSelectRepo(repo, 'architecture')` → `App.goToRepo` (scope change + breadcrumb + pushState). I verified the real-data path: `GET /workspaces/{id}/graph` (graph.rs:774) aggregates per-repo nodes whose `repo_id` field is populated (`GraphNodeResponse.repo_id: String`), so the drill fires on genuine server data, not just test fixtures; unknown repo_id → toast, no navigation (tested). `ExplorerView.selectRepo` pushes/pops `?repo=` with mount-restore and popstate sync (4 tests, mutation-killed).
- **Inline Expansion** — `AccordionItem.svelte` is a real reusable controlled component (header `<button>` with `aria-expanded`/`aria-controls`/`aria-labelledby`, actions snippet as a sibling row respecting the HTML content model, body only when open). Inbox (`expandedId`) and Briefing (`expandedSection`) both enforce single-expansion; both mutation-killed. Entity refs in expanded items open the detail panel (Inbox: spec/agent/MR; Briefing: spec/agent/mr — all tested through the real context). The previously-found dead-CSS defect class (`:global()` needed because AccordionItem authors the header button) is now guarded at compile time by `AccordionScoping.test.js`, which fails on regression (mutation 3).
- **Contextual Chat** — pre-existing `InlineChat.svelte` with recipient indicator (`Message to X ▸` / `Ask about X ▸` / `Edit spec: "…" ▸` via i18n `locales/en.json` `inline_chat.*`) and per-type capability hints; integrated at detail-panel bottom (agent, MR author), Briefing Q&A (including task-196's `onassistant` history), MetaSpecs spec-edit, WorkspaceHome agent messaging. Untouched by this candidate (diff clean); covered by InlineChat (16) + DetailPanelChat tests. The task correctly treats this as alignment of the §3 pattern family rather than re-implementing existing infrastructure.
- Full-page entity routing (`primaryTypes` → `goToEntityDetail`) predates this branch (introduced in `1056059c`, an ancestor of base) and `ui-navigation.md` (the newer shell spec that supersedes ui-layout §1) describes the spec-detail-panel flow consistently with the implementation; not a regression of this candidate.
- Hygiene: commit attribution passes the mechanical check; the "unlisted" checkpoint commits are either process/spec-doc-only (0 product files) or dist-only (`b4d8ce72` touches only `web/dist`, which the attribution script and the final diff both exclude); the temporary `zz-probe.test.js` scratch file was added and removed within the branch and is absent from the candidate.

## Sandbox limitation

TCP listener probe unsupported (capabilities.json, errno 95) — no live browser/server smoke run. jsdom component tests + compile-time CSS guard + mutation probes are the verification surface; CI browser checks remain for the verifier.
