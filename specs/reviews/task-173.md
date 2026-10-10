# Review — task-173 (Interaction Patterns — scope transitions, drill-down, inline expansion)

Spec: `specs/system/ui-layout.md` §3 (Interaction Patterns: Scope Transitions, Drill-Down, Inline Expansion, Contextual Chat). Task: `specs/tasks/task-173.md`.
Candidate: `64a31664` (base `e77537fa`). Verdict: **approved**.

## Round 1

Sandbox: TCP listener probe unsupported (capabilities.json errno 95) — no browser/server smoke test; the verification surface is jsdom component tests, compile-time CSS guards, and `vite build`. Evidence: `/tmp/stage/review-evidence/task-173-review2/`.

Test runs (`npm ci` first; working tree verified byte-identical to candidate before and after every probe):

- Full interaction-pattern suite (11 files: AccordionItem, AccordionScoping, Inbox, Briefing, InlineChat, ScopeTransitions, WorkspaceHomeScopeDrill, ExplorerViewDrillUrl, ExplorerCanvas, DetailPanel, DetailPanelChat): **11 files, 271 tests passed**, exit 0.
- `npx vite build` at candidate: clean (exit 0). Artifacts removed; `web/dist` verified unchanged from base (`git diff --stat base..candidate -- web/dist` empty).
- `git diff --check e77537fa 64a31664`: exit 0 — the trailing-blank-line-at-EOF failure that killed the prior verification round (finding `e4d8d7e0`) is fixed by `6bdd55fd`.
- Attribution: all task-labeled product-surface commits in range (`6bdd55fd`, `1a036edc`, `43f67b86`, `94de2edc`, `2c5641ae`, `add218c8`) are in the task's `commits:` frontmatter (which also lists the recovered checkpoints `3b28d848`, `99e99039`, `e25dd524`). The transient `zz-probe.test.js` debug file (added in `94de2edc`) is deleted at the final head.

Mutation probes (production behavior disabled, focused test file run, source restored and diff-verified identical):

1. Inbox `toggleExpand` → `expandedId = id`: collapse test fails. Multi-expansion variant (`expandedId ?? id`): collapse test fails; switch-item guard variant survives Inbox tests alone.
2. Briefing `toggleSection` multi-expansion or switch-blocked variants: **2 tests fail each** (collapse + single-expansion) — the Briefing accordion constraint is fully covered.
3. AccordionItem group host switch-blocked guard: group single-expansion test fails (asserts the aria-expanded flip from item one to item two), so the "click another item → current collapses" spec point is covered at component level.
4. InlineChat agent recipient label removed: 5 tests fail across InlineChat/DetailPanelChat/Briefing. llm-qa recipient label removed: 2 fail.
5. ExplorerCanvas scope-drill branch disabled (`if (false)`): drill test fails (dblclick → `onScopeDrill`, not `onNodeDetail`/`drillInto`).
6. WorkspaceHome `onScopeDrill` rewired to open the panel instead of scope change: 2 tests fail (drill→`onSelectRepo(repo,'architecture')`; unknown-repo → toast, no navigation).
7. App `fadeContent()` no-op: fake-timer fade-window test fails. Fade duration 150ms→300ms: same test fails (duration is behaviorally asserted).
8. Inbox agent+MR ref-link onclicks removed: 2 entity-ref tests fail.

Verified per acceptance criterion:

- **Scope transitions** — `fadeContent()` (150ms, `--transition-fast`) + `pushState` on every scope change (`goToWorkspaceHome`/`goToRepo`/`goToRepoTab`/`goToEntityDetail`/settings/rules/profile/cross-workspace); no `location.href` assignment on these paths; breadcrumb from the same reactive state. ScopeTransitions.test.js renders the real App with the real WorkspaceHome, spies `pushState` (delegating to the real implementation), asserts URL change, breadcrumb content, workspace-segment persistence, fade-window timing under fake timers, and content-root survival (no reload).
- **Drill-down** — DetailPanel `transition: width 200ms ease-out` (changed from `var(--transition-normal)`; same 200ms value, now literal per spec), `.detail-panel.open` = 40% (main compresses to 60%). Panel open/close/Esc/✕ covered by the 38 pre-existing DetailPanel tests. Double-click drill: ExplorerCanvas routes repo_id leaf nodes (no Contains children) to `onScopeDrill`; WorkspaceHome wires it to `onSelectRepo(repo,'architecture')` → `App.goToRepo` (scope change + breadcrumb + pushState, no reload); nodes with Contains children keep in-graph drill; ExplorerView `selectRepo` pushes `?repo=` via pushState with restore-on-mount and popstate sync (ExplorerViewDrillUrl.test.js: deep-link, Back, popstate).
- **Inline expansion** — reusable AccordionItem (controlled, header is a real `<button>` with `aria-expanded`/`aria-controls`/`aria-labelledby`, actions snippet as sibling — no interactive descendants of the header). Inbox cards (`expandedId`) and Briefing sections (`expandedSection`, completed default-expanded) enforce single expansion; entity refs inside expanded items open the detail panel (Inbox: spec/agent/MR refs; Briefing: spec/agent/mr refs). The prior dead-CSS defect class (parent-scoped rules never matching the child-authored button) is guarded by AccordionScoping.test.js, which compiles Inbox/Briefing/AccordionItem and asserts the `:global`-wrapped rules are live and un-pruned.
- **Contextual chat** — InlineChat with `Message to X ▸` / `Ask about X ▸` / `Edit spec: "…" ▸` (i18n), capability hints per recipient type (agent signed/persisted, llm-qa read-only, spec-edit drafts). Integrated at the agent detail panel chat tab (signed Directed-tier steering), MR chat tab (author agent), Briefing Q&A (SSE-streamed, `answer ?? text ?? streamBuffer`), MetaSpecs editing, WorkspaceHome orchestrator messaging.

Minor (non-blocking, recorded for completeness):

- The Inbox single-expansion test (`expand one collapses another`) asserts only the `.card-body` count, so a hypothetical switch-blocking mutation survives that one test in isolation; the constraint's switch path is separately covered by the Briefing tests (both fail under mutation) and the AccordionItem group-host test. Production code implements the constraint correctly; no defect.
- The 200ms ease-out panel slide and the 60% compression are CSS-only properties jsdom cannot assert; the value is source-verified (`DetailPanel.svelte` `.detail-panel`/`.detail-panel.open`), and the 150ms cross-fade (the other timing in the same section) is behaviorally asserted under fake timers. A real-browser visual check remains for host verification given the sandbox's listener restriction.
