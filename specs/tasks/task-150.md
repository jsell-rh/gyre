---
title: "Implement MCP Prompts (prompts/list, prompts/get) and persona injection at spawn"
spec_ref: "platform-model.md §4.3"
depends_on: [task-140, task-141]
progress: ready-for-review
coverage_sections:
  - "platform-model.md §4.3 MCP Prompts (Injected at Agent Startup)"
commits: []
---

## Spec Excerpt

From `platform-model.md` §4 — Agent Coordination Protocol (MCP-Based):

> ### MCP Prompts (Injected at Agent Startup)
>
> | Prompt | Purpose |
> |---|---|
> | `system://persona` | The agent's persona prompt |
> | `system://protocol` | Interaction norms, Ralph loop definition, escalation rules |
> | `system://context` | Current task, spec refs, acceptance criteria, worktree info |
> | `system://constraints` | Budget limits, lifetime, scope boundaries |
>
> These are injected into the agent's system prompt at spawn time. The agent doesn't need to fetch them - they're part of its initial context.

## Implementation Plan

1. **Add `prompts/list` handler to MCP server** (`crates/gyre-server/src/mcp.rs`):
   - Handle JSON-RPC method `prompts/list`
   - Return the 4 prompt names: `system://persona`, `system://protocol`, `system://context`, `system://constraints`
   - Each entry includes name, description, and any argument schemas (e.g., `system://context` might take an optional `task_id`)

2. **Add `prompts/get` handler to MCP server**:
   - Handle JSON-RPC method `prompts/get` with `{name: string}` parameter
   - For `system://persona`: resolve the agent's persona via `resolve_persona()` (see `personas.rs:269-304`) and return the persona's `system_prompt`
   - For `system://protocol`: return static protocol norms text (Ralph loop definition, escalation rules) — source from a built-in template or a spec file
   - For `system://context`: build context from the agent's current task (title, spec_refs, acceptance criteria), worktree info (branch, repo path), and conversation history
   - For `system://constraints`: build from the agent's budget config (max_tokens_per_day, max_cost_per_day, max_agent_lifetime_secs), workspace scope boundaries, and remaining budget

3. **Inject prompts at spawn time** (`crates/gyre-server/src/api/spawn.rs`):
   - After agent JWT is minted, resolve the persona for this agent
   - Build the 4 prompt strings
   - Pass them as container env vars (e.g., `GYRE_PROMPT_PERSONA`, `GYRE_PROMPT_PROTOCOL`, `GYRE_PROMPT_CONTEXT`, `GYRE_PROMPT_CONSTRAINTS`) or include in the spawn response body
   - Set `meta_spec_set_sha` from the resolved persona's meta-spec set (currently hardcoded to `None`)

4. **Tests**:
   - Unit test: `prompts/list` returns 4 prompts with correct names
   - Unit test: `prompts/get` for each prompt returns non-empty content
   - Unit test: `prompts/get` for unknown prompt returns error
   - Integration test: spawn handler includes persona prompt in container env

## Acceptance Criteria

- [ ] MCP server responds to `prompts/list` with the 4 spec-required prompts
- [ ] MCP server responds to `prompts/get` for each prompt with populated content
- [ ] `system://persona` resolves via the 3-level persona fallback (repo → workspace → tenant)
- [ ] `system://context` includes current task title, spec refs, and worktree info
- [ ] `system://constraints` includes budget limits and scope boundaries
- [ ] Spawn handler resolves persona and injects prompt content into agent environment
- [ ] `meta_spec_set_sha` is set from the resolved persona's meta-spec set (not `None`)
- [ ] Tests cover all 4 prompts and error cases

## Agent Instructions

- Read `crates/gyre-server/src/mcp.rs` for the existing MCP handler structure (JSON-RPC dispatch, `tools/list`, `tools/call`, `resources/list`, `resources/read` patterns)
- Read `crates/gyre-server/src/api/spawn.rs` for the spawn handler and container env var injection
- Read `crates/gyre-server/src/api/personas.rs` for `resolve_persona()` (line ~269-304)
- Read `crates/gyre-domain/src/workspace.rs` for the Persona struct (line ~107-175)
- Follow the existing JSON-RPC pattern in mcp.rs for the new handlers
- The prompts/list and prompts/get methods are part of the MCP specification — follow the same response shape as tools/list and resources/list

## Shipped

### MCP prompts/list + prompts/get (mcp.rs)

`prompts/list` returns the four §4.3 prompts (`system://persona`,
`system://protocol`, `system://context`, `system://constraints`) with
descriptions and argument schemas (`slug` for persona, `task_id` for
context); `initialize` advertises the `prompts` capability. `prompts/get`
renders one prompt as `{description, messages: [{role, content: {type,
text}}]}`; unknown or missing `name` → JSON-RPC `-32602`.

### Prompt builders (api/prompts.rs, single source of truth)

- `system://persona` — explicit `slug` resolved nearest-scope-wins
  (repo → workspace → tenant) via `resolve_persona_in_chain`; without a
  slug, the persona bound to the agent record. Only `Approved` personas
  resolve (fail-closed); an unbound agent errors honestly instead of
  getting fabricated text.
- `system://protocol` — static platform norms: MCP-only interaction,
  heartbeat, complete-never-merge, the Ralph loop
  (observe/plan/act/monitor/reconcile), and escalation rules.
