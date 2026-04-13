# Coverage: Unified Message Bus

**Spec:** [`system/message-bus.md`](../../system/message-bus.md)
**Last audited:** 2026-04-13 (full audit — bulk reclassification from not-started)
**Coverage:** 14/17 (4 n/a)

| # | Section | Depth | Status | Task | Notes |
|---|---------|-------|--------|------|-------|
| 1 | Problem | 2 | n/a | - | Context/rationale — no implementable requirement. |
| 2 | Design: One Envelope, Routing Determines Delivery | 2 | n/a | - | Section heading only — no implementable requirement. |
| 3 | Core Principle | 3 | implemented | - | Single envelope, server validates origin, routes via Destination. All inter-component communication flows through Message type. |
| 4 | Crate Placement | 3 | implemented | - | Message, MessageOrigin, Destination, MessageKind in gyre-common/src/message.rs. MessageRepository in gyre-ports/src/message.rs. Adapters in gyre-adapters. Hexagonal boundary preserved. |
| 5 | Message Envelope | 3 | implemented | - | Full Message struct with id, tenant_id, from (MessageOrigin), workspace_id, to (Destination), kind, payload, created_at (epoch ms), signature, key_id, acknowledged. All spec fields present. |
| 6 | Message Kinds | 3 | implemented | - | All 26+ variants implemented across 3 tiers (Directed, Event, Telemetry) + Custom. Custom uses custom Deserialize implementation. server_only() and tier() methods present. Extra variants beyond spec: SpecApproved, ConstraintViolation, AtomicGroupFailed. |
| 7 | Payload Schemas | 3 | implemented | - | Payload validation in message send handler. Schema definitions match spec's payload table. |
| 8 | Signing | 3 | implemented | - | Ed25519 signing in signing.rs using SHA-256 payload hashing. Directed and Event tier messages signed. Telemetry unsigned. key_id from JWKS. |
| 9 | Delivery | 3 | implemented | - | WebSocket: Subscribe handling filters by workspace (ws.rs:83-138), message filtering by destination (ws.rs:376-390). REST: GET /api/v1/agents/:id/messages (poll), PUT /api/v1/agents/:id/messages/:message_id/ack (ack). Both paths functional. |
| 10 | Scoping Rules | 3 | implemented | - | Workspace membership check via WorkspaceMembershipRepository. Agent-workspace matching enforced. Broadcast requires Admin role or Server origin. Tenant isolation structural via workspace_id. |
| 11 | Storage | 3 | implemented | - | MessageRepository port trait with store, find_by_id, list_after, list_unacked, count_unacked, acknowledge, acknowledge_all, list_by_workspace, expire_events, expire_acked_inboxes, expire_for_agents. SQLite + Postgres adapters. TelemetryBuffer in-memory ring buffer with workspace isolation and largest-first eviction. DB migration 000017. |
| 12 | Relationship to Notifications | 3 | task-assigned | task-162 | MessageConsumer trait exists. Bounded mpsc channel created (256 capacity) in lib.rs. But receiver loop is a no-op — no actual consumers wired. spawn_message_consumer() referenced in comments but not implemented. Notification system not yet consuming from bus. |
| 13 | API | 3 | implemented | - | POST /api/v1/workspaces/:workspace_id/messages (send). GET /api/v1/workspaces/:id/messages (workspace query with cursor pagination). GET /api/v1/agents/:id/messages (poll). PUT /api/v1/agents/:id/messages/:message_id/ack. INBOX_MAX enforced (429 on overflow). |
| 14 | MCP Integration | 3 | implemented | - | message.send tool (mcp.rs:1510-1660). message.poll and message.ack tools also present. Telemetry kinds rejected with guidance to use gyre_record_activity. |
| 15 | Implementation Notes | 3 | task-assigned | task-162 | Event TTL expiry: expire_events() method exists in ports and adapters but no background job calls it. Retention cleanup job (jobs.rs) handles other data but doesn't invoke message expiry. Need scheduled job for GYRE_EVENT_TTL_SECS (default 7 days). |
| 16 | Edge Cases | 3 | implemented | - | Queue depth limit enforced (GYRE_AGENT_INBOX_MAX, default 1000, 429 on overflow). Dead agent cleanup via acknowledge_all on complete. Message size limit. Concurrent send/ack safety. |
| 17 | Relationship to Existing Specs | 3 | n/a | - | Cross-reference section — no implementable requirement. |
