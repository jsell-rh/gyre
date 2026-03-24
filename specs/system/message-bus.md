# Unified Message Bus

## Problem

Agent communication currently flows through three overlapping channels with different type systems, delivery models, and storage mechanisms:

1. **REST inbox** (`POST/GET /api/v1/agents/:id/messages`) — poll-and-drain, stored as a serialized JSON array in KvJsonStore, 5 `MessageType` variants, no sender authentication, `from` is a free-form string, race conditions on concurrent writes, messages lost on drain if receiver crashes before processing.

2. **WebSocket domain events** — server-generated broadcast to ALL connected clients, 14 `DomainEvent` variants, in-memory `broadcast::channel(256)`, no workspace scoping, events dropped if no one is listening or if a client lags.

3. **WebSocket activity events** — agent-generated AG-UI telemetry broadcast to ALL clients, 7 `AgEventType` variants, in-memory `ActivityStore` ring buffer (1000 entries), overlaps with domain events on the same WebSocket.

Problems:
- An agent deciding how to communicate must choose between three mechanisms with different semantics
- No message is signed or attestable — there is no proof that a specific agent sent a specific message
- The REST inbox has no sender validation — the `from` field is caller-supplied, not derived from auth
- Domain events broadcast to all clients regardless of workspace — tenant/workspace isolation is violated
- No delivery guarantees on point-to-point messages — drain-on-read loses messages on receiver failure
- No routing — to send a message you must know the recipient's UUID
- Activity telemetry and domain events share a WebSocket but are separate type systems

## Design: One Envelope, Routing Determines Delivery

### Core Principle

All inter-component communication flows through one message type with routing metadata. The server is the notary — it validates sender identity, signs directed messages, and routes based on destination. No message broker, no external queue. The existing WebSocket + a database table + the existing in-memory broadcast channel.

### Message Envelope

```rust
pub struct Message {
    pub id: Id,
    pub tenant_id: Id,              // Tenant isolation — derived from sender context

    // Identity — WHO sent this
    pub from: MessageOrigin,        // Derived server-side, never caller-supplied
    pub workspace_id: Id,           // Scoping — messages never leak across workspaces

    // Routing — WHO receives this
    pub to: Destination,

    // Content — WHAT was said
    pub kind: MessageKind,          // "kind" everywhere — struct, API, DB column
    pub payload: Option<Value>,     // Structured data specific to the kind

    // Attestation — PROOF it happened (None for telemetry tier)
    pub created_at: u64,
    pub signature: Option<String>,  // Ed25519 — present on directed + event tier, absent on telemetry
    pub key_id: Option<String>,     // kid from server's JWKS

    // Delivery state (point-to-point only)
    pub acknowledged: bool,
}

pub enum MessageOrigin {
    Server,                         // Domain events, system notifications
    Agent(Id),                      // Agent-to-agent or agent-to-server
    User(Id),                       // Human-initiated (dashboard, CLI)
}

pub enum Destination {
    Agent(Id),                      // Point-to-point: one specific agent
    Workspace(Id),                  // Fan-out: all agents/clients in a workspace
    Broadcast,                      // All connected clients (admin dashboards only)
}
```

**Origin resolution from auth context:**

| Auth mechanism | Maps to |
|---|---|
| Agent JWT (EdDSA, starts with `ey`) | `Agent(sub claim)` |
| Keycloak JWT (OIDC) | `User(user_id from token)` |
| API key (`gyre_*`) | `User(user_id from key lookup)` |
| Global `GYRE_AUTH_TOKEN` | `Server` |

**Note on `Destination::Repo`:** The earlier draft included `Repo(Id)` as a destination. This is removed. Repo-scoped delivery is achieved by sending to `Workspace(id)` with a repo-specific `kind` — the repo_id is in the payload. This avoids a lookup indirection (repo → workspace) on every delivery and keeps the routing model simple.

### Message Kinds

One enum replaces the three current type systems. Organized into three **tiers** that determine signing and persistence behavior:

