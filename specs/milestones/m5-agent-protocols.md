# Milestone 5: Agent Protocols & Advanced Source Control

Implement the agent communication protocols and advanced source control features. After M5, agents communicate via MCP and A2A protocols, the activity feed uses AG-UI event taxonomy, and agents use jj for VCS operations.

## Deliverables

### 1. MCP Server Integration

Gyre serves MCP (Model Context Protocol) endpoints that agents consume:

- **MCP server in gyre-server** — expose Gyre capabilities as MCP tools
  - `gyre_create_task` — create a task
  - `gyre_list_tasks` — query tasks
  - `gyre_update_task` — update task fields/status
  - `gyre_create_mr` — create merge request
  - `gyre_list_mrs` — list merge requests
  - `gyre_record_activity` — log activity event
  - `gyre_agent_heartbeat` — send heartbeat
  - `gyre_agent_complete` — signal task completion
- **MCP transport** — serve over stdio (for local agents) and HTTP/SSE (for remote)
- **MCP tool discovery** — agents can list available tools via MCP protocol
- **Server-defined MCP config** — MCP servers defined on the Gyre server, injected to agents at spawn time

### 2. A2A (Agent-to-Agent) Protocol

Inter-agent discovery and communication:

- **Agent Cards** — each agent publishes capabilities via A2A Agent Card
- **Agent discovery** — `GET /api/v1/agents/discover` returns Agent Cards for active agents
- **Task delegation** — agents can delegate subtasks to discovered peers
- **Structured messages** — typed messages (TaskAssignment, ReviewRequest, StatusUpdate, Escalation)

### 3. AG-UI Event Taxonomy

Adopt AG-UI event types for the activity feed:

- Replace free-form event_type strings with typed events:
  - `TOOL_CALL_START`, `TOOL_CALL_END`
  - `TEXT_MESSAGE_CONTENT`
  - `RUN_STARTED`, `RUN_FINISHED`
  - `STATE_CHANGED`
  - `ERROR`
- WebSocket broadcasts use typed events
- Dashboard activity feed renders event-type-specific UI

### 4. Jujutsu (jj) Integration

Agent-facing VCS layer on top of git:

- **jj operations** — atomic per-action changes, auto-rebase, undo
- **GitOpsPort extension** — add jj-specific operations alongside git2
- **Agent workflow** — agents use jj for commits (anonymous WIP, auto-rebase)
- **Operation log** — full undo capability via jj's operation log

### 5. Agent Compose Spec

Declarative team blueprints (like docker-compose for agents):

- **agent-compose.yaml** format — defines agents, roles, hierarchy, tasks, MCP servers
- **Parse + validate** — server reads compose file, creates agent tree
- **Spawn from compose** — `POST /api/v1/compose/apply` creates the full agent team
- **Reconcile** — compare desired state (compose) with actual state, converge

### 6. Dashboard Updates

- MCP tool catalog view
- Agent Card display
- Compose file editor/viewer
- Typed activity events with rich rendering

## Success Criteria

- Agents can discover and communicate via MCP and A2A
- Activity feed uses typed AG-UI events
- Agent compose spec creates multi-agent teams
- 350+ tests

## Dependencies

- M4 Identity & Observability (complete)
