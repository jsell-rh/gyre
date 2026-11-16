---
title: "HSI Scoped Inline Chat + Hard Interrupt"
spec_ref: "human-system-interface.md §4 Scoped Inline Chat + Hard Interrupt"
depends_on: []
progress: complete
coverage_sections:
  - "human-system-interface.md §4 Scoped Inline Chat"
  - "human-system-interface.md §4 Hard Interrupt"
commits: ["76b3da22e99d4cf0724289376b17bf62db3eda01", "3e22e49085b3a488bfe9acc680dd940f873333e6", "a8d036f466482a9a00157a496d10000d0f36d25c", "58e4568219fd5324a2d2a2b30642602e36a47180", "0674cb65ca1589c3822136c4dc0dd94c97fefe70"]
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

## Shipped

Scoped inline chat and hard interrupt per HSI §4, plus the role-based recipient repair from review gap d382faa3 and the verification repair at head 76b3da22e (rustfmt of the `orchestrator_type` test lines in `agents.rs` — the sole failing check in verification run 635d3a33; whitespace-only, no behavior change). Earlier rounds verified at merge head b2419479a with `web/src` and `crates` byte-identical to reviewed candidate 015340baf; this round's only source delta beyond that state is the rustfmt rewrap.

### Behavior

- **Scoped chat component = `web/src/lib/InlineChat.svelte`** (reused, not duplicated as `ScopedChat`): props `recipient`, `recipientType`, `onmessage`; renders `Message to {recipient} ▸`; exports `focus()` so the "Message" hard-interrupt button opens/focuses the chat.
- **Directed-tier delivery.** `api.sendAgentMessage` forwards an optional `tier`; human steering/feedback/priority chat sends kind `user_message` with `tier: "directed"` (server: `POST /workspaces/:id/messages` — Custom opts into Directed). Messages are signed, persisted, ack-based, and drained by `message.poll` (`GET /agents/:id/messages`). **Pause** sends kind `status_update` (natively Directed) with `{status: "pause_requested", summary: "Human requested pause"}`; **Stop** confirms then calls `api.adminKillAgent` (`POST /admin/agents/:id/kill`); **Message** opens/focuses the chat.
- **Panels.** Agent detail panel (`DetailPanel.svelte`): chat tab with `InlineChat` + Hard Interrupt row while the agent is live. MR detail panel: `chat` tab targeting the author agent (`api.agent`); completed/dead author → "Agent completed" + **Ask Why**. Briefing Q&A (`Briefing.svelte`) via `api.briefingAsk` → `POST /workspaces/:id/briefing/ask` (LLM Q&A, not agent communication).
- **Workspace orchestrator recipient is selected by ROLE, not name** (commit 3e22e490, repair of gap d382faa3). `AgentResponse` now exposes `orchestrator_type` (`worker`/`workspace_orchestrator`/`repo_orchestrator`, platform-model.md §3) on every agent list/get endpoint; `OrchestratorAgentResponse` drops its outer copy so the flattened `AgentResponse` carries the key exactly once (wire stays strict-parseable — covered by the existing duplicate-key regression tests). `WorkspaceHome.svelte` filters workspace agents on `orchestrator_type === 'workspace_orchestrator'` (preferring active), so a repo orchestrator or a name-matching worker is never the priority-chat recipient, and a custom-named workspace orchestrator (e.g. `"coord"`) is found; the widget is hidden when no workspace orchestrator exists.

### Test evidence

- Frontend (this round, head b2419479a; `npm ci` then `npx vitest run`): `WorkspaceHomeSections` + `InlineChat` + `DetailPanelChat` + `Briefing` — 4 files, 97 passed / 41 skipped (skips are pre-existing `describe.skip` old-layout blocks). Includes both d382faa3 regression tests: a repo orchestrator or a name-matching worker is not selected as the workspace chat recipient, and a custom-named workspace orchestrator (`"coord"`) is selected alongside live repo orchestrators.
- Server: `web/src` and `crates` at this head are byte-identical to candidate 015340baf, whose suites passed in the verified prior round (`api::agents` 11 incl. `agent_response_exposes_orchestrator_type`, `api::orchestrator` 12, `api::spawn` 52, `mcp::` 88, `cargo build -p gyre-cli -p gyre-server` clean). Verifier should re-run `cargo test -p gyre-server --lib api::agents api::orchestrator api::spawn` plus the mcp suites; no cold server build was attempted in this sandbox (stage 4 GiB budget — a local `cargo check` adds no information over the byte-identity proof).
- `bash scripts/check-task-commit-attribution.sh` — OK at this head; all four frontmatter commits (3e22e490, a8d036f, 58e4568, 76b3da2) are reachable from HEAD.
- `cargo fmt -p gyre-server --check` — clean at this head (the 635d3a33 verification failure, agents.rs lines 959-961/987-988, fixed by 76b3da22e). Workspace-wide `cargo fmt --check` still reports a pre-existing blank-line drift in `crates/gyre-ports/src/notification.rs` that exists on the assignment base and is untouched by this branch (changed-lines scope, not a task-091 finding).
- Evidence retained at `/tmp/stage/review-evidence/task-091-round3/evidence.md` and `/tmp/stage/review-evidence/task-091-round4/evidence.md`.

### Review finding bfe34dfa (stale web/dist) disposition

- The finding is correct that the committed `web/dist` bundle predates the role-based repair (it still contains the deleted name-convention filter and no `orchestrator_type`). Per this stage's protocol the executor restores generated `web/dist` files from the assignment base and final integration rebuilds and publishes the bundle remotely, so dist is intentionally **not** regenerated or committed in this assignment. The role-based selection ships in source — `web/src/components/WorkspaceHome.svelte` (filters on `orchestrator_type === 'workspace_orchestrator'`) and `crates/gyre-server/src/api/agents.rs` (`AgentResponse.orchestrator_type`) — which is the input final integration builds the published bundle from.

## Agent Instructions

Read `specs/system/human-system-interface.md` §4 "Scoped Inline Chat" and "Hard Interrupt" for the full spec. Check the message bus implementation: `specs/system/message-bus.md` defines the tier model (Directed tier for human→agent messages). The agent message polling endpoint is `GET /api/v1/agents/:id/messages` (registered in api/mod.rs at line 229). The kill endpoint is at `POST /api/v1/admin/agents/:id/kill`. The briefing ask endpoint is at `POST /api/v1/workspaces/:id/briefing/ask`. For the ScopedChat component, look at how existing chat interfaces work in the frontend (e.g., ExplorerChat.svelte). The key design rule: the user always knows where their message is going — the recipient is always visible.
