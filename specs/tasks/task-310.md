---
title: "WebSocket as the primary agent↔server transport: WS client in the agent runtime, real-time agent-directed push, and WS-borne liveness"
spec_ref: "Spec gap spec-f907eda3bd014fe546b838e2 / websocket-agent-transport / 4156114b8040"
depends_on: []
progress: ready-for-review
commits: ["bfd09c09e00db84e9d1db181f48ba46ab4f27ddb", "b6a6ccf2"]
---

## Required behavior

Per specs/development/architecture.md §API: 'WebSocket as the primary transport for agent ↔ server communication'. REST and the versioning rules in the same section are satisfied; the gap is that the canonical agent runtime (docker/gyre-agent/agent-runner.mjs, the entrypoint mandated by specs/system/agent-runtime.md) never opens a WebSocket — every agent→server interaction is HTTP fetch (heartbeat POST /api/v1/agents/{id}/heartbeat, logs, MCP JSON-RPC POST /mcp, completion) and server→agent delivery is MCP/REST polling (gyre_message_poll / GET /api/v1/agents/{id}/messages), which the runner does not even perform — so the agent receives no server-directed signal in real time. Much of the server side already exists (ws.rs authenticates agent JWTs via the Auth message, subscribes agent connections to their own workspace, and delivers Destination::Agent bus messages to the agent's own live session). Implement: (1) Agent-side WS client — agent-runner.mjs opens and maintains a WebSocket connection to /ws for the entire session using the agent's token (Auth message, Subscribe to its own workspace), with reconnect/backoff so the transport survives transient drops. (2) Real-time inbound — the runner receives server-pushed agent-directed messages (Destination::Agent: gate-failure feedback, shutdown commands, spec-rejection grace signals) over the WS and acts on them per the protocol (e.g. a shutdown signal triggers the graceful completion/stop path), instead of never receiving them. (3) WS-borne liveness — the runner's heartbeat travels over the WS as the primary channel, and the server feeds WS heartbeats into the same domain liveness path as POST /api/v1/agents/{id}/heartbeat so stale-agent detection behaves identically; the HTTP heartbeat and MCP polling surfaces remain as compatibility/fallback per their own canonical contracts (docs/agent-protocol.md, agent-gates.md live-session broadcast) — the spec says primary, not exclusive, and those surfaces keep their documented semantics. (4) Documentation — update docs/agent-protocol.md to describe the WS as the agent's primary channel with HTTP/MCP as fallback (docs, not spec amendment). (5) Coordination — task-272 (agent-workflow pre-PR local verification) modifies agent-runner.mjs's completion flow under a different contract; coordinate merge order, do not fold its behavior in. (6) gRPC bullet — per the supplied audit, the realized platform is a single server binary with no internal service-to-service path (only the scoped OTLP trace receiver and the external SPIFFE Workload API client exist), so the 'where performance matters' conditional has no trigger; that reading is recorded for the controller and this task implements no gRPC service-to-service surface either way.

## Acceptance Criteria

- [ ] agent-runner.mjs maintains an authenticated WS connection for the session's duration (server-side observable: the connection is registered under the agent's verified identity).
- [ ] A bus message sent to Destination::Agent(<id>) is received by the running agent over the WebSocket without any polling, and the runner reacts to it (shutdown signal → graceful stop within the specced window).
- [ ] WS heartbeats keep the agent Alive in stale-agent detection through the same liveness path as the HTTP heartbeat: with no HTTP heartbeat sent, an agent heartbeating only over WS is not marked Dead; with the WS heartbeat path removed, the agent is marked Dead per the existing staleness window.
- [ ] The HTTP heartbeat, MCP polling, and /ws human-client (SPA/CLI) paths keep their current documented semantics — fallback and UI channels, unchanged.

## Verification intent

Start the real agent runner (preview/local mode) against a live test server, send a shutdown message to that agent's id on the bus, and assert the runner initiates its graceful stop within a bounded window: on the current code the probe times out — the runner never connects (zero WebSocket usage in agent-runner.mjs) and never polls, so no server-directed signal can reach it; the fixed transport must deliver the message over the WS and produce the reaction. A second probe: disable the WS-heartbeat feed and assert stale-agent detection marks the agent Dead, proving WS heartbeats genuinely drive liveness rather than existing as an unused client code path.

Update the relevant coverage rows only after implementing the behavior, with concrete source evidence. Do not mark unrelated rows complete or claim independent verification before it occurs.

## Discovery evidence

Canonical spec: `specs/development/architecture.md`. Source: `678a8ac3e3bfeafeb66cdb47e897b44e8ecda2ce`; plan: `68777e1ea1f58acc822cb4f700aaa458b561ec119531c168e7c1d07d2026a23e`.

