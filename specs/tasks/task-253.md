---
title: "MCP SSE stream: workspace filtering and MessageKind-to-AG-UI event mapping"
spec_ref: "Spec gap spec-c62aac6701125cb09e0c736c / mcp-sse-workspace-agui / 60b177f85a11"
depends_on: []
progress: not-started
commits: []
---

## Required behavior

Per message-bus.md MCP Integration: 'The MCP SSE endpoint (GET /mcp/sse) subscribes to the message bus filtered by the authenticated agent's workspace. It maps MessageKind variants to AG-UI event types for protocol compatibility.' The current handler (crates/gyre-server/src/mcp.rs:5064-5083) subscribes to the unfiltered global message_broadcast_tx and emits every message as a generic SSE 'message' event carrying the raw envelope JSON — every connected MCP client receives every tenant's and workspace's bus messages, and the specced event names never appear. Fix mcp_sse_handler to: (1) resolve the authenticated agent's workspace (from the verified JWT hierarchy claims / agent record) and deliver only bus messages scoped to that workspace (Destination::Workspace(ws) where ws matches, Broadcast per the bus's broadcast semantics, and Destination::Agent(id) only when id is the authenticated agent) — a client in workspace A must not receive workspace B's or another tenant's messages; (2) map MessageKind to AG-UI SSE event names per the spec table — ToolCallStart→TOOL_CALL_START, ToolCallEnd→TOOL_CALL_END, RunStarted→RUN_STARTED, RunFinished→RUN_FINISHED, TextMessageContent→TEXT_MESSAGE_CONTENT, StateChanged→STATE_CHANGED, AgentError→ERROR — emitting each as an SSE event with the AG-UI event name (data payload per the existing envelope format); kinds outside the table keep a sensible default event name. Existing server→client notifications already carried on this SSE stream must continue to be delivered. The MCP message tools (gyre_message_send/poll/ack) and gyre_record_activity are already conformant and are out of scope.

## Acceptance Criteria

- [ ] Workspace isolation: two MCP SSE connections authenticated as agents in different workspaces; a Workspace-scoped message emitted to workspace A is received by A's connection only — B's connection receives nothing for it (today both receive everything).
- [ ] AG-UI mapping: a Telemetry-tier ToolCallStart message for the subscriber's workspace arrives as an SSE event named TOOL_CALL_START (not the generic 'message'), and likewise for the other six kinds in the spec table; AgentError arrives as ERROR.
- [ ] Directed delivery: a Directed message to the authenticated agent is delivered on its SSE stream; Directed messages to other agents are not.
- [ ] Existing SSE notification events (server→client notifications) still arrive on the stream after the change.
- [ ] gyre_record_activity-produced telemetry surfaces on the SSE stream of clients in the same workspace with the mapped AG-UI event name.

## Verification intent

Isolation probe: open an MCP SSE stream authenticated as an agent in workspace B, then emit a Workspace-scoped Event/Telemetry message to workspace A (different workspace, or a different tenant's workspace); the current handler forwards every broadcast message to every subscriber with no filtering (mcp.rs:5071-5077), so B's stream receives A's message — the test asserts B's stream stays silent for A's message (only keep-alives) and therefore fails against the current code, passing only with the workspace filter. Mapping probe: push a ToolCallStart telemetry message to the subscriber's workspace and inspect the SSE event name field; today it is always 'message' (mcp.rs:5077), so an assertion that the event name is TOOL_CALL_START fails until the kind→AG-UI mapping exists.

Update the relevant coverage rows only after implementing the behavior, with concrete source evidence. Do not mark unrelated rows complete or claim independent verification before it occurs.

## Discovery evidence

Canonical spec: `specs/system/message-bus.md`. Source: `4127964001113dda08e4852404a68ba560ceb764`; plan: `151021041bfb974bd08554e6182cff451df6efb83f37d8a11f6dfdcc1950d05e`.

[
  {
    "id": "103aac23ed1cdc9d4788f31d",
    "decision": "gap",
    "requirement_quote": "The MCP SSE endpoint (`GET /mcp/sse`) subscribes to the message bus filtered by the authenticated agent's workspace. It maps `MessageKind` variants to AG-UI event types for protocol compatibility:",
    "detail": "The MCP tools are implemented and correctly gated: gyre_message_send / gyre_message_poll / gyre_message_ack are registered with schemas matching the REST wrappers, derive the agent identity from the authenticated JWT, apply the same tier/destination and payload validation, and both write-performing tools (send, ack) sit in the needs_write RBAC gate while the read-only poll does not; gyre_record_activity creates a Telemetry-tier Message with Destination::Workspace (the caller's workspace), maps AG-UI event strings to the corresponding MessageKinds, and pushes to the TelemetryBuffer and broadcast channel. The SSE requirement is NOT met: GET /mcp/sse subscribes to the unfiltered global message_broadcast_tx broadcast with no filtering by the authenticated agent's workspace (every connected MCP client receives every tenant's/workspace's bus messages), and it performs no MessageKind → AG-UI event-type mapping — every message is emitted as a generic SSE 'message' event carrying the raw envelope JSON, so the specced TOOL_CALL_START/RUN_STARTED/ERROR etc. event names never appear on the SSE stream.",
    "evidence": [
      {
        "file": "crates/gyre-server/src/mcp.rs",
        "quote": "\"name\": \"gyre_message_send\",",
        "detail": "The message.send tool is registered (with gyre_message_poll and gyre_message_ack alongside), wrapping the REST endpoints per platform-model.md §4 as specced."
      },
      {
        "file": "crates/gyre-server/src/mcp.rs",
        "quote": "| \"gyre_message_send\"",
        "detail": "gyre_message_send and gyre_message_ack are in the needs_write RBAC gate (write-performing tools), while read-only gyre_message_poll is correctly outside it."
      },
      {
        "file": "crates/gyre-server/src/mcp.rs",
        "quote": "let rx = state.message_broadcast_tx.subscribe();",
        "detail": "The SSE handler subscribes to the global, unfiltered bus broadcast channel — there is no filter by the authenticated agent's workspace, contradicting 'subscribes to the message bus filtered by the authenticated agent's workspace'."
      },
      {
        "file": "crates/gyre-server/src/mcp.rs",
        "quote": "// Map all bus messages to SSE \"message\" events.\n                    let sse_event = Event::default().event(\"message\").data(data);",
        "detail": "No MessageKind → AG-UI event-type mapping exists on the SSE output path: every bus message becomes a generic 'message' event, so the specced TOOL_CALL_START/TOOL_CALL_END/RUN_STARTED/RUN_FINISHED/TEXT_MESSAGE_CONTENT/STATE_CHANGED/ERROR event names are never emitted."
      }
    ]
  }
]
