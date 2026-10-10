---
title: "Interaction Patterns — scope transitions, drill-down, inline expansion"
spec_ref: "ui-layout.md §3"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "ui-layout.md §3. Interaction Patterns"
  - "ui-layout.md §Scope Transitions"
  - "ui-layout.md §Drill-Down (Entity Detail)"
  - "ui-layout.md §Inline Expansion (Inbox/Briefing)"
  - "ui-layout.md §Contextual Chat"
commits: ["43f67b86d9912e4f4b945cf15dd42700a23ffe52", "94de2edcff81d1fc9c048f000c5d9f31c78acd29", "2c5641aeb1f5a725d14cf0741aa4066703bcb40e", "add218c8c0e9af1a21c20e2587f02d7f734e10ff", "3b28d8480fe841c3b602ad4d4eb38dc3bb215100", "99e99039f01c69ef3b7c19f712aeb8de3f83c864", "e25dd52404f06f48b170c4b9cabd9817b036e7f2"]
---

## Spec Excerpt

ui-layout.md §3 defines standardized interaction patterns used across all views:

**Scope Transitions**: Breadcrumb click → content cross-fades (150ms), sidebar active item unchanged, URL updates via pushState. No full-page reload.

**Drill-Down (Entity Detail)**: Click entity → detail panel slides in (200ms ease-out), main content compresses to 60%. Double-click graph node → drill down to next C4 level (changes scope, breadcrumb updates, URL changes).

**Inline Expansion**: Inbox items and Briefing sections use accordion pattern — click expands below header, only one item expanded at a time. Clicking entity reference within expanded item opens detail panel (Split layout).

**Contextual Chat**: Chat input at bottom of detail panel with explicit recipient indicator: `Message to worker-12 ▸` / `Ask about this briefing ▸` / `Edit spec: "..." ▸`. Different recipients have different capabilities (agent messages signed/persisted, LLM Q&A read-only, spec editing produces drafts).

## Implementation Plan

1. **Standardize scope transitions**:
   - Verify cross-fade timing (150ms opacity) in App.svelte's `fadeContent()`
   - Ensure all scope changes use pushState, no full reloads
   - Verify breadcrumb updates immediately on scope change

2. **Standardize drill-down pattern**:
   - Verify detail panel slide-in timing (200ms ease-out)
   - Implement double-click → C4 drill-down on graph nodes in ExplorerView
   - Ensure clicking another entity replaces panel (no stacking)
   - Esc or ✕ closes panel, main returns to full-width

3. **Inline expansion (accordion) pattern**:
   - Create reusable `AccordionItem.svelte` component
   - Apply to Inbox.svelte items
   - Apply to Briefing.svelte sections
   - Only one item expanded at a time
   - Entity reference clicks within expanded items open detail panel

4. **Contextual Chat component**:
   - Create `ContextualChat.svelte` with recipient indicator
   - Support multiple recipient types with different UI/behavior
   - Integrate into detail panel bottom

5. **Tests**:
   - Accordion: expand/collapse, single-expansion constraint
   - Chat: recipient display, message sending
   - Drill-down: double-click vs single-click behavior

## Acceptance Criteria

- [ ] Scope transitions use 150ms cross-fade, no reload, pushState
- [ ] Detail panel slides in 200ms ease-out, compresses main to 60%
- [ ] Double-click on graph node drills down (scope change + URL update)
- [ ] Accordion pattern in Inbox with single-expansion constraint
- [ ] Entity references in expanded items open detail panel
- [ ] Contextual Chat component with recipient indicator
- [ ] Tests pass for all interaction patterns

## Agent Instructions

Read `ui-layout.md` §3 for the full interaction pattern definitions. Check existing App.svelte for the detail panel and fade implementations — much of this infrastructure exists but may need alignment with the spec's exact timing and behavior requirements. The AccordionItem component should be reusable across Inbox, Briefing, and any future accordion views. The ContextualChat component should be generic enough for agent messages, LLM Q&A, and spec editing.

## Shipped

Interaction patterns per ui-layout.md §3, recovered from two interrupted checkpoints (`99e99039`, `e25dd524`) and completed with the missing test coverage (`add218c8`):

**Scope Transitions** — every scope change in `App.svelte` runs `fadeContent()` (150ms opacity cross-fade on `.content-inner`, `--transition-fast` = 150ms ease) then `history.pushState` (`goToWorkspaceHome`, `goToRepo`, `goToRepoTab`, `goToEntityDetail`, settings/rules/profile/cross-workspace). Breadcrumb renders from the same reactive state, so it updates immediately; the no-sidebar shell keeps the workspace segment in scope (repo-mode breadcrumb leads with the workspace name). No `location.href` assignment or reload anywhere on these paths. `ScopeTransitions.test.js` (new) proves all four spec points at App level with the real `WorkspaceHome`: URL change via a `pushState` spy (real implementation preserved), breadcrumb content, the `faded` class present during the 150ms window and cleared after (fake-timer test: still faded at +100ms, visible at +160ms), workspace segment persistence, and same content-root node survival (no reload).

