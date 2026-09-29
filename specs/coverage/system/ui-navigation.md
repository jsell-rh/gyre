# Coverage: UI Navigation Model

**Spec:** [`system/ui-navigation.md`](../../system/ui-navigation.md)
**Last audited:** 2026-09-29
**Coverage:** 25/26

| # | Section | Depth | Status | Task | Notes |
|---|---------|-------|--------|------|-------|
| 1 | Why a New Navigation Model | 2 | n/a | - | Rationale — no implementation |
| 2 | Design Principles | 2 | n/a | - | Rationale — no implementation |
| 3 | 1. Application Shell | 2 | verified | - | App.svelte: topbar + status bar + no-sidebar layout |
| 4 | Top Bar (always visible, all modes) | 3 | implemented | - | Partial — repo-mode decisions badge shows workspace-wide count, not repo-scoped. App.svelte `loadDecisionsCount()` always calls `notificationCount(workspace_id)`; backend `get_notification_count` (users.rs) → `count_unresolved(user_id, workspace_id)` has no repo_id filter. All other top-bar features present (workspace selector, ⌘K search, back arrow, repo path, avatar). Re-verified 2026-09-29. |
| 5 | Status Bar (bottom, always visible) | 3 | verified | - | App.svelte: WebSocket status (connected/offline/connecting), trust level, budget % with color-coded bar, PresenceAvatars with real-time WS updates |
| 6 | 2. Workspace Home | 2 | verified | - | WorkspaceHome.svelte: dashboard landing page with real data loading for decisions, repos, specs, tasks, MRs, agents, budget |
| 7 | Sections | 3 | implemented | - | Partial — Decisions (inline actions, trust filtering) and Repos (cards, health, New/Import) genuine. Missing: Specs cross-repo list, Briefing (removed from template), Agent Rules section. Architecture partial (DependencyGraph only). |
| 8 | Workspace Settings | 3 | verified | - | WorkspaceSettings.svelte: 6 tabs (General, Trust & Policies, Teams, Budget, Compute, Audit) + gear icon access + back arrow |
| 9 | 3. Repo Mode | 2 | verified | - | RepoMode.svelte: TABS array + role=tablist tab bar + tab-content routing to all specced tabs. Re-verified 2026-09-29. |
| 10 | Repo Header | 3 | verified | - | RepoMode header: agent-count button → AgentCardPanel slide-in, budget %, copyable clone URL. Re-verified 2026-09-29. |
| 11 | Tab: Specs (default, landing tab) | 3 | implemented | - | Partial — registry list/filters/New Spec + DetailPanel mini arch canvas + predict/preview loop + Ask Why (interrogation) genuine. Meta-spec binding editor (stale pins) + inline assertion results confirmed only in Architecture/ExplorerView (§12, task-assigned), not the Specs-tab DetailPanel. Re-verified 2026-09-29. |
| 12 | Tab: Architecture (Moldable Development Surface) | 3 | task-assigned | task-178 | ExplorerView exists; needs full moldable surface features |
| 13 | Tab: Decisions | 3 | verified | - | Inbox.svelte: real client-side repo_id filter (scope="repo"). Re-verified 2026-09-29. |
| 14 | Tab: Code | 3 | verified | - | ExplorerCodeTab.svelte: clone URL, branches/commits/files/hot-files/provenance sub-tabs, commit log with agent attribution. MRs + Merge Queue live in separate RepoMode 'mrs' tab (impl divergence from spec sub-tab list). Re-verified 2026-09-29. |
| 15 | Tab: ⚙ (Settings) | 3 | verified | - | RepoSettings.svelte: General/Gates/Policies/Budget/Audit/Danger Zone tabs all present. Re-verified 2026-09-29. |
| 16 | 4. Meta-Spec Management | 2 | implemented | - | MetaSpecs.svelte |
| 17 | 5. Navigation Flows | 2 | implemented | - | App.svelte: onMount entrypoint + popstate |
| 18 | First Visit | 3 | implemented | - | App.svelte: entrypoint flow with localStorage |
| 19 | Daily Flow | 3 | implemented | - | App.svelte: workspace home first |
| 20 | Exception Flow | 3 | implemented | - | App.svelte: decisions badge → scroll to section |
| 21 | Meta-Spec Editing Flow | 3 | implemented | - | App.svelte: goToAgentRules |
| 22 | 6. Keyboard Shortcuts | 2 | implemented | - | App.svelte: handleKeydown with g-key sequences |
| 23 | 7. URL Structure | 2 | implemented | - | App.svelte: parseUrl + urlFor |
| 24 | 8. Responsive Design | 2 | implemented | - | App.svelte: CSS media queries + mobile drawer |
| 25 | Desktop (≥1024px) | 3 | implemented | - | Full layout |
| 26 | Tablet (768-1024px) | 3 | implemented | - | Detail panels as overlays |
| 27 | Mobile (<768px) | 3 | implemented | - | Hamburger drawer, scrollable tabs |
| 28 | 9. What This Replaces | 2 | n/a | - | Documentation — no implementation |
| 29 | From HSI §1 (Navigation Model) | 3 | n/a | - | Documentation |
| 30 | From ui-layout.md §1 (Application Shell) | 3 | n/a | - | Documentation |
| 31 | Preserved (not changed by this spec) | 3 | n/a | - | Documentation |
| 32 | 10. Cross-Workspace View | 2 | implemented | - | CrossWorkspaceHome.svelte + TenantSettings.svelte |
| 33 | Relationship to Existing Specs | 2 | n/a | - | Documentation — no implementation |
