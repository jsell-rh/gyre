# TASK-001: MCP Message Bus Tools (message.send, message.poll, message.ack)

**Spec reference:** `message-bus.md` §MCP Integration  
**Progress:** `ready-for-review`

## Spec Excerpt

From `message-bus.md`:

> **MCP tools for the message bus** (additions to `platform-model.md` §4 tool table):
>
> | Tool | Scope | Purpose |
> |---|---|---|
> | `message.send` | workspace | Send a Directed or Custom message to an agent in the same workspace |
> | `message.poll` | agent | Poll own inbox for new Directed messages (wraps `GET .../messages?after_ts=`) |
> | `message.ack` | agent | Acknowledge a received message (wraps `PUT .../messages/:id/ack`) |
>
> These are thin wrappers around the REST endpoints. Per `platform-model.md` §4, all agent-to-server interaction is via MCP tools — agents should use these tools rather than calling the REST API directly.

Additionally, `message-bus.md` specifies:

> The MCP `gyre_record_activity` tool becomes a thin wrapper that creates a Telemetry-tier `Message` with the appropriate `MessageKind` and `Destination::Workspace(caller's workspace)`.

## Current State

The MCP server (`crates/gyre-server/src/mcp.rs`) exposes these tools:
- `gyre_create_task`, `gyre_list_tasks`, `gyre_update_task`
- `gyre_create_mr`, `gyre_list_mrs`
- `gyre_record_activity`
- `gyre_agent_heartbeat`, `gyre_agent_complete`
- `gyre_analytics_query`, `gyre_search`

The `conversation.upload` MCP tool is implemented (line ~806).

**Missing:** `message.send`, `message.poll`, `message.ack` tools. These are required for agents to send Directed messages to other agents, poll their own inbox, and acknowledge received messages via the unified message bus.

## Implementation Plan

1. Add `gyre_message_send` tool definition to the MCP tools list in `mcp.rs`:
   - Parameters: `to` (destination JSON: `{"agent": "<id>"}` or `{"workspace": "<id>"}`), `kind` (MessageKind string), `payload` (optional JSON), `tier` (optional, for Custom kinds)
   - Wraps `POST /api/v1/workspaces/:workspace_id/messages`
   - Derives `workspace_id` from agent JWT claims
   - Validates same-workspace constraint for Directed messages

2. Add `gyre_message_poll` tool definition:
   - Parameters: `after_ts` (optional, default 0), `after_id` (optional), `limit` (optional, default 100), `unacked_only` (optional bool for crash recovery)
   - Wraps `GET /api/v1/agents/:id/messages`
   - Derives agent_id from JWT `sub` claim

3. Add `gyre_message_ack` tool definition:
   - Parameters: `message_id` (required)
   - Wraps `PUT /api/v1/agents/:id/messages/:message_id/ack`
   - Derives agent_id from JWT `sub` claim

4. Verify `gyre_record_activity` creates Telemetry-tier messages through the unified bus (it may already do this — check current implementation).

5. Add tests for all three new tools.

## Acceptance Criteria

- [ ] `gyre_message_send` tool available in MCP tools list
- [ ] `gyre_message_poll` tool available in MCP tools list
- [ ] `gyre_message_ack` tool available in MCP tools list
- [ ] `gyre_record_activity` routes through the unified message bus (Telemetry tier)
- [ ] Tests verify all three tools work with agent JWT auth
- [ ] Tests verify same-workspace constraint on `message.send`

## Agent Instructions

When working on this task:
1. Update the progress field above to `in-progress`
2. Read `crates/gyre-server/src/mcp.rs` for the existing tool pattern
3. Read `crates/gyre-server/src/api/messages.rs` for the REST handlers being wrapped
4. Implement the tools following the existing pattern in `mcp.rs`
5. Add tests following the existing test pattern
6. On completion, update progress to `ready-for-review` and list git commits below

## Git Commits

- `feat(mcp): add message bus tools (send, poll, ack) — TASK-001`
