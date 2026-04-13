---
title: "HSI Stable Sidebar Navigation Model"
spec_ref: "human-system-interface.md §1.3"
depends_on: []
progress: not-started
coverage_sections:
  - "human-system-interface.md §1.3 Stable Sidebar, Adaptive Content"
commits: []
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

- [ ] Permanent 6-item sidebar visible on all routes (Inbox, Briefing, Explorer, Specs, Meta-specs, Admin)
- [ ] Sidebar does NOT change based on current scope (tenant, workspace, repo)
- [ ] Active sidebar item is visually highlighted
- [ ] Clicking a sidebar item preserves current scope
- [ ] Sidebar + topbar breadcrumb coexist without layout conflicts
- [ ] Mobile/responsive behavior: sidebar collapses gracefully
- [ ] `npm test` passes in `web/`
- [ ] No regression in existing navigation flows

## Agent Instructions

Read `specs/system/human-system-interface.md` §1.3 (Stable Sidebar, Adaptive Content) and `docs/ui.md` for the current navigation model. The main app layout is in `web/src/App.svelte`. The spec explicitly states "no sidebar" was a previous decision — this task replaces that with the HSI spec's 6-item permanent sidebar. Check `ui-layout.md` §1 for sidebar dimensions and layout constraints. The sidebar items must exactly match the 6 listed (no more, no fewer). Keyboard shortcuts Cmd+1 through Cmd+6 (§1.8) should map to these same items — verify the existing g-key shortcuts still work alongside.
