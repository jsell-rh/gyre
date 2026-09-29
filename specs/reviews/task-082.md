# Review: task-082 — HSI Stable Sidebar Navigation Model

## R1 Findings

- [-] [process-revision-complete] **F1: `activeSidebarItem` maps repo-mode `code` tab to `specs` instead of `explorer`.** The HSI spec §1.3 explicitly states: "At repo scope, the Explorer has two tabs in its control bar: **Architecture** (default — C4 graph) and **Code** (branches, commits, MRs, merge queue). The Code tab is part of the Explorer, not a separate nav item." In `App.svelte:786`, `repoTab === 'code'` falls through the default case (`return 'specs'`) because the `code` tab is not matched by any `if` branch. The comment on line 786 even explicitly lists `code` in the fallthrough: `// tasks, mrs, agents, code — fall to specs as closest match`. Since the spec defines Code as part of the Explorer, the sidebar should highlight **Explorer** when the Code tab is active. Fix: add `if (repoTab === 'code') return 'explorer';` alongside the existing `architecture`/`dependencies` branch.

- [-] [process-revision-complete] **F2: Missing server version indicator in sidebar footer.** `ui-layout.md` §1 (explicitly referenced by the task plan: "see `ui-layout.md` §1 for sidebar dimensions and layout constraints") defines the Sidebar structure as: "Six nav items, always in this order, always present. Active item highlighted. At the bottom: **server version indicator**." The `Sidebar.svelte` footer (`sidebar-footer` div) contains only the collapse toggle button. The server version indicator is absent. This is a missing spec-required element per the companion layout spec.

## R2 Findings

- [-] [process-revision-complete] **F3: `mrs` repoTab also maps to `specs` instead of `explorer`.** Same root cause as F1 but a distinct code path. The HSI spec §1.3 says twice that MRs belong to the Explorer: (1) line 57: "At repo scope, the Explorer has two tabs in its control bar: **Architecture** (default — C4 graph) and **Code** (branches, commits, MRs, merge queue)." (2) line 66: "The Code tab (branches, commits, MRs, merge queue) is accessed via the Explorer at repo scope, not as a separate nav item." In `App.svelte:786-787`, `repoTab === 'mrs'` falls through the default `return 'specs'` because no `if` branch matches it. Since MRs are explicitly part of the Explorer's Code tab, the sidebar should highlight **Explorer** when viewing MRs. The F1 fix suggestion (`if (repoTab === 'code') return 'explorer';`) will not cover `mrs` unless expanded — fix should be `if (repoTab === 'code' || repoTab === 'mrs') return 'explorer';` or the fallthrough default should change.

## R3 Findings

F1, F2, and F3 are all still present — verified in code at the same locations. No fixes have been applied.

- [-] [process-revision-complete] **F4: Active sidebar item stays on Inbox when clicking Briefing, Explorer, or Specs at workspace scope.** At workspace scope, `handleSidebarNavigate` for `briefing`, `explorer` (with a current workspace), and `specs` all call `goToWorkspaceHome(currentWorkspace)` and then scroll to the relevant section (`App.svelte:812-823`). `goToWorkspaceHome` sets `mode = 'workspace_home'` (`App.svelte:284`). The `activeSidebarItem` derived value for `workspace_home` mode unconditionally returns `'inbox'` (`App.svelte:772`). Result: clicking Briefing scrolls to the briefing section but the sidebar still highlights **Inbox**, not **Briefing**. Same for Explorer and Specs. This violates the acceptance criterion "Active sidebar item is visually highlighted" — the active item is NOT the one the user just clicked. Only Meta-specs and Admin correctly update the sidebar at workspace scope (they navigate to distinct modes: `agent_rules` and `workspace_settings`). Fix: track which sidebar item was selected at workspace scope (e.g., a `workspaceActiveSection` state variable set by `handleSidebarNavigate`) and use it in `activeSidebarItem` when `mode === 'workspace_home'`.

## R4 Findings

F1, F2, F3, and F4 are all still present — verified in code at the same locations (commit `dcf33e5c`). No fix commits have been applied after the implementation commit.

No new findings. The 4 open findings cover the spec violations in the current implementation.

## R5 Resolution

All four findings are fixed in code. The implementation (`cda91d38`) reconstructs the stable 6-item sidebar on the worker branch and addresses every finding:

