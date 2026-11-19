# Review — task-248 (Message send-path guards: scope validation, mem store contract, WS acknowledged exclusion, auth-mechanism origin table, Agent-destination tenant isolation)

Spec: `specs/system/message-bus.md` §Message Envelope (send_message() helper
validation; store()-Err-on-None; `acknowledged` conditional serialization),
§Origin and tenant resolution from auth context (normative table),
§Scoping Rules rule 1 (Agent-destination tenant isolation) and rule 6
(Broadcast Server-origin-or-Admin-role).
Candidate under review: `1c00b1b336e7b89bc91b624ffad0d816b179bdba`
(base `321615ec`, includes the rebase onto task-250's WS replay-window work).
Commits scoped per `commits:` frontmatter: `955c134a`, `9dd4a1f0`,
`873d0821` (round-4 fail-closed repair), `3bb9c9bd` (round-5 rule-6 probe
repair), `a2781ff0` (round-7 clippy repair).
Verdict: **approved** — all five required behaviors are implemented in
production code on the send path, fail-closed where scope is indeterminable,
and each acceptance criterion is covered by a test anchored to the specific
gate with discriminating controls. Runtime execution of the gyre-server test
suite is deferred to the independent verifier (the executor's build wrapper
rejects gyre-server test compilation within the review sandbox to fit the
4 GiB budget); the required probe list is recorded in
`/tmp/stage/review-evidence/REVIEW-NOTES.md`.

## Independent evidence (this review)

- `cargo check -p gyre-server` (lib) — pass, 6m32s.
- `cargo check -p gyre-server --tests` — pass (test code type-checks).
- Invariant scripts — check-mem-port-contracts, check-inert-enforcement,
  check-fabricated-scope-defaults, check-scope-literal-defaults,
  check-lossy-secret-conversion, check-fail-open-ref-resolution — all pass.
- `cargo test -p gyre-server --lib --no-run` — REJECTED by the build wrapper
  (4 GiB budget; server tests belong to the verifier). Not a code defect.
- Full diff read base..candidate; every production path below traced in the
  candidate tree, including the ABAC middleware ordering for each new test
  (system-full-access priority 1000 lets the global token reach the handler
  on a foreign-tenant workspace so the rule-1 403 — not middleware — is what
  the cross-tenant probe asserts; builtin-developer-write-access 800 lets the
  non-Admin member reach the Broadcast guard).

## Verified behavior

1. **Scopeless-message guard** — `emit_scoped_event` (lib.rs), the shared
   body of the internal emitter family, returns `Err` and logs at `error`
   level when `destination != Broadcast && workspace_id.is_none()`; the
   message is never constructed, stored, or broadcast. Test
   `emit_scoped_event_rejects_scopeless_non_broadcast_message` drives the
   emitter with Workspace and Agent destinations + `None` scope through a
   counting MessageRepository wrapper, asserts the Err, a captured
   ERROR-level record, store count 0, and empty broadcast/dispatch channels —
   plus two discriminating controls (Broadcast passes and is broadcast but
   not stored; a scoped Workspace message is stored exactly once). The
   tracing capture confines the MutexGuard before later awaits (round-7
   clippy repair).
2. **Mem store contract** — `MemMessageRepository::store` returns `Err` on
   `workspace_id: None`, never panics, mirroring the SQLite/PostgreSQL
   `message_to_new_row` contract (check-mem-port-contracts invariant passes).
   Test `mem_store_rejects_scopeless_message` covers Broadcast and scopeless
   non-Broadcast shapes plus the scoped happy path. Verified statically that
   no production send path passes a Broadcast/scopeless message to `store()`
   (emitter, REST, and MCP all skip Broadcast and derive `Some(workspace_id)`
   otherwise).
3. **WS `acknowledged` exclusion** — both WebSocket Message serialization
   sites (live bus-delivery arm and Subscribe replay arm, ws.rs) serialize
   through `ws_message_payload`, which strips `acknowledged` with the same
   manual-removal pattern as `send_response`; the only other
   `serde_json::to_string(&msg)` in ws.rs serializes a presence WsMessage,
   not a bus Message. Tests cover both arms
   (`ws_live_delivery_strips_acknowledged`,
   `ws_subscribe_replay_strips_acknowledged` — round-3 correction) and
   `get_inbox_still_includes_acknowledged` pins the GET-inbox half of the
   conditional-serialization contract. The rebase onto task-250's
   newest-1000 replay window preserved the strip at the replay site.
4. **Origin table** — origin now follows the auth mechanism threaded through
   `AuthenticatedAgent.auth_mechanism` by the extractor on every path
   (global token → Server; API key / OIDC user JWT / impersonation →
   User(user_id); agent token / federated JWT → Agent), never role membership
   or spoofable identity strings. Rule 6 is preserved independently: the
   Broadcast guard's separate Admin-role check still admits Admin-role
   API-key callers (the rule-6 probes correctly use a non-server_only Custom
   kind, since the server_only gate — which now applies to Admin API-key/OIDC
   callers as User origin — precedes the Broadcast guard), and the global
   token keeps full forge authority. Consequences tested: Admin API-key and
   Admin OIDC callers are rejected 403 on a server_only kind with nothing
   stored, and the rule-3 membership check applies to them on Directed sends
   (403 "not a member" without membership, 201 with).
5. **Rule 1 tenant isolation** — both the REST handler and the mirrored MCP
   `handle_message_send` load the target agent's workspace record and compare
   its tenant with the caller's verified tenant on every Agent-directed send,
   including Server-origin global-token callers (403 / tool error, nothing
   stored or broadcast; same-tenant control succeeds and is persisted and
   broadcast). Unresolvable target workspace (dangling agent after a
   non-cascading workspace delete) rejects fail-closed rather than
   skip-and-send — the round-4 repair, with tests on both paths.

## Scope discipline

Diff touches only gyre-server sources/tests, the task file, and the
message-bus coverage matrix. Coverage rows 5, 9, 10 were updated with
concrete source evidence and honestly downgraded `verified` → `implemented`
pending independent verification. No gates weakened, no exemption files
grown, no unrelated rows touched. Commit attribution complete in the
candidate (the working-tree `commits: []` state is the harness checkpoint
restore, not the candidate). No web/ or generated-output changes.

## Deferred to the verifier (runtime execution)

`cargo test -p gyre-server --lib` (module filters and full-suite regression
list in `/tmp/stage/review-evidence/REVIEW-NOTES.md`), plus
`cargo test -p gyre-server --test api_integration agent_messages_send_and_receive`
(the test seeds the bootstrap "default" workspace row the rule-1 lookup
requires). Type-checking of all of the above passed here; the wrapper blocks
test compilation in the review sandbox by design.

Findings: none.
