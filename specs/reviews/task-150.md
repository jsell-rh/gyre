# Review — task-150 (MCP Prompts prompts/list, prompts/get and persona injection at spawn)

Spec: `specs/system/platform-model.md` §4.3 "MCP Prompts (Injected at Agent Startup)".
Candidate: `e8a00d3cdae547fd8f5b46e7c798cd68d9114b9d` (base `cba416b46a73d3568f745d7b4eae49a0700a90a6`).
Product commits: `30fba783f`, `727befd94`, `12c5fe5e5`, `010dad798`, `894fe8b73`, `b3b97d79e` (all recorded in task-150.md `commits:` frontmatter).
Verdict: **needs-revision** — one finding (F1: persona-approval gate missing on the orchestrator restart injection path); the core §4.3 contract is otherwise genuinely implemented.

## Probes run (this sandbox)

- `cargo check -p gyre-server` (via `/tmp/stage/bin/cargo`) → exit 0, clean (evidence: `/tmp/stage/review-evidence/cargo_check_gyre-server.log`).
- `scripts/check-arch.sh`, `scripts/check-mcp-write-tools.sh`, `scripts/check-inert-enforcement.sh`, `scripts/check-scope-literal-defaults.sh` → all exit 0.
- `cargo test -p gyre-server --lib mcp_prompts` → **blocked by the executor wrapper** (gyre-server test codegen exceeds the 4 GiB model budget — infrastructure restriction, not a code defect). The full focused test list the verifier must run is recorded in `/tmp/stage/review-evidence/probe-summary.md`.

## Verified working (diff inspection + type check)

- **`prompts/list`** returns exactly the four §4.3 prompts in spec-table order with descriptions and argument schemas (`slug` for persona, `task_id` for context); `initialize` now advertises the `prompts` capability. Response shape mirrors `tools/list`/`resources/list`.
- **`prompts/get`** renders `{description, messages:[{role, content:{type:"text", text}}]}`; unknown or missing `name` → JSON-RPC `-32602`. All four prompts serve real, populated content:
  - `system://persona` — explicit `slug` resolved nearest-scope-wins repo→workspace→tenant (`api/prompts.rs::resolve_persona_in_chain`), else the persona bound to the agent record; **Approved-only, fail-closed**; an unbound agent errors honestly rather than getting fabricated text.
  - `system://protocol` — static built-in template covering interaction norms, the Ralph loop, and escalation rules.
  - `system://context` — task title/spec ref/description/status/labels/depends_on, worktree branch/path, repo info; the caller's scope is resolved via `agent_secret_scope` (JWT claims → agent record → task; tenant always from the governing workspace record, never fabricated).
  - `system://constraints` — budget limits + used/remaining + lifetime cap read from `budget_configs`/`budget_usages` (the same stores spawn-time enforcement reads), plus tenant/workspace/repo scope boundaries.
