# Review — task-253 (MCP SSE stream: workspace filtering and MessageKind→AG-UI event mapping)

Spec: `specs/system/message-bus.md` §MCP Integration (lines 577–591): "The MCP SSE
endpoint (`GET /mcp/sse`) subscribes to the message bus filtered by the authenticated
agent's workspace. It maps `MessageKind` variants to AG-UI event types."

Candidate under review: `a5f1bb1e9eced03ebff65efe0a05c9b7731179b6` (assigned base
`61b6080d1866f32d98cc7a35d62e3fb2b88a290a`). Product-surface commits in scope (per
`commits:` frontmatter, verified complete by `check-task-commit-attribution.sh`):
`c9c8b4c3` (filter + mapping + tests), `b4ac4adc` (test-helper repair), `1388c32e`
(principal-namespace guard). `5fba4da7` (CI repair of `scripts/test_dev_cargo_clean.py`,
non-product-surface) also inspected.

Verdict: **approved** (fresh verdict for this candidate; the earlier approval of
`09f18bae` was re-verified, not transferred — the rebase onto base `61b6080d` brought
in only task-251's base-side `handle_agent_complete`/inbox tests; the task-253 SSE
code is unchanged since the reviewed candidate).

## Evidence (this sandbox, review stage)

- `cargo check -p gyre-server --tests` → **clean, exit 0** (11m08s, build wrapper;
  `review-evidence/cargo-check.txt`). All six focused SSE tests compile.
- `cargo fmt --check -p gyre-server` → clean.
- Mechanical invariants all pass: `check-arch.sh`, `check-abac-route-registry.sh`,
  `check-abac-exempt-handlers.sh`, `check-inert-enforcement.sh`,
  `check-fabricated-scope-defaults.sh`, `check-scope-literal-defaults.sh`,
  `check-mcp-write-tools.sh`, `check-dead-message-kinds.sh`,
  `check-byte-slice-truncation.sh`, `check-relative-path-defaults.sh`,
  `check-lossy-secret-conversion.sh`, `check-forwarded-header-trust.sh`,
  `check-in-memory-state-stores.sh`, `check-unbounded-external-http.sh`,
  `check-fail-open-ref-resolution.sh`, `check-migration-versions.sh`,
  `check-migration-sql-portability.sh`, `check-mem-port-contracts.sh`
  (`review-evidence/mechanical.txt`, `mechanical2.txt`).
- `check-task-commit-attribution.sh` fails **only against the working tree**, where
  the review harness reset `specs/tasks/task-253.md` to assignment state
  (`progress: not-started`, `commits: []`). The committed candidate records all
  three product-surface task-253 commits; `5fba4da7` touches `scripts/` only, which
  the check correctly does not count as product surface. Not a candidate defect.
- `python3 -m unittest scripts/test_dev_cargo_clean.py` → the rebuild/retention test
  cannot execute here: the sandbox build wrapper rejects the bare `cargo test` the
  test invokes (exit 64, `review-evidence/cargo-clean-test.txt`). Infrastructure
  restriction, not a code defect — deferred to the verifier.

## Code verification (independent trace, no implementation claims taken on trust)

- **Scope resolution is fail-closed and never fabricated** (`mcp.rs` `mcp_sse_handler`):
  agent/system principals resolve the stored agent record
  (`state.agents.find_by_id`) — the authoritative ABAC boundary — with the
  signature-verified `workspace_id` JWT claim (mirrored by
  `AuthenticatedAgent::with_hierarchy_from_claims`, auth.rs:1052) as fallback when no
  record exists; user principals (API key, Keycloak — `user_id` set, `agent_id` =
  display name) are claim-only and never inherit a same-named agent record's
  workspace; principals with neither see Broadcast only. No `"default"` literal
  anywhere on the new path (confirmed by the fabricated-defaults scripts).