- `system://context` — task title/spec ref/description/status/priority,
  worktree branch+path, repo name/path/default branch (explicit `task_id`
  argument, else the caller's bound task; unknown task errors). Every
  task load is containment-checked against the caller's scope: a task in
  another workspace (or, for a repo-scoped agent, another repo) is
  rejected instead of rendered.
- `system://constraints` — workspace + tenant budget limits with remaining
  budget, agent lifetime cap, and workspace/tenant scope boundaries, read
  from the same `budget_configs`/`budget_usages` stores spawn-time
  enforcement reads.

### Spawn injection (api/spawn.rs)

Optional `persona_slug` request field resolves the persona through the
repo → workspace → tenant chain, fails closed (400) on unknown or
unapproved slugs, and persists the binding as the agent's `persona_id`.
The four prompts are injected into the container env as `GYRE_PROMPT_PERSONA`
(only when a persona is bound), `GYRE_PROMPT_PROTOCOL`,
`GYRE_PROMPT_CONTEXT`, and `GYRE_PROMPT_CONSTRAINTS` — content identical
to what `prompts/get` serves. Orchestrator launches
(`launch_orchestrator_process`) inject the same set from the bound
persona. `meta_spec_set_sha` on the spawn response is the SHA-256 of the
assembled meta-spec prompt set — exactly the set delivered as
`GYRE_META_SPEC_SET_SHA`/`GYRE_META_SPEC_PROMPT` — null when the assembled
set is empty; no value is fabricated.

### Merge repair: launch-ctx refactor drift (b3b97d79e)

Assignment base `cba416b46` (task-118) extracted
`launch_agent_process(state, ctx)` behind an `AgentLaunchCtx` shared by
first spawn and the Ralph-loop re-spawn. The merge kept task-150's
prompt-building block inside the extracted function but left it
referencing `spawn_agent_core` locals — E0425 x4 — and `signal_chain`'s
`try_spawn_worker` initializer lost `persona_slug` (E0063). Repaired
within the base's shared-context design: `AgentLaunchCtx` carries the
resolved `persona` and `is_interrogation`; the prompt scope/builders read
from `ctx`; the Ralph-loop re-spawn resolves the persona bound to the
agent row at first spawn (fail-closed when the binding no longer resolves
to an Approved persona); `try_spawn_worker` sets `persona_slug: None`.

### Review repair: orchestrator persona approval fail-closed (this round, review F1)

`launch_orchestrator_process` (api/orchestrator.rs) previously injected
the bound persona's `system_prompt` as `GYRE_PROMPT_PERSONA` with no
`approval_status` check — the one persona-injection site that failed
open. Reachable in production: a persona-file push (git_http.rs) resets
an Approved persona to Pending while swapping its `system_prompt`, and
the stale-agent detector restarts a dead orchestrator with the preserved
`persona_id` by calling `launch_orchestrator_process` directly, so the
replacement would have run under the new, human-unapproved prompt
content. The launch now fails closed: a bound persona that is not
`Approved` returns `launch_status: "launch_failed"` with a detail naming
the persona and approval state plus an error-level log — consistent with
`relaunch_agent_session` (workers) and `resolve_orchestrator_persona`
(first spawn). Regression test
`orchestrator_launch_fails_closed_on_unapproved_persona` simulates the
push (system_prompt swap + Pending reset), invokes the launch exactly as
`restart_orchestrator` does, and asserts both the failed outcome and that
no process was spawned (the prompt-dump script never ran).

### Test evidence

`cargo check -p gyre-server` and `cargo check -p gyre-server --lib
--tests` clean (production and test code type-checked in this sandbox);
`cargo fmt --check` passes. gyre-server test compilation exceeds this
sandbox's 4 GiB budget (build-wrapper blocked), so the verifier must run
the focused suites (also recorded in /tmp/stage/review-evidence):

- `cargo test -p gyre-server --lib mcp_prompts` — 11 tests: list
  shape+order+args, initialize capability, unknown/missing-name errors,
  persona scope-chain fallback + bound-persona + unapproved/unknown
  rejection, context task+spec+worktree + cross-scope task rejection,
  constraints budget+scope, protocol content, agent-identity requirement.
- `cargo test -p gyre-server --lib api::orchestrator::tests` — includes
  `orchestrator_launch_fails_closed_on_unapproved_persona` (F1 repair)
  and `orchestrator_launch_injects_system_prompts` (approved-persona
  injection still works), plus the spawn/restart/JWT regressions.
- `cargo test -p gyre-server --lib
  spawn_injects_all_four_system_prompts_with_persona` and
  `spawn_without_persona_injects_prompts_but_no_persona` and
  `spawn_persona_slug_fails_closed` — spawn env-dump through a real child
  process.
- `cargo test -p gyre-server --lib
  gate_failure_respawns_agent_with_fresh_context` and
  `max_iterations_blocks_task_and_fails_agent` — Ralph-loop re-spawn
  path touched by the launch-ctx repair.
- `cargo test -p gyre-server --lib
  spawn_omits_meta_spec_set_sha_without_required_meta_specs` and
  `spawn_delivers_meta_spec_prompt_to_agent_env` — meta-spec SHA
  semantics.
