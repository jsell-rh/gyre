# Review: task-252 — Workspace-scoped telemetry access

**Candidate:** `07259b463bdce4abe689a5e8d20d5643fcf04b2c` (branch `pipeline/task-252/77e78a69908b43da91ffdfc4a369f91c-1`, base `67cc03988b938c4635973761c08108cee8f8459d`)
**Round:** 3 (frontend flake repair round; crates/ byte-identical to the base merge)
**Verdict:** **Approved** — no blocking findings.

## What was reviewed

Full diff base..HEAD (gyre-common `ag_event_type` mapping, ws.rs Subscribe
membership gate + leak-probe tests, `workspace_activity_handler` + tests,
ABAC registry entry, coverage rows 9/11/13, the WorkspaceSettings frozen-clock
test repair, and the prior-round `scripts/test_dev_cargo_clean.py` CI repair).

## Findings

None blocking. Evidence and probe logs: `/tmp/stage/review-evidence/`
(summary: `review-summary.md`).

### Subscribe membership authorization (gap 2b439cf656cf14c344afdb07) — genuine

ws.rs Subscribe authorizes per workspace: preserved Admin bypass, preserved
tenant containment, then real membership verification — users via
`WorkspaceMembershipRepository::find_by_user_and_workspace` (implemented by
SQLite, Postgres, and mem adapters), agents against their agent record's
workspace. Lookup errors fail closed. Only the authorized list becomes
`subscribed_workspaces`, and every delivery path (bus destination filter,
presence rebroadcast, replay) keys on it, so a rejected scope receives
nothing. `ws_subscribe_rejects_same_tenant_non_member` is a real leak probe:
against the pre-fix tenant-match-only handler it fails (no SubscribeError
frame arrives), and it asserts the same-tenant member still receives both
telemetry and bus messages while the non-member receives nothing.

### REST telemetry endpoint (gap 64d1be477f27ebf27ec73076) — genuine

`GET /api/v1/workspaces/:workspace_id/activity` queries
`state.telemetry_buffer.list_since` (strictly-greater epoch-ms boundary,
limit default 100) and maps to `ActivityEventData` exactly per the spec
table (event_id ← id, agent_id ← payload with Agent-origin fallback,
event_type ← `MessageKind::ag_event_type`, description ← tool_name or kind
name, timestamp ← created_at). Per-handler authorization is a verbatim
mirror of the sibling `list_workspace_messages` pattern (Admin bypass;
membership for API-key users; agent-record scoping for agent tokens; 403).
The route is registered in the ABAC RouteResourceMapping ("activity"
resource); because `resource.workspace_id` is extracted from the path
param, the seeded builtin `workspace-membership-required` Deny also fires
for non-member users at the middleware — `check-abac-route-registry.sh`
passes with no exemption changes. The unscoped `/api/v1/activity` and WS
ActivityQuery paths are unchanged.

### This round's repair (frontend countdown flake) — sound

WorkspaceSettings.test.js freezes `Date.now` (established
InvitationAccept pattern, restored in afterEach) and seeds both
time-dependent fixtures from the frozen instant; the exact-text assertion
`'Expires in 30m'` is retained, so the test still verifies the countdown.
85/85 pass in this sandbox after `npm ci` (log retained).

### Probes run (evidence retained)

- `check-abac-route-registry.sh`, `check-arch.sh`,
  `check-abac-exempt-handlers.sh`, `check-task-commit-attribution.sh` —
  all exit 0.
- `cargo test -p gyre-common --lib` — 129 passed (incl.
  `kind_ag_event_type_mapping_matches_spec` and the 11 telemetry-buffer
  tests).
- `cargo check -p gyre-server --tests` — clean (type-checks the new
  handler, gate, and all new tests).
- `npx vitest run src/__tests__/WorkspaceSettings.test.js` — 85/85.

### Deferred to the independent verifier

gyre-server test-binary link exceeds the review-stage memory budget; run
`cargo test -p gyre-server ws_subscribe`, `cargo test -p gyre-server
workspace_activity`, `python3 -m unittest scripts.test_dev_cargo_clean`
(blocked here only by the stage cargo wrapper), plus the full workspace
suite and full `npm test`.

### Non-blocking observations

- `20fb8239` (scripts/-only CI repair from the prior round) is no longer
  in the current `commits:` frontmatter; scripts/ is not product surface
  per the attribution check (which passes), and the change is covered by
  this review's diff inspection regardless.
- The WS ActivityQuery no-subscription fallback to the tenant default
  workspace predates this task and was required to remain unchanged.
