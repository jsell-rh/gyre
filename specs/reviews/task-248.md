# Review: task-248 — Message send-path guards

Candidate: `28d767d31545219a487659ec71b87ec73a96ca21` (base
`ee03c29984b19a2bbcc9b6f2370e8babb6e61e92`). Review round 9.
Verdict: **approved** — independent evidence supports the task contract.

## What was reviewed

The full diff `ee03c299..28d767d3` (19 files; the code surface is
bit-identical to the round-7 verified candidate 951fa88b — the delta since is
specs/docs only: the task-274 spec-backlog merge and task-file records).

## Required behavior — all five verified in source

1. **Scopeless-message guard**: `emit_scoped_event` (lib.rs) rejects
   `destination != Broadcast && workspace_id.is_none()` with `Err` + an
   error-level log BEFORE the Message is constructed, stored, or broadcast;
   `emit_event`/`emit_tenant_event` wrap it best-effort. Discriminating test
   (Err + captured ERROR log + counting store wrapper + empty
   broadcast/dispatch channels, with Broadcast and scoped controls) fails if
   the guard is removed.
2. **Mem store contract**: `MemMessageRepository::store` returns `Err` on
   `workspace_id: None` (mirroring SQLite/PostgreSQL `message_to_new_row`),
   with a contract test and happy-path control; no send path passes Broadcast
   to `store()`.
3. **WS `acknowledged` exclusion**: both bus-Message WebSocket serialization
   sites (live delivery arm and Subscribe replay arm) serialize through
   `ws_message_payload`, which strips `acknowledged` with the same
   manual-removal pattern as `send_response`; GET inbox keeps the field.
   Grep confirms no other WS site serializes a bus Message (remaining sites
   are the `WsMessage` presence protocol). Both arms have tests, plus a
   GET-inbox-keeps-the-field test.
4. **Origin table**: `origin_from_auth` derives origin from
   `AuthenticatedAgent.auth_mechanism`, set by the extractor on every auth
   path (global token → Server; API key / OIDC / impersonation →
   User(user_id); agent token / federated JWT → Agent) — never from roles or
   identity strings. The rule-6 Broadcast guard keeps its separate
   Admin-role check, and the global token retains full forge authority.
   Tests prove the consequences: Admin API-key/OIDC server_only forge → 403,
   rule-3 membership applies to their Directed sends, Admin API-key
   Broadcast allowed while non-Admin is 403 (probe uses a non-server_only
   Custom kind so it exercises the Broadcast guard, not the earlier
   server_only gate).
5. **Rule-1 tenant isolation**: the REST send handler and the mirrored MCP
   `handle_message_send` resolve the target agent's workspace record and
   compare its tenant with the caller's verified tenant on every
   `Destination::Agent(id)` send; mismatch → 403 / tool error with nothing
   stored or broadcast, including Server-origin (global-token) callers, and
   an unresolvable workspace record rejects fail-closed (dangling-agent
   leak vector). Cross-tenant, same-tenant-control, dangling-agent, and MCP
   tests all present and discriminating.

Coverage rows 5, 9, 10 of specs/coverage/system/message-bus.md were updated
to `implemented` with source/test evidence that matches inspection.

## Independent probes

- All mechanical invariant scripts (arch, ABAC registry, mem-port-contracts,
  dead-message-kinds, byte-slice, relative-path, fail-open-ref-resolution,
  scope/lossy/forged-scope, inert-enforcement, forwarded-header,
  in-memory-state, unbounded-HTTP, migration versions/portability,
  MCP write tools): **pass**.
- Task-commit-attribution: passes on the exact HEAD tree (verified in a
  temp worktree). Its failure when run in the live review working tree is an
  artifact of the review stage's own reset of specs/tasks/task-248.md
  (commits: []), not a candidate defect.
- `cargo fmt --check`: clean. `cargo check -p gyre-server --tests`: exit 0
  (type-checks every `AuthenticatedAgent` construction site and all new
  tests).
- gyre-server test execution is deferred to the independent verifier by the
  build-budget cargo wrapper (not a transport restriction or code defect).
  Required deferred probes are recorded in
  /tmp/stage/review-evidence/task-248-round9.md: `cargo test -p gyre-server
  --lib` (fourteen guard tests listed by name), `cargo test -p gyre-server
  --test message_size_and_key_rotation`, and full `cargo test --all`.

## Findings

None blocking. No fake implementations, no missing enforcement, no unsafe
scope handling, and no self-confirming tests were found.