- [x] **F1: repo `code` tab → Explorer.** `App.svelte` `activeSidebarItem` now maps `repoTab === 'code'` to `explorer` (the repo-mode branch explicitly enumerates every `REPO_TABS` value). Test: `AppShell.test.js` "highlights Explorer when the repo Code tab is active".
- [x] **F2: server version indicator.** `Sidebar.svelte` renders a `sidebar-version` element in the footer, fed by a `serverVersion` prop; `App.svelte` fetches `api.version()` on mount and passes it through. Tests: `Sidebar.test.js` version-indicator tests.
- [x] **F3: repo `mrs` tab → Explorer.** The same enumeration also maps `repoTab === 'mrs'` (and `agents`) to `explorer`. Test: `AppShell.test.js` "highlights Explorer when the repo MRs tab is active".
- [x] **F4: workspace-scope active item.** New `workspaceActiveSection` state tracks the clicked section; `activeSidebarItem` returns it for `workspace_home` mode; `handleSidebarNavigate` and the SearchBar section-nav set it, and `goToWorkspaceHome` resets it to `inbox`. Tests: `AppShell.test.js` "highlights Briefing/Specs after clicking … at workspace scope" and "returns highlight to Inbox".

## R6 Findings

F1, F2, F3, F4 remain resolved in the current implementation (`d99ace7b`) — verified in code.

- [-] [process-revision-complete] **F5: repo-scope Briefing click highlights Inbox instead of Briefing (incomplete F4 fix-class exhaustion).** The F4 fix added `workspaceActiveSection = 'briefing'` (and `'explorer'`/`'specs'`) at the **workspace-scope** branch of `handleSidebarNavigate` (`App.svelte:822-836`), so clicking Briefing at workspace scope correctly highlights Briefing. But the **repo-scope** branch was not swept: `case 'briefing': goToWorkspaceHome(currentWorkspace); return;` (`App.svelte:812`) calls `goToWorkspaceHome`, which unconditionally sets `workspaceActiveSection = 'inbox'` (`App.svelte:293`) and never re-sets it to `'briefing'`. Result: clicking the Briefing sidebar item while in repo mode lands on workspace home with the sidebar highlighting **Inbox**, not Briefing. This violates the acceptance criterion "Active sidebar item is visually highlighted" for the clicked item — the identical defect class F4 addressed, applied incompletely. All other repo-mode sidebar items highlight correctly (inbox→decisions→inbox, specs→specs, explorer→architecture→explorer, admin→settings→admin, meta-specs→agent_rules→meta-specs); only `briefing` is broken. Fix: set `workspaceActiveSection = 'briefing'` before returning in the repo-mode `briefing` case (mirroring line 824). Add a test: at `/workspaces/payments/r/core/specs`, click the Briefing sidebar item, assert the active item becomes `briefing`.

## R7 Findings

F1, F2, F3, F4 remain resolved in the current implementation (`d99ace7b`) — verified in code.

**F5 remains OPEN — not fixed.** Verified on disk at `App.svelte:812`: the repo-mode `briefing` case is still `case 'briefing': goToWorkspaceHome(currentWorkspace); return;` with no `workspaceActiveSection = 'briefing'` assignment. `goToWorkspaceHome` unconditionally sets `workspaceActiveSection = 'inbox'` (`App.svelte:293`). Clicking the Briefing sidebar item while in repo mode still lands on workspace home highlighting **Inbox**, not Briefing — and, unlike the workspace-scope branch (lines 822-836), does not scroll to `section-briefing`. The repo-scope branch (line 812) was still not swept. Fix unchanged: set `workspaceActiveSection = 'briefing';` before the `return` on line 812 (mirroring line 824), and add the repo-scope regression test described in F5.

## R8 Resolution

- [x] **F5: repo-scope Briefing click now highlights Briefing.** The repo-scope branch of `handleSidebarNavigate` (`App.svelte:812-817`) was swept: `case 'briefing'` now calls `goToWorkspaceHome(currentWorkspace)`, then re-sets `workspaceActiveSection = 'briefing'` and scrolls to `section-briefing` — mirroring the workspace-scope branch (lines 822-826). `goToWorkspaceHome`'s reset to `'inbox'` is overridden by the subsequent assignment. Regression test: `AppShell.test.js` "highlights Briefing after clicking it at repo scope" — starts at repo scope (`/workspaces/payments/r/core/specs`), asserts the Specs tab highlights Specs, clicks the Briefing sidebar item, and asserts the highlight becomes Briefing. The test fails against the pre-fix code (highlight stays Inbox).
