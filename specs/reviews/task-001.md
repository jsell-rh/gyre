# Review: TASK-001 — MCP Message Bus Tools

**Reviewer:** Verifier  
**Date:** 2026-04-07  
**Verdict:** needs-revision

---

## Findings

- [x] [process-revision-complete] **F1: `gyre_record_activity` acceptance criterion unmet.** The task acceptance criteria states "`gyre_record_activity` routes through the unified message bus (Telemetry tier)." The commit does not modify `gyre_record_activity`. The existing implementation has two spec violations:  
  (a) Always emits `MessageKind::StateChanged` regardless of the AG-UI `event_type` parameter. The spec (`message-bus.md` §MCP Integration) maps each AG-UI event type to a distinct `MessageKind` (e.g., `TOOL_CALL_START` → `ToolCallStart`, `RUN_STARTED` → `RunStarted`).  
  (b) Uses hardcoded `Id::new("default")` as the workspace instead of the caller's workspace. The spec says: "creates a Telemetry-tier `Message` with the appropriate `MessageKind` and `Destination::Workspace(caller's workspace)`." The handler signature (`handle_record_activity(state, args)`) does not accept `&AuthenticatedAgent`, so it cannot derive the caller's workspace.

- [x] [process-revision-complete] **F2: Broadcast destination accepted but out of scope for `message.send`.** `parse_mcp_destination` accepts the string `"broadcast"` as a valid destination. The spec (`message-bus.md` §MCP Integration tool table) defines `message.send` as: "Send a Directed or Custom message to an agent in the same workspace." Broadcast is not a valid destination for this tool. Separately, the spec scoping rules (§Scoping Rules, rule 6) require "Server origin or Admin role" for Broadcast. The MCP handler does not enforce this — an Agent-role caller can send a Broadcast message with no rejection.

- [x] [process-revision-complete] **F3: Telemetry-tier standard kinds not rejected by `message.send`.** The spec defines the tool's purpose as sending "a Directed or Custom message." The implementation accepts any non-`server_only()` kind, including Telemetry-tier kinds (`ToolCallStart`, `ToolCallEnd`, `TextMessageContent`, `RunStarted`, `RunFinished`, `StateChanged`). These bypass signing and persistence (correct for telemetry), but allowing them through `message.send` violates the tool's stated scope. The spec routes telemetry through `gyre_record_activity`, not `message.send`.

- [x] [process-revision-complete] **F4: Duplicated signing logic will drift.** `sign_mcp_message` (mcp.rs:1487–1539) is a verbatim copy of `sign_message` (api/messages.rs:63–106). The spec defines a single deterministic signing algorithm (§Signing). If the canonical `sign_message` is updated, the MCP copy will silently diverge, producing invalid signatures for MCP-originated messages. Extract to a shared function (e.g., in `gyre-server/src/signing.rs` or as a public method on `AppState`).

- [ ] **F5: `emit_telemetry` hardcodes `MessageOrigin::Server` and `tenant_id: "default"` for agent-originated telemetry.** When an agent calls `gyre_record_activity`, the message is emitted via `emit_telemetry` (`lib.rs:487–499`), which hardcodes `from: MessageOrigin::Server` and `tenant_id: Id::new("default")`. The spec's origin resolution table (`message-bus.md` §Message Envelope) says Agent JWT maps to `MessageOrigin::Agent(sub claim)` with `tenant_id` from the JWT claim. This misattributes agent telemetry as server-originated and breaks multi-tenant isolation on telemetry messages. Fix: either pass `MessageOrigin` and `tenant_id` as parameters to `emit_telemetry`, or construct the `Message` directly in `handle_record_activity` instead of using the shared helper.

- [ ] **F6: `gyre_record_activity` payload does not conform to per-kind payload schemas.** The handler constructs the same payload shape for all mapped kinds: `{event_id, agent_id, event_type, description, timestamp}`. The spec (`message-bus.md` §Payload Schemas) defines distinct required fields per kind — e.g., `ToolCallStart` requires `{agent_id: Id, tool_name: String}`, `ToolCallEnd` requires `{agent_id: Id, tool_name: String, duration_ms: u64}`, `RunStarted` requires `{agent_id: Id, task_id: Option<Id>}`. Consumers relying on spec-defined payload fields (e.g., the MCP SSE endpoint's AG-UI mapping per §MCP Integration, or downstream `MessageConsumer` implementations) will receive unexpected data. The handler should construct kind-specific payloads using the available arguments, or at minimum include the spec-required fields alongside legacy fields.
