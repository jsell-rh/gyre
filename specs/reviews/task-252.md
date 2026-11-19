# Review — task-252 (Workspace-scoped telemetry access: Subscribe membership authorization and the REST activity endpoint)

Spec: `specs/system/message-bus.md` §Design/Storage/Telemetry Store (Subscription authorization) and §API/Telemetry.
Candidate under review: `0da7a002fa117f47d4f39d205adfad54d0ee1da9` (base `05024b3e`, rebase-repair round for durable finding 3008f070a71f46fbbfb3bf6e75fb4c2a; merge commit `26eefb2d`).
Product commits: `81910e02` (feat), `d2fe75e4` (test sync), `9fa69115` (web test flake), `20fb8239` (dev-script test repair).
Verdict: **approved** — no findings. Full probe log: `/tmp/stage/review-evidence/task-252-review-round3.md`.

## Round 3 (rebase repair onto task-254 base)

Scope: the base..candidate diff contains only task-252's own changes — the
merge with `05024b3e` is a clean union (single conflict in the
`specs/coverage/system/message-bus.md` "Last audited" header, resolved by
combining the task-252 and task-254 audit sentences chronologically;
task-254's `DefaultBodyLimit::max` on POST messages and signing-key history
test file are preserved alongside task-252's route, ABAC entry, and Subscribe
gate).

Probes run on the exact candidate checkout:

- `scripts/check-abac-route-registry.sh` → OK (new activity route registered, no exemption-file growth).
- `scripts/check-arch.sh`, `scripts/check-mem-port-contracts.sh`, `scripts/check-abac-exempt-handlers.sh` → OK.
- `cargo test -p gyre-common --lib` → 130 passed, 0 failed (incl. `kind_ag_event_type_mapping_matches_spec`).
- `cargo check -p gyre-server` and `cargo check -p gyre-server --tests` → clean.
- `cargo fmt -p gyre-server -p gyre-common -- --check` → clean.

Verified genuine:

- **Subscribe membership gate (ws.rs)**: Admin bypass and tenant containment
  preserved in order; user connections verify via
  `WorkspaceMembershipRepository::find_by_user_and_workspace` (fail-closed on
  lookup error), agent connections against their agent record's workspace.
  Rejected scopes get the SubscribeError frame, are excluded from the
  connection workspace index, and therefore receive neither bus messages,
  telemetry, nor replay — the buffer's lack of tenant isolation is now
  compensated exactly as the spec's Subscription-authorization paragraph
  requires. Existing replay tests (global token → Admin bypass) and presence
  tests (memberships seeded) remain consistent with the new contract.
- **REST endpoint (api/activity.rs `workspace_activity_handler`)**: mirrors
  the sibling `list_workspace_messages` per-handler authorization (403 for
  same-tenant non-members; agent tokens scoped to their agent record's
  workspace); backed by the real `TelemetryBuffer` with `since` in epoch ms
  (the pre-existing, unchanged `list_since` `created_at > since` filter gives
  the exact `<= since` excluded / 1ms-newer included boundary) and `limit`
  (default 100). The ActivityEventData mapping follows the spec table
  field-for-field via the new `MessageKind::ag_event_type` (AgentError→ERROR,
  non-telemetry kinds → Custom wire name — unit-tested in gyre-common).
- **Unchanged paths**: the unscoped `/api/v1/activity` handler and the WS
  ActivityQuery arm have zero diff lines.
- **Tests are mutation-resistant**: the leak probe asserts the SubscribeError
  frame for a non-member (no frame exists on the pre-fix handler → timeout
  failure) plus member-positives and a nothing-delivered negative; the
  endpoint probe pairs the non-member 403 with a same-tenant member 200 so
  the 403 is attributable to the membership check; the boundary test pins
  `<= since` exclusion, 1ms inclusion, and limit. None are self-confirming.

Deferred to the independent verifier (gyre-server codegen is blocked for the
review stage by the build wrapper — mechanical deferral): the focused server
tests `ws_subscribe_rejects_same_tenant_non_member`,
`ws_subscribe_agent_scoped_to_own_workspace_record`, the four
`workspace_activity_*` tests, the full ws module and activity module suites,
full workspace suite + all-target clippy, and `cd web && npm ci && npm test`.

Test-inflation check: none found. Fixture-only changes to existing tests
(presence membership seeding, Date.now freeze, cargo-clean sentinel) are
additive adjustments to the new contract or flake repairs, not weakened
assertions.
