# Review — task-252: Workspace-scoped telemetry access

**Candidate:** `6c40126253354cd1c3beeb331b0f62e169b79c14` (base `25497b9e3cc16c7e405c641870ff3c3b1905cf6b`)
**Verdict:** approved (independent review, 2026-11-19)
**Evidence:** `/tmp/stage/review-evidence/probes.md`, `server-tests-for-verifier.md`

## Subscribe membership authorization (gap 2b439cf656cf14c344afdb07)

The ws.rs Subscribe handler (crates/gyre-server/src/ws.rs:98-192) now authorizes
each requested workspace in order: the preserved Admin bypass (checked first,
unchanged semantics for the global token), the preserved tenant-containment
check, then real workspace-membership verification — user connections
(`caller.user_id` set) via `state.workspace_memberships.find_by_user_and_workspace`
(fail-closed on lookup error), agent connections via their agent record's
`workspace_id` (fail-closed on missing record or error). A rejected scope is
named in a `SubscribeError` frame and is excluded from `subscribed_workspaces`
and the `ws_connection_workspaces` index; the bus delivery filter
(ws.rs:522-541) and telemetry broadcast key on that list, so a same-tenant
non-member receives neither workspace bus messages nor telemetry — and no
replay, since replay iterates only authorized workspaces. This is real
enforcement, not audit-only.

## REST telemetry endpoint (gap 64d1be477f27ebf27ec73076)

`GET /api/v1/workspaces/:workspace_id/activity` (`workspace_activity_handler`,
crates/gyre-server/src/api/activity.rs) is wired in api/mod.rs and registered in
the ABAC RouteResourceMapping (`"activity"` resource, workspace path param) —
`scripts/check-abac-route-registry.sh` passes with no exemption-file changes
(diff of `scripts/*-exemptions.txt` is empty). The handler's per-handler
authorization mirrors the sibling `list_workspace_messages` exactly: Admin
bypass, API-key users require a `WorkspaceMembershipRepository` record, agent
tokens require the agent record's workspace to match, 403 otherwise. It is
backed by `state.telemetry_buffer.list_since` (epoch milliseconds, `created_at
> since` so entries `<= since` are excluded and 1-ms-newer ones included; `limit`
honored, default 100). The `ActivityEventData` mapping follows the spec table
exactly (event_id ← id, agent_id ← payload["agent_id"] with Agent-origin
fallback, event_type ← `MessageKind::ag_event_type`, description ←
payload["tool_name"] or kind name, timestamp ← created_at); the new
`ag_event_type` maps the seven telemetry kinds per the spec's MCP Integration
table (AgentError → ERROR) with a `Custom(kind wire name)` fallback. The
unscoped `/api/v1/activity` and the WS `ActivityQuery` path are untouched by
the diff (additions only).

## Verification performed

- `cargo check -p gyre-server` and `cargo check -p gyre-server --tests`: pass
  (type-checks the new production code and the new test modules).
- `cargo test -p gyre-common --lib` focused runs: pass, including
  `kind_ag_event_type_mapping_matches_spec` and the TelemetryBuffer suite.
- Invariant scripts: check-abac-route-registry, check-arch,
  check-abac-exempt-handlers, check-dead-message-kinds, check-inert-enforcement,
  check-forged-scope-fields, check-migration-versions — all pass.
- Test teeth (static): the leak probe `ws_subscribe_rejects_same_tenant_non_member`
  fails against the pre-fix tenant-match-only handler (no SubscribeError within
  5s → `expect` panics; the 300ms no-frame assertion would observe the leak),
  and `workspace_activity_rejects_same_tenant_non_member` fails if the handler
  serves the buffer without the membership check (200+rows instead of 403).
- CI-repair probe (scripts/test_dev_cargo_clean.py, cargo 1.100 layout change):
  the test's exact sequence replicated manually on cargo 1.99 — good passes,
  mutant fails 101, dependency sentinel retained across helper cleans.
- Server test execution is blocked by the sandbox cargo guard (exit 64, 4 GiB
  budget) — infrastructure restriction, not a code defect. Required probes are
  recorded in `/tmp/stage/review-evidence/server-tests-for-verifier.md` for the
  independent verifier: the two ws_subscribe_* leak tests, the four
  workspace_activity_* tests, the full gyre-server suite (presence tests were
  correctly re-seeded with memberships), and the dev-controller
  `test_dev_cargo_clean.py` unittest on cargo 1.100.

## Non-blocking observations

- `limit` is not clamped on the new endpoint (the sibling messages route clamps
  at 500). Not a spec violation: the spec fixes only the default (100) and the
  TelemetryBuffer caps a workspace at 10,000 entries; the response is
  membership-gated either way.
- A same-tenant Admin non-member on the new REST route is allowed by the
  handler's Admin bypass but denied 403 by the ABAC builtin
  `workspace-membership-required` (priority 940, no admin exemption) before the
  handler runs — identical to the sibling workspace-scoped message routes, so
  the "preserving the per-handler authorization pattern" requirement is met and
  the effective behavior is the stricter (deny), which matches the spec's intent.

Coverage rows 9, 11, 13 in specs/coverage/system/message-bus.md were updated
with accurate source references for exactly these two gaps; no unrelated rows
were touched.
