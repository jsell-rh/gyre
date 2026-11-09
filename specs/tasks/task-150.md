---
title: "Implement MCP Prompts (prompts/list, prompts/get) and persona injection at spawn"
spec_ref: "platform-model.md §4.3"
depends_on: [task-140, task-141]
progress: ready-for-review
coverage_sections:
  - "platform-model.md §4.3 MCP Prompts (Injected at Agent Startup)"
commits: [30fba783f1fb1ab90a52cb7ce658970eb32d3780]
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

- [x] MCP server responds to `prompts/list` with the 4 spec-required prompts
- [x] MCP server responds to `prompts/get` for each prompt with populated content
- [x] `system://persona` resolves via the 3-level persona fallback (repo → workspace → tenant)
- [x] `system://context` includes current task title, spec refs, and worktree info
- [x] `system://constraints` includes budget limits and scope boundaries
- [x] Spawn handler resolves persona and injects prompt content into agent environment
- [x] `meta_spec_set_sha` is set from the resolved persona's meta-spec set (not `None`)
- [x] Tests cover all 4 prompts and error cases

## Shipped

Commit `30fba783f1fb1ab90a52cb7ce658970eb32d3780` on
`pipeline/task-150/f06d043bf7b14804af98d1a0de824406-1`.

### Behavior

- **`prompts/list` / `prompts/get` (mcp.rs):** two new JSON-RPC dispatcher arms.
  `prompts/list` returns the four §4.3 prompts (`system://persona`,
  `system://protocol`, `system://context`, `system://constraints`) in
  spec-table order with descriptions and argument schemas (`slug` for
  persona, `task_id` for context). `prompts/get` renders one prompt in the
  MCP result shape `{description, messages: [{role, content: {type,
  text}}]}`; unknown or missing `name` → JSON-RPC `-32602`. `initialize` now
  advertises the `prompts` capability.
- **Prompt builders (new `api/prompts.rs`, single source of truth):**
  - `system://persona` — explicit `slug` argument resolved
    nearest-scope-wins (repo → workspace → tenant) via
    `resolve_persona_in_chain`; without a slug, the persona bound to the
    agent record. Only `Approved` personas resolve (fail-closed); an
    unbound agent errors honestly instead of getting fabricated text.
  - `system://protocol` — static platform norms: interaction norms (MCP-only
    interaction, heartbeat, complete-never-merge), the Ralph loop
    (observe/plan/act/monitor/reconcile), and escalation rules (cross-repo
    via orchestrators, spec changes are human decisions).
  - `system://context` — task title/spec ref/description/status/priority/
    labels/depends_on, worktree branch+path, repo name/path/default branch
    (explicit `task_id` argument, else the caller's bound task; unknown task
    errors).
  - `system://constraints` — workspace + tenant budget limits with
    used/remaining, `max_agent_lifetime_secs` cap, and tenant/workspace/repo
    scope boundaries, read from `budget_configs`/`budget_usages` (the same
    stores `check_spawn_budget` enforces from, so prompt and enforcement
    cannot disagree).
- **Spawn injection (`api/spawn.rs`):** `SpawnAgentRequest` gains an optional
  `persona_slug`. Resolution is fail-closed (unknown or non-Approved slug →
  400, no orphaned agent rows) and the resolved persona is persisted as
  `agent.persona_id`. The four prompts are injected into the container env
  as `GYRE_PROMPT_PERSONA` (only when a persona is bound — the spec defines
  personas only for the orchestrator tiers, so an unbound worker gets no
  persona prompt rather than a fabricated one), `GYRE_PROMPT_PROTOCOL`,
  `GYRE_PROMPT_CONTEXT`, `GYRE_PROMPT_CONSTRAINTS`. `gyre_spawn_worker`
  (MCP) forwards `persona_slug`.
- **Orchestrator launches (`api/orchestrator.rs`):**
  `launch_orchestrator_process` injects the same four prompts, the persona
  prompt coming from the fail-closed-resolved bound persona; the
  orchestrator context prompt honestly records "no task / no worktree"
  (orchestrators are not task-bound).
- **`meta_spec_set_sha`:** computed with `compute_meta_spec_set_sha` for the
  governing workspace (the same hash spec approval and constraint evaluation
  use); `null` only when the workspace has no meta-spec set configured.
  Personas carry no meta-spec-set field of their own, so the workspace's
  active set — the judgment set the persona draws from — is the source.
- **Docs:** `GYRE_PROMPT_*` rows in docs/agent-protocol.md env-var table;
  prompts table + spawn row update in docs/api-reference.md.

### Test evidence

- `mcp::tests` — 9 new prompt tests: list shape/order/argument schemas;
  initialize capability; persona 3-level fallback + repo-scope override;
  bound-persona without slug + honest unbound error; unapproved/unknown slug
  rejection; context task+spec+description+worktree+repo and unknown-task
  error; constraints budget+remaining+lifetime+scope boundaries; protocol
  content + result shape; agent-identity requirement for scoped prompts;
  unknown/missing name error paths. Module total: 95 passed.
- `api::spawn::tests` — env-dump tests through a real child process:
  all-four-prompts with persona (persona prompt verbatim, persona_id bound,
  task title/spec ref/branch/workspace in the dump), without persona (other
  three injected, no `GYRE_PROMPT_PERSONA`, no fabricated persona_id),
  persona_slug fail-closed (unknown + Pending → 400, no orphaned rows),
  `meta_spec_set_sha` = SHA-256 of the stored set JSON / null without a set.
  Module total: 45 passed.
- `api::orchestrator::tests` — launch-injection test (persona prompt from
  the bound persona, all four vars, honest no-task context). Module total:
  12 passed.
- Full `cargo test -p gyre-server --lib`: **1552 passed, 0 failed**.
- Mechanical checks at HEAD: `check-arch.sh`, `check-mcp-write-tools.sh`
  (prompts are read-only, correctly outside the `needs_write` gate),
  `check-abac-route-registry.sh`, `check-inert-enforcement.sh`,
  `check-forged-scope-fields.sh`, `check-scope-literal-defaults.sh`,
  `check-fabricated-scope-defaults.sh` — all OK; `cargo fmt --check` clean;
  clippy clean on all new code.

### Notes for review

- A worker spawned without `persona_slug` receives no
  `GYRE_PROMPT_PERSONA`: the spec tables personas only for the two
  orchestrator tiers and defines no worker persona, so there is nothing
  real to inject. `prompts/get system://persona` reports the same honestly
  ("no persona bound; pass a slug").
- Multi-line prompt values are delivered as multi-line env vars (same
  delivery channel as `GYRE_CRED_*`); the reference entrypoint/agent-runner
  reads them as whole variables.

## Agent Instructions

- Read `crates/gyre-server/src/mcp.rs` for the existing MCP handler structure (JSON-RPC dispatch, `tools/list`, `tools/call`, `resources/list`, `resources/read` patterns)
- Read `crates/gyre-server/src/api/spawn.rs` for the spawn handler and container env var injection
- Read `crates/gyre-server/src/api/personas.rs` for `resolve_persona()` (line ~269-304)
- Read `crates/gyre-domain/src/workspace.rs` for the Persona struct (line ~107-175)
- Follow the existing JSON-RPC pattern in mcp.rs for the new handlers
- The prompts/list and prompts/get methods are part of the MCP specification — follow the same response shape as tools/list and resources/list
