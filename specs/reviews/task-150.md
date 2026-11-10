# Review: TASK-150 — MCP Prompts (prompts/list, prompts/get) and persona injection at spawn

**Reviewer:** Independent review (Gyre stage)
**Date:** 2026-11-10
**Comparison base:** 402142b1c8d214b03977d689742baae6a1af7910
**Candidate:** 5eb92fb95d01379af9c016d6174dd975cb1293d8
**Verdict:** approved

---

## Scope

`git diff 402142b1..5eb92fb95`: `mcp.rs` (+774), `api/prompts.rs` (new, +478),
`api/spawn.rs` (+449), `api/orchestrator.rs` (+140), `api/mod.rs`, docs
(agent-protocol.md, api-reference.md), coverage row 23, task file. All three
attributed commits (30fba783f, 727befd94, 12c5fe5e5) are the only product
commits in the range (plus the assigned-base merge 9cfa690b3) — attribution is
clean.

## Findings

No material defects. All acceptance criteria are met by production code with
real data sources; the containment repair is mutation-verified.

### Behavior verification

- [x] **prompts/list** — dispatcher arm returns the four §4.3 prompt names in
  spec-table order with descriptions and argument schemas (`slug` for persona,
  `task_id` for context). `initialize` advertises the `prompts` capability.
  Verified through the full HTTP/JSON-RPC path (`mcp_post` → `mcp_handler`)
  by `mcp_prompts_list_returns_four_system_prompts` and
  `mcp_initialize_advertises_prompts_capability`.
- [x] **prompts/get** — renders `{description, messages:[{role, content:{type,
  text}}]}`. Unknown or missing `name` → JSON-RPC `-32602`. Happy path through
  the dispatcher verified by a temporary in-process probe (see evidence:
  `task150-review-dispatcher-probe.log`; probe test removed, tree restored
  clean). All four prompts return populated, concrete content.
- [x] **system://persona 3-level fallback** — `resolve_persona_in_chain`
  (api/prompts.rs) walks Repo → Workspace → Tenant of the *caller's* scope
  (tenant taken from the governing workspace record via `agent_secret_scope`,
  never fabricated). Approved-only, fail-closed. Test covers tenant-level
  fallback for a repo-scoped agent and repo-level override of the same slug.
  Without a `slug`, the agent-record-bound `persona_id` is used; an unbound
  agent errors honestly rather than getting fabricated text.
- [x] **system://context** — task title/spec ref/description/status/labels/
  depends_on + worktree branch/path + repo name/path/default branch, from real
  store lookups. Unknown `task_id` errors.
- [x] **Scope containment (repair commit 12c5fe5e5)** —
  `context_prompt_for_caller` rejects a caller-supplied `task_id` (and the
  bound task) whose workspace differs from the caller's, and rejects a repo
  mismatch when both caller and task carry a repo; a workspace-scoped agent
  may read same-workspace tasks in any repo. Mirrors `handle_spawn_worker`'s
  check and is at least as strict. **Mutation-verified**: with the containment
  disabled in the working tree, `mcp_prompts_get_context_rejects_cross_scope_task_id`
  FAILS with the victim task's "SECRET victim title" actually rendered
  (evidence: `task150-review-mutation-probe-both.log`); single-check mutation
  also caught (`task150-review-mutation-probe.log`). Source restored, clean
  `git status`.
- [x] **system://constraints** — reads `budget_configs`/`budget_usages` keyed
  `workspace:{id}`/`tenant:global` (the same stores spawn enforcement reads),
  renders limits + used/remaining + lifetime cap + tenant/workspace/repo scope
  boundaries. Test asserts concrete values (100000/75000/50.0000/3600).
- [x] **Spawn injection** — `spawn_agent_core` injects `GYRE_PROMPT_PERSONA`
  (only when bound) / `GYRE_PROMPT_PROTOCOL` / `GYRE_PROMPT_CONTEXT` /
  `GYRE_PROMPT_CONSTRAINTS` into the container env, content identical to
  `prompts/get` (shared builders in api/prompts.rs — single source of truth).
  Verified through a **real child process** env-dump compute target
  (`spawn_injects_all_four_system_prompts_with_persona`: persona text
  verbatim, task title, spec ref, branch, workspace scope; without-persona
  case asserts `GYRE_PROMPT_PERSONA` absent). Optional `persona_slug` request
  field resolves fail-closed (400 on unknown or Pending slug, no orphaned
  agent rows) and persists `agent.persona_id`.
