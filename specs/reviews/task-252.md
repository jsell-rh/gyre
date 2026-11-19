# Review — task-252 (round: rebase repair onto task-253 base 0ea6a629)

**Candidate:** `385b5069552b39e04ab4c203330bf7dd8d116b22` (base `0ea6a629c985e15089888a078cd2fc4fef532d4c`)
**Verdict:** APPROVED (fresh, independent; no earlier approval transferred)
**Evidence:** `/tmp/stage/review-evidence/task-252-review.md`

## Scope

Both capabilities of the task, plus the rebase repair (merge `0fd60541` of base
`0ea6a629`, which brought in task-253's MCP SSE work).

## Subscribe membership authorization (gap 2b439cf656cf14c344afdb07)

The ws.rs Subscribe handler now authorizes each requested workspace in order:
the preserved Admin bypass, the preserved tenant-containment check, then real
workspace-membership verification — user connections via
`WorkspaceMembershipRepository.find_by_user_and_workspace` (fail-closed on
lookup error), agent connections against their own agent record's workspace.
A rejected scope produces the SubscribeError frame, creates no subscription,
and delivers neither workspace-scoped bus messages, telemetry, nor replay.
The leak-probe test `ws_subscribe_rejects_same_tenant_non_member` asserts a
same-tenant non-member receives NOTHING (no SubscribeError-free acceptance, no
telemetry, no bus message) while a seeded member receives both — it fails
against the pre-fix tenant-match-only handler by construction (the
SubscribeError `expect` times out), so it is a genuine bug-killing test.
`ws_subscribe_agent_scoped_to_own_workspace_record` covers the agent path.

## REST telemetry endpoint (gap 64d1be477f27ebf27ec73076)

`GET /api/v1/workspaces/:workspace_id/activity` is registered in api/mod.rs
and backed by `state.telemetry_buffer.list_since` (`since` epoch ms, entries
with `created_at <= since` excluded, `limit` default 100). Per-handler
authorization mirrors the sibling `list_workspace_messages` pattern exactly
(Admin bypass; membership lookup for users; agent-record workspace match for
agent tokens; 403 otherwise), on top of the ABAC middleware's builtin
`workspace-membership-required` deny (the route has a RouteResourceMapping
entry; `check-abac-route-registry.sh` passes with no new exemptions). The
`ActivityEventData` mapping matches the spec table exactly (event_id ← id,
agent_id ← payload["agent_id"] with Agent-origin fallback, event_type ← new
`MessageKind::ag_event_type()` with AgentError→ERROR, description ←
tool_name or kind name, timestamp ← created_at epoch ms). The unscoped
`/api/v1/activity` handler and the WS ActivityQuery path are untouched.

## Checks run (details and outputs in the evidence file)

- `check-abac-route-registry.sh`, `check-arch.sh`, `check-abac-exempt-handlers.sh`,
  `check-task-commit-attribution.sh` (against the committed tree — the working
  tree was reset to the assignment state by the controller, which is not a
  candidate defect), `check-forged-scope-fields`, `check-scope-literal-defaults`,
  `check-inert-enforcement`, `check-dead-message-kinds`,
  `check-byte-slice-truncation` — all PASS.
- `cargo check -p gyre-server` and `cargo check -p gyre-server --tests` — clean.
- `cargo test -p gyre-common` — 130 passed, including the new
  `kind_ag_event_type_mapping_matches_spec` and the TelemetryBuffer boundary
  tests.
- `web: npx vitest run src/__tests__/WorkspaceSettings.test.js` — 85 passed
  (the frontend change is a test-only frozen-clock flake repair; no assertion
  weakened).
- Rebase repair verified: `git diff cfe34f2c..385b5069 -- crates/ web/ scripts/`
  touches only `mcp.rs` (task-253 content via the merge) — every task-252
  source file is byte-identical to the previously approved head; the merge
  resolution combines task-252's coverage rows (9, 11, 13) with task-253's
  (header, row 14) coherently.

## Deferred to the independent verifier

gyre-server test compilation exceeds the sandbox build budget; the verifier
must run `cargo test -p gyre-server --lib ws_subscribe`,
`cargo test -p gyre-server --lib workspace_activity`,
`cargo test -p gyre-server --lib presence`, the full workspace suite, and
Clippy. Test-strength analysis (evidence §2) shows both leak probes fail
against the pre-fix behavior by construction.
