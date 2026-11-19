# Review — task-248 (Message send-path guards: scope validation, mem store contract, WS acknowledged exclusion, auth-mechanism origin table, Agent-destination tenant isolation)

Spec: `specs/system/message-bus.md` §Message Envelope (send_message() scopeless guard, store()-Err-on-None, `acknowledged` conditional serialization) and §Scoping Rules rule 1 (Agent-destination tenant isolation), plus the normative "Origin and tenant resolution from auth context" table.
Candidate under review: `cc17a6a886e3bd0ac550840773c1ba310002e8fd` (branch `pipeline/task-248/dd5c391292444c778755f269e6647df3-1`), base `61b6080d1866f32d98cc7a35d62e3fb2b88a290a`. Product-surface commits per `commits:` frontmatter: `955c134a`, `9dd4a1f0`, `873d0821` (round-4 fail-closed repair), `3bb9c9bd` (round-5 probe-kind repair), `a2781ff0` (round-7 clippy repair); merge repair `8a92d51b` reconciled with main (task-251) preserving every guard file.
Verdict: **complete** (round 8; all five required behaviors implemented with discriminating tests; prior-round corrections verified present).

## What was verified (independent, this round)

### 1. Scopeless-message guard — real
`emit_scoped_event` (crates/gyre-server/src/lib.rs), the shared body of `emit_event`/`emit_tenant_event`, returns `Err` and logs at `error` level (`tracing::error!` before any construction/sign/store/broadcast) when `destination != Broadcast && workspace_id.is_none()`. `emit_event`/`emit_tenant_event` wrap it best-effort (warn + drop), Broadcast is the specced carve-out. The sole `emit_tenant_event` production caller (reconciliation.rs:1509) still passes `Destination::Broadcast` — no regression. Test `emit_scoped_event_rejects_scopeless_non_broadcast_message` drives the real emitter through a counting `MessageRepository` wrapper (`test_state_with_messages`) and asserts: Err for Workspace and Agent destinations with `workspace_id: None`, an ERROR-level captured log record, `store()` count 0, empty broadcast AND dispatch channels, plus positive controls (Broadcast passes and is broadcast-not-stored; scoped Workspace message stores exactly once). The test fails if the guard is removed (a scopeless Workspace message would be persisted → counting store = 1).

### 2. Mem store contract — real
`MemMessageRepository::store` (crates/gyre-server/src/mem.rs) returns `Err` on `workspace_id: None` before inserting, matching the SQLite/PostgreSQL `message_to_new_row` contract; `mem_store_rejects_scopeless_message` mirrors the SQLite adapter's contract test (Err on Broadcast and on scopeless non-Broadcast, `find_by_id` confirms nothing inserted, scoped happy path still stores). No send path passes Broadcast to `store()`: REST `send_message` skips persistence for Broadcast (verified in handler), the MCP path likewise, and the internal emitter persists non-Broadcast only.

### 3. WS `acknowledged` exclusion — real, both arms
Exactly two sites serialize a bus `Message` over the WebSocket (verified by grep: the only other `serde_json::to_string` in ws.rs serializes a `WsMessage::UserPresence`, not a bus Message) — the live bus-delivery arm and the Subscribe replay arm — and both now go through `ws_message_payload`, which strips `acknowledged` with the same manual-removal pattern as `send_response`. GET inbox keeps the field (`get_inbox_still_includes_acknowledged` asserts a boolean `acknowledged` key). No frontend consumer reads `acknowledged` from WS frames (grep of web/src). Tests: `ws_live_delivery_strips_acknowledged` (real TCP server, real auth, real bus emission, asserts the delivered JSON lacks the key) and `ws_subscribe_replay_strips_acknowledged` (round-3 correction: Subscribe with last_seen=0 against a persisted Event-tier message, replayed JSON lacks the key).

