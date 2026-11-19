# Review — task-248 (Message send-path guards: scope validation, mem store contract, WS acknowledged exclusion, auth-mechanism origin table, Agent-destination tenant isolation)

Spec: `specs/system/message-bus.md` §Message Envelope (send_message() scopeless
guard, store() Err-on-None, `acknowledged` conditional serialization, "Origin
and tenant resolution from auth context" table) and §Scoping Rules rule 1.
Candidate: `ffe47ec6` on `pipeline/task-248/5197cdefaa9f43febfbff91686e0df7e-1`,
base `bd611083`, commits `955c134a`, `9dd4a1f0`, `873d0821` (scoped per the
task's `commits:` frontmatter).

## Round 5 verdict: needs revision — one blocking test defect (F1)

The five production behaviors are implemented correctly and faithfully to the
task contract; the round-4 fail-closed repair is in place. One of the fourteen
new tests cannot pass under the candidate's own (correct) behavior, and the
full gyre-server test suite was never executed by the implementation
(deferred to the verifier), so the failure has not yet surfaced.

### F1 (blocking): `admin_api_key_broadcast_allowed_non_admin_rejected` uses a `server_only` kind for its Broadcast probes

`crates/gyre-server/src/api/messages.rs:1469`. Both Broadcast sends use
`"kind": "task_created"` (lines 1512, 1526). `MessageKind::TaskCreated` is
`server_only()` → true (`crates/gyre-common/src/message.rs:262`, literal arm
of the `server_only` `matches!`; the passing `gyre-common` test class
`server_only_returns_true_for_event_builtins` exercises that function — run
this round: `cargo test -p gyre-common --lib server_only` → 2 passed).

After the origin-table fix — correctly — an Admin-role API-key caller
resolves to `MessageOrigin::User`, so the server_only 403 gate
(`api/messages.rs:127-133`), which runs **before** the Broadcast guard
(`api/messages.rs:160-176`), rejects both sends with `kind 'task_created' can
only be sent by the server`:

- Admin half (`gyre-admin-key-b6`): asserts `StatusCode::CREATED`
  (line 1517) — actual 403 → **fails**.
- Non-Admin half (`gyre-dev-key-b6`): status 403 passes, but the body
  assertion expects `Broadcast destination requires Server origin or Admin
  role` (lines 1529-1533) — actual body is the server_only message →
  **fails**.

Discrimination is inverted: with the pre-fix role-based Admin→Server mapping
restored, the Admin half would pass. The test as written validates the
regression, not the repair. Repair: use a non-`server_only` Event-tier kind
for the Broadcast probes (e.g. `Custom("rule6_broadcast_probe")` — `Custom`
is Event tier with `server_only() == false`; Directed-tier kinds cannot
target Broadcast, 400). Coverage row 10 in
`specs/coverage/system/message-bus.md` cites this test by name as passing
evidence and must be re-checked once the test is repaired.

### Verified correct (no findings) — production code

All five required behaviors were reviewed line-by-line against the task and
spec, with the diff scoped from base `bd611083`:

1. **Scopeless-message guard** — `emit_scoped_event`
   (`crates/gyre-server/src/lib.rs:739-754`) returns `Err` and logs at error
   level when `destination != Broadcast && workspace_id.is_none()`, before
   the `Message` is constructed; `emit_event`/`emit_tenant_event` wrap it
   best-effort (warn on drop). The guard test drives both bad shapes
   (Workspace and Agent destinations with `None` scope) through a counting
   `MessageRepository` wrapper plus broadcast/dispatch channel probes, with
   Broadcast and scoped controls — it discriminates.
2. **Mem store contract** — `MemMessageRepository::store`
   (`mem.rs:4840`) returns `Err` on `workspace_id: None`, never panics,
   mirroring SQLite/PostgreSQL `message_to_new_row`; all three send paths
   (REST, MCP, emitter) skip `store()` for Broadcast. Contract test mirrors
   the SQLite adapter's, with a scoped-message happy-path control.
   `scripts/check-mem-port-contracts.sh` passes.
3. **WS `acknowledged` exclusion** — both and only both full-bus-Message WS
   serialization sites (`ws.rs:188` replay arm, `ws.rs:474` live arm — grep
   confirms no others; `ws.rs:590` serializes a `WsMessage::UserPresence`,
   not a bus Message) route through `ws_message_payload`, which strips
   `acknowledged` with the same manual-removal pattern as `send_response`.
   GET inbox returns the full struct. Replay test uses Subscribe with
   `last_seen=0` against a persisted Event-tier message (round-3 correction
   covered).
4. **Origin table** — `AuthenticatedAgent.auth_mechanism` is set at every
   extractor/validation construction site (global token, agent token, API
   key, OIDC user JWT, federated JWT, impersonation; the server-internal
   scheduler principal in `signal_chain.rs` correctly uses GlobalToken — it
   is server-constructed, not request-derived). `origin_from_auth` maps
   mechanism → origin exactly per the spec table (GlobalToken → Server;
   ApiKey/UserJwt/Impersonation → User(user_id); AgentToken/FederatedJwt →
   Agent), with a fail-safe `Agent` fallback if a user mechanism ever lacks
   a user_id. Rule 6's separate Admin-role Broadcast check is preserved, and
   the global token keeps forge authority.
5. **Rule-1 tenant isolation, fail-closed** — both the REST handler
   (`api/messages.rs:195-241`) and `mcp.rs handle_message_send`
   (`mcp.rs:2967-3005`) resolve the target agent's workspace record on every
   Agent-directed send, compare its tenant with the caller's verified
   tenant, reject 403/tool-error on mismatch (including Server-origin
   global-token callers), and reject fail-closed when the workspace record
   is unresolvable (round-4 repair `873d0821`) or propagate lookup `Err` as
   500/tool-error. Nothing is stored or broadcast on rejection.

### Probes run this round (see /tmp/stage/review-evidence/task-248-round5-evidence.md)

- `/tmp/stage/bin/cargo check -p gyre-server --tests` → exit 0 (10m06s,
  clean) — candidate including all test code compiles.
- `cargo test -p gyre-common --lib server_only` → 2 passed.
- `check-mem-port-contracts.sh`, `check-fail-open-ref-resolution.sh`,
  `check-inert-enforcement.sh`, `check-arch.sh`, `check-mcp-write-tools.sh`,
  `check-abac-route-registry.sh`, `check-scope-literal-defaults.sh`,
  `check-forwarded-header-trust.sh`, `check-forged-scope-fields.sh` → all
  exit 0.
- gyre-server test binaries NOT run here (4 GiB build budget; the wrapper
  rejects server codegen). Verifier must run:
  `cargo test -p gyre-server --lib api::messages`,
  `--lib mcp_message_send`, the two ws ack tests,
  `emit_scoped_event_rejects_scopeless_non_broadcast_message`,
  `mem_store_rejects_scopeless_message`,
  `--test api_integration agent_messages_send_and_receive`,
  `--test message_persistence`. **Prediction:** every new test passes except
  `admin_api_key_broadcast_allowed_non_admin_rejected` (F1), which fails on
  both halves.

### Scope check

`git diff bd611083..ffe47ec6 --stat` touches only the send-path sources
(`api/messages.rs`, `mcp.rs`, `ws.rs`, `lib.rs`, `mem.rs`, `auth.rs`), the
`auth_mechanism` field threading (mechanical one-line additions at existing
`AuthenticatedAgent` construction sites in `abac.rs`, `api/key_binding.rs`,
`api/orchestrator.rs`, `api/spawn.rs`, `api/specs_assist.rs`,
`commit_signatures.rs`, `impersonation.rs`, `signal_chain.rs`), test fixtures
(`tests/api_integration.rs`), and task/coverage bookkeeping. No exemptions
added, no gates weakened, no unrelated rows touched. Pre-existing
message-path tests were audited for regressions from the new tenant check
(`queue_depth_enforcement_429`, `mcp_message_send_directed_succeeds`,
`mcp_message_poll_*`, `message_persistence.rs`, `orch_state`-based MCP tests
— all seed resolvable workspaces or fail before the check).

— Review round 5, needs revision on F1.
