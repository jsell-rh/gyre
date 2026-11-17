---
title: "Navigation: context-dependent collapsible rail (retargeted from HSI stable sidebar)"
spec_ref: "ui-navigation.md §1a"
depends_on: []
progress: complete
review: specs/reviews/task-082.md
coverage_sections:
  - "ui-navigation.md §1a Collapsible Navigation Rail (Optional)"
commits: ["86f33267ca3985bea90b43d8190aea79d81dfdf8", "3357752e5c91107369b524f4ba1036196b9fde18", "96a00d28b4f26443cc8a639714008e2ceac450e3", "b508a6f6538082f7c8f7d4ab95265a2d989fa263", "0fab4972d43f8fd9a477bd0177cb5a0a8f318682", "42e12c08d3b67f208e3bc55486a17ed9c9272d4f", "79323318aac3b3015c03867f8f6130d0e913e67a", "9561ee864b066ba4a41fed4aa1f33cb16f4a0b47", "72bd582329ac50dbb2062d3ea2f3ea0b7f6213d3", "f332f774069ee409b407f2b7c9ee63deba51e3b6", "81a61f3e6607b281bcb3644c9108be82f7b7c027"]
---

## Shipped

The retargeted requirement (ui-navigation.md §1a) is implemented on this
branch (rail component + tests in `42e12c08`, docs/coverage in `fa9cc057`,
review repairs F7–F9 in `0fab4972`, F10 single-source-of-truth in
`b508a6f6`) and independently reviewed through round F10. (In the
`commits:` frontmatter, `d99ace7b`–`79323318` are the historical
superseded-deliverable record; `42e12c08`–`3357752e` are the retargeted
deliverable.) This assignment
repaired the remaining CI failure: GitHub check "E2E Tests" run
44039546790 failed 16 explorer-visual tests because every screenshot was
exactly 48px narrower than its committed baseline — the collapsed rail
legitimately renders in repo mode (the Explorer architecture tab is §3
repo mode, not a §1a focused surface), and the baselines predated the rail.

### CI repair (this assignment)

- 16 baselines under `web/tests/e2e/explorer-visual.spec.js-snapshots/`
  replaced with the CI run's own actual PNGs (ground-truth renders of the
  rail-shifted layout at this head). CI is deterministic within a run (both
  screenshot attempts of every failing test reported identical diff
  counts) and across runs (49 pre-rail baselines have passed CI unchanged
  for months; runner image, playwright, browser build, and npm lockfile are
  pinned in `.github/workflows/e2e.yml`).
- `filter-preset-types-linux.png` (17th stale baseline, never reached in CI
  because its test failed at the first screenshot) regenerated locally;
  expected CI residual is the measured sandbox noise class for 871x330
  canvases (0.25% raw pixels, sibling `filter-preset-endpoints`), far under
  the 5% preset threshold.
- No product code, no test-logic edits (`explorer-visual.spec.js` is
  byte-identical to the assignment head), no threshold changes, no
  exemptions: assertion strength is preserved and the rail shift (6–34% of
  pixels per snapshot) remains far above the comparators' limits.
- Local regeneration was rejected with evidence: this sandbox's canvas
  rasterization differs from the pinned ubuntu-24.04 CI runner by
  0.25–2.2% raw pixels per snapshot (DOM screenshots are pixel-identical;
  forced-fonts pins those binaries). Locally-generated baselines would fail
  CI on `blast-radius-tiered` (2.2% > 2%). Full analysis in
  `/tmp/stage/review-evidence/task-082-e2e-baseline-repair.md` (retained
  controller evidence: reproduction log, per-snapshot fidelity matrix,
  final run log).

### Test evidence

- Focused local run against the shipped baselines:
  `npx playwright test explorer-visual` → **16/16 passed** (exit 0;
  reproduction of the pre-repair failure and the final green run are logged
  in `/tmp/stage/review-evidence/task-082-local-repro.log` and
  `task-082-final-run.log`). Playwright's perceptual comparator absorbs the
  sub-threshold sandbox rasterization noise; the rail's 48px shift still
  fails the comparison without the new baselines (verified by stashing
  them: 16/16 fail with the identical CI signature).
- Environment notes for remote verification: this sandbox blocks loopback
  `fetch` from Node (SSRF guard) — the seeded fixture was served through a
  loopback fetch shim and a stand-in static+WS server on 127.0.0.1:2222
  (scripts retained in review-evidence); no gyre-server build was attempted
  here (4 GiB budget rule). The authoritative gate is the exact-head GitHub
  "E2E Tests" check, which must be confirmed green remotely.
- Rail component behavior and its unit/integration suites (NavRail,
  NoSidebar, RepoMode, AppShell) were shipped and verified in the earlier
  rounds recorded in `specs/reviews/task-082.md`; this assignment changed
  no source, so those results stand.

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

**Honest status:** the rail is specified but **not implemented**. The
original deliverable below was removed from the product in commit `08e5f91f`
(nav rework, PRs #589–#599) and must not be re-landed as-is. Progress is
`not-started` against the retargeted requirement; the controller decomposes
implementation work after amendment task-246 merges. The `commits:` list
above is the historical record of the superseded deliverable.

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