### 4. Origin table — real, mechanism-derived
`AuthenticatedAgent` gains `auth_mechanism: AuthMechanism`, set by the extractor at token-validation time on every production path (global token ×3 sites, agent token ×2, API key ×2, OIDC JWT, federated JWT, impersonation; the scheduler's system principal is `GlobalToken`). `origin_from_auth` (api/messages.rs) now maps: GlobalToken → Server; ApiKey/UserJwt/Impersonation → User(user_id) (defensive Agent fallback when user_id is absent — never Server); AgentToken/FederatedJwt → Agent(sub). No role membership or identity strings involved. Consequences verified in the handler and pinned by tests: Admin-role API-key and Admin-role OIDC callers get User origin, so the `server_only()` 403 gate rejects them (`admin_api_key_cannot_forge_server_only_kind` — the origin-forging probe, real API-key extractor path; `admin_oidc_user_cannot_forge_server_only_kind` — real signed JWT with Admin realm role) and the rule-3 membership check applies to them (`admin_api_key_directed_send_requires_workspace_membership`: 403 "not a member" without membership, 201 with). Rule 6 preserved independently: the Broadcast guard keeps its separate Admin-role check (`admin_api_key_broadcast_allowed_non_admin_rejected`: Admin API key Broadcast → 201, non-Admin member → 403 from the Broadcast guard; probe kind is a non-server_only Custom kind with a documented reason — the server_only gate precedes the Broadcast guard). Global token keeps full forge authority (`global_token_keeps_server_origin_and_broadcast_authority`: server_only kind → 201, Broadcast → 201, Broadcast never stored). With the old role-based Admin→Server mapping restored, the two forge tests would get 201 and fail — the tests are genuinely anchored to the mechanism mapping.

### 5. Rule-1 tenant isolation — real, fail-closed, both paths
REST `send_message` (api/messages.rs): on every `Destination::Agent(id)` send, after the target-workspace match, loads the target agent's workspace record and compares `target_ws.tenant_id` with `auth.tenant_id` (the caller's verified tenant — global token resolves to `system_principal_tenant()`, API-key to the user's stored binding, fail-closed when absent); mismatch → 403 with RBAC-denied record, nothing stored or broadcast — including Server-origin callers (`cross_tenant_directed_send_rejected_for_server_origin`: tenant-"default" global token → agent in a tenant-2 workspace → 403, empty inbox, empty broadcast channel; same-tenant control → 201 + persisted + broadcast). Unresolvable workspace record (dangling agent after non-cascading workspace delete) → 403 fail-closed, never skip-and-log (`unresolvable_target_workspace_rejects_send`) — the round-4 repair. Mirrored in MCP `handle_message_send` (mcp.rs) with the same comparison and fail-closed handling (`mcp_message_send_cross_tenant_rejected` with a same-tenant control, `mcp_message_send_unresolvable_target_workspace_rejected`). With the tenant comparison removed, both cross-tenant tests get success and fail.

## Probes run (this checkout, evidence in the round's review-evidence dir)

- `/tmp/stage/bin/cargo check -p gyre-server --tests` → **exit 0** (9m11s; full unit/integration test surface type-checked, including every new test).
- `/tmp/stage/bin/cargo fmt -p gyre-server -- --check` → **exit 0**.
- `bash scripts/check-arch.sh` → OK.
- `bash scripts/check-mem-port-contracts.sh` → OK (the new mem store guard is the invariant this task closes).
- `bash scripts/check-task-commit-attribution.sh` → fails ONLY because the review-stage controller reset the working-tree `specs/tasks/task-248.md` to the pristine assignment (`commits: []`); the candidate commit `cc17a6a8` itself lists all five product-surface SHAs in its frontmatter (verified via `git show HEAD:specs/tasks/task-248.md`). Not a candidate defect.
- gyre-server test-binary compilation and execution deferred to the independent verifier per the 4 GiB budget contract.

## Tests the verifier must execute

- `cargo test -p gyre-server --lib api::messages` — origin-table + rule-1/3/6 suite (9 new tests incl. all forge/isolation probes).
- `cargo test -p gyre-server --lib emit_scoped_event_rejects_scopeless_non_broadcast_message`.
- `cargo test -p gyre-server --lib mem_store_rejects_scopeless_message`.
- `cargo test -p gyre-server --lib ws::` — incl. `ws_live_delivery_strips_acknowledged`, `ws_subscribe_replay_strips_acknowledged` (TCP listener).
- `cargo test -p gyre-server --lib mcp_message_send` — incl. `mcp_message_send_cross_tenant_rejected`, `mcp_message_send_unresolvable_target_workspace_rejected`.
- `cargo test -p gyre-server --test api_integration agent_messages_send_and_receive` (updated to seed the default workspace row the rule-1 resolution requires).
- Standard workspace suites (`cargo test --all`, web `npm test`) per verifier policy.

## Coverage matrix

Rows 5 (Message Envelope), 9 (Delivery), 10 (Scoping Rules) updated `verified`→`implemented` with task-248 evidence and concrete source/test citations — accurate and conservative (implemented, pending independent verification). No unrelated rows touched; the union resolution of the `Last-audited` header retains the task-250/251 notes from main.

Findings:

- (none)
