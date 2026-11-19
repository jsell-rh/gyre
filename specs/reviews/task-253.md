# Review — task-253: MCP SSE workspace filtering + AG-UI event mapping

**Candidate:** `2d40ef19f4050f5aba2d61926c3ec4a1f0885ea3`
**Base:** `67cc03988b938c4635973761c08108cee8f8459d`
**Verdict:** approved (fresh, this round; no earlier approval transfers)
**Evidence:** `/tmp/stage/review-evidence/` (review-notes.md and probes below)

## What was reviewed

`mcp_sse_handler` (crates/gyre-server/src/mcp.rs) rewritten to (1) filter bus
delivery by the authenticated agent's workspace and (2) map `MessageKind` →
AG-UI SSE event names, per message-bus.md §MCP Integration; plus six focused
tests, docs, coverage row 14, and a CI repair to
scripts/test_dev_cargo_clean.py (dev-controller run 44486313569). Note:
crates/gyre-server/ is byte-identical to previously reviewed candidate
09f18bae — this round's delta is the CI repair + docs; all findings below are
from fresh inspection of the full base→candidate diff.

## Independent evidence

1. **Spec-table conformance (inspection):** `agui_event_name` matches the
   canonical table in specs/system/message-bus.md exactly (all seven rows);
   kinds outside the table keep the generic `message` event, as the task
   allows. SSE data payload unchanged (raw envelope JSON).
2. **Filter semantics (inspection):** `Destination::Workspace(ws)` delivered
   only on exact scope match; `Destination::Broadcast` to all; 
   `Destination::Agent(id)` only on the authenticated agent's own stream, with
   a principal-namespace guard (`user_id.is_none() && !Admin`) that mirrors the
   existing ws.rs bus filter verbatim (ws.rs:488-497) — API-key users whose
   display_name collides with an agent id, and the global token vs an agent
   named "system", never receive another principal's Directed messages.
3. **Fail-closed scope resolution (inspection):** agent record (authoritative
   ABAC boundary) → signature-verified `workspace_id` JWT claim → None;
   non-admins with no scope see Broadcast only. Subscriber scope is always
   server-derived (record/claim), never client-chosen, and workspace ids are
   server-generated on all production creation paths.
4. **Compilation:** `cargo check -p gyre-server --tests` passes (13m25s, 0
   errors) — the six new tests compile against the real
   test_state/build_router/mint helpers.
5. **Mutation sensitivity (analysis against base handler):** the base handler
   emitted every message as `event: message` (mcp.rs:5064-5083 at base), so
   the mapping assertions (`event: TOOL_CALL_START` etc.) fail on the old
   code; with mapping present but the filter removed, the mapped event name
   would appear on workspace B's stream, failing the isolation assertion. The
   directed-collision assertions fail if the namespace guard is removed.
6. **Mechanical invariants:** check-arch.sh and check-mcp-write-tools.sh pass;
   task commit attribution complete for all product-surface task-labeled
   commits (c9c8b4c3, b4ac4adc, 1388c32e).
7. **CI repair verified:** the repaired test's property was replicated through
   the build wrapper on cargo 1.99.0 — the workspace-scoped clean rebuilds the
   current checkout (mutant detected, rc 101) while the dependency's artifact
   directory (and the sentinel placed beside it) is retained. The direct
   unittest run is blocked in this sandbox only because the review-stage
   wrapper rejects the bare `cargo test` the test invokes — an infrastructure
   restriction, not a code defect.

## Deferred to the independent verifier

- `cargo test -p gyre-server mcp_sse` (the six focused tests), then the full
  gyre-server suite for regressions.
- `python3 -m unittest discover -s scripts -p 'test_dev*.py'` in an
  environment where bare `cargo test` is permitted.
- Clippy all-targets.

## Non-blocking observations

- Unscoped admins (global token) keep the legacy unscoped view (all Workspace
  messages), mirroring the ws.rs Subscribe admin bypass — documented in the
  handler; consistent with existing reviewed behavior and outside the task's
  agent-client isolation contract.
- Coverage row 14 honestly labels the focused tests as pending the independent
  verifier run (local execution blocked by the sandbox build budget).
