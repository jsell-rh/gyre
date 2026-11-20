---
title: "Message send-path guards: scope validation, mem store contract, WS acknowledged exclusion, auth-mechanism origin table, Agent-destination tenant isolation"
spec_ref: "Spec gap spec-c62aac6701125cb09e0c736c / message-send-path-guards / 60b177f85a11"
depends_on: []
progress: complete
commits: ["8a299761faad97ef855e3a61031b57a3e15d5125", "001f585ac686c90f601fd210ca4a40cc5cad7c9d", "951fa88b25aa0c530aab4dc4c4e2c59cbe6994f2", "a2781ff06b71bc443eda82aa1c0022edf011c972", "3bb9c9bd4099810d8583ab647018a5a41297f8c2", "873d0821ca7a33501c178303bdc618a1802ccbb6", "9dd4a1f0f07930760dddf8036a9fede6bbafb44c", "955c134a8c669070c3b936eac55f0e5e3f448418"]
---

## Required behavior

Five deviations from the Message Envelope and Scoping Rules sections, all on the message construction/send path: (1) Scopeless-message guard (spec: 'The send_message() helper function validates: if destination != Broadcast && workspace_id.is_none(), it returns Err and logs at error level'): add this validation to the shared internal construction/emitter path (the emit_scoped_event/emit_tenant_event family in crates/gyre-server/src/lib.rs) so no server-internal emitter can produce a non-Broadcast message with workspace_id: None — it must return Err and log at error level, never construct, store, or broadcast the message. (2) Mem adapter port contract (spec: "The adapter's store() method also returns Err on workspace_id: None — never panics"): MemMessageRepository::store (crates/gyre-server/src/mem.rs:4840) currently inserts any message silently; make it return Err on workspace_id: None, matching the SQLite/PostgreSQL adapters (message_to_new_row) — the mem adapter must enforce the same port failure constraint in code (check-mem-port-contracts invariant). (3) acknowledged exclusion from WebSocket pushes (spec field contract: 'Excluded from send (POST) responses and WebSocket pushes via conditional serialization — the field IS included in GET inbox responses'): exactly two sites serialize a full bus Message over the WebSocket — the live bus delivery arm (ws.rs:459, serde_json::to_string(&msg)) and the Subscribe replay arm (ws.rs:173, serde_json::to_string(&m)). Strip `acknowledged` at BOTH sites using the same manual-removal pattern as send_response in api/messages.rs; do not change GET inbox responses, which must keep the field. (4) Origin resolution per the normative 'Origin and tenant resolution from auth context' table: origin_from_auth (crates/gyre-server/src/api/messages.rs:46) currently maps ANY Admin-role caller to MessageOrigin::Server. Fix it to derive origin from the actual auth mechanism — global GYRE_AUTH_TOKEN → Server; API key (gyre_*) → User(user_id from key lookup); Keycloak/OIDC JWT → User(user_id from token); agent JWT → Agent(sub). The mechanism discriminator must come from the authentication extractor (thread a reliable mechanism/global-token signal through AuthenticatedAgent), not from spoofable strings like agent_id == "system" or role membership. Preserve scoping rule 6 independently: the Broadcast guard's separate Admin-role check must still allow Admin-role API-key callers to target Broadcast, and the global token keeps Server origin (full forge authority). Consequence that must hold after the fix: Admin-role API-key/OIDC callers resolve to User origin, so the server_only() 403 gate applies to them (they can no longer forge built-in Event kinds or GateResult) and the rule-3 User-origin workspace-membership check on Directed sends applies to them. (5) Scoping rule 1 tenant isolation (spec: 'All Destination::Agent(id) messages validate tenant isolation. The target agent's tenant_id must match the message's tenant_id, regardless of origin (including Server)'): in the REST send handler (api/messages.rs) and the mirrored MCP send path (mcp.rs handle_message_send), when the destination is Agent(id), load the target agent record and compare its tenant_id with the message's tenant_id (the caller's verified tenant); on mismatch reject with 403 — including for Server-origin (global-token) callers, which is exactly the server-bug leak class rule 1 exists to prevent.

## Acceptance Criteria

- [ ] Scopeless guard: a test drives the internal emitter (or the construction helper it shares) with destination Workspace/Agent and workspace_id None and asserts an Err return plus an error-level log record, and that no message reaches store() or the broadcast channel.
- [ ] Mem contract: MemMessageRepository::store on a message with workspace_id None returns Err (test in mem.rs mirroring the SQLite adapter's contract test), while Broadcast messages are never passed to store() by any send path.
- [ ] WS acknowledged exclusion, live arm: a bus-delivered Message received on a subscribed WebSocket connection has no `acknowledged` key in the JSON (ws.rs:459 arm).
- [ ] WS acknowledged exclusion, replay arm (review round-3 correction): connect, Subscribe with last_seen=0 against a workspace with a persisted Event-tier message, and assert the replayed JSON also lacks the `acknowledged` key — both WebSocket Message serialization sites (ws.rs:459 live delivery and ws.rs:173 replay) are covered, not just the live arm.
- [ ] GET /api/v1/agents/:id/messages (inbox) responses still INCLUDE `acknowledged` (crash-recovery contract) after the change.
- [ ] Origin table: an Admin-role API-key caller and an Admin-role OIDC caller each resolve to User(<user_id>) origin — verified by sending a server_only kind (e.g. agent_created), which must now be rejected 403, and by a Directed send to an agent in a workspace the user is NOT a member of, which must now be rejected 403 via the rule-3 membership check.
- [ ] Origin table, global token: the global GYRE_AUTH_TOKEN still resolves to Server origin and can send server_only kinds and target Broadcast (rule 6 forge authority preserved).
- [ ] Rule 6 preserved for Admin API keys: an Admin-role API-key caller can still send a Broadcast message (separate role check), while a non-Admin API-key caller cannot (403).
- [ ] Tenant isolation rule 1: a global-token (Server-origin) caller whose tenant is T1 sending a Directed message to an agent in tenant T2's workspace is rejected with 403 and nothing is stored or broadcast; same-tenant sends still succeed. The mirrored MCP send path enforces the same comparison.

## Verification intent

Two probes against the current defects: (a) origin forging — authenticate with an Admin-role API key and POST a message with kind "agent_created" (server_only); the current code maps the caller to MessageOrigin::Server (api/messages.rs:46-52) and returns 201, letting a non-global-token caller forge server events; after the fix it must return 403, and the test must fail if the role-based → Server mapping returns. (b) cross-tenant Directed leak — with the global token (default tenant T1), POST to a workspace in tenant T2 targeting an agent in that workspace; the current handler stores the message with tenant_id=T1 pointed at a T2 agent with no tenant comparison anywhere in the send path; after the fix the request must fail 403 and the messages store must contain no row, and the test must fail if the tenant comparison is removed.

Update the relevant coverage rows only after implementing the behavior, with concrete source evidence. Do not mark unrelated rows complete or claim independent verification before it occurs.

## Discovery evidence

Canonical spec: `specs/system/message-bus.md`. Source: `4127964001113dda08e4852404a68ba560ceb764`; plan: `151021041bfb974bd08554e6182cff451df6efb83f37d8a11f6dfdcc1950d05e`.

[
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
  }
]

## Shipped

All five send-path guards are implemented on this branch (955c134a,
9dd4a1f0, 873d0821, 3bb9c9bd, a2781ff0, 951fa88b, 001f585a), carried
through the merge with base 00669baa, plus this round's compile repair
(8a299761).

### Behavior

1. **Scopeless-message guard** — `emit_scoped_event`
   (crates/gyre-server/src/lib.rs), the shared body of the internal
   emitter family, returns `Err` and logs at error level when
   `destination != Broadcast && workspace_id.is_none()`; the message is
   never constructed, stored, or broadcast.
2. **Mem store contract** — `MemMessageRepository::store`
   (crates/gyre-server/src/mem.rs) returns `Err` on `workspace_id: None`,
   matching the SQLite/PostgreSQL `message_to_new_row` contract; send
   paths skip `store()` for Broadcast destinations.
3. **WS `acknowledged` exclusion** — both WebSocket Message
   serialization sites (live bus-delivery arm and Subscribe replay arm
   in crates/gyre-server/src/ws.rs) serialize through
   `ws_message_payload`, which strips `acknowledged` with the same
   manual-removal pattern as `send_response`; GET inbox responses keep
   the field.
4. **Origin table** — `origin_from_auth` (crates/gyre-server/src/api/messages.rs)
   derives origin from `AuthenticatedAgent.auth_mechanism`, set by the
   extractor on every auth path: global `GYRE_AUTH_TOKEN` → Server;
   API key / OIDC user JWT → `User(user_id)`; agent token / federated
   JWT → `Agent(sub)`. Admin-role API-key/OIDC callers resolve to User
   origin (server_only 403 gate and rule-3 membership check apply to
   them); the separate rule-6 Admin-role check still lets them target
   Broadcast; the global token keeps full forge authority.
5. **Rule-1 tenant isolation, fail-closed** — the REST send handler and
   the mirrored MCP `handle_message_send` resolve the target agent's
   workspace record on every `Destination::Agent(id)` send and compare
   its tenant with the caller's verified tenant; mismatch rejects 403
   (nothing stored or broadcast), including Server-origin global-token
   callers. An unresolvable workspace record rejects fail-closed.

### This round — merged-base compile repair

CI run 44680564932 (PR head 1bcfb6dd) failed to compile the
gyre-server lib test: E0063, missing field `auth_mechanism` in an
`AuthenticatedAgent` initializer at crates/gyre-server/src/api/spawn.rs.
The initializer is `plain_auth`, a hand-rolled test fixture that came
in with merged base 00669baa and predated the field task-248 added to
the extractor. Commit 8a299761 sets it to
`AuthMechanism::AgentToken`, matching the sibling spawner fixtures in
the same test module (the same repair class as 001f585a in the prior
round). No task-248 guard code changed this round; the field is
consulted only by `origin_from_auth`, which this fixture's sole
consumer (`complete_agent` direct drive) does not exercise.

### Test evidence

- In-sandbox probes this round (build-budget limit — large gyre-server
  test compilation deferred to the verifier): `SKIP_WEB_BUILD=1 cargo
  check -p gyre-server --tests` — clean, exit 0 (was the E0063
  failure); `cargo fmt --check` — clean, exit 0;
  `bash scripts/check-task-commit-attribution.sh` — full-history scan,
  result recorded in review evidence.
- Prior-round probes, unchanged by this round's one-line fixture
  repair: origin-table probes (Admin API-key/OIDC server_only 403,
  directed-send membership 403, global-token Server origin +
  Broadcast authority, rule-6 Admin-Broadcast allowance), rule-1
  cross-tenant 403 with empty store (REST + MCP), WS live/replay
  `acknowledged` exclusion, GET inbox keeps `acknowledged`, mem-store
  Err-on-None contract test, scopeless-guard Err + error-level log
  with empty store/broadcast.
- Deferred to the independent verifier: `cargo test -p gyre-server`
  (the task-248 guard tests in api/messages.rs, mcp.rs, ws.rs, mem.rs,
  and lib.rs, plus the spawn.rs fixture suite that includes
  `plain_auth`).
