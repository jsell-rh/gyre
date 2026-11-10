# Review — task-150 (MCP Prompts: prompts/list, prompts/get, persona injection at spawn)

Spec: `specs/system/platform-model.md` §4.3 "MCP Prompts (Injected at Agent Startup)" — the four
`system://persona|protocol|context|constraints` prompts, served over MCP and injected into the
agent's initial context at spawn.
Candidate: `f26226bce911246a6e7d3d1d4dc182e970309775` (base `b5a872087899e8bc4a18092ab56990a26e9ca9af`).
Product commits: `30fba783f`, `727befd94`, `12c5fe5e5`, `010dad798` — all present on the branch and
recorded in the task's `commits:` frontmatter (`check-task-commit-attribution.sh` OK).
Verdict: **complete**.

## Round 1 (this review)

Probes (logs and raw sources under `/tmp/stage/review-evidence/`):

- `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib -- mcp::tests:: api::spawn::tests::
  api::prompts:: api::orchestrator::tests::orchestrator_launch` — **152 passed, 0 failed**,
  including all 18 task-150-relevant tests: `mcp_prompts_list_returns_four_system_prompts`,
  `mcp_initialize_advertises_prompts_capability`, `mcp_prompts_get_unknown_and_missing_name_errors`,
  `mcp_prompts_get_persona_resolves_scope_chain` (tenant-level fallback + repo-level override),
  `mcp_prompts_get_persona_uses_bound_persona_and_errors_without`,
  `mcp_prompts_get_persona_rejects_unapproved_and_unknown`,
  `mcp_prompts_get_context_includes_task_spec_and_worktree`,
  `mcp_prompts_get_context_rejects_cross_scope_task_id`,
  `mcp_prompts_get_constraints_include_budget_and_scope`,
  `mcp_prompts_get_protocol_returns_norms`,
  `mcp_prompts_get_scoped_prompts_require_agent_identity`,
  `spawn_injects_all_four_system_prompts_with_persona` (real child-process env dump: all four
  `GYRE_PROMPT_*` vars with task title, spec ref, branch, workspace scope),
  `spawn_without_persona_injects_prompts_but_no_persona`, `spawn_persona_slug_fails_closed`
  (unknown slug 400, Pending persona 400, no orphaned agent rows),
  `spawn_omits_meta_spec_set_sha_without_required_meta_specs`,
  `spawn_delivers_meta_spec_prompt_to_agent_env` (task-116 guard),
  `orchestrator_launch_injects_system_prompts`, plus the two `api::prompts` unit tests.
- `cargo fmt --check -p gyre-server` — clean.
- Invariant scripts all exit 0 on the candidate tree: `check-arch.sh`, `check-mcp-write-tools.sh`,
  `check-abac-exempt-handlers.sh`, `check-forged-scope-fields.sh`, `check-inert-enforcement.sh`,
  `check-scope-literal-defaults.sh`, `check-task-commit-attribution.sh`.
- **Mutation probe (killed, then restored byte-identical — `git status` clean after restore):**
  with both containment checks in `context_prompt_for_caller` disabled
  (`task150-mutation-full.log`), `mcp_prompts_get_context_rejects_cross_scope_task_id` FAILS by
  rendering the exact cross-tenant disclosure ("SECRET victim title" + confidential acceptance
  criteria) — proving the test kills the real vulnerability and the containment check is what
  prevents it. With only the workspace check disabled (`task150-mutation-containment.log`) the
  test still fails via the repo check (defense in depth).

Verified against the acceptance criteria:

- **prompts/list** — dispatcher arm + `prompt_definitions()`: the four §4.3 names in spec-table
  order, each with description; `slug` (persona) and `task_id` (context) argument schemas;
  `initialize` advertises the `prompts` capability. Tested through the real router (`mcp_post`).
- **prompts/get** — `handle_prompt_get` renders `{description, messages:[{role, content:{type,
  text}}]}`; unknown/missing name → JSON-RPC `-32602`. All four prompts return populated content;
  scoped prompts (persona/context/constraints) require a resolvable agent identity (fail, not
  fabricate).
- **persona 3-level fallback** — `resolve_persona_in_chain` builds the chain from the caller's own
  position (repo → workspace → tenant, tenant from the governing workspace record, never
  fabricated); Approved-only, fail-closed; without `slug`, the agent-record-bound `persona_id`.
  Test proves tenant-level resolution from a repo-scoped caller and repo-level override.
- **context** — task title/spec ref/description/status/labels/depends_on, worktree branch+path,
  repo name/path/default branch. Containment mirrors (slightly stricter than) `handle_spawn_worker`:
  cross-workspace always denied; cross-repo denied for repo-scoped callers; the repair commit
  `12c5fe5e5` closed a genuine cross-workspace/tenant disclosure via caller-supplied `task_id`, and
  the mutation probe above confirms the regression test detects it.
- **constraints** — read from `budget_configs`/`budget_usages` (the same stores spawn enforcement
  reads), with used/remaining, lifetime cap, and tenant/workspace/repo scope boundaries.
- **spawn injection** — `spawn_agent_core` accepts optional `persona_slug` (fail-closed chain
  resolution → persists `agent.persona_id`, no orphan rows on rejection) and injects
  `GYRE_PROMPT_PERSONA` (only when bound) / `GYRE_PROMPT_PROTOCOL` / `GYRE_PROMPT_CONTEXT` /
  `GYRE_PROMPT_CONSTRAINTS` into the container env; `gyre_spawn_worker` passes `persona_slug`
  through; orchestrator launches inject the same set from the bound persona. Proven by real
  child-process env-dump tests, not mock assertions.
- **meta_spec_set_sha** — verified at the assigned base `b5a87208`: the field is already computed
  from the assembled meta-spec prompt set (`prompt_set.set_sha`, null only when the assembled set
  is empty; end-to-end test asserts response SHA == injected `GYRE_META_SPEC_SET_SHA`). The task
  plan's "currently hardcoded to None" was stale, and its literal "the resolved persona's meta-spec
  set" names a field that does not exist on the domain `Persona` struct (checked
  `gyre-domain/src/workspace.rs`). The candidate's shipped reconciliation — keep the base's
  task-116 semantics where the response attests exactly what was injected — satisfies the criterion's
  substance (not `None`, derived from real meta-spec state) and is correctly documented in
  `docs/api-reference.md`. AC satisfied; no fakery.
- **Residual (honest, not a gap)**: worker spawns without `persona_slug` get no
  `GYRE_PROMPT_PERSONA` — the spec defines personas for the orchestrator tiers; with no persona
  bound there is nothing to inject, and the code reports rather than fabricates.

Minor (non-blocking, recorded for completeness):

- Coverage row 23 (`specs/coverage/system/platform-model.md`) describes `meta_spec_set_sha` as
  "computed via `compute_meta_spec_set_sha` for the governing workspace (null only when no set is
  configured)". That is the candidate's pre-merge mechanism; after the `727befd94` reconciliation
  the spawn path uses `prompt_set.set_sha` of the assembled prompt set (null when the assembled set
  is empty). The row's substantive claims (implemented, four prompts, tests, residual) are accurate,
  and the correct semantics are documented in the shipped notes and `docs/api-reference.md`; only
  this mechanism pointer inside the coverage note is stale.
- `prompts/get` maps every handler error (including transient store failures) to JSON-RPC `-32602`;
  a dedicated internal-error code would be cleaner. Cosmetic; error text is preserved.

Conclusion: the section is genuinely implemented with real resolution, real containment, real
injection through real child processes, and tests that fail when the behavior breaks. Approve.
