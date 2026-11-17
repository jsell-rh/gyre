---
title: "Navigation: context-dependent collapsible rail (retargeted from HSI stable sidebar)"
spec_ref: "ui-navigation.md §1a"
depends_on: []
progress: ready-for-review
review: specs/reviews/task-082.md
coverage_sections:
  - "ui-navigation.md §1a Collapsible Navigation Rail (Optional)"
commits: ["0fab4972d43f8fd9a477bd0177cb5a0a8f318682", "42e12c08d3b67f208e3bc55486a17ed9c9272d4f", "79323318aac3b3015c03867f8f6130d0e913e67a", "9561ee864b066ba4a41fed4aa1f33cb16f4a0b47", "72bd582329ac50dbb2062d3ea2f3ea0b7f6213d3", "f332f774069ee409b407f2b7c9ee63deba51e3b6"]
---

## Shipped

The retargeted requirement (ui-navigation.md §1a, Collapsible Navigation
Rail) is implemented and repaired through review round
`79201380b96745dd8707d170d3b1274c`: rail code + tests in `42e12c08`,
docs/spec/coverage updates in `fa9cc057`, and review-finding repairs
(F7–F9) in `0fab4972`.

### Behavior

- **Rail component** (`web/src/components/NavRail.svelte`): icon-only
  48px collapsed by default, toggle expands to labelled 240px;
  preference persisted per user per mode under
  `gyre_nav_rail_expanded.<mode>.<user-id>` (first use = collapsed);
  scope-labelled header; stable per-mode item order; active item
  highlighted via `.active` + `aria-current="page"`; hidden below
  768px so there is no persistent rail on mobile.
- **Where it renders** (App.svelte `showNavRail`): workspace home (with
  a selected workspace) and repo mode without an open entity detail.
  Never on focused surfaces: full-page entity detail, workspace
  settings / agent rules / analytics / profile sub-pages,
  cross-workspace view, notification settings. Every destination stays
  reachable without the rail (dashboard sections, repo tab strip, ⌘K,
  g-shortcuts unchanged).
- **Item sets**: workspace-home rail = §2 dashboard sections in
  canonical order + Settings; repo rail mirrors the RepoMode `TABS`
  strip exactly, each keeping its shipped tab position, selecting a
  rail item takes the same route as clicking the tab.
- **Section selection (§1a, repaired F8)**: section items scroll to
  their dashboard section on the workspace home (Architecture also
  expands the collapsed section via `archExpandSignal`; Agent Rules
  scrolls to `section-agent-rules` — fixed from the sub-page escape);
  Settings is the only navigation exception (workspace settings page).
- **Labels (repaired F7)**: all rail/drawer labels resolve through real
  i18n keys; `common.settings` added to `en.json` so the Settings item
  renders "Settings" instead of the raw-key fallback.
- **Narrow viewports**: the hamburger drawer offers the rail's per-mode
  item set (workspace sections + Settings at workspace scope; the repo
  tab strip exactly in repo mode); tenant scope (`/all`) keeps its §10
  section semantics.

### Test evidence

- `web/src/__tests__/NavRail.test.js` (14 tests, incl. F7/F9 repairs):
  component contract (collapse default, expand toggle + per-user
  per-mode persistence, restore on mount, active highlight + onSelect)
  and app integration (workspace/repo rail item sets, order, scope
  labels, Architecture scroll+expand, Agent Rules scroll to
  `section-agent-rules` with the actual `scrollIntoView` target pinned
  via `mock.contexts` and active highlight, Settings navigation + rail
  absence on the sub-page, repo tab switching, rail absence on focused
  surfaces, per-mode localStorage keys, resolved-label test asserting
  the expanded rail and mobile drawer Settings texts with no raw i18n
  key text surfacing).
- `web/src/__tests__/NoSidebar.test.js` (5 tests): no permanent sidebar;
  rail-vs-permanent-sidebar distinction; drawer canonical §1a item set
  and order; ⌘1-6 not advertised; `g 2` tab shortcut.
- `web/src/__tests__/AppShell.test.js` (77 tests): drawer agent-rules
  test retargeted to the §1a scroll semantics (stays on workspace
  home); repo-mode drawer tab-strip mirror and all prior URL/shortcut
  coverage unchanged.
- Focused runs (exit 0, fixed tree): NavRail + NoSidebar + AppShell =
  96 passed; WorkspaceDrawerSectionNav + BidirectionalNav = 19 passed.
  Mutation checks recorded in `/tmp/stage/review-evidence`
  (`task-082-repair.md`): removing `common.settings` fails the label
  test with `common.settings` surfaced; reverting the agent-rules case
  to the sub-page escape fails both the NavRail scroll test and the
  AppShell drawer test.

