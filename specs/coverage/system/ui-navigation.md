# Coverage: UI Navigation Model

**Spec:** [`system/ui-navigation.md`](../../system/ui-navigation.md)
**Last audited:** 2026-09-29 (§4 Meta-Spec Management group audited this cycle: row 16 Partial re-confirmed with fresh code evidence — surface, publish/approval/history wiring genuine; preview-loop backend structural-only, cascade registry absent. No status change. Other groups previously audited: §1-§3, §5 (rows 3-21, 25-27 re-verified 2026-09-29); rows 22-24, 32 still carry thin undated notes — candidates for a future cycle.)
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
| 16 | 4. Meta-Spec Management | 3 | implemented | - | Partial (audited 2026-09-29 §4 cycle) — full-page Editor Split surface genuine: /workspaces/:slug/agent-rules route with back arrow (App.svelte:247-248, 1229-1252, 1649-1653; "Manage rules" in WorkspaceHome wired via goToAgentRules context); prompt editor + spec-selector checklist + Preview/Iterate/Publish loop (MetaSpecs.svelte:580-750); publish → real PUT /meta-specs-registry/:id (api.js:556-557); editing content resets approval to Pending per spec (meta_specs.rs:1074-1078); version history with inline diff (GET /meta-specs-registry/:id/versions, mod.rs:730-736); Approve/Reject + Required toggle + InlineChat LLM assist + blast radius (workspaces/repos with reasons + arch nav links, meta_specs.rs:255-308). MISSING: (1) preview loop backend is structural-only — POST /workspaces/:id/meta-specs/preview marks all specs "complete" without spawning agents (meta_specs.rs:365-375, comment: "no agents are spawned in this endpoint... future extension"); PreviewRecord has no architecture_diff/specs_diff fields, so the frontend's real-result rendering path (previewApiResult.architecture_diff/specs_diff) never populates and only the simulated fallback renders fabricated diffs — spec preview-loop steps 3-5 (agents on throwaway branches, real code diff) not implemented; (2) registry cascade view absent at workspace scope — left panel is a flat unscoped dropdown of ALL meta-specs (loadWorkspaceData → api.getMetaSpecs()), no tenant-inherited 🔒 grouping, no effective-set summary; (3) impact panel not always-visible (shown only post-preview at workspace scope, behind 'impact' tab at tenant scope); (4) stale pins + drift status not queried — UI shows "coming soon" placeholders, drift section is static text; (5) "Configure drift policy" link absent; (6) reconciliation status after publishing a required meta-spec change absent from this surface. Consider splitting: task for agent-spawning preview + cascade registry. |
| 9 | 3. Repo Mode | 2 | verified | - | RepoMode.svelte: TABS array + role=tablist tab bar + tab-content routing to all specced tabs. Re-verified 2026-09-29. |
| 10 | Repo Header | 3 | verified | - | RepoMode header: agent-count button → AgentCardPanel slide-in, budget %, copyable clone URL. Re-verified 2026-09-29. |
| 11 | Tab: Specs (default, landing tab) | 3 | implemented | - | Partial — registry list/filters/New Spec + DetailPanel mini arch canvas + predict/preview loop + Ask Why (interrogation) genuine. Meta-spec binding editor (stale pins) + inline assertion results confirmed only in Architecture/ExplorerView (§12, task-assigned), not the Specs-tab DetailPanel. Re-verified 2026-09-29. |
| 12 | Tab: Architecture (Moldable Development Surface) | 3 | task-assigned | task-178 | ExplorerView exists; needs full moldable surface features |
| 13 | Tab: Decisions | 3 | verified | - | Inbox.svelte: real client-side repo_id filter (scope="repo"). Re-verified 2026-09-29. |
| 14 | Tab: Code | 3 | verified | - | ExplorerCodeTab.svelte: clone URL, branches/commits/files/hot-files/provenance sub-tabs, commit log with agent attribution. MRs + Merge Queue live in separate RepoMode 'mrs' tab (impl divergence from spec sub-tab list). Re-verified 2026-09-29. |
| 15 | Tab: ⚙ (Settings) | 3 | verified | - | RepoSettings.svelte: General/Gates/Policies/Budget/Audit/Danger Zone tabs all present. Re-verified 2026-09-29. |
| 16 | 4. Meta-Spec Management | 2 | implemented | - | MetaSpecs.svelte |
| 17 | 5. Navigation Flows | 2 | verified | - | App.svelte: onMount entrypoint routing (L892-1002) + popstate handler + full goTo* nav functions (goToWorkspaceHome/goToRepo/goToRepoTab/goToWorkspaceSettings/goToAgentRules/goToProfile/goToCrossWorkspace). Verified 2026-09-29. |
| 18 | First Visit | 3 | implemented | - | Partial — auth→home, localStorage workspace restore (onMount L957-963), land on home without auto-entering repo all present. Missing: "last repo pre-selected in dropdown" — no last-repo persistence (no gyre_repo in localStorage; grep confirmed). Re-audited 2026-09-29. |
| 19 | Daily Flow | 3 | implemented | - | Partial — always workspace-home-first (entrypoint flow restores workspace only, never a repo), decisions badge glance, handle decisions, click repo→Specs tab all present. Missing: "last-used repo remembered/highlighted in Repos section" — no last-repo persistence. Re-audited 2026-09-29. |
| 20 | Exception Flow | 3 | verified | - | Decisions badge (App.svelte L1445-1470): repo mode→goToRepoTab('decisions'), else goToWorkspaceHome + scroll to section-decisions. Decision item nav() → repo-mode entity detail (gate_failure→MR gates tab, WorkspaceHome L1256-1263). Back arrow→home. Verified 2026-09-29. |
| 21 | Meta-Spec Editing Flow | 3 | verified | - | Agent Rules "Manage rules"→goToAgentRules→MetaSpecs workspace preview loop: select spec(s)→api.previewPersona + poll previewPersonaStatus→architecture_diff/specs_diff impact panel→publish + approve via real API (updateMetaSpec). Back arrow→home. Verified 2026-09-29. |
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
