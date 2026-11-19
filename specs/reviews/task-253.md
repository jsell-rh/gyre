# Review — task-253 (MCP SSE stream: workspace filtering and MessageKind→AG-UI event mapping)

Spec: `specs/system/message-bus.md` §MCP Integration (gap
`103aac23ed1cdc9d4788f31d`).
Commits under review: `c9c8b4c3` (product — crates/gyre-server/src/mcp.rs),
`272fc568`/`3f5274a8` (docs only).
Candidate reviewed: `3f5274a8c87a75bc05f5131feefbf50f0c7f54da` on base
`fb1424fbac55cd23cb08606f4cd667a96f4dbc43`.
Verdict: **approved** — both halves of the gap are genuinely implemented;
see non-blocking observations and the required verifier probe below.

## Round 1 (2026-11-19)

Probes run (full detail in `/tmp/stage/review-evidence/task-253.md`):

- `SKIP_WEB_BUILD=1 /tmp/stage/bin/cargo check -p gyre-server --lib --tests`
  → clean (exit 0, 8m58s) — independently reproduces the implementer's
  type-check claim for the five new tests.
- `SKIP_WEB_BUILD=1 /tmp/stage/bin/cargo test -p gyre-server --lib mcp_sse`
  → refused by the executor's build wrapper (gyre-server test codegen exceeds
  the sandbox's 4 GiB model budget — infrastructure restriction, not a code
  defect). **Required verifier probe:**
  `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib mcp_sse` (expect 5
  passed). In lieu of execution, each test's runtime path was traced through
  the real auth chain, handler, gates, and bus semantics (see evidence file).
- `bash scripts/check-arch.sh`, `check-mcp-write-tools.sh`,
  `check-dead-message-kinds.sh`, `check-abac-route-registry.sh`,
  `check-byte-slice-truncation.sh`, `check-scope-literal-defaults.sh` → all
  OK.
- `git show --stat` over the three commits: only `c9c8b4c3` touches product
  surface and it is recorded in the task's `commits:` frontmatter; the two
  docs commits are outside the attribution gate by design.

Verified working:

- **Workspace filtering (AC1).** `mcp_sse_handler` now resolves the
  subscriber's scope fail-closed: for agent/system principals the stored
  agent record governs (`state.agents.find_by_id`), with the
  signature-verified `workspace_id` JWT claim (mirrored by
  `with_hierarchy_from_claims`) as fallback; user principals are claim-only;
  principals with neither get no scope (Broadcast only) unless Admin.
  `Destination::Workspace(ws)` is delivered only on exact workspace match
  (workspace ids are UUIDs, globally unique, so match implies tenant
  containment); `Destination::Broadcast` per bus semantics;
  `Destination::Agent(id)` only on the authenticated agent's own stream.
  The record-first resolution is consistent with the rest of the MCP tooling
  (`handle_record_activity`, message send/poll all derive workspace from the
  agent record). `mcp_sse_workspace_isolation` drives two real
  router connections with minted+KV-registered EdDSA JWTs in different
  tenants/workspaces and asserts B's stream stays silent for A's
  Workspace-scoped message — it fails against the base handler.
- **AG-UI mapping (AC2).** `agui_event_name` implements the exact spec table
  (ToolCallStart→TOOL_CALL_START … AgentError→ERROR); kinds outside the table
  keep the generic `message` event. `mcp_sse_agui_event_mapping` covers all
  seven mappings plus the TaskCreated default and fails against the base
  always-`message` handler.
- **Directed delivery (AC3).** `mcp_sse_directed_delivery` asserts the
  authenticated agent's directed message arrives and another agent's does
  not; the base handler delivered both.
- **Existing notifications (AC4).** Broadcast delivery is unconditional, so
  server→client notifications (e.g. `emit_tenant_event` Broadcast messages)
  still arrive; `mcp_sse_admin_connection_still_receives_notifications`
  guards this on the global-token admin connection.
- **record_activity telemetry (AC5).** `handle_record_activity` (unchanged,
  out of scope) derives the destination workspace from the caller's agent
  record — the same source the SSE filter uses — so
  `mcp_sse_record_activity_telemetry_reaches_workspace` exercises the full
  tool→bus→SSE path and asserts `event: RUN_STARTED` on a same-workspace
  subscriber.
- **No test inflation.** Four of the five tests fail against the pre-task
  handler (verified by reading the base code path); the fifth is an explicit
  regression guard for AC4. No stubs, mocks, or mirrored logic — real
  router, real auth extractor, real broadcast bus, no TCP listener.
- **Docs/coverage.** `docs/api-reference.md` /mcp/sse rows accurately
  describe the new behavior; coverage row 14 (MCP Integration) gained the
  SSE evidence with an honest "pending independent verifier run" note;
  no unrelated rows touched.

Non-blocking observations (recorded in the evidence file):

- [-] Admin principals with no resolvable workspace keep an unscoped legacy
  view (all Workspace messages across tenants). Deliberate and documented in
  the handler, admin-privileged and consistent with the global token's
  platform-wide ABAC bypass; the code comment slightly overstates parity
  with ws.rs (whose admin bypass covers Broadcast only — WS Workspace
  delivery requires subscription even for admins).
- [-] `Destination::Agent` matching compares `agent_id` strings without the
  `user_id.is_none()`/non-Admin guard ws.rs uses; this mirrors the REST
  inbox semantics (`poll_messages`) and the task text literally, and
  exposure requires an agent id colliding with a user display name.
- [-] The shipped task doc points at
  `/tmp/stage/review-evidence/task-253.md` for implementation evidence; that
  stage-scoped path was empty at review time (the stage /tmp is not durable).
  The substantive claims it referenced (clean cargo check, exact verifier
  command) were independently reproduced and hold.

Verifier must run (could not execute here — build-wrapper restriction):
`SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib mcp_sse` (expect 5
passed), plus the standard full workspace suite it already runs.
