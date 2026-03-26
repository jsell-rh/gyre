# M35: Unified Message Bus — Signed Envelope, Three-Tier Model

**Status:** Done
**Milestone:** M35

---

## Summary

M35 replaces Gyre's fragmented communication (REST inbox, domain events, ActivityStore) with a unified signed message bus. All inter-component messages use a single signed envelope (Ed25519). Three delivery tiers cover all communication patterns: directed (per-agent, acked, persisted), telemetry (ring buffer, ephemeral), and broadcast (fan-out).

Related spec: [specs/system/message-bus.md](../system/message-bus.md)

---

## M35.1 — Signed Message Envelope (PR #383)

All messages share a common envelope:

```json
{
  "id": "<uuid>",
  "kind": "<MessageKind variant>",
  "workspace_id": "<uuid>",
  "sender_id": "<agent_id or system>",
  "recipient_id": "<agent_id | null>",
  "payload": { ... },
  "signature": "<base64url Ed25519 sig>",
  "created_at": "<ISO8601>",
  "expires_at": "<ISO8601 | null>"
}
```

The server signs outbound messages with its Ed25519 keypair. Agents verify the signature using the public key available at `GET /api/v1/identity/public-key`.

---

## M35.2 — Three-Tier Delivery Model

| Tier | Storage | Retention | Use case |
|---|---|---|---|
| **Directed** | Persisted DB table | Until acked (TTL: `GYRE_AGENT_INBOX_MAX=100`) | Agent inbox, task assignments, gate results |
| **Telemetry** | In-memory ring buffer | `GYRE_TELEMETRY_BUFFER_SIZE=10000` per workspace | Metrics, heartbeats, high-frequency events |
| **Broadcast** | Fan-out to all workspace members | No persistence | Workspace-wide notifications, presence |

---

## M35.3 — API Endpoints (PR #383, PR #384)

| Method | Path | Auth | Description |
|---|---|---|---|
| `POST` | `/api/v1/workspaces/{id}/messages` | Agent+ | Send a message (directed/broadcast/telemetry) |
| `GET` | `/api/v1/workspaces/{id}/messages` | Agent+ | Poll directed inbox; supports `?since=<cursor>` |
| `POST` | `/api/v1/workspaces/{id}/messages/{msg_id}/ack` | Agent+ | Acknowledge a directed message |
| `GET` | `/api/v1/workspaces/{id}/messages/telemetry` | Developer+ | Read telemetry ring buffer |

**Environment variables:**
- `GYRE_AGENT_INBOX_MAX=100` — max unacked directed messages per agent before oldest evicted
- `GYRE_TELEMETRY_BUFFER_SIZE=10000` — telemetry ring buffer size per workspace

---

## M35.4 — Storage Layer (PR #384)

`MessageRepository` port and Diesel adapter (`gyre-adapters`) persist directed messages to the `messages` table (migration 000016). Telemetry uses an in-memory `VecDeque` keyed by workspace. Broadcast is fan-out via the WebSocket hub.

---

## Acceptance Criteria

- [x] All messages use signed Ed25519 envelope
- [x] `POST /api/v1/workspaces/{id}/messages` creates directed, broadcast, or telemetry messages
- [x] `GET /api/v1/workspaces/{id}/messages` returns agent's directed inbox with cursor pagination
- [x] `POST /api/v1/workspaces/{id}/messages/{id}/ack` marks message delivered
- [x] Telemetry ring buffer respects `GYRE_TELEMETRY_BUFFER_SIZE` per workspace
- [x] Directed inbox evicts oldest when `GYRE_AGENT_INBOX_MAX` exceeded
- [x] Migration 000016 creates `messages` table with indexes
- [x] CI: `spec/message-bus` linting enforces no unenveloped message patterns

---

## Implementation Notes

- `crates/gyre-common/src/message.rs` — `Message`, `MessageKind`, `MessageEnvelope` types
- `crates/gyre-ports/src/message.rs` — `MessageRepository` port trait
- `crates/gyre-adapters/src/sqlite/message.rs` and `postgres/message.rs` — Diesel adapters
- `crates/gyre-server/src/api/messages.rs` — HTTP handlers
- `crates/gyre-server/src/mem.rs` — in-memory telemetry ring buffers
- Migration: `crates/gyre-adapters/migrations/2026-03-24-000017_messages/`
