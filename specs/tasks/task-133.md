---
title: "Implement ABAC MCP permission resources for agents"
spec_ref: "abac-policy-engine.md §MCP Integration"
depends_on:
  - task-128
  - task-129
progress: ready-for-review
coverage_sections:
  - "abac-policy-engine.md §MCP Integration"
commits: ["cd372a6cab32dbc7e0ba8ac33600416591ca64e8"]
---

## Spec Excerpt

From `abac-policy-engine.md` §MCP Integration:

Agents don't interact with the policy engine directly. The MCP server enforces policies transparently on every tool call. However, agents can query their own permissions:

| Resource | Purpose |
|---|---|
| `permissions://me` | "What actions am I allowed to perform in my scope?" |
| `permissions://check?action={action}&resource={type}` | "Can I do this specific thing?" (pre-flight check) |

This lets agents avoid wasting tool calls on actions they'll be denied.

## Implementation Plan

1. **Add `permissions://me` MCP resource in `mcp.rs`:**
   - Register in resources/list response
   - On resources/read: extract agent's subject attributes from AuthenticatedAgent
   - Call the ABAC evaluation engine's effective permissions query for the agent's scope
   - Return list of allowed actions per resource type in the agent's scope
   - Format: JSON array of `{ resource_type, allowed_actions: [] }`

2. **Add `permissions://check` MCP resource in `mcp.rs`:**
   - Register in resources/list with URI template
   - Parse query params: `action={action}&resource={type}`
   - Evaluate single policy check: can this agent perform this action on this resource type?
   - Return `{ allowed: bool, matched_policy: string|null }`

3. **Wire to ABAC evaluation engine:**
   - Both resources call the evaluation engine from task-129
   - Use agent's JWT claims as subject attributes
   - Environment attributes from current context (time, budget_remaining)

## Acceptance Criteria

- [ ] `permissions://me` resource returns agent's effective permissions
- [ ] `permissions://check?action=push&resource=repo` performs pre-flight check
- [ ] Both resources listed in MCP resources/list
- [ ] Returns correct results based on ABAC policy evaluation
- [ ] Agent can only query its own permissions (not other agents')
- [ ] `cargo test --all` passes

## Agent Instructions

Read `specs/system/abac-policy-engine.md` §MCP Integration. MCP server: `gyre-server/src/mcp.rs`. Look at existing MCP resources (e.g., `briefing://`, `notifications://`, `agents://`) for the pattern of registering and reading resources. ABAC evaluation: depends on task-129's evaluation engine — check `gyre-server/src/policy_engine.rs` or equivalent. Agent auth: `AuthenticatedAgent` from `auth.rs` provides the agent's identity context. The `permissions://check` resource uses query params in the URI — check how other resources handle parameterized URIs.

## Shipped

Both §MCP Integration resources are live in the MCP server (`crates/gyre-server/src/mcp.rs`, commit cd372a6c), answered by the real task-129 evaluation engine against the live policy set — no stand-in logic.

### Behavior

- **`permissions://me`** — registered in `resources/list`; on `resources/read` it builds the CALLER's ABAC attribute context (`agent_permission_context`) and evaluates the real engine (`policy_engine::evaluate`) for every canonical resource type × the spec's §Attributes action vocabulary (read, write, delete, approve, spawn, push, merge, escalate, generate), returning `[{resource_type, allowed_actions: []}]`.
- **`permissions://check?action={action}&resource={type}`** — registered with the URI template `permissions://check?action={action}&resource={type}`; parses both query params (missing params → explicit usage error, not a silent deny) and returns `{allowed: bool, matched_policy: string|null}` from a single real evaluation.
- **Own-permissions-only** — there is no subject parameter on either resource; the subject context is derived exclusively from the caller's `AuthenticatedAgent` (auth-context type/id/tenant_id/global_role with role-SET semantics, the reserved non-spoofable system identity, allowlisted agent-JWT claims + `wl_stack_hash` normalization gated on `subject.type == agent`, membership/team stores for user identities).
- **Scope** — the resource scope chain comes from the caller's own resolved scope (`agent_secret_scope`: JWT claims → agent record → task, tenant from the governing workspace record, never fabricated), and `env.budget_remaining` from the real budget stores; `action`/`resource.type`/`env.time` are injected by the engine.
- **Vocabulary derivation** — `ResourceResolver::distinct_resource_types` (+ `abac_middleware::all_resource_types()`) exposes the live route registry's resource-type vocabulary, so the `permissions://me` probe set cannot drift from the resource types the middleware actually enforces with.

### Test evidence

Four focused tests in `mcp.rs` (module `tests`):

- `mcp_resources_list_includes_permission_resources` — both resources listed, `uriTemplate` correct.
- `permissions_resources_evaluate_real_policies_for_agent_jwt` — a real Gyre-minted worker JWT (via `register_worker`): `permissions://check?action=read&resource=repo` → `{allowed: true, matched_policy: "builtin-agent-scoped-access"}`; `?action=approve&resource=spec` → `{allowed: false, matched_policy: "builtin-require-human-spec-approval"}` (the immutable human-only Deny beats the agent role Allow); `permissions://me` repo entry carries read/write/approve but not push/merge/spawn/delete/escalate/generate, and the spec/persona entries exclude approve (persona-human-approval).
- `permissions_check_is_caller_scoped` — two JWT callers, one allowed `push` on `repo` by a `subject.id`-keyed custom Allow, the other denied the same action with `matched_policy: null` (kills any fixed-subject or subject-param bug).
- `permissions_check_requires_both_params` — missing param and unknown `permissions://` resource produce explicit errors.

### Verifier-owned probes

This sandbox's build wrapper blocks gyre-server test compilation (4 GiB budget); the tests type-check (`cargo check -p gyre-server --lib --tests` clean, `cargo fmt --check` clean, check-arch / check-mcp-write-tools / check-abac-route-registry pass) but were not executed here. The independent verifier must run:

```
cargo test -p gyre-server --lib permissions
# or the full gate:
cargo test --all
```

Mechanical checks run locally: `scripts/check-arch.sh`, `scripts/check-mcp-write-tools.sh`, `scripts/check-abac-route-registry.sh` — all pass; no new routes, tools, migrations, or exemption entries.
