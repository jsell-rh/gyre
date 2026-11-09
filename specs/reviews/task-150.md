# Review — task-150 (MCP Prompts + persona injection at spawn)

Spec: `specs/system/platform-model.md` §4.3 "MCP Prompts (Injected at Agent Startup)".
Candidate: `38e1eb00168199cc1359816010e157d37f24ae6c` (branch `pipeline/task-150/6506109dfa994f44ae80e963b4590733-1`), task commits `30fba783f` + `727befd94`, assignment base `402142b1`.
Verdict: **needs-revision** (1 finding).

## What was verified as real (independent evidence)

Evidence under `/tmp/stage/review-evidence/`:

- `focused-test.log` — `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib -- api::spawn::tests:: mcp::tests:: prompt_assembly::` → **155 passed, 0 failed** (cold build, /tmp/stage/bin/cargo), matching the shipped claim. Includes `spawn_injects_all_four_system_prompts_with_persona`, `spawn_without_persona_injects_prompts_but_no_persona`, `spawn_persona_slug_fails_closed`, `mcp_prompts_list_returns_four_system_prompts`, `spawn_delivers_meta_spec_prompt_to_agent_env`, `spawn_omits_meta_spec_set_sha_without_required_meta_specs`.
- `invariant-scripts.log` — all 17 mechanical invariant scripts exit 0 (arch, mcp-write-tools, abac-route-registry, migration-versions, byte-slice, relative-path-defaults, inert-enforcement, dead-message-kinds, mem-port-contracts, fail-open-ref-resolution, plus forged-scope-fields, forwarded-header-trust, unbounded-external-http, scope-literal-defaults, lossy-secret-conversion, fabricated-scope-defaults, in-memory-state-stores).

Verified working (code inspection + the passing tests):

- `prompts/list` returns exactly the four §4.3 names in spec-table order with descriptions and argument schemas; `initialize` advertises the `prompts` capability; `prompts/get` follows the MCP result shape; unknown/missing name → `-32602`.
- Prompt builders live in `api/prompts.rs` as a single source shared by MCP and spawn — content parity claim holds (both call the same functions).
- Persona resolution: `resolve_persona_in_chain` is exact-match per scope level (mem + SQLite adapters filter slug+scope equality), ordered repo → workspace → tenant, Approved-only, fail-closed on Pending/unknown. Contained to the caller's own scope chain — no cross-scope persona read.
- Spawn: optional `persona_slug` resolved against the target repo's workspace/tenant chain, 400 on unknown/unapproved, binding persisted as `agent.persona_id`, no orphaned agent rows on rejection (tested). All four `GYRE_PROMPT_*` vars injected through a real child process (env-dump compute target), persona var omitted honestly when unbound. Orchestrator launches inject the same set.
- `constraints_prompt` reads the same `budget_configs`/`budget_usages` stores (`workspace:{id}` / `tenant:global`) that spawn-time enforcement reads; tenant identity comes from the workspace record, never a fabricated "default".
- `meta_spec_set_sha`: the response now carries the SHA of the assembled meta-spec prompt set actually delivered as `GYRE_META_SPEC_SET_SHA`, null when the set is empty. The task text said "from the resolved persona's meta-spec set", but `Persona` has no meta-spec-set field; the reconciliation to the merged base's (task-116) assembled-set semantics is documented in the shipped notes and `docs/api-reference.md`, and the response attests what was actually injected — this is a reasonable spec reconciliation, not a defect. Note only.

## Findings

- [ ] **F1 (major, scope handling): cross-workspace/cross-tenant task disclosure via `prompts/get system://context` `task_id` argument.** `context_prompt_for_caller` (`crates/gyre-server/src/api/prompts.rs`, new in this task) loads `state.tasks.find_by_id(tid)` from the caller-supplied `task_id` with **no comparison of the task's workspace/repo against the caller's resolved `PromptScope`**, then renders the task's title, status, priority, spec ref, labels, depends_on, and full description/acceptance criteria. Proof (probe `review_probe_context_task_id_cross_workspace_disclosure`, source saved as `disclosure-probe-source.rs`, output `disclosure-probe.log`, run against the candidate then reverted — working tree clean): an agent of `ws-mcp-prompt`/tenant `default` requesting `{"task_id": "task-victim"}` where the task lives in `ws-victim`/tenant `tenant-b` gets `Ok` with the victim task's "SECRET victim title" and "confidential acceptance criteria" in the prompt text. This violates the codebase's own containment pattern — every other MCP consumer of a caller-supplied task id checks scope (e.g. `handle_spawn_worker`, mcp.rs:940-953, `PERMISSION_DENIED` on repo/workspace mismatch) and the AGENTS.md invariant that a handler loading a scoped entity must compare tenant/workspace against the caller. The shipped tests only cover the unknown-id error path, so the gap is invisible to them. Remedy: after the lookup, reject when `task.workspace_id != scope.workspace_id` (and on repo mismatch when both are set), mirroring `handle_spawn_worker`, plus a cross-workspace rejection test. Note the same unscoped task also feeds the repo fallback (`scope.repo_id.or(task.repo_id)`), so a repo-less caller additionally gets the foreign task's repo name/path/default branch.

## Verifier notes

- The failing probe test and its log are the primary evidence; re-adding the probe source to `crates/gyre-server/src/mcp.rs` tests and running `cargo test -p gyre-server --lib review_probe_context` reproduces the panic with the leaked text.
- Everything else in the task contract (all 8 acceptance criteria except the containment aspect implicit in "system://context includes current task") is genuinely implemented and tested; after F1 is fixed with a containment check + test, this task should be approvable.