```rust
pub enum MessageKind {
    // ── Tier 1: Directed (signed + persisted + ack-based) ─────────────
    TaskAssignment,
    ReviewRequest,
    StatusUpdate,
    Escalation,
    BudgetWarning,
    BudgetExhausted,

    // ── Tier 2: Events (signed + persisted with TTL) ──────────────────
    AgentCreated,
    AgentStatusChanged,
    AgentContainerSpawned,
    TaskCreated,
    TaskTransitioned,
    MrCreated,
    MrStatusChanged,
    QueueUpdated,
    PushRejected,
    PushAccepted,
    SpecChanged,
    GateFailure,
    StaleSpecWarning,
    SpeculativeConflict,
    SpeculativeMergeClean,
    HotFilesChanged,
    ActivityRecorded,
    DataSeeded,

    // ── Tier 3: Telemetry (unsigned + in-memory only) ─────────────────
    ToolCallStart,
    ToolCallEnd,
    TextMessageContent,
    RunStarted,
    RunFinished,
    StateChanged,
    AgentError,

    // ── Custom ────────────────────────────────────────────────────────
    Custom(String),
}
```

**Tier behavior:**

| Tier | Signed | Persisted | Delivery | Use case |
|---|---|---|---|---|
| **Directed** | Yes | Yes, until acked | At-least-once (poll + push) | Agent-to-agent commands |
| **Event** | Yes | Yes, TTL-based | Best-effort push + queryable | System state changes |
| **Telemetry** | No | No (in-memory ring buffer) | Best-effort push only | High-frequency AG-UI observability |