- [x] **Orchestrator launches** — `launch_orchestrator_process` injects the
  same four prompts from the bound persona; context honestly records
  no-task/no-worktree. Verified through a real child process
  (`orchestrator_launch_injects_system_prompts`).
- [x] **meta_spec_set_sha** — real derived value from the assembled meta-spec
  prompt set (`prompt_set.set_sha` via `assemble_prompt_set`, agent-runtime
  §2 / task-116 semantics merged from the assigned base), null when the
  assembled set is empty. Not hardcoded `None`. Tests cover both cases
  (`spawn_delivers_meta_spec_prompt_to_agent_env`,
  `spawn_omits_meta_spec_set_sha_without_required_meta_specs`). Note: the
  acceptance criterion's literal "resolved persona's meta-spec set" is
  unimplementable as written — `Persona` has no meta-spec-set field in the
  domain model — and the reconciliation to the assembled-set SHA (the value
  actually delivered as `GYRE_META_SPEC_SET_SHA`) is disclosed in the shipped
  notes and documented in docs/api-reference.md. The criterion's intent (a
  real, non-hardcoded value attesting what was injected) is satisfied.

### Focused test runs (evidence under /tmp/stage/review-evidence/)

- `cargo test -p gyre-server --lib -- api::spawn::tests:: mcp::tests::` —
  **146 passed, 0 failed** (`task150-review-focused-tests.log`), including
  all 9 `mcp_prompts_*` suites and the spawn env-dump tests.
- `cargo test -p gyre-server --lib -- api::orchestrator:: prompt_assembly` —
  **22 passed, 0 failed** (`task150-review-orch-prompt-assembly.log`),
  including `orchestrator_launch_injects_system_prompts`.
- Mutation probes: containment disabled (both checks) → cross-scope test
  FAILS with real disclosure; restored → 10/10 `mcp_prompts` pass.
- Dispatcher happy-path probe (temporary test, removed): prompts/get
  `system://protocol` through the full `mcp_handler` returns Ralph-loop
  content, no error.
- Invariant scripts all exit 0: check-arch, check-mcp-write-tools,
  check-abac-exempt-handlers, check-abac-route-registry,
  check-inert-enforcement, check-scope-literal-defaults,
  check-fabricated-scope-defaults, check-mem-port-contracts,
  check-migration-versions.
- `cargo build -p gyre-server` (SKIP_WEB_BUILD=1) succeeds; `cargo clippy -p
  gyre-server --lib` — warnings only, no errors; the gyre-common warning is
  pre-existing on the base (gyre-common is untouched by this diff).
- Server boot probe: candidate binary starts and listens (server-probe.log).
  A real TCP client round-trip was blocked by the sandbox (outbound connect
  denied; capabilities.json lists listener accept only) — not a code defect;
  the in-process dispatcher probe above covers the same path. Deferred check
  for verification recorded in server-probe.log.

### Minor observations (non-blocking, recorded for the record)

1. Coverage row 23's phrase "computed via `compute_meta_spec_set_sha` for the
   governing workspace" names a related function; the actual spawn-response
   value is `prompt_set.set_sha` from `assemble_prompt_set`. Behaviorally
   equivalent at summary level (real value, null when empty) and correctly
   described in docs/api-reference.md and the task's shipped notes.
2. docs/agent-protocol.md:36 retains pre-existing M32 meta_spec_set_sha
   wording inherited unchanged from the base (the diff only adds the
   GYRE_PROMPT_* rows to that file).
3. Workers spawned without `persona_slug` get no persona prompt — disclosed
   as a residual in the coverage row; fail-honest, not fabricated.

## Verdict

**Approved.** Real implementations behind every acceptance criterion, real
data sources (persona/task/repo/worktree/budget store lookups), enforced
scope containment that is mutation-verified to kill the disclosure scenario,
and env-injection proven through real child processes. No fake completion,
no test inflation, no scope-handling defects found.
