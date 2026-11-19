# Review — task-253 (MCP SSE stream: workspace filtering and MessageKind-to-AG-UI event mapping)

Spec: `specs/system/message-bus.md` §MCP Integration (+ §Real-time Delivery, §Destination).
Candidate: `88f17d9d` (base `bd611083`; product commits `c9c8b4c3`, `b4ac4adc`).
Verdict: **needs-revision** — one finding (F1, directed-delivery principal-namespace confusion); the workspace filter, AG-UI mapping, and test design are otherwise genuine.

## Probes run (review sandbox, candidate checked out)

- `SKIP_WEB_BUILD=1 cargo check -p gyre-server --tests` → **exit 0** (9m02s cold; full log + exit code in `/tmp/stage/review-evidence/cargo-check-gyre-server.log`). The new handler, `agui_event_name`, and all five tests type-check.
- `SKIP_WEB_BUILD=1 cargo clippy -p gyre-server --tests` → exit 0; **zero warnings in the new SSE code** (mcp.rs lines 5000-5999; the three mcp.rs warnings at 2092/3400/3401 are pre-existing, outside the diff). The `clippy::let_and_return` class that failed verification round a64f8857 is gone — the b4ac4adc repair is effective. (Log: `clippy-gyre-server.log`.)
- axum 0.7.9 `Event::field` source inspection: SSE fields serialize as `event: NAME` (name + `: ` + value), so the tests' `text.contains("event: TOOL_CALL_START")` assertions match the real wire format.
- Broadcast channel capacity is 256 (lib.rs:1234) — the mapping test's 8 in-flight messages cannot lag.
- Auth-extractor source verification of every test assumption: agent-JWT path (auth.rs:1740-1480 region) sets `user_id: None`, `roles: [Agent]`, mirrors `tenant_id`/`workspace_id` from signature-verified claims; global token `test-token` → `agent_id: "system"`, `roles: [Admin]`, `workspace_id: None`; API-key/Keycloak user paths set `agent_id = user.display_name`, `user_id: Some(...)`. `mint`/`mint_orchestrator` signatures match the test helpers' calls; the worker JWT's `repo:gyre:write` scope satisfies `check_repo_write_scope` for `gyre_record_activity` (target agent_id == caller skips the foreign-target check).
- Workspace ids are globally unique (`workspaces.id TEXT PRIMARY KEY`, migration 000019), so `Destination::Workspace` id-equality implies tenant containment — the filter's design premise holds.
- `handle_record_activity` (mcp.rs:2008-2131) resolves ws from the caller's agent record and broadcasts `Destination::Workspace(ws)` — the AC5 test's cross-path (tool → bus → subscriber filter) is real, not stubbed.
- `cargo test -p gyre-server mcp_sse` could not be executed here: the stage wrapper blocks gyre-server codegen (model + server codegen exceeds 4 GiB). **Verifier must run it at exact head** (the five tests: `mcp_sse_workspace_isolation`, `mcp_sse_agui_event_mapping`, `mcp_sse_directed_delivery`, `mcp_sse_admin_connection_still_receives_notifications`, `mcp_sse_record_activity_telemetry_reaches_workspace`), plus the changed-lines clippy gate.

## Verified working (no findings)