**Drill-Down (Entity Detail)** — `DetailPanel.svelte` slides in from the right with `transition: width 200ms ease-out` and compresses main to 60% (`.detail-panel.open` = 40% width; ExplorerView's node detail area and spec editor panels use the same 200ms ease-out slide-in). Panel open/close/Esc/✕ and tab behavior were already covered by `DetailPanel.test.js` / `DetailPanelChat.test.js` (38 tests). Double-click drill: `ExplorerCanvas.onDblClick` routes repo_id leaf nodes (no Contains children) to the new `onScopeDrill` callback; `WorkspaceHome` wires it to `onSelectRepo(repo, 'architecture')` → `App.goToRepo` (scope change, breadcrumb update, pushState, no reload); single-click still opens the detail panel without scope change. Covered three ways: `ExplorerCanvas.test.js` "scope drill-down" (double-click calls `onScopeDrill` not `onNodeDetail`/`drillInto`; single-click opens panel; nodes with Contains children keep in-graph drill; missing callback doesn't crash) and the new `WorkspaceHomeScopeDrill.test.js` (single-click → `openDetailPanel({type:'repo'})` no scope change; double-click → `onSelectRepo(repo,'architecture')` no panel; unknown repo_id → toast, no navigation). At Explorer workspace scope, `selectRepo` pushes `?repo=<name>` via pushState (deep-linkable) and `backToRepoList` pops it — new `ExplorerViewDrillUrl.test.js` asserts the URL param, graph load for the drilled repo, and Back removing it.

**Inline Expansion** — reusable `AccordionItem.svelte` (controlled: parent owns `open`, header is a real `<button>` with `aria-expanded`/`aria-controls`, body conditionally rendered below). Applied to Inbox cards (`expandedId` single-expansion state, quick-link buttons stopPropagation) and Briefing sections (`expandedSection`, completed default-expanded). Entity references inside expanded items (View Spec, agent ref-link, MR ref-link, quick-link buttons) open the detail panel — now directly tested in `Inbox.test.js` (spec/agent/MR ref clicks each call `openDetailPanel` with the right entity) and `Briefing.test.js` (spec/agent/mr ref links). Single-expansion constraint tested in `AccordionItem.test.js` (group host), `Inbox.test.js`, and `Briefing.test.js`.

**Contextual Chat** — `InlineChat.svelte` with explicit recipient indicator per spec (`Message to X ▸` / `Ask about X ▸` / `Edit spec: "…" ▸` via i18n, capability hints per recipient type: agent messages signed/persisted, LLM Q&A read-only, spec editing produces drafts). Integrated at the bottom of the agent detail panel (signed Directed-tier steering, `DetailPanel.svelte`), the MR chat tab (author agent), Briefing Q&A (`Ask about this briefing ▸`, SSE-streamed), MetaSpecs editing (`spec-edit` recipient), and WorkspaceHome agent messaging. `InlineChat.test.js` covers recipient display for all three types, send behavior, history, hints, and focus; `DetailPanelChat.test.js` covers the panel integration and message sending.

Test evidence (`npx vitest run` on the 10 interaction-pattern test files; log at `/tmp/stage/review-evidence/task-173/vitest-repair-head.log`): **10 files, 260 tests passed** — AccordionItem (7), InlineChat (16), Briefing (25), Inbox (30), ExplorerCanvas (136), WorkspaceHomeScopeDrill (3), ScopeTransitions (2), ExplorerViewDrillUrl (4), DetailPanel + DetailPanelChat (38). No production source changed in `add218c8` — the implementation shipped in the two recovered checkpoints; that commit added the missing wiring-level tests and restored `web/dist` to base `f4acb4eb` (the checkpoints accidentally shipped a rebuild; task branches build from source in CI — same policy as task-210 round 12).

Sandbox limitation: TCP listener probe unsupported (capabilities.json `tcp_listener_probe.errno 95`), so no local browser/server smoke test was run; the interaction patterns are verified through the jsdom component tests above. Host verification and exact-head GitHub checks remain with the reviewer/CI.

Contract repair round: the previous round's task record added an unapproved normative-looking section ("## Review round (code head ...)") — not one of the operational headings the pipeline treats as evidence-only — which changed the assigned contract. The assigned contract is restored verbatim above (only this Shipped section is updated). Repair work shipped this round: (1) `ExplorerView` drill URL was write-only — `selectRepo` pushed `?repo=` claiming deep-linkability, but nothing restored it on mount and browser Back (popstate) left the view showing the drilled graph while the URL said workspace scope. Added `drilledRepoFromUrl()` restore-on-mount (resolves the param against the loaded workspace repo list, no extra history entry) and a popstate listener that syncs `showingRepoGraph`/`selectedRepoId`/`graph` with the URL in both directions; two new tests in `ExplorerViewDrillUrl.test.js` (4 total) fail without the fix (negative control: 2 failed | 2 passed, log at `/tmp/stage/review-evidence/task-173/vitest-drill-negctl-without-fix.log`) and pass with it. (2) Removed a tautological assertion in `ScopeTransitions.test.js` (`contains('faded') || !contains('faded')` — always true; the deterministic fade assertions live in the fake-timer test and the settled-state check). (3) Replaced a conditional-guard keyboard test in `AccordionItem.test.js` (`if (ontoggle.mock.calls.length > 0)` — could never fail; jsdom does not synthesize keydown-to-click, that is browser behavior) with a real assertion of the activation mechanism: `<button type="button">` tag and type.
