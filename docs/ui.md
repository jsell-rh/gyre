# Gyre Dashboard UI

The Svelte SPA at `GET /*` is served from `web/dist/`. Access at `http://localhost:3000`.

**Navigation is path-based** -- navigate directly to `/<view>` in the URL bar or click the sidebar. Valid view paths: `dashboard`, `activity`, `agents`, `tasks`, `projects`, `merge-queue`, `mcp-catalog`, `compose`, `analytics`, `costs`, `audit`, `spec-approvals`, `specs`, `admin`, `settings`, `workspaces`, `personas`, `budget`, `dependencies`, `spec-graph`, `profile`, `inbox`, `briefing`, `explorer`, `meta-specs`. Keyboard shortcuts (M31): `i` -> Inbox, `b` -> Briefing. Browser back/forward buttons work correctly via `history.pushState`/`popstate`. Legacy `#<view>` hash URLs are still supported on initial load for backwards compatibility.

**Entity deep-link URLs (M28)** -- entity-scoped URLs restore a specific record on direct navigation: `/repos/:id`, `/tasks/:id`, `/merge-requests/:id`, `/workspaces/:id`.

---

## Views

- **Inbox** (M31, sidebar: "Inbox" under Overview, shortcut `i`): unified action queue aggregating items that need attention -- pending MR reviews, pending spec approvals, gate failures. Badge count on sidebar icon shows unread items. Mark-as-seen state persisted to `localStorage`; auto-refreshes every 60s.

- **Briefing** (M31, sidebar: "Briefing" under Overview, shortcut `b`): narrative digest since last visit. 4-card grid: active agents, spec health, since-last-visit stats, gate failures. (Distinct from `GET /api/v1/workspaces/{id}/briefing` which returns structured JSON.)

- **System Explorer Canvas** (M31, sidebar: "System Explorer" under Source Control, route `/explorer`): `MoldableView` switcher with three modes -- **Graph** (SVG pan+zoom canvas via `ExplorerCanvas.svelte`; 9 node type shapes with distinct colors: package/module/type/interface/function/endpoint/component/table/constant; click a node to open detail panel with file path, spec linkage, visibility, doc comment, complexity, churn metrics), **List** (sortable/filterable table of all nodes), **Timeline** (architectural timeline scrubber). Repo selector at top; fetches `GET /api/v1/repos/{id}/graph`.

- **Meta-Specs** (M32, sidebar: "Meta-Specs" under Overview, route `/meta-specs`): card grid of specs with `kind` badges (`meta:persona`, `meta:principle`, `meta:standard`, `meta:process`), approval status, and blast-radius modal showing affected workspaces and repos. Calls `GET /api/v1/specs?kind=<kind>` and `GET /api/v1/meta-specs/{path}/blast-radius`.

- **Dashboard Home** (M8.1 + M9.2): metric cards (active agents, open tasks, pending MRs, queue depth), agent health grid, recent activity feed, merge queue progress bar. Quick Actions row: New Project, New Task, Seed Demo Data buttons.