- **Workspace filtering is real and fail-closed.** Scope resolution prefers the stored agent record (`state.agents.find_by_id` — the same authoritative source `gyre_message_send` uses at mcp.rs:2042), falls back to the signature-verified `workspace_id` claim, and never fabricates a scope; callers with neither get Broadcast-only (or the admin legacy view). `Destination::Workspace(ws)` delivers only on exact id match; `Destination::Broadcast` follows the bus's "all connected clients" semantics (spec line 288); the old forward-everything behavior is gone.
- **AG-UI mapping matches the spec table exactly.** `agui_event_name`: the seven specced kinds → `TOOL_CALL_START`/`TOOL_CALL_END`/`RUN_STARTED`/`RUN_FINISHED`/`TEXT_MESSAGE_CONTENT`/`STATE_CHANGED`/`ERROR` (AgentError), everything else keeps the generic `message` default. Payload format unchanged (raw envelope JSON), as required.
- **The tests discriminate.** All five drive the real router (`build_router(test_state())`) through the real auth extractor with minted+registered JWTs and the real broadcast bus; the isolation and mapping tests assert exactly what the pre-task handler violated (B receiving A's messages; always-`message` event names), so they fail against the base and pass only with the fix. SSE frames are asserted on the real axum wire format. Not self-confirming, not stub-backed.
- **AC4 preserved:** broadcast server→client notifications still arrive (Broadcast is unconditionally delivered), and the admin test proves it end-to-end.
- Coverage row 14 and `docs/api-reference.md` updated with concrete source evidence; no unrelated rows touched.

## Findings

- [-] **F1 (major, code): directed-delivery arm compares a bus agent id against `auth.agent_id` with no principal-namespace guard — spec contradiction with message-bus.md §Real-time Delivery and a cross-namespace leak under id collision.** `Destination::Agent(aid) => aid.as_str() == agent_id` (mcp.rs ~5132). The spec is explicit: directed messages are "delivered only to the WebSocket connection authenticated with a JWT whose `sub` matches `id`", and shared-global-token clients receive "workspace-scoped and broadcast messages but NOT agent-targeted messages". The sibling WebSocket bus filter for the same destinations (ws.rs:437-456) enforces `caller.user_id.is_none() && !caller.roles.contains(&UserRole::Admin) && caller.agent_id == aid.as_str()` with a comment explaining exactly why: "User identities (user_id set) and the global token (Admin role, agent_id 'system') never match — they see agent inboxes through the REST/MCP read paths, not the bus." The SSE handler omits both guards:
  - API-key and Keycloak user principals carry `agent_id = user.display_name` with `user_id: Some(...)` (auth.rs:1629-1642, 2044-2046) — a Directed message to any agent whose id equals a user's display name is streamed to that user's `/mcp/sse` connection, bypassing the ABAC that gates the REST/MCP read paths, potentially cross-tenant (agent ids and display names are independent namespaces; tenant isolation for `Destination::Agent` is spec edge-case 1).
  - The global token carries `agent_id: "system"` + Admin — a Directed message to an agent literally named "system" lands on every global-token admin stream, which the spec rules out for shared-token clients.

  Reproduce: create a user with display_name "orchestrator" and an API key; open `GET /mcp/sse` with that key; send a Directed message to agent "orchestrator" via `POST /workspaces/:id/messages` — the user's stream receives the full envelope. The handler's own scope arm guards against precisely this confusion class for workspaces ("user principals ... carry a username in `agent_id`, which must never accidentally inherit a same-named agent record's workspace") — the directed arm needs the same one-line treatment mirroring ws.rs: `auth.user_id.is_none() && !auth.roles.contains(&UserRole::Admin) && aid.as_str() == agent_id`. None of the five tests covers user-principal or global-token directed delivery, so they cannot catch it; a sixth test (API-key user whose display_name equals an agent id must NOT receive that agent's directed message) would close the loop.

Minor (non-blocking, recorded for completeness):

- The `unscoped_admin` carve-out delivers **all** `Destination::Workspace` messages (every tenant) to scope-less Admin principals, where the task text reads "Workspace(ws) where ws matches" (no workspace → no match). This is a deliberate legacy-dashboard preservation, documented in the shipped notes and mirroring ws.rs Subscribe's admin bypass (admins may subscribe to any workspace), and Broadcast-only delivery would already satisfy AC4 — so it is an accepted design choice, not a defect. Worth a sentence in the coverage row if anyone questions it later.
- No test exercises the agent-record-governs-over-claim path (record workspace ≠ JWT claim). The record path is exercised indirectly via AC5's worker, but a divergence test (record ws-B, claim ws-A → subscriber scoped to ws-B) would pin the precedence.
- The directed-delivery test uses `text.contains("sse-directed-agent")` — the assertion happens to be sound because the envelope's `to` field serializes the target id, but asserting on the envelope JSON id field would be tighter.

Test-inflation check: none found — every test fails against the pre-task handler for the behavior it names.

Scope check: diff touches only mcp.rs, docs/api-reference.md, specs/coverage/system/message-bus.md (row 14 + audit note), specs/tasks/task-253.md. No out-of-scope edits; the MCP message tools and `gyre_record_activity` are untouched as instructed.

## Deferred to the independent verifier

- `cargo test -p gyre-server mcp_sse` (five focused tests) at exact head — blocked in this sandbox by the stage build wrapper (gyre-server codegen + model exceeds 4 GiB).
- Changed-lines clippy gate (package-level `cargo clippy -p gyre-server --tests` ran clean on all new code here).
- After F1's repair: the new/extended directed-delivery test for user-principal and global-token exclusion.