[
  {
    "id": "5e7a44c5f15c34d70101b959",
    "decision": "gap",
    "requirement_quote": "- **REST** for admin panel, user-facing endpoints, and external integrations.\n- **gRPC** for internal service-to-service communication where performance matters.\n- **WebSocket** as the primary transport for agent ↔ server communication.",
    "detail": "The REST bullet is satisfied; the WebSocket bullet is a gap, so the section is a gap. (1) REST: the domain API is REST, registered under literal /api/v1/ path prefixes (331 .route registrations in api/mod.rs plus the top-level router in lib.rs), covering the admin panel (api/admin.rs), user-facing endpoints, and external integrations (webhooks, SCIM, MCP-over-HTTP). (2) WebSocket as the primary transport for agent-servers communication: GAP - the canonical agent runtime (docker/gyre-agent/agent-runner.mjs, the entrypoint mandated by specs/system/agent-runtime.md) never opens a WebSocket (zero matches for WebSocket/ws:// in the file): every agent-to-server interaction is HTTP - heartbeat (POST /api/v1/agents/{id}/heartbeat), log upload (POST /api/v1/agents/{id}/logs), MCP JSON-RPC (POST /mcp), completion (POST /api/v1/agents/{id}/complete) - and server-to-agent message delivery is MCP/REST polling (gyre_message_poll, GET /api/v1/agents/{id}/messages) per docs/agent-protocol.md. The /ws endpoint is consumed by the browser dashboard SPA (web/src/lib/ws.js) and the CLI TUI (crates/gyre-cli/src/ws.rs) - human clients receiving event push. WebSocket is a UI push channel in the realized system, not the primary agent transport; making it the primary transport would require a WebSocket client in the agent runtime and server push of agent-directed messages over it. (3) gRPC: the realized platform is a single server binary with no internal service-to-service communication, so the 'where performance matters' condition has no trigger; the gRPC surfaces that exist are the OTLP trace receiver (gate-time telemetry ingest from the application under test - explicitly scoped, not general internal service-to-service) and the SPIFFE Workload API client to an external SPIRE server (crates/gyre-spiffe/src/workload_api.rs), neither of which is internal service-to-service communication. This is recorded for the controller to rule whether the monolithic realization already satisfies the conditional (no such communication exists to carry) or whether the spec's services decomposition was never built; it is not counted as an independent missing behavior because there is no realized internal service-to-service path that performs matters.",
    "evidence": [
      {
        "file": "crates/gyre-server/src/api/mod.rs",
        "quote": "\"/api/v1/merge-requests\",",
        "detail": "The REST bullet is satisfied: the domain API is REST, registered under literal /api/v1/ path prefixes (331 .route registrations in this file alone), covering admin (api/admin.rs), user-facing, and external-integration surfaces."
      },
      {
        "file": "docker/gyre-agent/agent-runner.mjs",
        "quote": "const resp = await fetch(`${serverUrl}/api/v1/agents/${agentId}/heartbeat`, {",
        "detail": "The canonical agent runtime (the entrypoint mandated by specs/system/agent-runtime.md) never opens a WebSocket - grep for WebSocket/ws:// in the 778-line file returns zero matches. All agent-to-server communication is HTTP fetch: heartbeat, log upload (POST /api/v1/agents/{id}/logs), MCP JSON-RPC (POST /mcp), and completion (POST /api/v1/agents/{id}/complete). The specced primary transport for agent-server communication is absent from the agent side."
      },
      {
        "file": "web/src/lib/ws.js",
        "quote": "ws = new WebSocket(`${protocol}//${window.location.host}/ws`);",
        "detail": "The /ws WebSocket endpoint is consumed by the browser dashboard SPA (and by the CLI TUI via crates/gyre-cli/src/ws.rs, DEFAULT_SERVER ws://localhost:3000/ws) - human clients receiving domain-event push. WebSocket serves the UI, not the agent transport, so it is not 'the primary transport for agent-server communication' in either direction."
      },
      {
        "file": "docs/agent-protocol.md",
        "quote": "can poll the message via the MCP surface (`gyre_message_poll`, which derives the inbox from the verified token subject — inherently own-inbox)",
        "detail": "Server-to-agent message delivery is also not WebSocket: the documented delivery path for agent inbound messages is MCP/REST polling (gyre_message_poll; the REST inbox route GET /api/v1/agents/{id}/messages exists for the same purpose), confirming the transport gap runs in both directions."
      },
      {
        "file": "crates/gyre-server/src/otlp_receiver.rs",
        "quote": "//! Lightweight OTLP gRPC receiver for gate-time trace capture (HSI §3a).",
        "detail": "On the gRPC bullet: the only in-process gRPC server is this OTLP trace receiver, scoped to gate-time trace capture from the application under test (the module doc explicitly disclaims being a general-purpose observability backend); the other tonic dependency is the SPIFFE Workload API client to an external SPIRE server. Neither is internal service-to-service communication - the realized platform is a single server binary with no internal service-to-service path, so the 'where performance matters' condition has no trigger."
      }
    ]
  }
]

