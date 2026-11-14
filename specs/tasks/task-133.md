---
title: "Implement ABAC MCP permission resources for agents"
spec_ref: "abac-policy-engine.md §MCP Integration"
depends_on:
  - task-128
  - task-129
progress: not-started
coverage_sections:
  - "abac-policy-engine.md §MCP Integration"
commits: []
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
