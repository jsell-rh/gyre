# Review: task-150 — Implement MCP Prompts (prompts/list, prompts/get) and persona injection at spawn

Comparison base: 4a203a066a75707001607bbe17dd8e6e616a9fa9 → candidate 03aa47cd75de97c205ae2ac5ee650dc0f21ffdf7.
Effective delta vs base: 11 files (+2165/−7) — mcp.rs, api/prompts.rs (new), api/spawn.rs,
api/orchestrator.rs, ralph_loop.rs, signal_chain.rs, api/mod.rs, docs (agent-protocol, api-reference),
coverage row 23, task file. The branch merges several upstream commits, but base 4a203a06 already
contains those trees, so the reviewable surface is exactly the 11 files. No `scripts/` changes, no
exemption-file growth, no deleted tests.

## R1 Findings

No blocking findings. Evidence reviewed:

### Spec conformance (platform-model.md §4.3 MCP Prompts)

- **prompts/list** (mcp.rs `prompt_definitions()`): returns exactly the four spec prompts
  (`system://persona`, `system://protocol`, `system://context`, `system://constraints`) in
  spec-table order, each with a description; persona takes an optional `slug` argument,
  context an optional `task_id`. `initialize` now advertises the `prompts` capability.
  Asserted by `mcp_prompts_list_returns_four_system_prompts` and
  `mcp_initialize_advertises_prompts_capability` (through the real `/mcp` router).
- **prompts/get** (mcp.rs `handle_prompt_get`, builders in `api/prompts.rs`): MCP
  result shape `{description, messages:[{role, content:{type:"text", text}}]}`.
  Unknown name and missing `name` → JSON-RPC −32602 (tested).
- **system://persona** resolves via the 3-level fallback: `resolve_persona_in_chain`
  walks Repo → Workspace → Tenant (nearest wins), Approved-only, fail-closed.
  Without a `slug` argument the agent-record-bound `persona_id` is used; an unbound
  agent with no slug errors honestly ("no persona bound") rather than fabricating
  generic text. Tested: tenant-level fallback through a repo-scoped agent, repo-level
  override, bound-persona, unbound error, Pending-persona rejection, unknown-slug
  rejection.
- **system://context** (`context_prompt_for_caller`): task id/title/status/type/spec
  ref/labels/depends_on + description-as-acceptance-criteria, worktree branch+path,
  repo name/path/default-branch. Tested with real seeded task/worktree/repo.
- **system://constraints** (`constraints_prompt`): reads the same `budget_configs` /
  `budget_usages` stores (keys `workspace:{id}`, `tenant:global`) that spawn-time
  budget enforcement reads, so the prompt cannot disagree with enforcement; renders
  limits + used/remaining, lifetime cap, and tenant/workspace/repo scope boundaries.
  Tested with seeded config+usage (exact remaining arithmetic asserted).
- **system://protocol**: static built-in template covering interaction norms, the
  Ralph loop (observe/plan/act/monitor/reconcile), and escalation rules.

### Spawn injection (api/spawn.rs)

- `SpawnAgentRequest.persona_slug` resolves through `resolve_persona_in_chain`
  (fail-closed → 400 on unknown or non-Approved slug) **before** the agent row is
  created, then persists `agent.persona_id` — so `system://persona` later resolves
  the same binding without a slug. `spawn_persona_slug_fails_closed` also asserts no
  orphaned agent rows from rejected spawns.
- `launch_agent_process` builds all four prompts and injects them into
  `container_env` → `SpawnConfig.env`, the shared path applied by every compute
  backend (container/ssh/k8s/local). `spawn_injects_all_four_system_prompts_with_persona`
  observes the **real** Kubernetes dispatch (`kubectl apply -f -` through a stub
  kubectl binary, process boundary crossed) and asserts verbatim persona content,
  protocol/context/constraints headers, task title, spec ref, branch, and workspace
  boundary in the applied Pod manifest. `spawn_without_persona_injects_prompts_but_no_persona`
  asserts the other three are injected and GYRE_PROMPT_PERSONA is **absent** (no
  fabricated persona).
- Re-spawn parity: `ralph_loop.rs::relaunch_agent_session` re-resolves the bound
  persona (fail-closed on unapproved/missing) so first spawn and re-spawn never
  drift. Orchestrator launches (`launch_orchestrator_process`) inject the same four
  prompts from the bound persona, with an honest no-task/no-worktree context prompt;
  a bound persona that has been reset to Pending (e.g. by a persona file push) fails
  the launch closed rather than injecting unapproved content
  (`orchestrator_launch_fails_closed_on_unapproved_persona`, including a
  no-process-ever-spawned assertion). MCP `gyre_spawn_worker` gained the optional
  `persona_slug` argument, wired to the same core.

### Security / scope handling

- `prompt_scope` derives the caller's scope exactly like the `secrets://` resources
  (`agent_secret_scope`: JWT claims → agent record → task; tenant always from the
  governing workspace record, never fabricated). Scoped prompts require an agent
  identity — a user token gets an error, not a prompt (tested).
