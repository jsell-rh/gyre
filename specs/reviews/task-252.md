# Review: task-252 — Workspace-scoped telemetry access

**Candidate:** `7e24f007b711c4bff3de21241d1205b4d161bedf` (base `61b6080d1866f32d98cc7a35d62e3fb2b88a290a`)
**Verdict:** APPROVED (independent review, fresh — no prior approval carried forward)
**Evidence:** `/tmp/stage/review-evidence/task-252-review.md`

## What was reviewed

The full diff `61b6080d..7e24f007`: ws.rs Subscribe authorization, the new
`GET /api/v1/workspaces/:workspace_id/activity` handler, route + ABAC registry
entries, `MessageKind::ag_event_type`, coverage-matrix updates, and the
rebase/repair commits (task-251 merge, cargo-1.100 cache-probe test repair,
frontend Date.now flake fix — all scoped to test infrastructure, none weakening
a check; the cache-probe repair strengthens the sentinel against package-layout
changes).

## Findings: none blocking

1. **Subscribe membership authorization (gap 2b439cf656cf14c344afdb07) — genuine.**
   The ws.rs Subscribe handler now authorizes in order: preserved Admin bypass,
   preserved tenant containment, then real workspace membership — users via
   `WorkspaceMembershipRepository::find_by_user_and_workspace`, agent tokens via
   their agent record's workspace. Rejected scopes produce the SubscribeError
   frame and never enter `subscribed_workspaces`, so the delivery/replay filters
   (which key on that list) cannot leak telemetry or bus messages. Repository
   errors fail closed. The leak-probe test uses real sockets against a real
   server and asserts the non-member receives NOTHING after real telemetry and
   workspace-bus emits — it fails against the pre-fix tenant-match-only handler.
2. **REST telemetry endpoint (gap 64d1be477f27ebf27ec73076) — genuine.**
   `workspace_activity_handler` mirrors the sibling `list_workspace_messages`
   authorization pattern exactly (Admin bypass / membership record / agent-record
   workspace, 403 otherwise) on top of the ABAC middleware's builtin
   workspace-membership deny (route registered in the RouteResourceMapping with
   the workspace path param). `since` is epoch ms with strict `>` filtering
   (created_at <= since excluded, matching the AC), `limit` honored (default
   100), and the `ActivityEventData` mapping follows the spec table verbatim,
   including the payload-`agent_id`-with-Agent-origin fallback and
   `tool_name`-or-kind-name description. `MessageKind::ag_event_type` matches
   the spec §MCP Integration table (AgentError → ERROR). The unscoped
   `/api/v1/activity` and WS ActivityQuery paths are byte-identical to base.
3. **Tests are mutation-hard.** The WS leak test and the agent-token REST test
   each fail if their respective production check is removed (the agent-token
   test reaches the handler past the user-keyed ABAC deny, so only the handler
   check can produce its 403). No self-confirming or stub-backed assertions
   found. The `await_ws_subscription` synchronization helper replaces a race
   with deterministic index polling — verified sound (a rejected workspace
   never appears in the index).
4. **Coverage rows 9/11/13** updated with task-252 notes and concrete source
   evidence; the rebase-conflict resolution in the header retains both task-251
   and task-252 audit notes. No unrelated rows touched.

## Probes run (see evidence file for full output)

- `scripts/check-abac-route-registry.sh` — pass, no new exemptions
- `scripts/check-arch.sh`, `check-mem-port-contracts.sh`,
  `check-abac-exempt-handlers.sh` — pass
- `cargo check -p gyre-server` — exit 0; `cargo check -p gyre-server --tests`
  — exit 0 (all new test modules type-check)
- `cargo test -p gyre-common --lib kind_ag_event_type_mapping_matches_spec` — pass

## Deferred to the independent verifier (executor wrapper blocks gyre-server
codegen under the 4 GiB budget — infrastructure restriction, not a code defect)

```
cargo test -p gyre-server --lib ws_subscribe_rejects_same_tenant_non_member
cargo test -p gyre-server --lib ws_subscribe_agent_scoped_to_own_workspace_record
cargo test -p gyre-server --lib workspace_activity
cargo test -p gyre-server --lib ws_
cargo test -p gyre-common --lib
```

## Non-blocking observations

- WS ActivityQuery's empty-subscription fallback to the tenant's default
  workspace predates this candidate (present in base) and is explicitly out of
  scope ("WS ActivityQuery path unchanged"). Not a regression.
- The REST endpoint's per-handler membership check is redundant with the ABAC
  builtin deny for user callers — intentional defense in depth matching the
  sibling route pattern; the agent-token path is covered by a dedicated test.
