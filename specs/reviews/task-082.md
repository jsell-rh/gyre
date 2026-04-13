# Review: task-082 — HSI Stable Sidebar Navigation Model

## R1 Findings

- [ ] **F1: `activeSidebarItem` maps repo-mode `code` tab to `specs` instead of `explorer`.** The HSI spec §1.3 explicitly states: "At repo scope, the Explorer has two tabs in its control bar: **Architecture** (default — C4 graph) and **Code** (branches, commits, MRs, merge queue). The Code tab is part of the Explorer, not a separate nav item." In `App.svelte:786`, `repoTab === 'code'` falls through the default case (`return 'specs'`) because the `code` tab is not matched by any `if` branch. The comment on line 786 even explicitly lists `code` in the fallthrough: `// tasks, mrs, agents, code — fall to specs as closest match`. Since the spec defines Code as part of the Explorer, the sidebar should highlight **Explorer** when the Code tab is active. Fix: add `if (repoTab === 'code') return 'explorer';` alongside the existing `architecture`/`dependencies` branch.

- [ ] **F2: Missing server version indicator in sidebar footer.** `ui-layout.md` §1 (explicitly referenced by the task plan: "see `ui-layout.md` §1 for sidebar dimensions and layout constraints") defines the Sidebar structure as: "Six nav items, always in this order, always present. Active item highlighted. At the bottom: **server version indicator**." The `Sidebar.svelte` footer (`sidebar-footer` div) contains only the collapse toggle button. The server version indicator is absent. This is a missing spec-required element per the companion layout spec.

## R2 Findings

- [ ] **F3: `mrs` repoTab also maps to `specs` instead of `explorer`.** Same root cause as F1 but a distinct code path. The HSI spec §1.3 says twice that MRs belong to the Explorer: (1) line 57: "At repo scope, the Explorer has two tabs in its control bar: **Architecture** (default — C4 graph) and **Code** (branches, commits, MRs, merge queue)." (2) line 66: "The Code tab (branches, commits, MRs, merge queue) is accessed via the Explorer at repo scope, not as a separate nav item." In `App.svelte:786-787`, `repoTab === 'mrs'` falls through the default `return 'specs'` because no `if` branch matches it. Since MRs are explicitly part of the Explorer's Code tab, the sidebar should highlight **Explorer** when viewing MRs. The F1 fix suggestion (`if (repoTab === 'code') return 'explorer';`) will not cover `mrs` unless expanded — fix should be `if (repoTab === 'code' || repoTab === 'mrs') return 'explorer';` or the fallthrough default should change.

## R3 Findings

F1, F2, and F3 are all still present — verified in code at the same locations. No fixes have been applied.

- [ ] **F4: Active sidebar item stays on Inbox when clicking Briefing, Explorer, or Specs at workspace scope.** At workspace scope, `handleSidebarNavigate` for `briefing`, `explorer` (with a current workspace), and `specs` all call `goToWorkspaceHome(currentWorkspace)` and then scroll to the relevant section (`App.svelte:812-823`). `goToWorkspaceHome` sets `mode = 'workspace_home'` (`App.svelte:284`). The `activeSidebarItem` derived value for `workspace_home` mode unconditionally returns `'inbox'` (`App.svelte:772`). Result: clicking Briefing scrolls to the briefing section but the sidebar still highlights **Inbox**, not **Briefing**. Same for Explorer and Specs. This violates the acceptance criterion "Active sidebar item is visually highlighted" — the active item is NOT the one the user just clicked. Only Meta-specs and Admin correctly update the sidebar at workspace scope (they navigate to distinct modes: `agent_rules` and `workspace_settings`). Fix: track which sidebar item was selected at workspace scope (e.g., a `workspaceActiveSection` state variable set by `handleSidebarNavigate`) and use it in `activeSidebarItem` when `mode === 'workspace_home'`.

## R4 Findings

F1, F2, F3, and F4 are all still present — verified in code at the same locations (commit `dcf33e5c`). No fix commits have been applied after the implementation commit.

No new findings. The 4 open findings cover the spec violations in the current implementation.