- **Delivery filter matches the task contract exactly**: `Destination::Workspace(ws)`
  delivered only on workspace-id match (workspace ids are server-generated UUIDs,
  `api/workspaces.rs:128`, so a match implies tenant containment);
  `Destination::Broadcast` delivered to all per message-bus.md line 288 ("all
  connected clients"); `Destination::Agent(id)` only on the authenticated agent's
  own stream. Admins with no resolvable scope keep the legacy unscoped dashboard
  view, mirroring the admin bypass in `ws.rs` Subscribe (ws.rs:107) — a narrowing
  for everyone else, no new exposure.
- **Principal-namespace guard (repair `1388c32e`) is semantically identical to the
  sibling ws.rs bus filter** (ws.rs:479-493): `agent_principal = user_id.is_none()
  && !roles.contains(Admin)`. Verified against the extractor: agent JWTs get
  `roles: [UserRole::Agent]`, `user_id: None` (auth.rs:1464); the global token gets
  `roles: [Admin]` (auth.rs:1371); API-key/Keycloak users get `user_id: Some(...)` —
  so neither a user whose display name collides with an agent id nor the global
  token versus an agent literally named "system" can receive another principal's
  Directed messages.
- **AG-UI mapping is the spec table verbatim** (`agui_event_name`): the seven
  spec-table kinds map to `TOOL_CALL_START`/`TOOL_CALL_END`/`RUN_STARTED`/
  `RUN_FINISHED`/`TEXT_MESSAGE_CONTENT`/`STATE_CHANGED`/`ERROR`; all other kinds
  (incl. `Custom`) keep the generic `message` event, and the SSE data payload is
  the unchanged raw envelope JSON. Existing server→client notifications keep
  arriving (Broadcast arm; regression test present).
- **`gyre_record_activity` telemetry surfaces on same-workspace streams**: the tool
  (out of scope, unchanged) broadcasts `Destination::Workspace(ws from the caller's
  agent record)` on `message_broadcast_tx` (mcp.rs:2122) — the new filter delivers
  it to same-workspace subscribers with the mapped event name; covered end-to-end by
  `mcp_sse_record_activity_telemetry_reaches_workspace` through the real MCP
  JSON-RPC path.

## Test quality (all six in `mcp.rs` tests, wire-level through `build_router` + real
broadcast channel — no mirrored logic)

- `mcp_sse_workspace_isolation` (cross-tenant A/B), `mcp_sse_agui_event_mapping`
  (all 7 kinds + out-of-table default), `mcp_sse_directed_delivery` (own delivered,
  other agent's not), `mcp_sse_directed_delivery_rejects_principal_namespace_collisions`
  (API-key display-name collision + global-token "system" collision, payload-marker
  assertions), `mcp_sse_admin_connection_still_receives_notifications` (regression
  guard), `mcp_sse_record_activity_telemetry_reaches_workspace`.
- Mutation reasoning (verified against handler + serde): mapping removed → the
  mapping/isolation/record_activity tests fail (event name reverts to `message`);
  workspace filter removed → B's stream receives `event: TOOL_CALL_START` → the
  isolation test's negative assertion fails; directed filter removed or namespace
  guard removed → the marker assertions in the directed/collision tests fail. No
  assertion depends on stubbed or self-confirmed state; auth is real (minted EdDSA
  JWTs registered in `agent_tokens`, real API-key hash lookup, global token).
- Minor observation (non-blocking): the isolation test's negative assertion
  (`!text_b.contains("TOOL_CALL_START")`) would not by itself fail against the
  pre-fix handler, because the raw envelope serializes the kind as snake_case
  `tool_call_start`; against pre-fix code the test fails via its positive assertion
  on stream A (which requires the mapping), and any post-fix workspace-filter
  regression is caught directly. The isolation property is genuinely guarded.

## CI repair `5fba4da7` (scripts/test_dev_cargo_clean.py)

The sentinel proving "a workspace-scoped clean retains dependency artifacts" moved
from the probe's own executable directory (which `cargo clean --package` legitimately
removes on toolchains using per-package `build/<pkg>/<hash>/` layouts — the observed
CI failure) to a genuine path dependency's Cargo-reported artifact directory. This
strengthens rather than weakens the invariant: the sentinel now sits exactly where a
real external dependency's artifacts live, which a package-scoped clean must retain
and a full clean must delete. Cannot be executed under the review-stage build
wrapper; deferred (below). Minor observation (non-blocking): the final round's
Shipped-notes rewrite (60b8d0c9) dropped the CI-repair paragraph that a1769f3b had
recorded; the rationale survives in the commit message and in-code comments.

## Deferred to the independent verifier (gyre-server test compilation is
verifier-owned per the sandbox build budget)

1. `cargo test -p gyre-server --lib mcp_sse` — all six focused SSE tests.
2. `python3 -m unittest scripts.test_dev_cargo_clean` — the repaired CI test
   (runs `cargo` directly; blocked by the review-stage wrapper here).
3. `python3 scripts/check-rustfmt-diff.py` / `check-clippy-diff.py` and the full
   workspace suite per usual CI.
