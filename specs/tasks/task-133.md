---
title: "Implement ABAC MCP permission resources for agents"
spec_ref: "abac-policy-engine.md §MCP Integration"
depends_on:
  - task-128
  - task-129
progress: ready-for-review
coverage_sections:
  - "abac-policy-engine.md §MCP Integration"
commits: ["1b63ee4eb590e181e45e06f5a07aeb62574a55cd", "cd372a6cab32dbc7e0ba8ac33600416591ca64e8"]
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

Both §MCP Integration resources are live in the MCP server (`crates/gyre-server/src/mcp.rs`, commits cd372a6c + 1b63ee4e, preserved on this head and re-merged against current main at b4ded2f9 with no source conflicts in the implementation — the only conflict was the coverage matrix, resolved by keeping main's task-132 UI row alongside this task's row 18), answered by the real task-129 evaluation engine (`policy_engine::evaluate`) against the live policy set — no stand-in logic.

### Behavior

- **`permissions://me`** — registered in `resources/list`; on `resources/read` it builds the CALLER's ABAC attribute context (`agent_permission_context`) and evaluates the real engine for every canonical resource type × the spec's §Attributes action vocabulary (read, write, delete, approve, spawn, push, merge, escalate, generate), returning `[{resource_type, allowed_actions: []}]`. The resource-type vocabulary is derived from the live route registry (`abac_middleware::all_resource_types()` → `ResourceResolver::distinct_resource_types`), so the probe set cannot drift from what the middleware actually enforces.
- **`permissions://check?action={action}&resource={type}`** — registered with the URI template `permissions://check?action={action}&resource={type}`; parses both query params (missing params → explicit usage error, not a silent deny) and returns `{allowed: bool, matched_policy: string|null}` from a single real evaluation. A default deny is attributed to the seeded `builtin-default-deny` catchall — the same convention as the REST `policies/evaluate` endpoint and the decision audit trail.
- **Own-permissions-only** — there is no subject parameter on either resource; the subject context is derived exclusively from the caller's `AuthenticatedAgent` (auth-context type/id/tenant_id/global_role with role-SET semantics, the reserved non-spoofable system identity, allowlisted agent-JWT claims + `wl_stack_hash` normalization gated on `subject.type == agent`, membership/team stores for user identities).
- **Scope** — the resource scope chain comes from the caller's own resolved scope (`agent_secret_scope`: JWT claims → agent record → task, tenant from the governing workspace record, never fabricated), and `env.budget_remaining` from the real budget stores; `action`/`resource.type`/`env.time` are injected by the engine.

### Test evidence

Four focused tests in `mcp.rs` (module `tests`):

- `mcp_resources_list_includes_permission_resources` — both resources listed, `uriTemplate` correct.
- `permissions_resources_evaluate_real_policies_for_agent_jwt` — a real Gyre-minted worker JWT (via `register_worker`): `permissions://check?action=read&resource=repo` → `{allowed: true, matched_policy: "builtin-agent-scoped-access"}`; `?action=approve&resource=spec` → `{allowed: false, matched_policy: "builtin-require-human-spec-approval"}` (the immutable human-only Deny beats the agent role Allow); `permissions://me` repo entry carries read/write/approve but not push/merge/spawn/delete/escalate/generate, and the spec/persona entries exclude approve.
- `permissions_check_is_caller_scoped` — two JWT callers, one allowed `push` on `repo` by a `subject.id`-keyed custom Allow (priority 2000, matched policy pinned to it), the other denied the same action with the denial attributed to `builtin-default-deny` (kills any fixed-subject or subject-param bug).
- `permissions_check_requires_both_params` — missing param and unknown `permissions://` resource produce explicit errors.

### Rebase evidence (this attempt)

- The published PR (837, head c76aae92) conflicted with current main (44cf25bf, task-132's ABAC policy management UI). The single conflicted path was `specs/coverage/system/abac-policy-engine.md`: this branch's row 18 (MCP Integration, implemented) vs main's row 17 (UI, implemented). Resolved by keeping BOTH implemented rows and updating the header to Coverage 16/19 (2 n/a) with task-132 and task-133 audit notes merged. No implementation source conflicted; the task-132 UI files (web/src/components/abac/, api/policies.rs case-insensitive scope, web/dist) merged cleanly.
- `cargo check -p gyre-server --lib --tests` on the merged head (b4ded2f9): exit 0, clean (no warnings on gyre-server). The build script's web rebuild reproduced the committed web/dist byte-identically (git status clean after check).

### Verifier-owned probes

Server test compilation in this implement stage is mechanically blocked by the build wrapper for the 4 GiB budget — an infrastructure restriction for remote verification, not a code defect. The independent verifier must run:

```
cargo test -p gyre-server --lib permissions
cargo test -p gyre-server --lib notifications
cargo test --all
```

(`notifications` covers the prior whole-second reordering flake fixed upstream by de74a59f, an ancestor of this head; no task-133 code change was required for it.)
