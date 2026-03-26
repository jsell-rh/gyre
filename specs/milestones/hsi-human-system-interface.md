# HSI: Human-System Interface

**Status:** Done
**Milestone:** HSI (Human-System Interface)

---

## Summary

HSI defines and implements how humans interact with Gyre's fully autonomous development system. When agents write all code, the human role changes: you decide what to build (specs), set meta-spec direction, maintain understanding of what exists (exploration), handle exceptions (escalation), and steer agents in-flight (communication). Every surface exists to serve one of those five activities.

Implemented across 29 PRs in 4 workstreams (WS1–WS4), ~130 minutes of parallel execution.

Related spec: [specs/system/human-system-interface.md](../system/human-system-interface.md)

---

## WS1: Server Foundation (7 PRs: #395, #398, #402, #403, #415, #419, #423)

- **Trust Gradient** — per-agent trust levels (Spectator/Reviewer/Developer/Admin); ABAC amendments for trust-scoped endpoints; trust delete endpoint scoped to workspace
- **Notification System** — 16 notification types; `NotificationRepository` port + Diesel adapter; SSE streaming; workspace-scoped notification API
- **User Workspace State** — `UserWorkspaceState` entity (last-visit, unread counts, preferences); `GET/PUT /api/v1/workspaces/{id}/user-state`
- **Presence** — lightweight presence tracking (heartbeat, online/offline, last_seen); `GET /api/v1/workspaces/{id}/presence`
- **LLM Rate Limiter** — token bucket per workspace; `POST /api/v1/workspaces/{id}/llm-request`; configurable via `GYRE_LLM_RATE_LIMIT`
- **MR Timeline** — per-MR event log (opened, commit pushed, gate result, review comment, merged); `GET /api/v1/merge-requests/{id}/timeline`
- **Divergence Detection** — tracks agent/workspace config divergence; threshold via `GYRE_DIVERGENCE_THRESHOLD=3`

## WS2: Agent Intelligence (6 PRs: #405, #413, #417, #420, #421, #422)

- **Conversation Provenance** — links agent messages to source commits/specs; `ConversationRepository` port; `POST /api/v1/agents/{id}/conversations`
- **Completion Summaries** — structured agent completion reports; `GET /api/v1/agents/{id}/summary`
- **Interrogation Agents** — spawn ephemeral question-answering sub-agents against a codebase context; `POST /api/v1/workspaces/{id}/interrogate`
- **Gate-Time Traces** — capture timing + output for each gate execution; `GET /api/v1/merge-requests/{id}/gate-traces`
- **S2.6 Divergence Detection** — final slice wiring divergence threshold and detection loop

## WS3: LLM Backend (5 PRs: #397, #399, #400, #414, #418)

- **Explorer Views** — `ExplorerView` entity (saved graph lens configs); `GET/POST/DELETE /api/v1/workspaces/{id}/explorer-views`
- **Briefing Backend** — narrative 4-card digest generation; `GET /api/v1/workspaces/{id}/briefing`; SSE streaming Q&A (`GET /api/v1/workspaces/{id}/briefing/ask`)
- **Spec Editing Backend** — LLM-assisted spec editing; `POST /api/v1/specs/{path}/assist`
- **Cross-Workspace Spec Links** — `SpecLink` across workspace boundaries; enforced on merge

## WS4: Frontend (12 PRs: #393, #394, #401, #404, #406–#412, #416)

Full Svelte 5 UI rewrite — **6-view model:**

| View | Route | Description |
|---|---|---|
| Inbox | `/inbox` | Unified action queue: pending reviews, spec approvals, gate failures; badge count; mark-as-seen; 60s auto-refresh |
| Briefing | `/briefing` | Narrative 4-card digest since last visit; SSE Q&A chat panel |
| Explorer | `/explorer` | Progressive graph explorer: C4 architecture view, LLM concept lens, flow animation, scope breadcrumb; SVG + ELK/dagre layout |
| Specs | `/specs` | Sortable spec table, progress bars, LLM-assisted inline editing |
| Meta-Specs | `/meta-specs` | Kind-filtered card grid (persona/principle/standard/process), blast-radius modal |
| Admin | `/admin` | Trust level selector, policies tab, workspace/repo settings, SIEM/compute/network panels |

**Additional components:** `PresenceAvatars`, `InlineChat`, `FlowCanvas`, `ScopeBreadcrumb`, `ContentArea`, `DetailPanel`, `ViewEvent` system.

---

## Environment Variables Added

| Variable | Default | Description |
|---|---|---|
| `GYRE_DIVERGENCE_THRESHOLD` | `3` | Number of config diffs before workspace flagged as diverged |
| `GYRE_LLM_RATE_LIMIT` | _(unset)_ | Token bucket limit for LLM requests per workspace per minute |

---

## Acceptance Criteria

- [x] Trust gradient with 4 levels; ABAC enforced on trust-scoped endpoints
- [x] 16 notification types; SSE streaming; unread badge count
- [x] User workspace state persisted (last-visit, preferences)
- [x] Presence heartbeat and online/offline status
- [x] MR timeline with ordered event log
- [x] Gate-time traces captured and queryable
- [x] Conversation provenance links messages to commits/specs
- [x] Completion summaries on agent completion
- [x] Interrogation agent spawn endpoint
- [x] Explorer views saved and restored
- [x] Briefing narrative digest + SSE Q&A
- [x] Spec editing backend (LLM-assisted)
- [x] Cross-workspace spec links enforced on merge
- [x] 6-view Svelte UI: Inbox / Briefing / Explorer / Specs / Meta-Specs / Admin
- [x] 50 Playwright E2E tests covering all UI views
- [x] 47 new server-side integration tests

---

## Implementation Notes

- 29 PRs merged in ~130 minutes across 4 parallel workstreams
- `GYRE_DIVERGENCE_THRESHOLD` env var controls divergence detection sensitivity
- Explorer uses SVG rendering with ELK hierarchical layout + dagre fallback
- `FlowCanvas.svelte` drives animated flow traces over the architecture graph
- Briefing SSE endpoint streams narrative generation token-by-token
