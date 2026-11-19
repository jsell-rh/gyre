---
title: "Workspace-scoped telemetry access: Subscribe membership authorization and the REST activity endpoint"
task: task-252
candidate: d504cbe37ca9a12cc67e0a3b3b4577452d387909
base: ee03c29984b19a2bbcc9b6f2370e8babb6e61e92
reviewer: independent review, round 5 (post task-274-base rebase)
verdict: approved
date: 2026-10-08
---

# Review: task-252 (candidate d504cbe3, base ee03c299)

## Scope

`git diff ee03c299..d504cbe3`: ws.rs (+373, Subscribe membership gate + leak-probe
tests), api/activity.rs (+425, workspace_activity_handler + mapping + tests),
api/mod.rs (+5, route), abac_middleware.rs (+5, RouteResourceMapping),
gyre-common/src/message.rs (+43, MessageKind::ag_event_type + mapping test),
coverage/task docs, and a frontend test flake repair (frozen Date.now in
WorkspaceSettings invitation-expiry test — assertions unchanged, stabilizing only).

## Findings: none

Both capabilities are real production implementations matching the canonical spec
(specs/system/message-bus.md §Telemetry Store / §API > Telemetry):

1. **Subscribe membership authorization** (gap 2b439cf656cf14c344afdb07). The
   ws.rs Subscribe handler now authorizes each requested workspace in order:
   preserved Admin bypass, preserved tenant containment, then real workspace
   membership — user connections via
   `WorkspaceMembershipRepository::find_by_user_and_workspace` (fail-closed on
   lookup error), agent connections scoped to their own agent record's workspace.
   Rejected scopes get the SubscribeError frame, no subscription, no delivery
   (bus messages, telemetry, or replay). Global-token connections keep Admin
   bypass; all pre-existing tests authenticate with the global token or seed the
   required membership (the presence test now seeds it). The leak-probe test
   (`ws_subscribe_rejects_same_tenant_non_member`) asserts the non-member
   connection receives NOTHING after telemetry + a workspace-scoped bus message
   are emitted — it fails against the tenant-match-only handler, so it kills the
   defect it targets.

2. **REST telemetry endpoint** (gap 64d1be477f27ebf27ec73076).
   `GET /api/v1/workspaces/:workspace_id/activity` is backed by
   `state.telemetry_buffer.list_since` (strictly-greater epoch-ms `since`
   boundary — entries `<= since` excluded, 1ms newer included; `limit` honored,
   default 100), per-handler membership enforcement byte-identical in structure
   to the sibling `list_workspace_messages` (403 for non-members, agent tokens
   scoped to their agent record's workspace, Admin bypass), and the exact spec
   field mapping (event_id ← id, agent_id ← payload["agent_id"] with Agent-origin
   fallback, event_type ← `MessageKind::ag_event_type` incl. AgentError → ERROR
   per the §MCP Integration table, description ← payload["tool_name"] or kind
   name, timestamp ← created_at). The route is registered in the ABAC
   RouteResourceMapping ("activity" resource, :workspace_id param) —
   `scripts/check-abac-route-registry.sh` exit 0, no exemption-file changes. The
   unscoped /api/v1/activity and WS ActivityQuery paths are unchanged.

3. **Rebase repair** (merge 5e4a506e onto ee03c299). The merge brings only
   main-side files (api/repos.rs verbatim from main, docs, main's
   test_dev_cargo_clean.py fix, task-259/273/274 specs); no task-252
   implementation file changed in the merge, and d504cbe3 on top is docs-only.
   Coverage rows 9/11/13 + header in message-bus.md updated accurately with
   source evidence; no unrelated rows touched.

## Evidence (this sandbox, candidate tree)

- `scripts/check-abac-route-registry.sh` exit 0 (no new exemptions).
- `cargo check -p gyre-server` exit 0 (7m51s, merged tree).
- `cargo check -p gyre-server --tests` exit 0 (4m01s — the new ws.rs and
  activity.rs test modules type-check).
- `cargo test -p gyre-common kind_ag_event_type_mapping_matches_spec` — 1 passed.
- Static invariant scripts all exit 0: check-arch, check-abac-exempt-handlers,
  check-in-memory-state-stores, check-forwarded-header-trust,
  check-forged-scope-fields.
- Full evidence: /tmp/stage/review-evidence/task-252-round5-review.md.

## Deferred to the independent verifier (executor wrapper blocks >4 GiB server
test compilation in this sandbox — infrastructure restriction, not a defect)

- `cargo test -p gyre-server --lib ws_subscribe_rejects_same_tenant_non_member`
- `cargo test -p gyre-server --lib ws_subscribe_agent_scoped_to_own_workspace_record`
- `cargo test -p gyre-server --lib workspace_activity` (4 tests)
- Full workspace suite, `npm test`, all-target clippy, exact-head GitHub checks.