Telemetry stays in-memory (like today's `ActivityStore`) because signing and persisting hundreds of events per second per agent is wasteful for fire-and-forget observability data. If an agent needs provable telemetry, it sends a `Custom("attested_telemetry")` message at Directed tier.

**`Custom(String)` serialization:** Uses serde `#[serde(tag = "kind")]` with an `other` fallback. A `Custom("my_event")` serializes as `{"kind": "my_event"}`. Deserialization: known variants match first; unknown strings fall through to `Custom(s)`. This means custom kind names must not collide with built-in variant names — the server rejects `Custom("AgentCreated")` with 400.

### Payload Schemas

Each `MessageKind` has a defined payload schema. Key schemas:

| Kind | Payload fields | Required |
|---|---|---|
| `TaskAssignment` | `task_id: Id, spec_ref: Option<String>` | `task_id` |
| `ReviewRequest` | `mr_id: Id` | `mr_id` |
| `StatusUpdate` | `status: String, summary: String` | both |
| `Escalation` | `reason: String, context: Option<String>` | `reason` |
| `AgentCreated` | `agent_id: Id` | `agent_id` |
| `AgentStatusChanged` | `agent_id: Id, status: String` | both |
| `TaskCreated` | `task_id: Id` | `task_id` |
| `TaskTransitioned` | `task_id: Id, status: String` | both |
| `MrCreated` | `mr_id: Id` | `mr_id` |
| `MrStatusChanged` | `mr_id: Id, status: String` | both |
| `PushRejected` | `repo_id: Id, branch: String, agent_id: Id, reason: String` | all |
| `PushAccepted` | `repo_id: Id, branch: String, agent_id: Id, commit_count: u32, task_id: Option<Id>` | `repo_id`, `branch`, `agent_id` |
| `GateFailure` | `mr_id: Id, gate_name: String, gate_type: String, status: String, output: String, spec_ref: Option<String>, gate_agent_id: Id` | `mr_id`, `gate_name` |
| `SpecChanged` | `repo_id: Id, spec_path: String, change_kind: String, task_id: Id` | all |
| `BudgetWarning` | `agent_id: Id, workspace_id: Id, usage_pct: f64` | all |
| `BudgetExhausted` | `agent_id: Id, workspace_id: Id, grace_secs: u64` | all |
| `ToolCallStart` | `agent_id: Id, tool_name: String` | both |
| `ToolCallEnd` | `agent_id: Id, tool_name: String, duration_ms: u64` | all |
| `RunStarted` | `agent_id: Id, task_id: Option<Id>` | `agent_id` |
| `RunFinished` | `agent_id: Id, task_id: Option<Id>` | `agent_id` |
| `QueueUpdated` | (none) | — |
| `DataSeeded` | (none) | — |
| `Custom(name)` | Any valid JSON object | — |

The server validates payload against the schema on receipt. Invalid payloads are rejected with 400.

### Signing

The server signs Directed and Event tier messages using its existing Ed25519 OIDC signing key (`AgentSigningKey`).

**Signed bytes:** The signature covers a deterministic byte string built by concatenating fixed-order fields separated by null bytes:

```
sign_input = id + '\0' + from_type + '\0' + from_id + '\0' + workspace_id + '\0' +
             to_type + '\0' + to_id + '\0' + kind + '\0' + sha256(payload_json) + '\0' +
             created_at_str
```

This is consistent with the existing commit signature approach (`commit_signatures.rs`) which signs raw byte content, not JCS-canonicalized JSON. The payload is hashed (SHA-256) rather than included directly, keeping the sign input bounded regardless of payload size.

**Verification:** Any party can verify by fetching the server's public key from `GET /.well-known/jwks.json` using the `key_id`, reconstructing the sign input from the message fields, and verifying the Ed25519 signature.

**Telemetry tier is unsigned.** `signature` and `key_id` are `None`. These messages are observability data, not attestable claims. The server is the origin of truth — it received the telemetry from an authenticated agent, and that's sufficient for dashboards.

### Delivery

#### WebSocket (primary — push delivery)

Clients connect to `GET /ws` and authenticate as today. After auth, the client sends a subscription message:

```json
{"type": "Subscribe", "scopes": [{"workspace_id": "ws-123"}, {"workspace_id": "ws-456"}]}
```

The `Subscribe` variant is added to `gyre_common::WsMessage`. During migration, clients that don't send `Subscribe` receive legacy broadcast behavior (all domain events, unscoped).

The server filters outgoing messages:
- `Destination::Agent(id)` — delivered only to that agent's WebSocket connection (matched by auth identity)
- `Destination::Workspace(id)` — delivered to all clients whose subscription includes that workspace
- `Destination::Broadcast` — delivered to all connected clients (admin dashboards, legacy clients)

**Reconnection and catch-up:** When a WebSocket reconnects, the client includes a `last_seen` timestamp in the Subscribe message:

```json
{"type": "Subscribe", "scopes": [{"workspace_id": "ws-123"}], "last_seen": 1711324700}
```

The server replays persisted Event-tier messages newer than `last_seen` for the subscribed workspaces. Telemetry-tier messages are not replayed (they're ephemeral). Directed messages are always available via REST poll regardless of WebSocket state.

#### REST (fallback — poll delivery)

For agents that don't maintain a WebSocket (e.g., short-lived container agents):

```
GET /api/v1/agents/:id/messages?acknowledged=false
```

Returns unacknowledged Directed-tier messages for the agent. Does NOT drain — messages persist until acknowledged:

```
PUT /api/v1/agents/:id/messages/:message_id/ack
```

Acknowledgment is **idempotent** — acking an already-acked message returns 200, not an error.

The agent must be the authenticated caller (verified from JWT `sub` claim). An agent cannot read another agent's inbox.

#### Delivery Guarantees

| Tier | Guarantee | Persistence |
|---|---|---|
| **Directed** | At-least-once (persisted until ack, redelivered on poll) | DB — no TTL, expires only on ack or agent completion |
| **Event** | Best-effort push + queryable history | DB — configurable TTL (default 7 days, `GYRE_EVENT_TTL_SECS`) |
| **Telemetry** | Best-effort push only | In-memory ring buffer (configurable, default 10,000 entries per workspace, `GYRE_TELEMETRY_BUFFER_SIZE`) |

**Directed-tier queue depth:** Max 1000 unacked messages per agent (configurable, `GYRE_AGENT_INBOX_MAX`). When the limit is reached, new messages to that agent are rejected with 429 and the sender receives a `BudgetWarning`-style notification. Messages are NOT silently dropped — this preserves the at-least-once guarantee. The agent or an admin must ack or the agent must be completed/killed to free the inbox.

**Agent completion cleanup:** When an agent completes (`POST /api/v1/agents/:id/complete`), all unacked Directed messages in its inbox are marked with `acknowledged: true` and `ack_reason: "agent_completed"`. They remain in the DB for audit but are no longer redelivered.

### Scoping Rules

1. **Agents can only send Directed messages to agents in the same workspace.** Enforced by comparing the sender's JWT `workspace_id` claim with the recipient agent's `workspace_id`. Returns 403 if mismatched.

2. **Cross-workspace messaging is server-mediated.** Workspace orchestrators do not message each other directly. Instead, cross-workspace coordination flows through server-originated events. When a Workspace Orchestrator creates a cross-repo task or MR dependency (via existing REST endpoints), the server emits the appropriate Event-tier messages (`TaskCreated`, `MrCreated`) into each affected workspace. The orchestrators observe these events in their own workspace's message stream. This avoids the need for cross-workspace agent messaging entirely.

3. **Server-originated messages inherit the workspace of the entity they describe.** An `AgentCreated` event for an agent in workspace X has `workspace_id: X` and is only delivered to clients subscribed to workspace X.

4. **Broadcast destination requires Server origin or Admin role.** Agent JWTs cannot target Broadcast. API key callers with Admin role can target Broadcast for operational announcements.

5. **Workspace fan-out requires workspace membership.** An agent can only send `Destination::Workspace(id)` if its JWT `workspace_id` matches `id`. Users can target any workspace they are a member of.

### Storage

#### Port Trait

```rust
#[async_trait]
pub trait MessageRepository: Send + Sync {
    /// Store a signed message (Directed or Event tier).
    async fn store(&self, message: &Message) -> Result<()>;

    /// Find a message by ID.
    async fn find_by_id(&self, id: &Id) -> Result<Option<Message>>;

    /// List unacknowledged Directed messages for an agent.
    async fn list_unacked(&self, agent_id: &Id) -> Result<Vec<Message>>;

    /// Count unacknowledged Directed messages for an agent (for limit enforcement).
    async fn count_unacked(&self, agent_id: &Id) -> Result<u64>;

    /// Acknowledge a message. Idempotent.
    async fn acknowledge(&self, message_id: &Id, agent_id: &Id) -> Result<()>;

    /// Bulk-acknowledge all messages for an agent (on agent completion).
    async fn acknowledge_all(&self, agent_id: &Id, reason: &str) -> Result<u64>;

    /// List messages in a workspace, optionally filtered by kind.
    async fn list_by_workspace(
        &self,
        workspace_id: &Id,
        kind: Option<&str>,
        since: Option<u64>,
        limit: Option<usize>,
    ) -> Result<Vec<Message>>;

    /// Delete messages older than the given epoch. Returns count deleted.
    async fn expire(&self, older_than: u64) -> Result<u64>;
}
```

#### Telemetry Store

Telemetry-tier messages do NOT go through `MessageRepository`. They use an in-memory ring buffer (replacing the current `ActivityStore`):

```rust
pub struct TelemetryBuffer {
    /// Per-workspace ring buffers. Key: workspace_id.
    buffers: DashMap<Id, VecDeque<Message>>,
    max_per_workspace: usize,
}
```

Telemetry is pushed through the existing `broadcast::channel` for WebSocket delivery and stored in the ring buffer for `GET /api/v1/activity` queries. This preserves the current performance characteristics.

#### DB Schema

```sql
CREATE TABLE messages (
    id TEXT PRIMARY KEY,
    tenant_id TEXT NOT NULL,
    from_type TEXT NOT NULL,          -- 'server', 'agent', 'user'
    from_id TEXT,                     -- NULL for server
    workspace_id TEXT NOT NULL,
    to_type TEXT NOT NULL,            -- 'agent', 'workspace', 'broadcast'
    to_id TEXT,                       -- NULL for workspace/broadcast
    kind TEXT NOT NULL,               -- 'kind' everywhere, matching the struct field
    payload TEXT,                     -- JSON
    created_at INTEGER NOT NULL,
    signature TEXT,                   -- NULL for telemetry (not stored in DB anyway)
    key_id TEXT,
    acknowledged INTEGER NOT NULL DEFAULT 0,
    ack_reason TEXT                   -- NULL, 'explicit', or 'agent_completed'
);

CREATE INDEX idx_messages_inbox ON messages (to_type, to_id, acknowledged)
    WHERE to_type = 'agent' AND acknowledged = 0;
CREATE INDEX idx_messages_workspace ON messages (workspace_id, created_at DESC);
CREATE INDEX idx_messages_kind ON messages (workspace_id, kind, created_at DESC);
CREATE INDEX idx_messages_expiry ON messages (created_at) WHERE to_type != 'agent';
```

### Relationship to Notifications

The existing `NotificationRepository` with 16 `NotificationType` variants (`MrNeedsReview`, `GateFailure`, `AgentBudgetWarning`, etc.) overlaps with the message bus. The notification system is **not removed** — it serves a different purpose:

- **Messages** are inter-component communication (agent-to-agent, server-to-agent). They are the raw events.
- **Notifications** are user-facing alerts derived from messages. They have read/unread state, priority levels, and are scoped to human users, not agents.

The notification system becomes a **consumer** of the message bus. When the server emits a `GateFailure` message, the notification system creates a `GateFailure` notification for the relevant human users. This replaces the current ad-hoc notification creation scattered across handlers.

### API

#### Sending

```
POST /api/v1/messages
Authorization: Bearer <agent-jwt>

{
    "to": {"agent": "<agent-id>"},   // or {"workspace": "<ws-id>"}
    "kind": "task_assignment",
    "payload": {"task_id": "TASK-42", "spec_ref": "specs/foo.md"}
}
```

Response (201):
```json
{
    "id": "<uuid>",
    "from": {"agent": "<sender-id>"},
    "to": {"agent": "<recipient-id>"},
    "workspace_id": "<ws-id>",
    "kind": "task_assignment",
    "payload": {"task_id": "TASK-42", "spec_ref": "specs/foo.md"},
    "created_at": 1711324800,
    "signature": "<base64-ed25519>",
    "key_id": "<kid>"
}
```

The `from` field is derived server-side from the JWT. The sender does not and cannot specify it.

#### Receiving (poll)

```
GET /api/v1/agents/:id/messages?acknowledged=false
Authorization: Bearer <agent-jwt>
```

Returns unacknowledged Directed-tier messages. Agent must be the authenticated caller.

#### Acknowledging

```
PUT /api/v1/agents/:id/messages/:message_id/ack
Authorization: Bearer <agent-jwt>
```

Idempotent. Returns 200 whether the message was already acked or not.

#### Querying (workspace-scoped)

```
GET /api/v1/workspaces/:id/messages?kind=gate_failure&since=<epoch>&limit=50
Authorization: Bearer <token>
```

Returns Event-tier messages for a workspace. Requires workspace membership. Supports cursor-based pagination via `?after=<message-id>` for paging through large result sets.

#### Telemetry (existing endpoint, new backing store)

```
GET /api/v1/activity?workspace_id=<ws-id>&since=<epoch>&limit=100
```

Queries the in-memory `TelemetryBuffer`. Response format is unchanged from today's `ActivityEventData` — the handler maps `Message` fields to the existing response shape for backwards compatibility.

### MCP Integration

The MCP SSE endpoint (`GET /mcp/sse`) subscribes to the message bus filtered by the authenticated agent's workspace. It maps `MessageKind` variants to AG-UI event types for protocol compatibility:

| MessageKind | AG-UI Event |
|---|---|
| `ToolCallStart` | `TOOL_CALL_START` |
| `ToolCallEnd` | `TOOL_CALL_END` |
| `RunStarted` | `RUN_STARTED` |
| `RunFinished` | `RUN_FINISHED` |
| `TextMessageContent` | `TEXT_MESSAGE_CONTENT` |
| `StateChanged` | `STATE_CHANGED` |
| `AgentError` | `ERROR` |

The MCP `gyre_record_activity` tool becomes a thin wrapper that creates a Telemetry-tier `Message` with the appropriate `MessageKind` and `Destination::Workspace(caller's workspace)`.

### Migration Path

Four phases. Each phase is independently revertible by feature flag (`GYRE_MESSAGE_BUS_PHASE`, default 0 = legacy behavior).

**Phase 1: Dual-write events.**
- Add `MessageRepository`, `Message` struct, `TelemetryBuffer`, new `POST /api/v1/messages` endpoint.
- Server-originated events write to both the new message table AND the existing `event_tx` broadcast channel.
- Consistency model: broadcast is fire-and-forget as today; DB write is authoritative. If DB write fails, the event is logged at `error` level but the broadcast still goes out. This is acceptable because Event tier is best-effort.
- Old WebSocket delivery unchanged. New endpoint available but not required.

**Phase 2: Subscription model.**
- Add `Subscribe` variant to `gyre_common::WsMessage`.
- Clients that send `Subscribe` get workspace-scoped delivery from the new bus.
- Clients that don't subscribe get legacy broadcast behavior (backwards compatible — all domain events, all activity, no scoping).
- Dashboard and CLI updated to send `Subscribe` on connect.

**Phase 3: Migrate agent inbox.**
- New `POST /api/v1/messages` becomes the primary send endpoint.
- Old `POST /api/v1/agents/:id/messages` becomes a **compatibility adapter**: it accepts the old request body (`{from, content, message_type}`), transforms it to the new format (`{to: {agent: id}, kind: message_type.type, payload: content}`), and forwards to the new handler. Returns the new response shape. Logs a deprecation warning.
- Old `GET /api/v1/agents/:id/messages` switches from drain-on-read to ack-based (returns unacked messages, does not drain). This is a behavior change — agents that relied on drain semantics will re-receive messages until they ack. Since most agents poll in a loop, this is benign.
- `FreeText` message type: the compatibility adapter maps `FreeText{body}` to `Custom("free_text")` with `payload: {"body": body}`. A deprecation warning is logged.

**Phase 4: Remove legacy.**
- Drop old `agent_messages` KvJsonStore namespace.
- Remove `ActivityStore` (replaced by `TelemetryBuffer`).
- Remove separate `DomainEvent` broadcast channel (unified bus handles delivery).
- Remove compatibility adapter on old inbox endpoint (returns 410 Gone with pointer to new endpoint).
- Requires: all known agent code and CLI updated to use new endpoints (verified by integration tests).

### Edge Cases

| Scenario | Behavior |
|---|---|
| Send to dead/completed agent | Message stored. `acknowledged` stays false. Cleaned up when `expire()` runs — Directed messages for completed agents expire after 7 days (configurable, `GYRE_DEAD_INBOX_TTL_SECS`). |
| Send to self | Allowed. Useful for self-reminders/scheduling. |
| Workspace fan-out to 100 agents | One DB write + one broadcast channel send. WebSocket push fans out via the broadcast channel (same performance as today). |
| Message too large | `payload` limited to 64KB (configurable via `GYRE_MAX_MESSAGE_SIZE`). Server rejects with 413. |
| Queue depth (unacked inbox) | Max 1000 per agent. New sends rejected with 429. No silent drops. Sender gets error, can retry later or alert. |
| Concurrent sends to same agent | Atomic — each send is a single DB INSERT. |
| Concurrent ack of same message | Idempotent — second ack returns 200. |
| Server restart | Persisted messages survive. Telemetry buffer is lost (acceptable — it's ephemeral). WebSocket connections drop; clients reconnect and re-subscribe with `last_seen`. |
| Signature key rotation | Messages signed with old key remain verifiable — JWKS endpoint serves historical keys. New messages use new key. |
| Broadcast with no workspace_id | Broadcast messages use a sentinel `workspace_id` (the tenant's root ID). They are never stored in the messages table — broadcast is in-memory only. |

### Relationship to Existing Specs

- **Amends** `platform-model.md` §4 (Agent Coordination Protocol) — MCP tools that record activity now go through the message bus; cross-workspace coordination is server-mediated, not agent-to-agent
- **Supersedes** the `WsMessage::ActivityEvent` protocol in `gyre-common/src/protocol.rs` (Phase 4)
- **Supersedes** the `DomainEvent` broadcast in `gyre-server/src/domain_events.rs` (Phase 4)
- **Supersedes** the REST inbox in `gyre-server/src/api/agent_messages.rs` (Phase 4)
- **Consumes from** `notification.rs` — notifications become a downstream consumer of the message bus, not a parallel system
- **Depends on** `hierarchy-enforcement.md` — workspace scoping on messages requires non-optional `workspace_id`
- **Uses** the Ed25519 signing infrastructure from `identity-security.md` (OIDC provider key)