- `system://context` enforces containment on any loaded task: workspace mismatch
  always denies; repo mismatch denies when both caller and task carry a repo —
  mirroring `handle_spawn_worker`'s PERMISSION_DENIED check. Tested for
  cross-workspace (explicit task_id **and** forged JWT task_id claim), cross-repo,
  workspace-scoped-agent allowed same-workspace, and in-scope allowed cases, with a
  no-title-leak assertion on the error path.
- `persona_slug` is not a forged-scope field: it is resolved server-side against the
  caller's own scope chain, and only Approved personas resolve everywhere (spawn,
  re-spawn, orchestrator relaunch, prompts/get).

### Task-plan deviation, assessed (not a defect)

The plan item "set `meta_spec_set_sha` from the resolved persona's meta-spec set" is
unimplementable as written: the domain `Persona` struct (gyre-domain workspace.rs)
has no meta-spec set field, and the premise "currently hardcoded to None" is stale —
base 4a203a06 already computes `meta_spec_set_sha` as the SHA-256 of the assembled
meta-spec prompt set (agent-runtime.md §2, prompt_assembly, task-116), null only
when the assembled set is empty. The candidate keeps that (correct) semantics,
documents it in coverage row 23 and docs/api-reference.md, and adds
`spawn_omits_meta_spec_set_sha_without_required_meta_specs` proving no fabrication
when the set is empty. The canonical spec (platform-model.md §4.3) says nothing
about `meta_spec_set_sha`, so no spec is contradicted.

### Test quality

All new tests assert observable behavior through real entry points (the `/mcp`
router, `spawn_agent_core` via the HTTP spawn route, real child-process dispatch via
stub kubectl / env-dump scripts). No mirrored logic, no assertionless tests. Each
acceptance criterion has at least one test that fails if the behavior breaks
(scope-chain order, containment, fail-closed approvals, env injection, error paths).

### Verification run (this review)

- `cargo check -p gyre-server` → pass (cold, 6m58s).
- `cargo check -p gyre-server --tests` → pass (new test code type-checks).
- `cargo clippy -p gyre-server --tests` → exit 0; zero warnings on any line this
  branch adds (blame-verified against the warned lines in touched files).
- Mechanical invariants: check-mcp-write-tools, check-abac-route-registry,
  check-inert-enforcement, check-scope-literal-defaults,
  check-fabricated-scope-defaults, check-in-memory-state-stores,
  check-lossy-secret-conversion, check-relative-path-defaults,
  check-fail-open-ref-resolution, check-forwarded-header-trust → all pass.
- Commit attribution: at the candidate commit, the frontmatter lists exactly the 8
  task-labeled product-surface commits the attribution script identifies.
- **Deferred to verifier (sandbox restriction, not a code defect):** the wrapper
  rejects gyre-server test-binary codegen (4 GiB budget), so test *execution* could
  not run here. Required probes (see /tmp/stage/review-evidence/summary.json):
  `cargo test -p gyre-server --lib mcp_prompts`, the four new spawn tests
  (`spawn_injects_all_four_system_prompts_with_persona`,
  `spawn_without_persona_injects_prompts_but_no_persona`,
  `spawn_persona_slug_fails_closed`,
  `spawn_omits_meta_spec_set_sha_without_required_meta_specs`),
  the two orchestrator tests (`orchestrator_launch_injects_system_prompts`,
  `orchestrator_launch_fails_closed_on_unapproved_persona`), and the two
  api::prompts unit tests. Supporting evidence: the controller-recorded verification
  of 3a12244d (checks.sh) failed on exactly one diff-scoped clippy lint
  (useless_format at mcp.rs:6476); the candidate's only crates/ delta since is the
  one-line fix for it (ac91266fc).

### Notes (non-blocking)

- N1: `context_prompt_for_caller` uses `worktrees.first()` when an agent has
  multiple worktrees; workers are single-worktree by construction (spawn provisions
  exactly one), so this is cosmetic.
- N2: In `launch_orchestrator_process` the local `PromptScope.task_id` is set to the
  agent id (orchestrators are not task-bound). The value is never consumed —
  `context_prompt_for_orchestrator` and `constraints_prompt` ignore `task_id` — so
  no functional impact, but it would read better as `None`.
- N3: Worker spawns without `persona_slug` get no `GYRE_PROMPT_PERSONA`. This is the
  honest behavior (no worker persona exists to inject; spec defines personas for the
  orchestrator tiers) and is documented as a residual in coverage row 23.
- N4 (generated output, non-blocking per review policy): `web/dist/` in this
  checkpoint may match the upstream bundle; owned by the deterministic verifier.

## Verdict

R1: no blocking findings — task-150 satisfies platform-model.md §4.3. Approve,
conditional on the verifier executing the deferred gyre-server test probes listed
above (type-check and clippy already pass at the candidate).
