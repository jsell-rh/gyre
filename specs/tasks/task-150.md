---
title: "Implement MCP Prompts (prompts/list, prompts/get) and persona injection at spawn"
spec_ref: "platform-model.md §4.3"
depends_on: [task-140, task-141]
progress: ready-for-review
coverage_sections:
  - "platform-model.md §4.3 MCP Prompts (Injected at Agent Startup)"
commits: ["894fe8b736994bc00ce6a0f8a016cdf0816d6525", "010dad798e075a8557f543bffc89b252b279da63", "12c5fe5e5d14feed60150756091094f4d9a0f576", "727befd94d89718cc3fe45b45a4c307da3277875", "30fba783f1fb1ab90a52cb7ce658970eb32d3780"]
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

### MCP prompts/list + prompts/get (mcp.rs, 30fba783f)

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
  task load is containment-checked against the caller's scope (repair
  12c5fe5e5): a task in another workspace (or, for a repo-scoped agent,
  another repo) is rejected instead of rendered.
- `system://constraints` — workspace + tenant budget limits with remaining
  budget, agent lifetime cap, and workspace/tenant scope boundaries.

### Spawn injection (api/spawn.rs)

Optional `persona_slug` request field resolves the persona through the
repo → workspace → tenant chain, fails closed (400) on unknown or
unapproved slugs, and persists the binding as the agent's `persona_id`.
The four prompts are injected into the container env as `GYRE_PROMPT_PERSONA`
(only when a persona is bound), `GYRE_PROMPT_PROTOCOL`,
`GYRE_PROMPT_CONTEXT`, and `GYRE_PROMPT_CONSTRAINTS` — content identical
to what `prompts/get` serves. `meta_spec_set_sha` follows task-116's
approved semantics (reconciliation 727befd94): the spawn response carries
the SHA-256 of the assembled meta-spec prompt set — exactly the set
delivered as `GYRE_META_SPEC_SET_SHA`/`GYRE_META_SPEC_PROMPT` — null when
the assembled set is empty; no value is fabricated.

### Verification repair: bearer-token merge drift (894fe8b73)

This round merged assignment base `cfeefbda` (task-091 HSI + task-107
Fulcio signing boundary), which added a required `bearer_token:
Option<String>` field to `auth::AuthenticatedAgent`. The three task-150
MCP prompt test initializers (`agent_auth_with_task` and the two
user-token auths in the prompts suites) still constructed the struct
without the field, failing `cargo check -p gyre-server --tests` with
E0063 at mcp.rs:5966/6476/6506. All three now set `bearer_token: None`
(header-token auth is irrelevant to `handle_prompt_get` — those handlers
never sign). No production behavior changed.

### Test evidence (this round)

- `SKIP_WEB_BUILD=1 cargo check -p gyre-server --tests` — exit 0
  (evidence `/tmp/stage/review-evidence/task-150-bearer-token-repair.md`).
- `rustfmt --check crates/gyre-server/src/mcp.rs` — clean.
- `scripts/check-arch.sh` — passes.

gyre-server test compilation is blocked in the implement sandbox by the
4 GiB build wrapper; the independent verifier must run
`SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib mcp_prompts` and
`SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib spawn` (prior round:
157 passed, 0 failed across `mcp::tests::`, `api::spawn::tests::`,
`api::prompts::`; this round's change only adds a `None` field to three
test auth initializers, so those suites' behavior is unchanged).
