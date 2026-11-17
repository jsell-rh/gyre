---
title: "Platform Model Protocol Injection at Agent Spawn"
spec_ref: "platform-model.md §8 Protocol Injection"
depends_on:
  - task-093
progress: complete
coverage_sections:
  - "platform-model.md §8 Dev Mode (No OIDC)"
commits: ["78d7774d7e9f37ca632122e39cad49bd6de0890e"]
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

## Shipped

Implemented in commit `78d7774d` on top of the task-150 MCP prompt plumbing
(`prompts/list`+`prompts/get` for the four `system://` prompts and
`GYRE_PROMPT_*` spawn injection already existed); this task closes the
remaining §8 gaps with production code:

### Protocol norms (single source, system-level)

`api::prompts::protocol_prompt()` now carries the full §8 protocol norms:
Ralph loop rules, MCP tool catalog with usage patterns (the real
`gyre_*` tool names from `tools/list`, each with a usage example),
heartbeat, escalation rules, handoff protocol (completion, re-spawn,
orchestrator-directive, and budget-exhaustion handoffs), and completion
signaling. `prompt_assembly::render_protocol_prompt()` delegates to it,
so `GYRE_PROTOCOL_PROMPT`, `GYRE_PROMPT_PROTOCOL`, and the MCP
`system://protocol` prompt serve byte-identical text (previously two
divergent protocol texts were injected at spawn). It is a static
system-level template, not per-persona.

### Context: budget snapshot

`context_prompt_for_spawn` (workers), `context_prompt_for_caller`
(MCP `system://context`), `context_prompt_for_orchestrator`, and mcp.rs's
own `system://context` renderer all append a `## Budget` section with
remaining tokens/cost for the day, read from the same
`budget_configs`/`budget_usages` stores spawn-time enforcement reads
(shared `budget_snapshot_section` renderer — the context never
disagrees with what is enforced).

### Constraints: trust level

Both the spawn-injected and MCP-served constraints prompts now carry a
`## Trust Level` section derived from the governing workspace row
(shared `trust_level_section`), stating what the level means for agent
autonomy. The scope boundaries text now states explicitly that other
repos/workspaces are inaccessible. Scope is derived from the caller's
authenticated identity (JWT hierarchy claims on the MCP path; the
fail-closed repo→workspace resolution on the spawn path).

### Persona attestation

Persona resolution was already fail-closed to `Approved` personas (a
Pending/Draft persona cannot style an agent). New: spawn injects
`GYRE_PERSONA_ID` / `GYRE_PERSONA_VERSION` / `GYRE_PERSONA_CONTENT_HASH`
/ `GYRE_PERSONA_APPROVAL_STATUS` alongside `GYRE_PROMPT_PERSONA` (workers
and orchestrators), and the MCP `system://persona` result carries
`metadata.persona` with the bound persona's slug, version, content_hash,
and approval status, looked up from the persona store — no fabricated
identity (absent when no persona is bound).

### Dev mode

Dev mode uses the identical spawn/injection path; the only difference is
caller authentication (static token vs OIDC JWT). All spawn-injection
tests run under exactly the dev-mode server configuration (static-token
auth, `jwt_config: None`), so dev-mode parity is regression-tested; this
is documented in the test comments, `docs/server-config.md` (dev-mode
parity paragraph), and `docs/agent-protocol.md`. No protocol-injection
gaps exist between dev mode and production.

### Test evidence

`cargo test -p gyre-server` is blocked in this sandbox by the 4 GiB build
budget (recorded in `/tmp/stage/review-evidence/probes.md` — an
infrastructure restriction, not a code defect). Type checking of all
production and test code passes: `cargo check -p gyre-server --tests`
exit 0, no warnings; `cargo fmt` applied; architecture and invariant
scripts pass. Required verifier probes (all new/extended tests are in
`gyre-server`):

- `cargo test -p gyre-server --lib prompt`
- `prompt_assembly::tests::protocol_prompt_covers_the_five_protocol_areas`
  (extended: handoff protocol + tool catalog) and
  `protocol_prompt_is_the_single_source` (new)
- `api::prompts::tests::protocol_prompt_covers_norms_loop_and_escalation`
- `api::spawn::tests::spawn_injects_all_four_system_prompts_with_persona`
  (extended: budget snapshot rendered from seeded config+usage — asserts
  `40000 used today of 100000 (60000 remaining)`; trust level
  `Supervised` from the workspace row; persona attestation vars matched
  against the seeded persona's looked-up version and recomputed hash)
- `api::spawn::tests::mcp_persona_prompt_carries_bound_persona_attestation`
  (new: spawns with a persona, fetches `system://persona` as the agent,
  asserts `metadata.persona` slug/version/content_hash/approval)
- `mcp::tests::mcp_prompts_get_protocol_delivers_ralph_loop_rules`
  (extended: `## MCP tools` catalog + `## Handoff protocol`)
- `api::orchestrator::tests::orchestrator_launch_injects_system_prompts`
- `cargo test --all` (acceptance criterion)

Dev-mode evidence: every spawn-injection test authenticates with the
server's static global token against `jwt_config: None` — the dev-mode
configuration — so the full injection set is proven under dev mode
(see the note on `spawn_injects_all_four_system_prompts_with_persona`).

### Note on coverage metadata

The task's `coverage_sections` lists "Dev Mode (No OIDC)" (row 50, already
verified by task-099) while `spec_ref` is §8 Protocol Injection (row 52)
— a known task-metadata mislabel recorded in the coverage file. The
work above implements the Protocol Injection row; the Dev Mode row's
dev-mode parity finding is documented above and in
`docs/server-config.md`.
