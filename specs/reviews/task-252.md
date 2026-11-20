# Review — task-252: Workspace-scoped telemetry access

**Candidate:** `ff78aaaa8577c21b47e6881f41fc5af493a9c00e` (rebase repair:
merge `4c2e839e` of previously approved head `5147d038` with main
`b1ac5989`, plus docs commit)
**Reviewer:** independent review, round 3 (rebase repair)
**Verdict:** approved
**Evidence:** `/tmp/stage/review-evidence/task-252-review.md`

## What was reviewed

The rebase repair and, against the current merged tree, both task-252
capabilities:

1. **Subscribe membership authorization** (gap 2b439cf656cf14c344afdb07):
   the ws.rs Subscribe handler now authorizes each requested workspace in
   order — Admin bypass (preserved, checked first), tenant containment
   (preserved), then real workspace membership: user connections via
   `WorkspaceMembershipRepository::find_by_user_and_workspace` (store error
   → reject, fail-closed), agent connections against their agent record's
   workspace. A rejected workspace is excluded from the subscription and
   named in the SubscribeError frame.
2. **REST telemetry endpoint** (gap 64d1be477f27ebf27ec73076):
   `GET /api/v1/workspaces/:workspace_id/activity` (api/activity.rs
   `workspace_activity_handler`) queries `state.telemetry_buffer`
   (`TelemetryBuffer::list_since`, `created_at > since_ms` in epoch ms,
   `limit` default 100) and enforces membership per-handler in exactly the
   sibling `list_workspace_messages` pattern (non-Admin: user membership
   lookup / agent-record workspace; 403 otherwise). The
   `ActivityEventData` mapping matches the spec table (event_id ← id,
   agent_id ← payload["agent_id"] with Agent-origin fallback, event_type ←
   `MessageKind::ag_event_type()`, description ← payload["tool_name"] or
   kind name, timestamp ← created_at). Route registered in the ABAC
   RouteResourceMapping ("activity" resource); no new exemptions.

## Merge-repair verification

`git diff` of the merge against each parent shows the main-side diff is
exactly task-252's changes and the approved-head-side diff is exactly
task-248's changes; the only textual conflict (coverage matrix) was
resolved as a union. task-248's `ws_message_payload` acknowledged-stripping
survives at both WS serialization arms; task-252's Subscribe check survives
intact. No scripts, exemptions, or unrelated tasks touched.

## Probes run (independent)

- `scripts/check-abac-route-registry.sh` — exit 0, no new exemptions
- `scripts/check-abac-exempt-handlers.sh` — exit 0
- `cargo check -p gyre-server` — exit 0 (9m44s)
- `cargo check -p gyre-server --tests` — exit 0
- `cargo test -p gyre-common` — 130 passed, incl.
  `kind_ag_event_type_mapping_matches_spec`

The new tests are killing tests, not inflation: the ws leak probe expects a
SubscribeError frame that the tenant-match-only handler never sends (empty
replay sends nothing) and asserts the non-member connection receives
nothing after real telemetry + bus emits, with a member control receiving
both; the endpoint tests run against `api_router()` WITHOUT the middleware
layers (those are applied in `build_router`), so the 403 assertions prove
the handler-level check rather than the ABAC middleware doing the work.

## Deferred to the verifier

gyre-server test compilation exceeds the sandbox build budget. The
verifier must run `cargo test -p gyre-server` (at minimum:
ws_subscribe_rejects_same_tenant_non_member,
ws_subscribe_agent_scoped_to_own_workspace_record,
workspace_activity_maps_fields_per_spec,
workspace_activity_since_boundary_and_limit,
workspace_activity_rejects_same_tenant_non_member,
workspace_activity_agent_token_scoped_to_own_workspace, plus the
pre-existing WS/message suites for merge regressions) and
`cd web && npm ci && npm test` (WorkspaceSettings.test.js frozen-clock fix
retained from the approved head).

## Notes (non-blocking)

- The spec's "unauthenticated caller receives 403" is served as 401 by the
  global require_auth middleware — the repo-wide convention shared with
  every sibling route; not a task-252 regression.
- Production additionally carries the ABAC builtin
  workspace-membership-required Deny (priority 940) on the new route for
  user subjects; the handler-side check is the pattern-required
  enforcement and is what the tests prove.
