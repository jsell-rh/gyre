# Review — task-173 (Interaction Patterns — scope transitions, drill-down, inline expansion)

Spec: `specs/system/ui-layout.md` §3 (Scope Transitions, Drill-Down, Inline Expansion, Contextual Chat).
Candidate: `810cf39b5ea39455cba73c48cf83dbcb4e40be85` (assigned base `a1751da1`). Assigned contract restored verbatim in this round after the prior round's task-record mutation (`ac069af4` durable finding) — verified: the diff from base touches only `specs/tasks/task-173.md` (frontmatter progress/commits + Shipped section), web sources, and tests; `web/dist` is byte-identical to base; no spec files under `specs/system/` changed.
Verdict: **needs-revision** (2 code findings).

## Round 1 — independent review

### Verification runs (candidate head, `npm ci` first per instructions)

All 10 interaction-pattern test files at head (`npx vitest run`, logs in `/tmp/stage/review-evidence/task-173-vitest-*.log`):

- Core set (AccordionItem, InlineChat, ScopeTransitions, WorkspaceHomeScopeDrill, ExplorerViewDrillUrl): **5 files, 30 tests passed**.
- Inbox, Briefing, ExplorerCanvas: **3 files, 192 tests passed**.
- DetailPanel + DetailPanelChat: **2 files, 38 tests passed**.

Total: 260/260 green — matches the Shipped claim.

### Negative controls (scratch worktree, each reverted after the run; logs `task-173-negctl-*.log`)

Each mutation breaks the relevant tests — the suite is not self-confirming:

| Mutation | Result |
|---|---|
| Disable `onScopeDrill` branch in `ExplorerCanvas.onDblClick` (`if (false && node?.repo_id…`) | ExplorerCanvas "scope drill-down": 1 failed (double-click drill test) |
| Break Inbox single-expansion (`expandedId = id`, never collapse) | Inbox: 1 failed ("clicking same card again collapses it") |
| Remove `selectRepo`/`backToRepoList` pushState in ExplorerView | ExplorerViewDrillUrl: 3 of 4 failed |
| Remove Briefing default expansion (`expandedSection = $state(null)`) | Briefing: 4 failed |
| Remove Briefing toggle (`toggleSection` body voided) | Briefing: 12 failed |
| Remove `fadeContent()` body (return early) | ScopeTransitions: 1 failed (the fake-timer 150ms-window test) |
| Remove agent recipient label in InlineChat (`message_to` → '') | InlineChat: 1 failed (recipient display) |

### Spec-contract verification (positive evidence)

- **Scope Transitions**: `fadeContent()` 150ms (`App.svelte:119-122`, `--transition-fast: 150ms ease` in `design-system.css:101`), `transition: opacity var(--transition-fast)` on `.content-inner` (`App.svelte:2357`), every `goTo*` path calls `fadeContent()` + `history.pushState`; no `location.href` assignment/`reload()` anywhere in `App.svelte`. Breadcrumb buttons wire to the `goTo*` handlers. App-level test renders the real App + WorkspaceHome.
- **Drill-Down**: `DetailPanel.svelte:5342` `transition: width 200ms ease-out`, `.detail-panel.open { width: 40% }` in the flex row (main compresses to 60%); ExplorerView's node-detail and editor panels use the same 200ms ease-out. Esc/✕ close covered by pre-existing DetailPanel tests. Double-click → `onScopeDrill` (repo_id leaf, no Contains children) → `WorkspaceHome` wires to `onSelectRepo(repo, 'architecture')` → `App.goToRepo` (scope change + pushState); single-click opens the panel without scope change. ExplorerView workspace-scope drill pushes `?repo=` and this round added restore-on-mount + popstate sync (2 new tests; the implementer's own negative control — 2 failed without the fix — was re-verified as passing at head).
- **Inline Expansion**: `AccordionItem.svelte` controlled accordion (`aria-expanded`/`aria-controls`, real `<button type="button">`); Inbox `expandedId` and Briefing `expandedSection` single-expansion state; entity refs in expanded items call `openDetailPanel` (Inbox tests: spec/agent/MR; Briefing tests: agent/mr).
- **Contextual Chat**: `InlineChat.svelte` recipient labels `Message to X ▸` / `Ask about X ▸` / `Edit spec: "X" ▸` (`en.json:1732-1734`) with per-type capability hints; integrated in DetailPanel agent/MR chat tabs, Briefing Q&A, MetaSpecs.

### Findings

