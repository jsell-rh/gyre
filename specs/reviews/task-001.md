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
