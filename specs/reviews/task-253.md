# Review — task-253 (MCP SSE stream: workspace filtering and MessageKind-to-AG-UI event mapping)

Spec: `specs/system/message-bus.md` §MCP Integration (lines 577-591): "The
MCP SSE endpoint (`GET /mcp/sse`) subscribes to the message bus filtered by
the authenticated agent's workspace. It maps `MessageKind` variants to
AG-UI event types for protocol compatibility" + the seven-row event table.

Candidate: `3126c0ba` (base `05024b3e`), product-surface commits
`c9c8b4c3` (filter + mapping + tests), `b4ac4adc` (clippy repair),
`1388c32e` (principal-namespace guard). Diff touches only
`crates/gyre-server/src/mcp.rs`, `docs/api-reference.md`,
`scripts/test_dev_cargo_clean.py` (CI repair, see below), and task/coverage
bookkeeping.

Verdict: **complete** — approved; server test execution deferred to the
independent verifier per the sandbox build-budget rule (exact commands
recorded below and in `/tmp/stage/review-evidence/`).

## What was verified

**Workspace filtering is real and fail-closed** (`mcp_sse_handler`,
mcp.rs). Scope resolution never fabricates: agent/system principals
resolve the stored agent record (authoritative ABAC boundary) with the
signature-verified `workspace_id` JWT claim (mirrored by
`with_hierarchy_from_claims`, auth.rs:1110) as fallback; user principals
(API key, Keycloak) are claim-only; principals with neither scope see
Broadcast messages only. Delivery: `Destination::Workspace(ws)` only on
workspace-id match (ids globally unique → tenant containment),
`Destination::Broadcast` to all subscribers per message-bus.md §Delivery
("all connected clients"), `Destination::Agent(id)` only on the
authenticated agent's own stream. Admins without a resolvable workspace
(global token) keep the legacy unscoped view — mirrors the ws.rs Subscribe
admin bypass and message-bus.md line 96 ("global GYRE_AUTH_TOKEN …
bypasses workspace scoping").

**Directed-delivery principal-namespace guard** (review repair `1388c32e`):
`Destination::Agent` matches only genuine agent principals
(`user_id.is_none() && !Admin`) — byte-for-byte the ws.rs bus filter
semantics (ws.rs:479-497). An API-key user whose display_name collides
with an agent's id, or the global token (`agent_id: "system"`) versus an
agent literally named "system", can never receive another principal's
Directed messages.

**AG-UI mapping matches the spec table exactly** (`agui_event_name`):
ToolCallStart→TOOL_CALL_START, ToolCallEnd→TOOL_CALL_END,
RunStarted→RUN_STARTED, RunFinished→RUN_FINISHED,
TextMessageContent→TEXT_MESSAGE_CONTENT, StateChanged→STATE_CHANGED,
AgentError→ERROR; kinds outside the table (incl. `Custom`) keep the
generic `message`. Data payload unchanged (raw envelope JSON). Existing
broadcast notifications (DataSeeded/QueueUpdated → `message`) still
arrive; no in-repo frontend consumer of `/mcp/sse` relies on the old
all-`message` naming (the EventSource consumers in web/src target the
audit and agent-log streams).

**Tests are hard and mutant-killing** (six focused tests in mcp.rs, real
auth chain — minted EdDSA JWTs registered in `agent_tokens`, real API-key
hash lookup, real global token; mutation analysis in
`/tmp/stage/review-evidence/task-253-mutant-analysis.md`): the isolation
test's B-side assertion kills a filter-removal mutant *given* the mapping
(MessageKind serializes snake_case, so "TOOL_CALL_START" can only come
from the SSE event name); the mapping test pins all seven table rows plus
the default; the directed tests kill directed-filter and
namespace-guard removal; the record_activity test is a full end-to-end
MCP tools/call → bus → SSE probe. No conditional guards, no
assertionless bodies (mechanical checks pass).

**Independent probes** (this checkout, candidate tree):

- `cargo check -p gyre-server --tests` → exit 0 (10m57s; log:
  `/tmp/stage/review-evidence/task-253-review-cargo-check.log`).
- `rustfmt --check crates/gyre-server/src/mcp.rs` → clean.
- Mechanical checks at the candidate commit: arch, mcp-write-tools,
  inert-enforcement, dead-message-kinds, abac-route-registry,
  fabricated-scope-defaults, scope-literal-defaults,
  conditional-test-guards, identical-baselines, aspirational-test-names,
  task-commit-attribution all pass; check-mcp-wrapper-parity's 45
  violations are byte-identical to base (line-number shifts only — no new
  violations). Full table:
  `/tmp/stage/review-evidence/task-253-check-scripts.log`.

**Out-of-scope-looking change inspected** — `scripts/test_dev_cargo_clean.py`
(commit `5fba4da7`, CI repair round): moves the "dependency cache
retained" sentinel from the probe's own executable directory into a real
path dependency's artifact directory. Verified empirically
(`/tmp/stage/review-evidence/task-253-sentinel-experiment.md`) that this
does not weaken the guarded property (mutant-detection assertion
untouched; a full `cargo clean` still removes the new sentinel's
directory) and does not mask a cache-wipe defect (package-scoped clean
retains dependency artifacts and unknown files on cargo 1.99;
`dev-cargo-clean.py` is unchanged by the candidate). Not product surface
per the attribution check's definition; non-blocking.

## Findings

- (none blocking)

Minor observations, recorded for the verifier (not contract violations):

1. The agent-record-authoritative scope arm has no dedicated delivery test
   where record workspace and JWT claim diverge (all six tests' subscribers
   resolve via the claim fallback or a matching record). The task permits
   either resolution source and record-precedence is the stricter choice.
2. Pre-existing, unchanged by this candidate: `dev-cargo-clean.py`'s
   qualified `cargo clean --package <package-id>` is a no-op on cargo
   1.99 ("Removed 0 files" while the package's artifacts are present; the
   unqualified name removes them). Dev tool only, identical at base.
3. Server test execution deferred: the review-stage build wrapper blocks
   gyre-server codegen. Verifier must run
   `cargo test -p gyre-server --lib mcp_sse` (all six tests) and
   `python3 scripts/test_dev_cargo_clean.py`.

— Review, 2026-11-19
