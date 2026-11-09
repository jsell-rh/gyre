---
title: "Implement MCP Prompts (prompts/list, prompts/get) and persona injection at spawn"
spec_ref: "platform-model.md §4.3"
depends_on: [task-140, task-141]
progress: ready-for-review
coverage_sections:
  - "platform-model.md §4.3 MCP Prompts (Injected at Agent Startup)"
commits: ["727befd94d89718cc3fe45b45a4c307da3277875", "30fba783f1fb1ab90a52cb7ce658970eb32d3780"]
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
context). `prompts/get` renders one prompt as
`{description, messages: [{role, content: {type, text}}]}`; unknown or
missing `name` → JSON-RPC `-32602`. `initialize` advertises the `prompts`
capability.

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
  argument, else the caller's bound task; unknown task errors).
- `system://constraints` — workspace + tenant budget limits with remaining
  budget, agent lifetime cap, and workspace/tenant scope boundaries.

### Spawn injection (api/spawn.rs)

Optional `persona_slug` request field resolves the persona through the
repo → workspace → tenant chain, fails closed (400) on unknown or
unapproved slugs, and persists the binding as the agent's `persona_id`.
The four prompts are injected into the container env as `GYRE_PROMPT_PERSONA`
(only when a persona is bound), `GYRE_PROMPT_PROTOCOL`,
`GYRE_PROMPT_CONTEXT`, and `GYRE_PROMPT_CONSTRAINTS` — content identical
to what `prompts/get` serves.

### meta_spec_set_sha semantics (merge reconciliation)

This round merged the assignment base `402142b1` (task-106 jj rebase +
task-116 meta-spec prompt assembly) into the candidate branch. Where both
sides had fixed the previously-hardcoded `None` `meta_spec_set_sha`, the
resolution keeps task-116's approved semantics: the spawn response carries
the SHA-256 of the **assembled meta-spec prompt set** (required tenant →
required workspace → spec bindings), which is exactly the set delivered as
`GYRE_META_SPEC_SET_SHA`/`GYRE_META_SPEC_PROMPT` — the response attests
what was actually injected. Null when the assembled set is empty. The
candidate's parallel workspace-`meta_spec_sets` hash was dropped in favor
of that single source of truth, and `docs/api-reference.md` describes the
actual semantics. `docs/agent-protocol.md` retains the pre-existing M32
wording inherited unchanged from the base.

### Test evidence

Focused probes (logs under `/tmp/stage/review-evidence/`):

- `cargo test -p gyre-server --lib -- api::spawn::tests:: mcp::tests::
  prompt_assembly::` — **155 passed, 0 failed**, including:
  `spawn_injects_all_four_system_prompts_with_persona` (env-dump compute
  target asserts all four `GYRE_PROMPT_*` vars with task title, spec ref,
  branch, workspace scope), `spawn_without_persona_injects_prompts_but_no_persona`,
  `spawn_persona_slug_fails_closed` (unknown slug 400 "not found in scope
  chain", Pending persona 400, no orphaned agent rows), the four
  `mcp_prompts_*` suites (list shape, all four get handlers, `-32602`
  errors, scope-chain persona resolution, budget/scope constraints),
  `spawn_delivers_meta_spec_prompt_to_agent_env` (task-116 delivery guard)
  and `spawn_omits_meta_spec_set_sha_without_required_meta_specs`
  (empty-set null case, added this round).
- `docker/gyre-agent` `npm test` — 12/12 pass (cred-proxy + agent-runner).
- Invariant scripts all exit 0: check-arch, check-mcp-write-tools,
  check-abac-route-registry, check-migration-versions,
  check-byte-slice-truncation, check-relative-path-defaults,
  check-inert-enforcement, check-dead-message-kinds,
  check-mem-port-contracts, check-fail-open-ref-resolution.
- `cargo build -p gyre-server` (SKIP_WEB_BUILD=1) succeeds. A
  `cargo clippy` failure in `gyre-common` (`used as a loop counter`) is
  pre-existing on the assigned base — `gyre-common` is byte-identical to
  `402142b1` in this branch; not introduced by this task.