- **Spawn injection** (`api/spawn.rs`): `persona_slug` resolved fail-closed *before* the agent row is created (rejections leave no orphan rows — tested), bound as `agent.persona_id`, and all four prompts injected into the shared `container_env` every compute-target dispatch consumes (`GYRE_PROMPT_PERSONA` only when bound — no fabrication). The worker re-spawn path (`ralph_loop.rs::relaunch_agent_session`) re-resolves the bound persona and fails closed on non-Approved, so first spawn and re-spawn don't drift.
- **Containment**: `system://context` denies cross-workspace tasks always and cross-repo tasks for repo-scoped agents (mirroring `handle_spawn_worker`'s check), with error text that does not leak task content; the bound-task (JWT claim) path is containment-checked too. Tests cover the cross-workspace, cross-repo, and workspace-scoped-agent cases including a no-leak assertion.
- **Tests are real, not inflated**: MCP tests drive the real JSON-RPC dispatcher (`mcp_post`) or `handle_prompt_get` against real stores; spawn tests capture the Pod manifest a real kubernetes dispatch applies via a stub `kubectl` (process-boundary double over real dispatch code) and assert verbatim prompt content; negative paths (unknown prompt, missing name, unapproved/unknown persona, cross-scope task, no-persona spawn) all asserted.
- **`meta_spec_set_sha`**: the task plan's premise ("currently hardcoded to None", "from the resolved persona's meta-spec set") was stale/wrong — at base, task-116 already computes it as the SHA-256 of the assembled meta-spec prompt set (required tenant → workspace → spec bindings, `prompt_assembly::prompt_set_sha`), and the `Persona` domain struct has no meta-spec set field. The candidate keeps that (correct, agent-runtime §2 / authorization-provenance semantics), documents the correction in coverage row 23 (commit `98f7daf73`), and adds a no-fabrication test (null when the assembled set is empty). Reasonable resolution; no code defect.
- Docs (`agent-protocol.md`, `api-reference.md`) updated accurately; coverage row 23 → `implemented` with an honest residual note (worker spawns without `persona_slug` get no persona prompt — reported, not fabricated).

## Findings

- [+] **F1 (major, missing enforcement): `launch_orchestrator_process` injects the bound persona's `system_prompt` with no approval check — the only persona-injection site in this change that fails open.** `crates/gyre-server/src/api/orchestrator.rs:236-249`: `state.personas.find_by_id(persona_id)` → `Ok(Some(persona))` → `env.insert("GYRE_PROMPT_PERSONA", persona.system_prompt)`, with only a warn-and-skip when the record is missing. All four sibling sites added by this same change enforce Approved-only fail-closed: `api/prompts.rs:79` and `:130` (MCP `system://persona`), `api/spawn.rs::spawn_agent_core` (via `resolve_persona_in_chain`), `ralph_loop.rs:656-682` (worker re-spawn: *"a human must approve it before the agent can be re-spawned"*), and `api/orchestrator.rs:100` (`resolve_orchestrator_persona`, first orchestrator spawn). Reachable sequence: (1) orchestrator spawned with an Approved persona (bound `persona_id`); (2) a persona file is pushed to the repo — `git_http.rs:2049-2071` resets the persona to `Pending` and swaps its `system_prompt` to the new, human-unapproved content; (3) the orchestrator dies; (4) `stale_agents.rs:225/275` auto-restarts the replacement with the preserved `persona_id` and calls `launch_orchestrator_process`, which injects the **unapproved** prompt content into the fresh orchestrator. The worker-restart path refuses exactly this scenario; the orchestrator-restart path silently injects it. No test covers persona approval on the orchestrator (re)launch path (the new `orchestrator_launch_injects_system_prompts` test only exercises the happy path).

  **Repair path:** in `launch_orchestrator_process`, check `persona.approval_status == Approved` before injecting `GYRE_PROMPT_PERSONA` (on failure either fail the launch or skip injection with an error-level log + telemetry escalation to the workspace orchestrator/human — the posture choice is the implementer's, but it must not inject unapproved content; aligning with `relaunch_agent_session`'s fail-closed check is the simplest consistent fix), and add a test driving `launch_orchestrator_process` (or the stale-detector restart) with a bound persona whose approval has been reset to Pending, asserting the unapproved `system_prompt` is not injected.

Minor (non-blocking, recorded for completeness):

- `orchestrator.rs:232` sets `PromptScope.task_id = Some(agent.id.clone())` for orchestrators. It is inert (`constraints_prompt` ignores `task_id`) and mirrors the pre-existing `GYRE_TASK_ID = agent.id` orchestrator convention, but an agent id is not a task id; `None` would be the honest value if this scope ever feeds `context_prompt_for_caller`.

## Verifier must run (blocked in this sandbox by the build-memory wrapper)

See `/tmp/stage/review-evidence/probe-summary.md`: `cargo test -p gyre-server --lib mcp_prompts` plus the six spawn/orchestrator prompt tests listed there (spawn tests use real child processes — run with `--test-threads=1`).

Test-inflation check: none found — every new test asserts specific content or specific failure against real stores and real dispatch code, and the negative tests would fail if the enforcement were removed.
