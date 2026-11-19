# Review — task-248 (Message send-path guards: scope validation, mem store contract, WS acknowledged exclusion, auth-mechanism origin table, Agent-destination tenant isolation)

Spec: `specs/system/message-bus.md` §Message Envelope (`send_message()` scopeless guard, `store()` Err-on-None, `acknowledged` conditional serialization) and §Scoping Rules (rule 1 Agent-destination tenant isolation, rule 3/6 interplay with the §Origin and tenant resolution from auth context table).
Candidate under review: `0ba5407b33b39c0601386005037f724a73fb46ca` (base `bd6110839d3639df7436ec10960e27518829c703`), commits `955c134a` (implementation checkpoint) + `9dd4a1f0` (test-compilation repair + fmt), both listed in the task's `commits:` frontmatter.
Verdict: **needs revision** — one blocking finding (F1). Full probe log: `/tmp/stage/review-evidence/task-248-review-evidence.md`.

## What was verified as genuinely implemented

- **Origin table (auth mechanism, not roles).** `AuthenticatedAgent` gains an `auth_mechanism` enum set by the auth extractor at token-validation time on every path (global token → `GlobalToken`; per-agent UUID/JWT token → `AgentToken`; API key → `ApiKey`; Keycloak/OIDC JWT → `UserJwt`; federation JWT → `FederatedJwt`; impersonation → `Impersonation`). `origin_from_auth` (api/messages.rs) now derives origin from that mechanism: only the global `GYRE_AUTH_TOKEN` resolves to `MessageOrigin::Server`; API-key/OIDC/impersonation callers resolve to `User(user_id)` even when they hold Admin; agent tokens resolve to `Agent(sub)`. The `server_only()` 403 gate and the rule-3 User-origin membership check therefore now apply to Admin-role API-key/OIDC callers, while the Broadcast guard's separate Admin-role check (rule 6) is preserved, and the global token keeps full forge authority. No spoofable string or role discriminator remains on the origin path.
- **Scopeless-message guard.** `emit_scoped_event` (lib.rs), the shared body of the internal emitter family, returns `Err` and logs at `error` level when `destination != Broadcast && workspace_id.is_none()`, before the Message is constructed, stored, broadcast, or dispatched. `emit_event`/`emit_tenant_event` propagate the Err (warn on drop). Every other internal construction site always carries `Some(workspace_id)` for non-Broadcast destinations, so the guard covers the only path that could produce a scopeless message.
- **Mem store contract.** `MemMessageRepository::store` returns `Err` on `workspace_id: None` (mirroring SQLite/PostgreSQL `message_to_new_row`), with a contract test mirroring the SQLite adapter's. All three send paths skip `store()` for Broadcast.
- **WS `acknowledged` exclusion.** Both bus-Message WebSocket serialization sites (live bus-delivery arm and Subscribe replay arm, ws.rs) route through a `ws_message_payload` helper that strips `acknowledged` with the same manual-removal pattern as `send_response`; grep confirms no other WS site serializes a bus Message. GET inbox responses keep the field.
- **Rule-1 tenant isolation (resolvable case).** Both the REST handler and the mirrored MCP `handle_message_send` compare the target workspace's tenant with the caller's verified tenant on every Agent-directed send, for all origins including Server-origin global-token callers, rejecting 403 before store/broadcast.

Probes run in this review: `cargo check -p gyre-server --tests` → exit 0 (validates the implementer's compile-repair claim and all twelve new tests); `cargo fmt -p gyre-server --check` → clean; `check-mem-port-contracts`, `check-inert-enforcement`, `check-fabricated-scope-defaults`, `check-lossy-secret-conversion`, `check-abac-route-registry`, `check-dead-message-kinds` → all pass; no exemption files touched. The executor's wrapper blocks gyre-server test-binary codegen during review (4 GiB budget), so the twelve tests were **not executed** — neither by the implementer (their Shipped note records the same deferral) nor here; the verifier must run them (command list in the evidence file). Test soundness was verified by reading: each new test asserts the specific rejection message/persistence emptiness such that reverting the corresponding enforcement (role-based → Server mapping, tenant comparison, WS strip, store guard) makes it fail.

Coverage rows 5, 9, 10 were updated to `implemented` with concrete source evidence — accurate and appropriately not marked `verified`.

## Findings

### F1 (blocking): rule-1 tenant isolation fails open when the target agent's workspace record cannot be resolved

Both the REST handler (api/messages.rs, `send_message`) and the MCP path (`handle_message_send`) resolve the target agent's tenant via `workspaces.find_by_id(&target_agent.workspace_id)` and, on `Ok(None)` **or** `Err`, log a warning and continue the send ("tenant isolation check skipped"). Reproduction without any code change:

1. Tenant T2's workspace W contains agent A.
2. `DELETE /api/v1/workspaces/W` does not cascade agents (`MemWorkspaceRepository::delete` removes only the workspace record), leaving A orphaned with `workspace_id = W`.
3. A global-token caller (tenant "default") POSTs `/api/v1/workspaces/W/messages` with `{"to": {"agent": "A"}}`: the workspace-match precondition passes (A.workspace_id == W), the tenant lookup hits `Ok(None)` → warn → skip, Server origin has no membership check, and the message is stored with tenant "default" and delivered to A's inbox (`list_unacked` filters by agent_id only).

This is a cross-tenant Directed leak reachable through a supported admin operation, produced by the new guard silently disabling itself precisely in the inconsistent-state scenario rule 1 exists to defend against ("must match ... regardless of origin (including Server) ... prevents a server bug from leaking messages across tenants"). It is the fail-open ref-resolution class the codebase's own invariants prohibit: the established unresolvable-scope pattern skips the *operation* (e.g. `emit_event` drops the emission; the adjacent ws.rs Subscribe arm rejects workspaces on `Ok(None)`), whereas here only the check is skipped and the send proceeds. The in-code comment cites that pattern but does not follow it.

Fix direction: reject the send (403/500) when the target's tenant cannot be resolved, and update the pre-existing tests that rely on agents without workspace records (e.g. `queue_depth_enforcement_429` seeds `agent-full` in the nonexistent `ws-full`) to create the workspace — those tests are why the branch currently fails open. Neither skip branch is covered by any test.

## Verifier must run (deferred from this stage)

The twelve focused tests listed in `/tmp/stage/review-evidence/task-248-review-evidence.md` (emit guard, mem contract, both WS arms, inbox-keeps-acknowledged, the five origin-table/rule-6 tests, cross-tenant REST + MCP), plus the full workspace suite and scripts. They have never been executed in this pipeline.