## Supersession and retarget (2026-11-16)

This task originally instructed building the permanent six-item sidebar from
`human-system-interface.md` §1.3 ("Stable Sidebar, Adaptive Content"). That
instruction is **retired**. Operator decision `16871182b1370b61693fc691`
(option_c, recorded by jsell; full record in `specs/tasks/task-246.md` and
`ui-navigation.md` §1a) superseded both the literal six-fixed-items binding
and `ui-navigation.md`'s blanket "no sidebar" rule.

**Current requirement:** `ui-navigation.md` §1a (Collapsible Navigation Rail)
— an optional, collapsible, scope-labelled left panel on navigation-heavy
screens (workspace home and repo mode), with stable per-mode item sets
mirroring the dashboard sections / repo tab strip, collapsed (icon-only) by
default to preserve canvas/editor space, and a narrow-viewport drawer. The
rail is never load-bearing: every destination stays reachable without it, and
focused editor/canvas surfaces keep full width.

**Honest status:** implemented per the retargeted §1a requirement
(rail code `42e12c08`, docs `fa9cc057`), then repaired after independent
review (F7 i18n key, F8 agent-rules scroll semantics, F9 label test
gaps) in `0fab4972`. The `commits:` entries before `42e12c08` are the
historical record of the superseded deliverable.

## Requirement (retargeted) — acceptance criteria

- [ ] Optional collapsible navigation rail on workspace home and repo mode only; never rendered on focused editor/canvas surfaces (Editor Split, popped-out detail panels, full-screen modals)
- [ ] Scope-labelled per mode (workspace name at workspace home; workspace/repo path in repo mode); no global menu shared across modes
- [ ] Workspace-home item set mirrors the §2 dashboard sections in canonical order plus Settings; repo-mode item set mirrors the repo tab strip exactly
- [ ] Stable item ordering within each mode (items never move, appear, or disappear within a mode)
- [ ] Collapsed (icon-only, 48px) by default; toggle expands to labelled form (240px); preference persisted per user per mode
- [ ] Every rail destination remains reachable without the rail (dashboard, tabs, ⌘K, `g`-shortcuts); `NoSidebar.test.js` asserts the absence of a *permanent* sidebar (updated, not deleted, if its assertions need to distinguish rail from permanent sidebar)
- [ ] Narrow viewports (<768px): rail item set offered via the hamburger drawer; no persistent rail on mobile
- [ ] `docs/ui.md` updated to describe the shipped rail once implemented

## Historical record (SUPERSEDED — preserved for intent, not for implementation)

The following is the original task body. Its acceptance criteria were met by
the sidebar implementation that was later removed (`08e5f91f`); the checked
boxes below describe that deleted implementation and are record only.

### Spec Excerpt (superseded)

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

*(Superseding note: `human-system-interface.md` §1.3 now carries the
supersession record; the current requirement is `ui-navigation.md` §1a.)*

### Implementation Plan (superseded — do not execute)

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

### Acceptance Criteria (superseded — historical record of the deleted implementation)

- [x] Permanent 6-item sidebar visible on all routes (Inbox, Briefing, Explorer, Specs, Meta-specs, Admin)
- [x] Sidebar does NOT change based on current scope (tenant, workspace, repo)
- [x] Active sidebar item is visually highlighted
- [x] Clicking a sidebar item preserves current scope
- [x] Sidebar + topbar breadcrumb coexist without layout conflicts
- [x] Mobile/responsive behavior: sidebar collapses gracefully
- [x] `npm test` passes in `web/` (task-082 suites AppShell + Sidebar: 101/101 — 92 after R5, +1 repo-scope Briefing regression (F5), +9 tenant-scope suite (Post-R8 sweep); pre-existing canvas/ResizeObserver failures in FlowRenderer/MoldableViewNodeTypeFilter/ExplorerViewAskViewSpec are unrelated and reproduce identically at merge-base `4988865b`)
- [x] No regression in existing navigation flows

### Agent Instructions (superseded — do not execute)

Read `specs/system/human-system-interface.md` §1.3 (Stable Sidebar, Adaptive Content) and `docs/ui.md` for the current navigation model. The main app layout is in `web/src/App.svelte`. The spec explicitly states "no sidebar" was a previous decision — this task replaces that with the HSI spec's 6-item permanent sidebar. Check `ui-layout.md` §1 for sidebar dimensions and layout constraints. The sidebar items must exactly match the 6 listed (no more, no fewer). Keyboard shortcuts Cmd+1 through Cmd+6 (§1.8) should map to these same items — verify the existing g-key shortcuts still work alongside.

*(Retarget note: for the current requirement, read `ui-navigation.md` §1a and
`docs/ui.md` instead. Nothing in the superseded plan above is authorized.)*