- [-] **F1 (major, code): the accordion header styling contract between AccordionItem and both consumers is dead CSS — the Inbox/Briefing accordion headers render unstyled in a real browser.** `AccordionItem.svelte` authors the header `<button>` inside its own file, so at runtime the button carries only AccordionItem's Svelte scoping class (jsdom probe: header className `"accordion-header card-header svelte-xwb3sw"`; Briefing: `"accordion-header section-heading svelte-xwb3sw"`). Inbox passes `headerClass="card-header"` and Briefing passes `headerClass="section-heading"`, and the restoring styles live in the *parent* `<style>` blocks (`Inbox.svelte:853-872` `.card-header` padding/flex/typography; `Briefing.svelte:799-814` `.accordion-header.section-heading` full heading treatment). Svelte 5 scoped CSS only matches elements carrying the authoring component's hash — the child-authored button never does. Compile probe of `Inbox.svelte`: `.card-header` and `.card-header:focus-visible` are **pruned with `/* (unused) */` markers** (0 live `.card-header` rules); the vitest logs themselves warn `Unused CSS selector ".card-header"` / `".card-header:focus-visible"` (Inbox.svelte:869, 1150). Briefing's rules survive pruning but compile to `.accordion-header.section-heading.svelte-1rd56bs`, which cannot match the runtime button (hash `svelte-xwb3sw`). Net effect: in the browser the Inbox card headers render with only AccordionItem's `all: unset; display: block` reset (no padding, no flex row, no typography) and Briefing section headings lose the entire heading look. jsdom cannot see this (no layout engine), so all 260 tests pass while the visible UI is broken — exactly the class of defect the component's own "layout-neutral, callers compose via class prop" comment invites. Reproduce: compile both files (`svelte/compiler`, `css: 'external'`) and grep for live `.card-header`/`.accordion-header.section-heading` selectors vs the runtime button's hash class, or render in a real browser. Fix directions: author the header styling inside `AccordionItem` keyed on the passed class names, use `:global(.accordion-header.card-header)` in the parents, or have the parent wrap/render the header element itself.
- [-] **F2 (major, code): nested interactive elements — `<button>` inside the accordion header `<button>` (invalid HTML content model).** `AccordionItem.svelte`'s header is a real `<button type="button">`; Inbox's `{#snippet header()}` (`Inbox.svelte:391-443`) renders `.card-quick-link` buttons (📋/🔀/▶) and a `.card-subtitle-link` button *inside* it. DOM probe on the rendered Inbox: `header.querySelectorAll('button').length === 2+`. HTML forbids interactive descendants of `button`; parsing is implementation-defined and activation behavior of the inner buttons is unspecified. The pre-task base had the same quick-link buttons inside a `div[role=button]` (ARIA-invalid but DOM-legal); making the wrapper a real `<button>` without relocating the quick-links converted an ARIA violation into a content-model violation. The `stopPropagation` calls make the jsdom behavior correct, which is why the tests pass. Fix: render the quick-link buttons outside the header button (sibling overlay in `.inbox-card`, absolutely positioned or in a non-interactive header region), or move them into the expanded body.

### Non-findings (checked, no defect)

- `App.svelte`, `InlineChat.svelte`, `DetailPanel` chat integration, locales: byte-identical to base — the Contextual Chat and scope-transition infrastructure predates this task and the task correctly did not touch it; the new work is the accordion, the canvas scope-drill wiring, the ExplorerView drill URL, and the tests.
- Primary-entity types (spec/task/mr/agent) navigate to full-page detail views via `goToEntityDetail` rather than opening the side panel in Split layout — pre-existing base behavior, consistent with `ui-navigation.md` ("Click a spec → enters the repo…, detail panel open for that spec"; full-page entity detail views within repo mode), not a regression introduced by this task and not in this task's acceptance criteria.
- The `__accordion_group.svelte` test host lives in `src/lib/` (not `__tests__/helpers/`) — minor placement smell, not a defect.
- Sandbox: TCP listener probe unsupported (capabilities.json errno 95), so no live browser/server smoke test; component-level jsdom verification plus the CSS compile/DOM probes above were used instead. The F1/F2 findings are precisely the class a browser smoke test would have caught — recommend one during the revision round if a listener-capable environment is available.

### Verdict

needs-revision. The interaction-pattern behavior (toggle, single-expansion, drill wiring, URL handling, chat recipients) is real and well-tested, but the accordion header visual contract is broken in the actual rendering path (F1) and the header markup is invalid HTML (F2). Both are localized to `AccordionItem.svelte` + the two consumers' header snippets and header-class styling.