- **Design System** (M8.1): Red Hat brand CSS variables in `web/src/lib/design-system.css` -- dark theme with `gray-95` (#151515) background, `red-50` (#ee0000) primary actions. 13-component library: `Button`, `Badge`, `Card`, `Table`, `Input`, `Modal`, `Toast`, `Tabs`, `Skeleton`, `EmptyState`, `Breadcrumb`, `SearchBar`.

- **Sidebar** (M8.1): grouped nav sections (Overview / Source Control / Agents / Operations / Admin), collapsible to icon-only mode via chevron toggle, server status footer.

- **Global Search** (M8.1): Cmd+K opens `SearchBar` overlay with keyboard navigation across agents, tasks, repos, and MRs.

- **Activity Feed** (M8.2): timeline layout with colored event-type nodes, multi-select filter pills, relative timestamps, skeleton loading.

- **Agent List** (M8.2 + M11.2 + M19.3): 3-column card grid with table-view toggle, status filter pills, skeleton grid on load, slide-in detail panel with tabbed Info/Logs/Terminal view. Container-spawned agents show container metadata. Logs tab with live SSE streaming; Terminal tab streams live PTY output via `/ws/agents/{id}/tty`.

- **Task Board** (M8.2 + M9.2 + M20): kanban columns by status, "New Task" button opens Modal -> POST `/api/v1/tasks`. Cards are clickable -- navigate to Task Detail view with Info and Artifacts tabs.

- **Project List** (M8.2 + M9.2): responsive card grid, "New Project" button -> POST `/api/v1/projects`. Selecting a project shows "Add Repo" button -> POST `/api/v1/repos`.

- **Repo Detail** (M8.2 + M20): tabs for Branches, Commits, Diff, Gates, Activity, Policy. Policy tab: ABAC policy editor + spec-policy toggles. Activity tab: hot files + per-line blame. Gates tab: quality gate table + push-gate toggles. Commits tab: agent attribution + Ed25519 signature badge. Branches tab: speculative merge status badge.

- **Merge Request Detail** (M8.3 + M12.3 + M20): two-column layout -- diff panel left (side-by-side view with syntax highlighting), metadata + status timeline right. Dependencies panel in sidebar; spec_ref chip; atomic_group badge.

- **Merge Queue View** (M8.3 + M20): visual flow lanes with progress bars and cancel buttons. DAG toggle switches to dependency graph view.

- **Spec Dashboard** (M21.2, sidebar: "Specs" under Source Control): spec registry view with stats cards, filter pills, spec table, and slide-in detail panel (Info/History/Links tabs). Approve/Revoke buttons with modal confirmation.

- **Workspace List** (M22.5, sidebar: "Workspaces" under Overview): workspace switcher grid with create-workspace modal.

- **Workspace Detail** (M22.5): budget usage progress bars; tabs for Repos, Members (invite/remove with WorkspaceRole), Teams.

- **Persona Catalog** (M22.5, sidebar: "Personas" under Agents): card grid with scope badge, capabilities, model/temperature metadata. Scope ID field adapts to selected scope kind.

- **Budget Dashboard** (M22.5, sidebar: "Budget" under Operations): tenant-wide summary cards + per-workspace breakdown with usage progress bars.

- **Dependency Graph** (M22.5, sidebar: "Dependencies" under Source Control): SVG circular layout of cross-repo dependency edges; click node to open blast-radius panel.

- **Spec Graph** (M22.5, sidebar: "Spec Graph" under Source Control): SVG DAG of SpecLink records with link-type colored edges + legend.

- **User Profile** (M22.5, sidebar: "My Profile" under Overview): profile edit form; tabs for My Agents, My Tasks, My MRs, Notifications.

- **Global Workspace Selector** (M28, topbar): dropdown persists to `localStorage` (`gyre_selected_workspace_id`); filters AgentList, TaskBoard, and ProjectList.

- **Auth Token UI** (M9.3 + M20): auth status dot in topbar. Click opens Token modal to view/change token and display token kind, agent ID, task ID, scope, and expiry.

- **Admin Panel** (M4.3 + M8.3, Admin role required): tab-based navigation (Health / Jobs / Audit / Agents / SIEM / Compute / Network / Snapshots / Retention / BCP). Admin Panel requires `Admin` role via Keycloak JWT (`GYRE_OIDC_ISSUER`) or the global `GYRE_AUTH_TOKEN`.

- **Analytics View** (M6.1): event counts bar chart and recent events list with property drill-down.

- **Cost View** (M6.1): agent cost breakdown table with total display and per-agent detail drill-down.

- **Audit View** (M7.1 + M20, sidebar: "Audit"): two-tab view -- Live Stream tab (SSE from `GET /api/v1/audit/stream`); History tab (filtered query). Aggregate stats card shows event counts by type.

- **MCP Tool Catalog** (M5.1 + M8.3, sidebar: "MCP Tools"): card grid with collapsible JSON schema per tool.

- **Compose View** (M5.2 + M8.3, sidebar: "Compose"): mono textarea editor for agent-compose spec (JSON or YAML), interactive agent state tree, teardown button.

- **Settings** (M8.3): server info card, pulsing WebSocket connection indicator, configuration reference table, language selector.
