---
title: "Platform Model Protocol Injection at Agent Spawn"
spec_ref: "platform-model.md §8 Protocol Injection"
depends_on:
  - task-093
progress: not-started
coverage_sections:
  - "platform-model.md §8 Dev Mode (No OIDC)"
commits: []
---

## Spec Excerpt

### Protocol Injection

When any agent is spawned, the MCP server injects these system prompts:

1. **Persona prompt** — from the persona entity (versioned, approved)
2. **Protocol norms** — Ralph loop definition, MCP tool usage patterns, escalation rules, handoff protocol
3. **Context** — current task, spec refs, acceptance criteria, worktree path, budget limits
4. **Constraints** — "you are scoped to repo X, you cannot access repo Y, your budget is N tokens"

The agent doesn't need to read AGENTS.md to know how to behave. Everything is injected.

### Dev Mode

`gyre bootstrap --dev` skips OIDC, uses static tokens, single tenant/workspace. Already partially implemented via GYRE_AUTH_TOKEN.

## Implementation Plan

1. **Structured protocol injection in MCP prompts:**
   - Extend MCP `prompts/list` to return protocol norms prompt
   - The prompt includes: Ralph loop definition, MCP tool catalog with usage examples, escalation rules, handoff protocol
   - Store protocol norms as a system-level resource (not per-persona)

2. **Context injection per agent:**
   - At spawn time, assemble the agent's context document:
     - Task details (id, spec_ref, acceptance criteria)
     - Worktree path and branch
     - Budget limits (remaining tokens/cost for the day)
     - Repo scope boundaries
   - Inject via MCP `prompts/get` with `name: "context"`

3. **Constraint injection:**
   - Derive constraints from JWT claims and workspace config:
     - Repo scope: "you can only modify files in repo X"
     - Budget: "your budget is N tokens remaining"
     - Trust level: workspace trust level affects agent autonomy
   - Inject via MCP `prompts/get` with `name: "constraints"`

4. **Persona prompt versioning:**
   - Ensure the MCP server serves the approved persona version (not draft)
   - Include persona content_hash in the prompt metadata for attestation

5. **Dev mode verification:**
   - Verify `--dev` mode already works end-to-end
   - Ensure dev mode agents still receive protocol injection (protocol norms + context)
   - Document any gaps between dev mode and production mode

## Acceptance Criteria

- [ ] MCP prompts/list returns protocol norms, context, and constraints prompts
- [ ] Protocol norms prompt includes Ralph loop, tool patterns, escalation rules
- [ ] Context prompt assembled per-agent at spawn with task/worktree/budget
- [ ] Constraints prompt derived from JWT claims and workspace config
- [ ] Persona prompt serves approved version with content_hash
- [ ] Dev mode agents receive all protocol injection
- [ ] `cargo test --all` passes

## Agent Instructions

Read `specs/system/platform-model.md` §8 "Protocol Injection" for the full spec. The MCP server is in `gyre-server/src/mcp.rs`. Look at how `prompts/list` and `prompts/get` are handled (they may already have partial implementations for persona prompts). The spawn flow in `gyre-server/src/api/spawn.rs` sets up the agent's environment — check what context is already injected. The Ralph loop definition is in `specs/system/ralph-loop.md`. Budget limits can be queried from the budget usage repository.
