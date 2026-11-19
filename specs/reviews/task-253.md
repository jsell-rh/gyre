# Review: task-253 — MCP SSE workspace filtering + AG-UI event mapping

**Candidate:** `e0b38861cc47f0464923d8242597bbb2757575ee` (base `28a3a40b968b899c64097e85a3d6b3311bae5633`)
**Reviewer round:** 2026-11-19 (fresh verdict; no earlier approval transferred)
**Spec:** `specs/system/message-bus.md` §MCP Integration (gap `103aac23ed1cdc9d4788f31d`)

## Verdict: APPROVED

## What was reviewed

Diff base→candidate touches `crates/gyre-server/src/mcp.rs` (+516: handler
rewrite + 6 focused tests), `docs/api-reference.md`, coverage row 14 of
`specs/coverage/system/message-bus.md`, and `specs/tasks/task-253.md`. No
other production code changed. The DIRTY rebase onto base 28a3a40b (merge
ded5d029) resolved the `scripts/test_dev_cargo_clean.py` conflict in favor
of the base's newer CI-verified fix — verified: the file is
byte-identical to base in the candidate.

## Source findings (all resolved as correct)

1. **Workspace filtering** (`mcp.rs` `mcp_sse_handler`): scope resolution is
   fail-closed and never fabricated — agent/system principals resolve the
   stored agent record (`state.agents.find_by_id`), which is the
   authoritative ABAC boundary; record-less callers fall back to the
   signature-verified `workspace_id` JWT claim; lookup errors degrade to
   the same verified claim with a warning. User principals (`user_id` set)
   are claim-only, so an API-key user whose display_name collides with an
   agent's id cannot inherit that agent's workspace.
2. **Delivery filter**: `Destination::Workspace(ws)` delivered only on
   workspace-id equality (ids are UUIDs — globally unique, so the match
   implies tenant containment; the isolation test uses different tenants
   t1/t2); `Destination::Broadcast` delivered to all subscribers per
   message-bus.md §Delivery ("all connected clients" — Broadcast is
   Server/Admin-origin only, so no agent data-leak surface);
   `Destination::Agent(id)` delivered only on the authenticated agent's
   own stream AND only for genuine agent principals
   (`user_id.is_none() && !Admin`) — byte-for-byte the same
   principal-namespace guard as the production ws.rs bus filter
   (ws.rs:493-497), closing the display-name/"system" collision class
   (repair 1388c32e). Unscoped admins keep the legacy unscoped dashboard
   view, mirroring the ws.rs Subscribe admin bypass — existing
   server→client Broadcast notifications (DataSeeded/QueueUpdated) still
   arrive (probe test covers this).
3. **AG-UI mapping** (`agui_event_name`): all seven spec-table kinds map
   exactly (ToolCallStart→TOOL_CALL_START, ToolCallEnd→TOOL_CALL_END,
   RunStarted→RUN_STARTED, RunFinished→RUN_FINISHED,
   TextMessageContent→TEXT_MESSAGE_CONTENT, StateChanged→STATE_CHANGED,
   AgentError→ERROR); kinds outside the table keep the generic `message`
   event. SSE data payload unchanged (raw envelope JSON).
4. **No consumer regressions**: no web/ or CLI code consumes `/mcp/sse`
   (grep-verified); the only behavior change on the stream is the specced
   one.

## Test quality (no inflation found)

Six focused tests in `mcp.rs`, all through the real auth extractor
(minted orchestrator/repo JWTs registered in `agent_tokens` — established
patterns from the same file), the real broadcast channel, and, for
`mcp_sse_record_activity_telemetry_reaches_workspace`, a real MCP
`tools/call` dispatch of `gyre_record_activity` whose telemetry
destination derives from the caller's agent record. Each test fails
against the base handler by construction (base emits every bus message as
SSE event `message` with no filtering, so `event: TOOL_CALL_START`
assertions and the cross-workspace/other-agent negative assertions cannot
pass); the namespace-collision test kills the pre-1388c32e mutation
(absent the guard, the colliding directed messages are delivered).

## Independent probe evidence

Saved under `/tmp/stage/review-evidence/`:

- `cargo check -p gyre-server --tests` on the exact candidate — **exit 0**
  (~10 min; also built web assets via build.rs). All handler changes and
  all six SSE tests type-check against the merged base.
- `cargo fmt --check -p gyre-server` — **exit 0**.
- Rebase-conflict resolution verified identical to base (empty diff on
  `scripts/test_dev_cargo_clean.py`).

## Deferred to the independent verifier (sandbox 4 GiB budget rule)

gyre-server test-binary compilation is too heavy for this sandbox (prior
stage OOM-killed). Verifier must run:

```
cargo test -p gyre-server --lib mcp_sse
```

Expected: 6 passing tests (isolation, agui mapping, directed delivery,
namespace collisions, admin notifications, record_activity end-to-end).
