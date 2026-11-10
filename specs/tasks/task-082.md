---
title: "HSI Stable Sidebar Navigation Model"
spec_ref: "human-system-interface.md §1.3"
depends_on: []
progress: complete
review: specs/reviews/task-082.md
coverage_sections:
  - "human-system-interface.md §1.3 Stable Sidebar, Adaptive Content"
commits: ["79323318aac3b3015c03867f8f6130d0e913e67a", "9561ee864b066ba4a41fed4aa1f33cb16f4a0b47", "72bd582329ac50dbb2062d3ea2f3ea0b7f6213d3", "f332f774069ee409b407f2b7c9ee63deba51e3b6"]
---

## Spec Excerpt

The sidebar is **permanent and unchanging** regardless of scope. Users build muscle memory for nav item positions. The *content area* adapts to the current scope (tenant, workspace, repo), not the sidebar.

```
Sidebar (always visible, always these items):
  Inbox
  Briefing
  Explorer
  Specs
  Meta-specs
  Admin
```

Six items. That's it.

**Why not scope-dependent sidebar?** Changing the sidebar when the user navigates between scopes is disorienting — the same navigation item moves position or disappears. The content area is where scope manifests.

## Implementation Plan

The current UI has no sidebar (confirmed by auditor: App.svelte:1032 "no sidebar"). A mobile drawer exists with 5 items. The spec requires a permanent 6-item sidebar.

1. **Add a persistent sidebar component** to the app layout:
   - Fixed-width left sidebar (see `ui-layout.md` §1 for dimensions)
   - Always visible regardless of viewport (desktop); on mobile, collapse to icon-only or drawer
   - Contains exactly 6 items: Inbox, Briefing, Explorer, Specs, Meta-specs, Admin
   - Each item is an icon + label pair
   - Active item is highlighted based on the current route

2. **Wire sidebar items to routes:**
   - Clicking a sidebar item navigates to that section while preserving the current scope
   - Inbox → `/inbox` (tenant) or `/workspaces/:id/inbox` (workspace) or `/repos/:id/inbox` (repo)
   - Briefing → `/briefing` or `/workspaces/:id/briefing`
   - Explorer → `/explorer` or `/workspaces/:id/explorer` or `/repos/:id/explorer`
   - Specs → `/specs` or `/workspaces/:id/specs` or `/repos/:id/specs`
   - Meta-specs → `/meta-specs` or `/workspaces/:id/meta-specs`
   - Admin → `/admin` or `/workspaces/:id/admin` or `/repos/:id/admin`

3. **Preserve scope across nav transitions:**
   - The breadcrumb (scope indicator) stays in the topbar
   - Clicking a sidebar item changes the view but keeps the current scope
   - The content area re-renders for the new nav item at the current scope

4. **Remove old navigation patterns** that conflict with the stable sidebar model (mobile drawer items should align with sidebar).

## Acceptance Criteria

- [x] Permanent 6-item sidebar visible on all routes (Inbox, Briefing, Explorer, Specs, Meta-specs, Admin)
- [x] Sidebar does NOT change based on current scope (tenant, workspace, repo)
- [x] Active sidebar item is visually highlighted
- [x] Clicking a sidebar item preserves current scope
- [x] Sidebar + topbar breadcrumb coexist without layout conflicts
- [x] Mobile/responsive behavior: sidebar collapses gracefully
- [x] `npm test` passes in `web/` (task-082 suites AppShell + Sidebar: 101/101 — 92 after R5, +1 repo-scope Briefing regression (F5), +9 tenant-scope suite (Post-R8 sweep); pre-existing canvas/ResizeObserver failures in FlowRenderer/MoldableViewNodeTypeFilter/ExplorerViewAskViewSpec are unrelated and reproduce identically at merge-base `4988865b`)
- [x] No regression in existing navigation flows

## Agent Instructions

Read `specs/system/human-system-interface.md` §1.3 (Stable Sidebar, Adaptive Content) and `docs/ui.md` for the current navigation model. The main app layout is in `web/src/App.svelte`. The spec explicitly states "no sidebar" was a previous decision — this task replaces that with the HSI spec's 6-item permanent sidebar. Check `ui-layout.md` §1 for sidebar dimensions and layout constraints. The sidebar items must exactly match the 6 listed (no more, no fewer). Keyboard shortcuts Cmd+1 through Cmd+6 (§1.8) should map to these same items — verify the existing g-key shortcuts still work alongside.

## Shipped

### Status: complete in history; requirement superseded via spec lifecycle

The assigned product — the permanent 6-item sidebar — was implemented and
review-accepted in the original round (commits `d99ace7b`, `f332f774`,
`72bd5823`, `9561ee86`, `79323318`; review rounds R1-R10 in
`specs/reviews/task-082.md`, R10 closed with no findings). All five commits
are ancestors of this branch's base `078ec12a5`.

The requirement was then superseded through the spec lifecycle:
`ui-navigation.md` (header note + Principle 5 "No sidebar" + §9 "From HSI
§1") supersedes HSI §1's navigation model; task-210's 2026-10-09 navigation
binding correction removed the sidebar from the shell and reclassified this
task's coverage section (HSI §1.3) as `n/a`-superseded in the coverage matrix,
with the explicit note that task-082 is historical implementation, "not an
instruction to reintroduce the obsolete sidebar."

### This round's work

This round does **not** reintroduce the sidebar — that would ship code
contradicting the canonical spec and re-open the recorded ui-navigation
coverage row-3 regression that task-210 resolved. The remaining real gap in
this task's coverage section was that the HSI spec text itself still
literally mandated the removed sidebar with no supersession note in place —
the "formal spec amendment via spec lifecycle" half of the resolution the
2026-09-30 ui-navigation audit called for (task-208's user-management.md
amendment is the precedent). Shipped:

- `specs/system/human-system-interface.md`: dated supersession note under
  §1 (Navigation Model) and an amendment note on §1.3, recording the
  ui-navigation.md supersession, the task-082 historical implementation, and
  the task-210 removal. Historical text retained.
- `specs/coverage/system/human-system-interface.md` row 3: amended to record
  the in-place spec amendment.

### Test evidence

Focused web suites at base `078ec12a5` (no product-code change this round,
so base runs are the operative evidence): `NoSidebar.test.js`,
`WorkspaceDrawerSectionNav.test.js`, `AppShell.test.js` — 78/78 passed
(vitest; output retained under /tmp/stage/review-evidence/
web-focused-nosidebar-shell.txt). NoSidebar asserts the canonical no-sidebar
shell directly; reintroducing the sidebar would fail it.
