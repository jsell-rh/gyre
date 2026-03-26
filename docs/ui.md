# Gyre Dashboard UI

The Svelte SPA at `GET /*` is served from `web/dist/`. Access at `http://localhost:3000`.

> **HSI rewrite (S4.1-S4.8):** The UI was restructured from 25+ views to **6 focused journey views**. Old route paths (`/dashboard`, `/agents`, `/tasks`, etc.) are no longer the primary navigation model.

---

## Navigation

**Six primary views** (sidebar, keyboard shortcuts `⌘1` through `⌘6`):

| Shortcut | View | Route |
|---|---|---|
| `⌘1` | Inbox | `/inbox` |
| `⌘2` | Briefing | `/briefing` |
| `⌘3` | Explorer | `/explorer` |
| `⌘4` | Specs | `/specs` |
| `⌘5` | Meta-specs | `/meta-specs` |
| `⌘6` | Admin | `/admin` |

**Scope model:** A `ScopeBreadcrumb` in the topbar shows the active scope (Tenant / Workspace / Repo). Views are scope-aware -- they adapt their content to the currently selected tenant, workspace, or repo. Clicking breadcrumb segments navigates between scopes.

**Global overlays:** `⌘K` opens global search; `?` opens keyboard shortcuts overlay; `/` focuses search within the current view.

**Status bar** (24px footer): trust level, budget usage progress bar, presence avatars (workspace scope only), WebSocket connection status.

---

## Views

### Inbox (HSI S4.2, `⌘1`)
Unified action queue. Renders HSI notification cards by type (10 types: gate failures, MR reviews, spec approvals, divergence alerts, etc.). Accordion grouping by priority. Badge count on sidebar icon shows unresolved items. Calls `GET /api/v1/workspaces/{id}/notifications`. Inline approve/reject + gate-retry buttons. Workspace-scoped.

### Briefing (HSI S4.3, `⌘2`)
Narrative digest. Structured sections: active agents, spec health, since-last-visit stats, gate failures. SSE Q&A chat panel (asks questions answered by LLM via `POST /api/v1/workspaces/{id}/briefing/ask` SSE). Calls `GET /api/v1/workspaces/{id}/briefing`. Time-range selector. Workspace-scoped.

### Explorer (HSI S4.4, `⌘3`)
Scope-aware architecture explorer. Rendering adapts to the active scope:

- **Tenant scope** -- `WorkspaceCards.svelte`: grid of workspace summary cards with agent count, spec health, budget usage.
- **Workspace scope** -- `FlowCanvas.svelte` (particle animation, Canvas 2D with WebGL fallback): animated agent flow graph showing live agent interactions.
- **Repo scope** -- `ExplorerCanvas.svelte` (d3-force + ELK layouts): SVG node graph of the repo's knowledge graph. 9 node type shapes with distinct colors (package/module/type/interface/function/endpoint/component/table/constant). Click node to open `DetailPanel` with file path, spec linkage, visibility, doc comment, complexity, churn metrics.

Controls (`ExplorerControls.svelte`): lens/view selector, LLM "Ask" SSE chat (`GET /api/v1/workspaces/{id}/explorer-views`), filter panel, Code tab. Filter panel lets you filter by node type, complexity threshold, churn. Saved views persisted via `POST /api/v1/workspaces/{id}/explorer-views`.

### Specs (HSI S4.5, `⌘4`)
Spec registry + inline LLM-assisted editing. Sortable table with progress bars showing spec completion status. Calls `GET /api/v1/specs`. Slide-in `DetailPanel` with Info / History / Links tabs. Inline spec editor with LLM assist SSE (`POST /api/v1/specs/{path}/assist`), save (`PUT /api/v1/specs/{path}/save`), and prompt library (`GET /api/v1/specs/assist-prompts`). Approve/Revoke/Reject buttons. Cross-workspace links panel.

### Meta-specs (HSI S4.6, `⌘5`)
Persona catalog + meta-spec management. Shows personas (M22.5 catalog), meta:principle, meta:standard, meta:process specs. Preview loop cycles through 3 approval states. Cards show `kind` badge, approval status, version history. Blast-radius modal. Persona versioning: approve/resolve via `POST /api/v1/personas/{id}/approve` and `POST /api/v1/personas/{id}/resolve`.

### Admin (M4.3 + M8.3, `⌘6`)
Admin role required. Tab-based navigation: Health / Jobs / Audit / SIEM / Compute / Network / Snapshots / Retention / BCP / Tenants. Tenants tab: `TenantList.svelte` -- create/list/delete tenants (M34). Admin Panel requires `Admin` role via Keycloak JWT (`GYRE_OIDC_ISSUER`) or the global `GYRE_AUTH_TOKEN`.

---

## Shared Components (HSI S4.8)

- **`DetailPanel.svelte`** -- slide-in right panel for entity drill-in; used across Explorer, Specs, Inbox.
- **`InlineChat.svelte`** -- SSE-driven LLM chat widget embedded in Specs and Explorer views.
- **`PresenceAvatars.svelte`** -- live presence avatars in status bar; updates via WebSocket presence events.
- **`ViewEvent` / `viewEvents.js`** -- typed event bus for cross-component communication within a view.
- **`MoldableView.svelte`** -- view-mode switcher (graph / list / timeline / flow).
- **`FlowCanvas.svelte` / `FlowRenderer.svelte`** -- Canvas 2D animated flow graph with WebGL fallback and particle simulation.
- **`ScopeBreadcrumb.svelte`** -- topbar tenant/workspace/repo breadcrumb with click-to-navigate.
- **`ContentArea.svelte`** -- layout manager that adapts between canvas (full bleed) and canvas-controls (controls sidebar) layouts.

---

## Design System

Red Hat brand CSS variables in `web/src/lib/design-system.css` -- dark theme with `gray-95` (#151515) background, `red-50` (#ee0000) primary. Component library: `Button`, `Badge`, `Card`, `Table`, `Input`, `Modal`, `Toast`, `Tabs`, `Skeleton`, `EmptyState`, `Breadcrumb`, `SearchBar`, `NodeBadge`, `StatusBadge`, `DiffSuggestion`.

---

## Auth Token UI

Auth status in topbar user menu. Click opens token/sign-out controls showing token kind, agent ID, task ID, scope, and expiry.
