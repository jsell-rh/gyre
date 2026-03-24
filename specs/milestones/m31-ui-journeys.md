# M31: UI Journeys — Inbox, Briefing, System Explorer, and Navigation Restructure

**Status:** Done
**Milestone:** M31

---

## Summary

M31 delivers four journey-oriented UI improvements that shift the Gyre dashboard from a tool-centric layout to an intent-centric one. Engineers start their session at the **Inbox** (what needs attention) or **Briefing** (what happened), explore architecture via the **System Explorer Canvas**, and navigate with restructured, purpose-grouped sidebar sections.

Related specs:
- [specs/system/ui-journeys.md](../system/ui-journeys.md)
- [specs/system/system-explorer.md](../system/system-explorer.md)

---

## M31.1 — Inbox View (PR #341, TASK-178)

`Inbox.svelte` — unified action queue aggregating items that need attention:

- **Pending MR reviews** — MRs in `review` status assigned to the current user
- **Pending spec approvals** — specs in `Pending` state from the spec ledger
- **Gate failures** — recent `GateFailure` domain events

**Behavior:**
- Badge count on sidebar "Inbox" icon shows total unread items
- Mark-as-seen state persisted to `localStorage` (keyed per item ID)
- Auto-refreshes every 60 seconds
- Keyboard shortcut: `i` opens Inbox from anywhere in the app

**API:** no new endpoints — consumes existing `GET /api/v1/merge-requests`, `GET /api/v1/specs/pending`, and activity feed.

---

## M31.2 — Briefing View (PR #341, TASK-178)

`Briefing.svelte` — narrative digest of what happened since last visit:

4-card grid layout:
1. **Active agents** — count + list of currently running agents
2. **Spec health** — approved vs pending vs drifted counts
3. **Since last visit** — MRs merged, tasks completed, agents spawned since `localStorage`-persisted last-visit timestamp
4. **Gate failures** — summary of recent gate failures with repo/MR context

**Behavior:**
- Last-visit timestamp stored in `localStorage` (`gyre_last_briefing_visit`)
- Keyboard shortcut: `b` opens Briefing from anywhere in the app

**Note:** distinct from `GET /api/v1/workspaces/{id}/briefing` (which returns structured JSON for the workspace architectural summary). The Briefing view is a human-facing narrative digest.

**API:** no new endpoints — consumes existing activity, agent, spec, and gate APIs.

---

## M31.3 — System Explorer Canvas (PR #345, TASK-177)

`ExplorerView.svelte` + `ExplorerCanvas.svelte` + `MoldableView.svelte` — live architecture visualization:

**MoldableView** offers three modes (toggled via pill buttons):

| Mode | Component | Description |
|---|---|---|
| **Graph** | `ExplorerCanvas.svelte` | SVG pan+zoom canvas; nodes as colored shapes by type |
| **List** | inline table | Sortable/filterable table of all graph nodes |
| **Timeline** | stub | Architectural timeline scrubber (Phase 4, not yet implemented) |

**ExplorerCanvas node rendering** — 9 node types with distinct colors and shapes:

| Node type | Color | Shape |
|---|---|---|
| `package` | blue | rounded rect |
| `module` | teal | rect |
| `type` | green | rect |
| `interface` | purple | diamond |
| `function` | orange | rect |
| `endpoint` | red | rect |
| `component` | pink | rect |
| `table` | brown | rect |
| `constant` | gray | rect |

**Click a node** → slide-in detail panel with: file path, spec linkage (`spec_path` + `spec_confidence`), visibility, doc comment, complexity, `churn_count_30d`.

**Repo selector** at top of ExplorerView — fetches `GET /api/v1/repos/{id}/graph` for the selected repo.

**Route:** `/explorer` (sidebar: "System Explorer" under Source Control section).

---

## M31.4 — Navigation Restructure (PR #341)

Sidebar groups reorganized for intent-clarity:

| Section | Views |
|---|---|
| Overview | Dashboard, Inbox, Briefing, Meta-Specs, Profile |
| Source Control | Repos, Branches, Spec Graph, Dependencies, System Explorer |
| Agents | Agents, Personas, Compose |
| Operations | Merge Queue, Budget, Analytics, Costs |
| Admin | Admin Panel, Audit, Settings |

Keyboard shortcuts added:
- `i` → Inbox
- `b` → Briefing

---

## Acceptance Criteria

- [x] Inbox view shows pending MR reviews, spec approvals, gate failures; badge count on sidebar
- [x] Inbox mark-as-seen persists to localStorage across page reload
- [x] Inbox auto-refreshes every 60s
- [x] Briefing view renders 4-card digest (agents, spec health, since-last-visit, gate failures)
- [x] Keyboard shortcut `i` navigates to Inbox
- [x] Keyboard shortcut `b` navigates to Briefing
- [x] System Explorer Canvas renders SVG graph with 9 node type shapes
- [x] Click node → detail panel with file/spec/complexity metadata
- [x] MoldableView toggle switches between Graph/List/Timeline modes
- [x] Repo selector populates from existing repos
- [x] All new views reachable via sidebar nav and direct URL (`/inbox`, `/briefing`, `/explorer`)
- [x] 23 new vitest tests passing (Inbox: 5, Briefing: 5, ExplorerCanvas: 13)

---

## Implementation Notes

- `web/src/components/Inbox.svelte` — action queue component
- `web/src/components/Briefing.svelte` — narrative digest component
- `web/src/components/ExplorerView.svelte` — top-level explorer page (repo selector)
- `web/src/lib/ExplorerCanvas.svelte` — SVG canvas with pan/zoom
- `web/src/lib/MoldableView.svelte` — mode switcher wrapping ExplorerCanvas + list table
- `web/src/__tests__/Inbox.test.js`, `Briefing.test.js`, `ExplorerCanvas.test.js`
- Routes added to `App.svelte`: `inbox`, `briefing`, `explorer`
