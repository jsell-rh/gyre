---
title: "Publish reviewed spec backlog for specs/system/message-bus.md"
spec_ref: "GOAL.md — real implementations and meaningful verification"
depends_on: []
progress: complete
commits: []
---

## Required behavior

Publish exactly the independently reviewed missing-work plan below. Only create the assigned task documents and add their exact section bindings. Preserve every existing task, spec, registry entry, coverage status, product file and gate. Task creation is not implementation. Re-read the canonical spec and challenge the audit, decomposition, dependencies and duplicate-work analysis before approving.

{
  "document": "specs/system/message-bus.md",
  "plan_id": "151021041bfb974bd08554e6182cff451df6efb83f37d8a11f6dfdcc1950d05e",
  "audit": {
    "base": "4127964001113dda08e4852404a68ba560ceb764",
    "requirements": "60b177f85a11c736fce98d198e8882e62e8eb05e14ea49d357fc1a2df525acfc",
    "sections": [
      {
        "id": "f3837be36a16881f6408b1ac",
        "decision": "context",
        "requirement_quote": "Unified Message Bus",
        "detail": "Title-only heading. Every requirement of this specification lives in the child headings (Problem, Design and its subsections), each audited and accounted for separately below. No independent requirement attaches to this heading.",
        "evidence": []
      },
      {
        "id": "5f27ef4e0f2200aba4d7efe6",
        "decision": "context",
        "requirement_quote": "Agent communication currently flows through three overlapping channels with different type systems, delivery models, and storage mechanisms:",
        "detail": "Rationale-only section: it describes the pre-bus state (legacy REST inbox in KvJsonStore, unscoped DomainEvent broadcast, ActivityStore telemetry) that motivated the unified bus. The legacy channels it names were functionally replaced (audited under Implementation Notes, 6c71d737); no implementable requirement attaches to this heading itself.",
        "evidence": []
      },
      {
        "id": "0873bb032777343ecb350a92",
        "decision": "context",
        "requirement_quote": "Design: One Envelope, Routing Determines Delivery",
        "detail": "Heading with no body text of its own; all requirements are carried by its children (Core Principle through Relationship to Existing Specs), each audited separately. Recorded here so the ID is accounted for exactly once.",
        "evidence": []
      },
      {
        "id": "587ed8cf6bead4ef7920e4ab",
        "decision": "gap",
        "requirement_quote": "The server is the notary — it validates sender identity, signs directed messages, and routes based on destination. No message broker, no external queue. The existing WebSocket + a database table + the existing in-memory broadcast channel.",
        "detail": "The specced design is three components: the existing WebSocket, a database table, and the existing in-memory broadcast channel. Two of three are on the production path: WebSocket delivery runs over message_broadcast_tx (broadcast::Sender<Message>), and the in-memory channel is that same broadcast channel. The database table is NOT on the wired path: the production AppState constructor build_state (the single constructor main.rs uses) sets `messages: Arc::new(mem::MemMessageRepository::default())` unconditionally, while every sibling durable port in the same struct literal selects the SQLite/PostgreSQL adapter via the store! macro or an inline pg_db/sqlite_db conditional. MemMessageRepository is documented in mem.rs as 'In-memory MessageRepository for tests'. Both Diesel adapters (sqlite/message.rs, postgres/message.rs) and migration 2026-03-24-000017_messages exist and conform, but are dead code in production: with GYRE_DATABASE_URL set, every Directed/Event message (send, poll, ack, WS replay, expiry job — all go through state.messages) is stored in a process-local HashMap, and a server restart loses all of them. This wiring gap also invalidates the Directed at-least-once and Event TTL persistence guarantees (counted under Delivery Guarantees, ce88ec9) and the 'Server restart | Persisted messages survive' edge case (e5d4cc99).",
        "evidence": [
          {
            "file": "crates/gyre-server/src/lib.rs",
            "quote": "messages: Arc::new(mem::MemMessageRepository::default()),",
            "detail": "Inside the production AppState construction in build_state; set unconditionally, with no GYRE_DATABASE_URL conditional. This is the only MessageRepository ever wired into AppState (no `.messages =` reassignment exists anywhere in the server crate)."
          },
          {
            "file": "crates/gyre-server/src/lib.rs",
            "quote": "macro_rules! store {",
            "detail": "The store! macro used by every sibling durable port (workspace_memberships, notifications, meta_specs, ...) selects pg_db/sqlite_db adapters when a database is configured; the `messages` field alone bypasses it, so DB-backed deployments still run the message bus on the in-memory adapter."
          },
          {
            "file": "crates/gyre-server/src/mem.rs",
            "quote": "/// In-memory MessageRepository for tests.",
            "detail": "The adapter that is unconditionally production-wired is itself documented as a test stand-in; its backing store is a HashMap, so Directed/Event messages cannot survive restart in any deployment."
          },
          {
            "file": "crates/gyre-server/src/main.rs",
            "quote": "let state = build_state(&auth_token, &base_url, jwt_config);",
            "detail": "main.rs constructs the server state exclusively through build_state, so the unconditional mem wiring is the production path for every deployment, including GYRE_DATABASE_URL-backed ones."
          },
          {
            "file": "crates/gyre-adapters/src/sqlite/message.rs",
            "quote": "impl MessageRepository for SqliteStorage {",
            "detail": "A conforming durable adapter exists (as does its PostgreSQL twin), proving the 'database table' component could be wired but is not — the messages table and these adapters are unreachable in production."
          }
        ]
      },
      {
        "id": "31dbf3b98bf82f2ccab59cb6",
        "decision": "satisfied",
        "requirement_quote": "`Message`, `MessageOrigin`, `Destination`, and `MessageKind` live in **`gyre-common`** alongside `Id` and `WsMessage`. These are shared wire types used by server, CLI, and agents. `MessageRepository` lives in **`gyre-ports`** (depends only on `gyre-common`). Adapter implementations live in `gyre-adapters`.",
        "detail": "Placement is exactly as specced. Message/MessageOrigin/Destination/MessageKind (plus MessageTier and TelemetryBuffer) are defined in gyre-common/src/message.rs; the MessageRepository port trait is defined in gyre-ports/src/message.rs (importing only gyre-common types); adapter implementations exist in gyre-adapters (sqlite/message.rs, postgres/message.rs) and the mem stand-in in gyre-server. The hexagonal boundary holds: a repo-wide inspection of crates/gyre-domain/src finds no reference to gyre_common::message, MessageKind, or MessageOrigin — the domain crate keeps only its own legacy message_type module (a different, older MessageType enum), so gyre-domain does not import any of the bus types.",
        "evidence": [
          {
            "file": "crates/gyre-common/src/message.rs",
            "quote": "/// The unified message envelope — all inter-component communication flows through this type.",
            "detail": "Message (with MessageOrigin, Destination, MessageKind) is defined in gyre-common alongside Id, exactly as the spec places the shared wire types."
          },
          {
            "file": "crates/gyre-ports/src/message.rs",
            "quote": "pub trait MessageRepository: Send + Sync {",
            "detail": "The MessageRepository port trait lives in gyre-ports and depends only on gyre-common (its imports are anyhow, async_trait, gyre_common::{message::Message, Id})."
          },
          {
            "file": "crates/gyre-adapters/src/sqlite/message.rs",
            "quote": "impl MessageRepository for SqliteStorage {",
            "detail": "Adapter implementations live in gyre-adapters as specced (PostgreSQL twin at crates/gyre-adapters/src/postgres/message.rs)."
          },
          {
            "file": "crates/gyre-domain/src/lib.rs",
            "quote": "pub use message_type::MessageType;",
            "detail": "gyre-domain's only message-related export is its own legacy MessageType enum; inspected call paths (grep of crates/gyre-domain/src for gyre_common::message, MessageKind, MessageOrigin) return no matches, so the domain crate imports none of the bus types and the hexagonal boundary holds."
          }
        ]
      },
      {
        "id": "d221d8125b8e34bf71e2fbbd",
        "decision": "gap",
        "requirement_quote": "The `send_message()` helper function validates: if `destination != Broadcast && workspace_id.is_none()`, it returns `Err` and logs at `error` level. The adapter's `store()` method also returns `Err` on `workspace_id: None` — never panics.",
        "detail": "The envelope itself is implemented faithfully: all 11 fields with the specced types and serde external tagging, created_at in epoch milliseconds, signature/key_id Options, acknowledged bool. The REST send handler does derive workspace_id (None only for Broadcast) and manually strips `acknowledged` from POST responses while GET inbox returns the full struct. Four specific requirements are not met: (1) the specced helper validation 'destination != Broadcast && workspace_id.is_none() → Err + error log' does not exist anywhere — the shared internal emitter emit_scoped_event builds the Message with no such guard; emit_tenant_event constructs messages with workspace_id: None by signature, and while its sole production caller (reconciliation.rs emit_tenant_meta_spec_set_updated) currently passes Destination::Broadcast so no non-Broadcast scopeless message is produced today, the specced guard exists on no construction path and the wired mem adapter would silently store such a message (item 2). (2) The store()-Err-on-None contract is enforced by the SQLite and PostgreSQL adapters (message_to_new_row returns Err), but NOT by MemMessageRepository::store, which silently inserts any message — and the mem adapter is the only one wired into production AppState (lib.rs), so the adapter actually on the wired path fails the contract every DB adapter enforces (a mem-port-contract divergence; currently latent because REST send paths never produce None workspace_id for non-Broadcast, but emit_tenant_event's signature permits it and a wired DB adapter would reject what mem silently accepts). (3) `acknowledged` is excluded from POST responses but NOT from WebSocket pushes: ws.rs serializes the full Message struct (serde_json::to_string(&msg)) on the bus delivery path, contradicting the field's own contract 'Excluded from send (POST) responses and WebSocket pushes'. (4) The normative 'Origin and tenant resolution from auth context' table is violated by the send path's origin derivation: origin_from_auth maps ANY Admin-role caller to MessageOrigin::Server regardless of auth mechanism, where the table requires API-key callers to resolve to User(user_id from key lookup) and Keycloak JWT callers to User(user_id from token), reserving Server origin for the global GYRE_AUTH_TOKEN alone. API-key authentication resolves roles from the user record (auth.rs builds AuthenticatedAgent with roles: user.roles on the API-key path), and the Keycloak/OIDC JWT path does the same, so an Admin-role API-key user — and equally an Admin-role Keycloak user — receives Server origin. Verified consequences in the send handler: (a) the server_only() 403 gate passes for these callers, so an Admin API-key/OIDC user can forge every built-in Event-tier kind and GateResult — the forge authority the spec's security note reserves exclusively to the global token; (b) the User-origin workspace-membership check on Directed sends (scoping rule 3) is skipped for them because it is keyed on MessageOrigin::User. The anti-forgery property the table also protects — `from` is never caller-supplied — does hold on every path, but this is a code-versus-spec deviation on a normative security table and is tracked here as a gap requiring faithful repair (map per auth mechanism, preserving the independent Admin-role Broadcast allowance of scoping rule 6, which the Broadcast guard already enforces via a separate role check) or a human decision through the spec lifecycle; the code comment's spoofable-agent_id rationale is a design justification, not a spec amendment.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/lib.rs",
            "quote": "self.emit_scoped_event(tenant_id, None, to, kind, payload)",
            "detail": "emit_tenant_event hands every message to emit_scoped_event with workspace_id: None; its sole production caller (reconciliation.rs emit_tenant_meta_spec_set_updated) currently passes Destination::Broadcast, so no non-Broadcast scopeless message is produced today — but the shared emit_scoped_event body contains no 'destination != Broadcast && workspace_id.is_none() → Err' validation, so the specced helper guard does not exist on any internal construction path."
          },
          {
            "file": "crates/gyre-server/src/mem.rs",
            "quote": "async fn store(&self, message: &gyre_common::message::Message) -> Result<()> {",
            "detail": "MemMessageRepository::store unconditionally inserts into a HashMap and returns Ok(()) — it never returns Err on workspace_id: None. Because this adapter is the one unconditionally wired into production AppState, the 'adapter's store() returns Err on workspace_id: None' contract is not met by the wired adapter."
          },
          {
            "file": "crates/gyre-adapters/src/sqlite/message.rs",
            "quote": "ok_or_else(|| anyhow!(\"cannot store Broadcast message: workspace_id is None\"))?;",
            "detail": "The SQLite (and identically PostgreSQL) adapters DO enforce the store()-Err-on-None contract, so the port contract diverges across adapters — and the enforcing adapters are the ones that are unwired in production."
          },
          {
            "file": "crates/gyre-server/src/ws.rs",
            "quote": "let payload = serde_json::to_string(&msg).unwrap();",
            "detail": "WebSocket bus delivery serializes the full Message envelope including `acknowledged`, so the field leaks into WS pushes; only the POST response path strips it (send_response removes the key), i.e. the 'excluded from WebSocket pushes' half of the conditional-serialization contract is not implemented."
          },
          {
            "file": "crates/gyre-server/src/api/messages.rs",
            "quote": "obj.remove(\"acknowledged\");",
            "detail": "The send path does implement the other half of the contract: send_response strips `acknowledged` from POST /workspaces/:id/messages responses, and the GET inbox handler returns the full struct so agents see delivery state."
          },
          {
            "file": "crates/gyre-server/src/api/messages.rs",
            "quote": "fn origin_from_auth(auth: &AuthenticatedAgent) -> MessageOrigin {\n    if auth.roles.contains(&gyre_domain::UserRole::Admin) {\n        // Admin callers (global token, Admin-role users) → Server origin.\n        MessageOrigin::Server",
            "detail": "The send path's origin derivation is role-based: any caller whose resolved roles contain Admin maps to MessageOrigin::Server regardless of auth mechanism, where the spec's normative table maps API-key callers to User(user_id from key lookup) and Keycloak JWT callers to User(user_id from token), reserving Server origin for the global GYRE_AUTH_TOKEN alone."
          },
          {
            "file": "crates/gyre-server/src/auth.rs",
            "quote": "if let Ok(Some(user_id)) = state.api_keys.find_user_id(&hash_api_key(token)).await {\n            if let Ok(Some(user)) = state.users.find_by_id(&user_id).await {",
            "detail": "API-key authentication resolves the caller's roles from the looked-up user record (the resulting AuthenticatedAgent is built with roles: user.roles; the Keycloak/OIDC JWT path at auth.rs:2043 does the same), so an Admin-role API-key or Keycloak user genuinely reaches origin_from_auth with the Admin role and receives Server origin — the table's API-key and Keycloak rows are violated for Admin users, with the server_only-forge and rule-3-skip consequences verified in the send handler."
          },
          {
            "file": "crates/gyre-server/src/api/messages.rs",
            "quote": "if kind.server_only() && !matches!(from, MessageOrigin::Server) {",
            "detail": "Consequence (a) of the origin-table deviation: the server_only() 403 gate is keyed on the derived origin, so an Admin-role API-key/OIDC caller mapped to Server origin passes it and can send every built-in Event-tier kind and GateResult — forge authority the spec's security note reserves exclusively to the global GYRE_AUTH_TOKEN."
          },
          {
            "file": "crates/gyre-server/src/api/messages.rs",
            "quote": "// User-to-agent: verify workspace membership.\n        if let MessageOrigin::User(ref user_id) = from {",
            "detail": "Consequence (b) of the origin-table deviation: the rule-3 workspace-membership check on Directed sends is keyed on MessageOrigin::User, so an Admin-role API-key/OIDC caller mapped to Server origin by origin_from_auth skips it entirely."
          }
        ]
      },
      {
        "id": "a8f73059653f7d09adbd36b2",
        "decision": "gap",
        "requirement_quote": "They are server-only AND storage-exempt — they flow through the in-memory broadcast channel only, never hitting `MessageRepository::store()`. The send path must short-circuit before attempting storage for these kinds.",
        "detail": "The enum, tiers, and constraints are implemented: every specced variant exists (plus seven later server-only Event additions such as MrReverted and TaskBlocked), tier() classifies Custom as Event with Directed opt-in via the request 'tier' field, server_only() returns true for all built-in Event kinds plus the GateResult Directed exception exactly as specced, custom Deserialize maps unknown strings to Custom(s), and the send handler rejects Directed+Workspace/Broadcast and Telemetry+Agent/Broadcast with 400 and server_only kinds from non-Server origins with 403. Two specific requirements are unmet: (1) the kind-based storage short-circuit for DataSeeded/QueueUpdated is NOT implemented — the send path and internal emitter short-circuit on Destination::Broadcast only, so a server-originated DataSeeded or QueueUpdated sent to a Workspace destination would be persisted, contradicting 'never hitting MessageRepository::store()' for these kinds; (2) DataSeeded and QueueUpdated have no production emitter at all (repo-wide grep finds them only in enum definition/tests and one consumer match arm), so the broadcast-only delivery path this section specifies for them is dead — the exemptions file itself records QueueUpdated as 'Spec'd (message-bus QueueUpdated) but never emitted', a dead spec delivery link owned by this specification.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/messages.rs",
            "quote": "if effective_tier != MessageTier::Telemetry && !matches!(msg.to, Destination::Broadcast) {",
            "detail": "The REST send path's storage guard is tier- and destination-based only; there is no kind-based short-circuit for DataSeeded/QueueUpdated, so those kinds reach store() whenever their destination is not Broadcast. The internal emitter (lib.rs emit_scoped_event) mirrors this with the same destination-only guard."
          },
          {
            "file": "scripts/dead-message-kind-exemptions.txt",
            "quote": "# Spec'd (platform-model hot-file warnings; message-bus QueueUpdated)\n# but never emitted — pre-existing gaps, tracked here so the check is\n# green for NEW kinds.",
            "detail": "The broadcast-only kind QueueUpdated (and likewise DataSeeded, which has no emitter and no exemption entry) is never constructed in production code — the spec's broadcast-only kinds never flow through the in-memory broadcast channel because nothing emits them."
          },
          {
            "file": "crates/gyre-common/src/message.rs",
            "quote": "| MessageKind::GateResult => MessageTier::Directed,",
            "detail": "Tier classification matches the spec including the GateResult Directed exception; BudgetWarning/BudgetExhausted/AgentError sit in the Event arm and the telemetry kinds in the Telemetry arm, per the 'Changes from prior draft' list."
          },
          {
            "file": "crates/gyre-server/src/api/messages.rs",
            "quote": "\"Directed tier requires Agent destination, not Workspace\".to_string(),",
            "detail": "Tier+destination constraints are enforced with 400 in the send handler (and mirrored on the MCP path): Directed+Workspace, Directed+Broadcast, Telemetry+Agent, Telemetry+Broadcast are all rejected."
          },
          {
            "file": "crates/gyre-server/src/api/messages.rs",
            "quote": "if kind.server_only() && !matches!(from, MessageOrigin::Server) {",
            "detail": "The server_only() 403 check is enforced on the send path, covering the GateResult forge-authority exception and all built-in Event kinds, with Custom exempt as specced."
          }
        ]
      },
      {
        "id": "9118056e2689a152fa86ef18",
        "decision": "satisfied",
        "requirement_quote": "Each `MessageKind` has a defined payload schema. The server validates payloads on receipt — invalid payloads are rejected with 400.",
        "detail": "Fully implemented (task-200). MessageKind::validate_payload in gyre-common transcribes the spec table field-by-field including wire types (required-ness, string/u64/u32/f64/array-of-string checks, the AgentCompleted decisions array, null-treated-as-absent), and both the REST send handler and the MCP message.send handler call it before anything is signed or persisted, rejecting invalid payloads with 400. Schema spot-checks match the table: TaskAssignment requires task_id; StatusUpdate requires status and summary; PushAccepted requires repo_id/branch/agent_id with commit_count u64; GateResult requires mr_id/gate_name/attempt; AgentCompleted requires agent_id/task_id; ReconciliationCompleted requires workspace_id/persona_id.",
        "evidence": [
          {
            "file": "crates/gyre-common/src/message.rs",
            "quote": "pub fn validate_payload(&self, payload: Option<&Value>) -> Result<(), String> {",
            "detail": "The per-kind payload validator exists on MessageKind, backed by payload_schema() which transcribes the spec's schema table including required flags and wire types."
          },
          {
            "file": "crates/gyre-common/src/message.rs",
            "quote": "Some(&(const { [f(\"task_id\", Str, true), f(\"spec_ref\", Str, false)] }))",
            "detail": "Schema arms match the spec table row-for-row (TaskAssignment: task_id required, spec_ref optional), with the same field-by-field structure for every other specced kind."
          },
          {
            "file": "crates/gyre-server/src/api/messages.rs",
            "quote": "if let Err(reason) = kind.validate_payload(req.payload.as_ref()) {\n        return Err(ApiError::BadRequest(reason));\n    }",
            "detail": "The REST send handler validates the payload on receipt and rejects with 400 before signing or persisting; the MCP gyre_message_send handler applies the same validation."
          }
        ]
      },
      {
        "id": "f083786172e47a44a97b453f",
        "decision": "satisfied",
        "requirement_quote": "The server signs Directed and Event tier messages using its existing Ed25519 OIDC signing key (`AgentSigningKey`).",
        "detail": "Implemented as specced. A single shared implementation (crates/gyre-server/src/signing.rs, used by both the REST and MCP handlers, with a byte-identical internal copy sign_bus_message used by the server emitters) builds the deterministic sign input exactly as specified: id \\0 from_type \\0 from_id \\0 workspace_id \\0 to_type \\0 to_id \\0 kind \\0 sha256(payload_json) \\0 created_at, with absent fields encoded as the empty string and the canonical forms from the spec tables (server/\"\"/agent-uuid/user-uuid; agent/workspace/broadcast; workspace UUID or \"\" for Broadcast). The payload is SHA-256 hashed, the signature is Ed25519 via the server's existing AgentSigningKey (ring Ed25519KeyPair), and key_id is the JWKS kid. Directed and Event tiers are signed and Telemetry is left unsigned on all three send paths (REST, MCP, internal emitter). Verification is assigned by the spec to 'any party' via GET /.well-known/jwks.json: that endpoint serves the public key and every signed message carries its key_id, so external verification is possible; no server-side verify path exists, but none is required by this section (the key-rotation guarantee is separately counted under Edge Cases).",
        "evidence": [
          {
            "file": "crates/gyre-server/src/signing.rs",
            "quote": "let sign_input = format!(\n        \"{}\\0{}\\0{}\\0{}\\0{}\\0{}\\0{}\\0{}\\0{}\",",
            "detail": "The sign input is built by the exact fixed-order null-separated concatenation the spec defines, over id, from_type, from_id, workspace_id, to_type, to_id, kind, payload hash, created_at."
          },
          {
            "file": "crates/gyre-server/src/signing.rs",
            "quote": "gyre_common::message::MessageOrigin::Server => (\"server\", \"\".to_string()),",
            "detail": "Canonical forms match the spec tables: Server origin maps to from_type 'server' with empty from_id, Agent/User map to their UUIDs; destinations map to agent/workspace/broadcast with broadcast using the empty to_id, and absent workspace_id uses the empty string."
          },
          {
            "file": "crates/gyre-server/src/signing.rs",
            "quote": "let sig_bytes = state.agent_signing_key.sign_bytes(sign_input.as_bytes());",
            "detail": "Signing uses the server's existing Ed25519 OIDC signing key (AgentSigningKey) as specced, and the returned key_id is that key's JWKS kid."
          },
          {
            "file": "crates/gyre-server/src/api/messages.rs",
            "quote": "// Sign Directed and Event tier.\n    if effective_tier != MessageTier::Telemetry {",
            "detail": "The send path signs Directed and Event tier messages and leaves Telemetry unsigned (signature/key_id None), matching the tier table."
          },
          {
            "file": "crates/gyre-server/src/oidc.rs",
            "quote": "pub async fn jwks(State(state): State<Arc<AppState>>) -> impl IntoResponse {",
            "detail": "GET /.well-known/jwks.json serves the signing key's public JWK Set, enabling the spec's 'any party can verify' flow using the key_id carried on each signed message."
          }
        ]
      },
      {
        "id": "5a64e9d9ebac833f401124c2",
        "decision": "context",
        "requirement_quote": "Delivery",
        "detail": "Heading with no body text of its own; its requirements are carried entirely by the children WebSocket (57b85658), REST fallback (ab07ab53), and Delivery Guarantees (ce88ec9), each audited separately. Recorded here so the ID is accounted for exactly once.",
        "evidence": []
      },
      {
        "id": "57b856585e62b254ad3ab3d4",
        "decision": "gap",
        "requirement_quote": "If more than 1000 exist, the server sends the newest 1000 and includes a `{\"type\": \"ReplayCatchUp\", \"truncated\": true}` message — the client can use `GET /api/v1/workspaces/:id/messages` with cursor pagination for the full history.",
        "detail": "The subscription machinery is implemented: the Subscribe WsMessage variant (scopes, last_seen Option<u64> epoch millis, session_id Option<String>), the catch-all #[serde(other)] Unknown variant, replay of persisted Event-tier messages through state.messages.list_by_workspace with a 1000-message cap and a ReplayCatchUp{truncated:true} signal, telemetry never replayed (never persisted), Directed excluded from replay (list_by_workspace filters to_type != 'agent'), connection identity established at auth time so Destination::Agent(id) is delivered only to the connection whose verified agent id matches (shared-token/Admin connections are excluded), Workspace delivery gated on subscription, and Broadcast delivered to all. Two specific deviations from the specced replay behavior remain: (1) truncation window — the handler fetches the newest replay_limit+1 messages per workspace, reverses to oldest-first, and sends the first 1000 it encounters, i.e. the OLDEST 1000 of the newest 1001, dropping the newest message; the spec requires 'the server sends the newest 1000'. (2) Boundary comparison — the spec (and the protocol.rs doc comment 'created_at > last_seen') requires strictly greater, but both the mem and SQLite list_by_workspace implementations filter created_at >= since, so a message created exactly at the client's last_seen timestamp is re-delivered on every reconnect. The Subscribe-membership authorization gap is counted under Telemetry Store (2b439cf6) where the spec states it.",
        "evidence": [
          {
            "file": "crates/gyre-common/src/protocol.rs",
            "quote": "Subscribe {\n        scopes: Vec<SubscribeScope>,\n        last_seen: Option<u64>,\n        #[serde(default)]\n        session_id: Option<String>,\n    },",
            "detail": "The Subscribe variant exists with the specced shape: workspace scopes, optional last_seen (epoch millis), optional session_id for presence tracking."
          },
          {
            "file": "crates/gyre-server/src/ws.rs",
            "quote": "// Replay Event-tier messages since last_seen, oldest-first.\n                                    let mut replayed = 0usize;\n                                    let replay_limit = 1000usize;",
            "detail": "Replay on subscribe is implemented with a 1000-message cap, queries state.messages (the MessageRepository), and emits ReplayCatchUp{truncated:true} when the cap is hit."
          },
          {
            "file": "crates/gyre-server/src/ws.rs",
            "quote": "// list_by_workspace returns newest-first; reverse to oldest-first.\n                                            messages.reverse();\n                                            for m in messages {\n                                                if replayed >= replay_limit {\n                                                    truncated = true;\n                                                    break;\n                                                }",
            "detail": "The handler fetches the newest replay_limit+1 per workspace and then sends the first 1000 in oldest-first order — the oldest 1000 of the newest 1001 — whereas the spec requires sending the newest 1000; the newest message in the window is the one dropped."
          },
          {
            "file": "crates/gyre-adapters/src/sqlite/message.rs",
            "quote": "query = query.filter(messages::created_at.ge(since_ts as i64));",
            "detail": "The SQLite (and identically mem) list_by_workspace since-filter is inclusive (>=), so a message created exactly at the client's last_seen is replayed on reconnect; the spec requires created_at > last_seen (strictly greater, no re-delivery)."
          },
          {
            "file": "crates/gyre-server/src/ws.rs",
            "quote": "caller.user_id.is_none()\n                                    && !caller\n                                        .roles\n                                        .contains(&gyre_domain::UserRole::Admin)\n                                    && caller.agent_id == aid.as_str()",
            "detail": "Destination filtering is implemented per spec: agent-targeted messages reach only the connection whose verified agent identity matches (shared-token/Admin connections are excluded and must use REST poll), workspace delivery is subscription-gated, broadcast reaches all."
          }
        ]
      },
      {
        "id": "ab07ab533ae35135879f48bf",
        "decision": "satisfied",
        "requirement_quote": "Returns Directed-tier messages after the composite cursor `(after_ts, after_id)`, ordered oldest first. The agent stores the `created_at` and `id` of the last message as its cursor. First poll uses `after_ts=0&after_id=`. Messages persist — polling is non-destructive.",
        "detail": "The poll endpoint is implemented with the specced semantics: GET /api/v1/agents/:id/messages is registered, defaults to after_ts=0/limit=100, delegates ?acknowledged=false to list_unacked for crash recovery, and polls non-destructively through state.messages.list_after with the composite (created_at, id) cursor ordered oldest first. The ack endpoint (PUT .../messages/:message_id/ack) is idempotent and returns 200. Own-inbox enforcement: a non-admin caller whose agent_id differs from the path id gets 403 on both poll and ack. Two honest caveats: (a) an Admin-role caller may read and ack any agent's inbox — an operational extension beyond the spec's 'agent must be the authenticated caller', though the agent-to-agent isolation the rule protects is enforced; (b) in production the repository behind these endpoints is the in-memory MemMessageRepository (lib.rs:1697), not the SQLite/PG adapters — the endpoint semantics are correct, but the 'messages persist' clause across restarts is not met in production (counted under Core Principle 587ed8cf and Delivery Guarantees ce88ec9).",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/mod.rs",
            "quote": ".route(\"/api/v1/agents/:id/messages\", get(messages::poll_messages))",
            "detail": "The poll route is registered at the specced path, with the ack route at /api/v1/agents/:id/messages/:message_id/ack."
          },
          {
            "file": "crates/gyre-server/src/api/messages.rs",
            "quote": "if !is_admin && auth.agent_id != agent_id {\n        return Err(ApiError::Forbidden(\n            \"agents can only read their own inbox\".to_string(),",
            "detail": "Own-inbox enforcement: only the authenticated agent (or an Admin-role caller, a noted extension) may poll the inbox, satisfying the spec's agent-caller requirement for the agent-to-agent case."
          },
          {
            "file": "crates/gyre-server/src/api/messages.rs",
            "quote": "let after_ts = params.after_ts.unwrap_or(0);\n        let after_id = params.after_id.as_deref().map(Id::new);\n        state\n            .messages\n            .list_after(&aid, after_ts, after_id.as_ref(), limit)",
            "detail": "Cursor polling delegates to the port's list_after with defaults after_ts=0 and limit 100 (capped 1000); polling is a read, never a drain."
          },
          {
            "file": "crates/gyre-server/src/mem.rs",
            "quote": ".filter(|m| match after_id {\n                Some(aid) => {\n                    m.created_at > after_ts\n                        || (m.created_at == after_ts && m.id.as_str() > aid.as_str())\n                }\n                None => m.created_at > after_ts,\n            })",
            "detail": "The composite-cursor semantics (strict > on (created_at, id), oldest first) are implemented — note this evidence is from the mem adapter because that is what production actually polls; the SQLite/PG adapters implement the identical predicate but are unwired (see Core Principle)."
          },
          {
            "file": "crates/gyre-server/src/api/messages.rs",
            "quote": "// Idempotent ack.\n    state\n        .messages\n        .acknowledge(&mid, &aid)\n        .await\n        .map_err(ApiError::Internal)?;\n\n    Ok(StatusCode::OK)",
            "detail": "The ack endpoint acknowledges through the repository and unconditionally returns 200; the repository's acknowledge is a no-op for already-acked messages, making repeated acks idempotent as specced."
          }
        ]
      },
      {
        "id": "ce88ec9cd0bc4a86136ae33b",
        "decision": "gap",
        "requirement_quote": "**Directed** | At-least-once (persisted until ack, redelivered on poll) | DB — no TTL, expires only on ack or agent completion",
        "detail": "Partially implemented. Implemented: the Event TTL expiry job (GYRE_EVENT_TTL_SECS default 7 days, hourly background job calling expire_events, plus the dead-inbox retention GYRE_DEAD_INBOX_TTL_SECS), the telemetry ring buffer (in-memory, per-workspace 10,000 default via GYRE_TELEMETRY_BUFFER_SIZE, largest-first workspace eviction at 100 workspaces via GYRE_TELEMETRY_MAX_WORKSPACES), the Directed queue depth limit (GYRE_AGENT_INBOX_MAX default 1000, sends rejected with 429 before persisting, no silent drops), and poll-based redelivery. NOT implemented: (1) the Directed persistence medium — 'DB — no TTL, expires only on ack' is not on the wired path; production state.messages is the in-memory MemMessageRepository in every deployment, so restart orphans every unacked Directed message and the at-least-once guarantee fails exactly when it matters (after a crash); (2) agent completion cleanup — 'When an agent completes (POST /api/v1/agents/:id/complete), all unacked Directed messages in its inbox are marked acknowledged with ack_reason agent_completed' is never performed: acknowledge_all has no production caller (the only call site in the server crate is a test fixture; neither the REST complete handler in api/spawn.rs nor the MCP agent-complete path calls it); (3) consequently the orphaned-agent path ('the stale agent detector marks them Dead and calls acknowledge_all with ack_reason agent_orphaned') is also unimplemented — stale_agents.rs transitions agents to Dead but never bulk-acks, so expire_acked_inboxes can never match a dead inbox in production and unacked Directed messages to dead agents accumulate indefinitely.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/lib.rs",
            "quote": "messages: Arc::new(mem::MemMessageRepository::default()),",
            "detail": "The Directed/Event persistence medium in production is the in-memory adapter (unconditionally wired), so 'DB — no TTL, expires only on ack' does not describe the deployed system: a restart drops every unacked Directed message."
          },
          {
            "file": "crates/gyre-server/src/message_dispatcher.rs",
            "quote": "let events_deleted = state\n        .messages\n        .expire_events(now.saturating_sub(event_ttl_ms))\n        .await?;",
            "detail": "The Event-tier TTL expiry job IS implemented (GYRE_EVENT_TTL_SECS default 604_800s, run_message_expiry spawned hourly) — though it too runs against the in-memory store in production."
          },
          {
            "file": "crates/gyre-server/src/lib.rs",
            "quote": "agent_inbox_max: std::env::var(\"GYRE_AGENT_INBOX_MAX\")\n            .ok()\n            .and_then(|v| v.parse().ok())\n            .unwrap_or(1000),",
            "detail": "The Directed queue depth limit is configurable with the specced default of 1000; the send path rejects with 429 (TooManyRequests) before persisting, preserving the no-silent-drop requirement."
          },
          {
            "file": "crates/gyre-server/src/message_dispatcher.rs",
            "quote": ".acknowledge_all(&Id::new(\"agent-2\".to_string()), \"agent_completed\")",
            "detail": "The ONLY acknowledge_all call site in the server crate is this test fixture in message_dispatcher.rs's tests; the production completion paths (POST /api/v1/agents/:id/complete in api/spawn.rs, MCP agent-complete in mcp.rs) and stale_agents.rs were inspected and never call acknowledge_all, so neither 'agent_completed' nor 'agent_orphaned' is ever recorded in production and the dead-inbox cleanup never fires."
          },
          {
            "file": "crates/gyre-server/src/stale_agents.rs",
            "quote": "let _ = agent.transition_status(AgentStatus::Dead);",
            "detail": "The stale-agent detector marks orphaned agents Dead but performs no acknowledge_all with ack_reason 'agent_orphaned' — the spec's orphan cleanup path is absent."
          }
        ]
      },
      {
        "id": "87668c235d98ebb2cdc31db7",
        "decision": "gap",
        "requirement_quote": "**All `Destination::Agent(id)` messages validate tenant isolation.** The target agent's `tenant_id` must match the message's `tenant_id`, regardless of origin (including `Server`). This prevents a server bug from leaking messages across tenants.",
        "detail": "Rules 2, 3, 5, 6, and 7 are implemented and enforced in the send handler with the specced status codes: agent-to-agent Directed requires the sender's workspace to match (403); user-to-agent requires WorkspaceMembershipRepository membership (403); Broadcast requires Server origin or Admin role (403, and the MCP path rejects Broadcast outright); workspace fan-out requires the destination to match the URL workspace (400) with agent same-workspace (403) and user membership (403) checks; server-originated events inherit the workspace of the entity they describe (emit_event resolves the workspace record and skips on unresolvable scope). Rule 1 is NOT implemented as specified: there is no tenant_id comparison anywhere in the send path. Isolation for User and Agent origins is enforced only transitively — the target agent must belong to the URL workspace, and those origins must themselves be members of that workspace, which implies same-tenant. But the spec requires the target agent's tenant_id to match the message's tenant_id 'regardless of origin (including Server)': a Server-origin send (global GYRE_AUTH_TOKEN maps to Server with the caller's tenant) targeting an agent in another tenant's workspace passes the handler with the message's tenant_id set to the caller's tenant and no target-tenant validation, exactly the server-bug leak class rule 1 exists to prevent. Additionally, rule 3's User-origin membership check is keyed on MessageOrigin::User and is therefore skipped for Admin-role API-key/OIDC callers, who receive Server origin under origin_from_auth's role-based mapping — a consequence of the origin-table deviation tracked under Message Envelope (d221d8125b8e34bf71e2fbbd, item 4), recorded here because it weakens rule 3's enforcement on the send path.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/messages.rs",
            "quote": "// Target must be in this workspace (enforces tenant isolation since workspace IDs are\n        // globally unique and each workspace belongs to exactly one tenant).\n        if target_agent.workspace_id != ws_id {",
            "detail": "The only target validation is a workspace match against the URL workspace; there is no comparison of the target agent's tenant_id with the message's tenant_id, so rule 1's 'regardless of origin (including Server)' tenant check does not exist as an explicit validation."
          },
          {
            "file": "crates/gyre-server/src/api/messages.rs",
            "quote": "return Err(ApiError::Forbidden(\n                    \"agents can only send Directed messages to agents in the same workspace\"\n                        .to_string(),",
            "detail": "Rule 2 (agent→agent same workspace, 403) is implemented."
          },
          {
            "file": "crates/gyre-server/src/api/messages.rs",
            "quote": "let membership = state\n                .workspace_memberships\n                .find_by_user_and_workspace(user_id, &ws_id)",
            "detail": "Rule 3 (user→agent workspace membership via WorkspaceMembershipRepository, 403 on absence) is implemented — but the check is keyed on MessageOrigin::User, so Admin-role API-key/OIDC callers mapped to Server origin by origin_from_auth skip it (origin-table deviation tracked under Message Envelope, d221d8125b8e34bf71e2fbbd, item 4)."
          },
          {
            "file": "crates/gyre-server/src/api/messages.rs",
            "quote": "\"Broadcast destination requires Server origin or Admin role\".to_string(),",
            "detail": "Rule 6 (Broadcast requires Server origin or Admin role, 403) is implemented; agent JWTs map to Agent origin without Admin role and are rejected."
          },
          {
            "file": "crates/gyre-server/src/lib.rs",
            "quote": "let Some(tenant_id) = self.message_tenant_id(&workspace_id).await else {",
            "detail": "Rule 5 is implemented on the internal emitter: emit_event resolves the tenant from the referenced entity's workspace record and skips emission when the scope cannot be resolved rather than fabricating one — but nothing downstream re-validates the target's tenant against it."
          }
        ]
      },
      {
        "id": "174d273714cdb8c3d2daa505",
        "decision": "context",
        "requirement_quote": "Storage",
        "detail": "Heading with no body text of its own; its requirements are carried entirely by the children Port Trait (e5defe21), Telemetry Store (2b439cf6), Workspace Fan-Out Persistence (871518a7), and DB Schema (8ac100e8), each audited separately. Recorded here so the ID is accounted for exactly once.",
        "evidence": []
      },
      {
        "id": "e5defe214a094c014affd05e",
        "decision": "satisfied",
        "requirement_quote": "async fn store(&self, message: &Message) -> Result<()>;",
        "detail": "The port trait is implemented verbatim in gyre-ports/src/message.rs: all eleven specced methods (store, find_by_id, list_after, list_unacked, count_unacked, acknowledge, acknowledge_all, list_by_workspace, expire_events, expire_acked_inboxes, expire_for_agents) with the specced signatures, including the documented cursor semantics (strict > composite cursor for list_after; windowed since/before query newest-first for list_by_workspace). All three adapters implement the trait: SQLite and PostgreSQL with identical Diesel predicates (composite cursor expressed as created_at > ts OR (created_at = ts AND id > id); expire_events filtered to to_type != 'agent'; expire_acked_inboxes filtered to ack_reason IN agent_completed/agent_orphaned), and the mem stand-in with equivalent in-code filters. Honest caveats recorded elsewhere and not double-counted here: production wiring uses only the mem adapter (Core Principle, 587ed8cf), and the mem adapter diverges from the store()-Err-on-Broadcast contract that the DB adapters enforce (Message Envelope, d221d812).",
        "evidence": [
          {
            "file": "crates/gyre-ports/src/message.rs",
            "quote": "pub trait MessageRepository: Send + Sync {",
            "detail": "The port trait lives in gyre-ports exactly as placed by the spec, depending only on gyre-common types."
          },
          {
            "file": "crates/gyre-ports/src/message.rs",
            "quote": "/// When `after_id` is absent: `WHERE created_at > after_ts ORDER BY created_at, id LIMIT limit`.\n    /// When `after_id` is present: `WHERE (created_at, id) > (after_ts, after_id)`.",
            "detail": "The trait documents and its adapters implement the specced composite-cursor semantics: strict greater-than on both paths, first poll after_ts=0/after_id=None."
          },
          {
            "file": "crates/gyre-adapters/src/sqlite/message.rs",
            "quote": "                // Composite cursor: (created_at, id) > (after_ts, after_id)\n                // Expressed as: created_at > after_ts OR (created_at = after_ts AND id > after_id)",
            "detail": "The SQLite adapter implements the composite cursor exactly as the port doc specifies; the PostgreSQL adapter (impl MessageRepository for PgStorage) is a structural twin with no unimplemented methods."
          },
          {
            "file": "crates/gyre-adapters/src/postgres/message.rs",
            "quote": "impl MessageRepository for PgStorage {",
            "detail": "The PostgreSQL adapter implements the full trait; inspected for stubs — no unimplemented!/todo!() exists anywhere in the file."
          },
          {
            "file": "crates/gyre-server/src/mem.rs",
            "quote": "async fn acknowledge_all(&self, agent_id: &Id, reason: &str) -> Result<u64> {",
            "detail": "The mem adapter also implements every trait method including the reason-recording bulk ack; it is the only adapter wired into production (see Core Principle for that wiring gap)."
          }
        ]
      },
      {
        "id": "2b439cf656cf14c344afdb07",
        "decision": "gap",
        "requirement_quote": "**Subscription authorization:** The server verifies workspace membership when processing `Subscribe` messages — a client cannot subscribe to a workspace it doesn't belong to. This prevents telemetry leakage across workspaces even though the buffer itself has no tenant isolation.",
        "detail": "The buffer itself is implemented exactly as specced: TelemetryBuffer in gyre-common with per-workspace DashMap<Id, VecDeque<Message>> ring buffers, FIFO oldest-entry eviction at max_per_workspace (default 10,000, GYRE_TELEMETRY_BUFFER_SIZE), and largest-first workspace eviction when distinct workspaces exceed max_workspaces (default 100, GYRE_TELEMETRY_MAX_WORKSPACES) — wired in build_state with those env vars and defaults. Telemetry is pushed through the broadcast channel and stored only in the buffer, never in MessageRepository. The subscription-authorization requirement is NOT met: the ws.rs Subscribe handler validates only that each requested workspace belongs to the caller's TENANT (plus an Admin bypass) — it performs no workspace-membership verification (no WorkspaceMembershipRepository lookup, in contrast to the REST handlers which do check membership). A user authenticated in tenant T can subscribe to any workspace in T without being a member, and since workspace-scoped bus messages and telemetry are delivered to subscribed connections, same-tenant non-members receive workspace telemetry the spec says they must not.",
        "evidence": [
          {
            "file": "crates/gyre-common/src/message.rs",
            "quote": "pub struct TelemetryBuffer {\n    /// Per-workspace ring buffers. Key: workspace_id.\n    buffers: DashMap<Id, VecDeque<Message>>,",
            "detail": "The telemetry store is the specced in-memory per-workspace ring buffer, replacing ActivityStore; it is not part of MessageRepository."
          },
          {
            "file": "crates/gyre-common/src/message.rs",
            "quote": "// Enforce workspace count limit: evict the workspace with the most entries.\n        if self.buffers.len() > self.max_workspaces {",
            "detail": "Largest-first workspace eviction is implemented as specified (the anti-flushing mitigation), alongside FIFO eviction at max_per_workspace."
          },
          {
            "file": "crates/gyre-server/src/lib.rs",
            "quote": "let telemetry_buffer = Arc::new(TelemetryBuffer::new(\n        std::env::var(\"GYRE_TELEMETRY_BUFFER_SIZE\")",
            "detail": "The buffer is wired in build_state with GYRE_TELEMETRY_BUFFER_SIZE (default 10_000) and GYRE_TELEMETRY_MAX_WORKSPACES (default 100), matching the specced defaults."
          },
          {
            "file": "crates/gyre-server/src/ws.rs",
            "quote": "let tenant_match = caller.roles.contains(&gyre_domain::UserRole::Admin)\n                                                    || workspace.tenant_id.as_str() == caller.tenant_id;",
            "detail": "Subscribe processing authorizes on tenant match (plus Admin bypass) only — no workspace-membership verification is performed, so a same-tenant non-member can subscribe to a workspace it does not belong to, contradicting the section's explicit authorization requirement."
          }
        ]
      },
      {
        "id": "871518a78a55edde964dc11b",
        "decision": "satisfied",
        "requirement_quote": "When a message targets `Destination::Workspace(id)`, it is stored as **one row** with `to_type = 'workspace'` and `to_id = workspace_id`.",
        "detail": "Implemented as specified. A workspace-destined message is stored exactly once with to_type='workspace' and to_id=workspace_id (the intentional denormalization), never exploded into per-agent rows: the send handler performs a single store() call for the constructed message, and the adapters' dest_to_row maps Destination::Workspace to ('workspace', Some(workspace_id)). list_by_workspace finds these via the workspace index while list_unacked (to_type='agent') never returns them, matching the 'queryable history, not inbox items' design. The structural tenant-isolation rationale is honored operationally: check-tenant-filter.sh carries the specced skip-list entry for message.rs, and the expiry methods are cross-tenant housekeeping as documented. Caveat not double-counted here: in production the single row lands in the in-memory store rather than the DB (Core Principle, 587ed8cf).",
        "evidence": [
          {
            "file": "crates/gyre-adapters/src/sqlite/message.rs",
            "quote": "Destination::Workspace(id) => Ok((\"workspace\".to_string(), Some(id.as_str().to_string()))),",
            "detail": "Workspace destinations persist as one row with to_type='workspace' and to_id=workspace_id; there is no per-agent explosion anywhere in the send path or adapters."
          },
          {
            "file": "crates/gyre-server/src/api/messages.rs",
            "quote": "// Persist Directed and Event tier only (Telemetry is in-memory, Broadcast is not stored).\n    if effective_tier != MessageTier::Telemetry && !matches!(msg.to, Destination::Broadcast) {",
            "detail": "The send handler stores the constructed message exactly once — a workspace fan-out is one store() call plus one broadcast channel send."
          },
          {
            "file": "crates/gyre-adapters/src/sqlite/message.rs",
            "quote": "// list_by_workspace returns Event-tier workspace messages only;\n                // Directed (agent-inbox) messages are accessed via list_after/list_unacked.\n                .filter(messages::to_type.ne(\"agent\")",
            "detail": "The inbox and workspace-history query paths are kept orthogonal as specced: list_unacked/list_after filter to_type='agent', list_by_workspace excludes agent-targeted rows."
          },
          {
            "file": "scripts/check-tenant-filter.sh",
            "quote": "\"message.rs|messages are queried by globally-unique workspace_id (tenant-bound) or per-agent inbox key (to_id, tenant-bound agent); per-message expiry methods are intentionally cross-tenant housekeeping. Spec §3 names MessageRepository as the structural-isolation example.\"",
            "detail": "The spec's lint exemption is present in check-tenant-filter.sh's skip list with the structural-isolation rationale, covering both the SQLite and PostgreSQL message.rs adapters."
          }
        ]
      },
      {
        "id": "8ac100e8daf928b79767848b",
        "decision": "satisfied",
        "requirement_quote": "CREATE INDEX idx_messages_inbox ON messages (to_type, to_id, acknowledged)\n    WHERE to_type = 'agent' AND acknowledged = 0;",
        "detail": "The schema is implemented verbatim in migration 2026-03-24-000017_messages/up.sql: the messages table with all specced columns (id, tenant_id, from_type, from_id, workspace_id NOT NULL, to_type, to_id, kind, payload, created_at INTEGER epoch-milliseconds, signature, key_id, acknowledged INTEGER NOT NULL DEFAULT 0, ack_reason TEXT) and all five specced indexes including the two partial indexes (idx_messages_inbox WHERE to_type='agent' AND acknowledged=0; idx_messages_expiry WHERE to_type != 'agent'), idx_messages_workspace, idx_messages_kind, and idx_messages_tenant. Evidence-level caveat (not a schema defect, and the wiring gap is counted under Core Principle 587ed8cf): because AppState.messages is unconditionally the mem adapter, this table is never read or written in production — the DDL conforms but is dead until the adapters are wired.",
        "evidence": [
          {
            "file": "crates/gyre-adapters/migrations/2026-03-24-000017_messages/up.sql",
            "quote": "CREATE TABLE messages (\n    id            TEXT    NOT NULL PRIMARY KEY,",
            "detail": "The messages table exists with the specced column set, including workspace_id TEXT NOT NULL (Broadcast messages are never stored, per the migration's own header comment) and created_at INTEGER epoch milliseconds."
          },
          {
            "file": "crates/gyre-adapters/migrations/2026-03-24-000017_messages/up.sql",
            "quote": "CREATE INDEX idx_messages_inbox ON messages (to_type, to_id, acknowledged)\n    WHERE to_type = 'agent' AND acknowledged = 0;",
            "detail": "All five specced indexes are present, including this partial inbox index and the partial expiry index (CREATE INDEX idx_messages_expiry ON messages (created_at) WHERE to_type != 'agent')."
          },
          {
            "file": "crates/gyre-adapters/src/sqlite/message.rs",
            "quote": "// Always store as unacknowledged — ack state is set only via acknowledge() calls.\n        acknowledged: 0,",
            "detail": "The Diesel row model matches the DDL (workspace_id non-optional String, acknowledged defaulting 0, ack_reason separate from the bool), and both DB adapters round-trip every column."
          }
        ]
      },
      {
        "id": "8d8d65700137b99f6fd106d4",
        "decision": "satisfied",
        "requirement_quote": "The notification system implements `MessageConsumer`: when it receives a `GateFailure` message, it creates a notification for the relevant human users.",
        "detail": "Fully implemented (task-162). The MessageConsumer trait exists in gyre-ports with the specced on_message signature; the send hot path clones each stored message into a bounded tokio mpsc channel (capacity 256) via try_send and returns immediately, dropping with a warning log when full; a background dispatcher drains the channel and dispatches to registered consumers; the notification system's NotificationBridge implements MessageConsumer and creates user-facing notifications for GateFailure, AgentError, and ReconciliationCompleted messages (BudgetWarning deliberately excluded with a documented rationale). The dispatcher is spawned at startup and the same cloning is applied on the REST, MCP, and internal-emitter paths. Consumer drops do not affect the Directed at-least-once guarantee, which rests on persistence + ack (see Delivery Guarantees for that persistence gap).",
        "evidence": [
          {
            "file": "crates/gyre-ports/src/message.rs",
            "quote": "pub trait MessageConsumer: Send + Sync {\n    /// Called by the background dispatcher for each message stored to the bus.\n    /// May perform I/O (DB writes, etc.) — runs off the send hot path.\n    async fn on_message(&self, message: &Message);\n}",
            "detail": "The consumer trait is defined exactly as specced in gyre-ports."
          },
          {
            "file": "crates/gyre-server/src/lib.rs",
            "quote": "let (message_dispatch_tx, message_dispatch_rx) = tokio::sync::mpsc::channel(256);",
            "detail": "The bounded mpsc dispatch channel decouples the send hot path from consumer latency, as the spec's implementation mechanism requires."
          },
          {
            "file": "crates/gyre-server/src/message_dispatcher.rs",
            "quote": "impl MessageConsumer for NotificationBridge {\n    async fn on_message(&self, message: &Message) {\n        match &message.kind {\n            MessageKind::GateFailure => self.on_gate_failure(message).await,",
            "detail": "The notification system is a registered consumer: NotificationBridge derives Inbox notifications from GateFailure (and AgentError / ReconciliationCompleted) messages, spawned as the dispatcher's consumer list at startup."
          },
          {
            "file": "crates/gyre-server/src/lib.rs",
            "quote": "match self.message_dispatch_tx.try_send(msg) {",
            "detail": "The internal emitter (and equally the REST and MCP send paths) clones each stored message into the channel non-blockingly; the Full arm logs 'message dispatch channel full; consumer dropped a message (best-effort tier)' and never blocks delivery."
          }
        ]
      },
      {
        "id": "d4bab77878e4f45b31906f4b",
        "decision": "context",
        "requirement_quote": "API",
        "detail": "Heading with no body text of its own; its requirements are carried entirely by the children Sending (b6b3a140), Receiving (15598664), Acknowledging (25b00a55), Querying (6b2d92c7), and Telemetry (64d1be47), each audited separately. Recorded here so the ID is accounted for exactly once.",
        "evidence": []
      },
      {
        "id": "b6b3a14041df1958d9d53cfb",
        "decision": "satisfied",
        "requirement_quote": "The `from` field is derived server-side from the JWT. The sender does not and cannot specify it. The `workspace_id` is taken from the URL path parameter.",
        "detail": "Implemented. POST /api/v1/workspaces/:workspace_id/messages is registered; the handler derives `from` server-side from the authenticated caller (origin_from_auth — the request body carries no `from` field at all), takes workspace_id from the URL, rejects a body workspace destination that differs from the URL with 400, rejects a target agent outside the URL workspace with 400, supports the 'tier': 'directed' opt-in for Custom kinds (defaulting to Event), validates the payload schema with 400, signs Directed/Event tiers, enforces the queue-depth 429, and returns 201 with `acknowledged` stripped from the response. This section's own requirement — from derived server-side, never caller-supplied — is met. However, the derived VALUE deviates from the spec's normative origin/tenant resolution table (Message Envelope): origin_from_auth maps Admin-role API-key and Keycloak callers to Server origin where the table requires User(user_id). On this very send path that deviation both passes the server_only() 403 gate (an Admin API-key/OIDC user can forge built-in Event-tier kinds and GateResult — authority the spec reserves to the global GYRE_AUTH_TOKEN) and skips the User-origin workspace-membership check for Directed sends (scoping rule 3). This is not a benign footnote: the deviation is tracked as a gap under Message Envelope (d221d8125b8e34bf71e2fbbd, item 4) — the code comment's spoofable-agent_id rationale is a design justification, not a spec amendment, and repairing the table mapping loses no specced Admin capability (rule 6's Broadcast allowance is enforced by an independent role check in the Broadcast guard).",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/mod.rs",
            "quote": "\"/api/v1/workspaces/:workspace_id/messages\",\n            post(messages::send_message).get(messages::list_workspace_messages),",
            "detail": "The unified send endpoint is registered at the specced path, replacing the old POST /api/v1/agents/:id/messages sender."
          },
          {
            "file": "crates/gyre-server/src/api/messages.rs",
            "quote": "fn origin_from_auth(auth: &AuthenticatedAgent) -> MessageOrigin {\n    if auth.roles.contains(&gyre_domain::UserRole::Admin) {\n        // Admin callers (global token, Admin-role users) → Server origin.\n        MessageOrigin::Server",
            "detail": "`from` is derived exclusively server-side from the authenticated caller; the send request type has no `from` field, so the sender cannot specify it — this section's requirement holds. The derived value for Admin-role API-key/Keycloak callers (Server origin) deviates from the spec's normative origin table; that deviation and its send-path security consequences are tracked as a gap under Message Envelope (d221d8125b8e34bf71e2fbbd, item 4)."
          },
          {
            "file": "crates/gyre-server/src/api/messages.rs",
            "quote": "\"workspace destination must match URL workspace\".to_string(),",
            "detail": "The body workspace destination is validated against the URL workspace parameter and rejected with 400 on mismatch, as specced."
          },
          {
            "file": "crates/gyre-server/src/api/messages.rs",
            "quote": "let effective_tier = if let MessageKind::Custom(_) = &kind {\n        if req.tier.as_deref() == Some(\"directed\") {\n            MessageTier::Directed",
            "detail": "Custom kinds default to Event tier with the specced 'tier': 'directed' request opt-in for ack-based delivery."
          },
          {
            "file": "crates/gyre-server/src/api/messages.rs",
            "quote": "fn send_response(msg: Message) -> Value {\n    let mut v = serde_json::to_value(&msg).unwrap_or_default();\n    if let Some(obj) = v.as_object_mut() {\n        obj.remove(\"acknowledged\");",
            "detail": "The 201 response carries the specced envelope fields (id, from, to, workspace_id, kind, payload, created_at, signature, key_id) with `acknowledged` excluded."
          }
        ]
      },
      {
        "id": "15598664d2d0def5c045774d",
        "decision": "satisfied",
        "requirement_quote": "When no query params are provided, defaults to `?after_ts=0&after_id=&limit=100`.",
        "detail": "Implemented: the poll handler defaults after_ts to 0, after_id to None, and limit to 100 (capped at 1000); the ?acknowledged=false alternative delegates to list_unacked on the port trait rather than list_after, exactly as the spec's 'crash recovery' alternative requires; and the agent-caller requirement is enforced (non-admin callers may only poll their own inbox, 403 otherwise — with the noted Admin-role operational extension). The non-destructive cursor semantics are implemented in the repository layer (evidence under REST fallback, ab07ab53, where the delivery behavior is counted).",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/messages.rs",
            "quote": "let limit = params.limit.unwrap_or(100).min(1000);",
            "detail": "Default limit 100 with a 1000 cap, matching the specced default poll shape."
          },
          {
            "file": "crates/gyre-server/src/api/messages.rs",
            "quote": "let messages = if params.acknowledged.as_deref() == Some(\"false\") {\n        // Crash recovery: return all unacked messages.\n        state\n            .messages\n            .list_unacked(&aid, limit)",
            "detail": "The ?acknowledged=false crash-recovery alternative delegates to list_unacked on the port trait, not list_after, as specified."
          },
          {
            "file": "crates/gyre-server/src/api/messages.rs",
            "quote": "return Err(ApiError::Forbidden(\n            \"agents can only read their own inbox\".to_string(),",
            "detail": "The authenticated-caller requirement is enforced for poll (and ack): an agent cannot read another agent's inbox; Admin-role callers are an operational extension noted in the REST fallback section."
          }
        ]
      },
      {
        "id": "25b00a5594060091683d9f31",
        "decision": "satisfied",
        "requirement_quote": "Idempotent. Returns 200 whether the message was already acked or not.",
        "detail": "Implemented: PUT /api/v1/agents/:id/messages/:message_id/ack is registered; the handler enforces the agent-caller requirement (403 for a non-admin caller acking another agent's inbox) and delegates to MessageRepository::acknowledge, which is a no-op for already-acknowledged messages (mem guards on !m.acknowledged; the DB adapters' UPDATE matches only unacked rows), so a repeated ack returns 200 identically. The queue-depth and inbox-cleanup consequences of acks are counted under Delivery Guarantees.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/mod.rs",
            "quote": "\"/api/v1/agents/:id/messages/:message_id/ack\",\n            put(messages::ack_message),",
            "detail": "The explicit ack endpoint is registered at the specced path."
          },
          {
            "file": "crates/gyre-server/src/api/messages.rs",
            "quote": "// Idempotent ack.\n    state\n        .messages\n        .acknowledge(&mid, &aid)\n        .await\n        .map_err(ApiError::Internal)?;\n\n    Ok(StatusCode::OK)",
            "detail": "The handler returns 200 unconditionally after delegating to the idempotent repository ack."
          },
          {
            "file": "crates/gyre-server/src/mem.rs",
            "quote": "if matches!(&m.to, Destination::Agent(id) if id == agent_id) && !m.acknowledged {\n                m.acknowledged = true;",
            "detail": "Repository-level idempotency: acking an already-acked message is a no-op that still yields Ok(()), so the second ack returns HTTP 200, not an error."
          }
        ]
      },
      {
        "id": "6b2d92c7248542866b37b87a",
        "decision": "satisfied",
        "requirement_quote": "Returns Event-tier messages for a workspace as a bare JSON array (per `api-conventions.md` §3.1). Requires workspace membership (verified via `WorkspaceMembershipRepository`).",
        "detail": "Implemented: GET /api/v1/workspaces/:id/messages (registered as the .get twin of the send route) accepts exactly the specced query parameters (kind, since, before_ts, before_id, limit), verifies workspace membership for non-Admin callers (users via WorkspaceMembershipRepository.find_by_user_and_workspace, agent tokens via the agent's workspace), returns 403 'not a member of this workspace' on failure, and returns the messages as a bare JSON array (Json(Vec<Message>)) with newest-first composite-cursor pagination and limit default 50 capped at 500. Temporal parameters are epoch milliseconds via the port's u64 since/before_ts, matching the documented per-endpoint exception.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/messages.rs",
            "quote": "pub struct WorkspaceQueryParams {\n    pub kind: Option<String>,\n    pub since: Option<u64>,\n    pub before_ts: Option<u64>,\n    pub before_id: Option<String>,\n    pub limit: Option<usize>,\n}",
            "detail": "The query surface matches the spec exactly: kind filter, since lower bound, before_ts/before_id composite cursor, limit."
          },
          {
            "file": "crates/gyre-server/src/api/messages.rs",
            "quote": "if !is_member {\n            return Err(ApiError::Forbidden(\n                \"not a member of this workspace\".to_string(),",
            "detail": "Workspace membership is verified before returning results (users via WorkspaceMembershipRepository, agents via their workspace assignment), with 403 on failure."
          },
          {
            "file": "crates/gyre-server/src/api/messages.rs",
            "quote": "let messages = state\n        .messages\n        .list_by_workspace(",
            "detail": "The handler delegates to the port's windowed list_by_workspace (newest-first, before-cursor pagination) and returns the result as a bare JSON array."
          }
        ]
      },
      {
        "id": "64d1be477f27ebf27ec73076",
        "decision": "gap",
        "requirement_quote": "Queries the in-memory `TelemetryBuffer`. The `since` parameter on this endpoint uses **epoch milliseconds** (matching the message bus, not the legacy seconds convention). The handler verifies workspace membership before returning results — an unauthenticated or unauthorized caller receives 403.",
        "detail": "The specced endpoint does not exist. There is no GET /api/v1/workspaces/:workspace_id/activity route anywhere in the router; the only activity routes are /api/v1/activity (which synthesizes events from notifications, is not workspace-scoped, and takes an unused _auth parameter with no workspace-membership check) and /api/v1/users/:username/activity (an unrelated profile feed). The TelemetryBuffer itself is only queryable through the WebSocket ActivityQuery path (scoped to subscribed workspaces) and the admin-only /api/v1/admin/audit endpoint (cross-workspace) — neither is the REST endpoint with per-workspace membership enforcement and the ActivityEventData field mapping (event_id/agent_id/event_type/description/timestamp) that this section specifies. The mapping logic exists in the WS path but the specced endpoint, its 403 membership enforcement, and its response contract are missing.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/mod.rs",
            "quote": ".route(\"/api/v1/activity\", get(activity::activity_handler))",
            "detail": "The only general activity route is the unscoped /api/v1/activity; no /api/v1/workspaces/:workspace_id/activity route exists anywhere in the router (inspected the full route table in api/mod.rs)."
          },
          {
            "file": "crates/gyre-server/src/api/activity.rs",
            "quote": "/// GET /api/v1/activity — unified activity feed.\n///\n/// Synthesizes activity events from notifications (which are populated for all",
            "detail": "The existing activity handler is notification-synthesized, not TelemetryBuffer-backed: it never reads state.telemetry_buffer, is not workspace-scoped, and its _auth parameter is unused (no membership check), so neither the backing store nor the 403 authorization requirement of this section is met on any REST path."
          },
          {
            "file": "crates/gyre-common/src/message.rs",
            "quote": "pub fn list_since(&self, workspace_id: &Id, since_ms: u64, limit: usize) -> Vec<Message> {",
            "detail": "TelemetryBuffer does provide the per-workspace, epoch-millisecond since query the endpoint would need (used by the WebSocket ActivityQuery path), so the gap is the missing REST endpoint and its membership enforcement, not the store."
          }
        ]
      },
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
      },
      {
        "id": "6c71d73718036d5707e64761",
        "decision": "satisfied",
        "requirement_quote": "Since this is a greenfield system with no production deployment, the unified message bus replaces the existing channels directly — no phased migration or feature flags are needed.",
        "detail": "The replacement is carried out directly, without feature flags. event_tx and broadcast_tx are gone from AppState (replaced by the single message_broadcast_tx: broadcast::Sender<Message>); the agent_messages KvJsonStore drain-on-read inbox is deleted (api/agent_messages.rs no longer exists; GET /api/v1/agents/:id/messages is the non-destructive cursor poll); all five specced WsMessage additions exist (Subscribe, ReplayCatchUp, UserPresence with server-derived user_id, PresenceEvicted, catch-all Unknown with #[serde(other)]); the endpoint changes are in place (new unified send route, semantics-changed poll route, new ack route) and FreeText is absent from MessageKind. Workspace resolution for server events resolves the entity's workspace with skip-and-log on unresolvable scope. Two inert remnants, noted for completeness rather than as missing behavior: the legacy DomainEvent enum file (domain_events.rs) and ActivityStore (activity.rs) remain on disk as unreferenced dead code, and WsMessage::ActivityEvent is retained as a client→server ingestion shim that bridges legacy WS telemetry onto the bus via emit_telemetry (the server never emits ActivityEvent), a mild tension with 'Supersedes the WsMessage::ActivityEvent protocol' that preserves rather than violates bus behavior.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/lib.rs",
            "quote": "/// Unified broadcast channel for WebSocket message delivery (replaces broadcast_tx + event_tx).\n    pub message_broadcast_tx: broadcast::Sender<Message>,",
            "detail": "The DomainEvent event_tx and ActivityStore broadcast_tx are replaced by the single unified Message broadcast channel in AppState."
          },
          {
            "file": "crates/gyre-common/src/protocol.rs",
            "quote": "/// Catch-all for unrecognized message types — silently ignored for forward compatibility.\n    #[serde(other)]\n    Unknown,",
            "detail": "The specced WsMessage additions are all present: Subscribe, ReplayCatchUp, UserPresence (server derives user_id from the verified connection), PresenceEvicted, and this catch-all Unknown variant."
          },
          {
            "file": "crates/gyre-server/src/api/mod.rs",
            "quote": ".route(\"/api/v1/agents/:id/messages\", get(messages::poll_messages))",
            "detail": "The endpoint changes are in place: the old drain-on-read inbox handler (api/agent_messages.rs) is deleted and this path is now the non-destructive cursor poll, alongside the new unified send and ack routes."
          },
          {
            "file": "crates/gyre-server/src/ws.rs",
            "quote": "WsMessage::ActivityEvent {\n                                    event_id,\n                                    agent_id,\n                                    event_type,\n                                    description,\n                                    timestamp,\n                                } => {",
            "detail": "WsMessage::ActivityEvent survives only as a legacy client→server ingestion shim that converts to a Telemetry-tier bus message (emit_telemetry); the server never sends ActivityEvent, so unified Message delivery has replaced it on the output path as specced."
          }
        ]
      },
      {
        "id": "e5d4cc99cba7ab16b2b40bb5",
        "decision": "gap",
        "requirement_quote": "| Server restart | Persisted messages survive. Telemetry buffer is lost (acceptable — it's ephemeral). WebSocket connections drop; clients reconnect and re-subscribe with `last_seen`. |",
        "detail": "Implemented rows: send-to-self is allowed (no from==to check exists); workspace fan-out to 100 agents is one store plus one broadcast send; queue depth rejects with 429 at GYRE_AGENT_INBOX_MAX (default 1000) with no silent drops; concurrent sends are atomic (single INSERT in the DB adapters, mutex-guarded insert in mem); concurrent ack is idempotent (200). NOT implemented rows: (1) Server restart — persisted messages do NOT survive in any deployment because AppState.messages is unconditionally the in-memory MemMessageRepository (the SQLite/PG adapters and messages table are unwired); only the telemetry-buffer-lost and WS-reconnect halves of the row hold. (2) Message too large — there is no 64KB GYRE_MAX_MESSAGE_SIZE limit and no message-specific 413; only the global GYRE_MAX_BODY_SIZE (default 10MB) body limit applies, so a message payload can be ~10MB, far beyond the specced 64KB envelope cap. (3) Send to dead/completed agent — the message is stored (no status guard), but the row's cleanup lifecycle is broken: neither 'acknowledge_all was called at completion time' (no production caller on POST /api/v1/agents/:id/complete) nor the orphaned-agent 'agent_orphaned' bulk-ack (absent from stale_agents.rs) ever happens, so expire_dead_inboxes/expire_acked_inboxes can never reclaim those inboxes (counted under Delivery Guarantees ce88ec9). (4) Signature key rotation — the JWKS endpoint serves only the single current key (jwks_json is built from one key pair); there is no key-history storage, so 'messages signed with old key remain verifiable — JWKS endpoint serves historical keys' is unimplemented.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/lib.rs",
            "quote": "messages: Arc::new(mem::MemMessageRepository::default()),",
            "detail": "The 'Server restart | Persisted messages survive' row is not met: the production message store is a process-local HashMap in every deployment (DB adapters unwired), so a restart loses all Directed and Event messages."
          },
          {
            "file": "crates/gyre-server/src/lib.rs",
            "quote": "// Body size limit: configurable via GYRE_MAX_BODY_SIZE (bytes), default 10MB.\n    let max_body_bytes: usize = std::env::var(\"GYRE_MAX_BODY_SIZE\")",
            "detail": "The 'Message too large | 64KB GYRE_MAX_MESSAGE_SIZE, reject 413' row is not met: GYRE_MAX_MESSAGE_SIZE does not exist anywhere in the codebase (inspected via repo-wide grep); only this global 10MB router body limit applies, with no per-message 64KB cap."
          },
          {
            "file": "crates/gyre-server/src/message_dispatcher.rs",
            "quote": ".acknowledge_all(&Id::new(\"agent-2\".to_string()), \"agent_completed\")",
            "detail": "The dead-agent inbox lifecycle rows are not met in production: this test fixture is the only acknowledge_all call site in the server crate — the completion handler and the stale-agent detector never bulk-ack, so neither 'agent_completed' nor 'agent_orphaned' is ever recorded and the 7-day dead-inbox cleanup never fires."
          },
          {
            "file": "crates/gyre-server/src/oidc.rs",
            "quote": "let jwks: serde_json::Value = serde_json::from_str(&state.agent_signing_key.jwks_json)",
            "detail": "The 'Signature key rotation' row is not met: the JWKS endpoint serves only the single current key's JWK Set (jwks_json is built from one key pair); there is no historical-key storage, so old-key signatures stop being verifiable after rotation."
          },
          {
            "file": "crates/gyre-server/src/api/messages.rs",
            "quote": "return Err(ApiError::TooManyRequests(format!(\n                    \"agent {} inbox is full ({} unacked messages)\",",
            "detail": "The queue-depth row IS met: sends to a full inbox are rejected with 429 before persisting — no silent drops — at GYRE_AGENT_INBOX_MAX (default 1000)."
          }
        ]
      },
      {
        "id": "51c41ab0aaa5ad27913e15df",
        "decision": "satisfied",
        "requirement_quote": "**Consumes from** `notification.rs` — notifications become a downstream consumer of the message bus, not a parallel system",
        "detail": "The cross-spec relationships hold as stated: platform-model.md §4's tool table lists message.send/message.poll/message.ack (implemented as the gyre_message_* MCP tools); the notification system is a downstream MessageConsumer (NotificationBridge, counted under Relationship to Notifications); the spec depends on hierarchy-enforcement.md's non-optional workspace scoping, which holds in practice (workspace_id is None only for Broadcast and tenant-level server events, and the emitters skip rather than fabricate unresolvable scopes); and it uses the Ed25519 signing infrastructure from identity-security.md — message signing reuses the same AgentSigningKey exposed via /.well-known/jwks.json. The 'Supersedes' items are functionally carried out (DomainEvent broadcast, REST inbox, ActivityEvent output path — with the inert remnants noted under Implementation Notes); no contradiction with a related canonical spec was found during inspection.",
        "evidence": [
          {
            "file": "specs/system/platform-model.md",
            "quote": "| `message.send` | workspace | Send a Directed or Custom message to an agent |",
            "detail": "The amendment to platform-model.md §4 is in place: the tool table carries message.send, message.poll, and message.ack, matching the implemented MCP tools."
          },
          {
            "file": "crates/gyre-server/src/message_dispatcher.rs",
            "quote": "impl MessageConsumer for NotificationBridge {",
            "detail": "The 'Consumes from notification.rs' relationship is realized: notifications are a downstream bus consumer, not a parallel system."
          },
          {
            "file": "crates/gyre-server/src/signing.rs",
            "quote": "let sig_bytes = state.agent_signing_key.sign_bytes(sign_input.as_bytes());",
            "detail": "The 'Uses the Ed25519 signing infrastructure from identity-security.md' relationship is realized: bus messages are signed with the same server AgentSigningKey whose public key the OIDC provider exposes."
          }
        ]
      }
    ],
    "gaps": [
      {
        "id": "587ed8cf6bead4ef7920e4ab",
        "decision": "gap",
        "requirement_quote": "The server is the notary — it validates sender identity, signs directed messages, and routes based on destination. No message broker, no external queue. The existing WebSocket + a database table + the existing in-memory broadcast channel.",
        "detail": "The specced design is three components: the existing WebSocket, a database table, and the existing in-memory broadcast channel. Two of three are on the production path: WebSocket delivery runs over message_broadcast_tx (broadcast::Sender<Message>), and the in-memory channel is that same broadcast channel. The database table is NOT on the wired path: the production AppState constructor build_state (the single constructor main.rs uses) sets `messages: Arc::new(mem::MemMessageRepository::default())` unconditionally, while every sibling durable port in the same struct literal selects the SQLite/PostgreSQL adapter via the store! macro or an inline pg_db/sqlite_db conditional. MemMessageRepository is documented in mem.rs as 'In-memory MessageRepository for tests'. Both Diesel adapters (sqlite/message.rs, postgres/message.rs) and migration 2026-03-24-000017_messages exist and conform, but are dead code in production: with GYRE_DATABASE_URL set, every Directed/Event message (send, poll, ack, WS replay, expiry job — all go through state.messages) is stored in a process-local HashMap, and a server restart loses all of them. This wiring gap also invalidates the Directed at-least-once and Event TTL persistence guarantees (counted under Delivery Guarantees, ce88ec9) and the 'Server restart | Persisted messages survive' edge case (e5d4cc99).",
        "evidence": [
          {
            "file": "crates/gyre-server/src/lib.rs",
            "quote": "messages: Arc::new(mem::MemMessageRepository::default()),",
            "detail": "Inside the production AppState construction in build_state; set unconditionally, with no GYRE_DATABASE_URL conditional. This is the only MessageRepository ever wired into AppState (no `.messages =` reassignment exists anywhere in the server crate)."
          },
          {
            "file": "crates/gyre-server/src/lib.rs",
            "quote": "macro_rules! store {",
            "detail": "The store! macro used by every sibling durable port (workspace_memberships, notifications, meta_specs, ...) selects pg_db/sqlite_db adapters when a database is configured; the `messages` field alone bypasses it, so DB-backed deployments still run the message bus on the in-memory adapter."
          },
          {
            "file": "crates/gyre-server/src/mem.rs",
            "quote": "/// In-memory MessageRepository for tests.",
            "detail": "The adapter that is unconditionally production-wired is itself documented as a test stand-in; its backing store is a HashMap, so Directed/Event messages cannot survive restart in any deployment."
          },
          {
            "file": "crates/gyre-server/src/main.rs",
            "quote": "let state = build_state(&auth_token, &base_url, jwt_config);",
            "detail": "main.rs constructs the server state exclusively through build_state, so the unconditional mem wiring is the production path for every deployment, including GYRE_DATABASE_URL-backed ones."
          },
          {
            "file": "crates/gyre-adapters/src/sqlite/message.rs",
            "quote": "impl MessageRepository for SqliteStorage {",
            "detail": "A conforming durable adapter exists (as does its PostgreSQL twin), proving the 'database table' component could be wired but is not — the messages table and these adapters are unreachable in production."
          }
        ]
      },
      {
        "id": "d221d8125b8e34bf71e2fbbd",
        "decision": "gap",
        "requirement_quote": "The `send_message()` helper function validates: if `destination != Broadcast && workspace_id.is_none()`, it returns `Err` and logs at `error` level. The adapter's `store()` method also returns `Err` on `workspace_id: None` — never panics.",
        "detail": "The envelope itself is implemented faithfully: all 11 fields with the specced types and serde external tagging, created_at in epoch milliseconds, signature/key_id Options, acknowledged bool. The REST send handler does derive workspace_id (None only for Broadcast) and manually strips `acknowledged` from POST responses while GET inbox returns the full struct. Four specific requirements are not met: (1) the specced helper validation 'destination != Broadcast && workspace_id.is_none() → Err + error log' does not exist anywhere — the shared internal emitter emit_scoped_event builds the Message with no such guard; emit_tenant_event constructs messages with workspace_id: None by signature, and while its sole production caller (reconciliation.rs emit_tenant_meta_spec_set_updated) currently passes Destination::Broadcast so no non-Broadcast scopeless message is produced today, the specced guard exists on no construction path and the wired mem adapter would silently store such a message (item 2). (2) The store()-Err-on-None contract is enforced by the SQLite and PostgreSQL adapters (message_to_new_row returns Err), but NOT by MemMessageRepository::store, which silently inserts any message — and the mem adapter is the only one wired into production AppState (lib.rs), so the adapter actually on the wired path fails the contract every DB adapter enforces (a mem-port-contract divergence; currently latent because REST send paths never produce None workspace_id for non-Broadcast, but emit_tenant_event's signature permits it and a wired DB adapter would reject what mem silently accepts). (3) `acknowledged` is excluded from POST responses but NOT from WebSocket pushes: ws.rs serializes the full Message struct (serde_json::to_string(&msg)) on the bus delivery path, contradicting the field's own contract 'Excluded from send (POST) responses and WebSocket pushes'. (4) The normative 'Origin and tenant resolution from auth context' table is violated by the send path's origin derivation: origin_from_auth maps ANY Admin-role caller to MessageOrigin::Server regardless of auth mechanism, where the table requires API-key callers to resolve to User(user_id from key lookup) and Keycloak JWT callers to User(user_id from token), reserving Server origin for the global GYRE_AUTH_TOKEN alone. API-key authentication resolves roles from the user record (auth.rs builds AuthenticatedAgent with roles: user.roles on the API-key path), and the Keycloak/OIDC JWT path does the same, so an Admin-role API-key user — and equally an Admin-role Keycloak user — receives Server origin. Verified consequences in the send handler: (a) the server_only() 403 gate passes for these callers, so an Admin API-key/OIDC user can forge every built-in Event-tier kind and GateResult — the forge authority the spec's security note reserves exclusively to the global token; (b) the User-origin workspace-membership check on Directed sends (scoping rule 3) is skipped for them because it is keyed on MessageOrigin::User. The anti-forgery property the table also protects — `from` is never caller-supplied — does hold on every path, but this is a code-versus-spec deviation on a normative security table and is tracked here as a gap requiring faithful repair (map per auth mechanism, preserving the independent Admin-role Broadcast allowance of scoping rule 6, which the Broadcast guard already enforces via a separate role check) or a human decision through the spec lifecycle; the code comment's spoofable-agent_id rationale is a design justification, not a spec amendment.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/lib.rs",
            "quote": "self.emit_scoped_event(tenant_id, None, to, kind, payload)",
            "detail": "emit_tenant_event hands every message to emit_scoped_event with workspace_id: None; its sole production caller (reconciliation.rs emit_tenant_meta_spec_set_updated) currently passes Destination::Broadcast, so no non-Broadcast scopeless message is produced today — but the shared emit_scoped_event body contains no 'destination != Broadcast && workspace_id.is_none() → Err' validation, so the specced helper guard does not exist on any internal construction path."
          },
          {
            "file": "crates/gyre-server/src/mem.rs",
            "quote": "async fn store(&self, message: &gyre_common::message::Message) -> Result<()> {",
            "detail": "MemMessageRepository::store unconditionally inserts into a HashMap and returns Ok(()) — it never returns Err on workspace_id: None. Because this adapter is the one unconditionally wired into production AppState, the 'adapter's store() returns Err on workspace_id: None' contract is not met by the wired adapter."
          },
          {
            "file": "crates/gyre-adapters/src/sqlite/message.rs",
            "quote": "ok_or_else(|| anyhow!(\"cannot store Broadcast message: workspace_id is None\"))?;",
            "detail": "The SQLite (and identically PostgreSQL) adapters DO enforce the store()-Err-on-None contract, so the port contract diverges across adapters — and the enforcing adapters are the ones that are unwired in production."
          },
          {
            "file": "crates/gyre-server/src/ws.rs",
            "quote": "let payload = serde_json::to_string(&msg).unwrap();",
            "detail": "WebSocket bus delivery serializes the full Message envelope including `acknowledged`, so the field leaks into WS pushes; only the POST response path strips it (send_response removes the key), i.e. the 'excluded from WebSocket pushes' half of the conditional-serialization contract is not implemented."
          },
          {
            "file": "crates/gyre-server/src/api/messages.rs",
            "quote": "obj.remove(\"acknowledged\");",
            "detail": "The send path does implement the other half of the contract: send_response strips `acknowledged` from POST /workspaces/:id/messages responses, and the GET inbox handler returns the full struct so agents see delivery state."
          },
          {
            "file": "crates/gyre-server/src/api/messages.rs",
            "quote": "fn origin_from_auth(auth: &AuthenticatedAgent) -> MessageOrigin {\n    if auth.roles.contains(&gyre_domain::UserRole::Admin) {\n        // Admin callers (global token, Admin-role users) → Server origin.\n        MessageOrigin::Server",
            "detail": "The send path's origin derivation is role-based: any caller whose resolved roles contain Admin maps to MessageOrigin::Server regardless of auth mechanism, where the spec's normative table maps API-key callers to User(user_id from key lookup) and Keycloak JWT callers to User(user_id from token), reserving Server origin for the global GYRE_AUTH_TOKEN alone."
          },
          {
            "file": "crates/gyre-server/src/auth.rs",
            "quote": "if let Ok(Some(user_id)) = state.api_keys.find_user_id(&hash_api_key(token)).await {\n            if let Ok(Some(user)) = state.users.find_by_id(&user_id).await {",
            "detail": "API-key authentication resolves the caller's roles from the looked-up user record (the resulting AuthenticatedAgent is built with roles: user.roles; the Keycloak/OIDC JWT path at auth.rs:2043 does the same), so an Admin-role API-key or Keycloak user genuinely reaches origin_from_auth with the Admin role and receives Server origin — the table's API-key and Keycloak rows are violated for Admin users, with the server_only-forge and rule-3-skip consequences verified in the send handler."
          },
          {
            "file": "crates/gyre-server/src/api/messages.rs",
            "quote": "if kind.server_only() && !matches!(from, MessageOrigin::Server) {",
            "detail": "Consequence (a) of the origin-table deviation: the server_only() 403 gate is keyed on the derived origin, so an Admin-role API-key/OIDC caller mapped to Server origin passes it and can send every built-in Event-tier kind and GateResult — forge authority the spec's security note reserves exclusively to the global GYRE_AUTH_TOKEN."
          },
          {
            "file": "crates/gyre-server/src/api/messages.rs",
            "quote": "// User-to-agent: verify workspace membership.\n        if let MessageOrigin::User(ref user_id) = from {",
            "detail": "Consequence (b) of the origin-table deviation: the rule-3 workspace-membership check on Directed sends is keyed on MessageOrigin::User, so an Admin-role API-key/OIDC caller mapped to Server origin by origin_from_auth skips it entirely."
          }
        ]
      },
      {
        "id": "a8f73059653f7d09adbd36b2",
        "decision": "gap",
        "requirement_quote": "They are server-only AND storage-exempt — they flow through the in-memory broadcast channel only, never hitting `MessageRepository::store()`. The send path must short-circuit before attempting storage for these kinds.",
        "detail": "The enum, tiers, and constraints are implemented: every specced variant exists (plus seven later server-only Event additions such as MrReverted and TaskBlocked), tier() classifies Custom as Event with Directed opt-in via the request 'tier' field, server_only() returns true for all built-in Event kinds plus the GateResult Directed exception exactly as specced, custom Deserialize maps unknown strings to Custom(s), and the send handler rejects Directed+Workspace/Broadcast and Telemetry+Agent/Broadcast with 400 and server_only kinds from non-Server origins with 403. Two specific requirements are unmet: (1) the kind-based storage short-circuit for DataSeeded/QueueUpdated is NOT implemented — the send path and internal emitter short-circuit on Destination::Broadcast only, so a server-originated DataSeeded or QueueUpdated sent to a Workspace destination would be persisted, contradicting 'never hitting MessageRepository::store()' for these kinds; (2) DataSeeded and QueueUpdated have no production emitter at all (repo-wide grep finds them only in enum definition/tests and one consumer match arm), so the broadcast-only delivery path this section specifies for them is dead — the exemptions file itself records QueueUpdated as 'Spec'd (message-bus QueueUpdated) but never emitted', a dead spec delivery link owned by this specification.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/messages.rs",
            "quote": "if effective_tier != MessageTier::Telemetry && !matches!(msg.to, Destination::Broadcast) {",
            "detail": "The REST send path's storage guard is tier- and destination-based only; there is no kind-based short-circuit for DataSeeded/QueueUpdated, so those kinds reach store() whenever their destination is not Broadcast. The internal emitter (lib.rs emit_scoped_event) mirrors this with the same destination-only guard."
          },
          {
            "file": "scripts/dead-message-kind-exemptions.txt",
            "quote": "# Spec'd (platform-model hot-file warnings; message-bus QueueUpdated)\n# but never emitted — pre-existing gaps, tracked here so the check is\n# green for NEW kinds.",
            "detail": "The broadcast-only kind QueueUpdated (and likewise DataSeeded, which has no emitter and no exemption entry) is never constructed in production code — the spec's broadcast-only kinds never flow through the in-memory broadcast channel because nothing emits them."
          },
          {
            "file": "crates/gyre-common/src/message.rs",
            "quote": "| MessageKind::GateResult => MessageTier::Directed,",
            "detail": "Tier classification matches the spec including the GateResult Directed exception; BudgetWarning/BudgetExhausted/AgentError sit in the Event arm and the telemetry kinds in the Telemetry arm, per the 'Changes from prior draft' list."
          },
          {
            "file": "crates/gyre-server/src/api/messages.rs",
            "quote": "\"Directed tier requires Agent destination, not Workspace\".to_string(),",
            "detail": "Tier+destination constraints are enforced with 400 in the send handler (and mirrored on the MCP path): Directed+Workspace, Directed+Broadcast, Telemetry+Agent, Telemetry+Broadcast are all rejected."
          },
          {
            "file": "crates/gyre-server/src/api/messages.rs",
            "quote": "if kind.server_only() && !matches!(from, MessageOrigin::Server) {",
            "detail": "The server_only() 403 check is enforced on the send path, covering the GateResult forge-authority exception and all built-in Event kinds, with Custom exempt as specced."
          }
        ]
      },
      {
        "id": "57b856585e62b254ad3ab3d4",
        "decision": "gap",
        "requirement_quote": "If more than 1000 exist, the server sends the newest 1000 and includes a `{\"type\": \"ReplayCatchUp\", \"truncated\": true}` message — the client can use `GET /api/v1/workspaces/:id/messages` with cursor pagination for the full history.",
        "detail": "The subscription machinery is implemented: the Subscribe WsMessage variant (scopes, last_seen Option<u64> epoch millis, session_id Option<String>), the catch-all #[serde(other)] Unknown variant, replay of persisted Event-tier messages through state.messages.list_by_workspace with a 1000-message cap and a ReplayCatchUp{truncated:true} signal, telemetry never replayed (never persisted), Directed excluded from replay (list_by_workspace filters to_type != 'agent'), connection identity established at auth time so Destination::Agent(id) is delivered only to the connection whose verified agent id matches (shared-token/Admin connections are excluded), Workspace delivery gated on subscription, and Broadcast delivered to all. Two specific deviations from the specced replay behavior remain: (1) truncation window — the handler fetches the newest replay_limit+1 messages per workspace, reverses to oldest-first, and sends the first 1000 it encounters, i.e. the OLDEST 1000 of the newest 1001, dropping the newest message; the spec requires 'the server sends the newest 1000'. (2) Boundary comparison — the spec (and the protocol.rs doc comment 'created_at > last_seen') requires strictly greater, but both the mem and SQLite list_by_workspace implementations filter created_at >= since, so a message created exactly at the client's last_seen timestamp is re-delivered on every reconnect. The Subscribe-membership authorization gap is counted under Telemetry Store (2b439cf6) where the spec states it.",
        "evidence": [
          {
            "file": "crates/gyre-common/src/protocol.rs",
            "quote": "Subscribe {\n        scopes: Vec<SubscribeScope>,\n        last_seen: Option<u64>,\n        #[serde(default)]\n        session_id: Option<String>,\n    },",
            "detail": "The Subscribe variant exists with the specced shape: workspace scopes, optional last_seen (epoch millis), optional session_id for presence tracking."
          },
          {
            "file": "crates/gyre-server/src/ws.rs",
            "quote": "// Replay Event-tier messages since last_seen, oldest-first.\n                                    let mut replayed = 0usize;\n                                    let replay_limit = 1000usize;",
            "detail": "Replay on subscribe is implemented with a 1000-message cap, queries state.messages (the MessageRepository), and emits ReplayCatchUp{truncated:true} when the cap is hit."
          },
          {
            "file": "crates/gyre-server/src/ws.rs",
            "quote": "// list_by_workspace returns newest-first; reverse to oldest-first.\n                                            messages.reverse();\n                                            for m in messages {\n                                                if replayed >= replay_limit {\n                                                    truncated = true;\n                                                    break;\n                                                }",
            "detail": "The handler fetches the newest replay_limit+1 per workspace and then sends the first 1000 in oldest-first order — the oldest 1000 of the newest 1001 — whereas the spec requires sending the newest 1000; the newest message in the window is the one dropped."
          },
          {
            "file": "crates/gyre-adapters/src/sqlite/message.rs",
            "quote": "query = query.filter(messages::created_at.ge(since_ts as i64));",
            "detail": "The SQLite (and identically mem) list_by_workspace since-filter is inclusive (>=), so a message created exactly at the client's last_seen is replayed on reconnect; the spec requires created_at > last_seen (strictly greater, no re-delivery)."
          },
          {
            "file": "crates/gyre-server/src/ws.rs",
            "quote": "caller.user_id.is_none()\n                                    && !caller\n                                        .roles\n                                        .contains(&gyre_domain::UserRole::Admin)\n                                    && caller.agent_id == aid.as_str()",
            "detail": "Destination filtering is implemented per spec: agent-targeted messages reach only the connection whose verified agent identity matches (shared-token/Admin connections are excluded and must use REST poll), workspace delivery is subscription-gated, broadcast reaches all."
          }
        ]
      },
      {
        "id": "ce88ec9cd0bc4a86136ae33b",
        "decision": "gap",
        "requirement_quote": "**Directed** | At-least-once (persisted until ack, redelivered on poll) | DB — no TTL, expires only on ack or agent completion",
        "detail": "Partially implemented. Implemented: the Event TTL expiry job (GYRE_EVENT_TTL_SECS default 7 days, hourly background job calling expire_events, plus the dead-inbox retention GYRE_DEAD_INBOX_TTL_SECS), the telemetry ring buffer (in-memory, per-workspace 10,000 default via GYRE_TELEMETRY_BUFFER_SIZE, largest-first workspace eviction at 100 workspaces via GYRE_TELEMETRY_MAX_WORKSPACES), the Directed queue depth limit (GYRE_AGENT_INBOX_MAX default 1000, sends rejected with 429 before persisting, no silent drops), and poll-based redelivery. NOT implemented: (1) the Directed persistence medium — 'DB — no TTL, expires only on ack' is not on the wired path; production state.messages is the in-memory MemMessageRepository in every deployment, so restart orphans every unacked Directed message and the at-least-once guarantee fails exactly when it matters (after a crash); (2) agent completion cleanup — 'When an agent completes (POST /api/v1/agents/:id/complete), all unacked Directed messages in its inbox are marked acknowledged with ack_reason agent_completed' is never performed: acknowledge_all has no production caller (the only call site in the server crate is a test fixture; neither the REST complete handler in api/spawn.rs nor the MCP agent-complete path calls it); (3) consequently the orphaned-agent path ('the stale agent detector marks them Dead and calls acknowledge_all with ack_reason agent_orphaned') is also unimplemented — stale_agents.rs transitions agents to Dead but never bulk-acks, so expire_acked_inboxes can never match a dead inbox in production and unacked Directed messages to dead agents accumulate indefinitely.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/lib.rs",
            "quote": "messages: Arc::new(mem::MemMessageRepository::default()),",
            "detail": "The Directed/Event persistence medium in production is the in-memory adapter (unconditionally wired), so 'DB — no TTL, expires only on ack' does not describe the deployed system: a restart drops every unacked Directed message."
          },
          {
            "file": "crates/gyre-server/src/message_dispatcher.rs",
            "quote": "let events_deleted = state\n        .messages\n        .expire_events(now.saturating_sub(event_ttl_ms))\n        .await?;",
            "detail": "The Event-tier TTL expiry job IS implemented (GYRE_EVENT_TTL_SECS default 604_800s, run_message_expiry spawned hourly) — though it too runs against the in-memory store in production."
          },
          {
            "file": "crates/gyre-server/src/lib.rs",
            "quote": "agent_inbox_max: std::env::var(\"GYRE_AGENT_INBOX_MAX\")\n            .ok()\n            .and_then(|v| v.parse().ok())\n            .unwrap_or(1000),",
            "detail": "The Directed queue depth limit is configurable with the specced default of 1000; the send path rejects with 429 (TooManyRequests) before persisting, preserving the no-silent-drop requirement."
          },
          {
            "file": "crates/gyre-server/src/message_dispatcher.rs",
            "quote": ".acknowledge_all(&Id::new(\"agent-2\".to_string()), \"agent_completed\")",
            "detail": "The ONLY acknowledge_all call site in the server crate is this test fixture in message_dispatcher.rs's tests; the production completion paths (POST /api/v1/agents/:id/complete in api/spawn.rs, MCP agent-complete in mcp.rs) and stale_agents.rs were inspected and never call acknowledge_all, so neither 'agent_completed' nor 'agent_orphaned' is ever recorded in production and the dead-inbox cleanup never fires."
          },
          {
            "file": "crates/gyre-server/src/stale_agents.rs",
            "quote": "let _ = agent.transition_status(AgentStatus::Dead);",
            "detail": "The stale-agent detector marks orphaned agents Dead but performs no acknowledge_all with ack_reason 'agent_orphaned' — the spec's orphan cleanup path is absent."
          }
        ]
      },
      {
        "id": "87668c235d98ebb2cdc31db7",
        "decision": "gap",
        "requirement_quote": "**All `Destination::Agent(id)` messages validate tenant isolation.** The target agent's `tenant_id` must match the message's `tenant_id`, regardless of origin (including `Server`). This prevents a server bug from leaking messages across tenants.",
        "detail": "Rules 2, 3, 5, 6, and 7 are implemented and enforced in the send handler with the specced status codes: agent-to-agent Directed requires the sender's workspace to match (403); user-to-agent requires WorkspaceMembershipRepository membership (403); Broadcast requires Server origin or Admin role (403, and the MCP path rejects Broadcast outright); workspace fan-out requires the destination to match the URL workspace (400) with agent same-workspace (403) and user membership (403) checks; server-originated events inherit the workspace of the entity they describe (emit_event resolves the workspace record and skips on unresolvable scope). Rule 1 is NOT implemented as specified: there is no tenant_id comparison anywhere in the send path. Isolation for User and Agent origins is enforced only transitively — the target agent must belong to the URL workspace, and those origins must themselves be members of that workspace, which implies same-tenant. But the spec requires the target agent's tenant_id to match the message's tenant_id 'regardless of origin (including Server)': a Server-origin send (global GYRE_AUTH_TOKEN maps to Server with the caller's tenant) targeting an agent in another tenant's workspace passes the handler with the message's tenant_id set to the caller's tenant and no target-tenant validation, exactly the server-bug leak class rule 1 exists to prevent. Additionally, rule 3's User-origin membership check is keyed on MessageOrigin::User and is therefore skipped for Admin-role API-key/OIDC callers, who receive Server origin under origin_from_auth's role-based mapping — a consequence of the origin-table deviation tracked under Message Envelope (d221d8125b8e34bf71e2fbbd, item 4), recorded here because it weakens rule 3's enforcement on the send path.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/messages.rs",
            "quote": "// Target must be in this workspace (enforces tenant isolation since workspace IDs are\n        // globally unique and each workspace belongs to exactly one tenant).\n        if target_agent.workspace_id != ws_id {",
            "detail": "The only target validation is a workspace match against the URL workspace; there is no comparison of the target agent's tenant_id with the message's tenant_id, so rule 1's 'regardless of origin (including Server)' tenant check does not exist as an explicit validation."
          },
          {
            "file": "crates/gyre-server/src/api/messages.rs",
            "quote": "return Err(ApiError::Forbidden(\n                    \"agents can only send Directed messages to agents in the same workspace\"\n                        .to_string(),",
            "detail": "Rule 2 (agent→agent same workspace, 403) is implemented."
          },
          {
            "file": "crates/gyre-server/src/api/messages.rs",
            "quote": "let membership = state\n                .workspace_memberships\n                .find_by_user_and_workspace(user_id, &ws_id)",
            "detail": "Rule 3 (user→agent workspace membership via WorkspaceMembershipRepository, 403 on absence) is implemented — but the check is keyed on MessageOrigin::User, so Admin-role API-key/OIDC callers mapped to Server origin by origin_from_auth skip it (origin-table deviation tracked under Message Envelope, d221d8125b8e34bf71e2fbbd, item 4)."
          },
          {
            "file": "crates/gyre-server/src/api/messages.rs",
            "quote": "\"Broadcast destination requires Server origin or Admin role\".to_string(),",
            "detail": "Rule 6 (Broadcast requires Server origin or Admin role, 403) is implemented; agent JWTs map to Agent origin without Admin role and are rejected."
          },
          {
            "file": "crates/gyre-server/src/lib.rs",
            "quote": "let Some(tenant_id) = self.message_tenant_id(&workspace_id).await else {",
            "detail": "Rule 5 is implemented on the internal emitter: emit_event resolves the tenant from the referenced entity's workspace record and skips emission when the scope cannot be resolved rather than fabricating one — but nothing downstream re-validates the target's tenant against it."
          }
        ]
      },
      {
        "id": "2b439cf656cf14c344afdb07",
        "decision": "gap",
        "requirement_quote": "**Subscription authorization:** The server verifies workspace membership when processing `Subscribe` messages — a client cannot subscribe to a workspace it doesn't belong to. This prevents telemetry leakage across workspaces even though the buffer itself has no tenant isolation.",
        "detail": "The buffer itself is implemented exactly as specced: TelemetryBuffer in gyre-common with per-workspace DashMap<Id, VecDeque<Message>> ring buffers, FIFO oldest-entry eviction at max_per_workspace (default 10,000, GYRE_TELEMETRY_BUFFER_SIZE), and largest-first workspace eviction when distinct workspaces exceed max_workspaces (default 100, GYRE_TELEMETRY_MAX_WORKSPACES) — wired in build_state with those env vars and defaults. Telemetry is pushed through the broadcast channel and stored only in the buffer, never in MessageRepository. The subscription-authorization requirement is NOT met: the ws.rs Subscribe handler validates only that each requested workspace belongs to the caller's TENANT (plus an Admin bypass) — it performs no workspace-membership verification (no WorkspaceMembershipRepository lookup, in contrast to the REST handlers which do check membership). A user authenticated in tenant T can subscribe to any workspace in T without being a member, and since workspace-scoped bus messages and telemetry are delivered to subscribed connections, same-tenant non-members receive workspace telemetry the spec says they must not.",
        "evidence": [
          {
            "file": "crates/gyre-common/src/message.rs",
            "quote": "pub struct TelemetryBuffer {\n    /// Per-workspace ring buffers. Key: workspace_id.\n    buffers: DashMap<Id, VecDeque<Message>>,",
            "detail": "The telemetry store is the specced in-memory per-workspace ring buffer, replacing ActivityStore; it is not part of MessageRepository."
          },
          {
            "file": "crates/gyre-common/src/message.rs",
            "quote": "// Enforce workspace count limit: evict the workspace with the most entries.\n        if self.buffers.len() > self.max_workspaces {",
            "detail": "Largest-first workspace eviction is implemented as specified (the anti-flushing mitigation), alongside FIFO eviction at max_per_workspace."
          },
          {
            "file": "crates/gyre-server/src/lib.rs",
            "quote": "let telemetry_buffer = Arc::new(TelemetryBuffer::new(\n        std::env::var(\"GYRE_TELEMETRY_BUFFER_SIZE\")",
            "detail": "The buffer is wired in build_state with GYRE_TELEMETRY_BUFFER_SIZE (default 10_000) and GYRE_TELEMETRY_MAX_WORKSPACES (default 100), matching the specced defaults."
          },
          {
            "file": "crates/gyre-server/src/ws.rs",
            "quote": "let tenant_match = caller.roles.contains(&gyre_domain::UserRole::Admin)\n                                                    || workspace.tenant_id.as_str() == caller.tenant_id;",
            "detail": "Subscribe processing authorizes on tenant match (plus Admin bypass) only — no workspace-membership verification is performed, so a same-tenant non-member can subscribe to a workspace it does not belong to, contradicting the section's explicit authorization requirement."
          }
        ]
      },
      {
        "id": "64d1be477f27ebf27ec73076",
        "decision": "gap",
        "requirement_quote": "Queries the in-memory `TelemetryBuffer`. The `since` parameter on this endpoint uses **epoch milliseconds** (matching the message bus, not the legacy seconds convention). The handler verifies workspace membership before returning results — an unauthenticated or unauthorized caller receives 403.",
        "detail": "The specced endpoint does not exist. There is no GET /api/v1/workspaces/:workspace_id/activity route anywhere in the router; the only activity routes are /api/v1/activity (which synthesizes events from notifications, is not workspace-scoped, and takes an unused _auth parameter with no workspace-membership check) and /api/v1/users/:username/activity (an unrelated profile feed). The TelemetryBuffer itself is only queryable through the WebSocket ActivityQuery path (scoped to subscribed workspaces) and the admin-only /api/v1/admin/audit endpoint (cross-workspace) — neither is the REST endpoint with per-workspace membership enforcement and the ActivityEventData field mapping (event_id/agent_id/event_type/description/timestamp) that this section specifies. The mapping logic exists in the WS path but the specced endpoint, its 403 membership enforcement, and its response contract are missing.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/mod.rs",
            "quote": ".route(\"/api/v1/activity\", get(activity::activity_handler))",
            "detail": "The only general activity route is the unscoped /api/v1/activity; no /api/v1/workspaces/:workspace_id/activity route exists anywhere in the router (inspected the full route table in api/mod.rs)."
          },
          {
            "file": "crates/gyre-server/src/api/activity.rs",
            "quote": "/// GET /api/v1/activity — unified activity feed.\n///\n/// Synthesizes activity events from notifications (which are populated for all",
            "detail": "The existing activity handler is notification-synthesized, not TelemetryBuffer-backed: it never reads state.telemetry_buffer, is not workspace-scoped, and its _auth parameter is unused (no membership check), so neither the backing store nor the 403 authorization requirement of this section is met on any REST path."
          },
          {
            "file": "crates/gyre-common/src/message.rs",
            "quote": "pub fn list_since(&self, workspace_id: &Id, since_ms: u64, limit: usize) -> Vec<Message> {",
            "detail": "TelemetryBuffer does provide the per-workspace, epoch-millisecond since query the endpoint would need (used by the WebSocket ActivityQuery path), so the gap is the missing REST endpoint and its membership enforcement, not the store."
          }
        ]
      },
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
      },
      {
        "id": "e5d4cc99cba7ab16b2b40bb5",
        "decision": "gap",
        "requirement_quote": "| Server restart | Persisted messages survive. Telemetry buffer is lost (acceptable — it's ephemeral). WebSocket connections drop; clients reconnect and re-subscribe with `last_seen`. |",
        "detail": "Implemented rows: send-to-self is allowed (no from==to check exists); workspace fan-out to 100 agents is one store plus one broadcast send; queue depth rejects with 429 at GYRE_AGENT_INBOX_MAX (default 1000) with no silent drops; concurrent sends are atomic (single INSERT in the DB adapters, mutex-guarded insert in mem); concurrent ack is idempotent (200). NOT implemented rows: (1) Server restart — persisted messages do NOT survive in any deployment because AppState.messages is unconditionally the in-memory MemMessageRepository (the SQLite/PG adapters and messages table are unwired); only the telemetry-buffer-lost and WS-reconnect halves of the row hold. (2) Message too large — there is no 64KB GYRE_MAX_MESSAGE_SIZE limit and no message-specific 413; only the global GYRE_MAX_BODY_SIZE (default 10MB) body limit applies, so a message payload can be ~10MB, far beyond the specced 64KB envelope cap. (3) Send to dead/completed agent — the message is stored (no status guard), but the row's cleanup lifecycle is broken: neither 'acknowledge_all was called at completion time' (no production caller on POST /api/v1/agents/:id/complete) nor the orphaned-agent 'agent_orphaned' bulk-ack (absent from stale_agents.rs) ever happens, so expire_dead_inboxes/expire_acked_inboxes can never reclaim those inboxes (counted under Delivery Guarantees ce88ec9). (4) Signature key rotation — the JWKS endpoint serves only the single current key (jwks_json is built from one key pair); there is no key-history storage, so 'messages signed with old key remain verifiable — JWKS endpoint serves historical keys' is unimplemented.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/lib.rs",
            "quote": "messages: Arc::new(mem::MemMessageRepository::default()),",
            "detail": "The 'Server restart | Persisted messages survive' row is not met: the production message store is a process-local HashMap in every deployment (DB adapters unwired), so a restart loses all Directed and Event messages."
          },
          {
            "file": "crates/gyre-server/src/lib.rs",
            "quote": "// Body size limit: configurable via GYRE_MAX_BODY_SIZE (bytes), default 10MB.\n    let max_body_bytes: usize = std::env::var(\"GYRE_MAX_BODY_SIZE\")",
            "detail": "The 'Message too large | 64KB GYRE_MAX_MESSAGE_SIZE, reject 413' row is not met: GYRE_MAX_MESSAGE_SIZE does not exist anywhere in the codebase (inspected via repo-wide grep); only this global 10MB router body limit applies, with no per-message 64KB cap."
          },
          {
            "file": "crates/gyre-server/src/message_dispatcher.rs",
            "quote": ".acknowledge_all(&Id::new(\"agent-2\".to_string()), \"agent_completed\")",
            "detail": "The dead-agent inbox lifecycle rows are not met in production: this test fixture is the only acknowledge_all call site in the server crate — the completion handler and the stale-agent detector never bulk-ack, so neither 'agent_completed' nor 'agent_orphaned' is ever recorded and the 7-day dead-inbox cleanup never fires."
          },
          {
            "file": "crates/gyre-server/src/oidc.rs",
            "quote": "let jwks: serde_json::Value = serde_json::from_str(&state.agent_signing_key.jwks_json)",
            "detail": "The 'Signature key rotation' row is not met: the JWKS endpoint serves only the single current key's JWK Set (jwks_json is built from one key pair); there is no historical-key storage, so old-key signatures stop being verifiable after rotation."
          },
          {
            "file": "crates/gyre-server/src/api/messages.rs",
            "quote": "return Err(ApiError::TooManyRequests(format!(\n                    \"agent {} inbox is full ({} unacked messages)\",",
            "detail": "The queue-depth row IS met: sends to a full inbox are rejected with 429 before persisting — no silent drops — at GYRE_AGENT_INBOX_MAX (default 1000)."
          }
        ]
      }
    ],
    "source_evidence": [
      {
        "file": "crates/gyre-server/src/lib.rs",
        "sha256": "f775df8c33cf8aba7dfe75a838183a2a7724307a2259eeb0ed57575093afb4c5"
      },
      {
        "file": "crates/gyre-server/src/mem.rs",
        "sha256": "26936308a71694c9eb2fab27f651905697dbabef86a8ec94362af8322feec4d7"
      },
      {
        "file": "crates/gyre-server/src/main.rs",
        "sha256": "f4f6948bf1e818ab4437380f391e04e969f1530fb70d5672fe60638dc7a0d82a"
      },
      {
        "file": "crates/gyre-adapters/src/sqlite/message.rs",
        "sha256": "28d8f3d6ac8c6f7c60fa77182e946a782520b92c17755bc532b81f5fe11cc7ce"
      },
      {
        "file": "crates/gyre-common/src/message.rs",
        "sha256": "416b817e77f772d7806544822303a5a760e601b0212dd4abdc175301dd9983cb"
      },
      {
        "file": "crates/gyre-ports/src/message.rs",
        "sha256": "ba0a57ec575b673844a34c047a09fe2e66936592c0bee86b3e4a53970f66b574"
      },
      {
        "file": "crates/gyre-domain/src/lib.rs",
        "sha256": "cc33a9b7fa9e9700a8478106e6aaa6bc6312c687badc33bb316d2e341d81537f"
      },
      {
        "file": "crates/gyre-server/src/ws.rs",
        "sha256": "eb944c061e728112bfa25d826a302b46d4a8ad3edb4b5d91d0d71cf91d5631f0"
      },
      {
        "file": "crates/gyre-server/src/api/messages.rs",
        "sha256": "e65cd03184f491f911cf9a8a9281a56c4013efd5832aee4654533ade60df564d"
      },
      {
        "file": "crates/gyre-server/src/auth.rs",
        "sha256": "5877b30260e2fa22eb80278049d17d64ce65a7f9a34ec2fc037a811c774cfa69"
      },
      {
        "file": "scripts/dead-message-kind-exemptions.txt",
        "sha256": "ab62173e8723e5558a60dead222b8e02a0082eceda338fe1b1e9867b613a04c3"
      },
      {
        "file": "crates/gyre-server/src/signing.rs",
        "sha256": "7ed2a5e0ca9e2e6a091bf6cde1219938e88ea1e3d8dc2e01d855290108f80ecc"
      },
      {
        "file": "crates/gyre-server/src/oidc.rs",
        "sha256": "88c86864045d18270be95aac1f7afda7c46c008d0d295c82aecc8db364b481ce"
      },
      {
        "file": "crates/gyre-common/src/protocol.rs",
        "sha256": "e78f25a0483052879156f559381020417cc0e434819eb5cdf81ab4195e237c62"
      },
      {
        "file": "crates/gyre-server/src/api/mod.rs",
        "sha256": "7cd819be7922b9c62b5e48b6e84cf5c3b75c8015b76f16501439df4afc6219e1"
      },
      {
        "file": "crates/gyre-server/src/message_dispatcher.rs",
        "sha256": "aaa1c24d8e0d1e250160c5dfda344fd370235717b6f5ce3941b72d2ce716636d"
      },
      {
        "file": "crates/gyre-server/src/stale_agents.rs",
        "sha256": "221b4c73f788426bbef0bcf597f8bd4d463c993d02aabd6b00f27f8fd6d7eb43"
      },
      {
        "file": "crates/gyre-adapters/src/postgres/message.rs",
        "sha256": "9c3111deb941e03f2ff90d79b1d268da8b7f9bbaa739cc25c98993374a5a5882"
      },
      {
        "file": "scripts/check-tenant-filter.sh",
        "sha256": "47c996a9c6f9b20056ffe4ba657e5ba9ba2af5ddf9bf003a279339bf8234e8c0"
      },
      {
        "file": "crates/gyre-adapters/migrations/2026-03-24-000017_messages/up.sql",
        "sha256": "24dd75e48f7cf30f123b6c98a9e132b5acf72e1038b273f952be5314f670ee2a"
      },
      {
        "file": "crates/gyre-server/src/api/activity.rs",
        "sha256": "145fef00236babb0975f7b4011ca59ecee8892e86389972a5ea1552d9973f224"
      },
      {
        "file": "crates/gyre-server/src/mcp.rs",
        "sha256": "f08bb495c68513b7420d9ceec2a246365f7db8dddf6f0dece6fae17116381f09"
      },
      {
        "file": "specs/system/platform-model.md",
        "sha256": "78397e1267062ac7eb9f7063dc9eba224695ed6f94336f90a9c4494ebd9dfd49"
      }
    ]
  },
  "plan": {
    "audit": "c60978068a439497b2395c3cafeef956290d926e0da9389ce06c5f25c4e554ff",
    "tasks": [
      {
        "key": "db-message-repository-wiring",
        "title": "Wire the SQLite/PostgreSQL MessageRepository into production AppState",
        "covers": [
          "587ed8cf6bead4ef7920e4ab"
        ],
        "required_behavior": "Per message-bus.md Core Principle, the database table must be on the production path: 'The existing WebSocket + a database table + the existing in-memory broadcast channel.' In build_state (crates/gyre-server/src/lib.rs:1697) the messages port is currently set unconditionally to `Arc::new(mem::MemMessageRepository::default())` while every sibling durable port in the same struct literal selects its SQLite/PostgreSQL adapter via the store! macro or an inline pg_db/sqlite_db conditional. Replace the unconditional mem assignment with the same store! wiring pattern the sibling ports use (store!(dyn MessageRepository, mem::MemMessageRepository::default()) or the equivalent inline conditional), so that with GYRE_DATABASE_URL set the Diesel adapter (crates/gyre-adapters/src/sqlite/message.rs or postgres/message.rs, migration 2026-03-24-000017_messages) serves every state.messages operation (send, poll, ack, WS replay, expire jobs), and the mem adapter is used only where sibling durable ports fall back to mem. Do not change the port trait, the adapters, or the mem implementation in this task — only the production wiring. This is the persistence foundation for the Directed at-least-once guarantee (gap ce88ec9) and the server-restart edge-case row (gap e5d4cc99), which are owned by dependent tasks.",
        "acceptance": [
          "With GYRE_DATABASE_URL pointing at a real SQLite file, a Directed message sent via POST /api/v1/workspaces/:id/messages is present in the messages table (to_type='agent', acknowledged=0) and is returned by GET /api/v1/agents/:id/messages.",
          "Restart persistence: build a server against a SQLite GYRE_DATABASE_URL, store an unacked Directed message and an Event-tier workspace message, tear down the process/AppState, rebuild against the same database URL, and verify both messages are still returned (Directed via poll, Event via GET /api/v1/workspaces/:id/messages and via Subscribe replay) — the at-least-once and TTL guarantees hold across restart.",
          "The wired port is the Diesel adapter, not MemMessageRepository, whenever GYRE_DATABASE_URL is set (behavioral proof via the restart test above, which fails against the HashMap-backed mem adapter).",
          "Existing message-bus behavior is unchanged on the wired DB path: send response shape (acknowledged excluded), 429 queue-depth rejection via count_unacked, idempotent ack, and Event TTL expiry via expire_events all still pass their existing tests against the SQLite adapter."
        ],
        "negative_probe": "Store an unacked Directed message with GYRE_DATABASE_URL set, drop the AppState, rebuild it against the same URL, and poll the agent inbox. With the current wiring (mem::MemMessageRepository unconditionally in build_state, lib.rs:1697) the message is gone — the restart-persistence test fails. The fix only counts if the same probe passes with the Diesel adapter selected by the store! conditional.",
        "dependencies": [],
        "existing_task": null
      },
      {
        "key": "message-send-path-guards",
        "title": "Message send-path guards: scope validation, mem store contract, WS acknowledged exclusion, auth-mechanism origin table, Agent-destination tenant isolation",
        "covers": [
          "d221d8125b8e34bf71e2fbbd",
          "87668c235d98ebb2cdc31db7"
        ],
        "required_behavior": "Five deviations from the Message Envelope and Scoping Rules sections, all on the message construction/send path: (1) Scopeless-message guard (spec: 'The send_message() helper function validates: if destination != Broadcast && workspace_id.is_none(), it returns Err and logs at error level'): add this validation to the shared internal construction/emitter path (the emit_scoped_event/emit_tenant_event family in crates/gyre-server/src/lib.rs) so no server-internal emitter can produce a non-Broadcast message with workspace_id: None — it must return Err and log at error level, never construct, store, or broadcast the message. (2) Mem adapter port contract (spec: \"The adapter's store() method also returns Err on workspace_id: None — never panics\"): MemMessageRepository::store (crates/gyre-server/src/mem.rs:4840) currently inserts any message silently; make it return Err on workspace_id: None, matching the SQLite/PostgreSQL adapters (message_to_new_row) — the mem adapter must enforce the same port failure constraint in code (check-mem-port-contracts invariant). (3) acknowledged exclusion from WebSocket pushes (spec field contract: 'Excluded from send (POST) responses and WebSocket pushes via conditional serialization — the field IS included in GET inbox responses'): exactly two sites serialize a full bus Message over the WebSocket — the live bus delivery arm (ws.rs:459, serde_json::to_string(&msg)) and the Subscribe replay arm (ws.rs:173, serde_json::to_string(&m)). Strip `acknowledged` at BOTH sites using the same manual-removal pattern as send_response in api/messages.rs; do not change GET inbox responses, which must keep the field. (4) Origin resolution per the normative 'Origin and tenant resolution from auth context' table: origin_from_auth (crates/gyre-server/src/api/messages.rs:46) currently maps ANY Admin-role caller to MessageOrigin::Server. Fix it to derive origin from the actual auth mechanism — global GYRE_AUTH_TOKEN → Server; API key (gyre_*) → User(user_id from key lookup); Keycloak/OIDC JWT → User(user_id from token); agent JWT → Agent(sub). The mechanism discriminator must come from the authentication extractor (thread a reliable mechanism/global-token signal through AuthenticatedAgent), not from spoofable strings like agent_id == \"system\" or role membership. Preserve scoping rule 6 independently: the Broadcast guard's separate Admin-role check must still allow Admin-role API-key callers to target Broadcast, and the global token keeps Server origin (full forge authority). Consequence that must hold after the fix: Admin-role API-key/OIDC callers resolve to User origin, so the server_only() 403 gate applies to them (they can no longer forge built-in Event kinds or GateResult) and the rule-3 User-origin workspace-membership check on Directed sends applies to them. (5) Scoping rule 1 tenant isolation (spec: 'All Destination::Agent(id) messages validate tenant isolation. The target agent's tenant_id must match the message's tenant_id, regardless of origin (including Server)'): in the REST send handler (api/messages.rs) and the mirrored MCP send path (mcp.rs handle_message_send), when the destination is Agent(id), load the target agent record and compare its tenant_id with the message's tenant_id (the caller's verified tenant); on mismatch reject with 403 — including for Server-origin (global-token) callers, which is exactly the server-bug leak class rule 1 exists to prevent.",
        "acceptance": [
          "Scopeless guard: a test drives the internal emitter (or the construction helper it shares) with destination Workspace/Agent and workspace_id None and asserts an Err return plus an error-level log record, and that no message reaches store() or the broadcast channel.",
          "Mem contract: MemMessageRepository::store on a message with workspace_id None returns Err (test in mem.rs mirroring the SQLite adapter's contract test), while Broadcast messages are never passed to store() by any send path.",
          "WS acknowledged exclusion, live arm: a bus-delivered Message received on a subscribed WebSocket connection has no `acknowledged` key in the JSON (ws.rs:459 arm).",
          "WS acknowledged exclusion, replay arm (review round-3 correction): connect, Subscribe with last_seen=0 against a workspace with a persisted Event-tier message, and assert the replayed JSON also lacks the `acknowledged` key — both WebSocket Message serialization sites (ws.rs:459 live delivery and ws.rs:173 replay) are covered, not just the live arm.",
          "GET /api/v1/agents/:id/messages (inbox) responses still INCLUDE `acknowledged` (crash-recovery contract) after the change.",
          "Origin table: an Admin-role API-key caller and an Admin-role OIDC caller each resolve to User(<user_id>) origin — verified by sending a server_only kind (e.g. agent_created), which must now be rejected 403, and by a Directed send to an agent in a workspace the user is NOT a member of, which must now be rejected 403 via the rule-3 membership check.",
          "Origin table, global token: the global GYRE_AUTH_TOKEN still resolves to Server origin and can send server_only kinds and target Broadcast (rule 6 forge authority preserved).",
          "Rule 6 preserved for Admin API keys: an Admin-role API-key caller can still send a Broadcast message (separate role check), while a non-Admin API-key caller cannot (403).",
          "Tenant isolation rule 1: a global-token (Server-origin) caller whose tenant is T1 sending a Directed message to an agent in tenant T2's workspace is rejected with 403 and nothing is stored or broadcast; same-tenant sends still succeed. The mirrored MCP send path enforces the same comparison."
        ],
        "negative_probe": "Two probes against the current defects: (a) origin forging — authenticate with an Admin-role API key and POST a message with kind \"agent_created\" (server_only); the current code maps the caller to MessageOrigin::Server (api/messages.rs:46-52) and returns 201, letting a non-global-token caller forge server events; after the fix it must return 403, and the test must fail if the role-based → Server mapping returns. (b) cross-tenant Directed leak — with the global token (default tenant T1), POST to a workspace in tenant T2 targeting an agent in that workspace; the current handler stores the message with tenant_id=T1 pointed at a T2 agent with no tenant comparison anywhere in the send path; after the fix the request must fail 403 and the messages store must contain no row, and the test must fail if the tenant comparison is removed.",
        "dependencies": [],
        "existing_task": null
      },
      {
        "key": "broadcast-only-kind-emitters",
        "title": "Storage-exempt broadcast kinds: kind-based short-circuit and real DataSeeded/QueueUpdated emitters",
        "covers": [
          "a8f73059653f7d09adbd36b2"
        ],
        "required_behavior": "Per message-bus.md Message Kinds / Broadcast-only kinds: 'DataSeeded and QueueUpdated ... are server-only AND storage-exempt — they flow through the in-memory broadcast channel only, never hitting MessageRepository::store(). The send path must short-circuit before attempting storage for these kinds.' Two requirements: (1) Kind-based storage short-circuit: the REST send handler (api/messages.rs), the MCP send path, and the internal emitter path currently short-circuit storage on Destination::Broadcast (and Telemetry tier) only, so a server-originated DataSeeded or QueueUpdated addressed to a Workspace destination would be persisted. Add a kind-based short-circuit so these kinds never reach store() regardless of destination — broadcast-channel delivery only (optionally reject a non-Broadcast destination for these broadcast-only kinds with 400, but the mandatory behavior is that no DataSeeded/QueueUpdated row is ever stored). (2) Dead spec delivery links: DataSeeded and QueueUpdated have no production emitter (repo-wide they appear only in the enum, tests, and one consumer match arm; scripts/dead-message-kind-exemptions.txt:20 records QueueUpdated as spec'd but never emitted). Add real server-originated emitters at the actual production events: DataSeeded (payload: none, Destination::Broadcast, workspace_id: None) when workspace data seeding completes (the admin workspace-seed flow in crates/gyre-server/src/api/admin.rs), and QueueUpdated (payload: none, Destination::Broadcast) when merge-queue state changes (enqueue/dequeue/merge-completion transitions in the merge-queue/merge-processor paths). Remove QueueUpdated from scripts/dead-message-kind-exemptions.txt once it has a production emitter (the check-dead-message-kinds.sh invariant requires every MessageKind to have an emitter; add no new exemption entries).",
        "acceptance": [
          "Storage exemption: a test sends a DataSeeded (and QueueUpdated) message through the send path — including forced to a Workspace destination — and asserts via list_by_workspace/find_by_id that no row was stored, while the broadcast channel delivered the message to subscribers.",
          "DataSeeded emitter: running the workspace data-seed flow results in a DataSeeded broadcast message observable by a connected/subscribed client (real event, server origin, workspace_id: None).",
          "QueueUpdated emitter: a merge-queue state transition (e.g. an MR entering or leaving the queue) results in a QueueUpdated broadcast message observable the same way.",
          "scripts/check-dead-message-kinds.sh passes with QueueUpdated removed from the exemptions file (no new exemptions added).",
          "Existing broadcast/destination validation behavior is unchanged for all other kinds."
        ],
        "negative_probe": "Persistence probe for the short-circuit: emit a DataSeeded message addressed to a workspace and query state.messages.list_by_workspace for that workspace; with the current destination-only guard the kind is persisted (a row exists), violating 'never hitting MessageRepository::store()' — the test must fail unless the kind-based short-circuit is in place. Emitter probe: run the seed flow with a broadcast-channel subscriber attached; today no DataSeeded message ever appears (the kind has zero production constructors), so an assertion that a DataSeeded broadcast arrives fails against the current code and only passes once the real emitter exists.",
        "dependencies": [],
        "existing_task": null
      },
      {
        "key": "ws-replay-window",
        "title": "WebSocket replay correctness: newest-1000 window and strictly-greater last_seen boundary",
        "covers": [
          "57b856585e62b254ad3ab3d4"
        ],
        "required_behavior": "Per message-bus.md Delivery/WebSocket: the server replays persisted Event-tier messages with created_at > last_seen, capped at 1000 messages total across all subscribed workspaces, and 'If more than 1000 exist, the server sends the newest 1000 and includes a {\"type\": \"ReplayCatchUp\", \"truncated\": true} message.' Two deviations in the Subscribe replay handler (crates/gyre-server/src/ws.rs:150-195) and the adapters: (1) Truncation window: the handler fetches the newest replay_limit+1 (1001) messages per workspace, reverses to oldest-first, and sends the FIRST 1000 it encounters — i.e. the OLDEST 1000 of the newest 1001, silently dropping the newest message. Fix so the delivered set is the NEWEST 1000 qualifying messages across all subscribed workspaces (delivered oldest-first), with ReplayCatchUp{truncated:true} emitted exactly when more than 1000 qualified. (2) Boundary comparison: the spec and the protocol.rs doc comment require strictly greater (created_at > last_seen), but both MemMessageRepository::list_by_workspace and the SQLite adapter filter created_at >= since, so a message created exactly at the client's last_seen timestamp is re-delivered on every reconnect. Make the replay path deliver only strictly-newer messages while preserving the documented REST workspace-query semantics of the `since` filter (implement the strict comparison in the adapters and keep both adapters in agreement — the mem-port-contracts invariant — or filter strictly at the replay call site; either way the observable replay contract is strictly-greater and both storage backends behave identically).",
        "acceptance": [
          "Newest-window probe: seed 1001 Event-tier messages with distinct created_at into a workspace, Subscribe with last_seen=0, and assert the single newest message IS among the 1000 replayed messages and that a ReplayCatchUp{truncated:true} arrives (current code drops the newest message).",
          "Truncation signal: with ≤1000 qualifying messages, no ReplayCatchUp is sent; with >1000, exactly one ReplayCatchUp with truncated:true is sent after the 1000 messages.",
          "Cross-workspace cap: with messages in two subscribed workspaces totaling >1000, the replay delivers the newest 1000 across BOTH workspaces combined (not per-workspace caps), oldest-first, plus the truncation signal.",
          "Strict boundary: a message with created_at exactly equal to the Subscribe last_seen value is NOT replayed; one created 1ms later IS. Verified against both the mem adapter and the SQLite adapter (identical behavior on both storage backends).",
          "Delivery ordering of the replayed window remains oldest-first, and Directed/Telemetry messages remain excluded from replay."
        ],
        "negative_probe": "Boundary probe: persist an Event-tier message, capture its created_at value T, reconnect and Subscribe with last_seen=T; the current >= filter re-delivers the message on every reconnect — the test asserts the replay stream is empty (or contains only strictly-newer messages) and therefore fails against the current mem and SQLite list_by_workspace implementations, passing only when the strictly-greater comparison is enforced on the replay path.",
        "dependencies": [],
        "existing_task": null
      },
      {
        "key": "directed-inbox-lifecycle",
        "title": "Directed inbox lifecycle: acknowledge_all on agent completion and on stale-agent Dead transition",
        "covers": [
          "ce88ec9cd0bc4a86136ae33b"
        ],
        "required_behavior": "Per message-bus.md Delivery Guarantees: Directed tier is 'At-least-once (persisted until ack, redelivered on poll)' with persistence 'DB — no TTL, expires only on ack or agent completion', and 'Agent completion cleanup: When an agent completes (POST /api/v1/agents/:id/complete), all unacked Directed messages in its inbox are marked with acknowledged: true and ack_reason: \"agent_completed\". They remain in the DB for audit but are no longer redelivered.' The DB-persistence half of this guarantee is delivered by the wiring task (dependency); this task delivers the cleanup lifecycle that is entirely missing — acknowledge_all has no production caller (its only server-crate call site is a test fixture in message_dispatcher.rs): (1) Agent completion: the REST complete handler (crates/gyre-server/src/api/spawn.rs complete_agent, ~line 2632) and the MCP agent-complete path must call state.messages.acknowledge_all(&agent_id, \"agent_completed\") when the agent completes, after which the unacked count is zero, poll with ?acknowledged=false returns nothing, and the messages remain stored (audit) with ack_reason recorded. (2) Orphaned agents (spec Edge Cases: 'For orphaned agents (killed without complete), the stale agent detector marks them Dead and calls acknowledge_all with ack_reason: \"agent_orphaned\", enabling the same cleanup'): stale_agents.rs currently transitions agents to Dead but never bulk-acks — add the acknowledge_all(&agent_id, \"agent_orphaned\") call when the detector marks an agent Dead, so expire_acked_inboxes (dead-inbox retention GYRE_DEAD_INBOX_TTL_SECS) can actually reclaim dead agents' inboxes instead of unacked Directed messages accumulating indefinitely. Existing expiry semantics are unchanged: expire_acked_inboxes only deletes agent-targeted messages acked with agent_completed/agent_orphaned after the retention window.",
        "acceptance": [
          "Completion cleanup: after POST /api/v1/agents/:id/complete, count_unacked for that agent is 0, GET /api/v1/agents/:id/messages?acknowledged=false returns no messages, and the previously unacked messages are still retrievable (audit retention) with ack_reason 'agent_completed'.",
          "MCP parity: the MCP agent-complete tool path performs the same bulk-ack.",
          "Orphan cleanup: when the stale detector marks a heartbeated-out agent Dead, its unacked Directed messages are bulk-acked with ack_reason 'agent_orphaned' (count_unacked 0, rows retained).",
          "Reclamation end-to-end: after either cleanup path, running the message-expiry job with the dead-inbox TTL elapsed deletes those acked inbox rows via expire_acked_inboxes (returns the deleted count), while unacked inboxes of live agents and explicit-ack messages inside the retention window are untouched.",
          "At-least-once across the full lifecycle (with the wiring dependency): an unacked Directed message to a live agent survives a server restart and is redelivered on poll; the same message is never redelivered after its agent completes or is marked Dead."
        ],
        "negative_probe": "Completion probe: send an unacked Directed message to an agent, call POST /api/v1/agents/:id/complete, then run the message-expiry job with the dead-inbox TTL elapsed; today acknowledge_all is never called by any production path (spawn.rs complete_agent and the MCP complete tool never invoke it, and stale_agents.rs transitions to Dead without bulk-acking), so count_unacked stays > 0 and expire_acked_inboxes deletes zero rows for that inbox — the test asserts both the zero unacked count after completion/orphaning and the eventual reclamation, and therefore fails against the current code, passing only when the completion and stale-detector acknowledge_all calls exist on the production paths.",
        "dependencies": [
          "db-message-repository-wiring"
        ],
        "existing_task": null
      },
      {
        "key": "telemetry-workspace-access",
        "title": "Workspace-scoped telemetry access: Subscribe membership authorization and the REST activity endpoint",
        "covers": [
          "2b439cf656cf14c344afdb07",
          "64d1be477f27ebf27ec73076"
        ],
        "required_behavior": "Two capabilities gating access to workspace telemetry: (1) Subscribe membership authorization (spec Telemetry Store: 'Subscription authorization: The server verifies workspace membership when processing Subscribe messages — a client cannot subscribe to a workspace it doesn't belong to. This prevents telemetry leakage across workspaces even though the buffer itself has no tenant isolation.'): the ws.rs Subscribe handler (crates/gyre-server/src/ws.rs:98-115) currently validates only that each requested workspace belongs to the caller's TENANT (plus an Admin bypass) — add real workspace-membership verification via WorkspaceMembershipRepository for user connections (agents subscribe to their own workspace per their agent record); a non-member same-tenant user's Subscribe for that workspace must be rejected (no subscription created, no delivery), while the existing tenant check and Admin bypass behavior are preserved. (2) REST telemetry endpoint (spec API/Telemetry: 'GET /api/v1/workspaces/:workspace_id/activity?since=<epoch_ms>&limit=100 — Queries the in-memory TelemetryBuffer. The since parameter uses epoch milliseconds. The handler verifies workspace membership before returning results — an unauthenticated or unauthorized caller receives 403. Response format preserves backwards compatibility with today's ActivityEventData'): the route does not exist today (only the unscoped notification-synthesized /api/v1/activity). Add GET /api/v1/workspaces/:workspace_id/activity backed by state.telemetry (TelemetryBuffer), with `since` in epoch milliseconds and `limit`, membership verification via WorkspaceMembershipRepository (403 for non-members, preserving the per-handler authorization pattern of the sibling workspace-scoped message routes), and the ActivityEventData field mapping from the spec table: event_id = message.id, agent_id = payload[\"agent_id\"] (or from when Agent(id)), event_type = message.kind mapped to its AgEventType string, description = payload[\"tool_name\"] or the kind name, timestamp = message.created_at. The new route MUST be registered in the ABAC RouteResourceMapping in abac_middleware.rs (check-abac-route-registry.sh invariant — no new exemption-file entries), and the existing /api/v1/activity and WS ActivityQuery paths are unchanged.",
        "acceptance": [
          "Subscribe authorization: a user authenticated in tenant T with no membership record for workspace W (which belongs to T) has their Subscribe for W rejected — no workspace-scoped bus messages or telemetry for W are delivered to that connection — while a member of W in the same tenant receives them; agent connections still receive their own workspace's stream.",
          "REST endpoint, happy path: a member of W GETs /api/v1/workspaces/W/activity?since=<ms> and receives telemetry records shaped as ActivityEventData with the exact spec field mapping (event_id from message id, agent_id from payload or origin, event_type from the kind, description from tool_name or kind name, timestamp = created_at in epoch ms).",
          "REST endpoint, boundary: telemetry entries with created_at <= since are excluded; entries 1ms newer are included; limit is honored.",
          "REST endpoint, enforcement: a same-tenant non-member caller receives 403 and no telemetry rows; the route has an ABAC RouteResourceMapping entry and scripts/check-abac-route-registry.sh passes with no new exemptions.",
          "The unscoped /api/v1/activity route and the WebSocket ActivityQuery path continue to work unchanged."
        ],
        "negative_probe": "Subscribe leak probe: authenticate a user in tenant T, create workspace W in T without granting the user membership, connect the WebSocket and Subscribe to W, then emit telemetry and a workspace-scoped bus message to W; the current handler accepts the subscription on tenant match alone (ws.rs:107-108) and delivers both — the test asserts the connection receives NOTHING for W and therefore fails against the current code, passing only when the WorkspaceMembershipRepository check gates Subscribe. Endpoint probe: the same non-member GETs /api/v1/workspaces/W/activity — today the route does not exist (404 from the router), and after implementation a missing membership check would return 200 with rows; the test asserts 403, failing if the handler serves the buffer without verifying membership.",
        "dependencies": [],
        "existing_task": null
      },
      {
        "key": "mcp-sse-workspace-agui",
        "title": "MCP SSE stream: workspace filtering and MessageKind-to-AG-UI event mapping",
        "covers": [
          "103aac23ed1cdc9d4788f31d"
        ],
        "required_behavior": "Per message-bus.md MCP Integration: 'The MCP SSE endpoint (GET /mcp/sse) subscribes to the message bus filtered by the authenticated agent's workspace. It maps MessageKind variants to AG-UI event types for protocol compatibility.' The current handler (crates/gyre-server/src/mcp.rs:5064-5083) subscribes to the unfiltered global message_broadcast_tx and emits every message as a generic SSE 'message' event carrying the raw envelope JSON — every connected MCP client receives every tenant's and workspace's bus messages, and the specced event names never appear. Fix mcp_sse_handler to: (1) resolve the authenticated agent's workspace (from the verified JWT hierarchy claims / agent record) and deliver only bus messages scoped to that workspace (Destination::Workspace(ws) where ws matches, Broadcast per the bus's broadcast semantics, and Destination::Agent(id) only when id is the authenticated agent) — a client in workspace A must not receive workspace B's or another tenant's messages; (2) map MessageKind to AG-UI SSE event names per the spec table — ToolCallStart→TOOL_CALL_START, ToolCallEnd→TOOL_CALL_END, RunStarted→RUN_STARTED, RunFinished→RUN_FINISHED, TextMessageContent→TEXT_MESSAGE_CONTENT, StateChanged→STATE_CHANGED, AgentError→ERROR — emitting each as an SSE event with the AG-UI event name (data payload per the existing envelope format); kinds outside the table keep a sensible default event name. Existing server→client notifications already carried on this SSE stream must continue to be delivered. The MCP message tools (gyre_message_send/poll/ack) and gyre_record_activity are already conformant and are out of scope.",
        "acceptance": [
          "Workspace isolation: two MCP SSE connections authenticated as agents in different workspaces; a Workspace-scoped message emitted to workspace A is received by A's connection only — B's connection receives nothing for it (today both receive everything).",
          "AG-UI mapping: a Telemetry-tier ToolCallStart message for the subscriber's workspace arrives as an SSE event named TOOL_CALL_START (not the generic 'message'), and likewise for the other six kinds in the spec table; AgentError arrives as ERROR.",
          "Directed delivery: a Directed message to the authenticated agent is delivered on its SSE stream; Directed messages to other agents are not.",
          "Existing SSE notification events (server→client notifications) still arrive on the stream after the change.",
          "gyre_record_activity-produced telemetry surfaces on the SSE stream of clients in the same workspace with the mapped AG-UI event name."
        ],
        "negative_probe": "Isolation probe: open an MCP SSE stream authenticated as an agent in workspace B, then emit a Workspace-scoped Event/Telemetry message to workspace A (different workspace, or a different tenant's workspace); the current handler forwards every broadcast message to every subscriber with no filtering (mcp.rs:5071-5077), so B's stream receives A's message — the test asserts B's stream stays silent for A's message (only keep-alives) and therefore fails against the current code, passing only with the workspace filter. Mapping probe: push a ToolCallStart telemetry message to the subscriber's workspace and inspect the SSE event name field; today it is always 'message' (mcp.rs:5077), so an assertion that the event name is TOOL_CALL_START fails until the kind→AG-UI mapping exists.",
        "dependencies": [],
        "existing_task": null
      },
      {
        "key": "message-size-and-key-rotation",
        "title": "Message edge cases: 64KB send limit with 413 and JWKS historical-key serving for signature rotation",
        "covers": [
          "e5d4cc99cba7ab16b2b40bb5"
        ],
        "required_behavior": "The remaining unimplemented rows of the message-bus.md Edge Cases table (the server-restart row is delivered by the wiring dependency; the send-to-dead/completed-agent row's cleanup lifecycle is delivered by the inbox-lifecycle dependency; the already-implemented rows — send-to-self, 100-agent fan-out, 429 queue depth, atomic concurrent sends, idempotent concurrent ack — must remain passing): (1) Message too large: 'Total POST request body limited to 64KB (configurable via GYRE_MAX_MESSAGE_SIZE). This covers the full JSON envelope including to, kind, and payload. Server rejects with 413.' Today only the global GYRE_MAX_BODY_SIZE (default 10MB) applies, so a message payload can be ~10MB. Apply a message-specific body limit on the send endpoint (POST /api/v1/workspaces/:workspace_id/messages) — e.g. an axum DefaultBodyLimit route layer — reading GYRE_MAX_MESSAGE_SIZE with the specced 64KB default, returning 413 when the request body exceeds it; the limit covers the entire request body, not just the payload field. (2) Signature key rotation: 'Messages signed with old key remain verifiable — JWKS endpoint serves historical keys. New messages use new key.' Today the JWKS document (auth.rs:379) is built from the single current key pair, so after any key change old messages' key_id values no longer resolve. Implement key-history retention: when the signing key changes, the previous public key(s) remain resolvable — persist historical public keys (kid + public key material) in durable storage alongside the current key, and serve them in the JWKS keys array at GET /.well-known/jwks.json; verification that fetches by key_id must still resolve old kids and successfully verify messages signed before the rotation, while new messages are signed with the new key.",
        "acceptance": [
          "Size limit: POST /api/v1/workspaces/:id/messages with a total request body of 65KB (payload padding) is rejected with 413 and nothing is stored or broadcast; a body at/under the 64KB limit is accepted; GYRE_MAX_MESSAGE_SIZE overrides the default in tests.",
          "Key rotation verifiability: sign a Directed/Event message under key K1, rotate the server signing key to K2, then (a) GET /.well-known/jwks.json returns a keys array containing BOTH K1's and K2's kids, and (b) reconstructing the sign input and verifying the K1-signed message's Ed25519 signature against K1's served public key succeeds (real crypto verify, not a structural check).",
          "New messages after rotation are signed with K2 and verify against K2's entry.",
          "Server-restart row (via the wiring dependency): persisted Directed and Event messages survive a restart; the telemetry buffer is lost (ephemeral, acceptable); WS clients reconnect and re-subscribe with last_seen.",
          "Dead/completed-agent row (via the inbox-lifecycle dependency): messages to completed agents are stored and reclaimed by the dead-inbox expiry after the acknowledge_all lifecycle runs.",
          "All previously-implemented edge-case rows keep passing: send-to-self allowed, workspace fan-out one store + one broadcast, 429 at GYRE_AGENT_INBOX_MAX with no silent drops, atomic concurrent sends, idempotent concurrent ack (200)."
        ],
        "negative_probe": "Size probe: POST a message whose total JSON body is 65KB; today the request passes the global 10MB limit and returns 201 with a ~65KB persisted envelope — the test asserts 413 and no stored row, failing against the current code and passing only when the GYRE_MAX_MESSAGE_SIZE limit (64KB default) guards the send endpoint. Rotation probe: sign a message with the current key, rotate the key, then fetch /.well-known/jwks.json and attempt verification of the old message via its key_id; today the JWKS document contains only the single current key (auth.rs:379-393), so the old kid is absent and verification cannot succeed — the test asserts the old kid is present and the Ed25519 verify() call returns Ok, failing unless historical keys are retained and served.",
        "dependencies": [
          "db-message-repository-wiring",
          "directed-inbox-lifecycle"
        ],
        "existing_task": null
      }
    ]
  },
  "assigned_names": {
    "db-message-repository-wiring": "task-247",
    "message-send-path-guards": "task-248",
    "broadcast-only-kind-emitters": "task-249",
    "ws-replay-window": "task-250",
    "directed-inbox-lifecycle": "task-251",
    "telemetry-workspace-access": "task-252",
    "mcp-sse-workspace-agui": "task-253",
    "message-size-and-key-rotation": "task-254",
    "_publication": "task-255",
    "_base": "8c63e3673119a7d41c6089d3d031c84c9f4e92a5"
  }
}

## Acceptance Criteria

- [ ] Every planned task faithfully implements its assigned canonical requirements.
- [ ] No duplicate active assignment or missing dependency is introduced.
- [ ] No product behavior, spec requirement, coverage status or enforcement is changed.
- [ ] New tasks remain not-started until this backlog publication merges.

## Shipped

Task definitions and explicit bindings only; no product implementation claimed.
