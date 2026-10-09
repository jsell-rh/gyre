---
title: "Interaction Patterns — scope transitions, drill-down, inline expansion"
spec_ref: "ui-layout.md §3"
depends_on: []
progress: not-started
coverage_sections:
  - "ui-layout.md §3. Interaction Patterns"
  - "ui-layout.md §Scope Transitions"
  - "ui-layout.md §Drill-Down (Entity Detail)"
  - "ui-layout.md §Inline Expansion (Inbox/Briefing)"
  - "ui-layout.md §Contextual Chat"
commits: ["e25dd52404f06f48b170c4b9cabd9817b036e7f2"]
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
