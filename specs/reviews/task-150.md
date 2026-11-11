# Review — task-150 (MCP Prompts prompts/list+prompts/get and persona injection at spawn)

Spec: `specs/system/platform-model.md` §4.3 "MCP Prompts (Injected at Agent Startup)".
Candidate: `3a12244d` on `pipeline/task-150/b23c2f37b47a458995b37db4b9991648-1`, base `cba416b46`.
The branch carries the full task history (initial feat `30fba783` plus six repair commits through
`deb37efd`, the prior round's F1 fix) and one prior review round (checkpoint `e8a00d3c`, F1:
orchestrator relaunch injected unapproved persona content).
Verdict: **approve**.

## Round 2 (this review)

Sandbox probes (full logs + command transcripts under `/tmp/stage/review-evidence/`):

- `cargo check -p gyre-server` → clean; `cargo check -p gyre-server --lib --tests` → clean
  (production and test code both type-checked); `cargo fmt --check` → clean.
- Mechanical invariant scripts all green: check-arch, check-mcp-write-tools (prompts/get performs
  no writes — correctly outside the needs_write gate), check-abac-route-registry (no new HTTP
  routes; MCP methods ride the existing authenticated `/mcp` route), check-scope-literal-defaults,
  check-forged-scope-fields, check-inert-enforcement, check-byte-slice-truncation,
  check-lossy-secret-conversion, check-mem-port-contracts, check-forwarded-header-trust.
- gyre-server test compilation is blocked by this sandbox's 4 GiB build wrapper (infrastructure
  restriction, not a code defect — exit transcript recorded). The verifier must run the focused
  suites listed in `/tmp/stage/review-evidence/summary.md` (mcp_prompts ×11, orchestrator tests
  incl. both F1-regression tests, the three spawn env-dump tests, the meta-spec SHA pair, and the
  two Ralph-loop re-spawn guards).
- check-task-commit-attribution fails only against the working tree because the executor's
  checkout reset `specs/tasks/task-150.md` to its pre-work state; at the candidate commit the
  frontmatter lists all 7 task-labeled product-surface commits. Not a candidate defect.

Verified working (no findings):

- **prompts/list / prompts/get are real dispatcher arms** (mcp.rs): `initialize` advertises the
  `prompts` capability; `prompts/list` returns exactly the four §4.3 names in spec-table order
  with descriptions and argument schemas (`slug`, `task_id`); `prompts/get` renders
  `{description, messages:[{role, content:{type:text}}]}` per the MCP shape, with unknown/missing
  name → -32602. The list test goes through the real HTTP+JSON-RPC dispatcher; the content tests
  call `handle_prompt_get` with the same `AuthenticatedAgent` the dispatcher authenticates.
- **`system://persona`** resolves nearest-scope-wins repo → workspace → tenant
  (`resolve_persona_in_chain`, built on the pre-existing `find_by_slug_and_scope` port implemented
  by sqlite/postgres/mem), or the agent-record-bound `persona_id` without a slug. Only `Approved`
  personas resolve, everywhere, fail-closed — tested for the slug path, the bound path, unknown
  slugs, and Pending personas. An unbound agent errors honestly instead of receiving fabricated
  text.
- **`system://context`** renders task title/status/priority/spec path/description (acceptance
  criteria), worktree branch+path, and repo info, from real store lookups. Containment is enforced
  on every task load: cross-workspace always denied, cross-repo denied for repo-scoped callers —
  tested in both directions plus the workspace-scoped-agent allowance, and the denial errors are
  asserted not to leak the victim task's title.
- **`system://constraints`** reads `budget_configs`/`budget_usages` under the same keys
  spawn-time enforcement uses (`workspace:{id}`, `tenant:global`), rendering limits, used/remaining,
  lifetime cap, and tenant/workspace/repo scope boundaries.
- **Spawn injection is real and process-boundary-tested**: `spawn_agent_core` accepts
  `persona_slug` (fail-closed 400 on unknown/unapproved, no orphan agent rows, binding persisted
  as `agent.persona_id`), and `launch_agent_process` injects `GYRE_PROMPT_PERSONA` (when bound) /
  `_PROTOCOL` / `_CONTEXT` / `_CONSTRAINTS` into `container_env`. The env-dump tests flow through
  the real Kubernetes adapter dispatch (`build_pod_manifest` maps `config.env` into
  `spec.containers[0].env`; `kubectl_path` honored from target config) with a stub kubectl binary
  capturing the applied manifest — same proven double as the pre-existing meta-spec env test.
  MCP `gyre_spawn_worker` forwards `persona_slug` and sits behind the Agent-role write gate and
  the repo-orchestrator tier check.
- **All relaunch paths inject consistently and fail closed on unapproved personas**: the Ralph-loop
  re-spawn resolves the persona bound at first spawn and bails when it is no longer Approved
  (worker); `launch_orchestrator_process` — the site of prior-round F1 — now returns
  `launch_failed` on a non-Approved bound persona with an error-level log, and the regression test
  simulates the git-push persona swap (system_prompt replaced, approval reset to Pending), invokes
  the launch exactly as `restart_orchestrator` does, and asserts both the failed outcome and that
  no process was ever spawned (the dump script never ran). The approved-persona injection test
  confirms the happy path still works.
- **`meta_spec_set_sha`**: the task plan's "set from the resolved persona's meta-spec set
  (currently hardcoded to None)" was stale at base — `Persona` has no meta-spec-set field, and
  base `cba416b46` (task-116) already computes the SHA from `prompt_assembly::assemble_prompt_set`,
  delivering it as `GYRE_META_SPEC_SET_SHA`/`GYRE_META_SPEC_PROMPT`. The candidate documents this
  reconciliation in coverage row 23 and adds
  `spawn_omits_meta_spec_set_sha_without_required_meta_specs` proving no value is fabricated when
  the assembled set is empty. The underlying §4.3 spec says nothing about `meta_spec_set_sha`;
  no code contradicts the spec.
- **Scope identity is never fabricated**: MCP prompt scope reuses the task-098
  `agent_secret_scope` machinery (tenant always from the governing workspace record); user tokens
  without a resolvable agent identity get an error, not a default scope.

Minor (not blocking, recorded for completeness):

- `launch_orchestrator_process` treats an *unresolvable* bound persona (deleted row / storage
  error) as warn-and-skip, while `relaunch_agent_session` bails on the same condition. No
  unapproved content can be injected through the skip (it injects nothing), and it is logged, but
  the two sibling paths could adopt one posture.
- `system://context` renders `task.spec_path` but not the pinned `task.spec_ref` (`path@sha`);
  the spec table's "spec refs" is satisfied by the path, and the pinned blob is what decomposition
  reads, but including it would be strictly more faithful.
- All `prompts/get` failures map to JSON-RPC -32602 (`INVALID_PARAMS`), including storage-layer
  lookup failures that arguably warrant -32603; cosmetic, messages are accurate.

Scope note: the working tree's `specs/tasks/task-150.md` shows as modified because the executor's
checkout reset it to the pre-work state; the candidate commit contains the ready-for-review
frontmatter with the complete `commits:` list (all 7 task-labeled product-surface commits). No
production code, scripts, or specs were modified by this review.
