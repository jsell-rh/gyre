---
title: "HSI Scoped Inline Chat + Hard Interrupt"
spec_ref: "human-system-interface.md §4 Scoped Inline Chat + Hard Interrupt"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "human-system-interface.md §4 Scoped Inline Chat"
  - "human-system-interface.md §4 Hard Interrupt"
commits: []
---

## Spec Excerpt

### Scoped Inline Chat

Chat is **scoped to a context** — you always know what you're talking to and where the message goes.

| Context | Recipient | Where It Appears | Use Case |
|---|---|---|---|
| Agent detail panel | Specific agent | Inline chat in agent's activity view | Steering |
| Workspace orchestrator | Workspace orchestrator agent | Inline chat in workspace overview | Priority changes |
| Briefing view | LLM (read-only, grounded in briefing data) | Inline Q&A below the briefing | Follow-up |
| MR detail view | Author agent (or interrogation agent) | Inline chat in MR panel | Feedback |

**Visual indicator:** The chat input shows the recipient: `Message to worker-12 ▸`

**Message delivery:** Human messages are Directed-tier messages via the message bus. They are signed, persisted, and attestable.

### Hard Interrupt

```
Agent Detail Panel:
  [Pause] [Stop] [Message]

  Pause: Sends Directed-tier message with kind `StatusUpdate` and payload
         `{status: "pause_requested"}`. Agent picks up on next `message.poll`.

  Stop:  Calls existing `POST /api/v1/admin/agents/:id/kill`.
         Agent process terminated, work preserved.

  Message: Opens inline chat. Sends Directed-tier message.
```

## Implementation Plan

1. **Scoped chat input component:**
   - Create a reusable `ScopedChat.svelte` component
   - Props: `recipientId`, `recipientLabel`, `context` (agent/orchestrator/briefing/mr)
   - Display: `Message to {recipientLabel} ▸` in the input placeholder
   - On send: create a Directed-tier message via the message bus

2. **Agent detail panel chat:**
   - In the agent detail panel (slide-in), add `ScopedChat` with `recipientId=agent.id`
   - Label: `Message to {agent.name} ▸`
   - Messages are Directed-tier: `POST /api/v1/messages` with `tier: "Directed"`, `to: agent.id`
   - The agent receives via `message.poll` MCP tool (existing `GET /api/v1/agents/:id/messages`)

3. **Workspace orchestrator chat:**
   - In the workspace overview, add `ScopedChat` for the workspace orchestrator
   - Recipient is the workspace orchestrator agent (query active agents with orchestrator role)

4. **MR detail panel chat:**
   - In the MR detail panel, add `ScopedChat` with `recipientId=mr.author_agent_id`
   - If the author agent is no longer active, show "Agent completed. [Ask Why] to spawn interrogation agent."

5. **Briefing Q&A chat:**
   - Below the briefing view, add chat grounded in briefing data
   - This uses `POST /api/v1/workspaces/:id/briefing/ask` (already registered at line 892 in api/mod.rs)
   - Not a Directed-tier message — it's LLM Q&A, not agent communication

6. **Hard interrupt buttons on agent detail panel:**
   - **Pause button:**
     - Sends Directed-tier message with `kind: "StatusUpdate"`, payload: `{status: "pause_requested", summary: "Human requested pause"}`
     - Visual state: button toggles to "Paused" indicator
     - Agent finishes current action, sees message on next `message.poll`
   - **Stop button:**
     - Calls `POST /api/v1/admin/agents/:id/kill` (already registered at api/mod.rs)
     - Confirmation dialog: "Stop agent {name}? Work will be preserved in the worktree."
     - Visual state: agent status changes to terminated
   - **Message button:**
     - Opens/focuses the inline chat input (same as ScopedChat)

7. **Message bus integration:**
   - Verify that human users can send Directed-tier messages to agents
   - The message bus spec (`message-bus.md`) needs to support user→agent messages
   - Check if `POST /api/v1/messages` or equivalent exists for sending messages

## Acceptance Criteria

- [x] `ScopedChat` component renders with recipient label in placeholder
- [x] Agent detail panel has inline chat sending Directed-tier messages
- [x] Workspace orchestrator chat targets the orchestrator agent
- [x] MR detail panel chat targets the author agent
- [x] Briefing Q&A uses `POST /workspaces/:id/briefing/ask`
- [x] Pause button sends StatusUpdate Directed-tier message
- [x] Stop button calls `POST /admin/agents/:id/kill` with confirmation
- [x] Message button opens inline chat
- [x] Human messages appear in agent's `message.poll` results
- [x] Chat input always shows who the message is going to
- [x] `npm test` passes in `web/` (task-091 suites; pre-existing canvas/ResizeObserver failures in ExplorerCanvas/FlowRenderer/MoldableView/ExplorerViewAskViewSpec are unrelated and fail on `main` without these changes)

## Implementation Notes

- **Scoped chat component = `web/src/lib/InlineChat.svelte`.** The task plan names it `ScopedChat`, but the reusable scoped-chat component already exists as `InlineChat` (props `recipient`, `recipientType`, `onmessage`; renders `Message to {recipient} ▸`). Per the no-duplicate-convention rule it is reused rather than duplicated. Added an exported `focus()` method so the "Message" hard-interrupt button can focus the input.
- **Directed-tier delivery.** `api.sendAgentMessage` now forwards an optional `tier`. Human steering/feedback/priority chat sends a Custom kind `user_message` with `tier: "directed"` (server: `POST /workspaces/:id/messages`, `messages.rs:96-98` — Custom opts into Directed). These are signed, persisted, ack-based, and drained by `message.poll` (`GET /agents/:id/messages`, `list_after` returns any stored message to the agent regardless of tier).
- **Pause** sends kind `status_update` (natively Directed) with payload `{status: "pause_requested", summary: "Human requested pause"}`. **Stop** confirms then calls `api.adminKillAgent`. **Message** opens/focuses the chat.
- **Agent detail panel** (`DetailPanel.svelte`): chat tab renders `InlineChat` (recipient = agent name) below the delivered-message thread; a Hard Interrupt row (`Pause`/`Stop`/`Message`) is shown while the agent is active/running/spawning.
- **MR detail panel**: added a `chat` tab. The author agent is resolved once in the MR loader (`api.agent`, capturing `workspace_id`/`status`/`name`). Active author → `InlineChat`; completed/dead author → "Agent completed" with an **Ask Why** button.
- **Workspace orchestrator** (`WorkspaceHome.svelte`): orchestrator agent identified by name convention (agents carry no role field), sidebar `InlineChat` sends Directed priority messages.
- **Backend already complete** — no server changes needed; the message bus supports User-origin Directed messages to workspace agents.

## Agent Instructions

Read `specs/system/human-system-interface.md` §4 "Scoped Inline Chat" and "Hard Interrupt" for the full spec. Check the message bus implementation: `specs/system/message-bus.md` defines the tier model (Directed tier for human→agent messages). The agent message polling endpoint is `GET /api/v1/agents/:id/messages` (registered in api/mod.rs at line 229). The kill endpoint is at `POST /api/v1/admin/agents/:id/kill`. The briefing ask endpoint is at `POST /api/v1/workspaces/:id/briefing/ask`. For the ScopedChat component, look at how existing chat interfaces work in the frontend (e.g., ExplorerChat.svelte). The key design rule: the user always knows where their message is going — the recipient is always visible.