## Shipped

The canonical agent runtime now runs the WebSocket as its primary transport, and the server feeds WS-borne liveness through the same domain path as the HTTP heartbeat.

### Agent-side WS client (docker/gyre-agent/agent-runner.mjs)

- `connectWs()` opens one WS to `/ws` for the whole session: `Auth` with the agent token (connection registered under the verified identity server-side), then `Subscribe` scoped to the agent's own workspace (`GYRE_WORKSPACE_ID`, newly injected by `api/spawn.rs` in both `launch_agent_process` and `launch_taskless_agent_process`) with a `last_seen` replay bound that advances on every received bus message so a reconnect replays exactly what was missed and never re-fires stale persisted events.
- Reconnect with exponential backoff (`GYRE_WS_RECONNECT_BASE_MS` default 500ms doubling to `GYRE_WS_RECONNECT_MAX_MS` default 15s); backoff resets after a successful auth. Transport is torn down on every terminal path (`stopWsTransport`) so no timer keeps the process alive.

### Real-time inbound

- Pushed bus envelopes (no `type` tag) are handled by `handleBusMessage`. A shutdown command (`kind: status_update`, `payload.action: shutdown`) sets the shutdown signal, aborts the in-flight SDK query via the `options.abortController` passed to `query()`, and routes the run into `gracefulStop()` — provenance upload best-effort, log, exit 0, no gates, no completion/MR for server-cancelled work. The signal is honored from the query loop, the gate loop, and the error path. Other agent-directed messages are surfaced on the runner log and uploaded to the server log.

### WS-borne liveness (server)

- `gyre-common::WsMessage` gained `Heartbeat { svid, stack_fingerprint }` / `HeartbeatAck { success, message }` (additive, `type`-tagged per the versioning rules).
- `ws.rs` accepts `Heartbeat` only on agent-verified connections (human/admin principals get a failure ack and no liveness advance) and calls the new shared `api::agents::record_agent_heartbeat` — the exact domain path behind `PUT /api/v1/agents/{id}/heartbeat` (fail-closed SVID verification via `attest_session`, `Agent::heartbeat`, WireGuard peer `last_seen`, the agent-heartbeat audit trail). The REST handler now delegates to the same function; the HTTP heartbeat, MCP polling (`gyre_message_poll`), and `/ws` human-client (SPA/CLI) surfaces keep their documented semantics — the runner uses the HTTP heartbeat only as fallback when the WS transport is down or the ack fails/rejects.

### Documentation

- `docs/agent-protocol.md`: new **WebSocket primary transport** block (Auth/Subscribe, backoff, Heartbeat/HeartbeatAck semantics, same-liveness-path statement, real-time shutdown behavior, unchanged fallbacks) plus the `GYRE_WORKSPACE_ID` env-var row. `docs/api-reference.md`: `Heartbeat`/`HeartbeatAck` added to the WS protocol examples with the agent-connections-only note. No spec amendment; the gRPC bullet is recorded for the controller (no internal service-to-service path exists — no trigger), and task-272's completion-flow contract was not folded in (its `GYRE_LOCAL_GATES` flow is untouched and the WS stop path deliberately bypasses it for server-cancelled work).

### Test evidence

- `node --test docker/gyre-agent/agent-runner.test.mjs`: 15/15 pass, including three new suites that run the real runner as a child process against a WS-speaking server — authenticated session + heartbeats ride the WS with zero HTTP heartbeat fallback while connected + completion flow intact; server-pushed shutdown (no polling) → graceful stop inside the window (exit 0, no completion call, query loop ended); backoff reconnect after a transient drop. Log: `/tmp/stage/review-evidence/node-agent-runner-tests.log`.
- `cargo check -p gyre-server` and `cargo check -p gyre-server --tests`: exit 0 (serialized build wrapper). `cargo fmt --check -p gyre-server -p gyre-common`: clean after formatting the checkpoint's new Rust code.
- Deferred to the independent verifier (too heavy for this sandbox's 4 GiB budget; recorded in `/tmp/stage/review-evidence/README.md`): `cargo test -p gyre-server ws_agent_heartbeat_keeps_agent_alive_in_stale_detection`, `ws_agent_heartbeat_without_svid_is_rejected`, `ws_heartbeat_from_human_principal_is_rejected` (WS-only heartbeat keeps the agent Active through `stale_agents::run_once` while a no-heartbeat control agent is Stopped by the same window; rejected heartbeats do not advance liveness and are audited as Failure; human principals cannot advance liveness over WS), plus `cargo test -p gyre-common protocol` for the serde roundtrips.
