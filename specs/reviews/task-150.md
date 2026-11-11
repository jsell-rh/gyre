# Review — task-150: MCP Prompts (prompts/list, prompts/get) and persona injection at spawn

Round: 1 (this candidate).
Reviewer scope: `git diff cfeefbda..212c4c62` — mcp.rs (+~700), api/prompts.rs (new, 476),
api/spawn.rs (+~420 production/test), api/orchestrator.rs (+140), api/mod.rs (module decl),
docs/agent-protocol.md, docs/api-reference.md, specs/coverage/system/platform-model.md row 23,
specs/tasks/task-150.md.

## Verdict

Approved. Every §4.3 acceptance criterion is implemented in production code; the new
tests are real (state-backed, containment-checking, child-process env-dump through real
dispatch), and the full candidate compiles (`cargo check -p gyre-server --tests`, exit 0).
Evidence persisted under `/tmp/stage/review-evidence/` (diff, cargo check log, invariant
script logs, attribution proof).

## What was verified (independent evidence)

- **prompts/list** (mcp.rs dispatcher arm + `prompt_definitions()`): returns exactly the
  four §4.3 names in spec-table order with descriptions and argument schemas (`slug`,
  `task_id`); `initialize` now advertises the `prompts` capability. Tested through the
  real `/mcp` HTTP route (`mcp_post` → oneshot on the real router with auth).
- **prompts/get** (`handle_prompt_get`): MCP result shape `{description, messages:
  [{role, content: {type: text}}]}`; unknown/missing name → JSON-RPC `-32602`.
  - `system://persona`: explicit `slug` resolves nearest-scope-wins repo → workspace →
    tenant (`resolve_persona_in_chain`, mirroring the existing REST `resolve_persona`
    parent-id derivation — verified against personas.rs:342-405); without a slug, the
    agent record's bound `persona_id`. Approved-only, fail-closed (Pending persona →
    error, never injected). Tests cover tenant-fallback, repo-override, bound-persona,
    unbound-agent error, unapproved + unknown rejection.
  - `system://context`: task title/status/spec ref/description (acceptance criteria),
    worktree branch+path, repo info; optional `task_id` argument. **Containment is real**
    (`context_prompt_for_caller`): workspace mismatch always denies; repo mismatch denies
    for repo-scoped callers — a hard test seeds a victim workspace/tenant task and
    asserts denial with no title leak, for both explicit and JWT-bound task ids.
  - `system://constraints`: reads `budget_configs`/`budget_usages` keyed
    `workspace:{id}` / `tenant:global` — verified byte-identical keying with the
    spawn-time enforcement (`api/budget.rs` `check_spawn_budget`), so the prompt cannot
    disagree with what is enforced; renders limits, used/remaining, lifetime cap, and
    tenant/workspace/repo scope boundaries. Tenant identity always comes from the
    governing workspace record (`agent_secret_scope`, mcp.rs:2829-2888) — never
    fabricated.
  - Scoped prompts require an agent identity; user tokens get an honest error.
- **Spawn injection** (`spawn_agent_core`): optional `persona_slug` resolves through the
  repo → workspace → tenant chain and fails closed (400) on unknown or unapproved slugs
  *before* the agent row is created (test asserts no orphaned agent rows); the resolved
  persona is persisted as `agent.persona_id`. All four prompts are injected into the
  container env as `GYRE_PROMPT_PERSONA` (only when bound) / `GYRE_PROMPT_PROTOCOL` /
  `GYRE_PROMPT_CONTEXT` / `GYRE_PROMPT_CONSTRAINTS`. The injection tests are NOT
  mock-backed: a stub `kubectl` captures the Pod manifest that the **real** kubernetes
  dispatch applies via stdin, and the test parses `spec.containers[0].env` — removing
  the env insertion fails them. Orchestrator launches (`launch_orchestrator_process`)
  inject the same set from the bound persona; test dumps a real child process's env.
- **meta_spec_set_sha**: the task plan's "currently hardcoded to None" was stale — at the
  assigned base the field already carried task-116's approved semantics (SHA-256 of the
  assembled meta-spec prompt set, null when empty; Persona has no meta-spec set field to
  read from). The candidate keeps those semantics, documents the reconciliation in
  coverage row 23, and adds the no-fabrication complement test (empty set → null).
- **Invariants**: all 17 relevant mechanical checks pass at the candidate (arch, ABAC
  route registry — no new HTTP routes, mcp-write-tools — prompts/get is read-only,
  migration versions, dead message kinds, byte-slice, fail-open refs, fabricated scope
  defaults, lossy secrets, scope literal defaults, inert enforcement, forged scope
  fields, unbounded HTTP, forwarded headers, in-memory stores, relative paths,
  conditional test guards). `cargo fmt -p gyre-server -- --check` clean.
  `check-task-commit-attribution.sh` passes at the candidate commit (verified in a
  detached worktree; it fails only in the live tree because the executor's staging
  reverted task-150.md's frontmatter to the pre-work state — not a candidate defect).
- **Compilation**: `SKIP_WEB_BUILD=1 cargo check -p gyre-server --tests` exit 0 at
  212c4c62 (7m36s cold, log in evidence dir). This also confirms the prior round's
  E0063 `bearer_token` merge drift is fixed (894fe8b73).

## Non-blocking observations (no contract violation)

1. `system://context` renders `spec_path` but not the pinned `spec_ref` (`path@sha`)
   when a task carries one; `spec_path` satisfies the "spec refs" criterion, but the
   pinned SHA would be strictly better initial context. Follow-up nit, not a gap.
2. `prompts/get` maps internal lookup failures to `-32602` (INVALID_PARAMS) alongside
   genuine parameter errors — cosmetic protocol-accuracy nit.
3. Worktree selection for the MCP `system://context` uses the first worktree found for
   the agent; multiple worktrees pick an arbitrary one.
4. Residual (documented in coverage row 23): worker spawns without `persona_slug` get
   no `GYRE_PROMPT_PERSONA` — the spec defines no default worker persona, and the
   implementation reports rather than fabricates. Orchestrators always get one (bound
   at orchestrator spawn via the existing persona resolution).
5. `PromptScope.task_id = Some(agent.id)` on the orchestrator path follows the
   established platform convention (orchestrator JWTs carry their own id as `task_id`,
   auth.rs `mint_orchestrator`); it is not consumed by `constraints_prompt`.

## Verifier must run (gyre-server test compilation exceeds the reviewer budget)

```
SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib mcp_prompts        # 10 MCP prompt tests
SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib api::prompts       # 2 builder tests
SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib spawn_injects      # spawn env-dump tests
SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib spawn_without_persona_injects
SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib spawn_persona_slug_fails_closed
SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib spawn_omits_meta_spec_set_sha
SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib orchestrator_launch_injects_system_prompts
```
(or equivalently `--lib prompts` / `--lib spawn` / `--lib orchestrator` filters; the
implementer's prior full round recorded 157 passed / 0 failed across `mcp::tests::`,
`api::spawn::tests::`, `api::prompts::` before the bearer_token merge drift, which only
touched three test struct initializers and is now compile-verified fixed).
