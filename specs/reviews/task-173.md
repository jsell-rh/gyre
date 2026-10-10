# Review — task-173 (Interaction Patterns — scope transitions, drill-down, inline expansion)

Spec: `specs/system/ui-layout.md` §3 (Scope Transitions, Drill-Down, Inline Expansion, Contextual Chat).
Candidate: `9d5fcc13` on base `770785f7`. Verdict: **complete (approved)** — independent evidence below; all negative controls restored and `git status` verified clean after each.

## Evidence (this checkout, `npm ci` first, vitest)

- `npx vitest run src/__tests__/{AccordionItem,AccordionScoping,Inbox,Briefing,ExplorerCanvas}.test.js --maxWorkers=2` → **5 files, 208 passed** (`vitest-accordion-canvas.log`).
- `npx vitest run src/__tests__/{ScopeTransitions,WorkspaceHomeScopeDrill,ExplorerViewDrillUrl,InlineChat,DetailPanel,DetailPanelChat}.test.js --maxWorkers=2` → **6 files, 63 passed** (`vitest-scope-drill-chat.log`).
- Neighbor files sharing the touched components (`ExplorerViewScope`, `WorkspaceHome`, `WorkspaceHomeArchitecture`, `NoSidebar`) → **30 passed** (`vitest-neighbors.log`).
- Full suite `npx vitest run --maxWorkers=2` → 62/63 files, 1558 passed / 41 skipped / 10 failed — **all 10 failures in `ExplorerCanvas-performance.test.js` timeouts (10k-node graphs, 5s test timeout)**. Not a candidate regression: the same file fails at base `770785f7` in a separate worktree with identical node_modules (base: 3 failed incl. two of the same tests, candidate solo run: 1 failed — machine-load-sensitive timing, 100ms/5s budget on a shared sandbox). The candidate's ExplorerCanvas diff is 38 lines confined to `onDblClick` (a branch after layout/hit-test), with no path affecting 10k-node layout cost. Log: `vitest-full-suite.log`, `vitest-perf-base.log`, `vitest-perf-candidate.log`.
- `npx vite build` → **EXIT 0** (`vite-build.log`); `web/dist` restored to committed state afterwards (`git status` clean).

## Negative controls (each: patch → fail → restore → diff-verified identical)

1. **Scope drill branch disabled** (`if (false && onScopeDrill && …)` in `ExplorerCanvas.onDblClick`) → `double-click on a repo_id leaf calls onScopeDrill` **FAILS** (`vitest-negctl-scope-drill-disabled.log`).
2. **150ms fade removed** (`fadeContent` made a no-op in App.svelte) → fake-timer fade-window test **FAILS** (`vitest-negctl-fade-disabled.log`) — the test genuinely observes the 150ms window, not just the settled state.
3. **Single-expansion broken** (Inbox `toggleExpand` keeps `expandedId = id`) → accordion collapse test **FAILS** (`vitest-negctl-single-expansion-disabled.log`).
4. **Drill URL push removed** (ExplorerView `selectRepo` no pushState) → 3 of 4 drill-URL tests **FAIL** (`vitest-negctl-drill-url-disabled.log`).
5. **Actions snippet re-nested inside the header `<button>`** (the round-2 defect) → `actions snippet renders as a sibling…` **FAILS** (`vitest-negctl-nested-actions2.log`).
6. **CSS scoping guard** — independent compile probe (svelte/compiler, `scoping-probe`): Inbox emits `.inbox-card.svelte-HASH .accordion-header.card-header` (+`:focus-visible`, +reduced-motion `transition:none`), Briefing emits `.briefing-section.svelte-HASH .accordion-header.section-heading` (+`:hover`) and the base `.section-heading.svelte-HASH` for the metrics `h2`; **zero "Unused CSS selector" warnings** in all three components. The round-2 dead-CSS finding is genuinely fixed and compile-pinned by `AccordionScoping.test.js`.

## Spec-contract verification (production code, not test mirrors)

- **Scope Transitions**: `App.svelte` `fadeContent()` (150ms timer, `faded` class, `--transition-fast: 150ms ease` opacity transition on `.content-inner`) + `pushState()` → `history.pushState` on every scope change (`goToWorkspaceHome/goToRepo/goToRepoTab/goToEntityDetail/settings/rules/profile/cross-workspace`, verified by grep: no `location.href` assignment on these paths). App-level test renders the real `App.svelte` with the real `WorkspaceHome` and drives a repo-card click.
- **Drill-Down**: `DetailPanel.svelte` `transition: width 200ms ease-out, min-width 200ms ease-out`, `.detail-panel.open { width: 40% }` → main compresses to 60%; Esc/✕ close covered by pre-existing `DetailPanel.test.js` (re-verified passing). Double-click drill: `ExplorerCanvas.onDblClick` routes repo_id leaf nodes (no Contains children) to `onScopeDrill`; `WorkspaceHome` wires it to `onSelectRepo(repo, 'architecture')` → `App.goToRepo` (mode change, breadcrumb, pushState, no reload) — wiring traced end-to-end in source; `WorkspaceHomeScopeDrill.test.js` covers single-click=panel/no-scope-change, double-click=scope-change/no-panel, unknown repo_id=toast+no-navigation. ExplorerView workspace scope additionally pushes `?repo=` via pushState with mount-time deep-link restore and popstate sync (4 tests).
- **Inline Expansion**: reusable `AccordionItem.svelte` (controlled, real `<button type=button>` header with `aria-expanded`/`aria-controls`, body conditionally rendered below, actions snippet as sibling row — no nested interactive elements, guarded by DOM tests). Applied to Inbox cards (`expandedId`) and all four Briefing sections (`expandedSection`, completed default-expanded); single-expansion tested at group-host, Inbox, and Briefing level. Entity refs inside expanded items (View Spec / agent / MR / quick links) call `openDetailPanel` with the right entity types — tested.
- **Contextual Chat**: `InlineChat.svelte` (pre-existing, verified present at base) renders `Message to X ▸` / `Ask about X ▸` / `Edit spec: "…" ▸` from `en.json` (`inline_chat.message_to` = `"Message to {recipient} ▸"` etc., keys verified present) with per-type capability hints (signed+persisted / read-only / draft-suggestions); integrated at the bottom of the agent detail panel, MR chat tab, Briefing Q&A, MetaSpecs spec-edit, WorkspaceHome orchestrator message. Task plan said "create ContextualChat.svelte"; the component already existed as `InlineChat.svelte` with exactly this contract — reusing it is the right call, not a gap.

## Notes

- Sandbox TCP listener probe unsupported (`capabilities.json`, errno 95) — no browser smoke test possible; jsdom + compile-time guards are the verification surface, with negative controls proving the tests bind to real behavior. Host/CI checks: `cd web && npm ci && npx vitest run` (expect only `ExplorerCanvas-performance.test.js` timing flakes, pre-existing at base) and `npx vite build`.
- `web/src/__tests__/setup.js` guard (`typeof localStorage !== 'undefined'`) is required for the node-environment compile test; DOM tests unaffected.
