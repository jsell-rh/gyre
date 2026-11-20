# Review: task-248 — Message send-path guards

**Candidate:** `67623804cc04f028db91f291052d952a181c274b` (base `00669baacfd5112b24e1466ad51f5a9bec43d8e9`)
**Reviewer round:** independent review, fresh verdict (no earlier approval carried forward)
**Verdict:** approved — see `/tmp/stage/verdict.json`; evidence in `/tmp/stage/review-evidence/`

## Scope

Five deviations from `specs/system/message-bus.md` §Message Envelope and §Scoping Rules,
all on the message construction/send path: scopeless-message guard, mem store contract,
WS `acknowledged` exclusion (both serialization sites), auth-mechanism origin table,
and rule-1 Agent-destination tenant isolation (REST + mirrored MCP).

## What was verified

Independent diff inspection of `00669baa..67623804` plus in-sandbox probes:

- `SKIP_WEB_BUILD=1 cargo check -p gyre-server --tests` — exit 0 (resolves the E0063
  class from CI run 44680564932 at type level).
- `cargo fmt --check` — exit 0.
- `scripts/check-arch.sh` — exit 0.
- `scripts/check-mem-port-contracts.sh` — exit 0.
- `scripts/check-task-commit-attribution.sh` — the working-tree failure is an artifact
  of the stage-injected uncommitted reset of `specs/tasks/task-248.md` (it matches the
  assignment template); the committed candidate lists all 8 task-248 implementation
  commits in its `commits:` frontmatter, so attribution holds on the candidate.

### Behavior findings (all five implemented as specified)

1. **Scopeless guard** — `emit_scoped_event` (lib.rs) returns `Err` + `tracing::error!`
   before constructing the message when `destination != Broadcast &&
   workspace_id.is_none()`; `emit_event`/`emit_tenant_event` handle the Err. The test
   drives the real emitter through a counting `MessageRepository` wrapper and asserts
   Err, an ERROR-level captured log record, zero store() calls, and empty
   broadcast/dispatch channels, with Broadcast and scoped controls.
2. **Mem store contract** — `MemMessageRepository::store` returns Err on
   `workspace_id: None`, matching the SQLite/PostgreSQL adapters; contract test
   mirrors the SQLite one (Broadcast, scopeless, scoped happy path). No other path
   passes a scopeless message to store().
3. **WS `acknowledged` exclusion** — both bus-Message serialization sites in ws.rs
   (live delivery arm and Subscribe replay arm) route through `ws_message_payload`
   (same manual-removal pattern as `send_response`); GET inbox responses keep the
   field. Live-arm and replay-arm tests both assert the key is absent; a GET-inbox
   test asserts it remains.
4. **Origin table** — new `AuthMechanism` discriminator is set by the auth extractor
   on every authentication path (global token, agent token, API key, Keycloak/OIDC
   JWT, federated JWT, impersonation) and consumed by `origin_from_auth`; no
   spoofable string or role membership decides origin. Admin-role API-key/OIDC
   callers now resolve to `User(user_id)` (server_only 403 gate and rule-3 membership
   check apply — both probed through the real extractors with seeded users/keys and a
   signed RS256 test JWT), the global token keeps Server origin with full forge
   authority, and the Broadcast guard's separate Admin-role check preserves rule 6
   for Admin API keys (probed with a non-server_only Custom kind so the Broadcast
   guard is the gate under test).
5. **Rule-1 tenant isolation** — the REST handler and the mirrored MCP
   `handle_message_send` resolve the target agent's workspace record and compare its
   tenant with the caller's verified tenant on every `Destination::Agent(id)` send;
   mismatch → 403 (nothing stored or broadcast), including Server-origin global-token
   callers; an unresolvable workspace record rejects fail-closed rather than
   fabricating a scope. Cross-tenant, same-tenant, dangling-agent, and MCP-mirror
   tests all assert persistence/broadcast state, not just status codes.

Pre-existing test fixtures that would trip the new fail-closed guard were repaired by
seeding real workspace rows (message_size_and_key_rotation.rs, api_integration.rs,
mcp tests) with no assertion weakening; the cross-task attribution for those fixture
repairs is recorded in task-254/task-258 frontmatter.

Coverage rows 5, 9, 10 of `specs/coverage/system/message-bus.md` were updated with
concrete source and test evidence; no unrelated rows were touched.

## Verifier handoff (gyre-server test execution deferred per the 4 GiB build budget)

The verifier must run the gyre-server suites: `cargo nextest run -p gyre-server --lib`
and the `api_integration`, `message_size_and_key_rotation`, `message_persistence`,
`task134_review_probe` integration tests (full list of named tests in
`/tmp/stage/review-evidence/task-248-review.md`), plus the workspace-wide suite and
clippy-diff for regressions.

## Non-blocking observations

- The `/mcp/sse` bus filter (mcp.rs) still serializes the full Message including
  `acknowledged` over SSE; the spec field contract names POST responses and WebSocket
  pushes, and SSE delivery was task-253's scope, so this is outside task-248's
  contract — flagged for a future audit round, not a defect of this candidate.
- `ws_message_payload` degrades to the text `null` on an impossible serialization
  failure instead of panicking (the old code `.unwrap()`ed); acceptable.
